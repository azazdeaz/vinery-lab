## Python bindings

Requires `bevy_openusd` **and** `openusd` checked out as sibling directories
(both are path dependencies in `Cargo.toml`). No need to install `maturin`
yourself — uv fetches it automatically as a PEP 517 build backend.

`openusd` is patched rather than used from git: its crate-file writer types
`TfTokenVector` fields (`primChildren`, `xformOpOrder`) as `VtArray<TfToken>`,
which Pixar USD refuses to open, so every generated `.usd`/`.usdc` was
unreadable by Isaac Sim. The fix is two tokens in `write_token_vec`; see the
`[patch]` stanza in `Cargo.toml`. Check it out at the rev `usd_bevy` pins:

    git clone https://github.com/mxpv/openusd ../openusd
    git -C ../openusd checkout -b fix/token-vector-value-type \
        7934d9f3a375fabc93c14dc96b7900cbd035204d

A `[patch]` is only honoured in the workspace root, so building `usdview` in
`bevy_openusd` standalone needs the same stanza there.

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

### Implementation

Design notes for development.

## Elements

The scene is built from **elements** — self-contained producers of USD, one per
thing that exists in a vineyard (terrain, pole, vine, shoot, leaf, grape, weed).

Parts are named the way viticulture names them: a vine's **trunk** rises from
the ground to its **head**, where it turns into one or two **cordons** running
along the fruiting wire; the short pruning stubs on a cordon are **spurs**, and
the annual growth off those is **canes** and **shoots**. A vine with one cordon
is *unilateral*, with two *bilateral*.

Each element is a single file directly under `src/elements/` holding four
items:

```rust
// src/elements/grape.rs

/// The contract other elements rely on: this element guarantees a prototype here.
pub const PROTOTYPE: &str = "/Vineyard/parts/Grape";

#[derive(Resource, Clone, Debug)]
#[cfg_attr(feature = "python", pyo3::pyclass(get_all, set_all))]
pub struct GrapeParams { pub variations: u32, pub berry_radius: f32, /* ... */ }

pub fn plugin(app: &mut App) {
    app.init_resource::<GrapeParams>()
        .add_systems(PreUpdate, author
            .in_set(Grow::Prototypes)
            .run_if(resource_changed::<GrapeParams>));
}

fn author(live: NonSend<LiveStage>, params: Res<GrapeParams>) -> Result<()> { /* ... */ }

pub fn ui() -> impl Scene { /* sliders writing into GrapeParams */ }
```

Adding an element is one new file plus one line in `elements::plugin`.

Everything under `src/elements/` that *isn't* an element lives in
`src/elements/util/`: the USD authoring plumbing (`usd`), the tube-skinning
geometry kernel (`strand`), the one that fills a shape traced in SVG
(`outline`), the row-layout solver (`parcel`), the two ways a prototype gets
put on the ground (`place`), and the pass that walks the layout and plants them
(`planting`). The dividing line is identity, not file size — nothing there
corresponds to a thing that exists in a vineyard, so nothing there gets a
`/parts/<Name>` prototype or a line in `elements::plugin`.

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

### Rules

**Each element owns exactly one prim subtree.** Its author fn removes that
subtree and rewrites it from scratch; nothing else is allowed to touch it.
Prototypes go under `/Vineyard/parts/<Name>`, placed instances under
`/Vineyard/...`.
`/Vineyard` and the prototype library both belong to no element —
`stage::new_stage` defines the scene root, declares it the stage's default prim,
and defines `/Vineyard/parts` as a `class`, which keeps prototypes out of the
viewer's traversal so they don't render as a pile of stray geometry at the
origin. Neither is ever removed, so sibling elements keep their own subtrees
under the root. Placed instances are unaffected: a reference composes the target's
own opinions, not the ancestors it happened to sit under, and a `PointInstancer`
instance hangs off the instancer rather than off `/parts`.

**Elements compose by prim path only.** An element that instances another just
targets its `PROTOTYPE` path and trusts it to exist — no Rust data is passed
between elements. Placement is computed by whoever does the placing, in its own
local space: `vine` decides where shoots sit on its spurs, `shoot` where leaves
sit on a shoot, `terrain` where vines, poles and weeds sit on the ground. This
nests, so `/parts/Vine` contains its shoots and each of those contains its
leaves — which is why a prototype is an `Xform` over its own mesh rather than a
bare `Mesh` as soon as anything grows on it.

