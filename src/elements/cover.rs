//! Cover element — what grows in the alley between two rows.
//!
//! The alley is the drive surface, and over most of Europe it is grassed: a
//! permanent sward mown a few times a year, sown or simply left to come up,
//! or a winter cereal drilled after harvest and destroyed in spring. What it
//! looks like from a robot's height is decided by a handful of regime knobs —
//! which of those it is, how tall it stands, how much of the alley it covers,
//! whether every second alley is left bare — and those are the params. The
//! species are this module's to pick from the regime.
//!
//! # Tiles, not blades
//!
//! A sward is tens of thousands of blades per square metre, which nothing can
//! hold one prim at a time. The unit here is a **tile**: half a metre square
//! of blades baked into one mesh, laid down an alley on a grid and instanced.
//! Tiles are quantized like every other organ, so an alley of ten thousand of
//! them draws a dozen meshes.
//!
//! # The layer
//!
//! Layout-driven rather than hung off a layer above: [`author`] places a
//! [`TileConfig`] on every slot of every grassed alley from
//! [`VineyardLayout`] and [`Ground`], and [`build`] turns the distinct configs
//! into tile meshes. There is no `reauthor`: a params edit re-authors the
//! whole layer, which at a few thousand entities *is* the cheap in-place edit.
//!
//! Prims: `/Vineyard/Cover/Alley_003` is a `Scope` per alley, holding a
//! `Tile_0042` per grid slot. Alley `k` lies between `Row_k` and `Row_k+1`,
//! and a tile is named by its slot, so a width or density edit leaves gaps
//! rather than renumbering.

use std::f64::consts::{FRAC_PI_2, TAU};

use anyhow::Context;
use bevy::prelude::*;
use nalgebra::Point3;

use super::terrain::Ground;
use super::util::mesh::{MeshData, merge_meshes};
use super::util::parcel::{Band, VineyardLayout};
use super::util::scatter::jittered_grid;
use super::util::strand::{Bark, Strand, strand_mesh};
use super::util::{color, material, par_map, shapes};
use super::{Grow, Rng, SceneParams, salt};
use crate::params::{Choices, Label, Slider};
use crate::quantize::{Metric, farthest_first};
use crate::scene::{Geometry, Library, Order, PrimRoot, Surface, UsdType, configs_changed};

/// The mesh-library prefix the tiles are registered under.
pub const PART: &str = "Sward";

/// The prim this element owns under the scene root.
pub const COVER: &str = "Cover";

/// Marks the subtree, so a rebuild can drop the last one.
#[derive(Component)]
pub struct Cover;

/// A tile's side, in meters.
const TILE: f32 = 0.5;

/// How far a tile may sit off its grid cell, in cells. Small: tiles are
/// square and abut, and a nudge is only there to break the seam.
const TILE_JITTER: f32 = 0.08;

/// How far a tile's height strays from what the params ask, either way, as
/// a fraction — the continuous axis the budget is spent on.
const HEIGHT_SPREAD: f64 = 0.15;

/// Distinct blade meshes baked into one tile. Instanced dozens of times each
/// at their own yaw and lean, which is what keeps a tile from reading as a
/// print.
const BLADE_VARIANTS: usize = 4;

/// A blade's width at the base, in meters. Four millimetres: a ryegrass
/// blade, and the widest thing a sward is made of.
const BLADE_WIDTH: f64 = 0.004;

/// Triangles a blade's fill is cut into. A blade is a sliver; this is what a
/// droop needs and no more.
const BLADE_TRIANGLES: f64 = 8.0;

/// Fold about the midrib, as a curvature in 1/m. A blade is a gutter, and the
/// gutter is what catches the light on one side of the rib and not the other.
const KEEL: f64 = 250.0;

/// How far a blade leans off vertical at the base, in radians.
const LEAN: f64 = 0.35;

/// Drill lines across a cereal tile, in meters. The 12.5 cm a grain drill
/// sows at.
const DRILL_SPACING: f32 = 0.125;

