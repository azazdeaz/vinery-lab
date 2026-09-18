//! Weed element — the plants that come up where nothing was sown.
//!
//! Under the vines is a strip nothing drives over and, over most of Europe,
//! nothing grows on purpose: it is sprayed, hoed, mown or left alone, and
//! what comes up is decided by which. Out in the alley the sward keeps most
//! weeds down, and the few that escape are the tall ones a machine meets.
//! This element places individual plants in both zones from a short table
//! of growth habits — a tuft, a mat, a rosette, a bolter, a sprawling
//! broadleaf — each standing in for the species commonest in that habit, and
//! lets the strip's regime and the season pick between them.
//!
//! # One mesh per plant
//!
//! A plant is one geometry prim: its leaves and stems are built and merged
//! here, and the plant carries the part directly, as a leaf does. Two plants
//! of one species at about the same stage and size share a mesh.
//!
//! # The layer
//!
//! Layout-driven, like [`cover`](super::cover): [`author`] places a
//! [`WeedConfig`] on every kept slot of every strip and alley band, and
//! [`build`] turns the distinct configs into plants. No `reauthor`: a params
//! edit re-authors the layer.
//!
//! Prims: `/Vineyard/Weeds/Row_003/Weed_0017` stands in the under-vine strip
//! of `Row_003`, `/Vineyard/Weeds/Alley_003/Weed_0004` in the alley between
//! rows 3 and 4. A plant is named by its slot on a fixed grid, so a pressure
//! edit thins the weeds rather than renumbering them.

use std::f64::consts::TAU;

use anyhow::Context;
use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use nalgebra::Point3;

use super::terrain::Ground;
use super::util::mesh::{MeshData, merge_meshes};
use super::util::parcel::{Band, VineyardLayout};
use super::util::scatter::jittered_grid;
use super::util::shapes::{self, bent, lying, standing};
use super::util::strand::{Bark, Strand, strand_mesh};
use super::util::{color, material, par_map};
use super::{Grow, Rng, SceneParams, salt};
use crate::quantize::{Metric, farthest_first};
use crate::scene::{Geometry, Library, Order, PrimRoot, Surface, UsdType, configs_changed, placed};
use crate::ui::{Staged, Tip, dropdown};

/// The mesh-library prefix the plants are registered under.
pub const PART: &str = "Weed";

/// The prim this element owns under the scene root.
pub const WEEDS: &str = "Weeds";

/// Marks the subtree, so a rebuild can drop the last one.
#[derive(Component)]
pub struct Weeds;

/// The slot grid a weed may stand on, in meters: twenty-five slots to the
/// square metre, which caps `pressure` at that.
const SLOT: f32 = 0.2;

/// How far a plant sits off its slot's centre, in slots. Half: anywhere in
/// the cell, and never on top of a neighbour.
const SLOT_JITTER: f32 = 0.5;

/// How far a plant's stage strays from the season, either way.
const STAGE_JITTER: f64 = 0.25;

/// The spread of plant sizes, as a multiplier on a species' typical size.
const VIGOUR: std::ops::Range<f64> = 0.6..1.4;

/// Sides around a stem, and rings along it per meter. Stems are millimetres
/// across; nothing finer would show.
const SIDES: usize = 5;
const RINGS_PER_METER: f64 = 12.0;

/// The stream the strips are planted from, salted per row.
const STRIP_STREAM: u64 = 0x4C8A_2F17_D93B_E605;

/// The stream the alleys are planted from, salted per alley.
const ALLEY_STREAM: u64 = 0xB16E_9D40_73A2_5CF9;

/// The stream a plant's mesh is drawn from, salted per representative.
const PLANT_STREAM: u64 = 0xE73D_0B58_A14C_962F;

/// Salt splitting the weeds' shades off every other stream.
const WEED_SHADE: u64 = 0x5A1F_C7E3_08D6_B294;

/// How far apart two plants of different species are. Categorical, and
/// larger than any stage or size can reach, so a budget of five keeps one
/// mesh per species before it buys a second of any.
const SPECIES_APART: f32 = 1.0;

// ─── Strip ──────────────────────────────────────────────────────────

/// What is done to the under-vine strip — which is what decides what grows
/// there.
///
/// Named by string in [`WeedParams::strip`]; this is the parsed form.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Strip {
    /// Sprayed. What survives escapes by its cycle or tolerates the spray:
    /// small annual grasses, and the tall bolters glyphosate no longer kills.
    Herbicide,
    /// Hoed. Ruderal annuals that come back from seed between passes.
    Tilled,
    /// Mown. Perennials that duck the blade: rosettes and tufts.
    #[default]
    Mown,
    /// Left alone: everything, and the tall ones tallest.
    Untouched,
}

impl Strip {
    pub const ALL: [Strip; 4] = [
        Strip::Herbicide,
        Strip::Tilled,
        Strip::Mown,
        Strip::Untouched,
    ];

