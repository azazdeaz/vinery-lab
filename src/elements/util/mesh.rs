//! The geometry kernels' own mesh type, and the primitives built directly in
//! it.
//!
//! [`MeshData`] is what [`strand`](super::strand) and
//! [`outline`](super::outline) hand back: points plus **n-gon** faces, which is
//! what a tube skinner and a polygon filler naturally produce. The scene holds
//! Bevy [`Mesh`](bevy::mesh::Mesh)es, so [`MeshData::to_mesh`] triangulates on
//! the way out — the one place a fan triangulation happens, rather than once
//! per kernel.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};

/// A polygonal mesh, as the geometry kernels produce it.
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

impl MeshData {
    /// This mesh as a Bevy [`Mesh`]: an indexed triangle list with normals.
    ///
    /// The bridge out of the geometry kernels, which build polygon soup, and
    /// into the one representation the rest of the pipeline uses — the viewer
    /// draws it and the exporter reads it back, so there is nothing left that
    /// the two could disagree about.
    ///
    /// Faces are fan-triangulated from their first corner, which is exact for
    /// every face this crate produces: quads on a box and on a strand's
    /// barrel, one convex n-gon per cylinder cap, triangles everywhere else.
    ///
    /// Normals are *smooth*, because the kernels express shading through
    /// topology: a shape that wants a hard edge gives its faces unshared
    /// vertices (see [`box_mesh`]) and one that wants a round silhouette shares
    /// them (see [`cylinder_mesh`]).
    pub fn to_mesh(&self) -> Mesh {
        let mut indices: Vec<u32> = Vec::with_capacity(self.face_vertex_indices.len());
        let mut cursor = 0usize;
        for count in &self.face_vertex_counts {
            let face = &self.face_vertex_indices[cursor..cursor + *count as usize];
            cursor += face.len();
            for corner in 1..face.len().saturating_sub(1) {
                indices.extend([
                    face[0] as u32,
                    face[corner] as u32,
                    face[corner + 1] as u32,
                ]);
            }
        }

        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.points.clone())
            .with_inserted_indices(Indices::U32(indices))
            .with_computed_normals()
    }
}

/// One mesh holding every part, with each part's indices rebased.
///
/// A dozen tubes merge for free and cost one prim instead of a dozen, which is
/// what the export pays for.
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
    use bevy::mesh::VertexAttributeValues;

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

    /// The triangles a fan produces, as position triples.
    fn triangles(mesh: &MeshData) -> Vec<[Vec3; 3]> {
        let converted = mesh.to_mesh();
        let Some(Indices::U32(indices)) = converted.indices() else {
            panic!("to_mesh must produce u32 indices");
        };
        indices
            .chunks(3)
            .map(|tri| [0, 1, 2].map(|i| Vec3::from(mesh.points[tri[i] as usize])))
            .collect()
    }

    /// An n-gon becomes n-2 triangles, and nothing is dropped or invented.
    #[test]
    fn to_mesh_fan_triangulates_every_face() {
        // Six quads, two triangles each.
        assert_eq!(triangles(&box_mesh(2.0)).len(), 12);

        // Eight barrel quads plus two eight-sided caps: 8*2 + 2*6.
        assert_eq!(triangles(&cylinder_mesh(0.05, 1.0, 8)).len(), 28);

        assert_eq!(box_mesh(2.0).to_mesh().count_vertices(), 24, "points are untouched");
    }

    /// A fan from the first corner keeps each face's orientation, and getting
    /// that wrong turns a mesh inside out — which under back-face culling is a
    /// hole rather than an obviously wrong shape.
    #[test]
    fn to_mesh_preserves_outward_winding() {
        for mesh in [box_mesh(2.0), cylinder_mesh(0.4, 1.0, 8)] {
            // Both shapes enclose the average of their own points, so
            // "outward" is "away from that" for the caps as well as the sides.
            let center = mesh.points.iter().fold(Vec3::ZERO, |sum, p| sum + Vec3::from(*p))
                / mesh.points.len() as f32;
            for (i, [a, b, c]) in triangles(&mesh).into_iter().enumerate() {
                let normal = (b - a).cross(c - a);
                let outward = (a + b + c) / 3.0 - center;
                assert!(normal.dot(outward) > 0.0, "triangle {i} winds inward");
            }
        }
    }

    /// The kernels express hard and soft edges through what they share, so the
    /// conversion has to shade off the topology rather than impose one or the
    /// other: a box reads flat because its faces share nothing, a barrel reads
    /// round because its rings do.
    #[test]
    fn to_mesh_shades_off_the_topology_it_was_given() {
        let box_mesh = box_mesh(2.0).to_mesh();
        let normals = box_mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .and_then(VertexAttributeValues::as_float3)
            .expect("normals were computed");
        // Unshared corners: every normal is an axis, so the box shades flat.
        for n in normals {
            let n = Vec3::from(*n);
            assert!((n.length() - 1.0).abs() < 1e-5);
            assert_eq!(n.abs().to_array().iter().filter(|c| **c > 0.9).count(), 1);
        }

        let barrel = cylinder_mesh(0.4, 1.0, 8).to_mesh();
        let normals = barrel
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .and_then(VertexAttributeValues::as_float3)
            .unwrap();
        // The barrel's rings are shared, so its normals point out from the
        // axis rather than along a face.
        let n = Vec3::from(normals[0]);
        assert!(n.z.abs() < 0.2, "a barrel normal is level with the axis: {n}");
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

}