/// Plants along a drill line, in meters.
const DRILL_STEP: f32 = 0.03;

/// A cereal short of this, in meters, is still tillering and has no culm;
/// past it the crop has jointed and stands on a stem.
const CULM_MIN_HEIGHT: f64 = 0.3;

/// A culm's radius at the base, in meters.
const CULM_RADIUS: f64 = 0.002;

/// The longest cereal leaf, in meters, whatever the crop's height.
const CEREAL_LEAF_MAX: f64 = 0.3;

/// Leaves on a cereal plant.
const CEREAL_LEAVES: usize = 3;

/// The stream tiles are placed from.
const TILE_STREAM: u64 = 0x7A3F_9C21_5E0B_D4A7;

/// The stream a tile's blades are drawn from, salted per representative.
const BLADE_STREAM: u64 = 0x1D9E_47B3_C6F2_08A5;

/// The salt splitting the tile shades off every other stream.
const SWARD_SHADE: u64 = 0x2B7C_E5A9_1F63_D048;

/// How far apart two tiles of different kinds are. Categorical: a sward and a
/// cereal are different things, not nearby ones, so this outweighs every
/// continuous axis below.
const KIND_APART: f32 = 1.0;

// ─── Kind ───────────────────────────────────────────────────────────

/// What the alley grows.
///
/// Named by string in [`CoverParams::kind`], which is what reaches Python and
/// the config snippet; this is the parsed form the element works with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// Bare or tilled: nothing grows.
    None,
    /// A sward that came up on its own: ragged heights, gaps, the terroir's
    /// own grasses.
    #[default]
    Spontaneous,
    /// A sown grass sward: even, dense, one height.
    Sown,
    /// A winter cereal drilled in lines along the alley.
    Cereal,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::None, Kind::Spontaneous, Kind::Sown, Kind::Cereal];

    /// The names, in [`ALL`](Self::ALL) order — what a config says.
    pub const NAMES: [&'static str; 4] = ["none", "spontaneous", "sown", "cereal"];

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }

    pub fn parse(name: &str) -> Option<Kind> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

// ─── Config ─────────────────────────────────────────────────────────

/// One tile's shape.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct TileConfig {
    /// Index into [`Kind::ALL`]; never [`Kind::None`], which spawns no tiles.
    pub kind: u32,
    /// Standing height of the tile's tallest blades, in meters.
    pub height: f32,
    /// Fraction of the tile's ground its blades cover, `0..=1`.
    pub cover: f32,
    /// `0.0` green, `1.0` straw.
    pub dryness: f32,
    /// Blades per square metre at full cover. A cereal tile is drilled at a
    /// fixed geometry and ignores it.
    pub detail: u32,
}

impl TileConfig {
    /// Clamped here rather than in the builder, so the config a metric
    /// compares is the tile that actually gets built.
    pub fn new(kind: Kind, height: f32, cover: f32, dryness: f32, detail: u32) -> Self {
        Self {
            kind: kind as u32,
            height: height.max(0.01),
            cover: cover.clamp(0.0, 1.0),
            dryness: dryness.clamp(0.0, 1.0),
            detail: detail.max(1),
        }
    }
}

/// Two tiles share a mesh when they are of one kind and about as tall,
/// dense and dry.
///
/// Height is compared as a ratio: a centimetre shows on a mown sward and not
/// on a cereal, and the log of a ratio is still a metric.
pub struct TileMetric;

impl Metric<TileConfig> for TileMetric {
    fn distance(&self, a: &TileConfig, b: &TileConfig) -> f32 {
        let kind = if a.kind == b.kind { 0.0 } else { KIND_APART };
        let height = (a.height / b.height).ln().abs() * 0.5;
        let cover = (a.cover - b.cover) * 0.3;
        let dryness = (a.dryness - b.dryness) * 0.3;
        let detail = (a.detail as f32 - b.detail as f32) * 0.001;
        (kind * kind + height * height + cover * cover + dryness * dryness + detail * detail).sqrt()
    }
}

// ─── Params ─────────────────────────────────────────────────────────