Nesting is also what keeps the canopy affordable. A leaf is placed inside a
*shoot prototype*, so a vineyard's worth of leaves costs a few dozen placements
on the stage rather than one per leaf. The trade is that every instance of a
given shoot variation carries an identical canopy, which is the usual variation
arithmetic below.

An element may own a *second* subtree when it also places prototypes — `terrain`
authors `/Vineyard/Terrain` and, through `util::planting`, everything standing on
it at `/Vineyard/Planting`. Or when one thing comes in two shapes: `vine` also
authors `/parts/YoungVine`, a replant in its first season, which is a `shoot`
buried deep enough to hide the bend at its base and no wood at all. The rule is
unchanged: nobody else touches any of them, and `vine` still authors shapes only
— where a vine stands, and whether a slot got the young one, is `planting`'s
call.

**Ordering is a `SystemSet` enum**, chained once in `elements::plugin`, with
prototype authoring first so the path contract always holds:

```rust
enum Grow { Prototypes, Terrain, Layout, Plants, Scatter, Randomize }
```

**Re-author only on change.** `run_if(resource_changed::<XParams>)` is the
dirty-tracking mechanism — curvo tessellation is expensive enough that this
matters. Use `Local` only for private scratch (RNG state, reusable buffers).

### Placement

Two ways, both in `util::place`, taking the same `Placement` list:

`place_referenced` gives every instance **its own prim**, an internal reference
to the prototype carrying a `translate`/`rotateZ`/`scale` stack. That prim has a
path, which is what Isaac Lab needs to attach a semantic label, bind a rigid
body, or randomize one plant — so this is what anything addressable uses (vines,
posts). The referencing prim is defined with the *prototype's own* type name,
read off the stage: a reference is a weaker opinion than a local `typeName`, so
an `Xform` over a `Mesh` prototype composes to an `Xform` carrying stray
`points` that no renderer draws.

`place_instanced` authors **one** `PointInstancer` holding parallel arrays for
all of them. No instance has a path, but the stage cost is flat — the only
workable option for scatter that is never addressed individually (weeds, leaves,
grapes), where per-instance prims would run to five figures.

Anything that could go either way asks `place::Style` and calls `place`, which
dispatches. It is a resource, defaulting to `Referenced` — the export shape,
since that is the one Isaac Lab can address. The **viewer forces `Instanced`**:
nothing there addresses an individual plant, and re-authoring a parcel as four
arrays per row instead of a prim and three attributes per vine is what a slider
drag pays for on every frame it moves. Pressing `S` in the viewer still writes
the export shape — it re-generates the scene headlessly rather than saving the
stage on screen.

The style has to be **the same for a whole authoring pass**, which is why it is
a resource and not an argument each caller picks. The combination that breaks is
a `PointInstancer` nested inside a reference-placed prototype: its `prototypes`
relationship targets the library it draws from, outside the referenced subtree,
so the reference's namespace mapping drops it and the instancer keeps every
instance while losing every prototype — the same silent failure that puts
`/Vineyard/parts` inside the default prim, one level down.

Placed prims are named for their *slot*, not their rank: `Row_000/Vine_007` is
the eighth planting position of the first row whether or not slots before it
were skipped, so a config keyed on a path doesn't silently repoint when
`miss_rate` moves. It keeps that name whichever library it was planted from, so
`young_rate` can't repoint one either.

A row is placed as one batch per library — its mature vines, its replants, its
posts — since a batch draws from a single `/parts/<Name>`. Referenced, all of
them land as prims directly under the row; instanced, each becomes its own
`PointInstancer`.

### Variations

Every element authors `params.variations` prototypes
(`/Vineyard/parts/Leaf/Var_0`, `…`),
and whoever places them picks one per instance from a seeded RNG. For a
`PointInstancer` that means filling `protoIndices`, an attribute-only edit, so
re-rolling patches the stage without resyncing or recomputing any geometry. For
reference-placed prims it means rewriting `references` metadata, which is a
composition change and forces a resync of the subtree — the price of every
instance having a path.

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
