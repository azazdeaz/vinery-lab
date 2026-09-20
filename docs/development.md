# Development

## Building the Python extension

No need to install `maturin` yourself — uv fetches it automatically as a
PEP 517 build backend.

The project uses maturin's *mixed* layout: hand-written Python lives in
`python/vinerylab/`, and the compiled Rust extension is built into it as the
`_core` submodule, which `__init__.py` re-exports. `vinerylab.isaaclab` is the
only part that imports Isaac Lab, so plain `import vinerylab` stays usable
without it.

How the binding surface is shaped — why fragments are `Py<T>`, what
`write_usd` releases the GIL around — is in
[architecture.md](architecture.md#python-bindings).

### Iterating on the wrapper itself (rebuilds on every change)

    uv venv .venv && source .venv/bin/activate
    uvx maturin develop --release

### Consuming it as a dependency (e.g. examples/isaaclab_demo)

    cd examples/isaaclab_demo
    uv sync
    uv run python main.py

If you only changed Rust source (not pyproject.toml), force a rebuild:

    uv sync --reinstall-package vinerylab

### Build a distributable wheel

    uvx maturin build --release

## Checks

`.github/workflows/ci.yml` runs all of these on every push and pull request:

    cargo fmt --all -- --check
    cargo clippy --all-targets --features python -- -D warnings
    cargo test
    ruff check . && ruff format --check .
    mypy
    pytest tests

`--features python` is what puts `src/python.rs` in front of clippy —
it is out of the default feature set, so a plain `cargo clippy` never sees it.

`cargo test` also fails while the Python stub, the Isaac Lab cfg classes or
`docs/parameters.md` are stale against the Rust params structs they are
generated from. `cargo test regen_params -- --ignored` rewrites them — see
[editing-parameters.md](editing-parameters.md).

Isaac Lab is not installed on the runner, so `tests/test_isaaclab_cfg.py` skips
itself there. It runs locally, from the demo venv:

    examples/isaaclab_demo/.venv/bin/python -m pytest tests/ -q -s < /dev/null

Capture has to be off: pytest's stdin capture breaks Kit's kernel bootstrap.
The demo's own tests live beside its code and run the same way:

    examples/isaaclab_demo/.venv/bin/python -m pytest examples/isaaclab_demo -q -s < /dev/null

Formatting and the fast lints are also available as commit hooks:

    uvx pre-commit install

Clippy and the test suites stay out of them — a Bevy rebuild is too slow to sit
in front of a commit.

## Viewer (interactive)

    cargo run --release

The panel is built from the params structs: one section per fragment of
`VineyardParams`, one control per field, with the caption, range and tooltip
read off the field's declaration — see `src/ui.rs` and
[editing-parameters.md](editing-parameters.md). Sliders write a
staged copy of the params; a value reaches the live resources once it has held
still for 150 ms, and re-runs the layers below it. Dragging one is a single
rebuild rather than one per frame.
Press `S` to write the scene out as `scene.json`, and build it with:

    python -m vinerylab.usd scene.json scene.usd

`VINERYLAB_PERF=1 cargo run` logs a per-layer breakdown on any frame that
rebuilt something — see `src/perf.rs`.

`VINERYLAB_RECORD=demo.mp4 cargo run --release` records the window for the
whole run. One captured frame becomes one video frame, so the stall a rebuild
causes costs a frame rather than the freeze a screen recorder would keep —
which is the point of it, for demo videos. Frames are piped to `ffmpeg`, which
has to be on `PATH`; the extension picks the container. `VINERYLAB_RECORD_FPS`
sets the rate, 30 by default, and is also how fast the window is sampled. See
`src/record.rs`.

### Web build

The same viewer, compiled to wasm and published by the manually triggered
`.github/workflows/playground.yml`. To reproduce what it does locally:

    rustup target add wasm32-unknown-unknown
    cargo install wasm-bindgen-cli --version 0.2.127   # must match Cargo.lock
    cargo build --profile wasm-release --target wasm32-unknown-unknown --bin vinerylab
    mkdir -p site && cp web/index.html site/
    wasm-bindgen --target web --no-typescript --out-dir site \
      target/wasm32-unknown-unknown/wasm-release/vinerylab.wasm
    python3 -m http.server -d site 8000

Serve it rather than opening the file: the module is fetched, and `localhost`
is a secure context, which both WebGPU and the clipboard require.

Two things differ from the native build. The renderer is WebGPU — the `webgpu`
feature in `Cargo.toml` is target-gated, and WebGL2 would lose compute shaders,
which this many meshes need. And the `S` key is compiled out, since there is no
filesystem to write `scene.json` to; **Copy Isaac Lab cfg** is the whole export
path on the web.
