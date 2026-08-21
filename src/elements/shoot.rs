//! Shoot element — one season's green growth off a spur.
//!
//! A spur-pruned vine is cut back to short stubs each winter, each holding a
//! bud or two. In spring those buds push **shoots**: slender green canes that
//! leave the bud sideways, turn up within a few centimeters, and then grow
//! straight for the sky. Everything else the canopy is made of — leaves,
//! tendrils, bunches — hangs off them.
//!
//! # Local frame
//!
//! A shoot is authored **with its base at the origin, growing along +X, and
//! turning up to +Z** inside [`BEND_RADIUS`]:
//!
//! ```text
//!        │  ← the rise, up to `length`
//!        │
//!        ╭  ← the bend, a quarter turn of BEND_RADIUS
//!   ─────╯
//!   ↑
//!   the bud, at the origin, with the strand starting a little behind it
//! ```
//!
//! That is what makes placing one cheap: whoever owns the wood picks a point
//! on a spur and a yaw, and the shoot leaves it sideways at that bearing and
//! stands up on its own. No frame has to be transported along the spur.
//!
//! # One subtree
//!
//! Just the prototype meshes under [`PROTOTYPE`]. Where shoots *sit* is
//! [`vine`](super::vine)'s business — a shoot grows from a spur, and only the
//! vine knows where its spurs ended up.

use std::f64::consts::{FRAC_PI_2, TAU};

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use nalgebra::Point3;
use usd_bevy::authoring::{define_prim, remove_prim};
use usd_bevy::live::LiveStage;

use super::util::strand::{Bark, Strand, strand_mesh};
use super::util::usd::{author_mesh, merge_meshes};
use super::{Grow, Rng};

/// The prototype library this element owns: one `Var_<i>` mesh per variation.
pub const PROTOTYPE: &str = "/Vineyard/parts/Shoot";

// ─── Shape constants ────────────────────────────────────────────────

/// How far back behind the bud the strand starts, so a shoot placed on a spur
/// interpenetrates the wood instead of butting against it — the same trick
/// [`vine`](super::vine) uses for its own spurs.
const SHOOT_EMBED: f64 = 0.015;

/// Radius of the quarter turn from +X to +Z.
///
/// Wide enough to read as an arch rather than an elbow: a shoot leaves its bud
/// pointing outward and comes up over several centimeters, and the sweep is
/// what says "grown" rather than "assembled". It is also the only sharply
/// curved part of a shoot, so it is what sets how much ring density the whole
/// strand needs — see [`ShootParams::detail`].
const BEND_RADIUS: f64 = 0.045;

/// Control points around the bend. Four puts one at each of 0°, 30°, 60° and
/// 90°, which is enough for the cubic fit to sit on the arc rather than cut
/// the corner.
const BEND_NODES: usize = 4;

/// Spacing of the control points up the straight run.
const RISE_STEP: f64 = 0.08;

/// Tip radius as a fraction of the radius at the bud.
const TIP_TAPER: f64 = 0.45;

/// The S the lean rides on, as a fraction of the lean itself, and how many
/// times it crosses over across the rise. A shoot that only bowed one way
/// would read as bent rather than grown.
const SHOOT_SWAY: f64 = 0.35;
const SHOOT_WAVES: f64 = 1.25;

// ─── Params ─────────────────────────────────────────────────────────

#[derive(Resource, Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct ShootParams {
    /// How many differently-seeded shoots to author as prototypes.
    pub variations: u32,
    /// Drives the prototype shapes.
    pub seed: u64,
    /// Bud to tip, in meters — how tall a shoot stands above the spur it grew
    /// from. Whoever places one varies this a little per shoot.
    pub length: f32,
    /// Radius at the bud, in meters.
    pub radius: f32,
    /// How far the tip wanders off vertical, in meters.
    pub lean: f32,
    /// Vertices around the tube.
    pub sides: u32,
    /// Rings per meter along the tube.
    ///
    /// Higher here than anywhere else in the scene, and cheaper than it looks:
    /// a shoot is a *shared prototype*, so this buys ring density for the whole
    /// vineyard at the cost of a handful of meshes. It has to be high because
    /// stations are spaced by arc length and [`BEND_RADIUS`] packs a quarter
    /// turn into a few centimeters — at the density a trunk is happy with, the
    /// bend comes out a chamfer.
    pub detail: u32,
}

