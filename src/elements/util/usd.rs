//! USD authoring helpers shared by elements.
//!
//! Thin wrappers over `openusd`'s typed schemas, which are preferred over
//! `usd_bevy::authoring::set_attribute` for geometry: the typed `create_*_attr`
//! helpers author `custom = false`, so the output declares schema attributes
//! rather than `custom point3f[] points`.

use std::f32::consts::TAU;

use openusd::gf;
use openusd::schemas::geom::{
    Boundable, Gprim, Interpolation, Mesh, PointBased, SubdivisionScheme,
};
use openusd::sdf::{self, Value};
use openusd::usd::Stage;

/// The metadata key that says how many values a primvar carries per element.
/// `openusd` has no `UsdGeomPrimvar` wrapper yet, so it goes on by hand.
const INTERPOLATION: &str = "interpolation";

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

/// Authors `mesh` as a `UsdGeom.Mesh` at `path`, and hands the prim back for
/// the callers that have something else to say about it.
///
/// Normals are left unauthored on purpose: `usd_bevy` falls back to smooth
/// normals, which is right for organic geometry and — for a mesh whose faces
/// don't share vertices — degenerates to flat shading anyway.
///
/// `subdivisionScheme` and `extent` are *not* left to the schema defaults,
/// and both matter:
///
/// - USD's default scheme is `catmullClark`, which declares every mesh here a
///   subdivision *cage* rather than the surface it is. A renderer that honours
///   that — Isaac, usdview, anything on Hydra — rounds the drawn teeth off a
///   leaf's margin and sands away the [`Bark`](super::strand) ridges that
///   `strand` exists to produce. The viewer has been hiding this, because
///   `usd_bevy` defaults the same way we want it set.
/// - `extent` is what a consumer frustum-culls against. Unauthored, it has to
///   walk every point of every prim to find one; we already hold the points.
pub fn author_mesh(stage: &Stage, path: &str, mesh: &MeshData) -> anyhow::Result<Mesh> {
    let prim = Mesh::define(stage, openusd::sdf::path(path)?)?;
    prim.create_points_attr()?.set(Value::Vec3fVec(
        mesh.points.iter().copied().map(Into::into).collect(),
    ))?;
    prim.create_face_vertex_counts_attr()?
        .set(Value::IntVec(mesh.face_vertex_counts.clone()))?;
    prim.create_face_vertex_indices_attr()?
        .set(Value::IntVec(mesh.face_vertex_indices.clone()))?;
    prim.create_subdivision_scheme_attr()?
        .set(SubdivisionScheme::None)?;
    if let Some((min, max)) = extent(mesh) {
        prim.create_extent_attr()?
            .set(Value::Vec3fVec(vec![min, max]))?;
    }
    Ok(prim)
}

/// Authors one flat colour across the whole of `prim`.
///
/// A single-element `color3f[]` at `constant` interpolation — the cheapest
/// thing that says "this mesh is this colour", and what every prototype
/// carries until something wants a gradient across it.
///
/// This is the *only* channel that reaches both consumers. `usd_bevy` reads
/// `displayColor` into a vertex-colour attribute but bakes instanced
/// prototypes with a default material, so a bound material is invisible in the
/// viewer; and a bound material's `diffuseColor` reads this back out through a
/// primvar reader (see [`material`](super::material)), so the two agree.
pub fn set_display_color(prim: &Mesh, color: [f32; 3]) -> anyhow::Result<()> {
    prim.create_display_color_attr()?
        .set(Value::Vec3fVec(vec![color.into()]))?
        .set_metadata(
            INTERPOLATION,
            Value::token(Interpolation::Constant.as_token()),
        )?;
    Ok(())
}