    /// The names, in [`ALL`](Self::ALL) order — what a config says.
    pub const NAMES: [&'static str; 4] = ["herbicide", "tilled", "mown", "untouched"];

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }

    pub fn parse(name: &str) -> Option<Strip> {
        Self::ALL.into_iter().find(|strip| strip.name() == name)
    }
}

// ─── Species ────────────────────────────────────────────────────────

/// A growth habit: the shape a plant is built as. Each stands in for the
/// species commonest in it, and the mesh builders are one per habit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Habit {
    /// A tussock of soft blades from one crown.
    Tuft,
    /// Stems running out along the ground, small leaves in pairs.
    Mat,
    /// A flat whorl of leaves from a taproot.
    Rosette,
    /// A rosette that, once bolted, sends up one leafy wand — the weed that
    /// reaches the fruiting wire.
    Bolter,
    /// Stems that run out from a woody crown and turn up, with round leaves
    /// off them; hollow in the middle.
    Broadleaf,
}

/// One species: its habit, and how much of the strip it holds under each
/// regime.
#[derive(Clone, Copy, Debug)]
pub struct Species {
    pub name: &'static str,
    pub habit: Habit,
    /// Relative share under each [`Strip`], in [`Strip::ALL`] order.
    pub weights: [f32; 4],
    /// `-1.0` for a plant of the spring flush, `1.0` for one of the summer's;
    /// how its share tilts with [`SceneParams::season`].
    pub summer: f32,
}

/// The species the floor is built from, one per habit. Indexed by
/// [`WeedConfig::species`].
pub const SPECIES: &[Species] = &[
    Species {
        name: "Poa annua",
        habit: Habit::Tuft,
        weights: [0.5, 0.3, 0.3, 0.15],
        summer: 0.0,
    },
    Species {
        name: "Stellaria media",
        habit: Habit::Mat,
        weights: [0.1, 0.4, 0.1, 0.2],
        summer: -1.0,
    },
    Species {
        name: "Taraxacum officinale",
        habit: Habit::Rosette,
        weights: [0.05, 0.1, 0.5, 0.25],
        summer: 0.0,
    },
    Species {
        name: "Erigeron canadensis",
        habit: Habit::Bolter,
        weights: [0.3, 0.1, 0.05, 0.25],
        summer: 1.0,
    },
    Species {
        name: "Malva sylvestris",
        habit: Habit::Broadleaf,
        weights: [0.05, 0.1, 0.05, 0.15],
        summer: 0.5,
    },
];

/// Each species' share of the plants under `strip` at `season`, with the
/// tall habits — bolters and broadleaves — together holding `tall` of the
/// total and the low ones the rest.
fn shares(strip: Strip, season: f32, tall: f32) -> Vec<f32> {
    let is_tall = |species: &Species| matches!(species.habit, Habit::Bolter | Habit::Broadleaf);
    let raw: Vec<f32> = SPECIES
        .iter()
        .map(|species| {
            let tilt = 1.0 + species.summer * (2.0 * season - 1.0) * 0.5;
            species.weights[strip as usize] * tilt.max(0.05)
        })
        .collect();
    let sum = |tall_ones: bool| -> f32 {
        SPECIES
            .iter()
            .zip(&raw)
            .filter(|(species, _)| is_tall(species) == tall_ones)
            .map(|(_, weight)| weight)
            .sum()
    };
    let (tall_sum, low_sum) = (sum(true), sum(false));
    SPECIES
        .iter()
        .zip(&raw)
        .map(|(species, weight)| {
            let (group, share) = if is_tall(species) {
                (tall_sum, tall)
            } else {
                (low_sum, 1.0 - tall)
            };
            if group > 0.0 {
                weight / group * share
            } else {
                0.0
            }
        })
        .collect()
}

/// The species a unit draw lands on, walking the shares.
fn pick(shares: &[f32], draw: f64) -> usize {
    let mut cumulative = 0.0;
    for (index, share) in shares.iter().enumerate() {
        cumulative += *share as f64;
        if draw < cumulative {
            return index;
        }
    }
    shares.len().saturating_sub(1)
}

// ─── Config ─────────────────────────────────────────────────────────

/// One plant's shape.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct WeedConfig {
    /// Index into [`SPECIES`].
    pub species: u32,
    /// How far along its season the plant is, `0..=1`: size for every
    /// habit, and for a bolter whether it has bolted.
    pub stage: f32,
    /// Size, as a multiplier on the species' typical one.
    pub vigour: f32,
    /// Triangles a leaf's fill is cut into.
    pub detail: u32,
}

