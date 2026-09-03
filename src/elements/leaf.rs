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
//! # The layer
//!
//! The last one: a leaf has nothing hanging off it, so this element builds
//! meshes and expands into nothing. [`shoot`](super::shoot) authors a
//! [`LeafConfig`] on every node it offers and [`build`] turns the distinct
//! configs into blades — each one a full-grown leaf of exactly [`AREA`].
//!
//! With no children, a leaf is a geometry prim in its own right rather than an
//! `Xform` over one, so it carries its [`Geometry`] directly. At six figures of
//! them that is half the prims in the scene.
//!
//! Nothing here decides where a leaf hangs or how big it ends up; that belongs
//! to the shoot it hangs from, and the fixed area is what lets that shoot
//! express age as a scale without minding which shape came up.

use anyhow::Context;
use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};

use super::util::outline::{Outline, outline_mesh};
use super::util::mesh::MeshData;
use super::util::{color, material};
use super::{Grow, Rng};
use crate::quantize::{Metric, farthest_first};
use crate::scene::{Geometry, Library, Order, Surface, configs_changed};

/// The mesh-library prefix this element registers its blades under.
pub const PART: &str = "Leaf";

/// The drawn outlines, in variation order.
///
/// `pub` because the shoot that hangs a leaf is the one that picks its blade,
/// and it needs to know how many there are to pick from.
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
pub const OUTLINES: &[&str] = &[
    include_str!("../../assets/leaves/leaf_1.svg"),
    include_str!("../../assets/leaves/leaf_2.svg"),
    include_str!("../../assets/leaves/leaf_3.svg"),
    include_str!("../../assets/leaves/leaf_4.svg"),
    include_str!("../../assets/leaves/leaf_5.svg"),
];

/// How many drawn shapes there are to choose from.
///
/// A count of files, not a budget: [`LeafParams::variations`] is what caps the
/// meshes, and a budget at or above this keeps every drawing — see
/// [`LeafMetric`].
pub const SHAPES: usize = OUTLINES.len();

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

/// How far apart two blades cut from different drawings are.
///
/// The outline is categorical: two shapes are different, not nearby, so this
/// has to dominate every smooth axis below it. That is what keeps all five
/// drawings alive under any budget that can hold them.
const OUTLINE_APART: f32 = 1.0;

/// One blade's shape, as the shoot that hangs it specified.
///
/// No size: every drawing is built at exactly [`AREA`], so how big a leaf ends
/// up is the scale it is placed at and never a fact about the mesh.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeafConfig {
    /// Which drawing this blade is cut from — an index into [`OUTLINES`].
    pub outline: u32,
    pub detail: u32,
}

impl LeafConfig {
    /// The blade these params call for, cut from drawing `outline`.
    pub fn new(params: &LeafParams, outline: usize) -> Self {
        Self {
            outline: (outline % SHAPES) as u32,
            detail: params.detail.max(1),
        }
    }
}

/// Two blades share a mesh when they were cut from the same drawing at about
/// the same resolution.
pub struct LeafMetric;

impl Metric<LeafConfig> for LeafMetric {
    fn distance(&self, a: &LeafConfig, b: &LeafConfig) -> f32 {
        let shape = if a.outline == b.outline { 0.0 } else { OUTLINE_APART };
        // Weighted to stay well under the categorical step across the whole
        // range `detail` can be set to, so resolution never outbids shape.
        let detail = (a.detail as f32 - b.detail as f32) * 0.001;
        (shape * shape + detail * detail).sqrt()
    }
}

// ─── Params ─────────────────────────────────────────────────────────

#[derive(Resource, Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct LeafParams {
    /// How many distinct blade meshes the scene may hold.
    ///
    /// A budget, not a count. The shapes are drawn rather than seeded, so
    /// unlike every other element's `variations` there is a natural ceiling on
    /// what it can buy: at [`SHAPES`] every drawing gets a mesh, and above that
    /// it buys nothing, because two blades cut from one drawing at one
    /// resolution are the same blade.
    pub variations: u32,
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
        Self {
            variations: SHAPES as u32,
            detail: 120,
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<LeafParams>().add_systems(
        PreUpdate,
        build
            .in_set(Grow::Scatter)
            .run_if(configs_changed::<LeafConfig>.or_else(resource_changed::<LeafParams>)),
    );
}

