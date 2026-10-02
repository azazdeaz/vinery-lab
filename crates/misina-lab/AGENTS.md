# misina-lab

The framework under a scene generator: everything that knows nothing about
what is being grown. [README.md](README.md) is the front page, with the
smallest generator end to end, and [docs/](docs/) explains one idea per page.
This file is the map for working on the crate and its Python package.

## Commands

```bash
cargo test -p misina-lab                                          # includes the README's doctest
cargo clippy -p misina-lab --all-targets --features python -- -D warnings
cargo doc --open -p misina-lab                                    # the module docs, README first
```

The repository's full check list, which covers this crate and its Python
package, is in the root `AGENTS.md`.

## Where things are

### Rust

| Path | What it is |
| --- | --- |
| [src/lib.rs](src/lib.rs) | the crate root: the `Generator` and `Params` contracts and the `Build` set |
| [src/params.rs](src/params.rs) | what a params field declares, the walk that reads it, and the `generator!` macro that declares an aggregate |
| [src/ui.rs](src/ui.rs) | the parameter panel, built by walking the params structs |
| [src/viewer.rs](src/viewer.rs) | the interactive Bevy app |
| [src/generate.rs](src/generate.rs) | one headless build cycle, for Python and for tests |
| [src/scene/](src/scene/) | the scene graph's export directives; `export.rs`, the walk that fills the document; `doc.rs`, the JSON contract with Python |
| [src/codegen.rs](src/codegen.rs) | generates the Python stub, the Isaac Lab cfg classes and the parameters page |
| [src/snippet.rs](src/snippet.rs) | the viewer's **Copy Isaac Lab cfg** output |
| [src/python.rs](src/python.rs) | the PyO3 glue under the macro, behind the `python` feature |
| [src/stats.rs](src/stats.rs), [src/perf.rs](src/perf.rs), [src/record.rs](src/record.rs), [src/tour.rs](src/tour.rs) | scene footer, rebuild timing, window capture, and the storyboard player a demo video is recorded with |
| [src/terrain.rs](src/terrain.rs) | the ground: `TerrainParams`, the `Ground` height field, and the one build system every generator stages first |
| [src/palette.rs](src/palette.rs) | linear colour, the per-mesh jitter, and `Response`, how a surface answers light |
| [src/geometry/](src/geometry/) | the geometry kernels: `mesh`, `strand`, `outline`, `shapes`, `scatter`, and `par_map` |
| [src/quantize.rs](src/quantize.rs) | k-center clustering: a population of configs down to `variations` meshes |
| [src/rng.rs](src/rng.rs) | the seeded stream every element draws from |
| [src/testing.rs](src/testing.rs) | reading a built scene back in tests, in this crate and in a generator's; its `fixture` is the smallest generator there is |

### Python

Pure Python over `usd-core`, in [`../../python/misina-lab`](../../python/misina-lab).

| Path | What it is |
| --- | --- |
| [misina_lab/usd/build.py](../../python/misina-lab/misina_lab/usd/build.py) | scene document to stage; where every USD rule is written down |
| [misina_lab/usd/ground.py](../../python/misina-lab/misina_lab/usd/ground.py) | `Ground`: the terrain height under any (x, y), read back off a stage |
| [misina_lab/isaaclab/spawn.py](../../python/misina-lab/misina_lab/isaaclab/spawn.py) | `GeneratedSceneCfg`, the base of every generator's cfg; generates, caches and spawns the `.usd` |
| [misina_lab/isaaclab/rods.py](../../python/misina-lab/misina_lab/isaaclab/rods.py) | the coupled Newton config that lets a rod bend, and the tuning and collision groups it needs |
| [misina_lab/isaaclab/cutting.py](../../python/misina-lab/misina_lab/isaaclab/cutting.py) | `Shears`: cuts a rod anywhere along it while the simulation runs |

Only `misina_lab.isaaclab` imports Isaac Lab; plain `import misina_lab`
works without it.

## Docs

- [docs/generators.md](docs/generators.md) — what a generator declares, and every surface the framework builds from it
- [docs/elements.md](docs/elements.md) — how a scene is built: layers, configs, the mesh library, and the rules a layer follows
- [docs/quantization.md](docs/quantization.md) — a population of configs down to a budget of meshes, and how to write a metric
- [docs/randomness.md](docs/randomness.md) — one seed, many streams
- [docs/export.md](docs/export.md) — coordinates, the scene document, and what the USD builder makes of it
- [docs/isaac-lab.md](docs/isaac-lab.md) — spawning a generated scene: the cfg base, the cache, rods, and cutting them
- [docs/editing-parameters.md](docs/editing-parameters.md) — how to add or change a parameter

## Rules that bite

- **Nothing here names a generator.** The example everywhere is the row of
  boxes in the README and `testing::fixture`. A vine, a tomato or a post
  belongs in a generator's crate.
- **A parameter is declared once, on its Rust params struct.** The panel,
  the snippet, the Python class, the stub and the parameters page all derive
  from it. [docs/editing-parameters.md](docs/editing-parameters.md) is the
  how-to.
- **The README is the crate's rustdoc front page**, through
  `#![doc = include_str!]` in `lib.rs`, and its Rust block is a doctest.
  Keep the block compiling.
- **The `python` feature is off by default**, so a plain `cargo clippy`
  never sees the PyO3 side. `extension-module` is a generator's feature, not
  this crate's: it unlinks libpython, so it is for maturin builds only.
- **Rust tests are inline `#[cfg(test)]` modules** beside the code they
  cover, run against `testing::fixture::Boxes`.
- **The scene is authored Z-up, in meters** (REP-103). Bevy renders Y-up;
  the correction is a single parent entity, above where the export walk
  starts.
- **Doc pages link files and symbol names, never line numbers.** A page
  explains how modules fit; what a module is and its contract is its module
  doc, and a sentence that would fit both goes in the module doc.
