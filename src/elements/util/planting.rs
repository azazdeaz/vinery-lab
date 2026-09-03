//! Planting — what actually stands on the ground, and where.
//!
//! [`parcel`](super::parcel) solves *where* things go; this authors *what* is
//! there. It walks the solved [`VineyardLayout`], draping it onto [`Ground`],
//! and spawns one entity per vine and per post carrying that organ's config.
//!
//! Terrain's placement helper, exactly as [`parcel`] is its layout helper: it
//! owns no element identity of its own and is wired from [`terrain::plugin`]
//! rather than [`elements::plugin`]. What it does own is the `Planting`
//! subtree — so `terrain` is an element with two subtrees, its surface and
//! everything planted on it.
//!
//! # Context becomes config here
//!
//! The layers below turn distinct configs into meshes and know nothing about
//! rows, terrain or age. So anything that depends on *where* a plant stands has
//! to be decided in this module and written into its config — a plant whose
//! shape depended on context the builder cannot see would silently diverge from
//! the mesh it shares.
//!
//! # Nothing is planted exactly where it was solved
//!
//! The layout is a set of straight lines at exact spacings, and a vineyard
//! planted to it looks stamped out. So each thing placed here is nudged off the
//! position it was solved for — see [`row_poles`] for the three ways a post is,
//! and why they are drawn from a stream of their own.
//!
//! # Path stability
//!
//! `Row_000/Vine_007` names the *eighth planting slot of the first row*, not
//! the eighth vine that happened to be planted. A slot skipped by
//! [`PlantingParams::miss_rate`] leaves a gap in the numbering rather than
//! shifting every name after it, so nudging the miss rate can't silently
//! repoint a config keyed on a path at a different plant.
//!
//! [`parcel`]: super::parcel
//! [`terrain::plugin`]: crate::elements::terrain::plugin
//! [`elements::plugin`]: crate::elements::plugin

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};

use crate::elements::Rng;
use crate::elements::pole;
use crate::elements::terrain::Ground;
use crate::elements::vine;

use crate::elements::SceneParams;
use crate::scene::{Order, PrimRoot, UsdType, placed};

use super::parcel::{ParcelParams, Row, VineyardLayout};

/// The prim this module owns under the scene root, and rewrites from scratch.
pub const PLANTING: &str = "Planting";

/// Marks the planting subtree, so a rebuild can drop the last one.
#[derive(Component)]
pub struct Planting;

/// Salt splitting the post placements off the vine stream, so that nudging one
/// never re-rolls the other. An arbitrary odd constant; only its fixedness
/// matters — the same split [`vine`] keeps between its wood and its shoots.
///
/// [`vine`]: crate::elements::vine
const POLE_STREAM: u64 = 0x2545_F491_4F6C_DD1D;

/// The same, for the plants. Salted rather than drawing off the scene seed
/// directly, so that this module's stream and the mesh streams the layers below
/// draw from never coincide.
const VINE_STREAM: u64 = 0xC2B2_AE3D_27D4_EB4F;

/// How far a post's foot may end up from the position the layout solved, in
/// meters, along the row and across it. A post is driven by a machine walking
/// the row against a string line, not surveyed onto a point.
const POLE_OFFSET: f64 = 0.02;

/// How far a post may stand off plumb, in radians — a bit over a degree. It
/// leans about its own foot, so the top of a trellis-height post moves by
/// something under four centimeters.
const POLE_TILT: f64 = 0.02;

/// How much deeper than nominal a post may have been driven, in meters.
///
/// Deeper only, and that is the point: a pole prototype stands from the ground
/// up, so sinking one takes the same few centimeters off the *top*. What the
/// draw actually buys is a row whose post tops don't sit on one perfect line,
/// which is the thing that reads as a real vineyard from a distance at which
/// nothing else here is visible.
const POLE_SINK: f64 = 0.05;

#[derive(Resource, Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct PlantingParams {
    /// Fraction of planting positions left empty. Real vineyards have gaps,
    /// and a perception model trained without them learns that they can't
    /// happen.
    pub miss_rate: f32,
    /// Fraction of vines that are recent replants rather than mature.
    ///
    /// A replant is planted from [`vine::YOUNG_PROTOTYPE`] rather than from a
    /// shrunk mature vine: in its first season it is one green shoot out of
    /// the ground, and a scaled-down trunk-and-cordons is the one thing it is
    /// certainly not.
    pub young_rate: f32,
    /// How small the youngest replant is, relative to a full-grown shoot.
    pub young_scale: f32,
}

