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
//! A shoot is built **with its base at the origin, growing along +X, and
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
//! stands up on its own. No frame has to be transported along the spur. A
//! stray shoot turns up only part way and carries on at that pitch — see
//! [`ShootConfig::pitch`].
//!
//! The same frame is what lets a shoot stand in for a whole plant: a replant in
//! its first season is one of these out of the bare ground with the bend
//! buried [`PLANT_DEPTH`] deep, which is all a [`vine`](super::vine) below
//! [`VineConfig::is_mature`] is made of.
//!
//! [`VineConfig::is_mature`]: super::vine::VineConfig::is_mature
//!
//! # The layer
//!
//! [`vine`](super::vine) authors a [`ShootConfig`] on every bud its wood
//! offers; [`build`] turns the distinct configs into meshes. Every shoot gets:
//!
//! ```text
//! Shoot_00_0          the placed entity, carrying its ShootConfig
//!   Stem              -> parts/Shoot_<rep>, shared with every shoot that drew it
//!   Leaf_00           a LeafConfig of its own, hung on a node
//!   Leaf_01           ...
//! ```
//!
//! A stray one — a shoot the trellis failed to hold, leaning out over the
//! alley or standing over the top wire — carries a curve for the solver to
//! bend, and hangs both its geometry and its leaves on the rod segments that
//! solver builds from the curve rather than on itself:
//!
//! ```text
//! Shoot_00_0
//!   Cable             the centerline, simulated; `guide`, so never drawn
//!   Cable_edge_body_1 one rod segment, its transform driven by the solver
//!     Stem            the tube this segment is drawn with
//!     Leaf_00         rebased into that segment's frame
//!   Cable_edge_body_2 ...
//! ```
//!
//! The curve is not drawn because a `BasisCurves` whose points change every
//! frame renders with a visible glitch under Kit's RTX delegate, where a mesh
//! on a moving transform does not. One shared tube per segment also instances,
//! which a curve deformed per shoot cannot.
//!
//! Same split as one level up: where the nodes are comes from the
//! **representative**, because a leaf has to sit on the stem that actually got
//! built, while each leaf's bearing, droop, twist, size and blade are drawn per
//! shoot. Which is why a canopy off a handful of stem meshes does not read as a
//! handful of stem meshes.
//!
//! A leaf has nothing hanging off it, so it is a geometry prim in its own right
//! rather than an `Xform` over one — see [`scene`](crate::scene). At six
//! figures of them, that halves the prim count of the whole scene.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use nalgebra::Point3;

use super::leaf;
use super::util::mesh::{MeshData, cylinder_mesh};
use super::util::strand::{Bark, Strand, strand_mesh};
use super::util::{color, material, par_map};
use super::{Grow, Rng, SceneParams, salt};
use crate::quantize::{Metric, farthest_first};
use crate::scene::{CABLE, Geometry, Library, Order, Surface, cable, configs_changed, placed};
use crate::ui::{Staged, Tip};

/// The mesh-library prefix this element registers its stems under.
pub const PART: &str = "Shoot";

/// The mesh-library prefix a flexible shoot's per-segment tubes go under.
///
/// A layer of its own, because there is one tube per *segment* of every
/// representative where there is one stem per representative, and
/// [`Library::clear`] drops a whole prefix at a time.
pub const CANE: &str = "Cane";

/// The prim a shoot's stem takes, below the shoot itself. A child rather than
/// the shoot prim itself, because a shoot has leaves hanging off it and
/// geometry prims carry no children.
///
/// A flexible shoot uses the same name once per rod segment — see
/// [`segment_tubes`] — so a consumer looking for a shoot's geometry has one
/// rule whichever way it was drawn.
pub const STEM: &str = "Stem";

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

/// How deep a shoot has to be planted for its bend to be out of sight, in
/// meters.
///
/// The bend is exactly right on a spur and exactly wrong in the ground: a
/// shoot planted at the surface would leave the soil sideways and turn up in
/// front of everyone. Sunk this far, everything above ground is the straight
/// rise — the margin past [`BEND_RADIUS`] is what makes the tube cross the
/// surface already vertical rather than just as it finishes turning.
///
/// Public because a shoot is what a replant is made of, and whoever plants a
/// bare shoot is the one who has to bury it. It is the only thing about this
/// frame they need.
pub const PLANT_DEPTH: f64 = BEND_RADIUS + 0.03;

/// Spacing of the control points up the straight run.
const RISE_STEP: f64 = 0.08;

/// Tip radius as a fraction of the radius at the bud.
const TIP_TAPER: f64 = 0.45;

/// The S the lean rides on, as a fraction of the lean itself, and how many
/// times it crosses over across the rise. A shoot that only bowed one way
/// would read as bent rather than grown.
const SHOOT_SWAY: f64 = 0.35;
const SHOOT_WAVES: f64 = 1.25;

/// How far a stray shoot's rise is pitched off vertical, in radians, drawn
/// uniformly between these. The shallow end is a shoot that missed the hedger
/// and stands over the top wire; the steep end one that missed the catch wires
/// and lies out over the alley. Short of a right angle, so the bend keeps a
/// segment to be bolted by.
// ponytail: one uniform range covers both kinds of stray shoot; split it into
// two rates if a scene needs them in different proportions.
const STRAY_PITCH: (f64, f64) = (0.3, 1.3);

/// How much longer a stray shoot is than the shoots beside it: nothing cut it
/// back, and it has to reach a machine's leg a meter out from the row.
const STRAY_LENGTH: f32 = 1.6;

/// How far a stray shoot's tip curls back up off the line it was pitched on,
/// as a fraction of its length, in place of [`ShootParams::lean`]: a free
/// shoot's growing tip turns to the light. A fraction so that a short stray
/// shoot curls like a long one rather than coiling.
const STRAY_LEAN: f32 = 0.2;

// ─── Leaf constants ─────────────────────────────────────────────────

/// Salt splitting the stems' randomness off the scene seed, so that this layer
/// and the ones either side of it never draw from the same stream.
const STEM_STREAM: u64 = 0x589A_B41E_A1D2_F35B;

/// The same, splitting the leaves off the stems, so tuning the canopy never
/// reshapes the shoot underneath it — the same split [`vine`](super::vine)
/// keeps between its wood and its shoots.
const LEAF_STREAM: u64 = 0x2545_F491_4F6C_DD1D;

/// The same again, splitting *which* shoots strayed off both, so that moving
/// [`ShootParams::stray`] never redraws a canopy or reshuffles the shoots that
/// stayed held.
const STRAY_STREAM: u64 = 0xD1B5_4A32_D192_ED03;

/// Length of one segment of a flexible shoot's centerline, in meters.
///
/// A segment becomes a capsule body and a spring joint, so this is the whole
/// cost of a stray shoot: one of the default length buys fourteen of each.
/// Coarser than the mesh, which needs ring density the physics does not — a
/// bending rod is a chain of straight links either way.
const CABLE_SEGMENT: f64 = 0.09;

/// Below this the station loop would never terminate, so a shoot this closely
/// noded carries no leaves at all. That is also how the canopy gets turned off.
const MIN_INTERNODE: f64 = 0.005;

/// Bare tip left past the last node. The growing point itself is a curl of
/// scale leaves too small to be worth a mesh.
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

/// Spread of the droop about [`ShootConfig::leaf_droop`], as a fraction.
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
/// [`ShootConfig::internode`] is set to. Counting nodes instead would make the
/// canopy's age gradient change every time its density did.
const EXPANDING_REACH: f64 = 0.25;

// ─── Config ─────────────────────────────────────────────────────────

/// The shortest shoot we will build: below this the bend has nowhere to
/// finish, and the strand would double back on itself.
const MIN_LENGTH: f32 = (BEND_RADIUS * 1.5) as f32;

/// One shoot's shape, as the vine that grew it specified.
///
/// Everything a mesh is built from, and nothing about where the shoot sits —
/// that is the entity's `Transform`. Clamped on the way in rather than in the
/// shape functions, so the config a metric compares is the shoot that actually
/// gets built.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ShootConfig {
    pub length: f32,
    pub radius: f32,
    pub lean: f32,
    /// How far the rise is pitched off vertical, in radians, toward the side
    /// the shoot left its bud on — which the spur aims across the row.
    ///
    /// Zero for a shoot the trellis holds. A stray one keeps this much of the
    /// quarter turn its bend would have made and lies out over the alley or
    /// stands over the top wire with it; and since nothing holds it, it is
    /// what a solver bends — see [`is_stray`](Self::is_stray).
    pub pitch: f32,
    pub sides: u32,
    pub detail: u32,
    /// Distance between leaf nodes up the shoot, in meters. Below
    /// [`MIN_INTERNODE`] a shoot carries no leaves at all, which is how the
    /// canopy is turned off.
    pub internode: f32,
    /// How far a full-grown blade pitches below horizontal, in radians.
    ///
    /// Placement only — no part of the stem reads it — which is why
    /// [`ShootMetric`] does not either. Two shoots differing in nothing else
    /// must share a mesh and still hang their leaves at their own angle.
    pub leaf_droop: f32,
    /// The draws this shoot was authored at, kept so the params can be
    /// re-applied to a shoot already standing. Not shape: the fields above are
    /// what a mesh is built from, and [`ShootMetric`] reads these no more than
    /// a mesh does.
    pub vigour: f32,
    pub spacing: f32,
}

