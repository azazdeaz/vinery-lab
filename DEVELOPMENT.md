## Python bindings

No need to install `maturin` yourself — uv fetches it automatically as a
PEP 517 build backend.

The project uses maturin's *mixed* layout: hand-written Python lives in
`python/vinerylab/`, and the compiled Rust extension is built into it as the
`_core` submodule, which `__init__.py` re-exports. `vinerylab.isaaclab` is the
only part that imports Isaac Lab, so plain `import vinerylab` stays usable
without it.

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

Sliders write into the params resources, which re-run the layers below them.
Press `S` to write the scene out as `scene.json`, and build it with:

    python -m vinerylab.usd scene.json scene.usd

`VINERYLAB_PERF=1 cargo run` logs a per-layer breakdown on any frame that
rebuilt something — see `src/perf.rs`.

## Architecture

The scene is built in Bevy as ordinary meshes and transforms, exported as a
plain JSON **scene document**, and turned into USD by Python:

```
params → Bevy entities (Transform, Mesh3d, Name, UsdReference)
              ↓                              ↓
     the viewer renders them      src/scene/export.rs → SceneDoc (JSON)
                                             ↓
                            python/vinerylab/usd/build.py → .usd
```

Rust owns the *scene* — what geometry exists, where it goes, what references
what. Python owns *USD* — prim types, schemas, composition arcs, stage
metadata. `src/scene/doc.rs` is the whole contract between them, and
`build.py`'s module docstring is the only place USD knowledge lives.

There is no intermediate representation on the Rust side and no second scene
graph: the entities the viewer draws are the entities the export walks, so
there is no preview shape and export shape to keep in step.

## Elements

The scene is built from **elements**, one per thing that exists in a vineyard
(terrain, pole, vine, shoot, leaf, grape, weed).

Parts are named the way viticulture names them: a vine's **trunk** rises from
the ground to its **head**, where it turns into one or two **cordons** running
along the fruiting wire; the short pruning stubs on a cordon are **spurs**, and
the annual growth off those is **canes** and **shoots**. A vine with one cordon
is *unilateral*, with two *bilateral*.

Each element is a single file directly under `src/elements/`:

```rust
// src/elements/grape.rs

/// The mesh-library prefix this element registers its geometry under.
pub const PART: &str = "Grape";

/// One berry's shape, as the bunch that grew it specified.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct GrapeConfig { pub radius: f32, pub ripeness: f32, /* ... */ }

/// Two berries share a mesh when they are close in every dimension that shows.
pub struct GrapeMetric;
impl Metric<GrapeConfig> for GrapeMetric { /* weighted L2 over the fields */ }

#[derive(Resource, Clone, Debug)]
#[cfg_attr(feature = "python", pyo3::pyclass(get_all, set_all))]
pub struct GrapeParams { pub variations: u32, pub radius: f32, /* ... */ }

pub fn plugin(app: &mut App) {
    app.init_resource::<GrapeParams>()
        .add_systems(PreUpdate, build
            .in_set(Grow::Scatter)
            .run_if(configs_changed::<GrapeConfig>));
}

fn build(commands: Commands, library: Library, /* ... */) -> Result<()> { /* ... */ }

pub fn ui() -> impl Scene { /* sliders writing into GrapeParams */ }
```

Adding an element is one new file plus one line in `elements::plugin`.

Everything under `src/elements/` that *isn't* an element lives in
`src/elements/util/`: the geometry kernels (`strand` skins a polyline of radii
into a tube, `outline` fills a shape traced in SVG, `mesh` holds the type they
both produce), the palette (`color` for hue, `material` for how a surface
responds to light), the row-layout solver (`parcel`) and the pass that walks
the layout and places a config on every plant and post (`planting`). The
dividing line is identity, not file size — nothing there corresponds to a thing
that exists in a vineyard, so nothing there gets a mesh library or a line in
`elements::plugin`.

### Drawn shapes

Some shapes are cheaper to draw than to generate. A leaf blade is one, so
`assets/leaves/*.svg` holds one traced outline per leaf shape and
`util::outline` turns each into a filled mesh. A file holds one closed shape,
standing up the page and hanging by the point it attaches at — the bottom of
the drawing; nothing else about it matters, since its own scale is normalized
away and every transform in it is resolved on load. Outlines are pulled in
with `include_str!` rather than read at run time, because the crate also ships
as a Python extension module inside a wheel, where `assets/` is not there to
read.