impl Default for PlantingParams {
    fn default() -> Self {
        Self {
            miss_rate: 0.03,
            young_rate: 0.08,
            young_scale: 0.55,
        }
    }
}

/// Authors a config for every plant and post the layout calls for, and places
/// it on the ground.
///
/// This is where context becomes config: the layer below turns distinct
/// configs into meshes and knows nothing about rows, terrain or age. Anything
/// that depends on *where* a plant stands has to be decided here, or two plants
/// sharing a mesh would silently diverge.
pub fn plant(
    mut commands: Commands,
    scene: Res<SceneParams>,
    params: Res<PlantingParams>,
    parcel: Res<ParcelParams>,
    vine_params: Res<vine::VineParams>,
    pole_params: Res<pole::PoleParams>,
    layout: Res<VineyardLayout>,
    ground: Res<Ground>,
    root: Res<PrimRoot>,
    existing: Query<Entity, With<Planting>>,
) {
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let planting = commands
        .spawn((
            Planting,
            Name::new(PLANTING),
            Transform::IDENTITY,
            Visibility::default(),
            ChildOf(root.0),
        ))
        .id();

    let mut rng = Rng::new(scene.seed ^ VINE_STREAM);
    // Its own stream, so a nudge to the vines never moves a post — see
    // [`POLE_STREAM`].
    let mut pole_rng = Rng::new(scene.seed ^ POLE_STREAM);
    let mut order = 0u64;
    let mut next = || {
        order += 1;
        Order(order)
    };

    for (index, row) in layout.rows.iter().enumerate() {
        // A Scope, not an Xform: a row carries no transform of its own — its
        // plants are already placed in scene space, draped individually onto
        // terrain that a single row transform could not follow.
        let group = commands
            .spawn((
                Name::new(format!("Row_{index:03}")),
                UsdType("Scope"),
                Transform::IDENTITY,
                Visibility::default(),
                ChildOf(planting),
            ))
            .id();

        for (name, transform, established, vigour) in row_vines(row, &ground, &params, &mut rng) {
            commands.spawn((
                Name::new(name),
                transform,
                Visibility::default(),
                vine::VineConfig::new(&vine_params, &parcel, established, vigour),
                next(),
                ChildOf(group),
            ));
        }
        for (name, transform) in row_poles(row, &ground, &mut pole_rng) {
            commands.spawn((
                Name::new(name),
                transform,
                Visibility::default(),
                pole::PoleConfig::new(&pole_params, &parcel),
                next(),
                ChildOf(group),
            ));
        }
    }
}

/// The plants of one row, named by planting slot.
///
/// The *draw order* of `rng` is part of this module's output: all three draws
/// happen before the miss test, so the stream stays aligned no matter which
/// slots are skipped. Rolling them lazily instead would make every vine past
/// the first change re-roll whenever `miss_rate` was nudged.
fn row_vines(
    row: &Row,
    ground: &Ground,
    params: &PlantingParams,
    rng: &mut Rng,
) -> Vec<(String, Transform, f32, f32)> {
    let yaw = row.direction().to_angle();
    let mut plants = Vec::new();
    for (slot, position) in row.vine_positions(ground).enumerate() {
        let (miss, age, vigour) = (
            rng.unit(),
            rng.unit(),
            rng.range(1.0 - vine::VINE_VIGOUR, 1.0 + vine::VINE_VIGOUR),
        );
        if miss < params.miss_rate as f64 {
            continue;
        }
        // A plant stands upright and the row's direction is the whole rotation;
        // a lean is for nested placement, not for something out of the ground.
        plants.push((
            format!("Vine_{slot:03}"),
            placed(position, yaw, Vec2::ZERO, 1.0),
            young_scale(params, age).unwrap_or(1.0) as f32,
            vigour as f32,
        ));
    }
    plants
}