impl ShootConfig {
    /// The shoot these params call for, at this `vigour` and node `spacing`,
    /// pitched out of the trellis by `pitch` — zero for one it holds; see
    /// [`stray_pitch`].
    ///
    /// `vigour` and `spacing` are multipliers about `1.0`, drawn per shoot by
    /// whoever placed it — see [`vine`](super::vine). Vigour lengthens and
    /// thickens the shoot together, which is what more light does, and leaves
    /// the spacing alone, so a vigorous shoot also carries more leaves. A stray
    /// shoot is longer again by [`STRAY_LENGTH`] and curls by [`STRAY_LEAN`]:
    /// nothing cut it back, and nothing holds it straight.
    pub fn new(params: &ShootParams, vigour: f32, spacing: f32, pitch: f32) -> Self {
        let pitch = pitch.clamp(0.0, STRAY_PITCH.1 as f32);
        let stray = pitch > 0.0;
        let length =
            (params.length * vigour * if stray { STRAY_LENGTH } else { 1.0 }).max(MIN_LENGTH);
        Self {
            length,
            radius: (params.radius * vigour).max(0.0005),
            lean: if stray {
                STRAY_LEAN * length
            } else {
                params.lean.max(0.0)
            },
            pitch,
            sides: params.sides.max(3),
            detail: params.detail.max(1),
            internode: (params.internode * spacing).max(0.0),
            leaf_droop: params.leaf_droop,
            vigour,
            spacing,
        }
    }

    /// Whether this shoot escaped the trellis.
    ///
    /// A stray shoot is the one kind exported as a cable rather than a mesh:
    /// the trellis holds the others still, and nothing holds this one.
    pub fn is_stray(&self) -> bool {
        self.pitch > 0.0
    }
}

/// Two shoots share a mesh when they are close in every dimension that shows.
///
/// The weights turn each field into roughly how far apart it *looks*. Radius
/// is the extreme case: a shoot is six millimeters thick, so a millimeter of
/// it is a sixth of the silhouette, where a millimeter of length is nothing.
pub struct ShootMetric;

impl Metric<ShootConfig> for ShootMetric {
    fn distance(&self, a: &ShootConfig, b: &ShootConfig) -> f32 {
        [
            a.length - b.length,
            (a.radius - b.radius) * 8.0,
            (a.lean - b.lean) * 2.0,
            // A radian of pitch moves a tip about a meter, so it weighs what a
            // meter of length does.
            a.pitch - b.pitch,
            // Reaches the mesh only through where the nodes land, but two
            // shoots noded differently hang different canopies, and the nodes
            // come from whichever of them built the mesh.
            (a.internode - b.internode) * 2.0,
            (a.sides as f32 - b.sides as f32) * 0.01,
        ]
        .iter()
        .map(|d| d * d)
        .sum::<f32>()
        .sqrt()
    }
}

// ─── Params ─────────────────────────────────────────────────────────

#[derive(Resource, Clone, Debug, PartialEq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct ShootParams {
    /// How many distinct stem meshes the scene may hold.
    ///
    /// A budget, not a count: the shoots are clustered and this is how many
    /// representatives the clustering may keep — once for the shoots the
    /// trellis holds and again for the stray ones, which are clustered apart.
    pub variations: u32,
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
    /// a stem is a *shared mesh*, so this buys ring density for the whole
    /// vineyard at the cost of a handful of meshes. It has to be high because
    /// stations are spaced by arc length and [`BEND_RADIUS`] packs a quarter
    /// turn into a few centimeters — at the density a trunk is happy with, the
    /// bend comes out a chamfer.
    pub detail: u32,
    /// Distance between leaf nodes up the shoot, in meters.
    ///
    /// The count-like knob, the way [`shoots_per_spur`] is one level up: how
    /// many leaves a shoot carries is a fact about the shoot rather than about
    /// a leaf, and a spacing says it in the unit a viticulturist would.
    ///
    /// [`shoots_per_spur`]: super::vine::VineParams::shoots_per_spur
    pub internode: f32,
    /// How far a full-grown blade pitches below horizontal, in radians.
    ///
    /// Rides on each leaf's own maturity, so the mature blades down the shoot
    /// hang at about this and the small ones at the tip stand nearly straight
    /// out — which is what a petiole holding a tenth of the weight does.
    pub leaf_droop: f32,
    /// The fraction of shoots the trellis failed to hold, in `0..=1`.
    ///
    /// Missed by shoot positioning, so it grew out into the alley, or by
    /// hedging, so it kept growing past the top wire: either way a stray shoot
    /// leans out of the canopy, longer than the shoots beside it, and is what a
    /// machine passing over the row runs into — see [`STRAY_PITCH`].
    ///
    /// Every stray shoot is exported as a deformable curve rather than a mesh,
    /// a chain of rigid bodies in the simulation, so this is the most expensive
    /// knob in the scene — see [`CABLE_SEGMENT`] for what one costs.
    pub stray: f32,
}

impl Default for ShootParams {
    fn default() -> Self {
        Self {
            variations: 4,
            length: 0.75,
            radius: 0.006,
            lean: 0.06,
            sides: 6,
            detail: 40,
            internode: 0.07,
            leaf_droop: 0.35,
            stray: 0.0,
        }
    }
}

pub fn plugin(app: &mut App) {
    // `SceneParams` is not gated on here although `build` reads the seed:
    // `planting::plant` gates on it and respawns every vine when it moves, and
    // `vine::build` hangs fresh shoots off them, so the seed arrives as a
    // config change in the same frame.
    app.init_resource::<ShootParams>().add_systems(
        PreUpdate,
        (
            reauthor.run_if(resource_changed::<ShootParams>),
            build.run_if(
                configs_changed::<ShootConfig>
                    // `variations` reaches no config, so the reauthor above can
                    // leave every one of them alone and this still has to run.
                    .or_eager(resource_changed::<ShootParams>),
            ),
        )
            .chain()
            .in_set(Grow::Shoots),
    );
}

/// Re-grows every shoot already standing, in place.
///
/// [`vine`](super::vine) authors these configs, but re-running that layer to
/// change the numbers on them would despawn and respawn every canopy below it.
/// Each shoot keeps the draws it was authored at so the params can be applied
/// again without it.
fn reauthor(
    params: Res<ShootParams>,
    scene: Res<SceneParams>,
    mut shoots: Query<(&Order, &mut ShootConfig)>,
) {
    for (order, mut config) in &mut shoots {
        let pitch = stray_pitch(&params, scene.seed, order.0);
        let next = ShootConfig::new(&params, config.vigour, config.spacing, pitch);
        config.set_if_neq(next);
    }
}

// ─── Shape ──────────────────────────────────────────────────────────

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

/// Distances up the straight run to put control points at: an even spread,
/// and the tip itself whatever the spacing worked out to.
fn rise_nodes(rise: f64) -> Vec<f64> {
    let mut nodes = Vec::new();
    let mut u = RISE_STEP;
    while u < rise - RISE_STEP * 0.5 {
        nodes.push(u);
        u += RISE_STEP;
    }
    nodes.push(rise);
    nodes
}

/// The curve a shoot's tube is skinned onto, and the taper it is skinned at.
///
/// Kept as a thing in its own right so leaves can be put back on the *same*
/// curve rather than on a resampling of the mesh — the way a
/// [`Spur`](super::vine) is a bare axis with an `at`, precisely so that placing
/// on it needs no frame transported along it.
///
/// Parameterized by **station** — meters along the curve from the bud — and
/// never by height: a stray shoot lies over rather than standing up, so how
/// high a point is says nothing about how far along the shoot it sits.
#[derive(Clone, Copy, Debug)]
struct ShootAxis {
    /// Length of the straight run past the bend.
    rise: f64,
    /// How far the rise is pitched off vertical, toward the side the shoot
    /// left its bud on. See [`ShootConfig::pitch`].
    pitch: f64,
    radius: f64,
    lean: f64,
    azimuth: f64,
    phase: f64,
}

impl ShootAxis {
    /// The *draw order* is part of this element's output: the lean's bearing,
    /// its wave's phase, then how much of the nominal lean this one actually
    /// takes.
    fn new(config: &ShootConfig, rng: &mut Rng) -> Self {
        let azimuth = rng.unit() * TAU;
        Self {
            rise: config.length as f64 - BEND_RADIUS,
            pitch: config.pitch as f64,
            radius: config.radius as f64,
            // A stray shoot's lean is aimed back up its pitch plane rather
            // than drawn: nothing holds it, and a free tip turns up toward the
            // light. The draw is made all the same, so the two after it land
            // where they always did.
            azimuth: if config.is_stray() { PI } else { azimuth },
            phase: rng.unit() * TAU,
            lean: config.lean as f64 * rng.range(0.5, 1.0),
        }
    }

    /// How far round the bend turns: the quarter turn that stands an upright
    /// shoot up, less the pitch a stray one keeps.
    fn bend_angle(&self) -> f64 {
        FRAC_PI_2 - self.pitch
    }

    /// Length of the bend along its own curve — the station the rise starts
    /// at.
    fn bend_arc(&self) -> f64 {
        BEND_RADIUS * self.bend_angle()
    }

    /// Bud to tip along the curve.
    ///
    /// The rise is measured along the line it was pitched on rather than by
    /// true arc length. The lean buys it a fraction of a percent, and taking
    /// the run keeps the top node a fixed distance below the tip however far
    /// this shoot happened to wander — which is what the age gradient is keyed
    /// on.
    fn length(&self) -> f64 {
        self.bend_arc() + self.rise
    }

    /// The point `u` meters up the rise from the top of the bend.
    ///
    /// The rise's own parameter. [`at`](Self::at) reaches it through the
    /// station, but the stem's control points are spaced along the rise and
    /// go straight here, so that a reparameterization's rounding never reaches
    /// the mesh.
    fn at_run(&self, u: f64) -> Point3<f64> {
        let (dx, dy) = shoot_axis(u / self.rise, self.lean, self.azimuth, self.phase);
        let top = bend_point(self.bend_angle());
        // Up the rise and across it within the pitch plane: +Z and +X for an
        // upright shoot, turned over together by the pitch.
        let (sin, cos) = self.pitch.sin_cos();
        Point3::new(
            top.x + u * sin + dx * cos,
            top.y + dy,
            top.z + u * cos - dx * sin,
        )
    }