### The pipeline

Every element is one layer of the same five-step pipeline:

1. **Collect** every config of its own kind, sorted by `scene::Order`.
2. **Cluster** them to `params.variations` representatives.
3. **Build** each representative's mesh once, into the shared `Prototypes`
   library.
4. **Assign** each instance the geometry of the representative it drew.
5. **Expand** — author a *unique* config for the layer below at every frame the
   representative offers.

`planting` starts it by placing a `VineConfig` and a `PoleConfig` on every slot
the layout solved; `leaf` ends it, having nothing below to expand into.

**Frames come from the representative; child configs are per instance.** Step 5
is where the two halves meet. The structural skeleton — how many spurs, where
the buds are — has to come from the mesh that actually got built, or a shoot
would hang in mid-air beside the wood. The *parameters* at each frame are drawn
fresh per instance, so two plants sharing a mesh still carry different canopies.
That split is what makes a hundred vines off four meshes not read as four
meshes.

**Geometry prims are childless and instanceable; structural prims are unique
`Xform`s.**

```text
/Vineyard/Planting/Row_00/Vine_047     Xform, unique
  /Wood                                 -> parts/Vine_3, instanceable
  /Shoot_00_0                           Xform, unique
    /Stem                               -> parts/Shoot_1, instanceable
    /Leaf_00                            -> parts/Leaf_2, instanceable
```

An instanceable prim's descendants are not addressable, and a geometry prim has
none — so the rule is safe and mechanical. An organ with nothing hanging off it
(a post, a leaf) *is* the geometry prim rather than an `Xform` over one, which
at six figures of leaves is half the prims in the scene.

### Quantization

`src/quantize.rs` is the whole of it, and knows nothing about Bevy or botany:

```rust
pub trait Metric<T> { fn distance(&self, a: &T, b: &T) -> f32; }

pub fn farthest_first<T: Clone, M: Metric<T>>(
    items: &[T], k: usize, max_radius: f32, metric: &M,
) -> Codebook<T>;
```

Gonzalez farthest-first traversal — a 2-approximation for metric **k-center**.
k-center rather than k-means on purpose: it minimizes the *worst* distance
rather than a sum, which makes it density-blind, which is what lets a rare
variant survive. One replant among four hundred mature vines still earns a mesh.

Three rules the metrics follow:

- **Configs are flat, and the metric decides what matters.** No shape/instance
  split and no weight structs — one `VineConfig` with all its fields, and a
  `VineMetric` whose `distance` picks fields and hard-codes weights. The weights
  convert each field into roughly how far apart it *looks*.
- **A field the builder ignores must not reach the metric.** `ShootConfig`'s
  `leaf_droop` is pure placement — no part of a stem reads it — so two shoots
  differing only in it share a mesh and still hang their leaves at their own
  angle. Get this wrong and the budget gets spent telling apart things that
  build identically.
- **Categorical fields get a step.** A replant and a mature vine are different
  plants, not different sizes, so `VineMetric` puts them further apart than any
  two vines of one kind can be. A leaf's `outline` is the same: five drawings
  are five shapes, not five nearby numbers, so any budget of five or more keeps
  all five. And because a replant builds no wood at all, any two of them are
  *zero* apart — collapsing them is what stops a budget being spent on plants
  that cost no mesh.

### Variations

`params.variations` is a **budget**, not a count: how many representatives the
clustering may keep. Raise it to spend memory on variety, lower it to trade
variety for memory. The covering radius on the `Codebook` is the diagnostic —
it says how far the worst-served instance is from the mesh it drew.

The catch is that a budget can only buy what the population actually contains.
Configs that are all identical collapse to one representative however high the
budget, so each layer needs at least one real per-instance axis: `VINE_VIGOUR`
(girth, drawn per plant), `SHOOT_VIGOUR` and `SHOOT_SPACING` (length and node
spacing, drawn per shoot). These are constants for now; range-valued params in
the editor are what will replace them.

They go in the *config* rather than into a placement scale. A scale would be
free but invisible to the clustering, so the budget would land on one arbitrary
size instead of covering the range that occurs. Two independent axes beat one:
a budget spent covering a single line buys much less than one covering a plane.

### Randomness

One `SceneParams::seed` for the whole scene. Every layer salts it with a
constant of its own before drawing:

