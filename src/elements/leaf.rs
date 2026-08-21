//! Leaf element — one blade of the canopy.
//!
//! A grapevine leaf is a broad palmate blade on a stalk: five lobes around a
//! notch where the stalk meets it, and a toothed margin the whole way round.
//! That outline is what makes a vineyard read as a vineyard from any distance
//! at which the wood has stopped being visible, and for simplicity it is
//! *drawn*, one SVG per shape under `assets/leaves/`, and filled with
//! triangles here.
//!
//! The outlines are drawn complete with the **petiole**, the stalk a leaf
//! hangs by, so a leaf is one flat piece from the stalk's free end to the
//! blade's apex. The veins are shading rather than outline and are not here.
//!
//! # Local frame
//!
//! A leaf is authored **flat on the XY plane**, with the petiole's free end —
//! the point at which it joins a shoot — on the **origin**, and the leaf
//! running along **+X**, front face toward +Z:
//!
//! ```text
//!   +Y                       ╭──╮  ╭──╮
//!    ↑                      ╱    ╲╱    ╲
//!    ·──────────────────────┤  the blade ├──→ +X
//!    ↑     the petiole      ╲    ╱╲    ╱
//!  origin                    ╰──╯  ╰──╯
//! ```
//!
//! Same convention a [`shoot`](super::shoot) leaves its bud with, and for the
//! same reason: whatever ends up placing leaves picks a point and a direction,
//! and everything else follows without the placer knowing how a leaf was
//! drawn. Flat is a starting point, not the finished shape — a real leaf
//! curls and twists, and the fill leaves interior vertices to bend when that
//! pass arrives.
//!
//! # One subtree
//!
//! The prototype library under [`PROTOTYPE`], one `Var_<i>` per outline file,
//! every one of them a full-grown leaf of exactly [`AREA`]. Nothing here
//! decides where a leaf hangs or how big it ends up; that belongs to whoever
//! owns the shoot it hangs from, and the fixed area is what lets that owner
//! express age as a scale without minding which variation it drew.

use anyhow::Context;
use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use openusd::schemas::geom::Gprim;
use openusd::sdf::Value;
use usd_bevy::authoring::{define_prim, remove_prim};
use usd_bevy::live::LiveStage;

use super::Grow;
use super::util::outline::{Outline, outline_mesh};
use super::util::usd::{MeshData, author_mesh};

/// The prototype library this element owns: one `Var_<i>` mesh per outline.
pub const PROTOTYPE: &str = "/Vineyard/parts/Leaf";

/// The drawn outlines, in variation order.
///
/// Embedded at compile time rather than read from `assets/` at run time, so
/// that the same bytes reach every consumer: this crate ships as a Python
/// extension module inside a wheel, where a relative path to the repository's
/// `assets/` directory does not exist. `include_str!` sidesteps path
/// resolution and packaging both, and costs nothing — five outlines is
/// fifteen kilobytes.
///
/// Adding a shape is one line here. See
/// [`outline`](super::util::outline) for the frame a file has to be drawn in.
const OUTLINES: &[&str] = &[
    include_str!("../../assets/leaves/leaf_1.svg"),
    include_str!("../../assets/leaves/leaf_2.svg"),
    include_str!("../../assets/leaves/leaf_3.svg"),
    include_str!("../../assets/leaves/leaf_4.svg"),
    include_str!("../../assets/leaves/leaf_5.svg"),
];

/// How many prototypes this element authors.
///
/// A count rather than a parameter, unlike every other element's
/// `variations`: the shapes are drawn, not seeded, so there are exactly as
/// many as there are files. Whoever scatters leaves passes this to
/// [`variation_indices`](super::variation_indices).
pub const VARIATIONS: usize = OUTLINES.len();