    /// The point `s` meters along the axis from the bud.
    fn at(&self, s: f64) -> Point3<f64> {
        if s < self.bend_arc() {
            bend_point(s / BEND_RADIUS)
        } else {
            self.at_run(s - self.bend_arc())
        }
    }

    /// Stem radius `s` meters along the shoot, thinning to [`TIP_TAPER`] of
    /// the radius at the bud by the tip.
    ///
    /// By station rather than by point index: the bend's points are
    /// centimeters apart and the rise's are decimeters, so an index taper
    /// would spend the whole taper on the bend.
    fn radius_at(&self, s: f64) -> f64 {
        let t = (s / self.length()).clamp(0.0, 1.0);
        self.radius * (1.0 + (TIP_TAPER - 1.0) * t)
    }
}

/// The point on the bend `angle` radians round from the bud.
///
/// The bend is the same circle on every shoot — the lean only starts
/// accumulating above it — so this needs nothing off an axis.
fn bend_point(angle: f64) -> Point3<f64> {
    Point3::new(
        BEND_RADIUS * angle.sin(),
        0.0,
        BEND_RADIUS * (1.0 - angle.cos()),
    )
}

/// One shoot's stem, in its own local frame.
fn shoot_strand(axis: &ShootAxis, config: &ShootConfig) -> Strand {
    // Starts behind the bud, arcs up over the bend, then rises. The bend's
    // first point *is* the origin, so the embedded stub and the arc share a
    // tangent and the shoot leaves the wood pointing along +X.
    let mut stations = vec![-SHOOT_EMBED];
    stations.extend((0..BEND_NODES).map(|i| axis.bend_arc() * i as f64 / (BEND_NODES - 1) as f64));
    stations.extend(
        rise_nodes(axis.rise)
            .into_iter()
            .map(|u| axis.bend_arc() + u),
    );

    let mut points = vec![Point3::new(-SHOOT_EMBED, 0.0, 0.0)];
    points.extend(stations[1..].iter().map(|s| axis.at(*s)));
    let radii = stations.iter().map(|s| axis.radius_at(*s)).collect();

    // No bark: a shoot is a smooth green stem, and ridges on a six-millimeter
    // tube read as noise rather than texture.
    Strand::new(
        points,
        radii,
        config.sides as usize,
        config.detail as f64,
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

/// Where one leaf hangs, in the shoot's local frame.
///
/// A slot on the built stem rather than a finished placement: the bearing,
/// droop, twist, size and blade of the leaf that fills it are drawn per
/// *shoot*, in [`build`].
#[derive(Clone, Debug)]
struct LeafNode {
    name: String,
    position: Vec3,
    /// How far along the shoot it sits, in meters from the bud — what picks
    /// the rod segment it rides on a cane.
    station: f32,
    /// The rank's angle about the stem, before the per-shoot bearing and
    /// wander. Accumulates past a full turn; [`build`] wraps it.
    yaw: f32,
    /// How grown the blade here is, as a fraction of a full-grown one.
    maturity: f32,
}

/// Where this shoot's leaves sit.
///
/// Nodes climb the shoot from the top of the bend to a little short of the
/// tip. The bend is left bare because that is where a shoot is still lying
/// sideways, and a leaf's whole orientation is a bearing about Z plus a
/// droop — which only means "around the stem" once the stem is standing up.
///
/// Draws from a **stream of its own**, salted with [`LEAF_STREAM`], so that
/// tuning the canopy never reshapes the stem underneath it. One draw per node:
/// how far it slid off its nominal station.
///
/// Unlike a spur's fixed three buds there is no slot set to keep aligned — the
/// station list *is* the count, so changing [`ShootConfig::internode`] re-rolls
/// the whole canopy rather than adding to it. That is the honest behaviour for
/// a spacing.
fn leaf_nodes(config: &ShootConfig, axis: &ShootAxis, seed: u64) -> Vec<LeafNode> {
    let internode = config.internode as f64;
    let length = axis.length();
    if internode < MIN_INTERNODE {
        return Vec::new();
    }

    let mut rng = Rng::new(seed ^ LEAF_STREAM);
    let mut nodes = Vec::new();
    let mut station = axis.bend_arc();
    let mut index = 0usize;

    while station <= length - TIP_CLEARANCE {
        let slide = rng.range(-STATION_JITTER, STATION_JITTER) * internode;
        let at = (station + slide).clamp(axis.bend_arc(), length);
        // On the centerline rather than out at the stem's surface: that buries
        // the petiole's free end under a few millimeters of stem, which is the
        // trick `SHOOT_EMBED` already uses one level up and is what guarantees
        // no gap however the blade ends up turned.
        let position = axis.at(at);

        nodes.push(LeafNode {
            name: format!("Leaf_{index:02}"),
            position: Vec3::new(position.x as f32, position.y as f32, position.z as f32),
            station: at as f32,
            // Distichous: successive leaves sit half a turn apart, in two ranks
            // up opposite sides of the shoot, drifting slowly so ten of them do
            // not come out coplanar.
            yaw: ((PI + PHYLLOTAXY_DRIFT) * index as f64) as f32,
            maturity: leaf_scale(length - at) as f32,
        });

        station += internode;
        index += 1;
    }
    nodes
}

// ─── Cable ──────────────────────────────────────────────────────────

/// A flexible shoot's centerline, in the shoot's own frame: what its curve is
/// authored from, and what its tubes and leaves are placed against.
#[derive(Clone, Debug)]
struct Centerline {
    /// The control points a solver bends. See [`cable_stations`].
    points: Vec<[f32; 3]>,
    /// How far along the shoot each point sits, in meters from the bud.
    stations: Vec<f32>,
    /// The diameter each point is **drawn** at — the same taper as the stem
    /// mesh, so a cane reads as the shoots beside it. Nothing sizes a capsule
    /// from these; see `thickness`.
    widths: Vec<f32>,
    /// The one thickness the cable is **simulated** at, in meters.
    ///
    /// A rod has a single radius where the mesh tapers, so this splits the
    /// difference: the diameter at the taper's midpoint, too thin at the bud
    /// and too thick at the tip by the same amount.
    thickness: f32,
    /// How far the rise is pitched off vertical, in radians. What a leaf's
    /// posture is turned over by, so its ranks stay around the stem.
    pitch: f32,
}

impl Centerline {
    fn new(axis: &ShootAxis) -> Self {
        let stations = cable_stations(axis);
        Self {
            points: stations
                .iter()
                .map(|s| {
                    let p = axis.at(*s);
                    [p.x as f32, p.y as f32, p.z as f32]
                })
                .collect(),
            widths: stations
                .iter()
                .map(|s| 2.0 * axis.radius_at(*s) as f32)
                .collect(),
            thickness: 2.0 * axis.radius_at(axis.length() / 2.0) as f32,
            stations: stations.into_iter().map(|s| s as f32).collect(),
            pitch: axis.pitch as f32,
        }
    }
}

/// Where along a flexible shoot its control points sit: the bud, the top of
/// the bend, then the rise cut into [`CABLE_SEGMENT`] lengths of arc.
///
/// **The whole bend is the first segment**, rather than something the sampling
/// happens to cut across. That segment is the one bolted down — see
/// `_cable_point_masses` in `python/vinerylab/usd/build.py` — so its shape is
/// static and only its endpoints matter, while every segment that does move is
/// a piece of the rise of the same length. A solver derives one stiffness from
/// the mean segment length and mistunes whatever differs from it, so evenness
/// is worth arranging rather than hoping for.
///
/// Cut by arc rather than by run, because the lean curls the rise and a chain
/// cut at even runs comes out uneven where it curls most: the run is sampled
/// finely, the arc summed along it, and each cut put where the arc reaches its
/// share.
///
/// Starts at the bud rather than behind it: the first point is where the cane
/// is held, and holding it inside the spur would put the anchor somewhere no
/// reader can see.
fn cable_stations(axis: &ShootAxis) -> Vec<f64> {
    const SAMPLES: usize = 256;
    let run = |i: usize| axis.rise * i as f64 / SAMPLES as f64;
    // Arc length from the top of the bend to each sample of the rise.
    let mut arc = vec![0.0];
    for i in 1..=SAMPLES {
        let step = (axis.at_run(run(i)) - axis.at_run(run(i - 1))).norm();
        arc.push(arc[i - 1] + step);
    }
    let total = arc[SAMPLES];

    // At least one rise segment, so the shortest shoot still makes a chain of
    // two — a rod of one capsule is not a rod.
    let count = (total / CABLE_SEGMENT).round().max(1.0);
    let mut stations = vec![0.0, axis.bend_arc()];
    for k in 1..=count as usize {
        let target = total * k as f64 / count;
        let i = arc.partition_point(|a| *a < target).clamp(1, SAMPLES);
        let f = (target - arc[i - 1]) / (arc[i] - arc[i - 1]);
        stations.push(axis.bend_arc() + run(i - 1) + (run(i) - run(i - 1)) * f);
    }
    stations
}

/// The prim name Newton gives the rigid body it builds for segment `index` of
/// this shoot's cable.
///
/// A **sibling** of the curve rather than a child: the label is the curve's own
/// prim path with this suffix. Authoring the prim ourselves is the whole of how
/// a leaf gets attached — the importer adopts a prim already sitting at the
/// path, keeps its children, and drives its transform from then on.
///
/// Get the name wrong and nothing fails loudly: the importer defines its own
/// prim beside ours, and the leaves stay at the cane's rest shape.
fn segment_name(index: usize) -> String {
    format!("{CABLE}_edge_body_{index}")
}

/// The rest frame of every segment `points` becomes, in the shoot's own frame:
/// origin at the segment's midpoint, local +Z along it.
///
/// That is Newton's frame for the capsule body, so a prim authored here starts
/// exactly where the solver will go on keeping it.
fn segment_frames(points: &[[f32; 3]]) -> Vec<Transform> {
    points
        .windows(2)
        .map(|pair| {
            let (from, to) = (Vec3::from(pair[0]), Vec3::from(pair[1]));
            Transform {
                translation: (from + to) * 0.5,
                rotation: Quat::from_rotation_arc(Vec3::Z, (to - from).normalize()),
                scale: Vec3::ONE,
            }
        })
        .collect()
}

/// One tube per segment of `centerline`, each in that segment's own frame —
/// the geometry a flexible shoot is drawn with, in place of its curve.
///
/// Drawn at the widths the curve carries, so a cane and a rigid shoot off the
/// same representative still read alike. Each tube is centered on the segment
/// midpoint to match [`segment_frames`], and overruns both of its ends by its
/// own radius there: the tubes are butt-jointed and turn by up to about 45° at
/// the bend, where flush ends would leave a wedge of daylight.
fn segment_tubes(centerline: &Centerline, sides: usize) -> Vec<MeshData> {
    centerline
        .points
        .windows(2)
        .zip(centerline.widths.windows(2))
        .map(|(pair, widths)| {
            let (from, to) = (Vec3::from(pair[0]), Vec3::from(pair[1]));
            let length = (to - from).length();
            let (base, top) = (widths[0] / 2.0, widths[1] / 2.0);
            let mut tube = cylinder_mesh(base, top, length + base + top, sides);
            for point in &mut tube.points {
                point[2] -= length / 2.0 + base;
            }
            tube
        })
        .collect()
}

/// Which segment of a cable whose points sit at `stations` carries whatever
/// sits at station `s`.
///
/// Segment 0 is the bend and is the one bolted down; no leaf lands there,
/// because the nodes start above it.
fn segment_of(stations: &[f32], s: f32) -> usize {
    stations
        .iter()
        .rposition(|station| *station <= s)
        .unwrap_or(0)
        .min(stations.len() - 2)
}

/// `placement` expressed in `frame`, so that a prim under `frame` still lands
/// where `placement` put it. A frame is a rotation and a translation only,
/// which is what makes the inverse this short.
fn rebase(frame: &Transform, placement: &Transform) -> Transform {
    let turn = frame.rotation.inverse();
    Transform {
        translation: turn * (placement.translation - frame.translation),
        rotation: turn * placement.rotation,
        scale: placement.scale,
    }
}

/// How far the shoot authored at `order` leans out of the trellis, in radians:
/// zero for one the trellis holds, a draw across [`STRAY_PITCH`] for a stray
/// one.
///
/// Two draws off a stream of its own — whether it strayed, then how far — so
/// that moving [`ShootParams::stray`] never redraws a canopy or reshuffles the
/// shoots that stayed held.
pub fn stray_pitch(params: &ShootParams, seed: u64, order: u64) -> f32 {
    let mut rng = Rng::new(seed ^ STRAY_STREAM ^ salt(order));
    if rng.unit() < params.stray as f64 {
        rng.range(STRAY_PITCH.0, STRAY_PITCH.1) as f32
    } else {
        0.0
    }
}

// ─── Building ───────────────────────────────────────────────────────

/// What a representative is drawn with: one stem for a shoot the trellis
/// holds, or a tube on each rod segment for a stray one. Never both — the
/// other would be authored into the scene and referenced by nothing.
enum Drawn<G> {
    Stem(G),
    Tubes(Vec<G>),
}

/// One representative shoot: the nodes its leaves hang on, the centerline it
/// bends along, and the meshes it is drawn with.
struct ShootBuild {
    nodes: Vec<LeafNode>,
    centerline: Centerline,
    drawn: Drawn<Mesh>,
}

fn build_shoot(config: &ShootConfig, seed: u64) -> anyhow::Result<ShootBuild> {
    let axis = ShootAxis::new(config, &mut Rng::new(seed));
    let centerline = Centerline::new(&axis);
    let drawn = if config.is_stray() {
        Drawn::Tubes(
            segment_tubes(&centerline, config.sides as usize)
                .iter()
                .map(MeshData::to_mesh)
                .collect(),
        )
    } else {
        Drawn::Stem(strand_mesh(&shoot_strand(&axis, config))?.to_mesh())
    };
    Ok(ShootBuild {
        nodes: leaf_nodes(config, &axis, seed),
        centerline,
        drawn,
    })
}

/// Clusters the held shoots and the stray ones apart, each to `budget`
/// representatives, and hands the two codebooks back as one: the
/// representatives, and which one each config drew.
///
/// One codebook over both would spend itself on the strays: a stray shoot's
/// pitch puts it further from every held shoot than the held shoots are from
/// each other, and k-center chases the farthest, so the default budget would
/// leave the whole canopy one stem the moment anything strayed.
fn quantize(configs: &[ShootConfig], budget: usize) -> (Vec<ShootConfig>, Vec<u32>) {
    let mut representatives = Vec::new();
    let mut assignment = vec![0u32; configs.len()];
    for stray in [false, true] {
        let members: Vec<usize> = (0..configs.len())
            .filter(|i| configs[*i].is_stray() == stray)
            .collect();
        let population: Vec<ShootConfig> = members.iter().map(|i| configs[*i]).collect();
        let book = farthest_first(&population, budget, 0.0, &ShootMetric);
        for (member, drew) in members.iter().zip(&book.assignment) {
            assignment[*member] = representatives.len() as u32 + drew;
        }
        representatives.extend(book.representatives);
    }
    (representatives, assignment)
}

/// One representative, built once: the leaf nodes hanging off it, the
/// centerline a cane bends along, how it looks, and what it is drawn with.
/// Every shoot assigned to it clones the lot.
type Built = (Vec<LeafNode>, Centerline, Surface, Drawn<Geometry>);

/// Builds one mesh per distinct shoot, and hangs a leaf on every node.
///
/// The stem is shared and the canopy is not: every shoot authors its own
/// [`leaf::LeafConfig`] at each node, so two shoots off one mesh carry
/// different blades at different angles.
///
/// Each shoot's leaves draw from a stream keyed on its [`Order`], so a shoot's
/// canopy depends on which bud it grew from and on nothing else in the parcel.
pub(crate) fn build(
    mut commands: Commands,
    mut library: Library,
    scene: Res<SceneParams>,
    params: Res<ShootParams>,
    leaf_params: Res<leaf::LeafParams>,
    shoots: Query<(Entity, &Order, &ShootConfig)>,
) -> Result<()> {
    library.clear(PART);
    library.clear(CANE);

    let mut grown: Vec<(Order, Entity, ShootConfig)> = shoots
        .iter()
        .map(|(entity, order, config)| (*order, entity, *config))
        .collect();
    grown.sort_by_key(|(order, ..)| *order);

    let configs: Vec<ShootConfig> = grown.iter().map(|(_, _, config)| *config).collect();
    let (representatives, assignment) = quantize(&configs, params.variations.max(1) as usize);

    let stem_seed = |index: usize| scene.seed ^ STEM_STREAM ^ salt(index as u64);
    // The representatives are independent of each other, so they are grown in
    // parallel and registered serially: `Assets<Mesh>` takes one writer.
    let stems = par_map(&representatives, |index, config| {
        build_shoot(config, stem_seed(index))
    });

    let mut built: Vec<Built> = Vec::with_capacity(representatives.len());
    // Tube parts are numbered across the whole layer, because how many a
    // representative needs depends on how long its cane came out.
    let mut tubes_built = 0usize;
    for (index, grown) in stems.into_iter().enumerate() {
        let ShootBuild {
            nodes,
            centerline,
            drawn,
        } = grown?;
        // The curve draws in the mesh's colour, so a cane and a rigid shoot of
        // the same representative look alike.
        let skin = surface(stem_seed(index));
        let drawn = match drawn {
            Drawn::Stem(stem) => Drawn::Stem(library.part(PART, index, stem, skin)),
            Drawn::Tubes(tubes) => {
                let tubes: Vec<Geometry> = tubes
                    .into_iter()
                    .enumerate()
                    .map(|(segment, tube)| library.part(CANE, tubes_built + segment, tube, skin))
                    .collect();
                tubes_built += tubes.len();
                Drawn::Tubes(tubes)
            }
        };
        built.push((nodes, centerline, skin, drawn));
    }

    let mut leaf_order = 0u64;
    for ((order, entity, config), drew) in grown.iter().zip(&assignment) {
        let (nodes, centerline, skin, drawn) = &built[*drew as usize];
        let mut shoot = commands.entity(*entity);
        // The layer owns everything below a shoot, and a rebuild may hang a
        // different number of leaves than the last one did.
        shoot.despawn_children();

        // A stray shoot is a curve that is simulated and not drawn, with a
        // tube on every rod segment the solver builds from it; a held one is a
        // single mesh.
        let (mut frames, mut segments) = (Vec::new(), Vec::new());
        match drawn {
            Drawn::Stem(stem) => {
                shoot.with_child((Name::new(STEM), stem.clone()));
            }
            Drawn::Tubes(tubes) => {
                shoot.with_child((
                    Name::new(CABLE),
                    cable(
                        centerline.points.clone(),
                        centerline.widths.clone(),
                        centerline.thickness,
                        *skin,
                    ),
                ));
                frames = segment_frames(&centerline.points);
                // Every segment up front rather than the ones that turn out to
                // carry a leaf, so an index into `frames` is an index into
                // these.
                shoot.with_children(|shoot| {
                    segments.extend(frames.iter().zip(tubes).enumerate().map(
                        |(index, (frame, tube))| {
                            shoot
                                .spawn((
                                    Name::new(segment_name(index)),
                                    *frame,
                                    Visibility::default(),
                                ))
                                // The tube rides the segment rather than the
                                // segment prim carrying the mesh itself,
                                // because the solver drives that prim and a
                                // prim holding a reference can have no children
                                // — the leaves are already down here.
                                .with_child((Name::new(STEM), tube.clone()))
                                .id()
                        },
                    ));
                });
            }
        }

        let mut rng = Rng::new(scene.seed ^ LEAF_STREAM ^ salt(order.0));
        // Drawn once, so the two ranks are not lined up with the shoot's lean.
        let bearing = rng.unit() * TAU;

        for node in nodes {
            // Six draws per node: the bearing's wander, the droop, the twist
            // about the blade's own long axis, its vigour, then which blade it
            // drew and how far that blade curls.
            let turn = rng.range(-LEAF_SPREAD, LEAF_SPREAD);
            let sag = rng.range(1.0 - LEAF_DROOP_JITTER, 1.0 + LEAF_DROOP_JITTER);
            let roll = rng.range(-LEAF_ROLL, LEAF_ROLL);
            let vigour = rng.range(1.0 - LEAF_VIGOUR, 1.0 + LEAF_VIGOUR);
            let outline = (rng.unit() * leaf::OUTLINES.len() as f64) as usize;
            let curl = rng.unit();

            leaf_order += 1;
            let mut placement = placed(
                node.position,
                // Wrapped, because the rank angle runs past a full turn by the
                // fourth node.
                (bearing + node.yaw as f64 + turn).rem_euclid(TAU) as f32,
                // A leaf is drawn flat along +X with its face toward +Z, so the
                // tilt is the whole of its posture: X twists the blade about
                // its own long axis, Y pitches its tip down.
                Vec2::new(
                    roll as f32,
                    (config.leaf_droop as f64 * node.maturity as f64 * sag) as f32,
                ),
                // `leaf::AREA` is the same for every blade, so a scale is a
                // size in meters whichever one this node drew.
                node.maturity * vigour as f32,
            );
            // A bearing is a turn about the stem, which stands up +Z on a held
            // shoot and lies over with the rise on a stray one: the whole
            // posture is turned over with it, so the ranks stay around the
            // stem rather than around the vertical.
            placement.rotation = Quat::from_rotation_y(centerline.pitch) * placement.rotation;

            // On a cane the leaf hangs off the segment that carries it, which
            // is a frame of its own; on a held shoot, off the shoot.
            let (host, placement) = match drawn {
                Drawn::Tubes(_) => {
                    let index = segment_of(&centerline.stations, node.station);
                    (segments[index], rebase(&frames[index], &placement))
                }
                Drawn::Stem(_) => (*entity, placement),
            };

            commands.entity(host).with_child((
                Name::new(node.name.clone()),
                placement,
                Visibility::default(),
                leaf::LeafConfig::new(&leaf_params, outline, curl),
                Order(leaf_order),
            ));
        }
    }
    Ok(())
}

/// A green stem, shaded off this representative's own seed.
fn surface(seed: u64) -> Surface {
    material::FOLIAGE.surface(color::shade(
        color::srgb(color::CANE),
        &mut Rng::new(seed ^ color::COLOR_STREAM),
    ))
}

// ─── UI ─────────────────────────────────────────────────────────────

pub fn ui() -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            label_small("Shoot length"),
            (
                @FeathersSlider { @min: 0.1, @max: 1.6, @value: 0.75 }
                Tip("Bud to tip — how tall a shoot stands above the spur it grew from.")
                SliderStep(0.05)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.shoot.length = change.value;
                })
            ),
            label_small("Shoot radius"),
            (
                @FeathersSlider { @min: 0.002, @max: 0.015, @value: 0.006 }
                Tip("Radius at the bud, in metres.")
                SliderStep(0.001)
                SliderPrecision(3)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.shoot.radius = change.value;
                })
            ),
            label_small("Shoot lean"),
            (
                @FeathersSlider { @min: 0.0, @max: 0.25, @value: 0.06 }
                Tip("How far the tip wanders off vertical, in metres.")
                SliderStep(0.01)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.shoot.lean = change.value;
                })
            ),
            label_small("Stray shoots"),
            (
                @FeathersSlider { @min: 0.0, @max: 0.2, @value: 0.0 }
                Tip("Fraction of shoots the trellis failed to hold, leaning out of the canopy. Each is exported as a deformable curve, so this is the priciest knob here.")
                SliderStep(0.01)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.shoot.stray = change.value.clamp(0.0, 1.0);
                })
            ),
            label_small("Leaf spacing"),
            (
                @FeathersSlider { @min: 0.0, @max: 0.25, @value: 0.07 }
                Tip("Distance between leaf nodes up the shoot — how many leaves it carries, said the way a viticulturist would.")
                SliderStep(0.01)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.shoot.internode = change.value.max(0.0);
                })
            ),
            label_small("Leaf droop"),
            (
                @FeathersSlider { @min: 0.0, @max: 1.2, @value: 0.35 }
                Tip("How far a full-grown blade pitches below horizontal, in radians. The small blades at the tip stand nearly straight out.")
                SliderStep(0.05)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.shoot.leaf_droop = change.value;
                })
            ),
            label_small("Shoot sides"),
            (
                @FeathersSlider { @min: 3.0, @max: 12.0, @value: 6.0 }
                Tip("Vertices around the tube.")
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.shoot.sides = change.value.round().max(3.0) as u32;
                })
            ),
            label_small("Shoot detail"),
            (
                @FeathersSlider { @min: 8.0, @max: 90.0, @value: 40.0 }
                Tip("Rings per metre. High here and cheaper than it looks — a stem is a shared mesh, and the tip bend needs the density.")
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.shoot.detail = change.value.round().max(4.0) as u32;
                })
            ),
            label_small("Shoot variations"),
            (
                @FeathersSlider { @min: 1.0, @max: 8.0, @value: 4.0 }
                Tip("A budget, not a count: how many distinct stem meshes the clustering may keep.")
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.shoot.variations = change.value.round().max(1.0) as u32;
                })
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::VineyardParams;
    use crate::elements::util::mesh::MeshData;
    use crate::elements::util::testing::{self, bounds, named_children, organs};
    use crate::elements::vine;
    use crate::scene::{Cable, Prototypes, UsdReference};

    fn params() -> ShootParams {
        ShootParams::default()
    }

    /// The default shoot, at nominal vigour and spacing, held by the trellis.
    fn config() -> ShootConfig {
        ShootConfig::new(&params(), 1.0, 1.0, 0.0)
    }

    /// The same, with `edit` applied to the params first.
    fn config_with(edit: impl FnOnce(&mut ShootParams)) -> ShootConfig {
        let mut params = params();
        edit(&mut params);
        ShootConfig::new(&params, 1.0, 1.0, 0.0)
    }

    /// The default shoot, strayed out of the trellis by `pitch`.
    fn stray_config(pitch: f32) -> ShootConfig {
        ShootConfig::new(&params(), 1.0, 1.0, pitch)
    }

    /// A held shoot and a stray one: what every axis test has to hold for.
    fn both() -> [ShootConfig; 2] {
        [config(), stray_config(1.0)]
    }

    /// The centerline of `config`, built at `seed`.
    fn centerline(config: &ShootConfig, seed: u64) -> Centerline {
        Centerline::new(&axis(config, seed))
    }

    fn axis(config: &ShootConfig, seed: u64) -> ShootAxis {
        ShootAxis::new(config, &mut Rng::new(seed))
    }

    // ─── Flexible shoots ────────────────────────────────────────────

    /// The span between consecutive control points.
    fn spans(points: &[[f32; 3]]) -> Vec<f32> {
        points
            .windows(2)
            .map(|pair| {
                let [a, b] = [pair[0], pair[1]];
                ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt()
            })
            .collect()
    }

    /// The points a solver bends are points of the curve the mesh is skinned
    /// onto: a cable that wandered off would leave the collider somewhere the
    /// shoot visibly is not. The two ends are built by different code from the
    /// stem's, and everything between is put back on the axis by its station.
    #[test]
    fn a_cables_points_sit_on_the_shoots_own_axis() {
        for config in both() {
            let axis = axis(&config, 3);
            let cable = Centerline::new(&axis);
            let strand = shoot_strand(&axis, &config);

            assert_eq!(
                cable.points[0],
                [0.0, 0.0, 0.0],
                "the first point is the bud"
            );
            let tip = strand.points.last().unwrap();
            let end = Vec3::from(*cable.points.last().unwrap());
            assert!(
                end.abs_diff_eq(Vec3::new(tip.x as f32, tip.y as f32, tip.z as f32), 1e-5),
                "and the last is the stem's tip: {end:?} against {tip:?}"
            );
            for (index, (point, station)) in cable.points.iter().zip(&cable.stations).enumerate() {
                let on_axis = axis.at(*station as f64);
                let off = (point[0] as f64 - on_axis.x)
                    .hypot(point[1] as f64 - on_axis.y)
                    .hypot(point[2] as f64 - on_axis.z);
                assert!(off < 1e-5, "point {index} is {off} off the axis");
            }
        }
    }

    /// One stiffness is derived from the mean segment length, so a run that is
    /// not even leaves its outliers mistuned. The bend is exempt — it is the
    /// bolted segment, and does not move.
    #[test]
    fn a_cables_moving_segments_are_evenly_spaced() {
        for length in [MIN_LENGTH, 0.4, 0.75, 1.6] {
            let params = ShootParams { length, ..params() };
            for pitch in [0.0, 1.0] {
                let config = ShootConfig::new(&params, 1.0, 1.0, pitch);
                let points = centerline(&config, 1).points;
                // A rod is a chain: the bolted bend, and one rise segment at
                // least.
                assert!(points.len() >= 3, "{config:?} gave {} points", points.len());

                let spans = spans(&points);
                let rise = &spans[1..];
                let mean = rise.iter().sum::<f32>() / rise.len() as f32;
                for span in rise {
                    // Not exact: even arcs are measured across their chords,
                    // which the lean's curvature shortens by a hair.
                    assert!(
                        (span - mean).abs() < 0.02 * mean,
                        "{config:?}: a {span} m segment among a mean of {mean}"
                    );
                }
            }
        }
    }

    /// The bolted segment is the *whole* bend, at every length. Any of it left
    /// over would be a short second segment among the even ones, mistuned and
    /// free to move.
    #[test]
    fn the_bend_is_the_first_segment_and_nothing_more() {
        for length in [MIN_LENGTH, 0.4, 0.75, 1.6] {
            for pitch in [0.0, 1.0] {
                let config = ShootConfig::new(&ShootParams { length, ..params() }, 1.0, 1.0, pitch);
                let axis = axis(&config, 1);
                let cable = Centerline::new(&axis);
                assert_eq!(
                    cable.stations[1],
                    axis.bend_arc() as f32,
                    "{config:?}: the bend ends somewhere other than the first segment"
                );
                let top = bend_point(axis.bend_angle());
                assert!(
                    Vec3::from(cable.points[1])
                        .abs_diff_eq(Vec3::new(top.x as f32, 0.0, top.z as f32), 1e-6),
                    "{config:?}: the second point is not the top of the bend"
                );
            }
        }
    }

    /// A rod is one radius and a shoot is not, so the drawn taper is the whole
    /// of what makes a flexible cane read as the shoots beside it. It has to
    /// run bud to tip like the stem mesh, and the rod has to sit inside it.
    #[test]
    fn a_cable_is_drawn_tapering_and_simulated_between_its_ends() {
        for config in both() {
            let cable = centerline(&config, 3);
            let widths = &cable.widths;

            assert_eq!(widths.len(), cable.points.len(), "one width per point");
            assert!((widths[0] - 2.0 * config.radius).abs() < 1e-6, "the bud");
            let tip = widths.last().copied().unwrap();
            assert!(
                (tip - 2.0 * config.radius * TIP_TAPER as f32).abs() < 1e-6,
                "the tip"
            );
            assert!(tip < cable.thickness && cable.thickness < widths[0]);
        }
    }

    /// A cane is drawn by these rather than by its curve, so each tube has to
    /// stand where its segment's frame puts it — centered on the midpoint, +Z
    /// along the segment — reach past *both* ends of it, and carry the same
    /// taper the curve is drawn at.
    ///
    /// The overreach is the point: neighbouring tubes are butt-jointed and turn
    /// about 45° at the bend, where two that merely met would open a wedge.
    #[test]
    fn a_canes_tubes_cover_the_segments_they_stand_on() {
        for config in both() {
            let cable = centerline(&config, 3);
            let tubes = segment_tubes(&cable, config.sides as usize);

            assert_eq!(
                tubes.len(),
                segment_frames(&cable.points).len(),
                "one per segment"
            );

            for (index, (tube, span)) in tubes.iter().zip(spans(&cable.points)).enumerate() {
                let (low, high) = bounds(tube, 2);
                assert!(
                    low < -span / 2.0 && high > span / 2.0,
                    "tube {index} stops inside its own segment",
                );
                // Widest at the base, which is the taper the curve carries
                // there.
                let (_, radius) = bounds(tube, 0);
                assert!(
                    (2.0 * radius - cable.widths[index]).abs() < 1e-6,
                    "tube {index} is not drawn at the cable's width",
                );
            }
        }
    }

    /// A leaf on a cane rides the rod segment that carries it, so all three of
    /// these have to hold: the prim is named what Newton names that body, its
    /// frame is the one Newton builds, and rebasing a placement into it puts
    /// the leaf back where it was. The bolted first segment stays bare.
    #[test]
    fn a_leaf_on_a_cane_rides_the_segment_that_carries_it() {
        assert_eq!(segment_name(2), "Cable_edge_body_2");
        for config in both() {
            let (axis, leaves) = nodes(&config, 3);
            let cable = Centerline::new(&axis);
            let points = &cable.points;
            let frames = segment_frames(points);

            assert_eq!(frames.len(), points.len() - 1, "one frame per segment");
            for (index, frame) in frames.iter().enumerate() {
                let (from, to) = (Vec3::from(points[index]), Vec3::from(points[index + 1]));
                let along = frame.rotation * Vec3::Z;
                assert!(frame.translation.abs_diff_eq((from + to) * 0.5, 1e-6));
                assert!(
                    along.abs_diff_eq((to - from).normalize(), 1e-6),
                    "+Z runs along the segment"
                );
            }

            assert!(!leaves.is_empty(), "a cane with no leaves proves nothing");
            for leaf in &leaves {
                let index = segment_of(&cable.stations, leaf.station);
                assert!(index > 0, "nothing hangs on the bolted first segment");
                assert!(
                    (cable.stations[index]..=cable.stations[index + 1]).contains(&leaf.station),
                    "{} sits outside segment {index}",
                    leaf.name
                );

                let placement = placed(leaf.position, leaf.yaw, Vec2::ZERO, 1.0);
                let hung = frames[index] * rebase(&frames[index], &placement);
                assert!(
                    hung.translation.abs_diff_eq(placement.translation, 1e-6)
                        && hung.rotation.abs_diff_eq(placement.rotation, 1e-6),
                    "rebasing into a segment and back is the identity"
                );
            }
        }
    }

    /// The slider is a rate, so it has to land near the fraction it names --
    /// and a stream sharing a constant with `salt` would not. What it deals
    /// out is a pitch, and every one of them has to be a stray shoot's.
    #[test]
    fn the_stray_share_matches_the_rate_it_was_asked_for() {
        for rate in [0.0, 0.03, 0.2] {
            let params = ShootParams {
                stray: rate,
                ..params()
            };
            let pitches: Vec<f32> = (0..4000)
                .map(|order| stray_pitch(&params, 7, order))
                .filter(|pitch| *pitch > 0.0)
                .collect();
            let share = pitches.len() as f32 / 4000.0;
            assert!(
                (share - rate).abs() < 0.015,
                "asked for {rate}, got {share}"
            );
            let range = STRAY_PITCH.0 as f32..=STRAY_PITCH.1 as f32;
            assert!(pitches.iter().all(|pitch| range.contains(pitch)));
        }
    }

    fn mesh(config: &ShootConfig, seed: u64) -> MeshData {
        strand_mesh(&shoot_strand(&axis(config, seed), config)).expect("every shoot skins")
    }

    /// One shoot's canopy, with the axis it was hung on — every leaf test
    /// needs both, because a node only means anything against the curve it was
    /// placed on.
    fn nodes(config: &ShootConfig, seed: u64) -> (ShootAxis, Vec<LeafNode>) {
        let axis = axis(config, seed);
        let nodes = leaf_nodes(config, &axis, seed);
        (axis, nodes)
    }

    /// The whole placement contract: a shoot leaves its bud along +X and ends
    /// up going +Z. If either end drifted, every shoot on every spur would
    /// point somewhere the vine didn't ask for.
    #[test]
    fn a_shoot_leaves_along_x_and_ends_going_up() {
        let config = config();
        let strand = shoot_strand(&axis(&config, 1), &config);

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

    /// A stray shoot leaves its bud the way every shoot does and turns up only
    /// part way: its rise runs pitched out over the side it left on, it is
    /// longer than it would otherwise have been, and it is the kind that bends.
    #[test]
    fn a_stray_shoot_rises_pitched_out_over_the_side_it_left_on() {
        let held = config();
        let stray = ShootConfig {
            lean: 0.0,
            ..stray_config(1.0)
        };
        assert!(stray.is_stray() && !held.is_stray());
        assert!((stray.length - held.length * STRAY_LENGTH).abs() < 1e-6);

        let points = centerline(&stray, 1).points;
        assert_eq!(points[0], [0.0, 0.0, 0.0], "starts at the bud");
        let leaving = Vec3::from(points[1]);
        assert!(
            leaving.x > 0.0 && leaving.x > leaving.z,
            "leaves sideways before it turns up, got {leaving:?}"
        );
        let (tip, below) = (
            Vec3::from(points[points.len() - 1]),
            Vec3::from(points[points.len() - 2]),
        );
        let rising = (tip - below).normalize();
        let pitched = Vec3::new(1f32.sin(), 0.0, 1f32.cos());
        assert!(
            rising.abs_diff_eq(pitched, 1e-5),
            "runs at the pitch, got {rising:?}"
        );
        assert!(
            tip.x > held.length * 0.5,
            "and reaches out past the canopy, got {tip:?}"
        );
    }

    #[test]
    fn a_shoot_stands_as_tall_as_its_length() {
        let config = config();
        let mesh = mesh(&config, 1);

        let (z0, z1) = bounds(&mesh, 2);
        assert!(z0 > -config.radius * 2.0, "nothing below the bud, got {z0}");
        assert!(
            (z1 - config.length).abs() < config.radius * 2.0,
            "the tip lands at `length`, got {z1}"
        );

        // And it stays a narrow thing: the bend's reach plus the lean, no more.
        let slack = (BEND_RADIUS + config.lean as f64 + config.radius as f64 * 2.0) as f32;
        let (x0, x1) = bounds(&mesh, 0);
        assert!(x0 > -slack && x1 < slack, "{x0}..{x1}");
        let (y0, y1) = bounds(&mesh, 1);
        assert!(y0 > -slack && y1 < slack, "{y0}..{y1}");
    }

    /// A shoot shorter than its own bend has nowhere to put the rise. The
    /// config clamps rather than letting the strand fold back on itself, which
    /// would give curvo a rail that doubles over and a mesh full of NaN.
    #[test]
    fn a_shoot_asked_for_at_the_stops_still_builds() {
        // A pitch clamps short of the right angle that would leave the bend
        // nothing to be bolted by, and a negative one to a held shoot.
        assert_eq!(stray_config(9.0).pitch, STRAY_PITCH.1 as f32);
        assert!(!stray_config(-1.0).is_stray());

        let positions = |mesh: &Mesh| -> Vec<[f32; 3]> {
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|a| a.as_float3())
                .expect("with positions")
                .to_vec()
        };
        for config in [
            ShootConfig::new(
                &ShootParams {
                    length: 0.0,
                    radius: 0.0,
                    lean: 0.0,
                    sides: 0,
                    detail: 0,
                    internode: 0.0,
                    ..params()
                },
                0.0,
                0.0,
                0.0,
            ),
            ShootConfig::new(&params(), 4.0, 4.0, 0.0),
            ShootConfig::new(&params(), 0.0, 0.0, 9.0),
            ShootConfig::new(&params(), 4.0, 4.0, 9.0),
        ] {
            let built = build_shoot(&config, 1).expect("builds at the stops");
            let points: Vec<[f32; 3]> = match &built.drawn {
                Drawn::Stem(stem) => positions(stem),
                Drawn::Tubes(tubes) => tubes.iter().flat_map(&positions).collect(),
            };
            assert!(!points.is_empty(), "{config:?} came out empty");
            assert!(
                points.iter().flatten().all(|c| c.is_finite()),
                "{config:?} came out with a NaN in it"
            );
        }
    }

    #[test]
    fn a_shoot_with_no_lean_is_straight_above_the_bend() {
        let config = config_with(|p| p.lean = 0.0);
        let strand = shoot_strand(&axis(&config, 3), &config);
        for point in strand.points.iter().filter(|p| p.z > BEND_RADIUS) {
            assert!(
                (point.x - BEND_RADIUS).abs() < 1e-12 && point.y.abs() < 1e-12,
                "the rise is plumb without a lean, got {point:?}"
            );
        }
    }

    #[test]
    fn shoot_stems_are_reproducible() {
        assert_eq!(mesh(&config(), 9).points, mesh(&config(), 9).points);
        assert_ne!(mesh(&config(), 9).points, mesh(&config(), 10).points);
    }

    /// Nodes climb the shoot in order, start where it has finished standing
    /// up, and stop short of the growing point. The bend is left bare on
    /// purpose: a leaf's posture is a bearing about Z, which only means
    /// "around the stem" once the stem is vertical.
    #[test]
    fn leaves_climb_the_shoot_from_its_base_to_its_tip() {
        let (axis, nodes) = nodes(&config(), 1);
        assert!(
            nodes.len() > 4,
            "a default shoot is leafy, got {}",
            nodes.len()
        );

        let stations: Vec<f32> = nodes.iter().map(|n| n.station).collect();
        assert!(
            stations.windows(2).all(|w| w[0] < w[1]),
            "they climb rather than double back: {stations:?}"
        );
        assert!(
            stations[0] >= axis.bend_arc() as f32 - 1e-6,
            "the first is at the top of the bend, got {}",
            stations[0]
        );
        assert!(
            *stations.last().unwrap() < (axis.length() - TIP_CLEARANCE * 0.5) as f32,
            "and the last stops short of the tip, got {}",
            stations.last().unwrap()
        );
    }

    /// A leaf hangs off the stem's own centerline, not off a resampling of it.
    /// That is the whole reason [`ShootAxis`] was lifted out of
    /// [`shoot_strand`], and the check is exact because both go through
    /// [`ShootAxis::at`].
    #[test]
    fn a_leaf_sits_on_the_shoots_axis() {
        for config in both() {
            let (axis, nodes) = nodes(&config, 1);
            for node in &nodes {
                let on_axis = axis.at(node.station as f64);
                let off = (node.position.x as f64 - on_axis.x)
                    .hypot(node.position.y as f64 - on_axis.y)
                    .hypot(node.position.z as f64 - on_axis.z);
                assert!(off < 1e-6, "{} sits on the axis, off by {off}", node.name);
            }
        }
    }

    /// Grapevine phyllotaxis is distichous — successive leaves half a turn
    /// apart, in two ranks up opposite sides. The failure this catches is a
    /// canopy that grew out one side of every shoot, which reads as obviously
    /// wrong and would survive every other test here.
    #[test]
    fn leaves_alternate_around_the_shoot() {
        let (_, nodes) = nodes(&config(), 1);
        for pair in nodes.windows(2) {
            let turn = (pair[1].yaw - pair[0].yaw) as f64;
            // Wrapped into [0, 2π), so a half turn either way reads as zero.
            let apart = (turn.rem_euclid(TAU) - PI).abs();
            // The slack is f32: the rank angle accumulates past 60 rad up a
            // leafy shoot, where a single-precision step is a few 1e-6.
            assert!(
                apart <= PHYLLOTAXY_DRIFT + 1e-4,
                "{} follows {} half a turn round, off by {apart} rad",
                pair[1].name,
                pair[0].name
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

        let (axis, nodes) = nodes(&config(), 1);
        let mature: Vec<f32> = nodes
            .iter()
            .filter(|n| (axis.length() - n.station as f64) > EXPANDING_REACH * 1.5)
            .map(|n| n.maturity)
            .collect();

        assert!(
            mature.len() > 3,
            "most of a shoot is mature, got {mature:?}"
        );
        assert!(
            mature.iter().all(|m| (m - 1.0).abs() < 1e-6),
            "a grown leaf is exactly full size before its vigour, got {mature:?}"
        );
    }

    /// Age reads off position: a shoot lays its leaves down in order and keeps
    /// extending past them, so the ones near the growing point are the newest
    /// and the smallest.
    #[test]
    fn leaves_shrink_toward_the_growing_tip() {
        let (_, nodes) = nodes(&config(), 1);
        let sizes: Vec<f32> = nodes.iter().map(|n| n.maturity).collect();
        let mean = |s: &[f32]| s.iter().sum::<f32>() / s.len() as f32;

        let (base, tip) = (mean(&sizes[..3]), mean(&sizes[sizes.len() - 3..]));
        assert!(
            tip < base * 0.8,
            "the tip carries the young ones: base {base}, tip {tip}, all {sizes:?}"
        );
    }

    /// The canopy's randomness is a stream of its own, so tuning how leafy a
    /// shoot is never re-rolls the stem holding them up.
    #[test]
    fn changing_the_internode_leaves_the_stem_alone() {
        let stem = |internode| mesh(&config_with(|p| p.internode = internode), 7).points;
        assert_eq!(stem(0.07), stem(0.0), "same stem, however many leaves");
    }

    /// A spacing too fine to step by is how the canopy is turned off. Without
    /// the guard the station loop would never terminate.
    #[test]
    fn a_shoot_with_no_leaves_asked_for_still_builds() {
        for internode in [0.0, MIN_INTERNODE as f32 * 0.5] {
            let config = config_with(|p| p.internode = internode);
            assert!(nodes(&config, 1).1.is_empty(), "no leaves at {internode}");
            assert!(!mesh(&config, 1).points.is_empty(), "but still a stem");
        }
    }

    // ─── The layer ──────────────────────────────────────────────────

    /// `leaf_droop` reaches no part of a stem, so two shoots that differ only
    /// in it have to share a mesh and still hang their leaves at their own
    /// angle — the rule that a field a builder ignores must not reach the
    /// metric.
    #[test]
    fn the_droop_is_placement_and_never_costs_a_mesh() {
        let flat = config_with(|p| p.leaf_droop = 0.0);
        let steep = config_with(|p| p.leaf_droop = 1.2);
        assert_ne!(flat, steep);
        assert_eq!(ShootMetric.distance(&flat, &steep), 0.0);
        assert_eq!(mesh(&flat, 1).points, mesh(&steep, 1).points);
    }

    /// The two axes a vine rolls per shoot both have to reach the mesh, or the
    /// budget buys a single stem for the whole vineyard.
    #[test]
    fn vigour_and_spacing_both_move_a_shoot_apart() {
        let nominal = config();
        for varied in [
            ShootConfig::new(&params(), 1.15, 1.0, 0.0),
            ShootConfig::new(&params(), 1.0, 1.12, 0.0),
        ] {
            assert!(
                ShootMetric.distance(&nominal, &varied) > 0.0,
                "{varied:?} has to be tellable from the nominal shoot"
            );
        }
        // And the pitch a stray shoot keeps, among the stray ones.
        assert!(ShootMetric.distance(&stray_config(0.5), &stray_config(0.8)) > 0.0);
    }

    /// One codebook over both populations would spend itself on the strays —
    /// a pitch puts a shoot further from every held one than they are from
    /// each other — and leave the canopy one stem. So each gets the budget.
    #[test]
    fn stray_shoots_are_quantized_apart_from_the_held_ones() {
        let mut population: Vec<ShootConfig> = (0..40)
            .map(|i| ShootConfig::new(&params(), 0.85 + 0.3 * i as f32 / 39.0, 1.0, 0.0))
            .collect();
        population.extend((0..4).map(|i| stray_config(0.3 + 0.25 * i as f32)));

        let (representatives, assignment) = quantize(&population, 4);
        assert_eq!(
            representatives.iter().filter(|r| !r.is_stray()).count(),
            4,
            "the held shoots keep the whole budget, got {representatives:?}"
        );
        assert_eq!(
            representatives.len(),
            8,
            "and the stray ones get one of their own"
        );
        for (config, drew) in population.iter().zip(&assignment) {
            assert_eq!(
                config.is_stray(),
                representatives[*drew as usize].is_stray(),
                "a shoot draws its own kind"
            );
        }
        // With nothing strayed there is one population, and one budget.
        assert_eq!(quantize(&population[..40], 4).0.len(), 4);
    }

    /// End to end: every shoot the vines hung comes out carrying a stem from
    /// the library and a canopy of its own.
    #[test]
    fn every_shoot_draws_a_stem_and_hangs_its_own_leaves() {
        let mut app = testing::grown(VineyardParams::default());

        let shoots = organs::<ShootConfig>(app.world_mut());
        assert!(
            shoots.len() > 100,
            "the fixture grew shoots, got {}",
            shoots.len()
        );

        // Two shoots that drew the same stem still have to differ in canopy.
        let mut by_part: std::collections::BTreeMap<String, Vec<String>> = default();
        for shoot in &shoots {
            let entity = testing::prim(app.world_mut(), &shoot.path.split('/').collect::<Vec<_>>())
                .expect("the shoot is on the scene graph");
            let children = named_children(app.world_mut(), entity);
            let stem = children
                .iter()
                .find(|(name, _)| name == STEM)
                .expect("every shoot carries a stem");
            assert!(
                children.len() > 1,
                "{}: and leaves on it, got {children:?}",
                shoot.path
            );
            let part = app
                .world()
                .entity(stem.1)
                .get::<UsdReference>()
                .unwrap()
                .0
                .clone();
            by_part.entry(part).or_default().push(shoot.path.clone());
        }

        let library = app.world().resource::<Prototypes>();
        for part in by_part.keys() {
            assert!(library.get(part).is_some(), "{part} is not in the library");
        }
        // Nothing bends at the default rate, and a tube built anyway is
        // authored into the scene and referenced by nothing.
        assert!(
            library.iter().all(|(name, _)| !name.starts_with(CANE)),
            "a shoot that never bends built a cane tube"
        );

        let shared = by_part
            .values()
            .find(|paths| paths.len() > 1)
            .expect("some stem is shared by more than one shoot");
        let leaves = |app: &mut App, path: &str| -> Vec<Transform> {
            let entity =
                testing::prim(app.world_mut(), &path.split('/').collect::<Vec<_>>()).unwrap();
            named_children(app.world_mut(), entity)
                .into_iter()
                .filter(|(name, _)| name != STEM)
                .map(|(_, child)| *app.world().entity(child).get::<Transform>().unwrap())
                .collect()
        };
        let (a, b) = (
            leaves(&mut app, &shared[0]),
            leaves(&mut app, &shared[shared.len() - 1]),
        );
        assert_eq!(a.len(), b.len(), "the same nodes, from the same stem");
        assert_ne!(a, b, "but different leaves hung on them");
        for (a, b) in a.iter().zip(&b) {
            assert!(
                (a.translation - b.translation).length() < 1e-6,
                "a node is where the stem put it, on both shoots"
            );
        }
    }

    /// The other end of the same contract: a stray shoot draws nothing itself,
    /// hangs a tube on every rod segment the solver drives instead, and keeps
    /// its leaves around the stem it lies over with.
    ///
    /// The curve is simulated and never drawn, so a segment left bare is a
    /// stretch of invisible cane — and one indexed off by one is a cane drawn
    /// at the wrong taper. Both compose to a scene that loads. So would a leaf
    /// placed about the vertical instead of the stem, lying along a cane
    /// pitched over far enough.
    #[test]
    fn a_stray_shoot_draws_a_tube_on_every_segment() {
        let mut app = testing::grown(VineyardParams {
            shoot: ShootParams {
                stray: 1.0,
                // Droop is the one part of a leaf's posture that turns its
                // long axis off the stem, so the check below has it flat.
                leaf_droop: 0.0,
                ..default()
            },
            ..default()
        });

        let shoots = organs::<ShootConfig>(app.world_mut());
        assert!(shoots.len() > 100, "the fixture grew shoots");
        assert!(shoots.iter().all(|shoot| shoot.config.is_stray()));

        let mut drawn: Vec<String> = Vec::new();
        for shoot in &shoots {
            let entity = testing::prim(app.world_mut(), &shoot.path.split('/').collect::<Vec<_>>())
                .expect("the shoot is on the scene graph");
            let children = named_children(app.world_mut(), entity);
            assert!(
                !children.iter().any(|(name, _)| name == STEM),
                "{}: a cane draws by its segments, not by a stem of its own",
                shoot.path
            );

            // The pitch its leaves were turned over by is the representative's,
            // and the cable's second point — the top of the bend — carries it.
            let cable = children
                .iter()
                .find(|(name, _)| name == CABLE)
                .expect("a stray shoot carries a cable");
            let [x, _, z] = app.world().entity(cable.1).get::<Cable>().unwrap().0.points[1];
            let pitch = (1.0 - z / BEND_RADIUS as f32).atan2(x / BEND_RADIUS as f32);
            assert!(
                (STRAY_PITCH.0 as f32..=STRAY_PITCH.1 as f32).contains(&pitch),
                "{}: lies over at {pitch} rad",
                shoot.path
            );
            let rise = Vec3::new(pitch.sin(), 0.0, pitch.cos());

            let segments: Vec<(String, Entity)> = children
                .into_iter()
                .filter(|(name, _)| name.contains("_edge_body_"))
                .collect();
            assert!(!segments.is_empty(), "{}: no rod segments", shoot.path);

            for (name, segment) in segments {
                let frame = *app.world().entity(segment).get::<Transform>().unwrap();
                let (tubes, leaves): (Vec<_>, Vec<_>) = named_children(app.world_mut(), segment)
                    .into_iter()
                    .partition(|(child, _)| child == STEM);
                assert_eq!(tubes.len(), 1, "{}/{name} draws one tube", shoot.path);
                drawn.push(
                    app.world()
                        .entity(tubes[0].1)
                        .get::<UsdReference>()
                        .expect("the tube references a library part")
                        .0
                        .clone(),
                );
                for (leaf, entity) in leaves {
                    let hung = frame * *app.world().entity(entity).get::<Transform>().unwrap();
                    let along = hung.rotation * Vec3::X;
                    assert!(
                        along.dot(rise).abs() < 1e-4,
                        "{}/{name}/{leaf} lies along the cane rather than across it",
                        shoot.path
                    );
                }
            }
        }

        let library = app.world().resource::<Prototypes>();
        for part in &drawn {
            assert!(library.get(part).is_some(), "{part} is not in the library");
        }
        // Every shoot strayed, so no stem was registered: a part nothing
        // references is authored into the scene all the same.
        assert!(
            library
                .iter()
                .all(|(name, _)| !name.starts_with(&format!("{PART}_"))),
            "a stem was built for shoots that never draw one"
        );
    }

    /// A replant is a single buried shoot, so it has to come out of this layer
    /// carrying a stem like any other — the branch that would otherwise be
    /// silently canopy-less.
    #[test]
    fn a_replants_shoot_is_built_like_any_other() {
        let mut app = testing::grown(VineyardParams::default());
        let replant = organs::<vine::VineConfig>(app.world_mut())
            .into_iter()
            .find(|v| !v.config.is_mature())
            .expect("the default planting holds replants");

        let path: Vec<String> = replant
            .path
            .split('/')
            .map(str::to_string)
            .chain([vine::REPLANT_SHOOT.to_string()])
            .collect();
        let entity = testing::prim(
            app.world_mut(),
            &path.iter().map(String::as_str).collect::<Vec<_>>(),
        )
        .expect("a replant's shoot is on the scene graph");

        let children = named_children(app.world_mut(), entity);
        assert!(
            children.iter().any(|(name, _)| name == STEM),
            "{}: it carries a stem, got {children:?}",
            replant.path
        );
        assert!(children.len() > 1, "and leaves on it");
    }

    /// A rebuild that spends less of the budget must not leave the meshes it
    /// no longer uses in the library, exported and referenced by nothing.
    #[test]
    fn shrinking_the_budget_drops_the_stale_meshes() {
        let meshes = |app: &App| {
            app.world()
                .resource::<Prototypes>()
                .iter()
                .filter(|(name, _)| name.starts_with(&format!("{PART}_")))
                .count()
        };

        let mut app = testing::grown(VineyardParams {
            shoot: ShootParams {
                variations: 5,
                ..params()
            },
            ..default()
        });
        assert_eq!(meshes(&app), 5, "the budget is spent");

        app.world_mut().resource_mut::<ShootParams>().variations = 2;
        app.update();
        assert_eq!(meshes(&app), 2, "the three it stopped using are gone");
    }

    /// Two stems the same green would make a shared canopy read as one shoot
    /// repeated, which is the whole thing the budget is spent avoiding.
    #[test]
    fn every_stem_mesh_gets_its_own_shade() {
        let app = testing::grown(VineyardParams::default());
        let shades: Vec<[f32; 3]> = app
            .world()
            .resource::<Prototypes>()
            .iter()
            .filter(|(name, _)| name.starts_with(&format!("{PART}_")))
            .map(|(_, part)| part.color)
            .collect();

        assert!(shades.len() > 1, "there is more than one to tell apart");
        for (i, a) in shades.iter().enumerate() {
            for b in shades.iter().skip(i + 1) {
                assert_ne!(a, b, "two stem meshes came out the same shade");
            }
        }
    }
}
