## Python bindings

Requires `bevy_openusd` checked out as a sibling directory (path dependency
in `Cargo.toml`). No need to install `maturin` yourself — uv fetches it
automatically as a PEP 517 build backend.

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

## Viewer (interactive)

    cargo run --release