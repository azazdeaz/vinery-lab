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
//! The prototype library under [`PROTOTYPE`]. Each `Var_<i>` is an `Xform`
//! over a `Stem` mesh plus its leaves, placed by whichever path
//! [`place::Style`] names: a prim each, referencing a [`leaf`](super::leaf)
//! prototype, or one [`LEAVES`] instancer holding the lot.
//!
//! Leaves live *inside* the prototype, so a whole vineyard's canopy costs a
//! handful of placements on the stage rather than one per leaf. The trade is
//! that every instance of a given variation carries an identical canopy, and
//! variety comes from the product of the variation counts across nesting
//! levels — see the "Variations" section of `README.md`.
//!
//! Where shoots *sit* is [`vine`](super::vine)'s business — a shoot grows from
//! a spur, and only the vine knows where its spurs ended up. By the same rule,
//! where a leaf sits is settled here: only a shoot knows where its nodes are.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use nalgebra::Point3;
use usd_bevy::authoring::{define_prim, remove_prim};
use usd_bevy::live::LiveStage;

use super::leaf;
use super::util::place::{self, Placement};
use super::util::strand::{Bark, Strand, strand_mesh};
use super::util::usd::{author_mesh, merge_meshes};
use super::{Grow, Rng};

/// The prototype library this element owns: one `Var_<i>` per variation, each
/// an `Xform` over its stem and its leaves.
pub const PROTOTYPE: &str = "/Vineyard/parts/Shoot";

/// Name the `PointInstancer` holding a shoot's leaves takes, when they are
/// instanced rather than reference-placed.
pub const LEAVES: &str = "Leaves";

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

/// Length of the bend along its own curve: a quarter turn of [`BEND_RADIUS`].
const BEND_ARC: f64 = BEND_RADIUS * FRAC_PI_2;

// ─── Leaf constants ─────────────────────────────────────────────────

/// Salt splitting the leaves' randomness off the stem's, so tuning the canopy
/// never reshapes the shoot underneath it — the same split
/// [`vine`](super::vine) keeps between its wood and its shoots.
const LEAF_STREAM: u64 = 0x2545_F491_4F6C_DD1D;

/// Below this the station loop would never terminate, so a shoot this closely
/// noded carries no leaves at all. That is also how the canopy gets turned off.
const MIN_INTERNODE: f64 = 0.005;

/// Bare tip left past the last node. The growing point itself is a curl of
/// scale leaves too small to be worth a prototype.
const TIP_CLEARANCE: f64 = 0.02;

/// How far a node slides off its nominal station, as a fraction of the
/// internode. Real internodes are not a fixed length, and a perfectly even
/// ladder is the one thing that reads as generated.
const STATION_JITTER: f64 = 0.3;

/// Bearing wander off the rank a node belongs to, in radians.
const LEAF_SPREAD: f64 = 0.25;

/// How far the two ranks turn per node, past the half turn that defines them.
///
/// Grapevine phyllotaxis is *distichous* — leaves alternate 180°, in two ranks
/// up opposite sides of the shoot. Exactly 180° over ten nodes comes out
/// perfectly coplanar, which no shoot is, so the ranks are given a slow twist.
const PHYLLOTAXY_DRIFT: f64 = 0.08;

/// Twist about a leaf's own long axis, in radians.
const LEAF_ROLL: f64 = 0.35;

/// Spread of the droop about [`ShootParams::leaf_droop`], as a fraction.
const LEAF_DROOP_JITTER: f64 = 0.3;

/// How much a leaf's size varies about what its age asks for, as a fraction.
const LEAF_VIGOUR: f64 = 0.12;

/// Size of the youngest leaf a shoot carries, relative to a full-grown one.
const TIP_SCALE: f64 = 0.15;

