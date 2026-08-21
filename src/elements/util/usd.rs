//! USD authoring helpers shared by elements.
//!
//! Thin wrappers over `openusd`'s typed schemas, which are preferred over
//! `usd_bevy::authoring::set_attribute` for geometry: the typed `create_*_attr`
//! helpers author `custom = false`, so the output declares schema attributes
//! rather than `custom point3f[] points`.

use openusd::schemas::geom::{Mesh, PointBased};
use openusd::sdf::{self, Value};
use openusd::usd::Stage;

/// A polygonal mesh in USD's layout, ready to author.
///
/// `face_vertex_counts` gives the vertex count of each face in order;
/// `face_vertex_indices` is the flat concatenation of those faces' indices
/// into `points`.
#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub points: Vec<[f32; 3]>,
    pub face_vertex_counts: Vec<i32>,
    pub face_vertex_indices: Vec<i32>,
}

/// Authors `mesh` as a `UsdGeom.Mesh` at `path`.
///
/// Normals are left unauthored on purpose: `usd_bevy` falls back to smooth
/// normals, which is right for organic geometry and — for a mesh whose faces
/// don't share vertices — degenerates to flat shading anyway.
pub fn author_mesh(stage: &Stage, path: &str, mesh: &MeshData) -> anyhow::Result<()> {
    let prim = Mesh::define(stage, openusd::sdf::path(path)?)?;
    prim.create_points_attr()?.set(Value::Vec3fVec(
        mesh.points.iter().copied().map(Into::into).collect(),
    ))?;
    prim.create_face_vertex_counts_attr()?
        .set(Value::IntVec(mesh.face_vertex_counts.clone()))?;
    prim.create_face_vertex_indices_attr()?
        .set(Value::IntVec(mesh.face_vertex_indices.clone()))?;
    Ok(())
}

/// Makes the prim at `path` an internal reference to `target`: the whole
/// subtree under `target` composes in under `path`, transformed by whatever
/// `path` itself authors.
///
/// This is how one element nests another's subtree without copying geometry.
/// `target` must not be an ancestor of `path` — a reference to an ancestor is
/// a composition cycle and resolves to nothing.
///
/// Goes through `set_metadata` rather than a typed API because `openusd` has
/// no `UsdReferences` equivalent yet; `references` is a list op, and an
/// explicit one replaces whatever a previous author pass left behind.
pub fn reference_prim(stage: &Stage, path: &str, target: &str) -> anyhow::Result<()> {
    let reference = sdf::Reference {
        prim_path: sdf::path(target)?,
        ..Default::default()
    };
    stage.prim(sdf::path(path)?).set_metadata(
        sdf::FieldKey::References.as_str(),
        Value::ReferenceListOp(sdf::ReferenceListOp::explicit([reference])),
    )?;
    Ok(())
}

/// Concatenates meshes into one, offsetting each part's indices past the
/// points already emitted.
///
/// Lets an element build a shape out of several independent pieces and still
/// author it as a single `Mesh`. A vine is a dozen separate tubes but one
/// prototype, and prototypes are instanced hundreds of times — prim count is
/// what drives projection cost, so the pieces are merged rather than authored
/// side by side.
pub fn merge_meshes(parts: &[MeshData]) -> MeshData {
    let mut merged = MeshData::default();
    for part in parts {
        let offset = merged.points.len() as i32;
        merged.points.extend_from_slice(&part.points);
        merged
            .face_vertex_counts
            .extend_from_slice(&part.face_vertex_counts);
        merged
            .face_vertex_indices
            .extend(part.face_vertex_indices.iter().map(|i| i + offset));
    }
    merged
}

