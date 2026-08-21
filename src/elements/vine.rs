//! Vine element — the permanent woody framework of a grapevine.
//!
//! A vine is a **trunk** rising from the ground to the **head**, where it
//! turns into one or two **cordons** running along the fruiting wire. Each
//! cordon carries **spurs**: the short pruning stubs whose knuckles are the
//! strongest visual signature of a pruned vine. A **graft union** swells the
//! trunk about 15 cm up, as it does on essentially every commercial vine.
//!
//! Only the permanent wood is *shaped* here. The annual growth that hangs off
//! the spurs belongs to its own elements, nested into this one's prototypes
//! rather than placed alongside them — [`shoot`](super::shoot) is the first of
//! them, and this module decides where on its spurs they sit. Leaves and fruit
//! will hang off the shoots the same way.
//!
//! # Structure
//!
//! Every part is a [`Strand`]: a polyline of control points with a radius at
//! each, skinned into a closed tube. So this module places control points and
//! nothing else — [`strand`](super::strand) owns everything about turning them
//! into triangles. Trunk, cordon and spur differ only in where their points go.
//!
//! Cordons and spurs *start inside* the part they grow from — a cordon eight
//! centimeters below the head, on the trunk's axis. Interpenetrating tubes
//! need no CSG and leave no coincident surfaces to z-fight, and the resulting
//! lumpy junction is what a real cordon-trained head looks like anyway.
//!
//! # Local frame
//!
//! Prototypes are authored with the **origin at the trunk base, on the
//! ground**: +Z up, **+X along the row** so cordons run along ±X, +Y across
//! it. The placer therefore only ever needs a yaw about Z, and never has to
//! know which way a strand was authored.
//!
//! # One subtree
//!
//! The prototype library under [`PROTOTYPE`]. Each `Var_<i>` is an `Xform`
//! over a single `Wood` mesh — the trunk, cordons and spurs merged — plus one
//! prim per shoot, each referencing a [`shoot`](super::shoot) prototype. This
//! element authors shapes and nothing else — where vines *stand* is
//! [`planting`](super::util::planting)'s business, and it finds these
//! prototypes by path alone. That split is what lets a vine be planted as an
//! addressable prim, as a `PointInstancer` instance, or nested inside another
//! prototype, without this module knowing which.

use std::f64::consts::{PI, TAU};

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use nalgebra::{Point3, Vector3};
use usd_bevy::authoring::{define_prim, remove_prim};
use usd_bevy::live::LiveStage;

use super::shoot;
use super::util::parcel::ParcelParams;
use super::util::place::{self, Placement};
use super::util::strand::{Bark, Bulge, Strand, strand_mesh};
use super::util::usd::{author_mesh, merge_meshes};
use super::{Grow, Rng};

/// The prototype library this element owns: one `Var_<i>` mesh per variation.
pub const PROTOTYPE: &str = "/Vineyard/parts/Vine";

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
/// vine, and it is what fixes how many draws a spur takes — see
/// [`shoot_placements`].
const MAX_SHOOTS_PER_SPUR: usize = 3;

/// Where the buds sit along a spur, as fractions of its length. They fill from
/// the tip down, so a spur that pushes one shoot pushes it from the top bud —
/// which is the one that usually wins.
const BUD_TIP: f64 = 0.95;
const BUD_STEP: f64 = 0.35;

/// How far a shoot's bearing wanders off the bud it grew from, in radians.
const BUD_SPREAD: f64 = 0.4;

/// How far a shoot leans off the upright it was authored as, in radians —
/// drawn per shoot about both of its own horizontal axes.
///
/// This is the cheapest variety in the scene: a dozen shoots on a vine come
/// from a handful of prototypes, and without a per-instance lean the repeat is
/// obvious at a glance.
const SHOOT_TILT: f64 = 0.14;

/// How much a shoot's length varies about [`ShootParams::length`], as a
/// fraction. Applied as a uniform scale, so a longer shoot is also slightly
/// thicker — which is what [`Placement::scale`] already means.
///
/// [`ShootParams::length`]: super::shoot::ShootParams::length
const SHOOT_LENGTH_JITTER: f64 = 0.15;

/// Salt that splits the shoot placements off the wood's random stream. An
/// arbitrary odd constant; only its fixedness matters.
const SHOOT_STREAM: u64 = 0xA076_1D64_78BD_642F;