impl WeedConfig {
    /// Clamped here rather than in the builder, so the config a metric
    /// compares is the plant that actually gets built.
    pub fn new(species: usize, stage: f32, vigour: f32, detail: u32) -> Self {
        Self {
            species: (species % SPECIES.len()) as u32,
            stage: stage.clamp(0.05, 1.0),
            vigour: vigour.max(0.1),
            detail: detail.max(1),
        }
    }
}

/// Two plants share a mesh when they are one species at about the same
/// stage and size.
pub struct WeedMetric;

impl Metric<WeedConfig> for WeedMetric {
    fn distance(&self, a: &WeedConfig, b: &WeedConfig) -> f32 {
        let species = if a.species == b.species {
            0.0
        } else {
            SPECIES_APART
        };
        let stage = (a.stage - b.stage) * 0.5;
        let vigour = (a.vigour / b.vigour).ln().abs() * 0.3;
        let detail = (a.detail as f32 - b.detail as f32) * 0.001;
        (species * species + stage * stage + vigour * vigour + detail * detail).sqrt()
    }
}

// ─── Params ─────────────────────────────────────────────────────────

#[derive(Resource, Clone, Debug, PartialEq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct WeedParams {
    /// One of [`Strip::NAMES`]: what is done to the under-vine strip, which
    /// picks the species. A name that is none of them is read as the default
    /// with a warning — see [`strip`](Self::strip).
    pub strip: String,
    /// How far the under-vine strip reaches either side of the trunks, in
    /// meters.
    pub strip_width: f32,
    /// Plants per square metre in the strip. Zero is a clean strip; the
    /// ceiling is one plant per slot, twenty-five.
    pub pressure: f32,
    /// Plants per square metre in the alley, between the strips — the
    /// escapes.
    pub alley_pressure: f32,
    /// The share of plants that are tall — bolters and broadleaves — as
    /// against low tufts, mats and rosettes. `0..=1`.
    pub tall: f32,
    /// How many distinct plant meshes the scene may hold. A budget, not a
    /// count; see [`WeedMetric`].
    pub variations: u32,
    /// Triangles a leaf is cut into.
    pub detail: u32,
}

impl Default for WeedParams {
    fn default() -> Self {
        Self {
            strip: Strip::default().name().to_string(),
            strip_width: 0.3,
            pressure: 6.0,
            alley_pressure: 0.5,
            tall: 0.3,
            variations: 24,
            detail: 16,
        }
    }
}

impl WeedParams {
    /// The parsed strip regime. An unknown name comes back as the default
    /// rather than as an error: a build system is no place to fail, and the
    /// Python boundary has already rejected it — see `python.rs`.
    pub fn strip(&self) -> Strip {
        Strip::parse(&self.strip).unwrap_or_else(|| {
            warn!(
                "weed strip {:?} is not one of {:?}; using {:?}",
                self.strip,
                Strip::NAMES,
                Strip::default().name()
            );
            Strip::default()
        })
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<WeedParams>().add_systems(
        PreUpdate,
        (
            author.run_if(regime_changed),
            // Gated on the regime as well as on the configs, so a change that
            // leaves no plants at all still clears the library.
            build.run_if(configs_changed::<WeedConfig>.or_eager(regime_changed)),
        )
            .chain()
            .in_set(Grow::Scatter),
    );
}

/// Run condition: something this layer authors from has changed.
fn regime_changed(
    params: Res<WeedParams>,
    layout: Res<VineyardLayout>,
    scene: Res<SceneParams>,
) -> bool {
    params.is_changed() || layout.is_changed() || scene.is_changed()
}

// ─── Authoring ──────────────────────────────────────────────────────

/// Places a plant config on every kept slot of every strip and alley.
pub(crate) fn author(
    mut commands: Commands,
    scene: Res<SceneParams>,
    params: Res<WeedParams>,
    layout: Res<VineyardLayout>,
    ground: Res<Ground>,
    root: Res<PrimRoot>,
    existing: Query<Entity, With<Weeds>>,
) {
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let weeds = commands
        .spawn((
            Weeds,
            Name::new(WEEDS),
            UsdType("Scope"),
            Transform::IDENTITY,
            Visibility::default(),
            ChildOf(root.0),
        ))
        .id();

    let shares = shares(params.strip(), scene.season, params.tall.clamp(0.0, 1.0));
    let mut planter = Planter {
        commands: &mut commands,
        shares: &shares,
        detail: params.detail,
        season: scene.season,
        bounds: layout.bounds,
        ground: &ground,
        order: 0,
    };

    let strip_width = params.strip_width.max(0.0);
    for (index, row) in layout.rows.iter().enumerate() {
        let mut rng = Rng::new(scene.seed ^ STRIP_STREAM ^ salt(index as u64));
        planter.plant(
            weeds,
            format!("Row_{index:03}"),
            &row.band(strip_width),
            params.pressure,
            &mut rng,
        );
    }
    for alley in layout.alleys() {
        // The alley less the strips either side of it.
        let half_width = alley.band.half_width - strip_width;
        if half_width <= 0.0 {
            continue;
        }
        let mut rng = Rng::new(scene.seed ^ ALLEY_STREAM ^ salt(alley.index as u64));
        planter.plant(
            weeds,
            format!("Alley_{:03}", alley.index),
            &Band {
                half_width,
                ..alley.band
            },
            params.alley_pressure,
            &mut rng,
        );
    }

    if planter.order == 0 {
        commands.entity(weeds).despawn();
    }
}

/// What every band is planted with.
struct Planter<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    shares: &'a [f32],
    detail: u32,
    season: f32,
    bounds: Rect,
    ground: &'a Ground,
    order: u64,
}

