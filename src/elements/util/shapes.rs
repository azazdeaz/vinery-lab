//! Outlines built in code rather than traced.
//!
//! The counterpart of a drawing under `assets/`: the same closed ring, in the
//! same frame [`outline`](super::outline) reads a drawing into — attachment on
//! the origin, the shape running along +X, width across Y — but described by
//! a length, a width and a profile. A simple entire shape is cheaper to write
//! than to draw, and a grass blade is one. A margin worth drawing goes through
//! [`Outline::from_svg`] instead, and a call site here is what a traced file
//! replaces.
//!
//! Every ring is symmetric about the X axis and closes across its base at
//! `x = 0`, which is the attachment: the flat cut where a blade leaves its
//! sheath or a leaf its petiole.
//!
//! Also here: [`bent`], which fills a ring and folds it the way a leaf folds,
//! and the two rotations that take a shape out of this frame — [`standing`]
//! and [`lying`] — so every element places its foliage the same way.

use std::f64::consts::FRAC_PI_2;

use anyhow::Result;
use bevy::math::Quat;
use spade::Point2;

use super::mesh::{MeshData, bend};
use super::outline::{Outline, outline_mesh};

/// Stations along each edge of a ring. Enough that a bend has vertices to
/// bend, few enough that a blade stays a dozen triangles.
const STATIONS: usize = 10;

/// Narrowest a base may be, as a fraction of the half-width: a shape that
/// tapers to nothing at both ends has no edge to attach by.
const BASE: f64 = 0.08;

/// How deep a tooth cuts into the edge, as a fraction of the profile there.
const TOOTH: f64 = 0.55;

/// A grass blade: parallel-sided for most of its length, then drawn to a
/// point. `width` is at the base.
pub fn blade(length: f64, width: f64) -> Outline {
    ring(length, width, STATIONS, |_, t| (1.0 - t.powi(4)).sqrt())
}

/// An entire leaf, widest `peak` of the way along — a third for a lanceolate
/// one, a half for a round one — narrowing to a point at the tip and to a
/// stub at the base.
pub fn leaf(length: f64, width: f64, peak: f64) -> Outline {
    ring(length, width, STATIONS, move |_, t| profile(t, peak))
}

/// [`leaf`] with `teeth` triangular notches cut into each edge — the stand-in
/// for a lobed rosette leaf until one is drawn.
pub fn toothed(length: f64, width: f64, peak: f64, teeth: usize) -> Outline {
    let stations = (2 * teeth).max(2);
    ring(length, width, stations, move |i, t| {
        let cut = if i % 2 == 1 { TOOTH } else { 1.0 };
        profile(t, peak) * cut
    })
}

/// A hump over `0..1` peaking at `peak`, as a fraction of the half-width.
fn profile(t: f64, peak: f64) -> f64 {
    let peak = peak.clamp(0.05, 0.95);
    // `sin(pi * t^p)` peaks where `t^p = 1/2`, so `p` is solved from the peak.
    let p = 0.5f64.ln() / peak.ln();
    (std::f64::consts::PI * t.powf(p)).sin().max(0.0)
}

/// The ring: down the -Y edge from the base to the tip, back up the +Y edge,
/// closing across the base. `half(i, t)` is the half-width at station `i`
/// (`t` of the way along) as a fraction of `width / 2`; the tip is pinched to
/// a point and the base held open to at least [`BASE`].
fn ring(length: f64, width: f64, stations: usize, half: impl Fn(usize, f64) -> f64) -> Outline {
    let (length, width) = (length.max(1e-4), width.max(1e-5));
    let stations = stations.max(2);
    let w = |i: usize| -> f64 {
        let t = i as f64 / stations as f64;
        let fraction = if i == 0 {
            half(0, 0.0).max(BASE)
        } else {
            half(i, t).max(0.0)
        };
        fraction * width / 2.0
    };
    let x = |i: usize| length * i as f64 / stations as f64;

    let mut points = Vec::with_capacity(2 * stations + 1);
    for i in 0..stations {
        points.push(Point2::new(x(i), -w(i)));
    }
    points.push(Point2::new(length, 0.0));
    for i in (1..stations).rev() {
        points.push(Point2::new(x(i), w(i)));
    }
    points.push(Point2::new(0.0, w(0)));
    Outline { points }
}

// ─── Filling and placing ────────────────────────────────────────────

/// `outline` filled at about `triangles` triangles and folded like a leaf:
/// keeled about its midrib by `keel`, a curvature in 1/m that curls the
/// edges up toward the face, and drooped `droop` radians at the tip, away
/// from it. Either at zero is left flat. Still in the outline's frame.
pub fn bent(outline: Outline, droop: f64, keel: f64, triangles: f64) -> Result<MeshData> {
    let reach = outline
        .points
        .iter()
        .fold(0.0f64, |m, p| m.max(p.x))
        .max(1e-6);
    let mut mesh = outline_mesh(&outline, outline.area() / triangles.max(1.0))?;
    for point in &mut mesh.points {
        let (mut x, mut y, mut z) = (point[0] as f64, point[1] as f64, point[2] as f64);
        if keel != 0.0 {
            (y, z) = bend(y, z, -keel);
        }
        if droop != 0.0 {
            (x, z) = bend(x, z, droop / reach);
        }
        *point = [x as f32, y as f32, z as f32];
    }
    Ok(mesh)
}

