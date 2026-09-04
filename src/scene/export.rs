//! Walking the scene graph into a [`SceneDoc`].
//!
//! One pass over every named entity, then a recursion down `Children` from the
//! [`UsdRoot`]. Entities without a [`Name`] have no prim path and are skipped,
//! along with everything below them — which is what keeps the viewer's camera,
//! lights and UI out of the export without any of them having to opt out.

use anyhow::{anyhow, bail};
use bevy::mesh::{PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use super::doc::{Capsule, FORMAT, Node, PartEntry, SceneDoc, Xform};
use super::{Collider, Part, Prototypes, UsdReference, UsdRoot, UsdType};

/// The scene's coordinate convention. Authored natively rather than corrected
/// downstream: `upAxis` and `metersPerUnit` are root-layer-only stage metadata
/// that does not compose through references, and USD defaults to Y-up when
/// `upAxis` is unauthored, so an unset one is not neutral but wrong.
const UP_AXIS: &str = "Z";
const METERS_PER_UNIT: f64 = 1.0;

/// Builds the export document from the current world.
///
/// Takes `&mut World` because it queries several component combinations and
/// reads two resources; nothing is mutated. Both entry points call it from an
/// exclusive context — the headless generator after its single `update()`, and
/// the viewer's save key.
pub fn scene_doc(world: &mut World) -> anyhow::Result<SceneDoc> {
    let parts = part_entries(world)?;
    let solid: HashSet<&str> = parts
        .iter()
        .filter(|part| part.collision.is_some())
        .map(|part| part.name.as_str())
        .collect();
    let root = root_entity(world)?;
    let prims = collect_prims(world);
    let root_node = build_node(root, &prims, &solid)?
        .ok_or_else(|| anyhow!("the `UsdRoot` entity has no `Name`, so it has no prim path"))?;

    Ok(SceneDoc {
        format: FORMAT,
        up_axis: UP_AXIS.to_string(),
        meters_per_unit: METERS_PER_UNIT,
        parts,
        root: root_node,
    })
}

/// Serializes the document the way the Python builder expects to receive it.
pub fn scene_json(world: &mut World) -> anyhow::Result<String> {
    Ok(serde_json::to_string(&scene_doc(world)?)?)
}

fn root_entity(world: &mut World) -> anyhow::Result<Entity> {
    let mut query = world.query_filtered::<Entity, With<UsdRoot>>();
    let mut found = query.iter(world);
    let root = found
        .next()
        .ok_or_else(|| anyhow!("no `UsdRoot` entity — nothing to export"))?;
    if found.next().is_some() {
        bail!("more than one `UsdRoot` entity; the export needs a single scene root");
    }
    Ok(root)
}

// ─── The mesh library ───────────────────────────────────────────────

fn part_entries(world: &World) -> anyhow::Result<Vec<PartEntry>> {
    let prototypes = world.resource::<Prototypes>();
    let meshes = world.resource::<Assets<Mesh>>();

    // `Prototypes` iterates in name order, so the library — and with it the
    // whole document — comes out identical across runs without a sort.
    prototypes
        .iter()
        .map(|(name, part)| {
            let mesh = meshes.get(&part.mesh).ok_or_else(|| {
                anyhow!("part `{name}` points at a mesh that is not in `Assets<Mesh>`")
            })?;
            part_entry(name, mesh, part)
        })
        .collect()
}

fn part_entry(name: &str, mesh: &Mesh, part: &Part) -> anyhow::Result<PartEntry> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        bail!(
            "part `{name}` is {:?}, and the document carries triangle lists only",
            mesh.primitive_topology()
        );
    }

    let points = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(VertexAttributeValues::as_float3)
        .ok_or_else(|| anyhow!("part `{name}` has no float3 positions"))?
        .to_vec();

    // Both index widths, because a small mesh built through Bevy's own
    // primitives may come back `U16` while a generated one is `U32`.
    let indices: Vec<u32> = mesh
        .indices()
        .ok_or_else(|| anyhow!("part `{name}` is not indexed"))?
        .iter()
        .map(|i| i as u32)
        .collect();
    if !indices.len().is_multiple_of(3) {
        bail!(
            "part `{name}` has {} indices, which is not a whole number of triangles",
            indices.len()
        );
    }
    if let Some(out) = indices.iter().find(|i| **i as usize >= points.len()) {
        bail!(
            "part `{name}` indexes point {out}, past the {} it has",
            points.len()
        );
    }

    let normals = mesh
        .attribute(Mesh::ATTRIBUTE_NORMAL)
        .and_then(VertexAttributeValues::as_float3)
        .map(<[[f32; 3]]>::to_vec);

    let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(uvs)) => Some(uvs.clone()),
        _ => None,
    };

    Ok(PartEntry {
        name: name.to_string(),
        points,
        indices,
        normals,
        uvs,
        display_color: part.color,
        double_sided: part.double_sided,
        collision: part.collision.map(str::to_string),
    })
}