/// What grows in the alley between two rows.
///
/// `kind` is the regime, by name, and the species are the generator's to pick
/// from it. The sward is built as half-metre tiles of blades, instanced down
/// each alley: `variations` is the budget of distinct tile meshes and `detail`
/// the blades per square metre baked into one.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct CoverParams {
    /// What the alley grows, by name: `none` for a bare or tilled alley,
    /// `spontaneous` for a sward that came up on its own, ragged and gappy,
    /// `sown` for a drilled grass sward, one height and dense, or `cereal`
    /// for a winter rye in drill lines along the alley.
    ///
    /// A name that is none of [`Kind::NAMES`] is read as the default, with a
    /// warning; Python rejects it at the boundary instead.
    #[reflect(@Choices(&Kind::NAMES))]
    pub kind: String,
    /// Leave every second alley bare: the cover on half the alleys, the
    /// tractor's tyres on the other half. The commonest permanent arrangement
    /// in France.
    #[reflect(@Label("Alternate alleys"))]
    pub alternate: bool,
    /// How much of the alley's width the cover spans, as a fraction, centred
    /// on the alley. Three quarters keeps a band clear of the vines either
    /// side.
    #[reflect(@Slider { min: 0.1, max: 1.0, step: 0.05 })]
    pub width: f32,
    /// Standing height, in meters: a few centimetres just after mowing,
    /// half a metre when left to head, more for a cereal in spring.
    #[reflect(@Slider { min: 0.02, max: 1.5, step: 0.01 })]
    pub height: f32,
    /// Fraction of the ground inside the band the cover actually covers.
    #[reflect(@Slider { min: 0.0, max: 1.0, step: 0.05 }, @Label("Cover density"))]
    pub cover: f32,
    /// `0.0` green, `1.0` straw — a Mediterranean alley in August.
    #[reflect(@Slider { min: 0.0, max: 1.0, step: 0.05 })]
    pub dryness: f32,
    /// How many distinct tile meshes the scene may hold. A budget, not a
    /// count.
    ///
    /// See [`TileMetric`] for what it is spent on.
    #[reflect(@Slider { min: 1.0, max: 64.0, step: 1.0 })]
    pub variations: u32,
    /// Blades per square metre baked into a tile at full cover. The one
    /// knob that trades sward density for triangles.
    #[reflect(@Slider { min: 50.0, max: 2000.0, step: 50.0 })]
    pub detail: u32,
}

impl Default for CoverParams {
    fn default() -> Self {
        Self {
            kind: Kind::default().name().to_string(),
            alternate: false,
            width: 0.75,
            height: 0.15,
            cover: 0.7,
            dryness: 0.0,
            variations: 12,
            detail: 600,
        }
    }
}