impl Planter<'_, '_, '_> {
    /// Plants `band` at `pressure`, under a `Scope` named `name` — spawned
    /// only if a plant survives.
    ///
    /// Five draws per slot, after the grid's own two: whether it is kept,
    /// which species, its vigour, its stage, its yaw. All five are taken for
    /// every slot, so thinning the pressure leaves the survivors as they were.
    fn plant(&mut self, parent: Entity, name: String, band: &Band, pressure: f32, rng: &mut Rng) {
        let keep = (pressure * SLOT * SLOT).clamp(0.0, 1.0) as f64;
        let mut plants = Vec::new();
        for slot in jittered_grid(band, SLOT, SLOT_JITTER, rng) {
            let kept = rng.unit() < keep;
            let species = pick(self.shares, rng.unit());
            let vigour = rng.range(VIGOUR.start, VIGOUR.end);
            let stage = self.season as f64 + rng.range(-STAGE_JITTER, STAGE_JITTER);
            let yaw = rng.unit() * TAU;
            if !kept || !self.bounds.contains(slot.position) {
                continue;
            }
            plants.push((
                slot.index,
                self.ground.lift(slot.position),
                yaw as f32,
                WeedConfig::new(species, stage as f32, vigour as f32, self.detail),
            ));
        }
        if plants.is_empty() {
            return;
        }

        let group = self
            .commands
            .spawn((
                Name::new(name),
                UsdType("Scope"),
                Transform::IDENTITY,
                Visibility::default(),
                ChildOf(parent),
            ))
            .id();
        for (slot, position, yaw, config) in plants {
            self.order += 1;
            self.commands.spawn((
                Name::new(format!("Weed_{slot:04}")),
                // Upright: a plant grows against gravity whatever the slope.
                placed(position, yaw, Vec2::ZERO, 1.0),
                Visibility::default(),
                config,
                Order(self.order),
                ChildOf(group),
            ));
        }
    }
}

// ─── Building ───────────────────────────────────────────────────────

/// Builds one mesh per distinct plant and gives it to every plant that drew
/// it. A plant has nothing hanging off it, so it carries its geometry
/// directly and expands into nothing.
pub(crate) fn build(
    mut commands: Commands,
    mut library: Library,
    params: Res<WeedParams>,
    plants: Query<(Entity, &Order, &WeedConfig)>,
) -> Result<()> {
    library.clear(PART);

    let mut grown: Vec<(Order, Entity, WeedConfig)> = plants
        .iter()
        .map(|(entity, order, config)| (*order, entity, *config))
        .collect();
    grown.sort_by_key(|(order, ..)| *order);

    let configs: Vec<WeedConfig> = grown.iter().map(|(_, _, config)| *config).collect();
    let book = farthest_first(
        &configs,
        params.variations.max(1) as usize,
        0.0,
        &WeedMetric,
    );

    let meshes = par_map(&book.representatives, |index, config| {
        plant_mesh(config, index as u64)
            .map(|mesh| mesh.to_mesh())
            .with_context(|| {
                format!(
                    "weed {} could not be built",
                    SPECIES[config.species as usize].name
                )
            })
    });

    let mut geometry: Vec<Geometry> = Vec::with_capacity(book.len());
    for (index, mesh) in meshes.into_iter().enumerate() {
        geometry.push(library.part(PART, index, mesh?, surface(index as u64)));
    }
    for ((_, entity, _), drew) in grown.iter().zip(&book.assignment) {
        commands
            .entity(*entity)
            .insert(geometry[*drew as usize].clone());
    }
    Ok(())
}

/// One plant in its own frame: the root collar on the origin, growing up
/// `+Z`.
fn plant_mesh(config: &WeedConfig, index: u64) -> anyhow::Result<MeshData> {
    let species = &SPECIES[config.species as usize % SPECIES.len()];
    let mut rng = Rng::new(PLANT_STREAM ^ salt(index));
    let (size, stage, triangles) = (
        config.vigour as f64,
        config.stage as f64,
        config.detail as f64,
    );
    match species.habit {
        Habit::Tuft => tuft(size, stage, triangles, &mut rng),
        Habit::Mat => mat(size, stage, triangles, &mut rng),
        Habit::Rosette => rosette(size, stage, triangles, &mut rng),
        Habit::Bolter => bolter(size, stage, triangles, &mut rng),
        Habit::Broadleaf => broadleaf(size, stage, triangles, &mut rng),
    }
}