```text
a representative's mesh   seed ^ LAYER_STREAM ^ salt(representative index)
an instance's placement   seed ^ CHILD_STREAM ^ salt(its Order)
```

so nudging one layer never re-rolls another — tuning `shoots_per_spur` must not
reshape the trunk underneath the shoots being tuned. `Rng` is an inlined
SplitMix64 rather than a crate's default generator, because the requirement is
that the same seed gives the same scene on every machine and every version,
which a fixed algorithm guarantees and a crate does not promise.

The **draw order** of a stream is part of an element's output: inserting a draw
in the middle re-rolls everything downstream of it, so elements document their
order where a reader would otherwise be tempted to reorder it. Where a slot can
go empty — a bud that pushed no shoot — the draws happen anyway, so that a
lighter prune drops one shoot instead of reshuffling every one after it.

### Rules

**Each element owns its slice of the scene and rebuilds it from scratch.** A
layer clears its prefix from the mesh library and despawns the children it
spawned before repopulating, so a rebuild that produces fewer representatives
leaves nothing stale behind.

**Elements compose by typed value, not by path.** A layer reads the configs
spawned by the layer above and writes configs for the layer below; nothing
reaches across by prim path.

**Ordering is a `SystemSet` enum**, chained once in `elements::plugin`:

```rust
enum Grow { Terrain, Layout, Planting, Poles, Vines, Shoots, Scatter }
```

The chain is what makes the pipeline a pipeline: each set must have finished
spawning before the one after it queries.

**Rebuild only on change.** `run_if(configs_changed::<XConfig>)` is the
dirty-tracking mechanism — curvo tessellation is expensive enough that this
matters. A layer that also reads a resource the layer above does not re-author
on (`ShootParams` for the shoots a vine hangs) adds `resource_changed` for it.

**Determinism.** Bevy's query iteration order is not stable across runs and a
codebook has to be a function of its population alone, so every organ carries a
`scene::Order` assigned in authoring order and every layer sorts by it before
clustering. `Prototypes` is a `BTreeMap` for the same reason: a document has to
come out byte-identical across runs, because a downstream simulator keys its
cache on those bytes.

### Coordinates

The scene is authored **Z-up, meters** — REP-103, which is what both Isaac Lab
and ROS use. `upAxis` is root-layer-only metadata that does not compose through
references, so a consumer cannot correct for a stage that gets it wrong, and
USD's unauthored default is Y-up: silence is not neutral here, it is wrong.

Bevy's renderer is Y-up, so the viewer's correction is a single parent entity
above the scene root carrying `scene::z_up_to_y_up()`. The export walk starts
*below* it, so the emitted document is Z-up native and no geometry module has
to know.

### Export

`src/scene/export.rs` walks named entities from the `UsdRoot` down and emits a
`SceneDoc`. Unnamed entities and their subtrees are skipped, which is how the
Y-up correction stays out of the file. A prim carrying `UsdReference` may not
have children — the exporter errors rather than emitting something USD would
silently drop.

`build.py` turns the document into a stage. Its docstring is the single home of
every USD rule the project depends on, each of which fails *silently* if
dropped: stage metadata, the parts library as a `class` inside the default
prim, `subdivisionScheme = "none"`, `extent`, `displayColor` at `constant`
interpolation, and — the one that is easiest to get wrong — that a part is an
`Xform` *wrapping* its `Mesh` rather than a bare `Mesh`. A USD instance shares
its *descendants* through a prototype while the instance prim's own attributes
stay on the instance, so referencing a bare mesh and marking it instanceable
yields an empty prototype and a full copy of the points on every instance:
valid, drawn correctly, and with none of the sharing that was the point.

### Python

Each element's params fragment is a `#[pyclass]`; `VineyardParams` aggregates
them as `Py<T>` fields. `Py<T>` is required — a plain field would make the
getter clone, so `params.leaf.detail = 200` would silently mutate a temporary.
For the same reason fragments are declared `skip_from_py_object`: they are
live shared objects, and extracting one by value would hand back a copy.

`VineyardParams.write_usd(path)` generates the scene in Rust, serializes the
document, and hands it to `vinerylab.usd.build_usd` — so `usd-core` is a
dependency of the package rather than of the crate.
`generate_scene_json()` returns the document instead, for a caller that wants
to cache the bytes or build the stage elsewhere. Both release the GIL for the
Rust work: the params are copied out first, so nothing inside touches Python
objects and other threads in a host application like Isaac Sim keep running.
