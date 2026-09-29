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
the parts that know nothing about vines — and `crates/vinerylab` is the
vineyard built on it.

| Path | What it is |
| --- | --- |
| [misina-lab/src/lib.rs](crates/misina-lab/src/lib.rs) | the framework's crate root |
| [misina-lab/src/geometry/](crates/misina-lab/src/geometry/) | the geometry kernels: `mesh`, `strand`, `outline`, `shapes`, `scatter`, and `par_map` |
| [misina-lab/src/quantize.rs](crates/misina-lab/src/quantize.rs) | k-center clustering: a population of configs down to `variations` meshes |
| [misina-lab/src/rng.rs](crates/misina-lab/src/rng.rs) | the seeded stream every element draws from |
| [misina-lab/src/scene/doc.rs](crates/misina-lab/src/scene/doc.rs) | the JSON contract with Python |
| [misina-lab/src/testing.rs](crates/misina-lab/src/testing.rs) | mesh readers for tests, in either crate |
| [vinerylab/src/lib.rs](crates/vinerylab/src/lib.rs) | crate root; the architecture in one paragraph |
| [vinerylab/src/main.rs](crates/vinerylab/src/main.rs) | the `vinerylab` binary; calls `viewer::run` |
| [vinerylab/src/viewer.rs](crates/vinerylab/src/viewer.rs) | the interactive Bevy app |
| [vinerylab/src/ui.rs](crates/vinerylab/src/ui.rs) | the parameter panel, built by walking the params structs |
| [vinerylab/src/params.rs](crates/vinerylab/src/params.rs) | what a params field declares: caption, range, choices, tooltip |
| [vinerylab/src/codegen.rs](crates/vinerylab/src/codegen.rs) | generates the Python stub, the Isaac Lab cfg classes and `docs/parameters.md` |
| [vinerylab/src/elements/](crates/vinerylab/src/elements/) | one module per vineyard thing; `mod.rs` holds the pipeline contract |
| [vinerylab/src/elements/util/](crates/vinerylab/src/elements/util/) | palette, layout solver, planting walk — everything under `elements/` that is not an element |
| [vinerylab/src/scene/](crates/vinerylab/src/scene/) | the scene graph's export directives, and `export.rs`, the walk that fills the document |
| [vinerylab/src/generate.rs](crates/vinerylab/src/generate.rs) | one headless build cycle, for Python and for tests |
| [vinerylab/src/snippet.rs](crates/vinerylab/src/snippet.rs) | the viewer's **Copy Isaac Lab cfg** output |
| [vinerylab/src/stats.rs](crates/vinerylab/src/stats.rs), [vinerylab/src/perf.rs](crates/vinerylab/src/perf.rs), [vinerylab/src/record.rs](crates/vinerylab/src/record.rs) | scene footer, per-layer timing, window capture |
| [vinerylab/src/python.rs](crates/vinerylab/src/python.rs) | the PyO3 wrapper, behind the `python` feature |

### Python, under `crates/vinerylab/python/vinerylab/`

| Path | What it is |
| --- | --- |
| [usd/build.py](crates/vinerylab/python/vinerylab/usd/build.py) | scene document to stage; where every USD rule is written down |
| [usd/ground.py](crates/vinerylab/python/vinerylab/usd/ground.py) | `Ground`: the terrain height under any (x, y), read back off a stage |
| [isaaclab/vineyard_cfg.py](crates/vinerylab/python/vinerylab/isaaclab/vineyard_cfg.py) | `@configclass` fragments mirroring the Rust params |
| [isaaclab/vineyard.py](crates/vinerylab/python/vinerylab/isaaclab/vineyard.py) | generates, caches and spawns the `.usd` |
| [isaaclab/physics.py](crates/vinerylab/python/vinerylab/isaaclab/physics.py) | the coupled Newton config that lets shoots bend |
| [isaaclab/cutting.py](crates/vinerylab/python/vinerylab/isaaclab/cutting.py) | `Shears`: cuts a shoot anywhere along it while the simulation runs |
| [isaaclab/newton_patches.py](crates/vinerylab/python/vinerylab/isaaclab/newton_patches.py) | workarounds for the pinned Newton, applied by the demos; not imported by the package |
| [_core.pyi](crates/vinerylab/python/vinerylab/_core.pyi) | typed signatures for the compiled extension (generated) |

Only `vinerylab.isaaclab` imports Isaac Lab, so plain `import vinerylab` works
without it.

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
  `_core.pyi`, `isaaclab/vineyard_cfg.py` (both under `crates/vinerylab/python/vinerylab/`) or
  `docs/parameters.md` are stale; `cargo test regen_params -- --ignored`
  rewrites them.
- **The `python` feature is off by default**, so a plain `cargo clippy` never
  sees `src/python.rs`. Pass `--features vinerylab/python`, as CI does.
  `extension-module` is a different feature: it unlinks libpython, so it is
  for maturin builds only and breaks `cargo run` and `cargo test`.
- **Rust tests are inline `#[cfg(test)]` modules** beside the code they cover.
  The top-level `tests/` is Python only.
- **The scene is authored Z-up, in meters** (REP-103). Bevy renders Y-up; the
  correction is a single parent entity, above where the export walk starts.
