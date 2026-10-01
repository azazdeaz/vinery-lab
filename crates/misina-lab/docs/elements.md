# Elements

An **element** is one module per kind of thing in the scene: a post, a
plant, a leaf, a patch of ground cover. It owns a params fragment, a config
component, a metric over configs, a build system, and the plugin that wires
them. What a scene is made of is the list of its elements, and adding one is
one file plus one line in the generator's plugin.

```rust
/// The mesh-library prefix this element registers its geometry under.
pub const PART: &str = "Leaf";

/// One leaf's shape, as the stem that grew it specified.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct LeafConfig { pub length: f32, pub droop: f32, /* ... */ }

/// Two leaves share a mesh when they are close in every dimension that shows.
pub struct LeafMetric;
impl Metric<LeafConfig> for LeafMetric { /* weighted L2 over the fields that build */ }

/// The struct's doc comment is the Python class docstring.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "python", pyo3::pyclass(get_all, set_all, skip_from_py_object))]
pub struct LeafParams {
    /// Blade length, in meters. A field's first paragraph is its tooltip and docstring.
    #[reflect(@Slider { min: 0.02, max: 0.2, step: 0.005 })]
    pub length: f32,
    /* ... */
}

pub fn plugin(app: &mut App) {
    app.init_resource::<LeafParams>().add_systems(PreUpdate, (
        reauthor.run_if(resource_changed::<LeafParams>),
        build.run_if(configs_changed::<LeafConfig>),
    ).chain().in_set(Grow::Leaves));
}

/// Re-applies the params to every leaf already hanging, in place.
fn reauthor(params: Res<LeafParams>, mut leaves: Query<&mut LeafConfig>) { /* ... */ }

fn build(commands: Commands, library: Library, /* ... */) -> Result<()> { /* ... */ }
```

The panel section, the config snippet, the Python class, the stub and the
parameters page all follow from the params struct
([editing-parameters.md](editing-parameters.md)), and `cargo test` fails on
whatever was missed.

## What an element is built from

The geometry kernels know nothing about what they are shaping. `strand`
skins a polyline of radii into a tube, `outline` fills a shape traced in
SVG, `shapes` builds outlines in code and folds them, `scatter` spreads
points over a band of ground, and `mesh` holds the type they all produce.
`palette` turns a hex colour linear, jitters it per mesh and pairs it with a
`Response`, how a surface answers light. `terrain` is the one element this
crate ships: the ground, staged first by every generator, and the `Ground`
height field everything placed on it drapes onto.

What is specific to a kind of scene but is not an element, a layout solver,
the pass that places a config on every plant, a named palette, belongs to the
generator. The dividing line is identity: nothing there corresponds to a
thing that exists in the scene, so nothing there gets a mesh-library prefix
or a stage set.

### Drawn shapes

Some shapes are cheaper to draw than to generate. A leaf blade is one, so a
generator keeps one traced SVG outline per shape and `geometry::outline`
turns it into a filled mesh. A file holds one closed shape, standing up the
page and hanging by the point it attaches at, the bottom of the drawing; its
own scale is normalized away and every transform in it is resolved on load.
Outlines are pulled in with `include_str!` rather than read at run time,
because the crate also ships as a Python extension module inside a wheel,
where an assets directory is not there to read.

Some are cheaper to describe: a grass blade is a length, a width and a taper,
and `geometry::shapes` builds those as outlines in the same frame a drawing
is read into. A traced file replaces a `shapes::` call at the one place it is
made.

## The pipeline

Every element is one layer of the same five-step pipeline:

1. **Collect** every config of its own kind, sorted by `scene::Order`.
2. **Cluster** them to `params.variations` representatives
   ([quantization.md](quantization.md)).
3. **Build** each representative's mesh once, into the shared `Prototypes`
   library.
4. **Assign** each instance the geometry of the representative it drew.
5. **Expand**: author a *unique* config for the layer below at every frame
   the representative offers.

Whatever places the first configs on the ground starts it; the bottom layer
ends it, having nothing below to expand into.