/// A stem: a strand skinned at [`SIDES`] and [`RINGS_PER_METER`] with no
/// bark. The control points must not be collinear — a straight polyline has
/// no curvature for the tube's frames to follow — so every caller bows its
/// stems by at least a millimetre.
fn stem(points: Vec<Point3<f64>>, radii: Vec<f64>) -> anyhow::Result<MeshData> {
    strand_mesh(&Strand::new(
        points,
        radii,
        SIDES,
        RINGS_PER_METER,
        Bark::none(),
    ))
}

/// The point `t` of the way along a polyline, by segment.
fn along(points: &[Point3<f64>], t: f64) -> Vec3 {
    let segments = points.len().saturating_sub(1).max(1);
    let s = (t * segments as f64).clamp(0.0, segments as f64);
    let i = (s.floor() as usize).min(segments - 1);
    let f = s - i as f64;
    let (a, b) = (points[i], points[(i + 1).min(points.len() - 1)]);
    Vec3::new(
        (a.x + (b.x - a.x) * f) as f32,
        (a.y + (b.y - a.y) * f) as f32,
        (a.z + (b.z - a.z) * f) as f32,
    )
}

/// A leaf standing at `translation`, turned to `yaw` and lifted `pitch`.
fn leaf_at(translation: Vec3, yaw: f64, pitch: f64, scale: f64) -> Transform {
    Transform {
        translation,
        rotation: lying(yaw, pitch),
        scale: Vec3::splat(scale as f32),
    }
}

/// *Poa annua*: a tussock of soft blades from one crown, a hand high.
///
/// Draws: per variant its length and droop; per blade which variant, its
/// yaw and its lean.
fn tuft(size: f64, stage: f64, triangles: f64, rng: &mut Rng) -> anyhow::Result<MeshData> {
    let reach = 0.10 * size * (0.4 + 0.6 * stage);
    let variants: Vec<MeshData> = (0..3)
        .map(|_| {
            let length = reach * rng.range(0.5, 1.0);
            let droop = rng.range(0.4, 1.3);
            shapes::blade_mesh(length, 0.003, droop, 300.0, triangles)
        })
        .collect::<anyhow::Result<_>>()?;
    let parts: Vec<MeshData> = (0..30)
        .map(|_| {
            let variant = (rng.unit() * variants.len() as f64) as usize;
            let yaw = rng.unit() * TAU;
            let lean = rng.range(0.1, 1.0);
            variants[variant].transformed(&Transform {
                rotation: standing(yaw, lean),
                ..default()
            })
        })
        .collect();
    Ok(merge_meshes(&parts))
}

/// *Stellaria media*: stems running out along the ground from one root,
/// small ovate leaves in opposite pairs along them.
///
/// Draws: per stem its heading, its sway and the lift of its tip; per leaf
/// its turn off the stem and its pitch.
fn mat(size: f64, stage: f64, triangles: f64, rng: &mut Rng) -> anyhow::Result<MeshData> {
    let leaf = bent(
        shapes::leaf(0.015 * size, 0.008 * size, 0.4),
        0.3,
        0.0,
        triangles,
    )?;
    let run = 0.18 * size * (0.3 + 0.7 * stage);
    let stems = 4;
    let mut parts = Vec::new();
    for s in 0..stems {
        let heading = (s as f64 + rng.range(-0.3, 0.3)) / stems as f64 * TAU;
        let sway = rng.range(-0.15, 0.15);
        let lift = rng.range(0.01, 0.03);
        let points = vec![
            Point3::new(0.0, 0.0, 0.005),
            Point3::new(run * 0.5, run * sway, 0.01),
            Point3::new(run, 0.0, lift),
        ];
        let frame = Transform::from_rotation(Quat::from_rotation_z(heading as f32));
        parts.push(stem(points.clone(), vec![0.001, 0.0009, 0.0006])?.transformed(&frame));

        let nodes = ((run / 0.025) as usize).max(1);
        for n in 1..=nodes {
            let at = frame.transform_point(along(&points, n as f64 / nodes as f64));
            for side in [-1.0f64, 1.0] {
                let turn = heading + side * rng.range(0.9, 1.5);
                let pitch = rng.range(0.1, 0.4);
                parts.push(leaf.transformed(&leaf_at(at, turn, pitch, 1.0)));
            }
        }
    }
    Ok(merge_meshes(&parts))
}