/// Shortest cordon we will build. Load-bearing rather than cosmetic: a
/// `cordon_gap` wider than the vine spacing would otherwise ask for a
/// zero-length rail, whose total chord length is zero and whose interpolation
/// is entirely NaN.
const MIN_CORDON_REACH: f32 = 0.05;

// ─── Params ─────────────────────────────────────────────────────────

#[derive(Resource, Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct VineParams {
    /// How many differently-seeded vines to author as prototypes.
    pub variations: u32,
    /// Drives both the prototype shapes and which vines are missing or young.
    pub seed: u64,
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
            seed: 0,
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
    // owns it, and `elements::plugin` adds terrain first. If that order ever
    // changes, this element's author systems are what will notice.
    app.init_resource::<VineParams>().add_systems(
        PreUpdate,
        author_prototypes
            .in_set(Grow::Prototypes)
            // References the shoot prototypes and counts them off the stage,
            // so they have to be there first. `Grow` chains the *sets*; the
            // systems inside one are unordered until something says otherwise.
            .after(shoot::author_prototypes)
            .run_if(
                resource_changed::<VineParams>
                    .or_else(resource_changed::<ParcelParams>)
                    .or_else(resource_changed::<shoot::ShootParams>),
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
/// default 1.2 m target. One prototype is shared across rows whose steps
/// differ, so the nominal figure is the only input available — and it errs
/// short, which is the safe direction. `cordon_gap` absorbs the difference.
fn cordon_reach(vine_spacing: f32, cordon_gap: f32, arms: u32) -> f32 {
    let run = (vine_spacing - cordon_gap).max(0.0);
    let reach = if arms >= 2 { run / 2.0 } else { run };
    reach.max(MIN_CORDON_REACH)
}

/// The graft union's swelling, positioned for a trunk of this height.
///
/// Clamped to mid-trunk so a deliberately stunted vine doesn't end up with its
/// graft scar at the head.
fn graft_bulge(params: &VineParams) -> Bulge {
    Bulge {
        at: GRAFT_HEIGHT.min(params.trunk_height as f64 * 0.5),
        width: GRAFT_WIDTH,
        height: GRAFT_BULGE,
    }
}

/// Lateral offset of the trunk's axis at height fraction `f`.
///
/// Two low-frequency waves with seeded phases, tapered to nothing at both ends
/// by a `sin(pi f)` bell. Pinning both ends is deliberate: the base is the
/// planting position the vine was placed at, and the head has to stay
/// on the axis because the cordons attach there and the placer only rotates
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
fn trunk_radius_at(params: &VineParams, z: f64) -> f64 {
    let height = (params.trunk_height as f64).max(0.1);
    let f = ((z - TRUNK_BASE_Z) / (height - TRUNK_BASE_Z)).clamp(0.0, 1.0);
    let taper = 1.0 + (TRUNK_TIP_TAPER - 1.0) * f;
    params.trunk_radius as f64 * (taper + graft_bulge(params).value(z))
}

/// Heights to place trunk control points at: an even spread, plus the three
/// the graft union needs to resolve.
///
/// Without those three the radius is piecewise-linear across a 16 cm gap, and
/// a swelling 5 cm wide is simply stepped over — see [`Bulge::detail_positions`].
fn trunk_nodes(height: f64, graft: &Bulge) -> Vec<f64> {
    let mut nodes: Vec<f64> = (0..TRUNK_NODES)
        .map(|i| {
            TRUNK_BASE_Z
                + (height - TRUNK_BASE_Z) * i as f64 / (TRUNK_NODES - 1) as f64
        })
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
    let spacing = spacing.max(0.01);
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

/// The wood of one vine, in the prototype's local frame.
///
/// The *draw order* of `rng` is part of this element's output: inserting a
/// draw in the middle re-rolls every vine downstream of it. The order is the
/// trunk's axis phases, the trunk's bark, then per arm the cordon's phase, the
/// cordon's bark, and each spur's direction, length and bark in turn. The
/// shoots draw from a stream of their own — see [`shoot_placements`].
fn vine_shape(params: &VineParams, reach: f64, seed: u64) -> VineShape {
    let mut rng = Rng::new(seed);
    let mut shape = VineShape {
        strands: vec![trunk_strand(params, &mut rng)],
        spurs: Vec::new(),
    };
    for arm in 0..params.arms.clamp(1, 2) {
        let sign = if arm == 0 { 1.0 } else { -1.0 };
        let (strands, spurs) = cordon_shape(params, reach, sign, &mut rng);
        shape.strands.extend(strands);
        shape.spurs.extend(spurs);
    }
    shape
}

fn trunk_strand(params: &VineParams, rng: &mut Rng) -> Strand {
    let height = (params.trunk_height as f64).max(0.1);
    let phase = [rng.unit() * TAU, rng.unit() * TAU];
    let wobble = params.trunk_wobble as f64;

    let nodes = trunk_nodes(height, &graft_bulge(params));
    let points = nodes
        .iter()
        .map(|z| {
            let f = (z - TRUNK_BASE_Z) / (height - TRUNK_BASE_Z);
            let (x, y) = trunk_axis(f, wobble, phase);
            Point3::new(x, y, *z)
        })
        .collect();
    let radii = nodes.iter().map(|z| trunk_radius_at(params, *z)).collect();

    Strand::new(points, radii, sides(params), params.detail as f64, bark(params, rng))
}

/// One cordon and the spurs growing off it. `sign` is which way along the row
/// it runs.
fn cordon_shape(
    params: &VineParams,
    reach: f64,
    sign: f64,
    rng: &mut Rng,
) -> (Vec<Strand>, Vec<Spur>) {
    let head_z = (params.trunk_height as f64).max(0.1);
    let phase = rng.unit() * TAU;
    let sway = params.trunk_wobble as f64 * 0.5;
    let spurs = spur_positions(reach, params.spur_spacing as f64);

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
        params.cordon_radius as f64 * (taper + knuckles)
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
        sides(params),
        params.detail as f64,
        bark(params, rng),
    )];

    let mut stubs = Vec::new();
    if (params.spur_length as f64) >= MIN_SPUR_LENGTH {
        for (index, x) in spurs.iter().enumerate() {
            let stub = spur(params, centerline(*x), sign, index, rng);
            strands.push(spur_strand(params, &stub, rng));
            stubs.push(stub);
        }
    }
    (strands, stubs)
}

/// One spur: where a pruning stub meets its cordon, and the axis it stands on.
///
/// A shoot grows out of a bud partway along that axis, so this is the frame
/// [`shoot_placements`] needs — and the reason the stub's random draws are
/// made here rather than buried inside the geometry that consumes them.
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
    params: &VineParams,
    base: Point3<f64>,
    sign: f64,
    index: usize,
    rng: &mut Rng,
) -> Spur {
    let side = if index.is_multiple_of(2) { 1.0 } else { -1.0 };
    let jitter = rng.range(-0.15, 0.15);
    let length = params.spur_length as f64 * rng.range(0.8, 1.2);
    Spur {
        base,
        direction: Vector3::new(sign * 0.20, side * 0.55 + jitter, 0.80).normalize(),
        length,
    }
}