impl Default for ShootParams {
    fn default() -> Self {
        Self {
            variations: 4,
            seed: 0,
            length: 0.75,
            radius: 0.006,
            lean: 0.06,
            sides: 6,
            detail: 40,
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<ShootParams>().add_systems(
        PreUpdate,
        author_prototypes
            .in_set(Grow::Prototypes)
            .run_if(resource_changed::<ShootParams>),
    );
}

// ─── Shape ──────────────────────────────────────────────────────────

/// The tallest a shoot can be asked to stand: below this the bend has nowhere
/// to finish, and the strand would double back on itself.
fn height(params: &ShootParams) -> f64 {
    (params.length as f64).max(BEND_RADIUS * 1.5)
}

/// Lateral offset of a shoot's axis at fraction `f` up its straight run.
///
/// Grows as `f²` from nothing at the top of the bend: a shoot is a cantilever,
/// clamped at the bud and free at the tip, so its wander accumulates upward.
/// That is the opposite of [`trunk_axis`](super::vine), which is pinned at
/// *both* ends because the cordons attach at the top — nothing attaches to the
/// tip of a shoot.
fn shoot_axis(f: f64, lean: f64, azimuth: f64, phase: f64) -> (f64, f64) {
    let reach = lean * f * f;
    let sway = lean * SHOOT_SWAY * f * (TAU * SHOOT_WAVES * f + phase).sin();
    (
        reach * azimuth.cos() - sway * azimuth.sin(),
        reach * azimuth.sin() + sway * azimuth.cos(),
    )
}

/// Heights to put control points at up the straight run: an even spread, and
/// the tip itself whatever the spacing worked out to.
fn rise_nodes(height: f64) -> Vec<f64> {
    let mut nodes = Vec::new();
    let mut z = BEND_RADIUS + RISE_STEP;
    while z < height - RISE_STEP * 0.5 {
        nodes.push(z);
        z += RISE_STEP;
    }
    nodes.push(height);
    nodes
}

/// One shoot, in the prototype's local frame.
///
/// The *draw order* is part of this element's output: the lean's bearing, its
/// wave's phase, then how much of the nominal lean this one actually takes.
fn shoot_strand(params: &ShootParams, rng: &mut Rng) -> Strand {
    let height = height(params);
    let azimuth = rng.unit() * TAU;
    let phase = rng.unit() * TAU;
    let lean = params.lean as f64 * rng.range(0.5, 1.0);

    // Starts behind the bud, arcs up over the bend, then rises. The bend's
    // first point *is* the origin, so the embedded stub and the arc share a
    // tangent and the shoot leaves the wood pointing along +X.
    let mut points = vec![Point3::new(-SHOOT_EMBED, 0.0, 0.0)];
    for i in 0..BEND_NODES {
        let angle = FRAC_PI_2 * i as f64 / (BEND_NODES - 1) as f64;
        points.push(Point3::new(
            BEND_RADIUS * angle.sin(),
            0.0,
            BEND_RADIUS * (1.0 - angle.cos()),
        ));
    }
    for z in rise_nodes(height) {
        let f = (z - BEND_RADIUS) / (height - BEND_RADIUS);
        let (dx, dy) = shoot_axis(f, lean, azimuth, phase);
        points.push(Point3::new(BEND_RADIUS + dx, dy, z));
    }

    // Tapered by height rather than by point index: the bend's points are
    // centimeters apart and the rise's are decimeters, so an index taper would
    // spend the whole taper on the bend.
    let radii = points
        .iter()
        .map(|p| {
            let t = (p.z / height).clamp(0.0, 1.0);
            params.radius as f64 * (1.0 + (TIP_TAPER - 1.0) * t)
        })
        .collect();

    // No bark: a shoot is a smooth green stem, and ridges on a six-millimeter
    // tube read as noise rather than texture.
    Strand::new(
        points,
        radii,
        (params.sides as usize).max(3),
        params.detail as f64,
        Bark::none(),
    )
}

// ─── Authoring ──────────────────────────────────────────────────────

/// Authors one mesh per variation under [`PROTOTYPE`].
///
/// `pub` so [`vine`](super::vine) can order its own authoring after this one:
/// the vine references these prims and counts them off the stage, and
/// [`Grow`] chains the *sets*, not the systems inside one.
pub fn author_prototypes(live: NonSend<LiveStage>, params: Res<ShootParams>) -> Result<()> {
    let stage = &live.stage;
    remove_prim(stage, PROTOTYPE)?;
    define_prim(stage, PROTOTYPE, "Scope")?;

    for i in 0..params.variations.max(1) {
        // Mixing rather than adding, so neighbouring seeds give unrelated
        // shoots instead of the same shoot shifted by one variation.
        let seed = params.seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let strand = shoot_strand(&params, &mut Rng::new(seed));
        author_mesh(
            stage,
            &format!("{PROTOTYPE}/Var_{i}"),
            &merge_meshes(&[strand_mesh(&strand)?]),
        )?;
    }
    Ok(())
}

// ─── UI ─────────────────────────────────────────────────────────────

pub fn ui() -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            label_small("Shoot length"),
            (
                @FeathersSlider { @min: 0.1, @max: 1.6, @value: 0.75 }
                SliderStep(0.05)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<ShootParams>| {
                    params.length = change.value;
                })
            ),
            label_small("Shoot radius"),
            (
                @FeathersSlider { @min: 0.002, @max: 0.015, @value: 0.006 }
                SliderStep(0.001)
                SliderPrecision(3)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<ShootParams>| {
                    params.radius = change.value;
                })
            ),
            label_small("Shoot lean"),
            (
                @FeathersSlider { @min: 0.0, @max: 0.25, @value: 0.06 }
                SliderStep(0.01)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<ShootParams>| {
                    params.lean = change.value;
                })
            ),
            label_small("Shoot sides"),
            (
                @FeathersSlider { @min: 3.0, @max: 12.0, @value: 6.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<ShootParams>| {
                    params.sides = change.value.round().max(3.0) as u32;
                })
            ),
            label_small("Shoot detail"),
            (
                @FeathersSlider { @min: 8.0, @max: 90.0, @value: 40.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<ShootParams>| {
                    params.detail = change.value.round().max(4.0) as u32;
                })
            ),
            label_small("Shoot variations"),
            (
                @FeathersSlider { @min: 1.0, @max: 8.0, @value: 4.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<ShootParams>| {
                    params.variations = change.value.round().max(1.0) as u32;
                })
            ),
            label_small("Shoot seed"),
            (
                @FeathersSlider { @min: 0.0, @max: 64.0, @value: 0.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<ShootParams>| {
                    params.seed = change.value.round().max(0.0) as u64;
                })
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::VineyardParams;

    fn params() -> ShootParams {
        ShootParams::default()
    }

    fn mesh(params: &ShootParams, seed: u64) -> crate::elements::util::usd::MeshData {
        strand_mesh(&shoot_strand(params, &mut Rng::new(seed))).expect("every shoot skins")
    }

    fn bounds(mesh: &crate::elements::util::usd::MeshData, axis: usize) -> (f32, f32) {
        mesh.points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(p[axis]), hi.max(p[axis]))
        })
    }

    /// The whole placement contract: a shoot leaves its bud along +X and ends
    /// up going +Z. If either end drifted, every shoot on every spur would
    /// point somewhere the vine didn't ask for.
    #[test]
    fn a_shoot_leaves_along_x_and_ends_going_up() {
        let strand = shoot_strand(&params(), &mut Rng::new(1));

        let base = strand.points[0];
        assert!(base.x < 0.0, "starts behind the bud, got {base:?}");
        assert!(base.z.abs() < 1e-12, "and at the height of the bud");

        let leaving = strand.points[1] - strand.points[0];
        assert!(
            leaving.x > 0.0 && leaving.x > leaving.z.abs() * 10.0,
            "leaves sideways, got {leaving:?}"
        );

        let last = strand.points.len() - 1;
        let rising = strand.points[last] - strand.points[last - 1];
        assert!(
            rising.z > 0.0 && rising.z > rising.xy().norm() * 3.0,
            "and finishes going up, got {rising:?}"
        );
    }

    #[test]
    fn a_shoot_stands_as_tall_as_its_length() {
        let p = params();
        let mesh = mesh(&p, 1);

        let (z0, z1) = bounds(&mesh, 2);
        assert!(z0 > -p.radius * 2.0, "nothing below the bud, got {z0}");
        assert!(
            (z1 - p.length).abs() < p.radius * 2.0,
            "the tip lands at `length`, got {z1}"
        );

        // And it stays a narrow thing: the bend's reach plus the lean, no more.
        let slack = (BEND_RADIUS + p.lean as f64 + p.radius as f64 * 2.0) as f32;
        let (x0, x1) = bounds(&mesh, 0);
        assert!(x0 > -slack && x1 < slack, "{x0}..{x1}");
        let (y0, y1) = bounds(&mesh, 1);
        assert!(y0 > -slack && y1 < slack, "{y0}..{y1}");
    }

    /// A shoot shorter than its own bend has nowhere to put the rise. It has
    /// to clamp rather than fold back on itself, which would give curvo a rail
    /// that doubles over and a mesh full of NaN.
    #[test]
    fn a_shoot_shorter_than_its_bend_still_builds() {
        let p = ShootParams {
            length: 0.001,
            ..params()
        };
        let mesh = mesh(&p, 1);
        assert!(!mesh.points.is_empty());
        assert!(mesh.points.iter().flatten().all(|c| c.is_finite()));
    }

    #[test]
    fn a_shoot_with_no_lean_is_straight_above_the_bend() {
        let p = ShootParams {
            lean: 0.0,
            ..params()
        };
        let strand = shoot_strand(&p, &mut Rng::new(3));
        for point in strand.points.iter().filter(|p| p.z > BEND_RADIUS) {
            assert!(
                (point.x - BEND_RADIUS).abs() < 1e-12 && point.y.abs() < 1e-12,
                "the rise is plumb without a lean, got {point:?}"
            );
        }
    }

    #[test]
    fn shoot_strands_are_reproducible() {
        assert_eq!(mesh(&params(), 9).points, mesh(&params(), 9).points);
    }

    #[test]
    fn variations_differ_from_one_another() {
        assert_ne!(mesh(&params(), 1).points, mesh(&params(), 2).points);
    }

    #[test]
    fn authors_one_prototype_per_variation() {
        let stage = crate::generate::generate_stage(&VineyardParams {
            shoot: ShootParams {
                variations: 3,
                ..params()
            },
            ..default()
        })
        .unwrap();

        for i in 0..3 {
            assert!(
                usd_bevy::authoring::prim_exists(&stage, &format!("{PROTOTYPE}/Var_{i}")),
                "Var_{i} authored"
            );
        }
        assert!(!usd_bevy::authoring::prim_exists(
            &stage,
            &format!("{PROTOTYPE}/Var_3")
        ));
    }

    /// Re-authoring must not leave prototypes from a larger previous count
    /// behind — the element owns its subtree and clears it first.
    #[test]
    fn shrinking_the_count_drops_stale_prototypes() {
        let stage = crate::stage::new_stage("shoot.usda").unwrap();
        let mut world = World::new();
        world.insert_non_send(LiveStage::new(stage.clone()));
        world.insert_resource(ShootParams {
            variations: 4,
            ..params()
        });
        let mut schedule = Schedule::default();
        schedule.add_systems(author_prototypes);
        schedule.run(&mut world);
        assert!(usd_bevy::authoring::prim_exists(
            &stage,
            &format!("{PROTOTYPE}/Var_3")
        ));

        world.insert_resource(ShootParams {
            variations: 2,
            ..params()
        });
        schedule.run(&mut world);
        assert!(
            !usd_bevy::authoring::prim_exists(&stage, &format!("{PROTOTYPE}/Var_3")),
            "stale prototype removed on re-author"
        );
    }
}