/// *Taraxacum officinale*: a flat whorl of toothed leaves from a taproot's
/// crown, nothing standing up.
///
/// Draws: per leaf its length, its heading, its pitch and its droop.
fn rosette(size: f64, stage: f64, triangles: f64, rng: &mut Rng) -> anyhow::Result<MeshData> {
    let leaves = 10;
    let reach = 0.15 * size * (0.4 + 0.6 * stage);
    let mut parts = Vec::with_capacity(leaves);
    for i in 0..leaves {
        let length = reach * rng.range(0.6, 1.0);
        let heading = (i as f64 + rng.range(-0.3, 0.3)) / leaves as f64 * TAU;
        let pitch = rng.range(0.15, 0.6);
        let droop = rng.range(0.3, 0.8);
        let leaf = bent(
            shapes::toothed(length, length * 0.28, 0.6, 5),
            droop,
            0.0,
            triangles,
        )?;
        parts.push(leaf.transformed(&leaf_at(Vec3::Z * 0.005, heading, pitch, 1.0)));
    }
    Ok(merge_meshes(&parts))
}

/// *Erigeron canadensis*: a basal rosette that, once bolted, sends up one
/// wand clothed spirally in narrow leaves — to over a metre, into the
/// fruiting zone. The rosette withers as the wand grows.
///
/// Draws: per basal leaf its length, its heading and its pitch; then the
/// wand's sway; per wand leaf its turn and its pitch.
fn bolter(size: f64, stage: f64, triangles: f64, rng: &mut Rng) -> anyhow::Result<MeshData> {
    let mut parts = Vec::new();
    let basal = 8;
    for i in 0..basal {
        let length = 0.06 * size * rng.range(0.6, 1.0) * (1.0 - 0.5 * stage);
        let heading = (i as f64 + rng.range(-0.3, 0.3)) / basal as f64 * TAU;
        let pitch = rng.range(0.2, 0.6);
        let leaf = bent(
            shapes::leaf(length, length * 0.25, 0.45),
            0.4,
            0.0,
            triangles,
        )?;
        parts.push(leaf.transformed(&leaf_at(Vec3::Z * 0.005, heading, pitch, 1.0)));
    }

    let bolted = ((stage - 0.3) / 0.7).max(0.0);
    if bolted > 0.0 {
        let height = 1.2 * size * bolted;
        let sway = rng.range(0.0, TAU);
        let bow = 0.02 * height;
        let points = vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(bow * sway.cos(), bow * sway.sin(), height / 2.0),
            Point3::new(2.0 * bow * sway.cos(), 2.0 * bow * sway.sin(), height),
        ];
        parts.push(stem(
            points.clone(),
            vec![0.004 * size, 0.003 * size, 0.0015 * size],
        )?);

        let leaf = bent(
            shapes::leaf(0.04 * size, 0.008 * size, 0.4),
            0.3,
            0.0,
            triangles,
        )?;
        let nodes = ((height / 0.03) as usize).clamp(1, 40);
        // The golden angle: no two leaves up the wand line up.
        const GOLDEN: f64 = 2.399_963;
        for n in 0..nodes {
            let t = (n as f64 + 0.5) / nodes as f64;
            let turn = n as f64 * GOLDEN + rng.range(-0.2, 0.2);
            let pitch = rng.range(0.8, 1.2);
            let shrink = 1.0 - 0.6 * t;
            parts.push(leaf.transformed(&leaf_at(along(&points, t), turn, pitch, shrink)));
        }
    }
    Ok(merge_meshes(&parts))
}

/// *Malva sylvestris*: stems that run out from a woody crown and then turn
/// up, leaving the middle hollow, with round leaves off them. The shape a
/// weeder cannot reach the middle of.
///
/// Draws: per stem its heading and its sway; per leaf its turn and pitch.
fn broadleaf(size: f64, stage: f64, triangles: f64, rng: &mut Rng) -> anyhow::Result<MeshData> {
    let grown = 0.3 + 0.7 * stage;
    let height = 0.5 * size * grown;
    let reach = 0.3 * size * grown;
    let leaf = bent(
        shapes::leaf(0.06 * size, 0.06 * size, 0.5),
        0.3,
        0.0,
        triangles,
    )?;
    let stems = 4;
    let mut parts = Vec::new();
    for s in 0..stems {
        let heading = (s as f64 + rng.range(-0.3, 0.3)) / stems as f64 * TAU;
        let sway = rng.range(-0.2, 0.2);
        let points = vec![
            Point3::new(0.0, 0.0, 0.02),
            Point3::new(reach * 0.5, reach * sway, 0.05),
            Point3::new(reach, reach * sway * 0.5, height * 0.4),
            Point3::new(reach * 1.1, 0.0, height),
        ];
        let frame = Transform::from_rotation(Quat::from_rotation_z(heading as f32));
        let radii = [0.004, 0.0035, 0.003, 0.002].map(|r| r * size).to_vec();
        parts.push(stem(points.clone(), radii)?.transformed(&frame));

        let nodes = 4;
        for n in 1..=nodes {
            let at = frame.transform_point(along(&points, n as f64 / nodes as f64));
            let turn = heading + rng.range(-1.2, 1.2);
            let pitch = rng.range(0.1, 0.5);
            parts.push(leaf.transformed(&leaf_at(at, turn, pitch, 1.0)));
        }
    }
    Ok(merge_meshes(&parts))
}