// ─── Shape ──────────────────────────────────────────────────────────

/// One blade, in its own local frame, at [`AREA`].
fn blade_mesh(config: &LeafConfig) -> anyhow::Result<MeshData> {
    let area = AREA as f64;
    let outline = Outline::from_svg(OUTLINES[config.outline as usize])?.with_area(area);
    outline_mesh(&outline, area / config.detail as f64)
}

// ─── Building ───────────────────────────────────────────────────────

/// Builds one mesh per distinct blade and gives it to every leaf that drew it.
///
/// The end of the pipeline: a leaf has no children, so it carries its geometry
/// directly and expands into nothing.
pub(crate) fn build(
    mut commands: Commands,
    mut library: Library,
    params: Res<LeafParams>,
    leaves: Query<(Entity, &Order, &LeafConfig)>,
) -> Result<()> {
    library.clear(PART);

    let mut hung: Vec<(Order, Entity, LeafConfig)> = leaves
        .iter()
        .map(|(entity, order, config)| (*order, entity, *config))
        .collect();
    hung.sort_by_key(|(order, ..)| *order);

    let configs: Vec<LeafConfig> = hung.iter().map(|(_, _, config)| *config).collect();
    let book = farthest_first(&configs, params.variations.max(1) as usize, 0.0, &LeafMetric);

    let geometry = book
        .representatives
        .iter()
        .enumerate()
        .map(|(index, config)| {
            let mesh = blade_mesh(config)
                .with_context(|| format!("leaf shape {} could not be built", config.outline))?;
            Ok(library.part(PART, index, mesh.to_mesh(), surface(config.outline)))
        })
        .collect::<anyhow::Result<Vec<Geometry>>>()?;

    for ((_, entity, _), drew) in hung.iter().zip(&book.assignment) {
        commands.entity(*entity).insert(geometry[*drew as usize].clone());
    }
    Ok(())
}

/// A blade, shaded off the drawing it was cut from rather than off which
/// representative it happens to be, so one shape is one green however the
/// budget is spent.
fn surface(outline: u32) -> Surface {
    material::FOLIAGE.double_sided(color::shade(
        color::srgb(color::LEAF),
        &mut Rng::new(color::COLOR_STREAM ^ outline as u64),
    ))
}

// ─── UI ─────────────────────────────────────────────────────────────