/// The posts of one row, named by post slot.
///
/// Every post is placed; nothing here is the posts' equivalent of
/// [`PlantingParams::miss_rate`], because a missing post is a broken trellis
/// rather than a dead plant, and the row it was in would be lying on the
/// ground.
///
/// Five draws per post, in this order: the foot's offset along the row and
/// across it, the lean about each of the post's own horizontal axes, and how
/// deep it was driven. All five are taken for every post, so the stream stays
/// aligned however the parcel is re-solved around it.
///
/// The foot is re-draped after being offset rather than keeping the height it
/// was solved at. The offset is two centimeters and the correction is under a
/// millimeter on anything this terrain generates — but "a post sits on the
/// ground" is worth being exactly true rather than nearly, since it is what
/// every check of this placement rests on.
fn row_poles(row: &Row, ground: &Ground, rng: &mut Rng) -> Vec<(String, Transform)> {
    let (along, across) = (row.direction(), row.direction().perp());
    let yaw = row.direction().to_angle();
    let mut posts = Vec::new();
    for (slot, position) in row.post_positions(ground).enumerate() {
        let offset = along * rng.range(-POLE_OFFSET, POLE_OFFSET) as f32
            + across * rng.range(-POLE_OFFSET, POLE_OFFSET) as f32;
        let tilt = Vec2::new(
            rng.range(-POLE_TILT, POLE_TILT) as f32,
            rng.range(-POLE_TILT, POLE_TILT) as f32,
        );
        let sink = rng.range(0.0, POLE_SINK) as f32;
        posts.push((
            format!("Pole_{slot:03}"),
            // The yaw changes nothing on a round post. It is the row's all the
            // same: a post that grows a wire notch or a profile has to face
            // along the row, and this is where that is already true.
            placed(
                ground.lift(position.truncate() + offset) - Vec3::Z * sink,
                yaw,
                tilt,
                1.0,
            ),
        ));
    }
    posts
}

/// How established a plant whose age draw came out at `age` is, or `None` if it
/// drew a mature vine.
///
/// A replant is a shoot, and this is how much of a full-grown one it has put
/// out so far. It reaches [`vine::VineConfig::established`], which decides the
/// *shape* built for it rather than a size it is scaled to.
///
/// Ramping across the young band rather than stepping means a replanted
/// stretch of row shows a spread of ages, not one clone repeated.
fn young_scale(params: &PlantingParams, age: f64) -> Option<f64> {
    let young_rate = params.young_rate as f64;
    let young_scale = params.young_scale as f64;
    if young_rate <= 0.0 || age >= young_rate {
        return None;
    }
    Some(young_scale + (1.0 - young_scale) * (age / young_rate))
}