impl CoverParams {
    /// The parsed kind. An unknown name comes back as the default rather
    /// than as an error: a build system is no place to fail, and the Python
    /// boundary has already rejected it — see `python.rs`.
    pub fn kind(&self) -> Kind {
        Kind::parse(&self.kind).unwrap_or_else(|| {
            warn!(
                "cover kind {:?} is not one of {:?}; using {:?}",
                self.kind,
                Kind::NAMES,
                Kind::default().name()
            );
            Kind::default()
        })
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<CoverParams>().add_systems(
        PreUpdate,
        (
            author.run_if(regime_changed),
            // Gated on the regime as well as on the configs, so a change that
            // leaves no tiles at all still clears the library.
            build.run_if(configs_changed::<TileConfig>.or_eager(regime_changed)),
        )
            .chain()
            .in_set(Grow::Scatter),
    );
}

/// Run condition: something this layer authors from has changed.
fn regime_changed(
    params: Res<CoverParams>,
    layout: Res<VineyardLayout>,
    scene: Res<SceneParams>,
) -> bool {
    params.is_changed() || layout.is_changed() || scene.is_changed()
}

// ─── Authoring ──────────────────────────────────────────────────────

/// Lays a grid of tile configs down every grassed alley.
///
/// Per tile, after the grid's own two draws per slot: the height spread, the
/// cover spread, then the quarter turn — all taken before the bounds test,
/// so a tile that falls outside a rotated parcel leaves its neighbours where
/// they were.
pub(crate) fn author(
    mut commands: Commands,
    scene: Res<SceneParams>,
    params: Res<CoverParams>,
    layout: Res<VineyardLayout>,
    ground: Res<Ground>,
    root: Res<PrimRoot>,
    existing: Query<Entity, With<Cover>>,
) {
    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let kind = params.kind();
    if kind == Kind::None {
        return;
    }
    let cover = commands
        .spawn((
            Cover,
            Name::new(COVER),
            UsdType("Scope"),
            Transform::IDENTITY,
            Visibility::default(),
            ChildOf(root.0),
        ))
        .id();

    let mut order = 0u64;
    for alley in layout.alleys() {
        if params.alternate && alley.index % 2 == 1 {
            continue;
        }
        let band = Band {
            half_width: alley.band.half_width * params.width.clamp(0.0, 1.0),
            ..alley.band
        };
        let group = commands
            .spawn((
                Name::new(format!("Alley_{:03}", alley.index)),
                UsdType("Scope"),
                Transform::IDENTITY,
                Visibility::default(),
                ChildOf(cover),
            ))
            .id();

        let mut rng = Rng::new(scene.seed ^ TILE_STREAM ^ salt(alley.index as u64));
        let yaw = band.direction().to_angle();
        for slot in jittered_grid(&band, TILE, TILE_JITTER, &mut rng) {
            let height = params.height * rng.range(1.0 - HEIGHT_SPREAD, 1.0 + HEIGHT_SPREAD) as f32;
            let cover = params.cover * rng.range(0.85, 1.15) as f32;
            let quarter = (rng.unit() * 4.0) as u32 as f32 * FRAC_PI_2 as f32;
            if !layout.bounds.contains(slot.position) {
                continue;
            }
            // A cereal's drill lines run with the alley; a sward tile turns
            // by quarters so the grid does not read as one.
            let turn = if kind == Kind::Cereal { 0.0 } else { quarter };
            let position = ground.lift(slot.position);
            let normal = ground.normal(position.x, position.y);
            order += 1;
            commands.spawn((
                Name::new(format!("Tile_{:04}", slot.index)),
                Transform {
                    translation: position,
                    rotation: Quat::from_rotation_arc(Vec3::Z, normal)
                        * Quat::from_rotation_z(yaw + turn),
                    scale: Vec3::ONE,
                },
                Visibility::default(),
                TileConfig::new(kind, height, cover, params.dryness, params.detail),
                Order(order),
                ChildOf(group),
            ));
        }
    }
}

// ─── Building ───────────────────────────────────────────────────────

/// Builds one mesh per distinct tile and gives it to every tile that drew it.
///
/// A tile has nothing hanging off it, so it carries its geometry directly and
/// expands into nothing.
pub(crate) fn build(
    mut commands: Commands,
    mut library: Library,
    params: Res<CoverParams>,
    tiles: Query<(Entity, &Order, &TileConfig)>,
) -> Result<()> {
    library.clear(PART);

    let mut laid: Vec<(Order, Entity, TileConfig)> = tiles
        .iter()
        .map(|(entity, order, config)| (*order, entity, *config))
        .collect();
    laid.sort_by_key(|(order, ..)| *order);

    let configs: Vec<TileConfig> = laid.iter().map(|(_, _, config)| *config).collect();
    let book = farthest_first(
        &configs,
        params.variations.max(1) as usize,
        0.0,
        &TileMetric,
    );

    let meshes = par_map(&book.representatives, |index, config| {
        tile_mesh(config, index as u64)
            .map(|mesh| mesh.to_mesh())
            .with_context(|| format!("cover tile {index} could not be built"))
    });

    let mut geometry: Vec<Geometry> = Vec::with_capacity(book.len());
    for (index, (config, mesh)) in book.representatives.iter().zip(meshes).enumerate() {
        geometry.push(library.part(PART, index, mesh?, surface(config.dryness, index as u64)));
    }
    for ((_, entity, _), drew) in laid.iter().zip(&book.assignment) {
        commands
            .entity(*entity)
            .insert(geometry[*drew as usize].clone());
    }
    Ok(())
}

/// One tile, in its own frame: centred on the origin, `+X` along the alley,
/// blades standing up `+Z` from `z = 0`.
fn tile_mesh(config: &TileConfig, index: u64) -> anyhow::Result<MeshData> {
    let kind = Kind::ALL[config.kind as usize];
    let mut rng = Rng::new(BLADE_STREAM ^ salt(index));
    let tile = Band {
        start: Vec2::new(-TILE / 2.0, 0.0),
        end: Vec2::new(TILE / 2.0, 0.0),
        half_width: TILE / 2.0,
    };
    match kind {
        Kind::Cereal => cereal_tile(config, &tile, &mut rng),
        _ => sward_tile(config, kind, &tile, &mut rng),
    }
}

/// A patch of grass: [`BLADE_VARIANTS`] blades, each instanced across the
/// tile at its own yaw, lean and size.
///
/// Draws, in order: per variant its length and droop; then per blade slot,
/// after the grid's own two, which variant, its yaw, its lean and its size.
fn sward_tile(
    config: &TileConfig,
    kind: Kind,
    tile: &Band,
    rng: &mut Rng,
) -> anyhow::Result<MeshData> {
    // A sown sward is one height; one that came up on its own is not.
    let shortest = if kind == Kind::Spontaneous { 0.5 } else { 0.85 };
    let variants: Vec<MeshData> = (0..BLADE_VARIANTS)
        .map(|_| {
            let length = config.height as f64 * rng.range(shortest, 1.0);
            let droop = rng.range(0.3, 1.2);
            shapes::blade_mesh(length, BLADE_WIDTH, droop, KEEL, BLADE_TRIANGLES)
        })
        .collect::<anyhow::Result<_>>()?;

    let per_square_metre = (config.detail as f32 * config.cover).max(1.0);
    let spacing = (1.0 / per_square_metre).sqrt();
    let slots = jittered_grid(tile, spacing, 0.5, rng);

    let mut parts = Vec::with_capacity(slots.len());
    for slot in slots {
        let variant = (rng.unit() * BLADE_VARIANTS as f64) as usize;
        let yaw = rng.unit() * TAU;
        let lean = rng.range(0.0, LEAN);
        let size = rng.range(0.8, 1.2);
        parts.push(variants[variant].transformed(&Transform {
            translation: slot.position.extend(0.0),
            rotation: shapes::standing(yaw, lean),
            scale: Vec3::splat(size as f32),
        }));
    }
    Ok(merge_meshes(&parts))
}

/// A drilled cereal: plants in lines across the tile every
/// [`DRILL_SPACING`], each a culm — once the crop has jointed — with
/// [`CEREAL_LEAVES`] leaves off it.
///
/// Draws, in order: per variant its length and droop; then per plant its
/// keep, its two nudges, its height, its lean, its yaw, its culm wobble, and
/// per leaf which variant, its turn and its pitch.
fn cereal_tile(config: &TileConfig, tile: &Band, rng: &mut Rng) -> anyhow::Result<MeshData> {
    let height = config.height as f64;
    let variants: Vec<MeshData> = (0..BLADE_VARIANTS)
        .map(|_| {
            let length = (height * rng.range(0.25, 0.45)).clamp(0.02, CEREAL_LEAF_MAX);
            let droop = rng.range(0.4, 1.0);
            shapes::blade_mesh(length, BLADE_WIDTH, droop, KEEL, BLADE_TRIANGLES)
        })
        .collect::<anyhow::Result<_>>()?;

    let lines = ((2.0 * tile.half_width / DRILL_SPACING).round() as usize).max(1);
    let per_line = ((tile.length() / DRILL_STEP).round() as usize).max(1);
    let mut parts = Vec::new();
    for line in 0..lines {
        let y = tile.half_width * (2.0 * (line as f32 + 0.5) / lines as f32 - 1.0);
        for i in 0..per_line {
            let keep = rng.unit() < config.cover as f64;
            let nudge_x = rng.range(-0.4, 0.4) as f32 * DRILL_STEP;
            let nudge_y = rng.range(-0.15, 0.15) as f32 * DRILL_SPACING;
            let h = height * rng.range(0.8, 1.1);
            let lean = rng.range(0.0, 0.1);
            let yaw = rng.unit() * TAU;
            let wobble = rng.range(0.0, TAU);
            if !keep {
                continue;
            }
            let x = tile.start.x + tile.length() * (i as f32 + 0.5) / per_line as f32 + nudge_x;
            let plant = Transform {
                translation: Vec3::new(x, y + nudge_y, 0.0),
                rotation: Quat::from_rotation_z(yaw as f32) * Quat::from_rotation_x(lean as f32),
                scale: Vec3::ONE,
            };

            let jointed = h >= CULM_MIN_HEIGHT;
            if jointed {
                // Bowed by a few millimetres: a straight polyline has no
                // curvature for the tube's frames to follow.
                let bow = 0.004 * h;
                let culm = Strand::new(
                    vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(bow * wobble.cos(), bow * wobble.sin(), h / 2.0),
                        Point3::new(0.0, 0.0, h),
                    ],
                    vec![CULM_RADIUS, CULM_RADIUS * 0.9, CULM_RADIUS * 0.6],
                    5,
                    4.0,
                    Bark::none(),
                );
                parts.push(strand_mesh(&culm)?.transformed(&plant));
            }
            for leaf in 0..CEREAL_LEAVES {
                let variant = (rng.unit() * BLADE_VARIANTS as f64) as usize;
                let turn = rng.unit() * TAU;
                let pitch = rng.range(0.4, 0.9);
                let at = if jointed {
                    h * (0.2 + 0.25 * leaf as f64)
                } else {
                    0.0
                };
                let node = Transform {
                    translation: Vec3::new(0.0, 0.0, at as f32),
                    rotation: shapes::standing(turn, pitch),
                    scale: Vec3::ONE,
                };
                parts.push(variants[variant].transformed(&(plant * node)));
            }
        }
    }
    Ok(merge_meshes(&parts))
}

