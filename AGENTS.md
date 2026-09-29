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
per generator, which every framework surface is keyed on.

| Path | What it is |
| --- | --- |
| [misina-lab/src/lib.rs](crates/misina-lab/src/lib.rs) | the framework's crate root: the `Generator` and `Params` contracts and the `Build` set |
| [misina-lab/src/params.rs](crates/misina-lab/src/params.rs) | what a params field declares, the walk that reads it, and the `generator!` macro that declares an aggregate |
| [misina-lab/src/ui.rs](crates/misina-lab/src/ui.rs) | the parameter panel, built by walking the params structs |
| [misina-lab/src/viewer.rs](crates/misina-lab/src/viewer.rs) | the interactive Bevy app |
| [misina-lab/src/generate.rs](crates/misina-lab/src/generate.rs) | one headless build cycle, for Python and for tests |
| [misina-lab/src/scene/](crates/misina-lab/src/scene/) | the scene graph's export directives; `export.rs`, the walk that fills the document; `doc.rs`, the JSON contract with Python |
| [misina-lab/src/codegen.rs](crates/misina-lab/src/codegen.rs) | generates the Python stub, the Isaac Lab cfg classes and the parameters page |
| [misina-lab/src/snippet.rs](crates/misina-lab/src/snippet.rs) | the viewer's **Copy Isaac Lab cfg** output |
| [misina-lab/src/python.rs](crates/misina-lab/src/python.rs) | the PyO3 glue under the macro, behind the `python` feature |
| [misina-lab/src/stats.rs](crates/misina-lab/src/stats.rs), [misina-lab/src/perf.rs](crates/misina-lab/src/perf.rs), [misina-lab/src/record.rs](crates/misina-lab/src/record.rs) | scene footer, rebuild timing, window capture |
| [misina-lab/src/terrain.rs](crates/misina-lab/src/terrain.rs) | the ground: `TerrainParams`, the `Ground` height field, and the one build system every generator stages first |
| [misina-lab/src/palette.rs](crates/misina-lab/src/palette.rs) | linear colour, the per-mesh jitter, and `Response`, how a surface answers light |
| [misina-lab/src/geometry/](crates/misina-lab/src/geometry/) | the geometry kernels: `mesh`, `strand`, `outline`, `shapes`, `scatter`, and `par_map` |
| [misina-lab/src/quantize.rs](crates/misina-lab/src/quantize.rs) | k-center clustering: a population of configs down to `variations` meshes |
| [misina-lab/src/rng.rs](crates/misina-lab/src/rng.rs) | the seeded stream every element draws from |
| [misina-lab/src/testing.rs](crates/misina-lab/src/testing.rs) | reading a built scene back in tests, in either crate; its `fixture` is the smallest generator there is |
| [vinerylab/src/lib.rs](crates/vinerylab/src/lib.rs) | crate root: the `Vineyard` generator and the `_core` Python module |
| [vinerylab/src/main.rs](crates/vinerylab/src/main.rs) | the `vinerylab` binary: the viewer plus the layout gizmos and the per-layer timing |
| [vinerylab/src/elements/](crates/vinerylab/src/elements/) | one module per vineyard thing; `mod.rs` holds the pipeline contract, the ground and layout wiring, and the `generator!` call |
| [vinerylab/src/elements/util/](crates/vinerylab/src/elements/util/) | the named colours and responses, layout solver, planting walk — everything under `elements/` that is not an element |
| [vinerylab/src/perf.rs](crates/vinerylab/src/perf.rs) | the per-layer marks and the bench |

### Python

Two packages in one uv workspace, mirroring the crates: `python/misina-lab`
is the framework's Python side, pure Python over `usd-core`, and
`crates/vinerylab/python/vinerylab` is the vineyard's, a maturin package
holding the compiled `_core` and the generated cfg classes.

| Path | What it is |
| --- | --- |
| [misina_lab/usd/build.py](python/misina-lab/misina_lab/usd/build.py) | scene document to stage; where every USD rule is written down |
| [misina_lab/usd/ground.py](python/misina-lab/misina_lab/usd/ground.py) | `Ground`: the terrain height under any (x, y), read back off a stage |
| [misina_lab/isaaclab/spawn.py](python/misina-lab/misina_lab/isaaclab/spawn.py) | `GeneratedSceneCfg`, the base of every generator's cfg; generates, caches and spawns the `.usd` |
| [misina_lab/isaaclab/rods.py](python/misina-lab/misina_lab/isaaclab/rods.py) | the coupled Newton config that lets a rod bend, and the tuning and collision groups it needs |
| [misina_lab/isaaclab/cutting.py](python/misina-lab/misina_lab/isaaclab/cutting.py) | `Shears`: cuts a rod anywhere along it while the simulation runs |
| [vinerylab/isaaclab/vineyard_cfg.py](crates/vinerylab/python/vinerylab/isaaclab/vineyard_cfg.py) | `@configclass` fragments mirroring the Rust params (generated), and `VineyardCfg` over them |
| [vinerylab/isaaclab/physics.py](crates/vinerylab/python/vinerylab/isaaclab/physics.py) | the vineyard's names and numbers over `rods`: which prim a vine is, the demos' substeps |
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
  sees the PyO3 side. Pass `--features vinerylab/python`, as CI does.
  `extension-module` is a different feature: it unlinks libpython, so it is
  for maturin builds only and breaks `cargo run` and `cargo test`.
- **Rust tests are inline `#[cfg(test)]` modules** beside the code they cover.
  The top-level `tests/` is Python only.
- **The scene is authored Z-up, in meters** (REP-103). Bevy renders Y-up; the
  correction is a single parent entity, above where the export walk starts.