/// The area every prototype is built at, in m² — one full-grown leaf.
///
/// Fixed rather than a parameter, and that is the whole point: because all
/// five prototypes cover *exactly* this, a leaf's size is entirely a matter of
/// the scale it is placed at. A placer can shrink one for a leaf that is still
/// growing, or for a shaded interior leaf, and get the size it asked for
/// without knowing or caring which variation it drew — which it could not do
/// if the shapes were merely similar in size.
///
/// The number is the whole drawn outline, petiole included, so the *blade*
/// alone is a percent or two under it, by a little more on the shapes whose
/// stalks are drawn longer. A mature *Vitis vinifera* leaf runs 100–200 cm²,
/// and this is a middling one.
pub const AREA: f32 = 0.015;

// ─── Params ─────────────────────────────────────────────────────────

#[derive(Resource, Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct LeafParams {
    /// How finely the inside of a blade is subdivided, as the number of
    /// triangles its *area* is cut into.
    ///
    /// A floor rather than an exact count: the drawn margin is honoured
    /// exactly, and tiling that alone already costs about one triangle per
    /// point of the outline — some 180 of them — before this is consulted at
    /// all. What it buys past that is vertices across the *middle* of the
    /// blade, which has none of its own and is what a later curl has to bend.
    /// The default lands a blade at roughly 350 triangles.
    pub detail: u32,
}