/// An axis-aligned box of edge length `size`, centered on the origin.
///
/// Emits four unshared vertices per face rather than eight shared corners, so
/// the smooth-normal fallback averages only within a face and the box shades
/// flat.
pub fn box_mesh(size: f32) -> MeshData {
    let h = size / 2.0;
    #[rustfmt::skip]
    let points = vec![
        // +X                                    -X
        [ h, -h, -h], [ h,  h, -h], [ h,  h,  h], [ h, -h,  h],
        [-h, -h, -h], [-h, -h,  h], [-h,  h,  h], [-h,  h, -h],
        // +Y                                    -Y
        [-h,  h, -h], [-h,  h,  h], [ h,  h,  h], [ h,  h, -h],
        [-h, -h, -h], [ h, -h, -h], [ h, -h,  h], [-h, -h,  h],
        // +Z                                    -Z
        [-h, -h,  h], [ h, -h,  h], [ h,  h,  h], [-h,  h,  h],
        [-h, -h, -h], [-h,  h, -h], [ h,  h, -h], [ h, -h, -h],
    ];
    MeshData {
        points,
        face_vertex_counts: vec![4; 6],
        face_vertex_indices: (0..24).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_mesh_is_a_well_formed_hexahedron() {
        let m = box_mesh(2.0);
        assert_eq!(m.points.len(), 24, "four unshared vertices per face");
        assert_eq!(m.face_vertex_counts.iter().sum::<i32>(), 24);
        assert_eq!(m.face_vertex_indices.len(), 24);
        assert!(
            m.points.iter().all(|p| p.iter().all(|c| c.abs() == 1.0)),
            "size 2.0 puts every corner on ±1"
        );
    }

    /// Every face must wind counter-clockwise seen from outside, or the box
    /// renders inside-out under USD's default right-handed orientation.
    #[test]
    fn box_mesh_faces_wind_outward() {
        let m = box_mesh(2.0);
        for face in 0..6 {
            let [a, b, c] = [0, 1, 2].map(|i| bevy::math::Vec3::from(m.points[face * 4 + i]));
            let normal = (b - a).cross(c - a);
            let centroid = (0..4)
                .map(|i| bevy::math::Vec3::from(m.points[face * 4 + i]))
                .sum::<bevy::math::Vec3>()
                / 4.0;
            assert!(
                normal.dot(centroid) > 0.0,
                "face {face} normal points away from the center"
            );
        }
    }

    #[test]
    fn merge_meshes_offsets_each_parts_indices() {
        let merged = merge_meshes(&[box_mesh(1.0), box_mesh(2.0)]);
        assert_eq!(merged.points.len(), 48);
        assert_eq!(merged.face_vertex_counts, vec![4; 12]);
        // The second box's faces must index into its own copy of the points,
        // which starts where the first box's ended.
        assert_eq!(merged.face_vertex_indices[..24], (0..24).collect::<Vec<_>>());
        assert_eq!(merged.face_vertex_indices[24..], (24..48).collect::<Vec<_>>());
    }

    #[test]
    fn merging_nothing_yields_an_empty_mesh() {
        let merged = merge_meshes(&[]);
        assert!(merged.points.is_empty() && merged.face_vertex_counts.is_empty());
    }

    /// The referenced subtree must show up under the referencing prim in the
    /// *composed* stage — that's the whole point of nesting by reference, and
    /// it's what the viewer traverses.
    #[test]
    fn reference_prim_composes_the_target_subtree() {
        let stage = openusd::usd::Stage::builder().in_memory("ref.usda").unwrap();
        author_mesh(&stage, "/parts/Group/Box", &box_mesh(1.0)).unwrap();
        openusd::schemas::geom::Xform::define(&stage, sdf::path("/World/Nested").unwrap()).unwrap();
        reference_prim(&stage, "/World/Nested", "/parts/Group").unwrap();

        assert!(
            usd_bevy::authoring::prim_exists(&stage, "/World/Nested/Box"),
            "the referenced mesh composes in under the referencing prim"
        );
    }

    #[test]
    fn author_mesh_writes_schema_attributes() {
        let stage = openusd::usd::Stage::builder().in_memory("mesh.usda").unwrap();
        author_mesh(&stage, "/Box", &box_mesh(1.0)).unwrap();

        let usda = stage.root_layer().export_to_string().unwrap();
        assert!(usda.contains("def Mesh \"Box\""), "got:\n{usda}");
        assert!(
            !usda.contains("custom point3f[] points"),
            "points must be a schema attribute, not custom; got:\n{usda}"
        );
    }
}