/// The short tapered stub of a spur, reaching back into the cordon so the two
/// tubes meet without a seam.
fn spur_strand(params: &VineParams, spur: &Spur, rng: &mut Rng) -> Strand {
    let radius = params.cordon_radius as f64;
    // Four points so the fit is always cubic, and the last three so the stub
    // tapers rather than ending in a cylinder.
    let points = vec![
        spur.base - spur.direction * SPUR_EMBED,
        spur.at(0.35),
        spur.at(0.70),
        spur.at(1.0),
    ];
    let radii = vec![
        radius * 0.75,
        radius * 0.62,
        radius * 0.50,
        radius * 0.38,
    ];

    // A spur is five centimeters long — halving its ring resolution is
    // invisible at any distance a vineyard is looked at from.
    Strand::new(
        points,
        radii,
        (sides(params) / 2).max(4),
        params.detail as f64,
        bark(params, rng),
    )
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

/// Where this vine's shoots sit, in the prototype's local frame.
///
/// Each placement names a [`shoot`] prototype variation and the transform that
/// puts it on a bud: the shoot is authored leaving its bud along +X and
/// turning up, so a bearing is all the rotation the job needs — plus a small
/// lean, which is what stops a dozen copies of four meshes reading as a dozen
/// copies of four meshes.
///
/// # Two rules about the randomness
///
/// It draws from a **stream of its own**, salted off the vine's seed with
/// [`SHOOT_STREAM`]. Sharing the wood's stream would mean every nudge of
/// `shoots_per_spur` re-shaped the trunk underneath the shoots being tuned.
///
/// And every spur draws the **same number of values** whether it uses them or
/// not: the count, then five per *potential* shoot. Drawing only for the
/// shoots that exist would make the stream's length depend on the rate, so
/// raising it would re-roll every spur downstream instead of just adding
/// shoots — the same reason [`planting::row_vines`] draws before its miss
/// test.
///
/// [`planting::row_vines`]: super::util::planting
fn shoot_placements(
    params: &VineParams,
    spurs: &[Spur],
    variations: usize,
    seed: u64,
) -> Vec<(String, Placement)> {
    let variations = variations.max(1);
    let mut rng = Rng::new(seed ^ SHOOT_STREAM);
    let mut placements = Vec::new();

    for (index, spur) in spurs.iter().enumerate() {
        let count = shoot_count(params.shoots_per_spur as f64, rng.unit());
        for bud in 0..MAX_SHOOTS_PER_SPUR {
            // Drawn for every bud a spur *could* push, used or not.
            let turn = rng.range(-BUD_SPREAD, BUD_SPREAD);
            let tilt = Vec2::new(
                rng.range(-SHOOT_TILT, SHOOT_TILT) as f32,
                rng.range(-SHOOT_TILT, SHOOT_TILT) as f32,
            );
            let scale = rng.range(1.0 - SHOOT_LENGTH_JITTER, 1.0 + SHOOT_LENGTH_JITTER);
            let pick = rng.unit();
            if bud >= count {
                continue;
            }

            let bud_at = (BUD_TIP - BUD_STEP * bud as f64).max(0.0);
            let at = spur.at(bud_at);
            placements.push((
                // Named for the *bud*, not the shoot's rank: dropping a spur
                // from two shoots to one drops `_1` and leaves `_0` exactly
                // where it was.
                format!("Shoot_{index:02}_{bud}"),
                Placement {
                    position: Vec3::new(at.x as f32, at.y as f32, at.z as f32),
                    // Buds alternate around the stub, so two shoots off one
                    // spur grow apart rather than through each other.
                    yaw: (spur.azimuth() + PI * bud as f64 + turn) as f32,
                    tilt,
                    scale: scale as f32,
                    variation: (pick * variations as f64) as usize % variations,
                },
            ));
        }
    }
    placements
}

fn sides(params: &VineParams) -> usize {
    (params.sides as usize).max(3)
}

fn bark(params: &VineParams, rng: &mut Rng) -> Bark {
    Bark::new(rng, sides(params), params.roughness as f64)
}

// ─── Authoring ──────────────────────────────────────────────────────

/// Authors one merged mesh per variation under [`PROTOTYPE`].
///
/// Reads [`ParcelParams`] directly rather than the solved
/// [`VineyardLayout`] — only `vine_spacing` is needed, and taking it from the
/// params is what lets this run in [`Grow::Prototypes`], which is ahead of
/// where the layout is solved.
fn author_prototypes(
    live: NonSend<LiveStage>,
    params: Res<VineParams>,
    parcel: Res<ParcelParams>,
) -> Result<()> {
    let stage = &live.stage;
    remove_prim(stage, PROTOTYPE)?;
    define_prim(stage, PROTOTYPE, "Scope")?;

    let reach = cordon_reach(parcel.vine_spacing, params.cordon_gap, params.arms) as f64;
    // Counted off the stage rather than read from `ShootParams`, because
    // elements compose by prim path only.
    let shoot_variations = place::prototype_count(stage, shoot::PROTOTYPE);

    for i in 0..params.variations.max(1) {
        // Mixing rather than adding, so neighbouring seeds give unrelated
        // vines instead of the same vine shifted by one variation.
        let seed = params.seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let shape = vine_shape(&params, reach, seed);
        let parts = shape
            .strands
            .iter()
            .map(strand_mesh)
            .collect::<anyhow::Result<Vec<_>>>()?;

        // An `Xform` over a `Wood` mesh rather than one merged `Mesh`: the
        // shoots are references to another element's prototypes, so a vine has
        // to be a prim that can *have* children. Its own wood stays a single
        // mesh — a dozen tubes merge for free, and prim count is what drives
        // the cost of projecting the stage.
        let variation = format!("{PROTOTYPE}/Var_{i}");
        define_prim(stage, &variation, "Xform")?;
        author_mesh(stage, &format!("{variation}/Wood"), &merge_meshes(&parts))?;

        if shoot_variations > 0 {
            let shoots = shoot_placements(&params, &shape.spurs, shoot_variations, seed);
            place::place_referenced(stage, &variation, shoot::PROTOTYPE, &shoots)?;
        }
    }
    Ok(())
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
            label_small("Vine seed"),
            (
                @FeathersSlider { @min: 0.0, @max: 64.0, @value: 0.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
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

    fn params() -> VineParams {
        VineParams::default()
    }

    /// The prototype's merged mesh, at the default cordon reach.
    fn prototype(params: &VineParams) -> crate::elements::util::usd::MeshData {
        let reach =
            cordon_reach(ParcelParams::default().vine_spacing, params.cordon_gap, params.arms)
                as f64;
        let parts: Vec<_> = vine_shape(params, reach, 1)
            .strands
            .iter()
            .map(|s| strand_mesh(s).expect("every strand skins"))
            .collect();
        merge_meshes(&parts)
    }

    fn bounds(mesh: &crate::elements::util::usd::MeshData, axis: usize) -> (f32, f32) {
        mesh.points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(p[axis]), hi.max(p[axis]))
        })
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
        let bilateral = prototype(&params());
        let (x0, x1) = bounds(&bilateral, 0);
        assert!(x1 > 0.4 && x0 < -0.4, "arms both ways, got {x0}..{x1}");

        let unilateral = prototype(&VineParams {
            arms: 1,
            ..params()
        });
        let (x0, x1) = bounds(&unilateral, 0);
        assert!(x1 > 0.9, "one long arm out to the neighbour, got {x1}");
        assert!(
            x0 > -0.1,
            "and nothing much the other way past the trunk, got {x0}"
        );
    }

    #[test]
    fn the_trunk_starts_below_the_ground() {
        let (z0, _) = bounds(&prototype(&params()), 2);
        assert!(
            z0 <= TRUNK_BASE_Z as f32 + 0.01,
            "buried, so a slope leaves no gap, got {z0}"
        );
    }

    #[test]
    fn the_graft_union_is_thicker_than_the_trunk_just_above_it() {
        let p = params();
        assert!(trunk_radius_at(&p, GRAFT_HEIGHT) > trunk_radius_at(&p, 0.30) * 1.4);
        // And it is a local swelling, not just the base taper showing through.
        assert!(trunk_radius_at(&p, GRAFT_HEIGHT) > trunk_radius_at(&p, 0.05));
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
    /// cordons attach there and the placer only ever rotates about Z.
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
    fn a_vine_prototype_stays_within_a_plausible_bounding_box() {
        let p = params();
        let mesh = prototype(&p);
        let reach = cordon_reach(1.2, p.cordon_gap, p.arms);
        let slack = p.spur_length + p.trunk_radius + 0.05;

        let (x0, x1) = bounds(&mesh, 0);
        assert!(x0 > -(reach + slack) && x1 < reach + slack, "{x0}..{x1}");
        let (y0, y1) = bounds(&mesh, 1);
        assert!(y0 > -0.2 && y1 < 0.2, "narrow across the row, got {y0}..{y1}");
        let (z0, z1) = bounds(&mesh, 2);
        assert!(z0 > -0.1 && z1 < p.trunk_height + slack, "{z0}..{z1}");
    }

    #[test]
    fn a_vines_wood_is_reproducible() {
        let build = || {
            vine_shape(&params(), 0.5, 9)
                .strands
                .iter()
                .map(|s| strand_mesh(s).unwrap().points)
                .collect::<Vec<_>>()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn variations_differ_from_one_another() {
        let mesh = |seed| {
            let shape = vine_shape(&params(), 0.5, seed);
            merge_meshes(
                &shape
                    .strands
                    .iter()
                    .map(|s| strand_mesh(s).unwrap())
                    .collect::<Vec<_>>(),
            )
            .points
        };
        assert_ne!(mesh(1), mesh(2));
    }

    // ─── Shoots ─────────────────────────────────────────────────────

    fn shoots(params: &VineParams) -> Vec<(String, Placement)> {
        let shape = vine_shape(params, 0.5, 4);
        shoot_placements(params, &shape.spurs, 4, 4)
    }

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

    /// A shoot grows from a bud, and a bud is on a spur. If a placement drifted
    /// off its stub, the shoot would hang in mid-air beside the wood.
    #[test]
    fn shoots_sit_on_the_buds_of_their_spurs() {
        let p = params();
        let shape = vine_shape(&p, 0.5, 4);
        let placed = shoot_placements(&p, &shape.spurs, 4, 4);
        assert!(!placed.is_empty(), "the default vine grows some");

        for (name, placement) in &placed {
            let index: usize = name["Shoot_".len().."Shoot_00".len()]
                .parse()
                .expect("the name carries its spur's index");
            let spur = shape.spurs[index];
            let offset = Vector3::new(
                placement.position.x as f64 - spur.base.x,
                placement.position.y as f64 - spur.base.y,
                placement.position.z as f64 - spur.base.z,
            );

            let along = offset.dot(&spur.direction);
            assert!(
                (0.0..=spur.length).contains(&along),
                "{name} sits between the cordon and the spur's tip, got {along}"
            );
            assert!(
                offset.cross(&spur.direction).norm() < 1e-6,
                "{name} sits on the spur's axis, not beside it"
            );
        }
    }

    /// Every spur draws the same values whether it uses them or not, so a
    /// spur that drops from two shoots to one keeps the one it had — rather
    /// than every shoot in the vine shuffling because the stream got shorter.
    #[test]
    fn a_bud_keeps_its_shoot_when_the_rate_drops() {
        let lush = shoots(&VineParams {
            shoots_per_spur: 2.0,
            ..params()
        });
        let sparse = shoots(&VineParams {
            shoots_per_spur: 1.0,
            ..params()
        });

        assert!(sparse.len() < lush.len(), "fewer shoots overall");
        for (name, placement) in &sparse {
            assert!(name.ends_with("_0"), "only the top bud pushes, got {name}");
            let same = lush
                .iter()
                .find(|(other, _)| other == name)
                .expect("the bud is still there");
            assert_eq!(&same.1, placement, "and its shoot has not moved");
        }
    }

    #[test]
    fn a_vine_pruned_to_no_shoots_places_none() {
        assert!(
            shoots(&VineParams {
                shoots_per_spur: 0.0,
                ..params()
            })
            .is_empty()
        );
    }

    /// Shoots draw from their own stream, so dialling `shoots_per_spur` in the
    /// viewer must not re-shape the trunk under them.
    #[test]
    fn changing_the_shoot_rate_leaves_the_wood_alone() {
        let wood = |rate: f32| {
            let p = VineParams {
                shoots_per_spur: rate,
                ..params()
            };
            vine_shape(&p, 0.5, 4)
                .strands
                .iter()
                .map(|s| strand_mesh(s).unwrap().points)
                .collect::<Vec<_>>()
        };
        assert_eq!(wood(1.8), wood(0.0), "same wood, however many shoots");
    }

    /// Pins the prototype's shape: an `Xform` over one merged wood mesh plus a
    /// prim per shoot. `place_referenced` reads that type off the stage, so if
    /// `Var_0` were still a `Mesh` every planted vine would be a `Mesh`
    /// carrying children no renderer would reach.
    #[test]
    fn a_vine_prototype_is_an_xform_over_its_wood_and_its_shoots() {
        let stage = crate::generate::generate_stage(&VineyardParams::default()).unwrap();
        let path =
            |suffix: &str| openusd::sdf::path(format!("{PROTOTYPE}/Var_0{suffix}")).unwrap();

        assert!(
            openusd::schemas::geom::Xform::get(&stage, path(""))
                .unwrap()
                .is_some(),
            "the variation is an Xform, so it can have children"
        );
        assert!(
            openusd::schemas::geom::Mesh::get(&stage, path(""))
                .unwrap()
                .is_none(),
            "and not a Mesh, which would swallow them"
        );
        assert!(usd_bevy::authoring::prim_exists(
            &stage,
            &format!("{PROTOTYPE}/Var_0/Wood")
        ));
        assert!(
            usd_bevy::authoring::prim_exists(&stage, &format!("{PROTOTYPE}/Var_0/Shoot_00_0")),
            "the first bud of the first spur pushed a shoot"
        );
    }

    #[test]
    fn authors_one_prototype_per_variation() {
        let stage = crate::generate::generate_stage(&VineyardParams {
            vine: VineParams {
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
        let stage = crate::stage::new_stage("vine.usda").unwrap();
        let mut world = World::new();
        world.insert_non_send(LiveStage::new(stage.clone()));
        world.insert_resource(ParcelParams::default());
        world.insert_resource(VineParams {
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

        world.insert_resource(VineParams {
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
