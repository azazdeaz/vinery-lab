//! Vine element — the permanent woody framework of a grapevine.
//!
//! A vine is a **trunk** rising from the ground to the **head**, where it
//! turns into one or two **cordons** running along the fruiting wire. Each
//! cordon carries **spurs**: the short pruning stubs whose knuckles are the
//! strongest visual signature of a pruned vine. A **graft union** swells the
//! trunk about 15 cm up, as it does on essentially every commercial vine.
//!
//! Only the permanent wood is *shaped* here. The annual growth that hangs off
//! the spurs belongs to its own elements — [`shoot`](super::shoot) is the first
//! of them, and this module decides where on its spurs they sit. Leaves and
//! fruit hang off the shoots the same way.
//!
//! # Structure
//!
//! Every part is a [`Strand`]: a polyline of control points with a radius at
//! each, skinned into a closed tube. So this module places control points and
//! nothing else — [`strand`](super::util::strand) owns everything about turning
//! them into triangles. Trunk, cordon and spur differ only in where their
//! points go.
//!
//! Cordons and spurs *start inside* the part they grow from — a cordon eight
//! centimeters below the head, on the trunk's axis. Interpenetrating tubes
//! need no CSG and leave no coincident surfaces to z-fight, and the resulting
//! lumpy junction is what a real cordon-trained head looks like anyway.
//!
//! # Local frame
//!
//! A vine is built with the **origin at the trunk base, on the ground**: +Z up,
//! **+X along the row** so cordons run along ±X, +Y across it. Planting
//! therefore only ever needs a yaw about Z, and never has to know which way a
//! strand was built.
//!
//! # The layer
//!
//! [`planting`](super::util::planting) authors a [`VineConfig`] per plant;
//! [`build`] turns the distinct configs into meshes. Every plant gets:
//!
//! ```text
//! Vine_007            the planted entity, carrying its VineConfig
//!   Wood              -> parts/Vine_<rep>, shared with every vine that drew it
//!   Shoot_00_0        a ShootConfig of its own, placed on a bud
//!   Shoot_00_1        ...
//! ```
//!
//! The wood is shared and the shoots are not. Which buds pushed comes from the
//! **representative** — a shoot has to sit on a stub that actually got built —
//! while each shoot's own bearing, lean and vigour are drawn per plant, which
//! is what stops a hundred vines off four meshes reading as four meshes.
//!
//! # A replant has no wood
//!
//! Below [`VineConfig::is_mature`] a plant is a rooted cutting in its first
//! seasons: one green shoot out of bare ground and no permanent wood at all. So
//! it builds no mesh — it is a single [`shoot`](super::shoot) buried to
//! [`shoot::PLANT_DEPTH`], which is what keeps the bend at its base
//! underground. Same element, same config, one branch in [`build_vine`].

use std::f64::consts::{PI, TAU};

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use nalgebra::{Point3, Vector3};

use super::shoot;
use crate::quantize::{Metric, farthest_first};
use crate::scene::{
    COLLISION, Geometry, Library, Order, Surface, capsule, configs_changed, placed,
};

use super::util::parcel::ParcelParams;
use super::util::strand::{Bark, Bulge, Strand, strand_mesh};
use super::util::mesh::merge_meshes;
use super::util::{color, material};
use super::{Grow, Rng, SceneParams, salt};

/// The mesh-library prefix this element registers its wood under.
pub const PART: &str = "Vine";

/// The prim a vine's wood takes, below the plant itself.
///
/// A child rather than the plant prim itself, because a vine has shoots
/// hanging off it and geometry prims carry no children — see
/// [`scene`](crate::scene).
pub const WOOD: &str = "Wood";

/// The prim a replant's single shoot takes. Not `Shoot_00_0`: there is no spur
/// and no bud, so a name naming either would be a lie.
pub const REPLANT_SHOOT: &str = "Shoot";

// ─── Shape constants ────────────────────────────────────────────────
//
// Proportions rather than parameters: they are what makes a vine read as a
// vine, and nothing downstream wants to tune them individually.

/// Where the trunk starts, below ground. Buried, so a vine planted on a slope
/// never shows a gap between its base and the terrain.
const TRUNK_BASE_Z: f64 = -0.05;

/// Control points along the trunk, before the graft union's are merged in.
const TRUNK_NODES: usize = 7;

/// Trunk radius at the head, as a fraction of its radius at the base.
const TRUNK_TIP_TAPER: f64 = 0.60;

/// The graft union: a knobby swelling a hand's width above the ground. Its
/// height is absolute, not a fraction of the trunk — a vine grafted onto
/// rootstock carries the scar at the height it was grafted, however tall it
/// later grew.
const GRAFT_HEIGHT: f64 = 0.15;
const GRAFT_WIDTH: f64 = 0.05;
const GRAFT_BULGE: f64 = 0.45;

/// How far below the head a cordon starts, buried inside the trunk.
const CORDON_EMBED: f64 = 0.08;

/// How far out the head's bend reaches before the cordon settles into its run
/// along the wire, and how far it drops doing so.
const HEAD_RUN: f64 = 0.09;
const HEAD_DROP: f64 = 0.012;

/// How far the cordon sags between the head and its tip. A cordon is tied to a
/// wire, so this is a settle, not a catenary.
const CORDON_DROOP: f64 = 0.010;

/// Spacing of the cordon's own control points, before the spur knuckles' are
/// merged in.
const CORDON_STEP: f64 = 0.08;

/// Cordon radius at the tip, as a fraction of its radius at the head.
const CORDON_TIP_TAPER: f64 = 0.45;

/// The knuckle a spur leaves on the cordon it grows from.
const KNUCKLE_WIDTH: f64 = 0.022;
const KNUCKLE_HEIGHT: f64 = 0.55;

/// How far a spur reaches back into the cordon, so the two tubes meet without
/// a seam.
const SPUR_EMBED: f64 = 0.02;

/// Bare cordon left past the last spur, and past the head before the first.
const SPUR_MARGIN: f64 = 0.03;

/// Below this, a spur is not worth any geometry at all.
const MIN_SPUR_LENGTH: f64 = 1e-3;

/// The most shoots one spur is ever asked for. A spur is pruned to two buds,
/// so two is the honest number; the third is headroom for a deliberately lush
/// vine, and it is what fixes how many bud slots a spur offers — see
/// [`shoot_buds`].
const MAX_SHOOTS_PER_SPUR: usize = 3;

/// Where the buds sit along a spur, as fractions of its length. They fill from
/// the tip down, so a spur that pushes one shoot pushes it from the top bud —
/// which is the one that usually wins.
const BUD_TIP: f64 = 0.95;
const BUD_STEP: f64 = 0.35;

/// How far a shoot's bearing wanders off the bud it grew from, in radians.
const BUD_SPREAD: f64 = 0.4;

/// How far a shoot leans off the upright it was built as, in radians — drawn
/// per shoot about both of its own horizontal axes.
const SHOOT_TILT: f64 = 0.14;

/// How much a shoot's vigour varies about the nominal, as a fraction.
///
/// Vigour lengthens and thickens a shoot together, the way more light does — so
/// a vigorous shoot also carries more leaves, since it is longer at the same
/// node spacing.
const SHOOT_VIGOUR: f64 = 0.15;

/// How much a shoot's node spacing varies about the nominal, as a fraction.
///
/// Separate from [`SHOOT_VIGOUR`] because it is the other thing that visibly
/// differs between two shoots of the same length: how densely they set their
/// leaves. Together the two give the shoot layer a codebook worth more than one
/// axis — a budget spent covering a single line buys much less variety.
///
/// Both go into the [`shoot::ShootConfig`] rather than into a placement scale.
/// A scale would be free but invisible to the clustering, so the budget would
/// be spent on one arbitrary size instead of on the range that occurs.
const SHOOT_SPACING: f64 = 0.12;

