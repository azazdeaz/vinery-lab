//! Vine element — the permanent woody framework of a grapevine.
//!
//! A vine is a **trunk** rising from the ground to the **head**, where it
//! turns into one or two **cordons** running along the fruiting wire. Each
//! cordon carries **spurs**: the short pruning stubs whose knuckles are the
//! strongest visual signature of a pruned vine. A **graft union** swells the
//! trunk about 15 cm up, as it does on essentially every commercial vine.
//!
//! Only the permanent wood lives here. The annual growth that hangs off the
//! spurs — canes, shoots, leaves, fruit — belongs to elements that don't exist
//! yet, and will be nested into this one's prototypes rather than placed
//! alongside them.
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
//! # Subtrees
//!
//! Two, as the README's rules anticipate: the prototype meshes under
//! [`PROTOTYPE`], and the instancer that plants them under [`PLACEMENT`].
//! [`PLACEMENT`] is a direct sibling of the terrain's subtree rather than
//! nested inside it the way [`grid`](super::grid) is, because
//! [`Row::vine_positions`] already returns `/Vineyard`-space points and
//! `/Vineyard/Terrain` carries no transform — nesting would buy nothing and
//! cost plenty. It would have to move the instancer under `/parts` (a
//! reference target may not be an ancestor of the referrer), and since our
//! prototypes live under `/parts/Vine` too, referencing that subtree into the
//! scene would drag the prototype meshes in with it as a pile of geometry at
//! the origin. `grid` only escapes that because its prototypes are somebody
//! else's subtree.

use std::f64::consts::{PI, TAU};

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use nalgebra::{Point3, Vector3};
use openusd::gf::{self, f16};
use openusd::schemas::geom::PointInstancer;
use openusd::sdf::{self, Value};
use usd_bevy::authoring::{define_prim, remove_prim};
use usd_bevy::live::LiveStage;

use super::parcel::{ParcelParams, VineyardLayout};
use super::strand::{Bark, Bulge, Strand, strand_mesh};
use super::terrain::Ground;
use super::usd::{author_mesh, merge_meshes};
use super::{Grow, Rng};

/// The prototype library this element owns: one `Var_<i>` mesh per variation.
pub const PROTOTYPE: &str = "/parts/Vine";

/// The placed instances. See the module docs for why this is a sibling of the
/// terrain rather than nested inside it.
const PLACEMENT: &str = "/Vineyard/Vines";
const INSTANCER: &str = "/Vineyard/Vines/Rows";

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
    /// Depth of the bark ridges, as a fraction of the local radius.
    pub roughness: f32,
    /// Vertices around each tube. The silhouette — visible on every instance.
    pub sides: u32,
    /// Rings per meter along each tube. Barely visible at row distance, so
    /// this is the cheaper of the two detail knobs to turn down.
    pub detail: u32,
    /// Fraction of planting positions left empty. Real vineyards have gaps,
    /// and a perception model trained without them learns that they can't
    /// happen.
    pub miss_rate: f32,
    /// Fraction of vines that are recent replants rather than mature.
    pub young_rate: f32,
    /// How small the youngest replant is, relative to a mature vine.
    pub young_scale: f32,
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
            roughness: 0.14,
            sides: 8,
            detail: 20,
            miss_rate: 0.03,
            young_rate: 0.08,
            young_scale: 0.55,
        }
    }
}