/// The corners of `mesh`'s axis-aligned bounding box, or `None` when it has no
/// points to bound.
fn extent(mesh: &MeshData) -> Option<(gf::Vec3f, gf::Vec3f)> {
    let mut points = mesh.points.iter();
    let first = *points.next()?;
    let (min, max) = points.fold((first, first), |(min, max), p| {
        (
            [0, 1, 2].map(|i| min[i].min(p[i])),
            [0, 1, 2].map(|i| max[i].max(p[i])),
        )
    });
    Some((min.into(), max.into()))
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

/// A closed cylinder about the +Z axis, `sides` around: base on the origin,
/// top at `height`.
///
/// The barrel's two rings are *shared* between neighbouring quads, so the
/// smooth-normal fallback rounds the silhouette and an eight-sided post reads
/// as round rather than as a prism. The caps carry their own copy of each
/// ring, so that averaging stops at the rim instead of bevelling it.
///
/// Straight, untapered and unjittered: the variety a vineyard's posts show is
/// in how they were driven rather than in their shape, which is per placement
/// and needs no geometry of its own. Anything that bends wants
/// [`strand`](super::strand) instead — this is the cheap case, one ring at
/// each end and no curve to fit.
pub fn cylinder_mesh(radius: f32, height: f32, sides: usize) -> MeshData {
    let sides = sides.max(3);
    let ring = |z: f32| -> Vec<[f32; 3]> {
        (0..sides)
            .map(|i| {
                let angle = TAU * i as f32 / sides as f32;
                [radius * angle.cos(), radius * angle.sin(), z]
            })
            .collect()
    };
    // Barrel rings first, then a private copy of each for its cap.
    let (bottom, top) = (ring(0.0), ring(height));
    let points = [bottom.clone(), top.clone(), bottom, top].concat();

    let n = sides as i32;
    let mut face_vertex_counts = vec![4; sides];
    let mut face_vertex_indices: Vec<i32> = (0..n)
        .flat_map(|i| {
            let next = (i + 1) % n;
            // Counter-clockwise seen from outside: round the bottom ring in
            // the direction the angle increases, then back along the top.
            [i, next, n + next, n + i]
        })
        .collect();

    // The caps, one n-gon each. The bottom is wound in reverse because it is
    // looked at from below.
    face_vertex_counts.push(n);
    face_vertex_indices.extend((0..n).rev().map(|i| 2 * n + i));
    face_vertex_counts.push(n);
    face_vertex_indices.extend((0..n).map(|i| 3 * n + i));

    MeshData {
        points,
        face_vertex_counts,
        face_vertex_indices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec3;

    use crate::elements::util::testing::{bounds, face_centroid, face_normal, faces};

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
        for (i, face) in faces(&m).enumerate() {
            assert!(
                face_normal(&m, face).dot(face_centroid(&m, face)) > 0.0,
                "face {i} normal points away from the center"
            );
        }
    }

    /// A cylinder has to be closed and correctly wound whichever way it is
    /// looked at: the barrel outward from the axis, the caps along it. A ring
    /// wound the other way turns the post inside out, which under USD's
    /// default `rightHanded` orientation is a black tube with a bright hole.
    #[test]
    fn cylinder_mesh_is_a_closed_tube_wound_outward() {
        let (radius, height, sides) = (0.05, 1.8, 8);
        let m = cylinder_mesh(radius, height, sides);

        assert_eq!(m.points.len(), sides * 4, "a ring each for barrel and cap");
        assert_eq!(
            m.face_vertex_counts.len(),
            sides + 2,
            "barrel plus two caps"
        );
        assert_eq!(
            m.face_vertex_counts.iter().sum::<i32>() as usize,
            m.face_vertex_indices.len()
        );
        assert!(
            m.face_vertex_indices
                .iter()
                .all(|i| (*i as usize) < m.points.len())
        );

        let (z0, z1) = bounds(&m, 2);
        assert_eq!((z0, z1), (0.0, height), "it stands on its base");
        for p in &m.points {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            assert!((r - radius).abs() < 1e-6, "{p:?} is off the barrel");
        }

        for (i, face) in faces(&m).enumerate() {
            let normal = face_normal(&m, face).normalize();
            let centroid = face_centroid(&m, face);
            match i {
                // The barrel: away from the axis, and level with it.
                i if i < sides => {
                    let outward = Vec3::new(centroid.x, centroid.y, 0.0).normalize();
                    assert!(normal.dot(outward) > 0.99, "side {i} faces {normal:?}");
                }
                // Then the bottom cap, then the top.
                i if i == sides => assert!(normal.z < -0.99, "the base faces down"),
                _ => assert!(normal.z > 0.99, "the top faces up"),
            }
        }
    }

    /// The floor exists because two sides have no inside: the "barrel" would
    /// be two coincident quads and both caps degenerate.
    #[test]
    fn a_cylinder_is_never_asked_for_fewer_than_three_sides() {
        assert_eq!(cylinder_mesh(0.05, 1.0, 1).points.len(), 3 * 4);
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
        author_mesh(&stage, "/Vineyard/parts/Group/Box", &box_mesh(1.0)).unwrap();
        openusd::schemas::geom::Xform::define(&stage, sdf::path("/World/Nested").unwrap()).unwrap();
        reference_prim(&stage, "/World/Nested", "/Vineyard/parts/Group").unwrap();

        assert!(
            usd_bevy::authoring::prim_exists(&stage, "/World/Nested/Box"),
            "the referenced mesh composes in under the referencing prim"
        );
    }

    /// Nesting one prototype library inside another: a vine prototype holds
    /// shoot prims that reference `/parts/Shoot`, and the whole vine prototype
    /// is then referenced onto the ground. The inner arc has to survive the
    /// outer one, or every *placed* plant would come out bare while the
    /// prototype it references looked perfect.
    ///
    /// It survives because a reference is composed in the layer stack where it
    /// was authored, unlike a relationship target — which is namespace-mapped
    /// through the arc and dropped when it points outside the referenced
    /// subtree (see `crate::stage::define_parts_library`).
    #[test]
    fn a_reference_inside_a_referenced_subtree_composes_through_both() {
        let stage = crate::stage::new_stage("nested.usda").unwrap();
        author_mesh(&stage, "/Vineyard/parts/Shoot/Var_0", &box_mesh(1.0)).unwrap();

        // The vine prototype: its own geometry, plus a shoot referencing the
        // other library.
        openusd::schemas::geom::Xform::define(
            &stage,
            sdf::path("/Vineyard/parts/Vine/Var_0").unwrap(),
        )
        .unwrap();
        author_mesh(&stage, "/Vineyard/parts/Vine/Var_0/Wood", &box_mesh(2.0)).unwrap();
        usd_bevy::authoring::define_prim(&stage, "/Vineyard/parts/Vine/Var_0/Shoot_00", "Mesh")
            .unwrap();
        reference_prim(
            &stage,
            "/Vineyard/parts/Vine/Var_0/Shoot_00",
            "/Vineyard/parts/Shoot/Var_0",
        )
        .unwrap();

        // And the placed vine, referencing that.
        usd_bevy::authoring::define_prim(&stage, "/Vineyard/Planting/Vine_000", "Xform").unwrap();
        reference_prim(
            &stage,
            "/Vineyard/Planting/Vine_000",
            "/Vineyard/parts/Vine/Var_0",
        )
        .unwrap();

        assert!(
            usd_bevy::authoring::prim_exists(&stage, "/Vineyard/Planting/Vine_000/Wood"),
            "the prototype's own geometry composes in"
        );
        assert!(
            usd_bevy::authoring::prim_exists(&stage, "/Vineyard/Planting/Vine_000/Shoot_00"),
            "and so does the prim that references the nested library"
        );

        let shoot = openusd::schemas::geom::Mesh::get(
            &stage,
            sdf::path("/Vineyard/Planting/Vine_000/Shoot_00").unwrap(),
        )
        .unwrap()
        .expect("the shoot composes as a Mesh");
        assert!(
            matches!(
                shoot.points_attr().get::<Value>().unwrap(),
                Some(Value::Vec3fVec(p)) if p.len() == 24
            ),
            "the shoot's points resolve through both references"
        );
    }

    /// USD's default is `catmullClark`, which declares every mesh here a
    /// subdivision *cage*. A renderer that honours it rounds the drawn teeth
    /// off a leaf margin and sands away the bark ridges `strand` exists to
    /// produce — and the viewer cannot show the difference, because `usd_bevy`
    /// defaults to the value we want rather than to USD's.
    #[test]
    fn author_mesh_declares_a_polygon_mesh_rather_than_a_subdivision_cage() {
        let stage = crate::stage::new_stage("subdiv.usda").unwrap();
        let prim = author_mesh(&stage, "/Vineyard/Box", &box_mesh(1.0)).unwrap();
        assert_eq!(
            prim.subdivision_scheme_attr()
                .get::<openusd::schemas::geom::SubdivisionScheme>()
                .unwrap(),
            Some(SubdivisionScheme::None)
        );
    }

    /// `extent` is what a consumer frustum-culls against, and it has to be the
    /// *real* bound — a stale or too-small box culls geometry that was on
    /// screen, which reads as flickering rather than as a wrong number.
    #[test]
    fn author_mesh_bounds_its_own_points() {
        let stage = crate::stage::new_stage("extent.usda").unwrap();
        let mesh = merge_meshes(&[box_mesh(2.0), box_mesh(6.0)]);
        let prim = author_mesh(&stage, "/Vineyard/Box", &mesh).unwrap();

        match prim.extent_attr().get::<Value>().unwrap() {
            Some(Value::Vec3fVec(v)) => {
                assert_eq!(v.len(), 2, "extent is the two corners");
                let (min, max) = (v[0], v[1]);
                assert_eq!([min.x, min.y, min.z], [-3.0, -3.0, -3.0]);
                assert_eq!([max.x, max.y, max.z], [3.0, 3.0, 3.0]);
            }
            other => panic!("extent not authored as float3[]: {other:?}"),
        }
    }

    /// A mesh with nothing in it has no bound to author, and must not author a
    /// nonsense one built from `f32::MAX`.
    #[test]
    fn an_empty_mesh_is_authored_without_an_extent() {
        let stage = crate::stage::new_stage("empty.usda").unwrap();
        let prim = author_mesh(&stage, "/Vineyard/Nothing", &MeshData::default()).unwrap();
        assert_eq!(prim.extent_attr().get::<Value>().unwrap(), None);
    }

    #[test]
    fn set_display_color_authors_one_constant_value() {
        let stage = crate::stage::new_stage("color.usda").unwrap();
        let prim = author_mesh(&stage, "/Vineyard/Box", &box_mesh(1.0)).unwrap();
        set_display_color(&prim, [0.25, 0.5, 0.75]).unwrap();

        let attr = prim.display_color_attr();
        match attr.get::<Value>().unwrap() {
            Some(Value::Vec3fVec(v)) => {
                assert_eq!(v.len(), 1, "one value for the whole mesh");
                assert_eq!([v[0].x, v[0].y, v[0].z], [0.25, 0.5, 0.75]);
            }
            other => panic!("displayColor not authored as color3f[]: {other:?}"),
        }
        // Without the token a reader is entitled to take the schema default —
        // `UsdGeomPrimvar`'s is `constant`, but `usd_bevy`'s is `vertex`, and
        // one value where a vertex each was promised falls through to white.
        assert_eq!(
            attr.get_metadata::<openusd::tf::Token>("interpolation")
                .unwrap()
                .as_deref(),
            Some("constant")
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