/// Salt that splits the wood's random stream off the scene seed, so that this
/// layer and the ones either side of it never draw from the same stream. An
/// arbitrary odd constant; only its fixedness matters.
const WOOD_STREAM: u64 = 0x8EBC_6AF0_9C88_C6E3;

/// The same, splitting the shoot placements off the wood — so that every nudge
/// of `shoots_per_spur` does not re-shape the trunk underneath them.
const SHOOT_STREAM: u64 = 0xA076_1D64_78BD_642F;

/// How far a replant sits from any mature vine.
///
/// Further than two mature vines can ever be from each other, so a replant
/// earns a representative however few of them there are.
const REPLANT_APART: f32 = 10.0;

/// How much a plant's girth varies about the nominal, as a fraction.
///
/// A vine thickens with age and vigour while its head stays where the wire is,
/// so this reaches the radii and the spurs and leaves `trunk_height` alone.
///
/// It is also the axis the mesh budget gets spent covering. Without it every
/// mature plant in a parcel would be the same config and the whole vineyard
/// would share one mesh, however high `variations` was set — under the old
/// model the variety came from authoring N differently-seeded prototypes, and
/// under this one it has to be a real difference between the plants.
pub const VINE_VIGOUR: f64 = 0.25;

/// Shortest cordon we will build. Load-bearing rather than cosmetic: a
/// `cordon_gap` wider than the vine spacing would otherwise ask for a
/// zero-length rail, whose total chord length is zero and whose interpolation
/// is entirely NaN.
const MIN_CORDON_REACH: f32 = 0.05;

/// Shortest trunk we will build. A trunk of zero height would put the head at
/// the graft union and divide by the distance between them.
const MIN_TRUNK_HEIGHT: f32 = 0.1;

/// Closest two spurs may be asked to sit. Zero spacing is an infinite number
/// of spurs, and [`spur_positions`] would never terminate.
const MIN_SPUR_SPACING: f32 = 0.01;

// ─── Config ─────────────────────────────────────────────────────────

/// One vine's shape, as planting specified it.
///
/// Everything a mesh is built from, and nothing about where the plant stands —
/// that is the entity's `Transform`. Two vines with equal configs get one mesh
/// between them; two that differ get separate meshes only if the budget
/// stretches to it, which is what [`VineMetric`] decides.
///
/// Clamped on the way in rather than in the shape functions, so the config a
/// metric compares is the plant that actually gets built.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct VineConfig {
    /// How grown the plant is. `1.0` is a mature vine — trunk, cordons and
    /// spurs. Anything less is a replant in its first seasons: one shoot out of
    /// bare ground, at this fraction of a full-grown one.
    ///
    /// The two are different *shapes* rather than different sizes, which is why
    /// [`VineMetric`] steps here rather than reading it as a continuum.
    pub established: f32,
    pub trunk_height: f32,
    pub trunk_radius: f32,
    pub trunk_wobble: f32,
    pub arms: u32,
    /// How far a cordon reaches from the trunk, solved from the row's vine
    /// spacing so neighbouring plants meet without overlapping.
    pub cordon_reach: f32,
    pub cordon_radius: f32,
    pub spur_spacing: f32,
    pub spur_length: f32,
    pub shoots_per_spur: f32,
    pub roughness: f32,
    pub sides: u32,
    pub detail: u32,
}

impl VineConfig {
    /// The plant these params call for, at this stage of establishment and
    /// this `vigour` — a multiplier about `1.0`, drawn per plant. See
    /// [`VINE_VIGOUR`].
    pub fn new(
        params: &VineParams,
        parcel: &ParcelParams,
        established: f32,
        vigour: f32,
    ) -> Self {
        Self {
            established: established.clamp(0.0, 1.0),
            trunk_height: params.trunk_height.max(MIN_TRUNK_HEIGHT),
            trunk_radius: (params.trunk_radius * vigour).max(0.001),
            trunk_wobble: params.trunk_wobble.max(0.0),
            arms: params.arms.clamp(1, 2),
            cordon_reach: cordon_reach(parcel.vine_spacing, params.cordon_gap, params.arms),
            cordon_radius: (params.cordon_radius * vigour).max(0.001),
            spur_spacing: params.spur_spacing.max(MIN_SPUR_SPACING),
            spur_length: (params.spur_length * vigour).max(0.0),
            shoots_per_spur: params
                .shoots_per_spur
                .clamp(0.0, MAX_SHOOTS_PER_SPUR as f32),
            roughness: params.roughness.max(0.0),
            sides: params.sides.max(3),
            detail: params.detail.max(1),
        }
    }

    /// Whether this plant has wood on it, as opposed to being a replant.
    pub fn is_mature(&self) -> bool {
        self.established >= 1.0
    }
}

/// Two vines share a mesh when they are close in every dimension that shows.
pub struct VineMetric;

