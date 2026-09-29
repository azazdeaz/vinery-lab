//! Readers for a [`MeshData`] that the kernel tests share, and that a
//! generator's own tests read its meshes back through. Always compiled, since
//! another crate's `#[cfg(test)]` cannot reach this one's.

use bevy::math::Vec3;

use crate::geometry::mesh::MeshData;

/// Lowest and highest coordinate of a mesh's points along `axis`.
pub fn bounds(mesh: &MeshData, axis: usize) -> (f32, f32) {
    mesh.points
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(p[axis]), hi.max(p[axis]))
        })
}

/// The corner indices of each face, walking the `face_vertex_counts` /
/// `face_vertex_indices` pair a [`MeshData`] stores its faces as.
pub fn faces(mesh: &MeshData) -> impl Iterator<Item = &[i32]> {
    let mut cursor = 0usize;
    mesh.face_vertex_counts.iter().map(move |count| {
        let face = &mesh.face_vertex_indices[cursor..cursor + *count as usize];
        cursor += *count as usize;
        face
    })
}

/// A face's normal, from its first three corners. Unnormalized: the winding
/// tests only ever ask which side of something it points, and a zero-area face
/// should fail those rather than produce a NaN direction.
pub fn face_normal(mesh: &MeshData, face: &[i32]) -> Vec3 {
    let [a, b, c] = [0, 1, 2].map(|i| corner(mesh, face[i]));
    (b - a).cross(c - a)
}

/// A face's centroid — the reference point a normal is compared against when
/// "outward" means "away from the middle of the thing".
pub fn face_centroid(mesh: &MeshData, face: &[i32]) -> Vec3 {
    face.iter().map(|i| corner(mesh, *i)).sum::<Vec3>() / face.len() as f32
}

fn corner(mesh: &MeshData, index: i32) -> Vec3 {
    Vec3::from(mesh.points[index as usize])
}