pub fn ui() -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            label_small("Missing vines"),
            (
                @FeathersSlider { @min: 0.0, @max: 0.3, @value: 0.03 }
                SliderStep(0.01)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<PlantingParams>| {
                    params.miss_rate = change.value;
                })
            ),
            label_small("Young vines"),
            (
                @FeathersSlider { @min: 0.0, @max: 0.5, @value: 0.08 }
                SliderStep(0.01)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<PlantingParams>| {
                    params.young_rate = change.value;
                })
            ),
            label_small("Young vine scale"),
            (
                @FeathersSlider { @min: 0.2, @max: 1.0, @value: 0.55 }
                SliderStep(0.05)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<PlantingParams>| {
                    params.young_scale = change.value;
                })
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::SQRT_2;

    use super::*;
    use crate::elements::VineyardParams;
    use crate::elements::util::testing::{self, Organ, named_children, organs, prim};

    /// A parcel with every slot planted, so counts line up with the layout.
    fn no_misses() -> VineyardParams {
        VineyardParams {
            planting: PlantingParams {
                miss_rate: 0.0,
                ..default()
            },
            ..default()
        }
    }

    fn scene(params: VineyardParams) -> App {
        testing::grown(params)
    }

    fn vines(app: &mut App) -> Vec<Organ<vine::VineConfig>> {
        organs(app.world_mut())
    }

    fn poles(app: &mut App) -> Vec<Organ<pole::PoleConfig>> {
        organs(app.world_mut())
    }

    /// Where the layout put the posts, before this module nudged them.
    fn solved_posts(app: &App) -> Vec<Vec3> {
        let ground = app.world().resource::<Ground>();
        app.world()
            .resource::<VineyardLayout>()
            .rows
            .iter()
            .flat_map(|row| row.post_positions(ground))
            .collect()
    }

    /// Every plant and post is addressable by the slot it stands in — which is
    /// what an Isaac Lab config keys on.
    #[test]
    fn every_plant_and_post_has_its_own_prim_path() {
        let mut app = scene(no_misses());
        let row = prim(app.world_mut(), &["Planting", "Row_000"]).expect("the first row");
        let names: Vec<String> = named_children(app.world_mut(), row)
            .into_iter()
            .map(|(name, _)| name)
            .collect();

        assert!(names.contains(&"Vine_000".to_string()), "got {names:?}");
        assert!(names.contains(&"Pole_000".to_string()), "got {names:?}");
        assert_eq!(
            names.iter().collect::<std::collections::HashSet<_>>().len(),
            names.len(),
            "no two things in a row share a name"
        );
    }

    #[test]
    fn one_vine_is_planted_per_slot_in_the_layout() {
        let mut app = scene(no_misses());
        let ground = app.world().resource::<Ground>().clone();
        let slots: usize = app
            .world()
            .resource::<VineyardLayout>()
            .rows
            .iter()
            .map(|row| row.vine_positions(&ground).count())
            .sum();

        assert!(slots > 20, "the fixture planted a parcel");
        assert_eq!(vines(&mut app).len(), slots);
    }

    /// `Vine_007` has to keep meaning "the eighth slot of this row" whatever
    /// the miss rate is, or an Isaac Lab config keyed on a path silently
    /// repoints at a different plant when the rate is nudged.
    #[test]
    fn a_vines_name_marks_its_slot_not_its_rank() {
        let mut planted = scene(no_misses());
        let mut sparse = scene(VineyardParams {
            planting: PlantingParams {
                miss_rate: 0.5,
                ..default()
            },
            ..default()
        });

        // Keyed on the path: `Vine_007` repeats in every row.
        let at = |app: &mut App| -> Vec<(String, Vec3)> {
            vines(app)
                .into_iter()
                .map(|vine| (vine.path.clone(), vine.position()))
                .collect()
        };
        let (full, thinned) = (at(&mut planted), at(&mut sparse));
        assert!(thinned.len() < full.len(), "some slots were missed");

        // Every surviving name still stands exactly where it did.
        for (name, position) in &thinned {
            let same = full
                .iter()
                .find(|(other, _)| other == name)
                .unwrap_or_else(|| panic!("`{name}` is a slot the full parcel also planted"));
            assert!((same.1 - *position).length() < 1e-6, "`{name}` moved");
        }
    }

    /// Every vine sits on the terrain, not floating above or sunk into it.
    #[test]
    fn a_planted_vine_sits_on_the_ground() {
        let mut app = scene(no_misses());
        let ground = app.world().resource::<Ground>().clone();
        for vine in vines(&mut app) {
            let at = vine.position();
            assert!(
                (at.z - ground.height(at.x, at.y)).abs() < 1e-4,
                "`{}` stands at {at:?}, {} m off the ground",
                vine.name,
                at.z - ground.height(at.x, at.y)
            );
        }
    }

    /// Checked through the composed rotation rather than a yaw value, so what
    /// is under test is where the plant actually ends up pointing.
    #[test]
    fn every_vine_faces_along_its_row() {
        let mut app = scene(no_misses());
        let along = app.world().resource::<VineyardLayout>().rows[0].direction();
        for vine in vines(&mut app) {
            // A vine is authored running +X along the row.
            let facing = (vine.transform.rotation * Vec3::X).truncate();
            assert!(
                facing.angle_to(along).abs() < 1e-4,
                "`{}` faces {facing:?}, the row runs {along:?}",
                vine.name
            );
            assert!(
                (vine.up() - Vec3::Z).length() < 1e-6,
                "and stands upright, not leaning"
            );
        }
    }

    /// A replant is its own plant — a shoot out of bare ground — so the age
    /// draw reaches the *shape* through `established` rather than shrinking a
    /// mature vine.
    #[test]
    fn a_replant_is_less_established_than_a_grown_vine() {
        let mut app = scene(VineyardParams {
            planting: PlantingParams {
                miss_rate: 0.0,
                young_rate: 0.4,
                ..default()
            },
            ..default()
        });
        let planted = vines(&mut app);
        let (young, grown): (Vec<_>, Vec<_>) =
            planted.iter().partition(|v| !v.config.is_mature());

        assert!(!young.is_empty(), "some slots drew a replant");
        assert!(!grown.is_empty(), "and most did not");
        for vine in &grown {
            assert_eq!(vine.config.established, 1.0);
            assert!(
                (vine.transform.scale - Vec3::ONE).length() < 1e-6,
                "a mature vine stands at full size"
            );
        }
        for vine in &young {
            assert!(
                (0.0..1.0).contains(&vine.config.established),
                "a replant is partly grown, got {}",
                vine.config.established
            );
        }
    }

    /// Turning the rate off leaves a parcel of nothing but mature vines.
    #[test]
    fn no_young_rate_plants_no_replants() {
        let mut app = scene(VineyardParams {
            planting: PlantingParams {
                miss_rate: 0.0,
                young_rate: 0.0,
                ..default()
            },
            ..default()
        });
        assert!(vines(&mut app).iter().all(|v| v.config.is_mature()));
    }

    /// A replant is still growing, so it stands somewhere across the age band
    /// rather than all replants being one clone repeated.
    #[test]
    fn replants_spread_across_the_age_band() {
        let p = PlantingParams::default();
        assert_eq!(young_scale(&p, 0.5), None, "a mature vine is not a replant");
        assert!((young_scale(&p, 0.0).unwrap() - p.young_scale as f64).abs() < 1e-9);
        assert!(young_scale(&p, 0.04).unwrap() > p.young_scale as f64);

        let mut app = scene(VineyardParams {
            planting: PlantingParams {
                miss_rate: 0.0,
                young_rate: 0.3,
                ..default()
            },
            ..default()
        });
        let established: Vec<f32> = vines(&mut app)
            .iter()
            .map(|v| v.config.established)
            .filter(|e| *e < 1.0)
            .collect();

        assert!(!established.is_empty(), "some vines are young");
        assert!(
            established.iter().any(|e| *e < 0.999),
            "and the youngest of them are the least grown"
        );
        for e in &established {
            assert!(
                *e >= p.young_scale - 1e-6,
                "inside the age band, got {e}"
            );
        }
    }

    /// A post at every position the layout solved for one. Unlike a vine, a
    /// post is never skipped.
    #[test]
    fn a_pole_stands_at_every_solved_post_position() {
        let mut app = scene(no_misses());
        let solved = solved_posts(&app);
        assert!(solved.len() > 20, "the fixture solved posts");
        assert_eq!(poles(&mut app).len(), solved.len());
    }

    /// Every post is off the line the solver drew — but only just. Both
    /// directions matter: a post that drifted far enough would be standing in
    /// the row rather than in the trellis, and one that didn't drift at all
    /// leaves the parcel looking stamped out.
    #[test]
    fn poles_are_nudged_off_the_layout_without_leaving_it() {
        let mut app = scene(no_misses());
        let ground = app.world().resource::<Ground>().clone();
        let solved = solved_posts(&app);

        let (mut offsets, mut sinks, mut leans) = (Vec::new(), Vec::new(), Vec::new());
        for post in poles(&mut app) {
            let at = post.position();
            offsets.push(
                solved
                    .iter()
                    .map(|s| s.truncate().distance(at.truncate()))
                    .fold(f32::MAX, f32::min),
            );
            sinks.push(ground.height(at.x, at.y) - at.z);
            leans.push(post.up().angle_between(Vec3::Z));
        }
        let spread = |v: &[f32]| {
            v.iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(*x), hi.max(*x)))
        };

        // A foot may move in both directions at once, so the two draws compose
        // to a diagonal.
        let (_, furthest) = spread(&offsets);
        assert!(
            furthest <= (POLE_OFFSET * SQRT_2) as f32 + 1e-4,
            "a post landed {furthest} m off the layout"
        );
        assert!(furthest > 0.005, "the posts are all on the line");

        let (shallowest, deepest) = spread(&sinks);
        assert!(
            (-1e-4..=POLE_SINK as f32 + 1e-4).contains(&shallowest)
                && deepest <= POLE_SINK as f32 + 1e-4,
            "driven {shallowest}..{deepest} m into the ground"
        );
        assert!(
            deepest - shallowest > POLE_SINK as f32 / 2.0,
            "every post was driven to the same depth"
        );

        let (_, worst) = spread(&leans);
        assert!(
            worst <= (POLE_TILT * SQRT_2) as f32 + 1e-3,
            "a post leans {worst} rad off plumb"
        );
        assert!(worst > 0.005, "every post is dead plumb");
    }

    /// The posts draw from a stream of their own, so re-rolling the planting
    /// seed moves both — but changing what the *vines* do must leave the
    /// trellis exactly where it was.
    #[test]
    fn the_trellis_does_not_move_when_the_planting_around_it_changes() {
        let at = |miss_rate: f32| {
            let mut app = scene(VineyardParams {
                planting: PlantingParams {
                    miss_rate,
                    ..default()
                },
                ..default()
            });
            poles(&mut app)
                .iter()
                .map(Organ::position)
                .collect::<Vec<_>>()
        };
        assert_eq!(at(0.0), at(0.4));
    }

    /// Planting owns its subtree and rewrites it from scratch, so shrinking the
    /// layout must not leave the rows that no longer exist behind.
    #[test]
    fn re_authoring_drops_rows_that_no_longer_exist() {
        let mut app = scene(no_misses());
        let rows = |app: &mut App| {
            let planting = prim(app.world_mut(), &["Planting"]).expect("the planting subtree");
            named_children(app.world_mut(), planting).len()
        };
        let before = rows(&mut app);
        assert!(before > 2);

        app.world_mut().resource_mut::<ParcelParams>().row_spacing = 12.0;
        app.update();
        assert!(rows(&mut app) < before, "stale rows removed on re-author");
    }
}