**Frames come from the representative; child configs are per instance.**
Step 5 is where the two halves meet. The skeleton, how many branches and
where the buds are, has to come from the mesh that actually got built, or a
stem would hang in mid-air beside the wood. The *parameters* at each frame
are drawn fresh per instance, so two plants sharing a mesh still carry
different canopies. That split is what makes a hundred plants off four
meshes not read as four meshes.

**Geometry prims are childless and instanceable; structural prims are unique
`Xform`s.**

```text
/Scene/Planting/Row_00/Plant_047     Xform, unique
  /Wood                               -> parts/Plant_3, instanceable
  /Collision                          Capsule, the trunk's physics proxy
  /Stem_00                            Xform, unique
    /Tube                             -> parts/Stem_1, instanceable
    /Leaf_00                          -> parts/Leaf_2, instanceable
```

An instanceable prim's descendants are not addressable, and a geometry prim
has none, so the rule is safe and mechanical. An organ with nothing hanging
off it, a leaf, *is* the geometry prim rather than an `Xform` over one,
which at six figures of leaves is half the prims in the scene.

## Rules

**Each element owns its slice of the scene and rebuilds it from scratch.** A
layer clears its prefix from the mesh library and despawns the children it
spawned before repopulating, so a rebuild that produces fewer representatives
leaves nothing stale behind.

**Elements compose by typed value, not by path.** A layer reads the configs
spawned by the layer above and writes configs for the layer below; nothing
reaches across by prim path.

**Ordering is a `SystemSet` enum**, one variant per layer, chained once in
the generator's plugin inside `Build`. The chain is what makes the pipeline
a pipeline: each set must have finished spawning before the one after it
queries.

**Rebuild only on change.** `run_if(configs_changed::<XConfig>)` is the
dirty-tracking mechanism; curve tessellation is expensive enough that this
matters.

**A params edit is re-applied in place, not re-authored from above.** A
layer owns the configs the layer above placed, so it runs a `reauthor` pass
that writes its own params onto them with `set_if_neq` and leaves everything
else, the position and the draws they were authored from, exactly where it
was. Editing a leaf param therefore re-cuts the blades instead of replanting
the scene to reach them. Only a param that reaches no config field, any
`variations`, still needs a `resource_changed` on the layer's own build.

**Combine run conditions with `or_eager`, never `or_else`.** `or_else`
short-circuits, and a condition system that does not run does not advance
its `last_run`, so the change it skipped reads as new again the next frame
and rebuilds the layer a second time.

**A layout-driven element authors its own configs.** An element that hangs
off no layer above, ground cover for one, places configs in an `author`
system from the layout, the `Ground` and the scene params, gated on all of
them and on its own params, and has no `reauthor`: re-authoring the layer
*is* the in-place edit at its scale. Its `build` is gated on the same
condition as well as on `configs_changed`, so a change that leaves nothing
to build still clears the library.

**A categorical param is a string naming one of the element's fixed list**,
parsed once into a Rust enum with `ALL`, `NAMES` and `parse`, and declared on
the field as `@Choices(&Kind::NAMES)`. The name is validated where it enters
from Python; the element itself falls back to the default with a warning,
because a build system is no place to fail. The viewer offers the list as a
dropdown and only ever writes a valid name.

**Determinism.** Bevy's query iteration order is not stable across runs and
a codebook has to be a function of its population alone, so every organ
carries a `scene::Order` assigned in authoring order and every layer sorts by
it before clustering. `Prototypes` is a `BTreeMap` for the same reason: a
document has to come out byte-identical across runs, because a downstream
simulator keys its cache on those bytes.

## Where it lives

- [`src/scene/mod.rs`](../src/scene/mod.rs): the scene graph and its export directives; `Library`, `Prototypes`, `Order`, `configs_changed`, `placed`, and the capsule and cable proxies
- [`src/geometry/`](../src/geometry/): the kernels
- [`src/terrain.rs`](../src/terrain.rs): the ground
- [`src/palette.rs`](../src/palette.rs): colour and surface response
- [`src/testing.rs`](../src/testing.rs): reading a built scene back in a test; `organs`, `prim`, `prim_path`