impl Default for LeafParams {
    fn default() -> Self {
        Self { detail: 120 }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<LeafParams>().add_systems(
        PreUpdate,
        author_prototypes
            .in_set(Grow::Prototypes)
            .run_if(resource_changed::<LeafParams>),
    );
}

// ─── Shape ──────────────────────────────────────────────────────────

/// One blade, in the prototype's local frame, at [`AREA`].
fn blade_mesh(svg: &str, params: &LeafParams) -> anyhow::Result<MeshData> {
    let area = AREA as f64;
    let outline = Outline::from_svg(svg)?.with_area(area);
    outline_mesh(&outline, area / params.detail.max(1) as f64)
}

// ─── Authoring ──────────────────────────────────────────────────────

/// Authors one mesh per outline under [`PROTOTYPE`].
pub fn author_prototypes(live: NonSend<LiveStage>, params: Res<LeafParams>) -> Result<()> {
    let stage = &live.stage;
    remove_prim(stage, PROTOTYPE)?;
    define_prim(stage, PROTOTYPE, "Scope")?;

    for (i, svg) in OUTLINES.iter().enumerate() {
        let mesh = blade_mesh(svg, &params)
            .with_context(|| format!("leaf variation {i} could not be built"))?;
        // A blade is a surface with no inside, so it has to be lit and drawn
        // from underneath as well — a canopy is looked up into as often as
        // down onto. Single-sided, every leaf above the camera would vanish.
        author_mesh(stage, &format!("{PROTOTYPE}/Var_{i}"), &mesh)?
            .create_double_sided_attr()?
            .set(Value::Bool(true))?;
    }
    Ok(())
}

// ─── UI ─────────────────────────────────────────────────────────────

pub fn ui() -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            label_small("Leaf detail"),
            (
                @FeathersSlider { @min: 8.0, @max: 400.0, @value: 120.0 }
                SliderStep(8.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<LeafParams>| {
                    params.detail = change.value.round().max(1.0) as u32;
                })
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::VineyardParams;
    use crate::elements::util::place::prototype_count;
    use crate::elements::util::testing::{Authoring, bounds, face_normal, faces};

    fn params() -> LeafParams {
        LeafParams::default()
    }

    fn blades(params: &LeafParams) -> Vec<MeshData> {
        OUTLINES
            .iter()
            .enumerate()
            .map(|(i, svg)| {
                blade_mesh(svg, params).unwrap_or_else(|e| panic!("leaf_{}: {e:#}", i + 1))
            })
            .collect()
    }

    fn surface_area(mesh: &MeshData) -> f32 {
        // The cross product of two edges is twice the triangle's area.
        faces(mesh)
            .map(|face| face_normal(mesh, face).length() / 2.0)
            .sum()
    }

    /// Every committed outline has to survive the whole path from file to
    /// mesh. This is the guard that keeps a badly drawn *new* file — one that
    /// crosses itself, or was left open — from reaching the viewer, where the
    /// same problem surfaces as an author system failing mid-frame.
    #[test]
    fn every_drawn_outline_builds() {
        assert_eq!(VARIATIONS, 5, "five shapes are committed");
        for mesh in blades(&params()) {
            assert!(!mesh.face_vertex_counts.is_empty());
            assert!(mesh.points.iter().flatten().all(|c| c.is_finite()));
        }
    }

    /// The placement contract: flat, hanging from the origin, running +X. A
    /// leaf that drifted off any of the three would hang off its shoot at an
    /// angle nobody asked for.
    #[test]
    fn a_blade_lies_flat_and_runs_along_x_from_the_origin() {
        for (i, mesh) in blades(&params()).iter().enumerate() {
            let (z0, z1) = bounds(mesh, 2);
            assert_eq!((z0, z1), (0.0, 0.0), "leaf_{} is flat", i + 1);

            let (x0, x1) = bounds(mesh, 0);
            assert_eq!(x0, 0.0, "leaf_{} starts at the petiole", i + 1);
            assert!(x1 > 0.0, "leaf_{} reaches forward, got {x1}", i + 1);

            // And straddles the axis rather than sitting to one side of it.
            let (y0, y1) = bounds(mesh, 1);
            assert!(y0 < 0.0 && y1 > 0.0, "leaf_{}: {y0}..{y1}", i + 1);
        }
    }

    /// Greatest distance from the leaf's axis, among the points lying in
    /// `band` of the way along it.
    fn half_width(mesh: &MeshData, band: std::ops::Range<f32>) -> f32 {
        let reach = bounds(mesh, 0).1;
        mesh.points
            .iter()
            .filter(|p| band.contains(&(p[0] / reach)))
            .fold(0.0, |m: f32, p| m.max(p[1].abs()))
    }

    /// *Which end* of a leaf sits on the origin. The outlines are drawn
    /// complete with their petioles, so one end of a leaf is a few
    /// millimeters across and the other is the width of a hand.
    ///
    /// Anchoring the wrong one puts every leaf on backwards — apex buried in
    /// the shoot, stalk waving in the air — and leaves a mesh that is still
    /// flat, still runs along +X and still covers the area asked for. No
    /// other test here can tell the difference.
    #[test]
    fn a_leaf_hangs_by_its_petiole_rather_than_by_its_tip() {
        for (i, mesh) in blades(&params()).iter().enumerate() {
            let stalk = half_width(mesh, 0.0..0.05);
            let blade = half_width(mesh, 0.4..0.6);
            assert!(
                stalk * 8.0 < blade,
                "leaf_{}: {stalk:.4} m across at the origin, {blade:.4} m at mid-length",
                i + 1
            );
        }
    }

    /// The one thing the five shapes are asked to have in common, and the
    /// contract every later scale rests on: a prototype covers [`AREA`],
    /// whichever one it is. Placing a leaf at half size has to mean half size
    /// no matter which variation came up, and it only does if the five agree
    /// here to far better than the eye could tell them apart.
    #[test]
    fn every_variation_covers_the_same_area() {
        let areas: Vec<f32> = blades(&params()).iter().map(surface_area).collect();
        let (lo, hi) = areas
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), a| (lo.min(*a), hi.max(*a)));