pub fn plugin(app: &mut App) {
    // `ParcelParams` is deliberately not initialized here — `terrain::plugin`
    // owns it, and `elements::plugin` adds terrain first. If that order ever
    // changes, this element's author systems are what will notice.
    app.init_resource::<VineParams>().add_systems(
        PreUpdate,
        (
            author_prototypes.in_set(Grow::Prototypes).run_if(
                resource_changed::<VineParams>.or_else(resource_changed::<ParcelParams>),
            ),
            author_placement.in_set(Grow::Plants).run_if(
                resource_changed::<VineParams>.or_else(resource_changed::<VineyardLayout>),
            ),
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
/// planting position the instancer put the vine at, and the head has to stay
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

/// The strands of one vine, in the prototype's local frame.
///
/// The *draw order* of `rng` is part of this element's output: inserting a
/// draw in the middle re-rolls every vine downstream of it. The order is the
/// trunk's axis phases, the trunk's bark, then per arm the cordon's phase, the
/// cordon's bark, and each spur's direction, length and bark in turn.
fn vine_strands(params: &VineParams, reach: f64, seed: u64) -> Vec<Strand> {
    let mut rng = Rng::new(seed);
    let mut strands = vec![trunk_strand(params, &mut rng)];
    for arm in 0..params.arms.clamp(1, 2) {
        let sign = if arm == 0 { 1.0 } else { -1.0 };
        strands.extend(cordon_strands(params, reach, sign, &mut rng));
    }
    strands
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
fn cordon_strands(
    params: &VineParams,
    reach: f64,
    sign: f64,
    rng: &mut Rng,
) -> Vec<Strand> {
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

    if (params.spur_length as f64) >= MIN_SPUR_LENGTH {
        for (index, x) in spurs.iter().enumerate() {
            strands.push(spur_strand(params, centerline(*x), sign, index, rng));
        }
    }
    strands
}

/// One spur: a short stub angled up and out from the cordon, alternating sides
/// the way a pruned vine's spurs alternate along the wire.
fn spur_strand(
    params: &VineParams,
    base: Point3<f64>,
    sign: f64,
    index: usize,
    rng: &mut Rng,
) -> Strand {
    let side = if index.is_multiple_of(2) { 1.0 } else { -1.0 };
    let jitter = rng.range(-0.15, 0.15);
    let length = params.spur_length as f64 * rng.range(0.8, 1.2);
    let direction =
        Vector3::new(sign * 0.20, side * 0.55 + jitter, 0.80).normalize();

    let radius = params.cordon_radius as f64;
    // Four points so the fit is always cubic, and the last three so the stub
    // tapers rather than ending in a cylinder.
    let points = vec![
        base - direction * SPUR_EMBED,
        base + direction * (length * 0.35),
        base + direction * (length * 0.70),
        base + direction * length,
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
    for i in 0..params.variations.max(1) {
        // Mixing rather than adding, so neighbouring seeds give unrelated
        // vines instead of the same vine shifted by one variation.
        let seed = params.seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let parts = vine_strands(&params, reach, seed)
            .iter()
            .map(strand_mesh)
            .collect::<anyhow::Result<Vec<_>>>()?;
        // One Mesh per variation rather than one per strand: prototypes are
        // instanced hundreds of times, and prim count is what drives the cost
        // of projecting the stage.
        author_mesh(
            stage,
            &format!("{PROTOTYPE}/Var_{i}"),
            &merge_meshes(&parts),
        )?;
    }
    Ok(())
}

/// Plants the prototypes along the solved rows.
///
/// Runs in [`Grow::Plants`], after [`Grow::Terrain`], so the layout and the
/// ground it is draped on are both current. No `resource_changed::<Ground>`
/// condition is needed alongside the layout's: `parcel::author` reassigns
/// [`VineyardLayout`] unconditionally, so it already registers as changed
/// whenever the ground moved under it.
fn author_placement(
    live: NonSend<LiveStage>,
    params: Res<VineParams>,
    layout: Res<VineyardLayout>,
    ground: Res<Ground>,
) -> Result<()> {
    let stage = &live.stage;
    remove_prim(stage, PLACEMENT)?;
    define_prim(stage, PLACEMENT, "Xform")?;

    let variations = params.variations.max(1);
    let mut rng = Rng::new(params.seed);
    let mut positions = Vec::new();
    let mut orientations = Vec::new();
    let mut scales = Vec::new();
    let mut proto_indices = Vec::new();

    for row in &layout.rows {
        let orientation = yaw_about_z(row.direction());
        for position in row.vine_positions(&ground) {
            // All three draws happen before the miss test, so the stream stays
            // aligned no matter which vines are skipped. Rolling them lazily
            // instead would make every vine past the first change re-roll
            // whenever `miss_rate` was nudged.
            let (miss, age, pick) = (rng.unit(), rng.unit(), rng.unit());
            if miss < params.miss_rate as f64 {
                continue;
            }
            positions.push(gf::vec3f(position.x, position.y, position.z));
            orientations.push(orientation);
            let scale = age_scale(&params, age) as f32;
            scales.push(gf::vec3f(scale, scale, scale));
            proto_indices.push((pick * variations as f64) as i32 % variations as i32);
        }
    }

    let instancer = PointInstancer::define(stage, sdf::path(INSTANCER)?)?;
    instancer.create_prototypes_rel()?.set_targets(
        (0..variations)
            .map(|i| sdf::path(format!("{PROTOTYPE}/Var_{i}")))
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    instancer
        .create_positions_attr()?
        .set(Value::Vec3fVec(positions))?;
    instancer
        .create_orientations_attr()?
        .set(Value::QuathVec(orientations))?;
    instancer.create_scales_attr()?.set(Value::Vec3fVec(scales))?;
    instancer
        .create_proto_indices_attr()?
        .set(Value::IntVec(proto_indices))?;
    Ok(())
}

/// A rotation about Z that carries the prototype's local +X onto `direction`.
///
/// USD types `orientations` as `quath[]`, so the components are half floats —
/// about three decimal digits, which is far finer than a row's direction is
/// ever known to.
fn yaw_about_z(direction: Vec2) -> gf::Quath {
    let half = direction.y.atan2(direction.x) * 0.5;
    let half16 = |v: f32| f16::from_f32(v);
    gf::quath(half16(half.cos()), half16(0.0), half16(0.0), half16(half.sin()))
}

/// Uniform scale for a vine whose age draw came out at `age`.
///
/// Young vines are shorter *and* thinner, which one uniform scale gives for
/// free. Ramping across the young band rather than stepping means a replanted
/// stretch of row shows a spread of ages, not one clone repeated.
fn age_scale(params: &VineParams, age: f64) -> f64 {
    let young_rate = params.young_rate as f64;
    let young_scale = params.young_scale as f64;
    if young_rate <= 0.0 || age >= young_rate {
        return 1.0;
    }
    young_scale + (1.0 - young_scale) * (age / young_rate)
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
            label_small("Missing vines"),
            (
                @FeathersSlider { @min: 0.0, @max: 0.3, @value: 0.03 }
                SliderStep(0.01)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.miss_rate = change.value;
                })
            ),
            label_small("Young vines"),
            (
                @FeathersSlider { @min: 0.0, @max: 0.5, @value: 0.08 }
                SliderStep(0.01)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.young_rate = change.value;
                })
            ),
            label_small("Young vine scale"),
            (
                @FeathersSlider { @min: 0.2, @max: 1.0, @value: 0.55 }
                SliderStep(0.05)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<VineParams>| {
                    params.young_scale = change.value;
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
    fn prototype(params: &VineParams) -> super::super::usd::MeshData {
        let reach =
            cordon_reach(ParcelParams::default().vine_spacing, params.cordon_gap, params.arms)
                as f64;
        let parts: Vec<_> = vine_strands(params, reach, 1)
            .iter()
            .map(|s| strand_mesh(s).expect("every strand skins"))
            .collect();
        merge_meshes(&parts)
    }

    fn bounds(mesh: &super::super::usd::MeshData, axis: usize) -> (f32, f32) {
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
    fn vine_strands_are_reproducible() {
        let build = || {
            vine_strands(&params(), 0.5, 9)
                .iter()
                .map(|s| strand_mesh(s).unwrap().points)
                .collect::<Vec<_>>()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn variations_differ_from_one_another() {
        let mesh = |seed| {
            let strands = vine_strands(&params(), 0.5, seed);
            merge_meshes(
                &strands
                    .iter()
                    .map(|s| strand_mesh(s).unwrap())
                    .collect::<Vec<_>>(),
            )
            .points
        };
        assert_ne!(mesh(1), mesh(2));
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

    /// The instancer under test, plus the layout it was built from.
    fn placed(params: VineParams) -> (openusd::usd::Stage, usize) {
        let vineyard = VineyardParams {
            vine: params,
            ..default()
        };
        let stage = crate::generate::generate_stage(&vineyard).unwrap();
        let layout = {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .add_plugins(crate::elements::plugin);
            app.world_mut()
                .insert_non_send(LiveStage::new(crate::stage::new_stage("l.usda").unwrap()));
            vineyard.clone().insert(app.world_mut());
            app.finish();
            app.cleanup();
            app.update();
            let world = app.world();
            let layout = world.resource::<VineyardLayout>();
            let ground = world.resource::<Ground>();
            layout
                .rows
                .iter()
                .map(|r| r.vine_positions(ground).count())
                .sum()
        };
        (stage, layout)
    }

    fn instancer_value(stage: &openusd::usd::Stage, attr: &str) -> Value {
        let instancer = PointInstancer::get(stage, sdf::path(INSTANCER).unwrap())
            .unwrap()
            .expect("the vine instancer is authored");
        match attr {
            "positions" => instancer.positions_attr(),
            "orientations" => instancer.orientations_attr(),
            "scales" => instancer.scales_attr(),
            _ => instancer.proto_indices_attr(),
        }
        .get::<Value>()
        .unwrap()
        .expect("attribute authored")
    }

    #[test]
    fn the_vine_instancer_targets_every_prototype() {
        let (stage, _) = placed(VineParams {
            variations: 3,
            ..params()
        });
        let targets = PointInstancer::get(&stage, sdf::path(INSTANCER).unwrap())
            .unwrap()
            .unwrap()
            .prototypes_rel()
            .targets()
            .unwrap();
        assert_eq!(targets.len(), 3);
        assert!(targets.iter().all(|p| p.as_str().starts_with(PROTOTYPE)));
    }

    #[test]
    fn the_instancer_places_one_vine_per_planting_position() {
        let (stage, planted) = placed(VineParams {
            miss_rate: 0.0,
            ..params()
        });
        assert!(planted > 100, "the default parcel plants a lot of vines");
        match instancer_value(&stage, "positions") {
            Value::Vec3fVec(v) => assert_eq!(v.len(), planted),
            other => panic!("positions not authored: {other:?}"),
        }
    }

    /// Strictly fewer vines *and* a subset of the ones that survived the lower
    /// rate — which only holds because the miss roll is drawn before the skip,
    /// keeping the stream aligned across settings.
    #[test]
    fn raising_the_miss_rate_drops_instances() {
        let kept = |rate: f32| match instancer_value(
            &placed(VineParams {
                miss_rate: rate,
                ..params()
            })
            .0,
            "positions",
        ) {
            Value::Vec3fVec(v) => v
                .into_iter()
                .map(|p| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()))
                .collect::<Vec<_>>(),
            other => panic!("{other:?}"),
        };

        let few = kept(0.05);
        let many = kept(0.25);
        assert!(many.len() < few.len(), "{} vs {}", many.len(), few.len());
        assert!(
            many.iter().all(|p| few.contains(p)),
            "raising the rate only removes vines, never moves them"
        );
    }

    #[test]
    fn every_vine_instance_faces_along_its_row() {
        let mut vineyard = VineyardParams::default();
        vineyard.parcel.orientation = 30.0;
        vineyard.vine.miss_rate = 0.0;
        let stage = crate::generate::generate_stage(&vineyard).unwrap();

        let direction = Vec2::from_angle(30.0_f32.to_radians());
        match instancer_value(&stage, "orientations") {
            Value::QuathVec(v) => {
                assert!(!v.is_empty());
                for q in v {
                    // Rotating the prototype's local +X by a yaw-only
                    // quaternion has to land on the row's own direction.
                    let quat = Quat::from_xyzw(
                        q.x.to_f32(),
                        q.y.to_f32(),
                        q.z.to_f32(),
                        q.w.to_f32(),
                    )
                    .normalize();
                    let along = quat * Vec3::X;
                    assert!(
                        (along.truncate() - direction).length() < 1e-2,
                        "cordons run along the row, got {along:?} against {direction:?}"
                    );
                }
            }
            other => panic!("orientations not authored as quath[]: {other:?}"),
        }
    }

    #[test]
    fn young_vines_are_scaled_down() {
        let p = params();
        assert_eq!(age_scale(&p, 0.5), 1.0, "a mature vine is full size");
        assert!((age_scale(&p, 0.0) - p.young_scale as f64).abs() < 1e-9);
        assert!(age_scale(&p, 0.04) > p.young_scale as f64);

        let (stage, _) = placed(VineParams {
            young_rate: 0.3,
            ..p.clone()
        });
        match instancer_value(&stage, "scales") {
            Value::Vec3fVec(v) => {
                assert!(v.iter().any(|s| s.x < 0.999), "some vines are young");
                assert!(
                    v.iter().all(|s| s.x >= p.young_scale - 1e-6 && s.x <= 1.0 + 1e-6),
                    "and none outside the age band"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn young_vines_scale_uniformly() {
        let (stage, _) = placed(params());
        match instancer_value(&stage, "scales") {
            Value::Vec3fVec(v) => assert!(
                v.iter().all(|s| s.x == s.y && s.y == s.z),
                "a young vine is shorter and thinner by the same factor"
            ),
            other => panic!("{other:?}"),
        }
    }

    /// Every vine has to sit on the terrain, not float above or sink into it.
    #[test]
    fn vines_sit_on_the_ground() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(crate::elements::plugin);
        app.world_mut()
            .insert_non_send(LiveStage::new(crate::stage::new_stage("g.usda").unwrap()));
        app.finish();
        app.cleanup();
        app.update();

        let ground = app.world().resource::<Ground>();
        let layout = app.world().resource::<VineyardLayout>();
        for row in &layout.rows {
            for position in row.vine_positions(ground) {
                assert!(
                    (position.z - ground.height(position.x, position.y)).abs() < 1e-5,
                    "a vine's base is on the height field"
                );
            }
        }
    }

    #[test]
    fn proto_indices_stay_in_range() {
        let (stage, _) = placed(VineParams {
            variations: 3,
            ..params()
        });
        match instancer_value(&stage, "protoIndices") {
            Value::IntVec(v) => {
                assert!(v.iter().all(|i| (0..3).contains(i)));
                assert!(v.iter().any(|i| *i != v[0]), "picks actually vary");
            }
            other => panic!("{other:?}"),
        }
    }
}