/// A plant's foliage, shaded per representative.
fn surface(index: u64) -> Surface {
    material::FOLIAGE.blade(color::shade(
        color::srgb(color::WEED),
        &mut Rng::new(color::COLOR_STREAM ^ WEED_SHADE ^ salt(index)),
    ))
}

// ─── UI ─────────────────────────────────────────────────────────────

pub fn ui() -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            dropdown(
                "Strip",
                "What is done to the under-vine strip, which is what picks the species growing in it.",
                &Strip::NAMES,
                |params| &params.weed.strip,
                |params, name| params.weed.strip = name.to_string(),
            ),
            label_small("Strip width"),
            (
                @FeathersSlider { @min: 0.1, @max: 0.6, @value: 0.3 }
                Tip("How far the under-vine strip reaches either side of the trunks.")
                SliderStep(0.05)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.weed.strip_width = change.value.max(0.0);
                })
            ),
            label_small("Weed pressure"),
            (
                @FeathersSlider { @min: 0.0, @max: 25.0, @value: 6.0 }
                Tip("Plants per square metre in the strip. Zero is a clean strip; the ceiling is one per slot, twenty-five.")
                SliderStep(0.5)
                SliderPrecision(1)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.weed.pressure = change.value.max(0.0);
                })
            ),
            label_small("Alley weeds"),
            (
                @FeathersSlider { @min: 0.0, @max: 5.0, @value: 0.5 }
                Tip("Plants per square metre in the alley, between the strips — the escapes.")
                SliderStep(0.1)
                SliderPrecision(1)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.weed.alley_pressure = change.value.max(0.0);
                })
            ),
            label_small("Tall share"),
            (
                @FeathersSlider { @min: 0.0, @max: 1.0, @value: 0.3 }
                Tip("The share of plants that are tall bolters and broadleaves, against low tufts, mats and rosettes.")
                SliderStep(0.05)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.weed.tall = change.value.clamp(0.0, 1.0);
                })
            ),
            label_small("Weed variations"),
            (
                @FeathersSlider { @min: 1.0, @max: 64.0, @value: 24.0 }
                Tip("A budget, not a count: how many distinct plant meshes the scene may hold.")
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.weed.variations = change.value.round().max(1.0) as u32;
                })
            ),
            label_small("Weed detail"),
            (
                @FeathersSlider { @min: 4.0, @max: 64.0, @value: 16.0 }
                Tip("Triangles a leaf is cut into.")
                SliderStep(2.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.weed.detail = change.value.round().max(1.0) as u32;
                })
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::VineyardParams;
    use crate::elements::util::testing::{bounds, grown, organs, prim};
    use crate::scene::Prototypes;

    /// A parcel small enough to grow in a test and wide enough for three rows.
    fn small() -> VineyardParams {
        let mut params = VineyardParams::default();
        params.terrain.length = 30.0;
        params.terrain.width = 20.0;
        params
    }

    fn part_count(app: &App) -> usize {
        app.world()
            .resource::<Prototypes>()
            .iter()
            .filter(|(name, _)| name.starts_with("Weed_"))
            .count()
    }

    #[test]
    fn plants_stand_upright_on_the_ground_in_a_strip_or_an_alley() {
        let mut app = grown(small());
        let ground = app.world().resource::<Ground>().clone();
        let layout = app.world().resource::<VineyardLayout>().clone();
        let plants = organs::<WeedConfig>(app.world_mut());
        assert!(plants.len() > 50, "got {}", plants.len());

        let strips: Vec<Band> = layout.rows.iter().map(|row| row.band(0.3)).collect();
        let alleys: Vec<Band> = layout.alleys().iter().map(|alley| alley.band).collect();
        for plant in &plants {
            let p = plant.position();
            assert!(
                (p.z - ground.height(p.x, p.y)).abs() < 1e-3,
                "{} floats",
                plant.path
            );
            assert!(plant.up().z > 0.999, "{} leans", plant.path);
            let in_strip = strips.iter().any(|b| b.contains(p.truncate()));
            let in_alley = alleys.iter().any(|b| b.contains(p.truncate()));
            assert!(in_strip || in_alley, "{} is nowhere", plant.path);
            assert_eq!(
                plant.path.starts_with("Weeds/Row_"),
                in_strip,
                "{} is filed under the wrong zone",
                plant.path
            );
        }
    }

    #[test]
    fn the_pressure_is_plants_per_square_metre_of_strip() {
        let mut params = small();
        params.weed.alley_pressure = 0.0;
        params.weed.pressure = 6.0;
        let mut app = grown(params);
        let layout = app.world().resource::<VineyardLayout>().clone();
        let area: f32 = layout
            .rows
            .iter()
            .map(|row| row.band(0.3).length() * 0.6)
            .sum();
        let expected = 6.0 * area;
        let got = organs::<WeedConfig>(app.world_mut()).len() as f32;
        assert!(
            (got - expected).abs() < expected * 0.2,
            "expected about {expected} plants, got {got}"
        );
    }

    #[test]
    fn no_pressure_grows_nothing() {
        let mut params = small();
        params.weed.pressure = 0.0;
        params.weed.alley_pressure = 0.0;
        let mut app = grown(params);
        assert!(prim(app.world_mut(), &["Weeds"]).is_none());
        assert_eq!(part_count(&app), 0);
    }

    #[test]
    fn an_unknown_strip_falls_back_without_panicking() {
        let mut params = small();
        params.weed.strip = "napalm".into();
        let mut app = grown(params);
        assert!(!organs::<WeedConfig>(app.world_mut()).is_empty());
    }

    fn histogram(params: VineyardParams) -> Vec<usize> {
        let mut counts = vec![0; SPECIES.len()];
        for plant in organs::<WeedConfig>(grown(params).world_mut()) {
            counts[plant.config.species as usize] += 1;
        }
        counts
    }

    /// The regime picks the flora: a sprayed strip and one left alone do not
    /// grow the same things, and no share can be turned on without the tall
    /// ones following it.
    #[test]
    fn the_regime_and_the_tall_share_pick_the_species() {
        let mut sprayed = small();
        sprayed.weed.strip = "herbicide".into();
        let mut left = small();
        left.weed.strip = "untouched".into();
        assert_ne!(histogram(sprayed), histogram(left));

        let mut low = small();
        low.weed.tall = 0.0;
        let counts = histogram(low);
        for (species, count) in SPECIES.iter().zip(&counts) {
            if matches!(species.habit, Habit::Bolter | Habit::Broadleaf) {
                assert_eq!(*count, 0, "{} grew with no tall share", species.name);
            }
        }
        assert!(counts.iter().sum::<usize>() > 0);
    }

    #[test]
    fn shares_sum_to_one_and_honour_the_tall_share() {
        for strip in Strip::ALL {
            for season in [0.0, 0.5, 1.0] {
                let s = shares(strip, season, 0.3);
                assert!(
                    (s.iter().sum::<f32>() - 1.0).abs() < 1e-5,
                    "{strip:?} {season}"
                );
                let tall: f32 = SPECIES
                    .iter()
                    .zip(&s)
                    .filter(|(sp, _)| matches!(sp.habit, Habit::Bolter | Habit::Broadleaf))
                    .map(|(_, w)| w)
                    .sum();
                assert!((tall - 0.3).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn the_budget_caps_the_plant_meshes() {
        let mut params = small();
        params.weed.variations = 4;
        assert!(part_count(&grown(params)) <= 4);
    }

    #[test]
    fn a_bolted_bolter_stands_taller_than_its_rosette() {
        let species = SPECIES
            .iter()
            .position(|s| s.habit == Habit::Bolter)
            .unwrap();
        let low = plant_mesh(&WeedConfig::new(species, 0.1, 1.0, 16), 0).unwrap();
        let high = plant_mesh(&WeedConfig::new(species, 1.0, 1.0, 16), 0).unwrap();
        let (_, low_top) = bounds(&low, 2);
        let (_, high_top) = bounds(&high, 2);
        assert!(low_top < 0.1, "a rosette: {low_top}");
        assert!(high_top > 0.8, "a wand: {high_top}");
    }

    /// Every habit builds, stands on its own ground and stays about the size
    /// its species is.
    #[test]
    fn every_species_builds_a_grounded_plant() {
        for (species, expected) in SPECIES.iter().enumerate() {
            let mesh = plant_mesh(&WeedConfig::new(species, 0.7, 1.0, 16), 3)
                .unwrap_or_else(|e| panic!("{}: {e:#}", expected.name));
            assert!(!mesh.face_vertex_counts.is_empty());
            let (lo, hi) = bounds(&mesh, 2);
            assert!(lo > -0.02, "{} digs to {lo}", expected.name);
            assert!(hi > 0.02 && hi < 1.5, "{} reaches {hi}", expected.name);
        }
    }

    #[test]
    fn the_weeds_are_deterministic() {
        let a = organs::<WeedConfig>(grown(small()).world_mut());
        let b = organs::<WeedConfig>(grown(small()).world_mut());
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(
                (&x.path, x.transform, x.config),
                (&y.path, y.transform, y.config)
            );
        }
    }
}