        assert!(
            (hi - lo) / lo < 1e-3,
            "the five agree with each other: {areas:?}"
        );
        assert!(
            (lo - AREA).abs() / AREA < 1e-3,
            "and on {AREA} m²: {areas:?}"
        );
    }

    /// Matched by area, not by bounding box: the outlines are different
    /// shapes, so their extents must *not* agree, or the normalization would
    /// have been a plain scale-to-fit and the shapes would all be the same.
    #[test]
    fn matching_the_area_leaves_the_shapes_different() {
        let reaches: Vec<f32> = blades(&params())
            .iter()
            .map(|mesh| bounds(mesh, 0).1)
            .collect();
        let (lo, hi) = reaches
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), r| (lo.min(*r), hi.max(*r)));
        assert!(
            hi - lo > lo * 0.1,
            "same area, different proportions: {reaches:?}"
        );
    }

    /// A blade is drawn from above, so its faces have to point up. USD reads
    /// counter-clockwise as front-facing by default, and the wrong winding
    /// would light the whole canopy from the wrong side.
    #[test]
    fn every_face_points_up() {
        for (i, mesh) in blades(&params()).iter().enumerate() {
            assert!(
                faces(mesh).all(|face| face_normal(mesh, face).z > 0.0),
                "leaf_{} winds counter-clockwise seen from +Z",
                i + 1
            );
        }
    }

    /// The blades are drawn shapes rather than seeded ones, so no two may
    /// come out the same — five copies of one leaf would be a wiring mistake
    /// in [`OUTLINES`], and nothing else here would notice.
    #[test]
    fn the_variations_are_five_different_shapes() {
        let blades = blades(&params());
        for (i, a) in blades.iter().enumerate() {
            for (j, b) in blades.iter().enumerate().skip(i + 1) {
                assert_ne!(
                    a.points,
                    b.points,
                    "leaf_{} and leaf_{} are the same shape",
                    i + 1,
                    j + 1
                );
            }
        }
    }

    #[test]
    fn blades_are_reproducible() {
        assert_eq!(blades(&params())[0].points, blades(&params())[0].points);
    }

    /// `detail` has to reach the middle of the blade. The margin's teeth
    /// force a floor of small triangles whatever it is set to, so what the
    /// knob is worth is measured past that floor.
    #[test]
    fn more_detail_spends_more_triangles() {
        let coarse = blades(&LeafParams { detail: 8 });
        let fine = blades(&LeafParams { detail: 400 });
        for (i, (c, f)) in coarse.iter().zip(&fine).enumerate() {
            assert!(
                f.face_vertex_counts.len() > c.face_vertex_counts.len(),
                "leaf_{}: {} triangles at detail 400 vs {} at 8",
                i + 1,
                f.face_vertex_counts.len(),
                c.face_vertex_counts.len()
            );
        }
    }

    /// `detail` divides the blade's area, and the viewer's slider reaches its
    /// bottom stop — a leaf asked for no subdivision at all must clamp rather
    /// than divide by zero.
    #[test]
    fn a_blade_with_no_subdivision_asked_for_still_builds() {
        let mesh = blade_mesh(OUTLINES[0], &LeafParams { detail: 0 })
            .expect("a leaf at the slider's bottom stop still builds");
        assert!(mesh.points.iter().flatten().all(|c| c.is_finite()));
        assert!(!mesh.face_vertex_counts.is_empty());
    }

    #[test]
    fn authors_one_prototype_per_outline() {
        let stage = crate::generate::generate_stage(&VineyardParams::default()).unwrap();
        assert_eq!(prototype_count(&stage, PROTOTYPE), VARIATIONS);
    }

    /// A flat mesh drawn from one side only would disappear when the camera
    /// goes under the canopy, which is most of the time.
    #[test]
    fn blades_are_authored_double_sided() {
        let mut authoring = Authoring::new("leaf.usda", author_prototypes);
        authoring.insert(params()).run();

        let usda = authoring.stage.root_layer().export_to_string().unwrap();
        assert_eq!(
            usda.matches("uniform bool doubleSided = true").count(),
            VARIATIONS,
            "every blade renders from both sides; got:\n{usda}"
        );
    }

    /// The element owns its subtree and rewrites it from scratch, so a second
    /// pass must leave exactly what the first one did — no doubling up, no
    /// leftovers.
    #[test]
    fn re_authoring_rewrites_rather_than_accumulates() {
        let mut authoring = Authoring::new("leaf.usda", author_prototypes);
        authoring.insert(params()).run();
        assert!(authoring.has(&format!("{PROTOTYPE}/Var_{}", VARIATIONS - 1)));

        authoring.insert(params()).run();
        assert_eq!(prototype_count(&authoring.stage, PROTOTYPE), VARIATIONS);
    }
}
