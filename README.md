## Project description

This is a parametric vineyard generator. It generates USD scenes for robotics simulation. Mainly targeting Isaac Lab.


## Workflow
 - Start the viewer `cargo run --release`.
 - Edit the scene parameters in the UI.
 - # TODO: Copy the generated Isaac Lab configuration and paste it into your Isaac Lab envionment configuration file.
 - Run Isaac Lab environment. The provided python wrapper will generate the USD scene,


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

### Implementation

Design notes for development.

## Elements

The scene is built from **elements** — self-contained producers of USD, one per
thing that exists in a vineyard (terrain, pole, vine, cane, leaf, grape, weed).

Parts are named the way viticulture names them: a vine's **trunk** rises from
the ground to its **head**, where it turns into one or two **cordons** running
along the fruiting wire; the short pruning stubs on a cordon are **spurs**, and
the annual growth off those is **canes** and **shoots**. A vine with one cordon
is *unilateral*, with two *bilateral*.

Each element is a single file under `src/elements/` holding four items:

```rust
// src/elements/leaf.rs

/// The contract other elements rely on: this element guarantees a prototype here.
pub const PROTOTYPE: &str = "/parts/Leaf";

#[derive(Resource, Clone, Debug)]
#[cfg_attr(feature = "python", pyo3::pyclass(get_all, set_all))]
pub struct LeafParams { pub variations: u32, pub length: f32, /* ... */ }

pub fn plugin(app: &mut App) {
    app.init_resource::<LeafParams>()
        .add_systems(PreUpdate, author
            .in_set(Grow::Prototypes)
            .run_if(resource_changed::<LeafParams>));
}

fn author(live: NonSend<LiveStage>, params: Res<LeafParams>) -> Result<()> { /* ... */ }

pub fn ui() -> impl Scene { /* sliders writing into LeafParams */ }
```

Adding an element is one new file plus one line in `elements::plugin`.

### Rules

**Each element owns exactly one prim subtree.** Its author fn removes that
subtree and rewrites it from scratch; nothing else is allowed to touch it.
Prototypes go under `/parts/<Name>`, placed instances under `/Vineyard/...`.
`terrain` additionally *defines* the scene root `/Vineyard` and declares it the
stage's default prim — it never removes it, so sibling elements keep their own
subtrees under it.
`/parts` itself belongs to no element — `stage::new_stage` defines it and
marks it invisible, so prototypes don't render as a pile of stray geometry at
the origin. Instances are unaffected; they hang off their instancer.

**Elements compose by prim path only.** An element that instances another just
targets its `PROTOTYPE` path and trusts it to exist — no Rust data is passed
between elements. Placement is computed by whoever does the instancing, in its
own local space: `cane` decides where leaves sit on a cane, `vine` where canes
sit on its spurs, `terrain` where poles and weeds sit on the ground. This nests,
so `/parts/Vine` already contains its canes, which already contain their leaves.

An element may own a *second* subtree when it also places its own prototypes —
`vine` authors `/parts/Vine` and plants it from `/Vineyard/Vines`. The rule is
unchanged: nobody else touches either.

**Ordering is a `SystemSet` enum**, chained once in `elements::plugin`, with
prototype authoring first so the path contract always holds:

```rust
enum Grow { Prototypes, Terrain, Layout, Plants, Scatter, Randomize }
```

**Re-author only on change.** `run_if(resource_changed::<XParams>)` is the
dirty-tracking mechanism — curvo tessellation is expensive enough that this
matters. Use `Local` only for private scratch (RNG state, reusable buffers).

### Variations

Every element authors `params.variations` prototypes (`/parts/Leaf/Var_0`, `…`).
Instancing uses a `PointInstancer` whose `prototypes` relationship lists all of
them; picking a variation per instance is just filling `protoIndices` from a
seeded RNG. Rewriting `protoIndices` is an attribute-only edit, so re-rolling
variations patches the stage without resyncing or recomputing any geometry.

Variety comes from the product of variation counts across nesting levels: all
instances of `Vine/Var_0` share one cane arrangement, so upper-level counts
matter more than they look.

### Stage handling

One long-lived `Stage`, held in a `NonSend` `LiveStage` in **both** the viewer
and the headless/Python path (`LiveStage` has no Bevy dependency, and `Stage` is
`!Send`). Author fns mutate it in place; in the viewer `LiveStagePlugin` picks
up the diff and reprojects only what changed. The headless path never drains the
change queue.

There is **no intermediate ECS scene** — params author straight to the stage.
The only entities in the world are the ones `project_stage` creates for
rendering, plus params resources and UI.

Use `openusd::schemas::*` typed schemas for geometry (they author
`custom = false` correctly) and `usd_bevy::authoring` for namespace ops like
`remove_prim`.

### Python

Each element's params fragment is a `#[pyclass]`; `VineyardParams` aggregates
them as `Py<T>` fields. `Py<T>` is required — a plain field would make the
getter clone, so `params.leaf.length = 0.2` would silently mutate a temporary.
For the same reason fragments are declared `skip_from_py_object`: they are
live shared objects, and extracting one by value would hand back a copy.