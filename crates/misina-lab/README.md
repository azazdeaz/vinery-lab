# misina-lab

What a procedural scene generator is built from. A generator declares its
parameters as plain Rust structs and builds its scene in
[Bevy](https://bevy.org) as ordinary meshes and transforms; everything around
that is here. The viewer with its parameter panel, the mesh quantizer, the
seeded random stream, the export to a JSON scene document, the USD builder
that turns the document into a stage, and the Isaac Lab spawner that
generates and caches it. One crate ships a generator both as an interactive
viewer and as a Python extension module.

```text
params ──▶ build systems ──▶ Bevy entities ──▶ scene document (JSON) ──▶ USD stage ──▶ Isaac Lab
                                  │
                                  └──▶ the viewer draws them as they are
```

Two halves make one product. The Rust crate, `misina-lab`, builds and exports
the scene. The Python package, `misina_lab` in
[`python/misina-lab`](../../python/misina-lab), turns the document into USD
and spawns it. The line between them is one serde struct,
`scene::doc::SceneDoc`, and the same JSON is what a test in either half
asserts against.

## The smallest generator

A generator is a marker type implementing `Generator`, a params aggregate
declared with `generator!`, and one build system per element, spawning named
entities under the scene root. This one has a single element, a row of boxes.
It is this crate's doctest, so it compiles and runs under `cargo test`:

```rust
use bevy::prelude::*;
use misina_lab::geometry::mesh::box_mesh;
use misina_lab::params::Slider;
use misina_lab::scene::{Library, Order, PrimRoot, Surface};
use misina_lab::{Build, Generator};

/// A row of boxes along X. The struct's doc comment is the Python class docstring.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "python", pyo3::pyclass(get_all, set_all, skip_from_py_object))]
pub struct BoxParams {
    /// How many boxes stand in the row. A field's first paragraph is its tooltip.
    #[reflect(@Slider { min: 1.0, max: 8.0, step: 1.0 })]
    pub count: u32,
    /// Edge length of a box, in meters.
    #[reflect(@Slider { min: 0.1, max: 2.0, step: 0.1 })]
    pub size: f32,
}

impl Default for BoxParams {
    fn default() -> Self {
        Self { count: 3, size: 0.5 }
    }
}

// The aggregate: one field per element, and the Python class under the quoted name.
misina_lab::generator! {
    /// Every knob of the box row.
    pub struct BoxesParams as PyBoxesParams("BoxesParams") for Boxes {
        pub boxes: BoxParams,
    }
}

pub struct Boxes;

impl Generator for Boxes {
    type Params = BoxesParams;
    // The root prim, and the stem of the Python names: `BoxesParams`, `BoxesCfg`.
    const NAME: &'static str = "Boxes";
    // The Python package the extension module sits in.
    const PACKAGE: &'static str = "boxlab";

    fn plugin(app: &mut App) {
        app.init_resource::<BoxParams>().add_systems(
            PreUpdate,
            build.run_if(resource_changed::<BoxParams>).in_set(Build),
        );
    }
}

/// Marks the row, so a rebuild can take the last one down.
#[derive(Component)]
struct Row;

fn build(
    mut commands: Commands,
    params: Res<BoxParams>,
    root: Res<PrimRoot>,
    mut library: Library,
    standing: Query<Entity, With<Row>>,
) {
    for row in &standing {
        commands.entity(row).despawn();
    }
    library.clear("Box");
    let surface = Surface {
        color: [0.5, 0.35, 0.2],
        roughness: 0.8,
        reflectance: 0.4,
        translucency: 0.0,
        thickness: 0.0,
        double_sided: false,
    };
    // One mesh in the library; every box is a reference to it.
    let geometry = library.part("Box", 0, box_mesh(params.size).to_mesh(), surface);
    let row = commands
        .spawn((Row, Name::new("Row"), Transform::IDENTITY, Visibility::default(), ChildOf(root.0)))
        .id();
    for i in 0..params.count {
        commands.spawn((
            Name::new(format!("Box_{i:02}")),
            Order(i as u64),
            Transform::from_xyz(i as f32 * params.size * 1.5, 0.0, params.size / 2.0),
            Visibility::default(),
            geometry.clone(),
            ChildOf(row),
        ));
    }
}

// Headless: one build cycle, and the document the USD builder takes.
let doc = misina_lab::generate::scene::<Boxes>(&BoxesParams::default()).unwrap();
assert_eq!(doc.root.name, "Boxes");
assert_eq!(doc.parts.len(), 1);
```

Everything else follows from those declarations. The panel offers `count` and
`size` as sliders with the doc comments as tooltips, the viewer's copy button
emits `BoxesCfg(boxes=BoxCfg(count=5))`, and under the `python` feature the
same structs are the classes `boxlab.BoxesParams` and `boxlab.BoxParams`,
whose `write_usd` runs this same build headlessly.

## Reading on

Each page defines one idea, shows it, and ends with where it lives in the
source. The module docs are each module's contract;
`cargo doc --open -p misina-lab` renders them, and these pages say how the
modules fit.

| Page | What it answers |
| --- | --- |
| [generators.md](docs/generators.md) | what a generator declares, and every surface the framework builds from it |
| [elements.md](docs/elements.md) | how a scene is built: layers, configs, the mesh library, and the rules a layer follows |
| [quantization.md](docs/quantization.md) | how a population of configs becomes a budget of meshes, and how to write a metric |
| [randomness.md](docs/randomness.md) | one seed, many streams: how a scene stays reproducible while every part varies |
| [export.md](docs/export.md) | coordinates, the scene document, and what the USD builder makes of it |
| [isaac-lab.md](docs/isaac-lab.md) | spawning a generated scene: the cfg base, the cache, rods, and cutting them |
| [editing-parameters.md](docs/editing-parameters.md) | how to add or change a parameter |

## Development

`cargo test -p misina-lab` runs the crate's tests against
`testing::fixture::Boxes`, the row above with a second fragment; what a
generator's own tests get from this crate is in `src/testing.rs`.
`--features python` compiles the PyO3 side, which is off by default. The full
check list is the repository's, in
[docs/development.md](../../docs/development.md). How a page under `docs/`
is written and kept current is [docs/AGENTS.md](docs/AGENTS.md).