impl Metric<VineConfig> for VineMetric {
    fn distance(&self, a: &VineConfig, b: &VineConfig) -> f32 {
        // A replant and a mature vine are different plants, not different
        // sizes — hence the step. And a replant builds no wood at all, so any
        // two of them are the same thing to build however differently they were
        // configured: collapsing them to one point is what stops a budget from
        // being spent on plants that cost no mesh. `farthest_first` reads a
        // zero distance as already covered, which is exactly right here.
        match (a.is_mature(), b.is_mature()) {
            (false, false) => return 0.0,
            (true, false) | (false, true) => return REPLANT_APART,
            (true, true) => {}
        }
        [
            a.trunk_height - b.trunk_height,
            (a.trunk_radius - b.trunk_radius) * 4.0,
            (a.trunk_wobble - b.trunk_wobble) * 4.0,
            (a.arms as f32 - b.arms as f32) * 2.0,
            a.cordon_reach - b.cordon_reach,
            (a.cordon_radius - b.cordon_radius) * 4.0,
            a.spur_spacing - b.spur_spacing,
            a.spur_length - b.spur_length,
            (a.shoots_per_spur - b.shoots_per_spur) * 0.2,
            (a.roughness - b.roughness) * 0.5,
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
pub struct VineParams {
    /// How many distinct vine meshes the scene may hold.
    ///
    /// A budget, not a count: the plants are clustered and this is how many
    /// representatives the clustering may keep. Lower it to trade variety for
    /// memory, raise it to spend memory on variety.
    pub variations: u32,
    /// Ground to head, in meters — the height of the fruiting wire. Not the
    /// same thing as [`ParcelParams::trellis_height`], which is where the tops
    /// of the posts are.
    pub trunk_height: f32,
    /// Trunk radius at the base, in meters.
    pub trunk_radius: f32,
    /// How far the trunk's axis wanders off vertical, in meters.
    pub trunk_wobble: f32,
    /// Cordons per vine: 1 for a unilateral vine, 2 for a bilateral one.
    pub arms: u32,
    /// Bare wire left between the cordon tips of neighbouring vines, in
    /// meters. Together with the parcel's vine spacing this is what sets how
    /// far a cordon reaches — see [`cordon_reach`].
    pub cordon_gap: f32,
    /// Cordon radius at the head, in meters.
    pub cordon_radius: f32,
    /// Target distance between spurs along a cordon, in meters.
    pub spur_spacing: f32,
    /// How far a spur stands off its cordon, in meters.
    pub spur_length: f32,
    /// Shoots per spur, as a fractional count: the whole part is certain and
    /// the fraction is the odds of one more. A spur is pruned to two buds, so
    /// `1.8` — two shoots four times in five, one otherwise — is what a
    /// healthy spur-pruned vine looks like.
    pub shoots_per_spur: f32,
    /// Depth of the bark ridges, as a fraction of the local radius.
    pub roughness: f32,
    /// Vertices around each tube. The silhouette — visible on every instance.
    pub sides: u32,
    /// Rings per meter along each tube. Barely visible at row distance, so
    /// this is the cheaper of the two detail knobs to turn down.
    pub detail: u32,
}

impl Default for VineParams {
    fn default() -> Self {
        Self {
            variations: 4,
            trunk_height: 0.9,
            trunk_radius: 0.035,
            trunk_wobble: 0.02,
            arms: 2,
            cordon_gap: 0.15,
            cordon_radius: 0.022,
            spur_spacing: 0.12,
            spur_length: 0.05,
            shoots_per_spur: 1.8,
            roughness: 0.14,
            sides: 8,
            detail: 20,
        }
    }
}

pub fn plugin(app: &mut App) {
    // `ParcelParams` is deliberately not initialized here — `terrain::plugin`
    // owns it, and `elements::plugin` adds terrain first.
    app.init_resource::<VineParams>().add_systems(
        PreUpdate,
        build.in_set(Grow::Vines).run_if(
            configs_changed::<VineConfig>
                // The shoots this layer spawns are authored from `ShootParams`,
                // and nothing re-authors the vine configs when those change.
                .or_else(resource_changed::<shoot::ShootParams>)
                .or_else(resource_changed::<VineParams>),
        ),
    );
}

// ─── Shape ──────────────────────────────────────────────────────────

/// How far a cordon runs from the head, in meters.
///
/// Between two neighbouring vines there is exactly one vine spacing of wire to
/// share. A bilateral vine sends an arm each way, so that run splits in two and
/// the facing tips of neighbouring vines leave `cordon_gap` between them:
///
/// ```text
///   reach + cordon_gap + reach = vine_spacing
/// ```
///
/// A unilateral vine sends one arm the whole way, stopping `cordon_gap` short
/// of its neighbour's trunk:
///
/// ```text
///   reach + cordon_gap = vine_spacing
/// ```
///
/// `vine_spacing` is a *target*, though: the parcel solver rounds to a whole
/// number of vines per panel, so the planted step is 1.236 m against the
/// default 1.2 m target. One mesh is shared across rows whose steps differ, so
/// the nominal figure is the only input available — and it errs short, which is
/// the safe direction. `cordon_gap` absorbs the difference.
fn cordon_reach(vine_spacing: f32, cordon_gap: f32, arms: u32) -> f32 {
    let run = (vine_spacing - cordon_gap).max(0.0);
    let reach = if arms >= 2 { run / 2.0 } else { run };
    reach.max(MIN_CORDON_REACH)
}

/// The graft union's swelling, positioned for a trunk of this height.
///
/// Clamped to mid-trunk so a deliberately stunted vine doesn't end up with its
/// graft scar at the head.
fn graft_bulge(config: &VineConfig) -> Bulge {
    Bulge {
        at: GRAFT_HEIGHT.min(config.trunk_height as f64 * 0.5),
        width: GRAFT_WIDTH,
        height: GRAFT_BULGE,
    }
}

/// Lateral offset of the trunk's axis at height fraction `f`.
///
/// Two low-frequency waves with seeded phases, tapered to nothing at both ends
/// by a `sin(pi f)` bell. Pinning both ends is deliberate: the base is the
/// planting position the vine was placed at, and the head has to stay
/// on the axis because the cordons attach there and planting only rotates
/// about Z.
///
/// Being a continuous function of height rather than a draw per control point
/// is what lets the graft union's extra points be inserted anywhere without
/// adding wobble frequency along with them.
fn trunk_axis(f: f64, wobble: f64, phase: [f64; 2]) -> (f64, f64) {
    let bell = (PI * f).sin();
    (
        wobble * bell * (TAU * f + phase[0]).sin(),
        wobble * bell * (TAU * 1.3 * f + phase[1]).sin(),
    )
}

/// Trunk radius at height `z`: a linear taper with the graft union on top.
fn trunk_radius_at(config: &VineConfig, z: f64) -> f64 {
    let height = config.trunk_height as f64;
    let f = ((z - TRUNK_BASE_Z) / (height - TRUNK_BASE_Z)).clamp(0.0, 1.0);
    let taper = 1.0 + (TRUNK_TIP_TAPER - 1.0) * f;
    config.trunk_radius as f64 * (taper + graft_bulge(config).value(z))
}

/// Heights to place trunk control points at: an even spread, plus the three
/// the graft union needs to resolve.
///
/// Without those three the radius is piecewise-linear across a 16 cm gap, and
/// a swelling 5 cm wide is simply stepped over — see [`Bulge::detail_positions`].
fn trunk_nodes(height: f64, graft: &Bulge) -> Vec<f64> {
    let mut nodes: Vec<f64> = (0..TRUNK_NODES)
        .map(|i| TRUNK_BASE_Z + (height - TRUNK_BASE_Z) * i as f64 / (TRUNK_NODES - 1) as f64)
        .collect();
    nodes.extend(
        graft
            .detail_positions()
            .into_iter()
            .filter(|z| *z > TRUNK_BASE_Z && *z < height),
    );
    sorted_unique(nodes)
}

/// Distances from the head at which spurs sit along a cordon.
fn spur_positions(reach: f64, spacing: f64) -> Vec<f64> {
    let spacing = spacing.max(MIN_SPUR_SPACING as f64);
    let limit = reach - SPUR_MARGIN;
    (0..)
        .map(|k| HEAD_RUN + spacing * (k as f64 + 0.5))
        .take_while(|x| *x <= limit)
        .collect()
}

/// Distances from the head at which the cordon's own control points sit: an
/// even background, plus whatever the spur knuckles need to resolve, plus the
/// tip itself.
fn cordon_nodes(reach: f64, spurs: &[f64]) -> Vec<f64> {
    let mut nodes: Vec<f64> = Vec::new();
    let mut x = HEAD_RUN + CORDON_STEP;
    while x < reach {
        nodes.push(x);
        x += CORDON_STEP;
    }
    for spur in spurs {
        nodes.extend(knuckle(*spur).detail_positions());
    }
    nodes.push(reach);
    nodes.retain(|x| *x > HEAD_RUN && *x <= reach);
    sorted_unique(nodes)
}

fn knuckle(at: f64) -> Bulge {
    Bulge {
        at,
        width: KNUCKLE_WIDTH,
        height: KNUCKLE_HEIGHT,
    }
}

/// Sorts and drops near-duplicates, so merged detail positions don't leave
/// control points sitting on top of each other.
fn sorted_unique(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(|a, b| a.partial_cmp(b).expect("shape math stays finite"));
    values.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
    values
}

/// One vine's wood, and the spurs the annual growth hangs off.
///
/// The spurs come back out because nothing else can reconstruct them: their
/// direction and length are random draws made while the wood is being shaped,
/// and a shoot has to sit on the stub that actually got built.
struct VineShape {
    strands: Vec<Strand>,
    spurs: Vec<Spur>,
}

/// The wood of one vine, in its own local frame.
///
/// The *draw order* of `rng` is part of this element's output: inserting a
/// draw in the middle re-rolls every vine downstream of it. The order is the
/// trunk's axis phases, the trunk's bark, then per arm the cordon's phase, the
/// cordon's bark, and each spur's direction, length and bark in turn. The
/// shoots draw from a stream of their own — see [`shoot_buds`].
fn vine_shape(config: &VineConfig, seed: u64) -> VineShape {
    let mut rng = Rng::new(seed);
    let mut shape = VineShape {
        strands: vec![trunk_strand(config, &mut rng)],
        spurs: Vec::new(),
    };
    for arm in 0..config.arms {
        let sign = if arm == 0 { 1.0 } else { -1.0 };
        let (strands, spurs) = cordon_shape(config, sign, &mut rng);
        shape.strands.extend(strands);
        shape.spurs.extend(spurs);
    }
    shape
}

fn trunk_strand(config: &VineConfig, rng: &mut Rng) -> Strand {
    let height = config.trunk_height as f64;
    let phase = [rng.unit() * TAU, rng.unit() * TAU];
    let wobble = config.trunk_wobble as f64;

    let nodes = trunk_nodes(height, &graft_bulge(config));
    let points = nodes
        .iter()
        .map(|z| {
            let f = (z - TRUNK_BASE_Z) / (height - TRUNK_BASE_Z);
            let (x, y) = trunk_axis(f, wobble, phase);
            Point3::new(x, y, *z)
        })
        .collect();
    let radii = nodes.iter().map(|z| trunk_radius_at(config, *z)).collect();

    Strand::new(
        points,
        radii,
        config.sides as usize,
        config.detail as f64,
        bark(config, rng),
    )
}

/// One cordon and the spurs growing off it. `sign` is which way along the row
/// it runs.
fn cordon_shape(config: &VineConfig, sign: f64, rng: &mut Rng) -> (Vec<Strand>, Vec<Spur>) {
    let head_z = config.trunk_height as f64;
    let reach = config.cordon_reach as f64;
    let phase = rng.unit() * TAU;
    let sway = config.trunk_wobble as f64 * 0.5;
    let spurs = spur_positions(reach, config.spur_spacing as f64);

    // The cordon's centerline at distance `x` out from the head, and its
    // radius there. Shared by the cordon's own control points and by the spur
    // bases, so a spur always starts exactly on the wood it grows from.
    let centerline = |x: f64| {
        let g = ((x - HEAD_RUN) / (reach - HEAD_RUN)).clamp(0.0, 1.0);
        Point3::new(
            sign * x,
            sway * (TAU * 0.8 * x / reach + phase).sin(),
            head_z - HEAD_DROP - CORDON_DROOP * g,
        )
    };
    let radius = |x: f64| {
        let g = ((x - HEAD_RUN) / (reach - HEAD_RUN)).clamp(0.0, 1.0);
        let taper = 1.0 + (CORDON_TIP_TAPER - 1.0) * g;
        let knuckles: f64 = spurs.iter().map(|s| knuckle(*s).value(x)).sum();
        config.cordon_radius as f64 * (taper + knuckles)
    };

    // Starts buried in the trunk, on its axis, and arcs over the head. The
    // first two points give the bend something to leave the trunk along
    // rather than kinking straight out of it.
    let mut points = vec![
        Point3::new(0.0, 0.0, head_z - CORDON_EMBED),
        Point3::new(0.0, 0.0, head_z - CORDON_EMBED * 0.25),
        Point3::new(sign * HEAD_RUN * 0.45, 0.0, head_z),
        centerline(HEAD_RUN),
    ];
    let mut radii = vec![radius(0.0), radius(0.0), radius(0.0), radius(HEAD_RUN)];
    for x in cordon_nodes(reach, &spurs) {
        points.push(centerline(x));
        radii.push(radius(x));
    }

    let mut strands = vec![Strand::new(
        points,
        radii,
        config.sides as usize,
        config.detail as f64,
        bark(config, rng),
    )];

    let mut stubs = Vec::new();
    if (config.spur_length as f64) >= MIN_SPUR_LENGTH {
        for (index, x) in spurs.iter().enumerate() {
            let stub = spur(config, centerline(*x), sign, index, rng);
            strands.push(spur_strand(config, &stub, rng));
            stubs.push(stub);
        }
    }
    (strands, stubs)
}

/// One spur: where a pruning stub meets its cordon, and the axis it stands on.
///
/// A shoot grows out of a bud partway along that axis, so this is the frame
/// [`shoot_buds`] needs — and the reason the stub's random draws are made here
/// rather than buried inside the geometry that consumes them.
#[derive(Clone, Copy, Debug)]
struct Spur {
    /// Where it meets the cordon, on the cordon's own centerline.
    base: Point3<f64>,
    /// Unit vector along its axis, up and out from the cordon.
    direction: Vector3<f64>,
    length: f64,
}

impl Spur {
    /// The point a fraction `f` of the way along the axis.
    fn at(&self, f: f64) -> Point3<f64> {
        self.base + self.direction * (self.length * f)
    }

    /// Which way the stub leans, seen from above — the bearing a shoot pushed
    /// from a bud on its outer side faces.
    fn azimuth(&self) -> f64 {
        self.direction.y.atan2(self.direction.x)
    }
}

/// The `index`-th spur on a cordon running in direction `sign`, angled up and
/// out from the cordon and alternating sides the way a pruned vine's spurs
/// alternate along the wire.
fn spur(
    config: &VineConfig,
    base: Point3<f64>,
    sign: f64,
    index: usize,
    rng: &mut Rng,
) -> Spur {
    let side = if index.is_multiple_of(2) { 1.0 } else { -1.0 };
    let jitter = rng.range(-0.15, 0.15);
    let length = config.spur_length as f64 * rng.range(0.8, 1.2);
    Spur {
        base,
        direction: Vector3::new(sign * 0.20, side * 0.55 + jitter, 0.80).normalize(),
        length,
    }
}

/// The short tapered stub of a spur, reaching back into the cordon so the two
/// tubes meet without a seam.
fn spur_strand(config: &VineConfig, spur: &Spur, rng: &mut Rng) -> Strand {
    let radius = config.cordon_radius as f64;
    // Four points so the fit is always cubic, and the last three so the stub
    // tapers rather than ending in a cylinder.
    let points = vec![
        spur.base - spur.direction * SPUR_EMBED,
        spur.at(0.35),
        spur.at(0.70),
        spur.at(1.0),
    ];
    let radii = vec![radius * 0.75, radius * 0.62, radius * 0.50, radius * 0.38];

    // A spur is five centimeters long — halving its ring resolution is
    // invisible at any distance a vineyard is looked at from.
    Strand::new(
        points,
        radii,
        (config.sides as usize / 2).max(4),
        config.detail as f64,
        bark(config, rng),
    )
}

fn bark(config: &VineConfig, rng: &mut Rng) -> Bark {
    Bark::new(rng, config.sides as usize, config.roughness as f64)
}

// ─── Shoots ─────────────────────────────────────────────────────────

/// How many shoots a spur pushes, given a fractional `rate` and one draw.
///
/// The whole part is certain and the fraction is the odds of one more, so a
/// rate of `1.8` gives two shoots four times in five and one otherwise. That
/// beats rounding: a vineyard of spurs all pushing exactly two shoots reads as
/// a pattern, and rounding down to one loses half the canopy.
fn shoot_count(rate: f64, draw: f64) -> usize {
    let rate = rate.clamp(0.0, MAX_SHOOTS_PER_SPUR as f64);
    let whole = rate.floor();
    (whole as usize + usize::from(draw < rate - whole)).min(MAX_SHOOTS_PER_SPUR)
}

/// Where one shoot grows from, in the plant's local frame.
///
/// A slot on the built wood rather than a finished placement: the bearing, lean
/// and vigour of the shoot that fills it are drawn per *plant*, in [`build`].
#[derive(Clone, Debug)]
struct Bud {
    /// The prim the shoot takes. Named for the *bud*, not for the shoot's rank,
    /// so dropping a spur from two shoots to one drops `_1` and leaves `_0`
    /// exactly where it was.
    name: String,
    position: Vec3,
    /// Bearing the shoot leaves on, before the per-plant wander.
    yaw: f32,
    /// How far that bearing may wander, in radians. A bud on a spur faces where
    /// its stub does; a replant's one shoot may face anywhere.
    spread: f32,
    /// Uniform scale the shoot is placed at.
    scale: f32,
}

/// Which buds this vine's shoots grow from.
///
/// Draws from a **stream of its own**, salted off the wood's with
/// [`SHOOT_STREAM`]: sharing the wood's stream would mean every nudge of
/// `shoots_per_spur` re-shaped the trunk underneath the shoots being tuned.
/// One draw per spur — how many shoots it pushed.
///
/// A `None` is a bud slot the spur left empty. The empty slots are kept in the
/// list because [`build`] draws for every slot, used or not: that is what makes
/// a vine pruned to fewer shoots keep the ones it had, rather than reshuffling
/// every shoot past the first one that went away.
fn shoot_buds(config: &VineConfig, spurs: &[Spur], seed: u64) -> Vec<Option<Bud>> {
    let mut rng = Rng::new(seed ^ SHOOT_STREAM);
    let mut buds = Vec::new();

    for (index, spur) in spurs.iter().enumerate() {
        let count = shoot_count(config.shoots_per_spur as f64, rng.unit());
        for bud in 0..MAX_SHOOTS_PER_SPUR {
            if bud >= count {
                buds.push(None);
                continue;
            }
            let at = spur.at((BUD_TIP - BUD_STEP * bud as f64).max(0.0));
            buds.push(Some(Bud {
                name: format!("Shoot_{index:02}_{bud}"),
                position: Vec3::new(at.x as f32, at.y as f32, at.z as f32),
                // Buds alternate around the stub, so two shoots off one spur
                // grow apart rather than through each other.
                yaw: (spur.azimuth() + PI * bud as f64) as f32,
                spread: BUD_SPREAD as f32,
                scale: 1.0,
            }));
        }
    }
    buds
}

// ─── Building ───────────────────────────────────────────────────────

/// One representative plant: its wood, and the buds its shoots grow from.
struct VineBuild {
    /// Trunk, cordons and spurs as a single mesh — a dozen tubes merge for
    /// free, and prim count is what the export pays for. `None` for a replant,
    /// which has no permanent wood at all.
    wood: Option<Mesh>,
    buds: Vec<Option<Bud>>,
}

/// Builds one representative: its wood, and where its shoots go.
///
/// A replant builds no mesh. It is a shoot out of bare ground, sunk
/// [`shoot::PLANT_DEPTH`] so the bend at its base stays underground — and sunk
/// *proportionally*, so a half-grown replant is buried half as deep and hides
/// exactly as much.
fn build_vine(config: &VineConfig, seed: u64) -> anyhow::Result<VineBuild> {
    if !config.is_mature() {
        let scale = config.established;
        return Ok(VineBuild {
            wood: None,
            buds: vec![Some(Bud {
                name: REPLANT_SHOOT.to_string(),
                position: Vec3::new(0.0, 0.0, -shoot::PLANT_DEPTH as f32 * scale),
                // A shoot leaves its bud sideways and a plant is only ever
                // turned to face along the row, so without a bearing of its own
                // every replant would break the surface a bend's radius
                // down-row of where it was planted.
                yaw: 0.0,
                spread: PI as f32,
                scale,
            })],
        });
    }

    let shape = vine_shape(config, seed);
    let parts = shape
        .strands
        .iter()
        .map(strand_mesh)
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(VineBuild {
        wood: Some(merge_meshes(&parts).to_mesh()),
        buds: shoot_buds(config, &shape.spurs, seed),
    })
}

/// The trunk's collision proxy: a capsule up the trunk's nominal axis, from
/// its buried base to the head. `None` for a replant, which has no wood.
///
/// Only the trunk. The cordons and spurs are what a vine's mesh mostly is, and
/// nothing walking a vineyard has any business inside the canopy — while the
/// trunk is the one part of a plant that stops a robot.
///
/// Sized at the graft union, the widest the trunk is authored anywhere. A
/// straight capsule cannot follow [`VineConfig::trunk_wobble`], so proxy and
/// wood part company by up to that much around mid-height — which is what a
/// proxy is for.
fn trunk_collider(config: &VineConfig) -> Option<impl Bundle + Copy> {
    config.is_mature().then(|| {
        capsule(
            trunk_radius_at(config, GRAFT_HEIGHT) as f32,
            TRUNK_BASE_Z as f32,
            config.trunk_height,
        )
    })
}

/// Builds one mesh per distinct vine, and hangs a shoot on every bud.
///
/// The wood is shared: each plant gets a `Wood` child referencing the mesh its
/// representative built. The shoots are not — every plant authors its own
/// [`shoot::ShootConfig`] at each bud, so two vines off one mesh still carry
/// different canopies.
///
/// Each plant's shoots draw from a stream keyed on its [`Order`], so a plant's
/// canopy depends on which planting slot it stands in and on nothing else that
/// happens elsewhere in the parcel.
pub(crate) fn build(
    mut commands: Commands,
    mut library: Library,
    scene: Res<SceneParams>,
    params: Res<VineParams>,
    shoot_params: Res<shoot::ShootParams>,
    vines: Query<(Entity, &Order, &VineConfig)>,
) -> Result<()> {
    library.clear(PART);

    let mut plants: Vec<(Order, Entity, VineConfig)> = vines
        .iter()
        .map(|(entity, order, config)| (*order, entity, *config))
        .collect();
    plants.sort_by_key(|(order, ..)| *order);

    let configs: Vec<VineConfig> = plants.iter().map(|(_, _, config)| *config).collect();
    let book = farthest_first(
        &configs,
        params.variations.max(1) as usize,
        0.0,
        &VineMetric,
    );

    // Every representative is built once. A replant registers no part, so the
    // library can have gaps in its numbering — the index is a key, not a rank.
    let mut built: Vec<(Vec<Option<Bud>>, Option<Geometry>)> = Vec::with_capacity(book.len());
    for (index, config) in book.representatives.iter().enumerate() {
        // Mixing rather than adding, so neighbouring representatives give
        // unrelated vines instead of the same vine shifted by one.
        let seed = scene.seed ^ WOOD_STREAM ^ salt(index as u64);
        let VineBuild { wood, buds } = build_vine(config, seed)?;
        let geometry = wood.map(|mesh| library.part(PART, index, mesh, surface(seed)));
        built.push((buds, geometry));
    }

    let mut shoot_order = 0u64;
    for ((order, entity, _), drew) in plants.iter().zip(&book.assignment) {
        let (buds, geometry) = &built[*drew as usize];
        let mut plant = commands.entity(*entity);
        // The layer owns everything below a plant, and a rebuild may hang a
        // different number of shoots than the last one did.
        plant.despawn_children();

        if let Some(geometry) = geometry {
            plant.with_child((Name::new(WOOD), geometry.clone()));
        }
        // From the representative, like the wood beside it: a proxy built from
        // this plant's own config would describe a trunk it did not get.
        if let Some(collider) = trunk_collider(&book.representatives[*drew as usize]) {
            plant.with_child((Name::new(COLLISION), collider));
        }

        let mut rng = Rng::new(scene.seed ^ SHOOT_STREAM ^ salt(order.0));
        for bud in buds {
            // Five draws per bud slot, filled or not: the wander off the bud's
            // bearing, the lean about each of the shoot's own horizontal axes,
            // then its vigour and its node spacing. Drawing only for the buds
            // that pushed would make the stream's length depend on the pruning,
            // so a lighter prune would reshuffle every shoot after the first gap
            // instead of just leaving one out.
            let turn = rng.range(-1.0, 1.0) as f32;
            let tilt = Vec2::new(
                rng.range(-SHOOT_TILT, SHOOT_TILT) as f32,
                rng.range(-SHOOT_TILT, SHOOT_TILT) as f32,
            );
            let vigour = rng.range(1.0 - SHOOT_VIGOUR, 1.0 + SHOOT_VIGOUR) as f32;
            let spacing = rng.range(1.0 - SHOOT_SPACING, 1.0 + SHOOT_SPACING) as f32;

            let Some(bud) = bud else { continue };
            shoot_order += 1;
            plant.with_child((
                Name::new(bud.name.clone()),
                placed(bud.position, bud.yaw + turn * bud.spread, tilt, bud.scale),
                Visibility::default(),
                shoot::ShootConfig::new(&shoot_params, vigour, spacing),
                Order(shoot_order),
            ));
        }
    }
    Ok(())
}

/// Wood, shaded off this representative's own seed so that two vines standing
/// side by side are not the same brown.
fn surface(seed: u64) -> Surface {
    material::WOOD.surface(color::shade(
        color::srgb(color::WOOD),
        &mut Rng::new(seed ^ color::COLOR_STREAM),
    ))
}

// ─── UI ─────────────────────────────────────────────────────────────

pub fn ui() -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            label_small("Trunk height"),
            (
                @FeathersSlider { @min: 0.3, @max: 1.6, @value: 0.9 }
                SliderStep(0.05)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.trunk_height = change.value;
                })
            ),
            label_small("Trunk radius"),
            (
                @FeathersSlider { @min: 0.01, @max: 0.08, @value: 0.035 }
                SliderStep(0.005)
                SliderPrecision(3)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.trunk_radius = change.value;
                })
            ),
            label_small("Trunk wobble"),
            (
                @FeathersSlider { @min: 0.0, @max: 0.08, @value: 0.02 }
                SliderStep(0.005)
                SliderPrecision(3)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.trunk_wobble = change.value;
                })
            ),
            label_small("Cordons per vine"),
            (
                @FeathersSlider { @min: 1.0, @max: 2.0, @value: 2.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.arms = change.value.round().clamp(1.0, 2.0) as u32;
                })
            ),
            label_small("Cordon gap"),
            (
                @FeathersSlider { @min: 0.0, @max: 0.6, @value: 0.15 }
                SliderStep(0.05)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.cordon_gap = change.value;
                })
            ),
            label_small("Cordon radius"),
            (
                @FeathersSlider { @min: 0.008, @max: 0.05, @value: 0.022 }
                SliderStep(0.002)
                SliderPrecision(3)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.cordon_radius = change.value;
                })
            ),
            label_small("Spur spacing"),
            (
                @FeathersSlider { @min: 0.05, @max: 0.4, @value: 0.12 }
                SliderStep(0.01)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.spur_spacing = change.value;
                })
            ),
            label_small("Spur length"),
            (
                @FeathersSlider { @min: 0.0, @max: 0.15, @value: 0.05 }
                SliderStep(0.01)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.spur_length = change.value;
                })
            ),
            label_small("Shoots per spur"),
            (
                @FeathersSlider { @min: 0.0, @max: 3.0, @value: 1.8 }
                SliderStep(0.1)
                SliderPrecision(1)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.shoots_per_spur = change.value;
                })
            ),
            label_small("Bark roughness"),
            (
                @FeathersSlider { @min: 0.0, @max: 0.4, @value: 0.14 }
                SliderStep(0.01)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.roughness = change.value;
                })
            ),
            label_small("Vine sides"),
            (
                @FeathersSlider { @min: 3.0, @max: 16.0, @value: 8.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.sides = change.value.round().max(3.0) as u32;
                })
            ),
            label_small("Vine detail"),
            (
                @FeathersSlider { @min: 4.0, @max: 60.0, @value: 20.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.detail = change.value.round().max(4.0) as u32;
                })
            ),
            label_small("Vine variations"),
            (
                @FeathersSlider { @min: 1.0, @max: 8.0, @value: 4.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
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
    use crate::scene::{Collider, Prototypes, UsdReference};

    fn params() -> VineParams {
        VineParams::default()
    }

    /// The default mature vine, at the default parcel's cordon reach.
    fn config() -> VineConfig {
        VineConfig::new(&params(), &ParcelParams::default(), 1.0, 1.0)
    }

    /// The same, with `edit` applied to the params first.
    fn config_with(edit: impl FnOnce(&mut VineParams)) -> VineConfig {
        let mut params = params();
        edit(&mut params);
        VineConfig::new(&params, &ParcelParams::default(), 1.0, 1.0)
    }

    /// A vine's merged wood, as a mesh the geometry helpers can read.
    fn wood(config: &VineConfig, seed: u64) -> MeshData {
        let parts: Vec<_> = vine_shape(config, seed)
            .strands
            .iter()
            .map(|s| strand_mesh(s).expect("every strand skins"))
            .collect();
        merge_meshes(&parts)
    }

    /// The buds a vine of this config offers, empty slots dropped.
    fn buds(config: &VineConfig) -> Vec<Bud> {
        let shape = vine_shape(config, 4);
        shoot_buds(config, &shape.spurs, 4)
            .into_iter()
            .flatten()
            .collect()
    }

    #[test]
    fn cordon_reach_halves_for_a_bilateral_vine() {
        // 1.2 m of wire shared, 0.15 m left bare between facing tips.
        assert!((cordon_reach(1.2, 0.15, 2) - 0.525).abs() < 1e-6);
    }

    #[test]
    fn a_unilateral_vine_reaches_a_full_vine_spacing_less_the_gap() {
        assert!((cordon_reach(1.2, 0.15, 1) - 1.05).abs() < 1e-6);
    }

    /// A zero-length cordon would be a zero-length rail, and curvo's
    /// interpolation divides by exactly that.
    #[test]
    fn cordon_reach_never_collapses_to_zero() {
        assert_eq!(cordon_reach(1.0, 5.0, 2), MIN_CORDON_REACH);
        assert_eq!(cordon_reach(0.0, 0.0, 1), MIN_CORDON_REACH);
    }

    /// Tested on the geometry rather than the strand count: what matters is
    /// that a bilateral vine actually has wood on both sides of its trunk.
    #[test]
    fn a_bilateral_vine_grows_an_arm_in_each_direction() {
        let (x0, x1) = bounds(&wood(&config(), 1), 0);
        assert!(x1 > 0.4 && x0 < -0.4, "arms both ways, got {x0}..{x1}");

        let unilateral = config_with(|p| p.arms = 1);
        let (x0, x1) = bounds(&wood(&unilateral, 1), 0);
        assert!(x1 > 0.9, "one long arm out to the neighbour, got {x1}");
        assert!(
            x0 > -0.1,
            "and nothing much the other way past the trunk, got {x0}"
        );
    }

    #[test]
    fn the_trunk_starts_below_the_ground() {
        let (z0, _) = bounds(&wood(&config(), 1), 2);
        assert!(
            z0 <= TRUNK_BASE_Z as f32 + 0.01,
            "buried, so a slope leaves no gap, got {z0}"
        );
    }

    #[test]
    fn the_graft_union_is_thicker_than_the_trunk_just_above_it() {
        let c = config();
        assert!(trunk_radius_at(&c, GRAFT_HEIGHT) > trunk_radius_at(&c, 0.30) * 1.4);
        // And it is a local swelling, not just the base taper showing through.
        assert!(trunk_radius_at(&c, GRAFT_HEIGHT) > trunk_radius_at(&c, 0.05));
    }

    #[test]
    fn spurs_are_spaced_along_the_cordon() {
        let reach = cordon_reach(1.2, 0.15, 2) as f64;
        let spurs = spur_positions(reach, 0.12);
        assert_eq!(spurs.len(), 3, "got {spurs:?}");
        assert!(spurs.iter().all(|x| *x > HEAD_RUN && *x < reach));
        for pair in spurs.windows(2) {
            assert!((pair[1] - pair[0] - 0.12).abs() < 1e-9);
        }
    }

    #[test]
    fn a_cordon_too_short_for_a_spur_gets_none() {
        assert!(spur_positions(MIN_CORDON_REACH as f64, 0.12).is_empty());
    }

    /// The trunk's axis has to return to center at the head, because the
    /// cordons attach there and planting only ever rotates about Z.
    #[test]
    fn the_trunk_axis_is_pinned_at_both_ends() {
        for phase in [[0.0, 0.0], [1.0, 2.0], [3.0, 4.5]] {
            for f in [0.0, 1.0] {
                let (x, y) = trunk_axis(f, 0.05, phase);
                assert!(x.abs() < 1e-12 && y.abs() < 1e-12);
            }
        }
        // And it actually wanders in between, or the wobble does nothing.
        let (x, y) = trunk_axis(0.5, 0.05, [1.0, 2.0]);
        assert!(x.hypot(y) > 1e-3);
    }

    #[test]
    fn a_vine_stays_within_a_plausible_bounding_box() {
        let c = config();
        let mesh = wood(&c, 1);
        let slack = c.spur_length + c.trunk_radius + 0.05;

        let (x0, x1) = bounds(&mesh, 0);
        assert!(
            x0 > -(c.cordon_reach + slack) && x1 < c.cordon_reach + slack,
            "{x0}..{x1}"
        );
        let (y0, y1) = bounds(&mesh, 1);
        assert!(y0 > -0.2 && y1 < 0.2, "narrow across the row, got {y0}..{y1}");
        let (z0, z1) = bounds(&mesh, 2);
        assert!(z0 > -0.1 && z1 < c.trunk_height + slack, "{z0}..{z1}");
    }

    #[test]
    fn a_vines_wood_is_reproducible() {
        let build = || wood(&config(), 9).points;
        assert_eq!(build(), build());
    }

    #[test]
    fn differently_seeded_vines_differ_from_one_another() {
        assert_ne!(wood(&config(), 1).points, wood(&config(), 2).points);
    }

    /// Python can set either end of every knob, including the ones the
    /// viewer's sliders can't reach. Nothing may come back degenerate.
    #[test]
    fn a_vine_asked_for_at_the_stops_still_builds() {
        for edit in [
            |p: &mut VineParams| {
                *p = VineParams {
                    variations: 0,
                    trunk_height: 0.0,
                    trunk_radius: 0.0,
                    trunk_wobble: 0.0,
                    arms: 0,
                    cordon_gap: 99.0,
                    cordon_radius: 0.0,
                    spur_spacing: 0.0,
                    spur_length: 0.0,
                    shoots_per_spur: -1.0,
                    roughness: 0.0,
                    sides: 0,
                    detail: 0,
                }
            },
            |p: &mut VineParams| {
                p.trunk_height = 12.0;
                p.shoots_per_spur = 9.0;
                p.sides = 64;
                p.detail = 200;
            },
        ] {
            let config = config_with(edit);
            let built = build_vine(&config, 3).expect("builds at the stops");
            let mesh = built.wood.expect("a mature vine has wood");
            let points = mesh
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

    // ─── Shoots ─────────────────────────────────────────────────────

    /// The whole point of a fractional rate: `1.8` is not "two", it is "two
    /// most of the time". A vineyard of spurs all pushing exactly two shoots
    /// reads as a pattern from fifty meters away.
    #[test]
    fn shoot_count_takes_the_fraction_as_odds() {
        assert_eq!(shoot_count(1.8, 0.0), 2);
        assert_eq!(shoot_count(1.8, 0.79), 2);
        assert_eq!(shoot_count(1.8, 0.81), 1);
        assert_eq!(shoot_count(1.8, 0.999), 1);

        // Whole rates are certain, either way.
        assert_eq!(shoot_count(2.0, 0.999), 2);
        assert_eq!(shoot_count(1.0, 0.0), 1);
        assert_eq!(shoot_count(0.0, 0.0), 0);

        // And a rate past what a spur can carry clamps rather than overflows.
        assert_eq!(shoot_count(9.0, 0.5), MAX_SHOOTS_PER_SPUR);
        assert_eq!(shoot_count(-1.0, 0.5), 0);
    }

    /// A shoot grows from a bud, and a bud is on a spur. If a bud drifted off
    /// its stub, the shoot would hang in mid-air beside the wood.
    #[test]
    fn buds_sit_on_the_spurs_they_belong_to() {
        let config = config();
        let shape = vine_shape(&config, 4);
        let placed = buds(&config);
        assert!(!placed.is_empty(), "the default vine grows some");

        for bud in &placed {
            let index: usize = bud.name["Shoot_".len().."Shoot_00".len()]
                .parse()
                .expect("the name carries its spur's index");
            let spur = shape.spurs[index];
            let offset = Vector3::new(
                bud.position.x as f64 - spur.base.x,
                bud.position.y as f64 - spur.base.y,
                bud.position.z as f64 - spur.base.z,
            );

            let along = offset.dot(&spur.direction);
            assert!(
                (0.0..=spur.length).contains(&along),
                "{} sits between the cordon and the spur's tip, got {along}",
                bud.name
            );
            assert!(
                offset.cross(&spur.direction).norm() < 1e-6,
                "{} sits on the spur's axis, not beside it",
                bud.name
            );
        }
    }

    /// A spur that drops from two shoots to one keeps the one it had, rather
    /// than every shoot in the vine shuffling because a slot went away.
    #[test]
    fn a_bud_keeps_its_shoot_when_the_rate_drops() {
        let lush = buds(&config_with(|p| p.shoots_per_spur = 2.0));
        let sparse = buds(&config_with(|p| p.shoots_per_spur = 1.0));

        assert!(sparse.len() < lush.len(), "fewer shoots overall");
        for bud in &sparse {
            assert!(
                bud.name.ends_with("_0"),
                "only the top bud pushes, got {}",
                bud.name
            );
            let same = lush
                .iter()
                .find(|other| other.name == bud.name)
                .expect("the bud is still there");
            assert_eq!(same.position, bud.position, "and it has not moved");
        }
    }

    #[test]
    fn a_vine_pruned_to_no_shoots_grows_none() {
        assert!(buds(&config_with(|p| p.shoots_per_spur = 0.0)).is_empty());
    }

    /// Shoots draw from their own stream, so dialling `shoots_per_spur` in the
    /// viewer must not re-shape the trunk under them.
    #[test]
    fn changing_the_shoot_rate_leaves_the_wood_alone() {
        let at = |rate| wood(&config_with(|p| p.shoots_per_spur = rate), 4).points;
        assert_eq!(at(1.8), at(0.0), "same wood, however many shoots");
    }

    // ─── Replants ───────────────────────────────────────────────────

    /// A replant is a shoot in the ground and nothing else, and the burial is
    /// the whole of the shaping: a shoot leaves its bud sideways, so a replant
    /// planted at the surface would come out of the soil at a right angle
    /// before turning up.
    ///
    /// The depth scales with the plant, so a half-grown replant is buried half
    /// as deep — a fixed depth would swallow a small one whole.
    #[test]
    fn a_replant_is_one_buried_shoot_and_no_wood() {
        for established in [0.55f32, 0.8] {
            let config = VineConfig {
                established,
                ..config()
            };
            let built = build_vine(&config, 5).expect("a replant builds");

            assert!(built.wood.is_none(), "no permanent wood on a replant");
            let [Some(bud)] = &built.buds[..] else {
                panic!("a replant is exactly one shoot, got {:?}", built.buds);
            };
            assert_eq!(bud.name, REPLANT_SHOOT);
            assert_eq!(bud.scale, established);
            assert!(
                (bud.position.z + shoot::PLANT_DEPTH as f32 * established).abs() < 1e-6,
                "buried in proportion, got {}",
                bud.position.z
            );
            // The shoot's own frame is scaled with it, so what stays above
            // ground is the straight rise either way.
            let bend = shoot::PLANT_DEPTH as f32 * established;
            assert!(bend > 0.0 && bud.position.z < 0.0, "and it is underground");
        }
    }

    /// A replant may face any way; a bud on a spur may not stray far off the
    /// stub it grew from.
    #[test]
    fn a_replants_shoot_may_face_anywhere() {
        let replant = build_vine(&VineConfig { established: 0.7, ..config() }, 5).unwrap();
        assert_eq!(replant.buds[0].as_ref().unwrap().spread, PI as f32);
        assert!(buds(&config()).iter().all(|b| b.spread == BUD_SPREAD as f32));
    }

    /// The trunk's proxy has to cover the trunk it stands in for: short of the
    /// head and a robot walks through the top of a vine, above the base and the
    /// collider floats. It also has to be wide enough for the whole trunk, the
    /// graft union included.
    #[test]
    fn a_trunk_collider_covers_the_trunk_and_a_replant_has_none() {
        let mut world = World::new();
        for config in [
            config(),
            config_with(|p| p.trunk_height = 0.4),
            config_with(|p| p.trunk_radius = 0.1),
            config_with(|p| p.trunk_wobble = 0.0),
        ] {
            let proxy = trunk_collider(&config).expect("a mature vine has a trunk");
            let at = world.spawn(proxy).id();
            let at = world.entity(at);
            let shape = at.get::<Collider>().unwrap().0;
            let z = at.get::<Transform>().unwrap().translation.z;
            let reach = shape.height / 2.0 + shape.radius;

            assert!(
                (z - reach - TRUNK_BASE_Z as f32).abs() < 1e-6,
                "{config:?} floats above its buried base"
            );
            assert!(
                (z + reach - config.trunk_height).abs() < 1e-6,
                "{config:?} stops short of its head"
            );
            for z in [TRUNK_BASE_Z, 0.0, GRAFT_HEIGHT, config.trunk_height as f64] {
                assert!(
                    shape.radius >= trunk_radius_at(&config, z) as f32,
                    "{config:?} is thinner than its own trunk at {z} m"
                );
            }
        }

        // A replant is one green shoot out of bare ground — no wood, so
        // nothing standing there for a robot to bump into.
        let replant = VineConfig {
            established: 0.6,
            ..config()
        };
        assert!(trunk_collider(&replant).is_none());
    }

    // ─── The layer ──────────────────────────────────────────────────

    /// Every planted vine comes out carrying wood from the library, and every
    /// wood prim names a part that is actually in it.
    #[test]
    fn every_mature_vine_draws_its_wood_from_the_library() {
        let mut app = testing::grown(VineyardParams::default());

        let vines = organs::<VineConfig>(app.world_mut());
        assert!(vines.len() > 20, "the fixture planted vines");
        assert!(
            vines.iter().any(|v| !v.config.is_mature()),
            "and some of them are replants"
        );

        for vine in &vines {
            let entity = testing::prim(
                app.world_mut(),
                &vine.path.split('/').collect::<Vec<_>>(),
            )
            .expect("the plant is on the scene graph");
            let children = named_children(app.world_mut(), entity);
            for prim in [WOOD, COLLISION] {
                assert_eq!(
                    children.iter().any(|(name, _)| name == prim),
                    vine.config.is_mature(),
                    "{}: {prim} iff mature, got {children:?}",
                    vine.path
                );
            }
            assert!(
                children.iter().any(|(name, _)| !is_fixture(name)),
                "{}: and shoots on it",
                vine.path
            );
        }

        let mut query = app.world_mut().query::<(&Name, &UsdReference)>();
        let drawn: Vec<String> = query
            .iter(app.world())
            .filter(|(name, _)| name.as_str() == WOOD)
            .map(|(_, reference)| reference.0.clone())
            .collect();
        let library = app.world().resource::<Prototypes>();
        for name in &drawn {
            assert!(
                library.get(name).is_some(),
                "{name} names a part that is not in the library"
            );
        }
    }

    /// The budget is a cap, and the metric is what spends it: given vines that
    /// really differ, the distinct shapes have to come out on separate meshes.
    #[test]
    fn the_budget_caps_the_meshes_and_the_metric_spends_it() {
        let mature = config();
        let stunted = config_with(|p| p.trunk_height = 0.35);
        let replant = VineConfig {
            established: 0.6,
            ..mature
        };

        // One replant among forty mature vines, and it still earns a mesh —
        // k-center minimizes the worst distance, not the average, so a rare
        // shape cannot be averaged away.
        let mut population = vec![mature; 40];
        population.push(replant);
        population.extend([stunted; 4]);

        let book = farthest_first(&population, 3, 0.0, &VineMetric);
        assert_eq!(book.len(), 3);
        assert_eq!(book.assignment[0], book.assignment[39], "the mature ones share");
        assert_ne!(book.assignment[0], book.assignment[40], "the replant does not");
        assert_ne!(book.assignment[0], book.assignment[41], "nor the stunted ones");

        // And a budget of one is honoured, however varied the population.
        assert_eq!(farthest_first(&population, 1, 0.0, &VineMetric).len(), 1);
    }

    /// The wood is shared and the canopy is not: two vines off one mesh still
    /// have to carry different shoots, or the repeat is obvious down a row.
    #[test]
    fn vines_sharing_a_mesh_still_carry_their_own_shoots() {
        let mut app = testing::grown(VineyardParams::default());

        // Two plants that drew the same wood, found by what they reference —
        // which mesh a plant drew is the codebook's business, not a test's.
        let mut by_part: std::collections::BTreeMap<String, Vec<String>> = default();
        for vine in organs::<VineConfig>(app.world_mut()) {
            if let Some(part) = wood_of(&mut app, &vine.path) {
                by_part.entry(part).or_default().push(vine.path);
            }
        }
        let shared = by_part
            .values()
            .find(|paths| paths.len() > 1)
            .expect("some mesh is shared by more than one plant");

        let (a, b) = (
            shoots_of(&mut app, &shared[0]),
            shoots_of(&mut app, &shared[shared.len() - 1]),
        );
        assert!(!a.is_empty(), "the plants carry shoots");
        assert_eq!(a.len(), b.len(), "the same buds, from the same wood");
        assert_ne!(a, b, "but placed differently on them");
        for (a, b) in a.iter().zip(&b) {
            assert!(
                (a.translation - b.translation).length() < 1e-6,
                "a bud is where the wood put it, on both plants"
            );
        }
    }

    /// A rebuild that spends less of the budget must not leave the meshes it
    /// no longer uses in the library, exported and referenced by nothing.
    ///
    /// The counts are bounds rather than equalities: a replant builds no wood,
    /// so how much of a budget reaches the library depends on how many of the
    /// representatives came out mature.
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
                vine: VineParams {
                    variations: 6,
                    ..params()
                },
                ..default()
            });
        let before = meshes(&app);
        assert!(
            (2..=6).contains(&before),
            "the budget bought several meshes and no more than it allows, got {before}"
        );

        app.world_mut().resource_mut::<VineParams>().variations = 1;
        app.update();
        assert!(
            meshes(&app) < before,
            "the ones it stopped using are gone, still {before}"
        );
    }

    /// Two meshes the same brown would make a shared wood read as one plant
    /// stamped out, which is the whole thing the budget is spent avoiding.
    #[test]
    fn every_vine_mesh_gets_its_own_shade() {
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
                assert_ne!(a, b, "two vine meshes came out the same shade");
            }
        }
    }

    /// The part a plant's `Wood` prim references, or `None` for a replant.
    fn wood_of(app: &mut App, path: &str) -> Option<String> {
        let entity = testing::prim(app.world_mut(), &path.split('/').collect::<Vec<_>>())?;
        let wood = named_children(app.world_mut(), entity)
            .into_iter()
            .find(|(name, _)| name == WOOD)?
            .1;
        Some(app.world().entity(wood).get::<UsdReference>()?.0.clone())
    }

    /// Whether a prim below a plant is part of the plant itself rather than a
    /// shoot hung on it — everything a plant has that is not annual growth.
    fn is_fixture(name: &str) -> bool {
        name == WOOD || name == COLLISION
    }

    /// Where a plant's shoots ended up, in the order they were hung.
    fn shoots_of(app: &mut App, path: &str) -> Vec<Transform> {
        let entity =
            testing::prim(app.world_mut(), &path.split('/').collect::<Vec<_>>()).unwrap();
        named_children(app.world_mut(), entity)
            .into_iter()
            .filter(|(name, _)| !is_fixture(name))
            .map(|(_, child)| *app.world().entity(child).get::<Transform>().unwrap())
            .collect()
    }
}
