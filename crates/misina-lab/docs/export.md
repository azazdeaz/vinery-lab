# Export

The scene is built in Bevy as ordinary entities and leaves the crate as a
**scene document**: plain JSON saying what geometry exists, where it goes
and what references what. Python turns the document into a USD stage. Rust
owns the *scene*; Python owns *USD*: prim types, schemas, composition arcs,
stage metadata. `scene::doc::SceneDoc` is the whole contract between them,
and the builder's module docstring is where every USD rule is written down.

```text
Bevy entities (Transform, Mesh3d, Name, UsdReference)
      │
      ▼  src/scene/export.rs
SceneDoc, as JSON
      │
      ▼  python/misina-lab/misina_lab/usd/build.py
a .usd stage
```

There is no intermediate representation on the Rust side and no second
scene graph: the entities the viewer draws are the entities the export
walks, so there is no preview shape and export shape to keep in step.

## Coordinates

The scene is authored **Z-up, in meters**, REP-103, which is what both Isaac
Lab and ROS use. `upAxis` is root-layer-only metadata that does not compose
through references, so a consumer cannot correct for a stage that gets it
wrong, and USD's unauthored default is Y-up: silence is not neutral here, it
is wrong.

Bevy's renderer is Y-up, so the viewer's correction is a single parent
entity above the scene root carrying `scene::z_up_to_y_up()`. The export
walk starts *below* it, so the emitted document is Z-up native and no
geometry module has to know.

## The walk

`scene::export` walks named entities from the `UsdRoot` down. Unnamed
entities and their subtrees are skipped, which is how the camera, the
lights, the UI and the Y-up correction stay out of the file without opting
out. Siblings are emitted in name order rather than spawn order, so a layer
that rebuilt itself last does not move in the document. A prim carrying
`UsdReference` may not have children; the exporter errors rather than
emitting something USD would silently drop.

The document's conventions are listed in `doc.rs`: every mesh is a triangle
list; rotations are quaternions in xyzw order, never Euler triples; a node
with a `reference` draws the part of that name and has no children; a part's
surface response travels with it and becomes a bound material; colliders are
static.

## The builder

`misina_lab.usd.build_usd(doc, path)` turns the document into a stage, and
`python -m misina_lab.usd scene.json scene.usd` does it from the command
line, which is what the viewer's save key hands you. The builder's docstring
holds every USD rule the project depends on, each of which fails *silently*
if dropped: stage metadata, the parts library as a `class` inside the
default prim, `subdivisionScheme = "none"`, `extent`, `displayColor` at
`constant` interpolation, one MDL material per drawn prim as a child of it,
and the one that is easiest to get wrong: a part is an `Xform` *wrapping*
its `Mesh` rather than a bare `Mesh`. A USD instance shares its
*descendants* through a prototype while the instance prim's own attributes
stay on the instance, so referencing a bare mesh and marking it instanceable
yields an empty prototype and a full copy of the points on every instance:
valid, drawn correctly, and with none of the sharing that was the point.

## Colliders

The document says *what* collides and the builder applies the schemas. A
part can be its own collider through `Library::collide`, the ground at
`physics:approximation = "none"`; anything a proxy describes better gets a
`Capsule` prim from `scene::capsule`, which needs no cooking and is the only
round shape PhysX has natively. Nothing is a rigid body, so every collider is
static, which is what makes an exact triangle mesh legal for the ground. A
part carrying a collider is referenced non-instanceable: a collider inside a
prototype is reachable only through an instance proxy, and the ground has
one instance, so it gives up nothing.

A `Cable` is the one thing that moves: a flexible organ authored as a curve,
which a solver that understands one turns into bodies of its own
([isaac-lab.md](isaac-lab.md#rods)). It replaces the mesh rather than
standing in for it, so it is geometry and collider at once.

## Reading the ground back

A consumer that places anything on the terrain after the fact, a robot above
all, needs the height the generator placed everything against.
`misina_lab.usd.Ground` interpolates the same grid, read back off the stage.
It finds the surface by its height-field attribute rather than by name, that
being the one part a scene has at most one of.

## Where it lives

- [`src/scene/doc.rs`](../src/scene/doc.rs): the document, and the conventions the builder relies on
- [`src/scene/export.rs`](../src/scene/export.rs): the walk
- [`src/scene/mod.rs`](../src/scene/mod.rs): the export directives; `UsdRoot`, `UsdReference`, `capsule`, `cable`, `z_up_to_y_up`
- [`misina_lab/usd/build.py`](../../../python/misina-lab/misina_lab/usd/build.py): the builder, and every USD rule
- [`misina_lab/usd/ground.py`](../../../python/misina-lab/misina_lab/usd/ground.py): `Ground`
