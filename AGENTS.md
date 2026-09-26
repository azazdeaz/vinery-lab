# Vinery Lab

Parametric vineyard generator for robotics simulation, mainly Isaac Lab. A
Bevy app builds the scene as ordinary meshes and transforms, exports it as a
plain JSON scene document, and Python turns that into a USD stage. The same
crate ships as an interactive viewer and as a Python extension module,
`vinerylab`.

[README.md](README.md) is the user-facing side: features, the Isaac Lab
workflow, spawning a `VineyardCfg`. This file is the map for working on the
repo itself.

## Commands

```bash
cargo run --release                                          # viewer / parameter editor
cargo fmt --all
cargo clippy --all-targets --features python -- -D warnings
cargo test
uv run ruff check . && uv run ruff format --check .
uv run mypy
uv run pytest tests -q
```

Everything below the first line is what `.github/workflows/ci.yml` runs on
every push and pull request. Isaac Lab is not installed there, so the tests
that need it skip themselves; run those from a demo venv — see
[docs/development.md](docs/development.md).

## Where things are

### Rust

| Path | What it is |
| --- | --- |
| [src/lib.rs](src/lib.rs) | crate root; the architecture in one paragraph |
| [src/main.rs](src/main.rs) | the `vinerylab` binary; calls `viewer::run` |
| [src/viewer.rs](src/viewer.rs) | the interactive Bevy app |
| [src/ui.rs](src/ui.rs) | the parameter panel, built by walking the params structs |
| [src/params.rs](src/params.rs) | what a params field declares: caption, range, choices, tooltip |
| [src/codegen.rs](src/codegen.rs) | generates the Python stub, the Isaac Lab cfg classes and `docs/parameters.md` |
| [src/elements/](src/elements/) | one module per vineyard thing; `mod.rs` holds the pipeline contract |
| [src/elements/util/](src/elements/util/) | geometry kernels, palette, layout solver — everything under `elements/` that is not an element |
| [src/scene/](src/scene/) | `doc.rs` is the JSON contract with Python, `export.rs` the walk that fills it |
| [src/quantize.rs](src/quantize.rs) | k-center clustering: a population of configs down to `variations` meshes |
| [src/generate.rs](src/generate.rs) | one headless build cycle, for Python and for tests |
| [src/snippet.rs](src/snippet.rs) | the viewer's **Copy Isaac Lab cfg** output |
| [src/stats.rs](src/stats.rs), [src/perf.rs](src/perf.rs), [src/record.rs](src/record.rs) | scene footer, per-layer timing, window capture |
| [src/python.rs](src/python.rs) | the PyO3 wrapper, behind the `python` feature |

### Python, under `python/vinerylab/`

| Path | What it is |
| --- | --- |
| [usd/build.py](python/vinerylab/usd/build.py) | scene document to stage; where every USD rule is written down |
| [usd/ground.py](python/vinerylab/usd/ground.py) | `Ground`: the terrain height under any (x, y), read back off a stage |
| [isaaclab/vineyard_cfg.py](python/vinerylab/isaaclab/vineyard_cfg.py) | `@configclass` fragments mirroring the Rust params |
| [isaaclab/vineyard.py](python/vinerylab/isaaclab/vineyard.py) | generates, caches and spawns the `.usd` |
| [isaaclab/physics.py](python/vinerylab/isaaclab/physics.py) | the coupled Newton config that lets shoots bend |
| [isaaclab/cutting.py](python/vinerylab/isaaclab/cutting.py) | `Shears`: cuts a shoot anywhere along it while the simulation runs |
| [_core.pyi](python/vinerylab/_core.pyi) | typed signatures for the compiled extension (generated) |

Only `vinerylab.isaaclab` imports Isaac Lab, so plain `import vinerylab` works
without it.

### Everything else

| Path | What it is |
| --- | --- |
| [tests/](tests/) | Python tests: the params, the USD build, the Isaac Lab cfg and physics |
| [examples/isaaclab_demo/](examples/isaaclab_demo/) | a quadruped walking the alleys; its own uv project |
| [examples/straddler_demo/](examples/straddler_demo/) | a straddling robot driving every row, `--trim` to hedge it; its own uv project |
| [assets/leaves/](assets/leaves/) | traced leaf outlines, compiled in with `include_str!` |
| [web/index.html](web/index.html) | host page for the wasm playground |

## Docs

- [docs/development.md](docs/development.md) — building, the checks, running the viewer, the web build
- [docs/architecture.md](docs/architecture.md) — how a scene is built and exported: the element pipeline, quantization, randomness, coordinates
- [docs/editing-parameters.md](docs/editing-parameters.md) — how to add or change a parameter
- [docs/parameters.md](docs/parameters.md) — every parameter, with its default and range (generated)
- [docs/pruning-research.md](docs/pruning-research.md) — prior art for cutting shoots: pruning robots, hedgers, how simulators model a cut
- [examples/isaaclab_demo/DEVELOPMENT.md](examples/isaaclab_demo/DEVELOPMENT.md) — the pinned Isaac Lab revision and how to bump it

## Rules that bite

- **A parameter is declared once, on its Rust params struct.** The panel, the
  snippet, the Python class, the stub and `docs/parameters.md` all derive from
  it. [docs/editing-parameters.md](docs/editing-parameters.md) is the how-to.
- **Generated files are not hand-edited.** `cargo test` fails while
  `python/vinerylab/_core.pyi`, `python/vinerylab/isaaclab/vineyard_cfg.py` or
  `docs/parameters.md` are stale; `cargo test regen_params -- --ignored`
  rewrites them.
- **The `python` feature is off by default**, so a plain `cargo clippy` never
  sees `src/python.rs`. Pass `--features python`, as CI does.
  `extension-module` is a different feature: it unlinks libpython, so it is
  for maturin builds only and breaks `cargo run` and `cargo test`.
- **Rust tests are inline `#[cfg(test)]` modules** beside the code they cover.
  The top-level `tests/` is Python only.
- **The scene is authored Z-up, in meters** (REP-103). Bevy renders Y-up; the
  correction is a single parent entity, above where the export walk starts.