/// A tile's blades, shaded per representative between green and straw.
fn surface(dryness: f32, index: u64) -> Surface {
    let base = color::mix(
        color::srgb(color::SWARD),
        color::srgb(color::STRAW),
        dryness,
    );
    material::FOLIAGE.blade(color::shade(
        base,
        &mut Rng::new(color::COLOR_STREAM ^ SWARD_SHADE ^ salt(index)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::VineyardParams;
    use crate::elements::util::testing::{grown, named_children, organs, prim};
    use crate::scene::Prototypes;

    /// A parcel small enough to grow in a test and wide enough for three rows.
    fn small() -> VineyardParams {
        let mut params = VineyardParams::default();
        params.terrain.length = 30.0;
        params.terrain.width = 20.0;
        params
    }

    fn part_count(app: &App, prefix: &str) -> usize {
        app.world()
            .resource::<Prototypes>()
            .iter()
            .filter(|(name, _)| name.starts_with(&format!("{prefix}_")))
            .count()
    }

    #[test]
    fn tiles_sit_on_the_ground_inside_their_alley_and_follow_its_slope() {
        let mut app = grown(small());
        let ground = app.world().resource::<Ground>().clone();
        let layout = app.world().resource::<VineyardLayout>().clone();
        let tiles = organs::<TileConfig>(app.world_mut());
        assert!(tiles.len() > 100, "got {} tiles", tiles.len());

        let alleys = layout.alleys();
        for tile in &tiles {
            let p = tile.position();
            assert!(
                (p.z - ground.height(p.x, p.y)).abs() < 1e-3,
                "{} floats {} m off the ground",
                tile.path,
                p.z - ground.height(p.x, p.y)
            );
            assert!(
                alleys.iter().any(|a| a.band.contains(p.truncate())),
                "{} lies outside every alley",
                tile.path
            );
            assert!(
                tile.up().dot(ground.normal(p.x, p.y)) > 0.999,
                "{} does not follow the ground",
                tile.path
            );
        }
    }

    #[test]
    fn alternate_leaves_every_second_alley_bare() {
        let mut params = small();
        params.cover.alternate = true;
        let mut app = grown(params);
        let cover = prim(app.world_mut(), &["Cover"]).expect("a Cover subtree");
        let alleys = named_children(app.world_mut(), cover);
        assert!(!alleys.is_empty());
        for (name, _) in &alleys {
            let index: usize = name["Alley_".len()..].parse().unwrap();
            assert_eq!(index % 2, 0, "{name} should be bare");
        }
    }

    #[test]
    fn none_grows_nothing() {
        let mut params = small();
        params.cover.kind = "none".into();
        let mut app = grown(params);
        assert!(prim(app.world_mut(), &["Cover"]).is_none());
        assert_eq!(part_count(&app, PART), 0);
    }

    #[test]
    fn an_unknown_kind_falls_back_without_panicking() {
        let mut params = small();
        params.cover.kind = "kudzu".into();
        let mut app = grown(params);
        assert!(!organs::<TileConfig>(app.world_mut()).is_empty());
    }

    #[test]
    fn a_narrower_band_lays_fewer_tiles() {
        let mut wide = small();
        wide.cover.width = 1.0;
        let mut narrow = small();
        narrow.cover.width = 0.5;
        let wide = organs::<TileConfig>(grown(wide).world_mut()).len();
        let narrow = organs::<TileConfig>(grown(narrow).world_mut()).len();
        assert!(narrow < wide, "{narrow} vs {wide}");
    }

    #[test]
    fn the_budget_caps_the_tile_meshes() {
        let mut params = small();
        params.cover.variations = 3;
        let app = grown(params);
        assert!(part_count(&app, PART) <= 3);
    }

    #[test]
    fn cereal_tiles_run_with_the_alley() {
        let mut params = small();
        params.cover.kind = "cereal".into();
        params.cover.height = 0.8;
        let mut app = grown(params);
        let layout = app.world().resource::<VineyardLayout>().clone();
        let along = layout.rows[0].direction();
        for tile in organs::<TileConfig>(app.world_mut()) {
            let x = (tile.transform.rotation * Vec3::X).truncate().normalize();
            assert!(
                x.dot(along).abs() > 0.99,
                "{} is turned off the alley",
                tile.path
            );
        }
    }

    /// The whole layer is a function of the params: same seed, same tiles,
    /// same meshes.
    #[test]
    fn the_cover_is_deterministic() {
        let a = organs::<TileConfig>(grown(small()).world_mut());
        let b = organs::<TileConfig>(grown(small()).world_mut());
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.path, y.path);
            assert_eq!(x.transform, y.transform);
            assert_eq!(x.config, y.config);
        }
    }

    #[test]
    fn a_tile_stands_on_its_own_ground_and_reaches_about_its_height() {
        let config = TileConfig::new(Kind::Sown, 0.2, 0.7, 0.0, 600);
        let mesh = tile_mesh(&config, 0).unwrap();
        let (lo, hi) = crate::elements::util::testing::bounds(&mesh, 2);
        assert!(lo > -0.01, "nothing below the ground: {lo}");
        assert!(hi > 0.12 && hi <= 0.2 * 1.2 + 1e-3, "tallest blade at {hi}");
        let (x0, x1) = crate::elements::util::testing::bounds(&mesh, 0);
        assert!(x0 > -0.5 && x1 < 0.5, "blades stay about the tile");

        let cereal = TileConfig::new(Kind::Cereal, 0.8, 0.9, 0.0, 600);
        let mesh = tile_mesh(&cereal, 1).unwrap();
        let (_, hi) = crate::elements::util::testing::bounds(&mesh, 2);
        assert!(hi > 0.6, "a jointed cereal stands on its culms: {hi}");
    }
}