/// A [`blade`], filled and folded — see [`bent`].
pub fn blade_mesh(
    length: f64,
    width: f64,
    droop: f64,
    keel: f64,
    triangles: f64,
) -> Result<MeshData> {
    bent(blade(length, width), droop, keel, triangles)
}

/// Stands a shape drawn along `+X` up on `+Z`, leaning `lean` radians off
/// vertical, turned to `yaw` about the vertical.
pub fn standing(yaw: f64, lean: f64) -> Quat {
    Quat::from_rotation_z(yaw as f32) * Quat::from_rotation_y((-FRAC_PI_2 + lean) as f32)
}

/// Lays a shape drawn along `+X` out along the ground with its face up, the
/// tip lifted `pitch` radians, turned to `yaw` about the vertical.
pub fn lying(yaw: f64, pitch: f64) -> Quat {
    Quat::from_rotation_z(yaw as f32) * Quat::from_rotation_y(-pitch as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::util::outline::outline_mesh;

    fn all() -> Vec<(&'static str, Outline)> {
        vec![
            ("blade", blade(0.1, 0.004)),
            ("leaf", leaf(0.05, 0.02, 0.35)),
            ("round", leaf(0.05, 0.05, 0.5)),
            ("toothed", toothed(0.08, 0.02, 0.3, 6)),
        ]
    }

    /// The frame contract every placed outline depends on: the attachment on
    /// the origin, the shape ahead of it along +X, its width across Y.
    #[test]
    fn a_shape_hangs_from_the_origin_and_runs_along_x() {
        for (name, shape) in all() {
            assert!(shape.area() > 0.0, "{name} encloses area");
            assert!(
                shape.points.iter().all(|p| p.x >= 0.0),
                "{name} runs forward from the attachment"
            );
            assert!(
                shape.points.iter().any(|p| p.x == 0.0),
                "{name} has an edge on the attachment"
            );
            let reach = shape.points.iter().fold(0.0f64, |m, p| m.max(p.x));
            let across = shape.points.iter().fold(0.0f64, |m, p| m.max(p.y.abs()));
            assert!(reach > across, "{name} is longer than it is wide");
        }
    }

    #[test]
    fn a_shape_is_the_size_it_was_asked_for() {
        let shape = blade(0.1, 0.004);
        let reach = shape.points.iter().fold(0.0f64, |m, p| m.max(p.x));
        let width = shape.points.iter().fold(0.0f64, |m, p| m.max(p.y)) * 2.0;
        assert!((reach - 0.1).abs() < 1e-9);
        assert!((width - 0.004).abs() < 1e-9, "widest at the base: {width}");
    }

    /// A built ring has to be fillable: no crossings, and the teeth survive
    /// as constraint edges rather than being webbed over.
    #[test]
    fn every_shape_fills() {
        for (name, shape) in all() {
            let mesh = outline_mesh(&shape, shape.area() / 6.0)
                .unwrap_or_else(|e| panic!("{name} fills: {e:#}"));
            assert!(!mesh.face_vertex_counts.is_empty(), "{name} has faces");
        }
        let plain = leaf(0.08, 0.02, 0.3).area();
        let cut = toothed(0.08, 0.02, 0.3, 6).area();
        assert!(cut < plain, "teeth take area away: {cut} vs {plain}");
    }

    /// A droop takes the tip below the plane it was drawn on, and a keel lifts
    /// the edges above it; neither moves the attachment.
    #[test]
    fn bending_folds_the_sheet_and_holds_the_base() {
        let flat = bent(blade(0.1, 0.004), 0.0, 0.0, 8.0).unwrap();
        assert!(flat.points.iter().all(|p| p[2] == 0.0));

        let drooped = bent(blade(0.1, 0.004), 1.0, 0.0, 8.0).unwrap();
        let tip = drooped
            .points
            .iter()
            .max_by(|a, b| a[0].total_cmp(&b[0]))
            .unwrap();
        assert!(tip[2] < -0.02, "the tip drops: {tip:?}");
        assert!(
            drooped.points.iter().any(|p| p[0] == 0.0 && p[2] == 0.0),
            "the base stays on the plane"
        );

        let keeled = bent(blade(0.1, 0.004), 0.0, 250.0, 8.0).unwrap();
        let edge = keeled
            .points
            .iter()
            .max_by(|a, b| a[1].abs().total_cmp(&b[1].abs()))
            .unwrap();
        assert!(edge[2] > 0.0, "the edges curl up toward the face: {edge:?}");
    }

    /// The two placements: standing puts the tip up, lying leaves the face up.
    #[test]
    fn standing_and_lying_take_the_shape_out_of_its_frame() {
        let tip = standing(0.0, 0.0) * bevy::math::Vec3::X;
        assert!(tip.z > 0.999, "standing: {tip}");
        let tip = standing(0.0, 0.3) * bevy::math::Vec3::X;
        assert!(tip.z > 0.9 && tip.x > 0.0, "leaning: {tip}");

        let tip = lying(0.0, 0.2) * bevy::math::Vec3::X;
        assert!(tip.x > 0.9 && tip.z > 0.0, "lying, tip lifted: {tip}");
        let face = lying(1.0, 0.2) * bevy::math::Vec3::Z;
        assert!(face.z > 0.9, "lying, face up: {face}");
    }
}
