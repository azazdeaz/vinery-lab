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
//! stands up on its own. No frame has to be transported along the spur.
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
use super::util::color;
use super::util::strand::{Bark, Strand, strand_mesh};
use super::util::{material};
use super::{Grow, Rng, SceneParams, salt};
use crate::quantize::{Metric, farthest_first};
use crate::scene::{Geometry, Library, Order, Surface, configs_changed, placed};

/// The mesh-library prefix this element registers its stems under.
pub const PART: &str = "Shoot";

/// The prim a shoot's stem takes, below the shoot itself. A child rather than
/// the shoot prim itself, because a shoot has leaves hanging off it and
/// geometry prims carry no children.
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

/// Length of the bend along its own curve: a quarter turn of [`BEND_RADIUS`].
const BEND_ARC: f64 = BEND_RADIUS * FRAC_PI_2;

// ─── Leaf constants ─────────────────────────────────────────────────

/// Salt splitting the stems' randomness off the scene seed, so that this layer
/// and the ones either side of it never draw from the same stream.
const STEM_STREAM: u64 = 0x589A_B41E_A1D2_F35B;

/// The same, splitting the leaves off the stems, so tuning the canopy never
/// reshapes the shoot underneath it — the same split [`vine`](super::vine)
/// keeps between its wood and its shoots.
const LEAF_STREAM: u64 = 0x2545_F491_4F6C_DD1D;

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
}