// ─── The prim tree ──────────────────────────────────────────────────

/// One entity's worth of prim, before its children have been resolved.
struct Prim {
    name: String,
    type_name: String,
    xform: Option<Xform>,
    reference: Option<String>,
    collider: Option<Capsule>,
    children: Vec<Entity>,
}

fn collect_prims(world: &mut World) -> HashMap<Entity, Prim> {
    let mut query = world.query::<(
        Entity,
        &Name,
        Option<&Transform>,
        Option<&UsdType>,
        Option<&UsdReference>,
        Option<&Collider>,
        Option<&Children>,
    )>();

    query
        .iter(world)
        .map(|(entity, name, transform, prim_type, reference, collider, children)| {
            let prim = Prim {
                name: name.as_str().to_string(),
                type_name: prim_type.map_or("Xform", |t| t.0).to_string(),
                xform: transform.and_then(xform_of),
                reference: reference.map(|r| r.0.clone()),
                collider: collider.map(|c| c.0),
                children: children.map(|c| c.to_vec()).unwrap_or_default(),
            };
            (entity, prim)
        })
        .collect()
}

/// The op stack for a transform, or `None` when there is nothing to author.
///
/// Exact comparison against the identity rather than an epsilon: an entity
/// that never placed itself holds exactly `Transform::IDENTITY`, and one that
/// did should author what it computed even when the result rounds to nothing.
fn xform_of(transform: &Transform) -> Option<Xform> {
    (*transform != Transform::IDENTITY).then(|| Xform {
        translate: transform.translation.to_array(),
        // `Quat::to_array` is xyzw, which is the order the document declares.
        orient: transform.rotation.to_array(),
        scale: transform.scale.to_array(),
    })
}