pub fn ui() -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            label_small("Leaf variations"),
            (
                @FeathersSlider { @min: 1.0, @max: 5.0, @value: 5.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<LeafParams>| {
                    params.variations = change.value.round().max(1.0) as u32;
                })
            ),
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
    use crate::elements::util::testing::{self, bounds, face_normal, faces, organs};
    use crate::scene::Prototypes;

    fn params() -> LeafParams {
        LeafParams::default()
    }

    /// Every drawn shape, at this resolution.
    fn blades(params: &LeafParams) -> Vec<MeshData> {
        (0..SHAPES)
            .map(|i| {
                blade_mesh(&LeafConfig::new(params, i))
                    .unwrap_or_else(|e| panic!("leaf_{}: {e:#}", i + 1))
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
        assert_eq!(SHAPES, 5, "five shapes are committed");
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
        let coarse = blades(&LeafParams { detail: 8, ..params() });
        let fine = blades(&LeafParams { detail: 400, ..params() });
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
        let mesh = blade_mesh(&LeafConfig::new(&LeafParams { detail: 0, ..params() }, 0))
            .expect("a leaf at the slider's bottom stop still builds");
        assert!(mesh.points.iter().flatten().all(|c| c.is_finite()));
        assert!(!mesh.face_vertex_counts.is_empty());
    }

    /// Every drawing survives the budget: the outline is categorical in the
    /// metric, so a budget that can hold five shapes holds all five however
    /// unevenly the canopy drew them.
    ///
    /// The failure this catches is a metric that treats the outline as a
    /// number — four blades of shape 0 and one of shape 4 would then look like
    /// one tight cluster and a far outlier, and the rare shapes would vanish.
    #[test]
    fn every_drawn_shape_survives_a_budget_that_can_hold_it() {
        let params = params();
        let mut canopy: Vec<LeafConfig> = vec![LeafConfig::new(&params, 0); 200];
        canopy.extend((1..SHAPES).map(|i| LeafConfig::new(&params, i)));

        let book = farthest_first(&canopy, SHAPES, 0.0, &LeafMetric);
        let kept: std::collections::BTreeSet<u32> =
            book.representatives.iter().map(|c| c.outline).collect();
        assert_eq!(kept.len(), SHAPES, "all five drawings kept, got {kept:?}");

        // And a budget too small to hold them spends what it has on distinct
        // shapes rather than on near-duplicates of one.
        let book = farthest_first(&canopy, 3, 0.0, &LeafMetric);
        let kept: std::collections::BTreeSet<u32> =
            book.representatives.iter().map(|c| c.outline).collect();
        assert_eq!(kept.len(), 3, "three distinct shapes, got {kept:?}");
    }

    /// End to end: every leaf the shoots hung comes out carrying a blade, and
    /// the whole canopy is drawn from at most the budget's worth of meshes.
    #[test]
    fn every_hung_leaf_draws_a_blade_from_the_library() {
        let app = testing::grown(VineyardParams::default());
        let blades: Vec<(&String, &crate::scene::Part)> = app
            .world()
            .resource::<Prototypes>()
            .iter()
            .filter(|(name, _)| name.starts_with(&format!("{PART}_")))
            .collect();

        assert_eq!(blades.len(), SHAPES, "the default budget keeps every shape");
        for (i, (_, a)) in blades.iter().enumerate() {
            assert!(a.double_sided, "a blade is lit from underneath too");
            for (_, b) in blades.iter().skip(i + 1) {
                assert_ne!(a.color, b.color, "two blades came out the same green");
            }
        }

        let mut app = app;
        let leaves = organs::<LeafConfig>(app.world_mut());
        assert!(leaves.len() > 1000, "the fixture hung leaves, got {}", leaves.len());
        assert!(
            leaves.iter().map(|l| l.config.outline).collect::<std::collections::BTreeSet<_>>()
                == (0..SHAPES as u32).collect(),
            "and the shoots picked from every shape"
        );
    }

    /// A leaf has no children, so it is the geometry prim itself rather than
    /// an `Xform` over one — which is what halves the prim count of a scene
    /// that is mostly leaves.
    #[test]
    fn a_leaf_carries_its_geometry_rather_than_wrapping_it() {
        let mut app = testing::grown(VineyardParams::default());
        let mut query = app
            .world_mut()
            .query_filtered::<(Entity, Option<&Children>), With<LeafConfig>>();
        let (entity, children) = query.iter(app.world()).next().expect("some leaf was hung");

        assert!(children.is_none_or(|c| c.is_empty()), "a blade is a leaf of the tree");
        assert!(
            app.world().entity(entity).contains::<crate::scene::UsdReference>(),
            "and references its blade directly"
        );
    }
    /// The layer owns its slice of the library and rebuilds it from scratch,
    /// so a second pass must leave exactly what the first one did — and a
    /// lowered budget must leave less.
    #[test]
    fn rebuilding_rewrites_the_library_rather_than_adding_to_it() {
        let blades = |app: &App| {
            app.world()
                .resource::<Prototypes>()
                .iter()
                .filter(|(name, _)| name.starts_with(&format!("{PART}_")))
                .count()
        };

        let mut app = testing::grown(VineyardParams::default());
        assert_eq!(blades(&app), SHAPES);

        app.world_mut().resource_mut::<LeafParams>().set_changed();
        app.update();
        assert_eq!(blades(&app), SHAPES, "a rebuild does not double up");

        app.world_mut().resource_mut::<LeafParams>().variations = 2;
        app.update();
        assert_eq!(blades(&app), 2, "and the three it stopped using are gone");
    }
}