impl ShootConfig {
    /// The shoot these params call for, at this `vigour` and node `spacing`.
    ///
    /// Both are multipliers about `1.0`, drawn per shoot by whoever placed it —
    /// see [`vine`](super::vine). Vigour lengthens and thickens the shoot
    /// together, which is what more light does, and leaves the spacing alone,
    /// so a vigorous shoot also carries more leaves.
    pub fn new(params: &ShootParams, vigour: f32, spacing: f32) -> Self {
        Self {
            length: (params.length * vigour).max(MIN_LENGTH),
            radius: (params.radius * vigour).max(0.0005),
            lean: params.lean.max(0.0),
            sides: params.sides.max(3),
            detail: params.detail.max(1),
            internode: (params.internode * spacing).max(0.0),
            leaf_droop: params.leaf_droop,
        }
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

#[derive(Resource, Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct ShootParams {
    /// How many distinct stem meshes the scene may hold.
    ///
    /// A budget, not a count: the shoots are clustered and this is how many
    /// representatives the clustering may keep.
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
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<ShootParams>().add_systems(
        PreUpdate,
        build.in_set(Grow::Shoots).run_if(
            configs_changed::<ShootConfig>
                // The leaves this layer hangs are authored from `LeafParams`,
                // and nothing re-authors the shoot configs when those change.
                .or_else(resource_changed::<leaf::LeafParams>),
        ),
    );
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
    fn new(config: &ShootConfig, rng: &mut Rng) -> Self {
        Self {
            height: config.length as f64,
            azimuth: rng.unit() * TAU,
            phase: rng.unit() * TAU,
            lean: config.lean as f64 * rng.range(0.5, 1.0),
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

/// One shoot's stem, in its own local frame.
fn shoot_strand(axis: &ShootAxis, config: &ShootConfig) -> Strand {
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
            config.radius as f64 * (1.0 + (TIP_TAPER - 1.0) * t)
        })
        .collect();

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
    let mut station = BEND_ARC;
    let mut index = 0usize;

    while station <= length - TIP_CLEARANCE {
        let slide = rng.range(-STATION_JITTER, STATION_JITTER) * internode;
        let at = (station + slide).clamp(BEND_ARC, length);
        // On the centerline rather than out at the stem's surface: that buries
        // the petiole's free end under a few millimeters of stem, which is the
        // trick `SHOOT_EMBED` already uses one level up and is what guarantees
        // no gap however the blade ends up turned.
        let position = axis.at(at);

        nodes.push(LeafNode {
            name: format!("Leaf_{index:02}"),
            position: Vec3::new(position.x as f32, position.y as f32, position.z as f32),
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

// ─── Building ───────────────────────────────────────────────────────

/// One representative shoot: its stem, and the nodes its leaves hang on.
struct ShootBuild {
    stem: Mesh,
    nodes: Vec<LeafNode>,
}

fn build_shoot(config: &ShootConfig, seed: u64) -> anyhow::Result<ShootBuild> {
    let axis = ShootAxis::new(config, &mut Rng::new(seed));
    Ok(ShootBuild {
        stem: strand_mesh(&shoot_strand(&axis, config))?.to_mesh(),
        nodes: leaf_nodes(config, &axis, seed),
    })
}

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

    let mut grown: Vec<(Order, Entity, ShootConfig)> = shoots
        .iter()
        .map(|(entity, order, config)| (*order, entity, *config))
        .collect();
    grown.sort_by_key(|(order, ..)| *order);

    let configs: Vec<ShootConfig> = grown.iter().map(|(_, _, config)| *config).collect();
    let book = farthest_first(&configs, params.variations.max(1) as usize, 0.0, &ShootMetric);

    let mut built: Vec<(Vec<LeafNode>, Geometry)> = Vec::with_capacity(book.len());
    for (index, config) in book.representatives.iter().enumerate() {
        let seed = scene.seed ^ STEM_STREAM ^ salt(index as u64);
        let ShootBuild { stem, nodes } = build_shoot(config, seed)?;
        built.push((nodes, library.part(PART, index, stem, surface(seed))));
    }

    let mut leaf_order = 0u64;
    for ((order, entity, config), drew) in grown.iter().zip(&book.assignment) {
        let (nodes, geometry) = &built[*drew as usize];
        let mut shoot = commands.entity(*entity);
        // The layer owns everything below a shoot, and a rebuild may hang a
        // different number of leaves than the last one did.
        shoot.despawn_children();
        shoot.with_child((Name::new(STEM), geometry.clone()));

        let mut rng = Rng::new(scene.seed ^ LEAF_STREAM ^ salt(order.0));
        // Drawn once, so the two ranks are not lined up with the shoot's lean.
        let bearing = rng.unit() * TAU;

        for node in nodes {
            // Five draws per node: the bearing's wander, the droop, the twist
            // about the blade's own long axis, its vigour, then which blade it
            // drew.
            let turn = rng.range(-LEAF_SPREAD, LEAF_SPREAD);
            let sag = rng.range(1.0 - LEAF_DROOP_JITTER, 1.0 + LEAF_DROOP_JITTER);
            let roll = rng.range(-LEAF_ROLL, LEAF_ROLL);
            let vigour = rng.range(1.0 - LEAF_VIGOUR, 1.0 + LEAF_VIGOUR);
            let outline = (rng.unit() * leaf::OUTLINES.len() as f64) as usize;

            leaf_order += 1;
            shoot.with_child((
                Name::new(node.name.clone()),
                placed(
                    node.position,
                    // Wrapped, because the rank angle runs past a full turn by
                    // the fourth node.
                    (bearing + node.yaw as f64 + turn).rem_euclid(TAU) as f32,
                    // A leaf is drawn flat along +X with its face toward +Z, so
                    // the tilt is the whole of its posture: X twists the blade
                    // about its own long axis, Y pitches its tip down.
                    Vec2::new(
                        roll as f32,
                        (config.leaf_droop as f64 * node.maturity as f64 * sag) as f32,
                    ),
                    // `leaf::AREA` is the same for every blade, so a scale is a
                    // size in meters whichever one this node drew.
                    node.maturity * vigour as f32,
                ),
                Visibility::default(),
                leaf::LeafConfig::new(&leaf_params, outline),
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
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::VineyardParams;
    use crate::elements::util::testing::{self, bounds, named_children, organs};
    use crate::elements::util::mesh::MeshData;
    use crate::elements::vine;
    use crate::scene::{Prototypes, UsdReference};

    fn params() -> ShootParams {
        ShootParams::default()
    }

    /// The default shoot, at nominal vigour and spacing.
    fn config() -> ShootConfig {
        ShootConfig::new(&params(), 1.0, 1.0)
    }

    /// The same, with `edit` applied to the params first.
    fn config_with(edit: impl FnOnce(&mut ShootParams)) -> ShootConfig {
        let mut params = params();
        edit(&mut params);
        ShootConfig::new(&params, 1.0, 1.0)
    }

    fn axis(config: &ShootConfig, seed: u64) -> ShootAxis {
        ShootAxis::new(config, &mut Rng::new(seed))
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
            ),
            ShootConfig::new(&params(), 4.0, 4.0),
        ] {
            let built = build_shoot(&config, 1).expect("builds at the stops");
            let points = built
                .stem
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|a| a.as_float3())
                .expect("with positions");
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
        assert!(nodes.len() > 4, "a default shoot is leafy, got {}", nodes.len());

        let heights: Vec<f32> = nodes.iter().map(|n| n.position.z).collect();
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
        let (axis, nodes) = nodes(&config(), 1);
        for node in &nodes {
            let on_axis = axis.at_height(node.position.z as f64);
            let off = (node.position.x as f64 - on_axis.x).hypot(node.position.y as f64 - on_axis.y);
            assert!(off < 1e-6, "{} sits on the axis, off by {off}", node.name);
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
            .filter(|n| (axis.length() - n.position.z as f64) > EXPANDING_REACH * 1.5)
            .map(|n| n.maturity)
            .collect();

        assert!(mature.len() > 3, "most of a shoot is mature, got {mature:?}");
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
            ShootConfig::new(&params(), 1.15, 1.0),
            ShootConfig::new(&params(), 1.0, 1.12),
        ] {
            assert!(
                ShootMetric.distance(&nominal, &varied) > 0.0,
                "{varied:?} has to be tellable from the nominal shoot"
            );
        }
    }

    /// End to end: every shoot the vines hung comes out carrying a stem from
    /// the library and a canopy of its own.
    #[test]
    fn every_shoot_draws_a_stem_and_hangs_its_own_leaves() {
        let mut app = testing::grown(VineyardParams::default());

        let shoots = organs::<ShootConfig>(app.world_mut());
        assert!(shoots.len() > 100, "the fixture grew shoots, got {}", shoots.len());

        // Two shoots that drew the same stem still have to differ in canopy.
        let mut by_part: std::collections::BTreeMap<String, Vec<String>> = default();
        for shoot in &shoots {
            let entity = testing::prim(
                app.world_mut(),
                &shoot.path.split('/').collect::<Vec<_>>(),
            )
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
            let part = app.world().entity(stem.1).get::<UsdReference>().unwrap().0.clone();
            by_part.entry(part).or_default().push(shoot.path.clone());
        }

        let library = app.world().resource::<Prototypes>();
        for part in by_part.keys() {
            assert!(library.get(part).is_some(), "{part} is not in the library");
        }

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
