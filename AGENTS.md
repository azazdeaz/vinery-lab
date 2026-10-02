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
cargo clippy --workspace --all-targets --features vinerylab/python -- -D warnings
cargo test --workspace
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

Two crates in one Cargo workspace: `crates/misina-lab` is the framework —
everything that knows nothing about vines — and `crates/vinerylab` is the
vineyard built on it. The seam is `misina_lab::Generator`: one marker type
per generator, which every framework surface is keyed on. The framework has
a map of its own, [crates/misina-lab/AGENTS.md](crates/misina-lab/AGENTS.md),
and its docs explain how a generator is built; what follows is the vineyard.

| Path | What it is |
| --- | --- |
| [vinerylab/src/lib.rs](crates/vinerylab/src/lib.rs) | crate root: the `Vineyard` generator and the `_core` Python module |
| [vinerylab/src/main.rs](crates/vinerylab/src/main.rs) | the `vinerylab` binary: the viewer plus the layout gizmos and the per-layer timing |
| [vinerylab/src/elements/](crates/vinerylab/src/elements/) | one module per vineyard thing; `mod.rs` holds the pipeline contract, the ground and layout wiring, and the `generator!` call |
| [vinerylab/src/elements/util/](crates/vinerylab/src/elements/util/) | the named colours and responses, layout solver, planting walk — everything under `elements/` that is not an element |
| [vinerylab/src/perf.rs](crates/vinerylab/src/perf.rs) | the per-layer marks and the bench |

### Python

Two packages in one uv workspace, mirroring the crates: `python/misina-lab`
is the framework's Python side, pure Python over `usd-core` and mapped in the
framework's `AGENTS.md`; `crates/vinerylab/python/vinerylab` is the
vineyard's, a maturin package holding the compiled `_core` and the generated
cfg classes.

| Path | What it is |
| --- | --- |
| [vinerylab/isaaclab/vineyard_cfg.py](crates/vinerylab/python/vinerylab/isaaclab/vineyard_cfg.py) | `@configclass` fragments mirroring the Rust params (generated), and `VineyardCfg` over them |
| [vinerylab/isaaclab/physics.py](crates/vinerylab/python/vinerylab/isaaclab/physics.py) | the vineyard's names and numbers over the framework's `rods`: which prim a vine is, the demos' substeps |
| [vinerylab/isaaclab/vineyard.py](crates/vinerylab/python/vinerylab/isaaclab/vineyard.py) | the spawner under the vineyard's names, for the demos |
| [vinerylab/isaaclab/newton_patches.py](crates/vinerylab/python/vinerylab/isaaclab/newton_patches.py) | workarounds for the pinned Newton, applied by the demos; not imported by the package |
| [vinerylab/_core.pyi](crates/vinerylab/python/vinerylab/_core.pyi) | typed signatures for the compiled extension (generated) |

Only the `isaaclab` subpackages import Isaac Lab, so plain `import vinerylab`
and `import misina_lab` work without it.

### Everything else

| Path | What it is |
| --- | --- |
| [tests/](tests/) | Python tests: the params, the USD build, the Isaac Lab cfg and physics |
| [examples/isaaclab_demo/](examples/isaaclab_demo/) | a quadruped walking the alleys; its own uv project |
| [examples/straddler_demo/](examples/straddler_demo/) | a straddling robot driving every row, `--trim` to hedge it; its own uv project |
| [examples/pruning_demo/](examples/pruning_demo/) | a Bumblebee-like arm pruning a dormant row to two-bud spurs; its own uv project |
| [assets/leaves/](crates/vinerylab/assets/leaves/) | traced leaf outlines, compiled in with `include_str!` |
| [web/index.html](web/index.html) | host page for the wasm playground |

## Docs

- [docs/development.md](docs/development.md) — building, the checks, running the viewer, the web build
- [docs/parameters.md](docs/parameters.md) — every parameter, with its default and range (generated)
- [docs/pruning-research.md](docs/pruning-research.md) — prior art for cutting shoots: pruning robots, hedgers, how simulators model a cut
- [examples/isaaclab_demo/DEVELOPMENT.md](examples/isaaclab_demo/DEVELOPMENT.md) — the pinned Isaac Lab revision and how to bump it
- [crates/misina-lab/docs/](crates/misina-lab/docs/) — the framework's: how a scene is built and exported, quantization, randomness, Isaac Lab, and how to add or change a parameter
- [crates/misina-lab/docs/AGENTS.md](crates/misina-lab/docs/AGENTS.md) — how a page is written and kept current

## Rules that bite

- **A parameter is declared once, on its Rust params struct.** The panel, the
  snippet, the Python class, the stub and `docs/parameters.md` all derive from
  it. [editing-parameters.md](crates/misina-lab/docs/editing-parameters.md)
  is the how-to.
- **Generated files are not hand-edited.** `cargo test` fails while
  `_core.pyi`, `isaaclab/vineyard_cfg.py` (both under `crates/vinerylab/python/vinerylab/`) or
  `docs/parameters.md` are stale; `cargo test regen_params -- --ignored`
  rewrites them.
- **The `python` feature is off by default**, so a plain `cargo clippy` never
  sees the PyO3 side. Pass `--features vinerylab/python`, as CI does.
  `extension-module` is a different feature: it unlinks libpython, so it is
  for maturin builds only and breaks `cargo run` and `cargo test`.
- **Rust tests are inline `#[cfg(test)]` modules** beside the code they cover.
  The top-level `tests/` is Python only, and `test_docs.py` there fails on a
  Markdown link that resolves to nothing or points at a line number.
- **The scene is authored Z-up, in meters** (REP-103). Bevy renders Y-up; the
  correction is a single parent entity, above where the export walk starts.
- **Comments and docs are for someone who has only the code in front of
  them.** A note states a fact about the code or a rule to follow, not how it
  was found, and not the answer to a question only ever asked in a chat. If a
  detail doesn't change what the next reader does, cut it.