/// One entity's prim and everything below it. `solid` names the parts that are
/// their own collider, which is what decides whether a reference to one may be
/// instanced.
fn build_node(
    entity: Entity,
    prims: &HashMap<Entity, Prim>,
    solid: &HashSet<&str>,
) -> anyhow::Result<Option<Node>> {
    let Some(prim) = prims.get(&entity) else {
        // Unnamed: no prim path, so neither it nor its subtree is exported.
        return Ok(None);
    };

    let children: Vec<Node> = prim
        .children
        .iter()
        .filter_map(|child| build_node(*child, prims, solid).transpose())
        .collect::<anyhow::Result<_>>()?;

    // The invariant `instanceable` rests on. An instanceable prim's
    // descendants are not addressable, so a referencing prim that grew
    // children would silently lose them — enforced here rather than left to
    // whoever spawns the next organ to remember.
    if prim.reference.is_some() && !children.is_empty() {
        bail!(
            "prim `{}` both references `{}` and has {} children; geometry prims \
             must be leaves",
            prim.name,
            prim.reference.as_deref().unwrap_or_default(),
            children.len()
        );
    }

    Ok(Some(Node {
        name: prim.name.clone(),
        type_name: prim.type_name.clone(),
        xform: prim.xform,
        // A part that is its own collider is referenced non-instanceable: the
        // collision schema lives inside the part, and inside a prototype it
        // would be reachable only through an instance proxy. It costs nothing
        // — the ground is the only such part, and it has one instance.
        instanceable: prim
            .reference
            .as_deref()
            .is_some_and(|name| !solid.contains(name)),
        reference: prim.reference.clone(),
        collider: prim.collider,
        children,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::capsule;
    use bevy::mesh::Indices;

    /// A world with the resources the exporter reads and a single named root.
    fn world() -> World {
        let mut world = World::new();
        world.insert_resource(Prototypes::default());
        world.insert_resource(Assets::<Mesh>::default());
        world
    }

    fn triangle() -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        )
        .with_inserted_indices(Indices::U32(vec![0, 1, 2]))
    }

    /// Registers `triangle()` as one part and returns its name.
    fn add_part(world: &mut World, prefix: &str, index: usize) -> String {
        let handle = world.resource_mut::<Assets<Mesh>>().add(triangle());
        world.resource_mut::<Prototypes>().insert(
            prefix,
            index,
            Part {
                mesh: handle,
                color: [0.1, 0.2, 0.3],
                roughness: 0.9,
                ior: 1.5,
                double_sided: true,
                collision: None,
            },
        )
    }

    fn root(world: &mut World) -> Entity {
        world.spawn((UsdRoot, Name::new("Vineyard"))).id()
    }

    #[test]
    fn the_root_becomes_the_documents_root_prim() {
        let mut world = world();
        root(&mut world);

        let doc = scene_doc(&mut world).unwrap();
        assert_eq!(doc.root.name, "Vineyard");
        assert_eq!(doc.root.type_name, "Xform");
        assert_eq!(doc.up_axis, "Z");
        assert!(doc.root.xform.is_none(), "an identity transform is not authored");
    }

    #[test]
    fn the_hierarchy_becomes_the_prim_tree() {
        let mut world = world();
        let root = root(&mut world);
        let row = world
            .spawn((Name::new("Row_00"), UsdType("Scope"), ChildOf(root)))
            .id();
        world.spawn((
            Name::new("Vine_000"),
            Transform::from_xyz(1.0, 2.0, 3.0),
            ChildOf(row),
        ));

        let doc = scene_doc(&mut world).unwrap();
        let row = &doc.root.children[0];
        assert_eq!((row.name.as_str(), row.type_name.as_str()), ("Row_00", "Scope"));

        let vine = &row.children[0];
        assert_eq!(vine.name, "Vine_000");
        assert_eq!(vine.xform.unwrap().translate, [1.0, 2.0, 3.0]);
    }

    /// A referencing prim is the geometry, so it is what carries
    /// `instanceable` — and that is what makes tens of thousands of them
    /// affordable.
    #[test]
    fn a_referencing_prim_is_instanceable_and_carries_no_points() {
        let mut world = world();
        let root = root(&mut world);
        let name = add_part(&mut world, "Leaf", 2);
        world.spawn((Name::new("Leaf_00"), UsdReference(name), ChildOf(root)));

        let doc = scene_doc(&mut world).unwrap();
        let leaf = &doc.root.children[0];
        assert_eq!(leaf.reference.as_deref(), Some("Leaf_2"));
        assert!(leaf.instanceable);
        assert!(leaf.children.is_empty());

        assert_eq!(doc.parts.len(), 1);
        assert_eq!(doc.parts[0].points.len(), 3);
        assert_eq!(doc.parts[0].indices, [0, 1, 2]);
        assert!(doc.parts[0].double_sided);
    }

    /// A collision proxy is a prim in its own right — no geometry, no
    /// reference, and nothing to instance. It is the shape a physics engine
    /// reads instead of the mesh beside it.
    #[test]
    fn a_collider_becomes_a_capsule_prim() {
        let mut world = world();
        let root = root(&mut world);
        world.spawn((Name::new("Collision"), capsule(0.04, 0.0, 1.8), ChildOf(root)));
        // Shorter than its own caps: a sphere, rather than a capsule with a
        // negative side, which USD would take without a word.
        world.spawn((Name::new("Stub"), capsule(0.5, 0.0, 0.2), ChildOf(root)));

        let doc = scene_doc(&mut world).unwrap();
        let node = &doc.root.children[0];

        assert_eq!(node.type_name, "Capsule");
        assert_eq!(
            node.collider,
            Some(Capsule {
                radius: 0.04,
                height: 1.8 - 2.0 * 0.04
            })
        );
        assert_eq!(node.xform.unwrap().translate, [0.0, 0.0, 0.9]);
        assert!(node.reference.is_none() && !node.instanceable);

        assert_eq!(doc.root.children[1].collider.unwrap().height, 0.0);
    }

    /// The invariant `instanceable` rests on: USD would keep the prim and
    /// silently drop its descendants, so the export refuses instead.
    #[test]
    fn a_referencing_prim_may_not_have_children() {
        let mut world = world();
        let root = root(&mut world);
        let name = add_part(&mut world, "Shoot", 0);
        let stem = world
            .spawn((Name::new("Stem"), UsdReference(name), ChildOf(root)))
            .id();
        world.spawn((Name::new("Leaf_00"), ChildOf(stem)));

        let err = scene_doc(&mut world).unwrap_err().to_string();
        assert!(err.contains("must be leaves"), "got: {err}");
    }

    /// The viewer's camera, lights and UI are unnamed, and neither they nor
    /// anything under them may reach the document.
    #[test]
    fn unnamed_entities_and_their_subtrees_are_skipped() {
        let mut world = world();
        let root = root(&mut world);
        let anonymous = world.spawn(ChildOf(root)).id();
        world.spawn((Name::new("Hidden"), ChildOf(anonymous)));
        world.spawn((Name::new("Kept"), ChildOf(root)));

        let doc = scene_doc(&mut world).unwrap();
        let names: Vec<&str> = doc.root.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Kept"]);
    }

    /// The library is the tuning knob's output, so it has to be reproducible:
    /// same world, same bytes.
    #[test]
    fn the_document_is_reproducible() {
        let mut world = world();
        let root = root(&mut world);
        for i in [3, 0, 2, 1] {
            let name = add_part(&mut world, "Leaf", i);
            world.spawn((Name::new(format!("Leaf_{i:02}")), UsdReference(name), ChildOf(root)));
        }

        let once = scene_json(&mut world).unwrap();
        let twice = scene_json(&mut world).unwrap();
        assert_eq!(once, twice);

        let doc = scene_doc(&mut world).unwrap();
        let names: Vec<&str> = doc.parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Leaf_0", "Leaf_1", "Leaf_2", "Leaf_3"]);
    }

    #[test]
    fn a_world_with_no_root_cannot_be_exported() {
        let err = scene_doc(&mut world()).unwrap_err().to_string();
        assert!(err.contains("no `UsdRoot`"), "got: {err}");
    }

    /// A part whose mesh was dropped from `Assets<Mesh>` would export as a
    /// reference to geometry that never gets written.
    #[test]
    fn a_part_without_its_mesh_is_an_error() {
        let mut world = world();
        root(&mut world);
        world.resource_mut::<Prototypes>().insert(
            "Leaf",
            0,
            Part {
                mesh: Handle::default(),
                color: [0.0; 3],
                roughness: 0.5,
                ior: 1.5,
                double_sided: false,
                collision: None,
            },
        );

        let err = scene_doc(&mut world).unwrap_err().to_string();
        assert!(err.contains("not in `Assets<Mesh>`"), "got: {err}");
    }
}