/// How far below the growing point a shoot's leaves are still expanding, in
/// meters.
///
/// A *length* rather than a number of nodes, and that is the point: a shoot
/// extends at a roughly steady rate and a leaf takes about a month to reach
/// full size, so the growing tip is the same span of shoot whatever
/// [`ShootParams::internode`] is set to. Counting nodes instead would make the
/// canopy's age gradient change every time its density did.
const EXPANDING_REACH: f64 = 0.25;

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
    /// Distance between leaf nodes up the shoot, in meters.
    ///
    /// The count-like knob, the way [`shoots_per_spur`] is one level up: how
    /// many leaves a shoot carries is a fact about the shoot rather than about
    /// a leaf, and a spacing says it in the unit a viticulturist would. Below
    /// [`MIN_INTERNODE`] a shoot carries no leaves at all, which is how the
    /// canopy is turned off.
    ///
    /// [`shoots_per_spur`]: super::vine::VineParams::shoots_per_spur
    pub internode: f32,
    /// How far a full-grown blade pitches below horizontal, in radians.
    ///
    /// Rides on each leaf's own maturity, so the mature blades down the shoot
    /// hang at about this and the small ones at the tip stand nearly straight
    /// out — which is what a petiole holding a tenth of the weight does.
    pub leaf_droop: f32,
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
            internode: 0.07,
            leaf_droop: 0.35,
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<ShootParams>().add_systems(
        PreUpdate,
        author_prototypes
            .in_set(Grow::Prototypes)
            // Places the leaf prototypes and counts them off the stage, so
            // they have to be there first — the same debt `vine` owes this
            // system. `Grow` chains the *sets*; the systems inside one are
            // unordered until something says otherwise.
            .after(leaf::author_prototypes)
            .run_if(
                resource_changed::<ShootParams>
                    .or_else(resource_changed::<leaf::LeafParams>)
                    .or_else(resource_changed::<place::Style>),
            ),
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

/// The curve a shoot's tube is skinned onto.
///
/// Kept as a thing in its own right so leaves can be put back on the *same*
/// curve rather than on a resampling of the mesh — the way a
/// [`Spur`](super::vine) is a bare axis with an `at`, precisely so that placing
/// on it needs no frame transported along it.
#[derive(Clone, Copy, Debug)]
struct ShootAxis {
    height: f64,
    lean: f64,
    azimuth: f64,
    phase: f64,
}

impl ShootAxis {
    /// The *draw order* is part of this element's output: the lean's bearing,
    /// its wave's phase, then how much of the nominal lean this one actually
    /// takes.
    fn new(params: &ShootParams, rng: &mut Rng) -> Self {
        Self {
            height: height(params),
            azimuth: rng.unit() * TAU,
            phase: rng.unit() * TAU,
            lean: params.lean as f64 * rng.range(0.5, 1.0),
        }
    }

    /// Bud to tip along the curve.
    ///
    /// The rise is measured by height rather than by true arc length. The lean
    /// buys it a fraction of a percent, and taking the height keeps the top
    /// node a fixed distance below the tip however far this shoot happened to
    /// wander — which is what the age gradient is keyed on.
    fn length(&self) -> f64 {
        BEND_ARC + (self.height - BEND_RADIUS)
    }

    /// The point on the rise at height `z`.
    ///
    /// The rise's own parameter. [`at`](Self::at) reaches it through arc
    /// length, but the stem's control points are spaced by height and go
    /// straight here, so that a reparameterization's rounding never reaches the
    /// mesh.
    fn at_height(&self, z: f64) -> Point3<f64> {
        let f = (z - BEND_RADIUS) / (self.height - BEND_RADIUS);
        let (dx, dy) = shoot_axis(f, self.lean, self.azimuth, self.phase);
        Point3::new(BEND_RADIUS + dx, dy, z)
    }

    /// The point `s` meters along the axis from the bud.
    fn at(&self, s: f64) -> Point3<f64> {
        if s < BEND_ARC {
            bend_point(s / BEND_RADIUS)
        } else {
            self.at_height(BEND_RADIUS + (s - BEND_ARC))
        }
    }
}

/// The point on the bend `angle` radians round from the bud.
///
/// The bend is the same quarter circle on every shoot — the lean only starts
/// accumulating above it — so this needs nothing off an axis.
fn bend_point(angle: f64) -> Point3<f64> {
    Point3::new(
        BEND_RADIUS * angle.sin(),
        0.0,
        BEND_RADIUS * (1.0 - angle.cos()),
    )
}

/// One shoot's stem, in the prototype's local frame.
fn shoot_strand(axis: &ShootAxis, params: &ShootParams) -> Strand {
    let height = axis.height;

    // Starts behind the bud, arcs up over the bend, then rises. The bend's
    // first point *is* the origin, so the embedded stub and the arc share a
    // tangent and the shoot leaves the wood pointing along +X.
    let mut points = vec![Point3::new(-SHOOT_EMBED, 0.0, 0.0)];
    for i in 0..BEND_NODES {
        points.push(bend_point(FRAC_PI_2 * i as f64 / (BEND_NODES - 1) as f64));
    }
    for z in rise_nodes(height) {
        points.push(axis.at_height(z));
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

// ─── Leaves ─────────────────────────────────────────────────────────

/// How big a leaf `below_tip` meters short of the growing point has grown to,
/// as a fraction of a full-grown one.
///
/// A shoot's leaves are laid down in order and expand for about a month while
/// the shoot keeps extending past them, so age reads straight off position:
/// the ones nearest the tip are the newest and the smallest, and everything
/// below [`EXPANDING_REACH`] is done growing.
///
/// Smoothstep rather than a straight ramp because expansion is sigmoid in
/// time — slow to unfold, fastest in the middle, then flat — and because the
/// flat end is what makes "mature" mean *one* size rather than a size that
/// keeps creeping up the whole length of the shoot.
fn leaf_scale(below_tip: f64) -> f64 {
    let t = (below_tip / EXPANDING_REACH).clamp(0.0, 1.0);
    TIP_SCALE + (1.0 - TIP_SCALE) * (t * t * (3.0 - 2.0 * t))
}

/// Where this shoot's leaves sit, in the prototype's local frame.
///
/// Nodes climb the shoot from the top of the bend to a little short of the
/// tip. The bend is left bare because that is where a shoot is still lying
/// sideways, and a leaf's whole orientation here is a bearing about Z plus a
/// droop — which only means "around the stem" once the stem is standing up.
///
/// # Two rules about the randomness
///
/// It draws from a **stream of its own**, salted with [`LEAF_STREAM`], so that
/// tuning the canopy never reshapes the stem underneath it — the same split
/// [`vine`](super::vine) keeps between its wood and its shoots.
///
/// Unlike a spur's fixed three buds, though, there is no slot set to keep
/// aligned: the station list *is* the count, so changing
/// [`internode`](ShootParams::internode) re-rolls the whole canopy rather than
/// adding to it. That is the honest behaviour for a spacing, and it is why
/// this does not go through the draw-everything-anyway discipline
/// [`shoot_placements`](super::vine) needs.
fn leaf_placements(
    params: &ShootParams,
    axis: &ShootAxis,
    variations: usize,
    seed: u64,
) -> Vec<(String, Placement)> {
    let variations = variations.max(1);
    let internode = params.internode as f64;
    let length = axis.length();
    if internode < MIN_INTERNODE {
        return Vec::new();
    }

    let mut rng = Rng::new(seed ^ LEAF_STREAM);
    // Drawn once, so the two ranks are not lined up with the shoot's lean.
    let bearing = rng.unit() * TAU;

    let mut placements = Vec::new();
    let mut station = BEND_ARC;
    let mut node = 0usize;
    while station <= length - TIP_CLEARANCE {
        // Draw order: the node's slide up the shoot, its bearing's wander, its
        // droop, its twist, its vigour, then which blade it drew.
        let slide = rng.range(-STATION_JITTER, STATION_JITTER) * internode;
        let turn = rng.range(-LEAF_SPREAD, LEAF_SPREAD);
        let sag = rng.range(1.0 - LEAF_DROOP_JITTER, 1.0 + LEAF_DROOP_JITTER);
        let roll = rng.range(-LEAF_ROLL, LEAF_ROLL);
        let vigour = rng.range(1.0 - LEAF_VIGOUR, 1.0 + LEAF_VIGOUR);
        let pick = rng.unit();

        let at = (station + slide).clamp(BEND_ARC, length);
        // On the centerline rather than out at the stem's surface: that buries
        // the petiole's free end under a few millimeters of stem, which is the
        // trick `SHOOT_EMBED` already uses one level up and is what guarantees
        // no gap however the blade ends up turned.
        let position = axis.at(at);
        let maturity = leaf_scale(length - at);

        placements.push((
            format!("Leaf_{node:02}"),
            Placement {
                position: Vec3::new(position.x as f32, position.y as f32, position.z as f32),
                // Distichous: successive leaves sit half a turn apart, in two
                // ranks up opposite sides of the shoot, drifting slowly so ten
                // of them do not come out coplanar. Wrapped, because the sum
                // runs past a full turn by the fourth node and an authored
                // `rotateXYZ` of 1000° is a thing nobody wants to read.
                yaw: (bearing + (PI + PHYLLOTAXY_DRIFT) * node as f64 + turn).rem_euclid(TAU)
                    as f32,
                // A leaf is drawn flat along +X with its face toward +Z, so
                // the tilt is the whole of its posture: X twists the blade
                // about its own long axis, Y pitches its tip down.
                tilt: Vec2::new(
                    roll as f32,
                    (params.leaf_droop as f64 * maturity * sag) as f32,
                ),
                scale: (maturity * vigour) as f32,
                variation: (pick * variations as f64) as usize % variations,
            },
        ));

        station += internode;
        node += 1;
    }
    placements
}

// ─── Authoring ──────────────────────────────────────────────────────

/// Authors one mesh per variation under [`PROTOTYPE`].
///
/// `pub` so [`vine`](super::vine) can order its own authoring after this one:
/// the vine references these prims and counts them off the stage, and
/// [`Grow`] chains the *sets*, not the systems inside one.
pub fn author_prototypes(
    live: NonSend<LiveStage>,
    params: Res<ShootParams>,
    style: Res<place::Style>,
) -> Result<()> {
    let stage = &live.stage;
    remove_prim(stage, PROTOTYPE)?;
    define_prim(stage, PROTOTYPE, "Scope")?;

    // Counted off the stage rather than read from `LeafParams`, because
    // elements compose by prim path only.
    let leaf_variations = place::prototype_count(stage, leaf::PROTOTYPE);

    for i in 0..params.variations.max(1) {
        // Mixing rather than adding, so neighbouring seeds give unrelated
        // shoots instead of the same shoot shifted by one variation.
        let seed = params.seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let axis = ShootAxis::new(&params, &mut Rng::new(seed));
        let strand = shoot_strand(&axis, &params);

        // An `Xform` over a `Stem` mesh rather than a bare `Mesh`: the leaves
        // are references to another element's prototypes, so a shoot has to be
        // a prim that can *have* children — the same reason a vine is an
        // `Xform` over its wood.
        let variation = format!("{PROTOTYPE}/Var_{i}");
        define_prim(stage, &variation, "Xform")?;
        author_mesh(
            stage,
            &format!("{variation}/Stem"),
            &merge_meshes(&[strand_mesh(&strand)?]),
        )?;

        if leaf_variations > 0 {
            let leaves = leaf_placements(&params, &axis, leaf_variations, seed);
            place::place(
                stage,
                *style,
                &variation,
                LEAVES,
                leaf::PROTOTYPE,
                leaf_variations,
                &leaves,
            )?;
        }
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
            label_small("Leaf spacing"),
            (
                @FeathersSlider { @min: 0.0, @max: 0.25, @value: 0.07 }
                SliderStep(0.01)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<ShootParams>| {
                    params.internode = change.value.max(0.0);
                })
            ),
            label_small("Leaf droop"),
            (
                @FeathersSlider { @min: 0.0, @max: 1.2, @value: 0.35 }
                SliderStep(0.05)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<ShootParams>| {
                    params.leaf_droop = change.value;
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
    use crate::elements::util::place::prototype_count;
    use crate::elements::util::testing::{Authoring, bounds};

    fn params() -> ShootParams {
        ShootParams::default()
    }

    fn axis(params: &ShootParams, seed: u64) -> ShootAxis {
        ShootAxis::new(params, &mut Rng::new(seed))
    }

    fn mesh(params: &ShootParams, seed: u64) -> crate::elements::util::usd::MeshData {
        strand_mesh(&shoot_strand(&axis(params, seed), params)).expect("every shoot skins")
    }

    /// One shoot's canopy, with the axis it was hung on — every leaf test
    /// needs both, because a placement only means anything against the curve
    /// it was placed on.
    fn leaves(params: &ShootParams, seed: u64) -> (ShootAxis, Vec<(String, Placement)>) {
        let axis = axis(params, seed);
        let placements = leaf_placements(params, &axis, leaf::VARIATIONS, seed);
        (axis, placements)
    }

    /// The whole placement contract: a shoot leaves its bud along +X and ends
    /// up going +Z. If either end drifted, every shoot on every spur would
    /// point somewhere the vine didn't ask for.
    #[test]
    fn a_shoot_leaves_along_x_and_ends_going_up() {
        let strand = shoot_strand(&axis(&params(), 1), &params());

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
        let strand = shoot_strand(&axis(&p, 3), &p);
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
        assert_eq!(leaves(&params(), 9).1, leaves(&params(), 9).1);
        assert_ne!(leaves(&params(), 9).1, leaves(&params(), 10).1);
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

        assert_eq!(
            prototype_count(&stage, PROTOTYPE),
            3,
            "three variations authored, and nothing past them"
        );
    }

    /// Nodes climb the shoot in order, start where it has finished standing
    /// up, and stop short of the growing point. The bend is left bare on
    /// purpose: a leaf's posture here is a bearing about Z, which only means
    /// "around the stem" once the stem is vertical.
    #[test]
    fn leaves_climb_the_shoot_from_its_base_to_its_tip() {
        let p = params();
        let (axis, leaves) = leaves(&p, 1);
        assert!(
            leaves.len() > 4,
            "a default shoot is leafy, got {}",
            leaves.len()
        );

        let heights: Vec<f32> = leaves.iter().map(|(_, l)| l.position.z).collect();
        assert!(
            heights.windows(2).all(|w| w[0] < w[1]),
            "they climb rather than double back: {heights:?}"
        );
        assert!(
            heights[0] >= BEND_RADIUS as f32 - 1e-6,
            "the first is at the top of the bend, got {}",
            heights[0]
        );
        assert!(
            *heights.last().unwrap() < (axis.height - TIP_CLEARANCE * 0.5) as f32,
            "and the last stops short of the tip, got {}",
            heights.last().unwrap()
        );
    }

    /// A leaf hangs off the stem's own centerline, not off a resampling of it.
    /// That is the whole reason [`ShootAxis`] was lifted out of
    /// [`shoot_strand`], and the check is exact because both go through
    /// [`ShootAxis::at_height`].
    #[test]
    fn a_leaf_sits_on_the_shoots_axis() {
        let p = params();
        let (axis, leaves) = leaves(&p, 1);
        for (name, leaf) in &leaves {
            let on_axis = axis.at_height(leaf.position.z as f64);
            let off =
                (leaf.position.x as f64 - on_axis.x).hypot(leaf.position.y as f64 - on_axis.y);
            assert!(off < 1e-6, "{name} sits on the axis, off by {off}");
        }
    }

    /// Grapevine phyllotaxis is distichous — successive leaves half a turn
    /// apart, in two ranks up opposite sides. The failure this catches is a
    /// canopy that grew out one side of every shoot, which reads as obviously
    /// wrong and would survive every other test here.
    #[test]
    fn leaves_alternate_around_the_shoot() {
        let p = params();
        let (_, leaves) = leaves(&p, 1);
        let slack = PHYLLOTAXY_DRIFT + LEAF_SPREAD * 2.0;
        for pair in leaves.windows(2) {
            let turn = (pair[1].1.yaw - pair[0].1.yaw) as f64;
            // Wrapped into [0, 2π), so a half turn either way reads as zero.
            let apart = (turn.rem_euclid(TAU) - PI).abs();
            assert!(
                apart <= slack,
                "{} follows {} half a turn round, off by {apart} rad",
                pair[1].0,
                pair[0].0
            );
        }
    }

    /// What the whole fixed-[`AREA`](leaf::AREA) contract was for: a leaf that
    /// has finished growing is placed at about 1.0, so a scale is a size in
    /// meters and nothing downstream has to know which blade it drew.
    #[test]
    fn a_mature_leaf_is_placed_at_about_full_size() {
        assert_eq!(leaf_scale(EXPANDING_REACH), 1.0, "past the growing tip");
        assert_eq!(leaf_scale(0.0), TIP_SCALE, "and at the very tip");

        let p = params();
        let (axis, leaves) = leaves(&p, 1);
        let mature: Vec<f32> = leaves
            .iter()
            .filter(|(_, l)| (axis.length() - l.position.z as f64) > EXPANDING_REACH * 1.5)
            .map(|(_, l)| l.scale)
            .collect();

        assert!(
            mature.len() > 3,
            "most of a shoot is mature, got {mature:?}"
        );
        for scale in &mature {
            assert!(
                (scale - 1.0).abs() <= LEAF_VIGOUR as f32 + 1e-6,
                "a grown leaf is full size give or take its vigour, got {scale} in {mature:?}"
            );
        }
    }

    /// Age reads off position: a shoot lays its leaves down in order and keeps
    /// extending past them, so the ones near the growing point are the newest
    /// and the smallest. Asserted as a trend rather than pair by pair, because
    /// vigour jitters each leaf either way.
    #[test]
    fn leaves_shrink_toward_the_growing_tip() {
        let p = params();
        let (_, leaves) = leaves(&p, 1);
        let scales: Vec<f32> = leaves.iter().map(|(_, l)| l.scale).collect();
        let mean = |s: &[f32]| s.iter().sum::<f32>() / s.len() as f32;

        let (base, tip) = (mean(&scales[..3]), mean(&scales[scales.len() - 3..]));
        assert!(
            tip < base * 0.8,
            "the tip carries the young ones: base {base}, tip {tip}, all {scales:?}"
        );
    }

    /// The canopy's randomness is a stream of its own, so tuning how leafy a
    /// shoot is never re-rolls the stem holding them up.
    #[test]
    fn changing_the_internode_leaves_the_stem_alone() {
        let stem = |internode: f32| {
            let p = ShootParams {
                internode,
                ..params()
            };
            mesh(&p, 7).points
        };
        assert_eq!(stem(0.07), stem(0.0), "same stem, however many leaves");
    }

    /// A spacing too fine to step by is how the canopy is turned off. Without
    /// the guard the station loop would never terminate.
    #[test]
    fn a_shoot_with_no_leaves_asked_for_still_builds() {
        for internode in [0.0, -1.0, MIN_INTERNODE as f32 * 0.5] {
            let p = ShootParams {
                internode,
                ..params()
            };
            assert!(leaves(&p, 1).1.is_empty(), "no leaves at {internode}");
            assert!(!mesh(&p, 1).points.is_empty(), "but still a stem");
        }
    }

    /// A shoot prototype has to be a prim that can *have* children, or the
    /// leaves referenced under it would hang off a `Mesh` no renderer walks
    /// into — the same trap `vine` avoids by wrapping its wood in an `Xform`.
    ///
    /// The wrapper holds whichever way the leaves were placed; the style only
    /// changes what hangs below it, never that there is something to hang.
    #[test]
    fn a_shoot_prototype_is_an_xform_over_its_stem_and_its_leaves() {
        for style in crate::elements::util::testing::STYLES {
            let (stage, _) =
                crate::elements::util::testing::grown(VineyardParams::default(), style);
            let path =
                |suffix: &str| openusd::sdf::path(format!("{PROTOTYPE}/Var_0{suffix}")).unwrap();

            assert!(
                openusd::schemas::geom::Xform::get(&stage, path(""))
                    .unwrap()
                    .is_some(),
                "{style:?}: the variation is an Xform, so it can have children"
            );
            assert!(
                openusd::schemas::geom::Mesh::get(&stage, path(""))
                    .unwrap()
                    .is_none(),
                "{style:?}: and not a Mesh, which would swallow them"
            );
            assert!(usd_bevy::authoring::prim_exists(
                &stage,
                &format!("{PROTOTYPE}/Var_0/Stem")
            ));

            match style {
                place::Style::Referenced => assert!(
                    usd_bevy::authoring::prim_exists(&stage, &format!("{PROTOTYPE}/Var_0/Leaf_00")),
                    "the first node carries a leaf"
                ),
                place::Style::Instanced => {
                    let leaves = openusd::schemas::geom::PointInstancer::get(
                        &stage,
                        path(&format!("/{LEAVES}")),
                    )
                    .unwrap()
                    .expect("the leaves are one instancer, nested inside the prototype");
                    // Nested instancing is where a `prototypes` relationship is
                    // most easily lost — see `place::Style`. Empty here and
                    // every previewed shoot comes out bare.
                    let targets = leaves.prototypes_rel().targets().unwrap();
                    assert!(!targets.is_empty());
                    assert!(
                        targets
                            .iter()
                            .all(|t| usd_bevy::authoring::prim_exists(&stage, t.as_str())),
                        "the nested instancer names leaf prototypes that exist, got {targets:?}"
                    );
                }
            }
        }
    }

    /// Re-authoring must not leave prototypes from a larger previous count
    /// behind — the element owns its subtree and clears it first.
    #[test]
    fn shrinking_the_count_drops_stale_prototypes() {
        let mut authoring = Authoring::new("shoot.usda", author_prototypes);
        authoring.insert(place::Style::default());
        authoring
            .insert(ShootParams {
                variations: 4,
                ..params()
            })
            .run();
        assert!(authoring.has(&format!("{PROTOTYPE}/Var_3")));

        authoring
            .insert(ShootParams {
                variations: 2,
                ..params()
            })
            .run();
        assert!(
            !authoring.has(&format!("{PROTOTYPE}/Var_3")),
            "stale prototype removed on re-author"
        );
    }
}
