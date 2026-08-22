//! Planting — what actually stands on the ground, and where.
//!
//! [`parcel`](super::parcel) solves *where* things go; this authors *what* is
//! there. It walks the solved [`VineyardLayout`], draping it onto
//! [`Ground`], and places one prim per vine and per post through
//! [`place`](super::place).
//!
//! Terrain's placement helper, exactly the standing [`parcel`] has as its
//! layout helper: it owns no element identity, has no `/parts` prototypes of
//! its own, and is wired from [`terrain::plugin`] rather than
//! [`elements::plugin`]. What it does own is the `/Vineyard/Planting`
//! subtree — so `terrain` is an element with two subtrees, its surface and
//! everything planted on it.
//!
//! # Nothing is planted exactly where it was solved
//!
//! The layout is a set of straight lines at exact spacings, and a vineyard
//! planted to it looks stamped out. So each thing placed here is nudged off
//! the position it was solved for — see [`row_poles`] for the three ways a
//! post is, and why they are drawn from a stream of their own.
//!
//! # Why every plant gets its own prim
//!
//! A `PointInstancer` is one prim holding parallel arrays, so no individual
//! vine has a prim path. Isaac Lab needs one: to attach a semantic label for
//! perception training, to bind a rigid body, or to randomize a single plant.
//! So vines are placed by reference — see [`place::place_referenced`] — and
//! the instancer is kept for scatter that is only ever seen in aggregate.
//!
//! The prices, both accepted deliberately: the stage gains a prim per plant
//! rather than one per kind, and re-rolling a variation rewrites composition
//! metadata instead of patching one `protoIndices` array.
//!
//! Which is why that is the *export* shape and not the only one. Nothing in
//! the viewer addresses an individual plant, and those prices are paid again
//! on every frame a slider moves, so the viewer sets
//! [`place::Style::Instanced`] and gets one `PointInstancer` per row instead —
//! see [`place`](super::place). This module authors whichever it is told to.
//!
//! # Path stability
//!
//! `Row_00/Vine_007` names the *eighth planting slot of the first row*, not
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
use usd_bevy::authoring::{define_prim, remove_prim};
use usd_bevy::live::LiveStage;

use crate::elements::Rng;
use crate::elements::pole;
use crate::elements::terrain::Ground;
use crate::elements::vine;

use super::parcel::{Row, VineyardLayout};
use super::place::{self, Placement};

/// The subtree this module owns and rewrites from scratch.
pub const PLANTING: &str = "/Vineyard/Planting";

/// Name the `PointInstancer` holding a row's vines takes, when the vines are
/// instanced rather than reference-placed. A row is a `Scope` either way.
pub const VINES: &str = "Vines";

/// The same, for the row's young vines. A third batch for the same reason the
/// posts are a second one: a batch draws from one prototype library, and a
/// replant is not a variation of a mature vine — it is a shoot out of the bare
/// ground, with no wood on it at all.
pub const YOUNG: &str = "Young";

/// The same, for the row's posts. They are a second batch rather than part of
/// the first: a batch draws from one prototype library, and a post is not a
/// variation of a vine.
pub const POLES: &str = "Poles";

/// Salt splitting the post placements off the vine stream, so that nudging one
/// never re-rolls the other. An arbitrary odd constant; only its fixedness
/// matters — the same split [`vine`] keeps between its wood and its shoots.
///
/// [`vine`]: crate::elements::vine
const POLE_STREAM: u64 = 0x2545_F491_4F6C_DD1D;

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
    /// Drives which slots are missing, which plants are young, and which
    /// variation each one takes.
    pub seed: u64,
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
            seed: 0,
            miss_rate: 0.03,
            young_rate: 0.08,
            young_scale: 0.55,
        }
    }
}

/// Plants the prototypes along the solved rows.
///
/// Runs in [`Grow::Plants`](crate::elements::Grow::Plants), after
/// [`Grow::Terrain`](crate::elements::Grow::Terrain), so the layout and the
/// ground it is draped on are both current — and after
/// [`Grow::Prototypes`](crate::elements::Grow::Prototypes), so the prototypes
/// this references are already on the stage.
pub fn author(
    live: NonSend<LiveStage>,
    params: Res<PlantingParams>,
    layout: Res<VineyardLayout>,
    ground: Res<Ground>,
    style: Res<place::Style>,
) -> Result<()> {
    let stage = &live.stage;
    remove_prim(stage, PLANTING)?;
    define_prim(stage, PLANTING, "Xform")?;

    let variations = place::prototype_count(stage, vine::PROTOTYPE).max(1);
    // Not `max(1)`, unlike the others: an empty young library is a real
    // state — nothing to plant a replant *from* — and `row_plants` reads the
    // zero as "every slot gets a mature vine" rather than referencing a
    // prototype that was never authored.
    let young = place::prototype_count(stage, vine::YOUNG_PROTOTYPE);
    let poles = place::prototype_count(stage, pole::PROTOTYPE).max(1);
    let mut rng = Rng::new(params.seed);
    // Its own stream, so a nudge to the vines never moves a post — see
    // [`POLE_STREAM`].
    let mut pole_rng = Rng::new(params.seed ^ POLE_STREAM);

    for (index, row) in layout.rows.iter().enumerate() {
        let group = format!("{PLANTING}/Row_{index:03}");
        // A Scope, not an Xform: a row carries no transform of its own — its
        // plants are already placed in stage space, draped individually onto
        // terrain that a single row transform could not follow.
        define_prim(stage, &group, "Scope")?;
        let plants = row_plants(row, &ground, &params, variations, young, &mut rng);
        // One instancer per row rather than one for the whole planting: the
        // row grouping survives either way, and eighteen prims is already flat
        // enough. Collapsing to a single instancer is the next step if it ever
        // isn't.
        // No material: a plant's prototype carries its own, which is the only
        // place a binding survives being referenced onto the ground.
        place::place(
            stage,
            *style,
            &group,
            VINES,
            vine::PROTOTYPE,
            variations,
            &plants.vines,
            None,
        )?;
        // A second batch off a second library, and the reason a young vine
        // costs a placement rather than a scale: the two draw different
        // geometry. Referenced, both land as prims directly under the row, so
        // `Vine_007` is the same path whichever age it came out — which is
        // what keeps a config keyed on it pointing at the same slot when the
        // young rate moves.
        //
        // Skipped entirely when a row drew no replants, rather than authored
        // empty: the instanced path would leave a `PointInstancer` holding
        // nothing but a relationship to a library that may not even exist.
        if !plants.young.is_empty() {
            place::place(
                stage,
                *style,
                &group,
                YOUNG,
                vine::YOUNG_PROTOTYPE,
                young,
                &plants.young,
                None,
            )?;
        }
        place::place(
            stage,
            *style,
            &group,
            POLES,
            pole::PROTOTYPE,
            poles,
            &row_poles(row, &ground, &mut pole_rng),
            None,
        )?;
    }
    Ok(())
}

/// One row's plants, split by the library each is placed from.
///
/// Both halves are named by planting slot and both go under the same row, so
/// the split is about which geometry a plant draws and nothing else.
struct RowPlants {
    /// The mature vines, from [`vine::PROTOTYPE`].
    vines: Vec<(String, Placement)>,
    /// The recent replants, from [`vine::YOUNG_PROTOTYPE`]. Empty when the
    /// young library is, whatever [`PlantingParams::young_rate`] says.
    young: Vec<(String, Placement)>,
}

/// The plants of one row, named by planting slot.
///
/// The *draw order* of `rng` is part of this module's output: all three draws
/// happen before the miss test, so the stream stays aligned no matter which
/// slots are skipped. Rolling them lazily instead would make every vine past
/// the first change re-roll whenever `miss_rate` was nudged. The same reason
/// the age draw is spent whether or not there is a young library to honour it
/// with.
fn row_plants(
    row: &Row,
    ground: &Ground,
    params: &PlantingParams,
    variations: usize,
    young_variations: usize,
    rng: &mut Rng,
) -> RowPlants {
    let yaw = row.direction().to_angle();
    let mut plants = RowPlants {
        vines: Vec::new(),
        young: Vec::new(),
    };
    for (slot, position) in row.vine_positions(ground).enumerate() {
        let (miss, age, pick) = (rng.unit(), rng.unit(), rng.unit());
        if miss < params.miss_rate as f64 {
            continue;
        }
        let name = format!("Vine_{slot:03}");
        // A plant stands upright and the row's direction is the whole
        // rotation, whichever library it comes from. Tilt is for nested
        // placement — see `Placement`.
        let placement = |scale: f64, variations: usize| Placement {
            position,
            yaw,
            tilt: Vec2::ZERO,
            scale: scale as f32,
            variation: (pick * variations as f64) as usize % variations,
        };
        match young_scale(params, age) {
            Some(scale) if young_variations > 0 => plants
                .young
                .push((name, placement(scale, young_variations))),
            // Full size and full grown: a mature vine is the only thing the
            // vine library holds, so nothing here is ever scaled by age.
            _ => plants.vines.push((name, placement(1.0, variations))),
        }
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
fn row_poles(row: &Row, ground: &Ground, rng: &mut Rng) -> Vec<(String, Placement)> {
    let (along, across) = (row.direction(), row.direction().perp());
    let yaw = row.direction().to_angle();
    let mut placed = Vec::new();
    for (slot, position) in row.post_positions(ground).enumerate() {
        let offset = along * rng.range(-POLE_OFFSET, POLE_OFFSET) as f32
            + across * rng.range(-POLE_OFFSET, POLE_OFFSET) as f32;
        let tilt = Vec2::new(
            rng.range(-POLE_TILT, POLE_TILT) as f32,
            rng.range(-POLE_TILT, POLE_TILT) as f32,
        );
        let sink = rng.range(0.0, POLE_SINK) as f32;
        placed.push((
            format!("Pole_{slot:03}"),
            Placement {
                position: ground.lift(position.truncate() + offset) - Vec3::Z * sink,
                // Round, so the yaw changes nothing today. It is the row's all
                // the same: a post that grows a wire notch or a profile has to
                // face along the row, and this is where that is already true.
                yaw,
                tilt,
                scale: 1.0,
                // There is only one — see [`pole::VARIATIONS`].
                variation: 0,
            },
        ));
    }
    placed
}

/// The scale a plant whose age draw came out at `age` stands at, or `None` if
/// it drew a mature vine.
///
/// The scale is the *replant's* own — a young vine is a shoot, and this is how
/// much of a full-grown one it has put out so far. Shorter *and* thinner,
/// which one uniform scale gives for free, and it takes the shoot's burial
/// down with it so the bend at its base stays underground at any size — see
/// [`vine::YOUNG_PROTOTYPE`].
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
            label_small("Planting seed"),
            (
                @FeathersSlider { @min: 0.0, @max: 64.0, @value: 0.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<PlantingParams>| {
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
    use crate::elements::util::testing::{self, Instance, STYLES};
    use openusd::schemas::geom::{Imageable, Mesh, PointBased, Visibility};
    use openusd::sdf::{self, Value};
    use openusd::usd::Stage;
    use std::f64::consts::SQRT_2;

    fn no_misses() -> VineyardParams {
        VineyardParams {
            planting: PlantingParams {
                miss_rate: 0.0,
                ..default()
            },
            ..default()
        }
    }

    /// Everything placed from the library at `prototype`, read back whichever
    /// way it was placed. A row carries three batches — its vines, its
    /// replants and its posts — and no test wants all of them at once.
    fn placed(stage: &Stage, prototype: &str) -> Vec<Instance> {
        stage
            .prim(sdf::path(PLANTING).unwrap())
            .children()
            .unwrap()
            .iter()
            .flat_map(|row| testing::instances(stage, row.path().as_str()))
            .filter(|instance| instance.prototype.starts_with(prototype))
            .collect()
    }

    /// Every plant standing in the parcel, mature or young. The two come off
    /// different libraries, so anything counting *slots* wants both.
    fn planted(stage: &Stage) -> Vec<Instance> {
        let mut all = mature(stage);
        all.extend(young(stage));
        all
    }

    /// Just the grown vines.
    fn mature(stage: &Stage) -> Vec<Instance> {
        placed(stage, vine::PROTOTYPE)
    }

    /// Just the recent replants — a shoot out of the bare ground.
    fn young(stage: &Stage) -> Vec<Instance> {
        placed(stage, vine::YOUNG_PROTOTYPE)
    }

    /// Every post standing in the parcel.
    fn poles(stage: &Stage) -> Vec<Instance> {
        placed(stage, pole::PROTOTYPE)
    }

    /// Where the layout put the posts, before this module nudged them.
    fn solved_posts(app: &App) -> Vec<Vec3> {
        let layout = app.world().resource::<VineyardLayout>();
        let ground = app.world().resource::<Ground>();
        layout
            .rows
            .iter()
            .flat_map(|row| row.post_positions(ground).collect::<Vec<_>>())
            .collect()
    }

    #[test]
    fn every_planted_vine_has_its_own_prim_path() {
        let (stage, _) = testing::grown(no_misses(), place::Style::Referenced);
        assert!(
            usd_bevy::authoring::prim_exists(&stage, "/Vineyard/Planting/Row_000/Vine_000"),
            "the first slot of the first row is addressable"
        );
        assert!(
            planted(&stage).len() > 100,
            "the default parcel plants a lot"
        );
    }

    /// The trade the viewer takes: no vine has a path any more, and the whole
    /// row is four arrays on one prim.
    #[test]
    fn the_preview_style_authors_one_instancer_per_row() {
        let (stage, _) = testing::grown(no_misses(), place::Style::Instanced);
        let row = "/Vineyard/Planting/Row_000";

        assert_eq!(
            stage
                .prim(sdf::path(row).unwrap())
                .type_name()
                .unwrap()
                .map(|t| t.to_string())
                .as_deref(),
            Some("Scope"),
            "a row is still a Scope — only what hangs below it changed"
        );
        assert!(
            openusd::schemas::geom::PointInstancer::get(
                &stage,
                sdf::path(format!("{row}/{VINES}")).unwrap()
            )
            .unwrap()
            .is_some(),
            "its vines are one instancer"
        );
        assert!(
            !usd_bevy::authoring::prim_exists(&stage, &format!("{row}/Vine_000")),
            "and no vine has a prim of its own"
        );
    }

    /// The whole point of referencing rather than copying: the geometry has to
    /// compose in under the placed prim. It only does if the referencing prim
    /// was defined with the prototype's *own* type — a `Mesh` reference under
    /// an `Xform` composes to an `Xform` carrying stray `points`, which no
    /// renderer dispatches on.
    ///
    /// The wood is a child rather than the placed prim itself because a vine
    /// prototype is an `Xform` over its wood *and* its shoots — see
    /// [`a_planted_vine_composes_its_shoots`].
    #[test]
    fn the_referenced_prototype_composes_in() {
        let (stage, _) = testing::grown(no_misses(), place::Style::Referenced);
        let mesh = Mesh::get(
            &stage,
            sdf::path("/Vineyard/Planting/Row_000/Vine_000/Wood").unwrap(),
        )
        .unwrap()
        .expect("the vine's wood composes in as the Mesh it references");
        assert!(
            matches!(
                mesh.points_attr().get::<Value>().unwrap(),
                Some(Value::Vec3fVec(p)) if !p.is_empty()
            ),
            "the prototype's points resolve through the reference"
        );
    }

    /// The nesting the whole element layout rests on, end to end: a leaf
    /// prototype is referenced into a shoot prototype, that shoot into a vine
    /// prototype, and that vine onto the ground. Every inner arc has to
    /// survive the outer ones, three deep.
    ///
    /// It is worth its own test because the failure is silent and one-sided —
    /// `/parts/Vine` would look perfect in isolation while every vine actually
    /// standing in the vineyard came out pruned bare. A relationship target
    /// *would* fail this way (see `stage::define_parts_library`, and the
    /// reason `place::Style` is one value for a whole pass); a reference does
    /// not, because it composes in the layer stack it was authored in.
    #[test]
    fn a_planted_vine_composes_its_shoots() {
        let (stage, _) = testing::grown(no_misses(), place::Style::Referenced);
        let path = "/Vineyard/Planting/Row_000/Vine_000/Shoot_00_0";

        assert!(
            usd_bevy::authoring::prim_exists(&stage, path),
            "the vine prototype's shoots compose in under the planted vine"
        );
        let shoot = Mesh::get(&stage, sdf::path(format!("{path}/Stem")).unwrap())
            .unwrap()
            .expect("and arrive typed as the Mesh they reference");
        assert!(
            matches!(
                shoot.points_attr().get::<Value>().unwrap(),
                Some(Value::Vec3fVec(p)) if !p.is_empty()
            ),
            "with points resolved through both references"
        );

        // And one level deeper again, which is where a leaf lives.
        let leaf = Mesh::get(&stage, sdf::path(format!("{path}/Leaf_00")).unwrap())
            .unwrap()
            .expect("a leaf composes three references deep");
        assert!(
            matches!(
                leaf.points_attr().get::<Value>().unwrap(),
                Some(Value::Vec3fVec(p)) if !p.is_empty()
            ),
            "with a blade at the end of it"
        );
    }

    /// `stage::new_stage` makes the prototype library a `class` so it stays out
    /// of the viewer's traversal. A placed vine references a prototype from
    /// inside that class, and must not pick up its abstractness: `place_referenced`
    /// authors its own `def` first, and the local specifier is what composes.
    /// If that ever regressed, every vine would vanish from viewer and sim alike.
    #[test]
    fn a_placed_vine_is_concrete_despite_referencing_an_abstract_prototype() {
        let (stage, _) = testing::grown(no_misses(), place::Style::Referenced);
        let path = sdf::path("/Vineyard/Planting/Row_000/Vine_000").unwrap();

        assert!(
            stage
                .prim(sdf::path(crate::elements::vine::PROTOTYPE).unwrap())
                .is_abstract()
                .unwrap(),
            "the prototype library is abstract"
        );

        let vine = stage.prim(path.clone());
        assert!(!vine.is_abstract().unwrap(), "a placed vine is concrete");
        assert!(vine.is_defined().unwrap(), "a placed vine is defined");

        let wood = Mesh::get(
            &stage,
            sdf::path("/Vineyard/Planting/Row_000/Vine_000/Wood").unwrap(),
        )
        .unwrap()
        .unwrap();
        assert!(
            !matches!(
                wood.visibility_attr().get::<Visibility>().unwrap(),
                Some(Visibility::Invisible)
            ),
            "and it renders"
        );
    }

    #[test]
    fn one_vine_is_planted_per_slot_in_the_layout() {
        for style in STYLES {
            let (stage, app) = testing::grown(no_misses(), style);
            let slots: usize = {
                let layout = app.world().resource::<VineyardLayout>();
                let ground = app.world().resource::<Ground>();
                layout
                    .rows
                    .iter()
                    .map(|r| r.vine_positions(ground).count())
                    .sum()
            };
            assert_eq!(planted(&stage).len(), slots, "{style:?}");
        }
    }

    /// `Vine_007` has to keep meaning "the eighth slot of this row" whatever
    /// the miss rate is, or an Isaac Lab config keyed on a path silently
    /// repoints at a different plant when the rate is nudged.
    ///
    /// Referenced only: the instanced path has no names to keep stable, which
    /// is the whole reason it is not what gets exported.
    #[test]
    fn a_vines_path_names_its_slot_not_its_rank() {
        let at = |rate: f32| {
            let (stage, _) = testing::grown(
                VineyardParams {
                    planting: PlantingParams {
                        miss_rate: rate,
                        ..default()
                    },
                    ..default()
                },
                place::Style::Referenced,
            );
            planted(&stage)
                .into_iter()
                .map(|v| {
                    let at = v.position();
                    (v.name.expect("a reference-placed vine is named"), at)
                })
                .collect::<Vec<_>>()
        };
        let few = at(0.05);
        let many = at(0.25);

        assert!(many.len() < few.len(), "{} vs {}", many.len(), few.len());
        for entry in &many {
            assert!(
                few.contains(entry),
                "{} kept both its name and its position",
                entry.0
            );
        }
    }

    /// Every vine sits on the terrain, not floating above or sunk into it.
    #[test]
    fn a_planted_vine_sits_on_the_ground() {
        for style in STYLES {
            let (stage, app) = testing::grown(no_misses(), style);
            let ground = app.world().resource::<Ground>();
            for vine in planted(&stage) {
                let at = vine.position();
                assert!(
                    (at.z - ground.height(at.x, at.y)).abs() < 1e-4,
                    "{style:?}: {vine:?} is on the height field"
                );
            }
        }
    }

    /// Checked through the composed transform rather than the `rotateZ` value
    /// alone, so the op *order* is under test too: a stack that scaled after
    /// translating would still carry the right angle.
    #[test]
    fn every_vine_faces_along_its_row() {
        let mut params = no_misses();
        params.parcel.orientation = 30.0;
        let direction = Vec2::from_angle(30.0_f32.to_radians());

        for style in STYLES {
            let (stage, _) = testing::grown(params.clone(), style);
            let vines = planted(&stage);
            assert!(vines.len() > 100, "{style:?}");
            for vine in vines {
                // Where the prototype's local +X — the axis its cordons run
                // along — ends up. Normalized, because a young vine's scale is
                // baked in and only the direction is under test here.
                let along = vine.transform.x_axis.truncate().truncate().normalize();
                assert!(
                    // `orientations` is `quath[]`, so the instanced path is
                    // only good to about three digits.
                    (along - direction).length() < 1e-2,
                    "{style:?}: cordons run along the row, got {along:?} against {direction:?}"
                );
            }
        }
    }

    /// The age draw picks a *library*, not a size: a replant is its own plant
    /// — a shoot out of the bare ground — and a mature vine is never shrunk to
    /// stand in for one.
    #[test]
    fn a_replant_is_planted_from_the_young_library() {
        let mostly_young = VineyardParams {
            planting: PlantingParams {
                miss_rate: 0.0,
                young_rate: 0.4,
                ..default()
            },
            ..default()
        };
        for style in STYLES {
            let (stage, _) = testing::grown(mostly_young.clone(), style);
            let (grown, replants) = (mature(&stage), young(&stage));

            assert!(!replants.is_empty(), "{style:?}: some slots drew a replant");
            assert!(!grown.is_empty(), "{style:?}: and most did not");
            assert!(
                replants
                    .iter()
                    .all(|v| v.prototype.starts_with(vine::YOUNG_PROTOTYPE)),
                "{style:?}: a replant draws the young library"
            );
            for vine in &grown {
                assert!(
                    (vine.scale() - Vec3::ONE).length() < 1e-6,
                    "{style:?}: a mature vine stands at full size, got {:?}",
                    vine.scale()
                );
            }
        }
    }

    /// Turning the rate off leaves a parcel of nothing but mature vines — and,
    /// with no replants to hold, no young batch on any row at all.
    #[test]
    fn no_young_rate_plants_no_replants() {
        for style in STYLES {
            let (stage, _) = testing::grown(
                VineyardParams {
                    planting: PlantingParams {
                        miss_rate: 0.0,
                        young_rate: 0.0,
                        ..default()
                    },
                    ..default()
                },
                style,
            );
            assert!(young(&stage).is_empty(), "{style:?}");
            assert!(
                !usd_bevy::authoring::prim_exists(
                    &stage,
                    &format!("/Vineyard/Planting/Row_000/{YOUNG}")
                ),
                "{style:?}: and no empty instancer left standing in for them"
            );
        }
    }

    /// A replant's shoot is still growing, so it stands somewhere between
    /// `young_scale` and full size — uniformly, which is what takes its burial
    /// down with it and keeps the bend at its base underground.
    #[test]
    fn replants_are_scaled_across_the_age_band() {
        let p = PlantingParams::default();
        assert_eq!(young_scale(&p, 0.5), None, "a mature vine is not a replant");
        assert!((young_scale(&p, 0.0).unwrap() - p.young_scale as f64).abs() < 1e-9);
        assert!(young_scale(&p, 0.04).unwrap() > p.young_scale as f64);

        let params = VineyardParams {
            planting: PlantingParams {
                miss_rate: 0.0,
                young_rate: 0.3,
                ..default()
            },
            ..default()
        };
        for style in STYLES {
            let (stage, _) = testing::grown(params.clone(), style);
            let scales: Vec<Vec3> = young(&stage).iter().map(Instance::scale).collect();

            assert!(!scales.is_empty(), "{style:?}: some vines are young");
            assert!(
                scales.iter().any(|s| s.x < 0.999),
                "{style:?}: and the youngest of them are the smallest"
            );
            for s in &scales {
                assert!(
                    s.x >= p.young_scale - 1e-6 && s.x <= 1.0 + 1e-6,
                    "{style:?}: inside the age band, got {s:?}"
                );
                assert!(
                    (s.x - s.y).abs() < 1e-6 && (s.y - s.z).abs() < 1e-6,
                    "{style:?}: a young vine is shorter and thinner by the same \
                     factor, got {s:?}"
                );
            }
        }
    }

    /// Every vine draws a prototype that exists, and the picks actually vary
    /// across the variations on offer.
    #[test]
    fn placed_vines_spread_across_the_prototype_library() {
        for style in STYLES {
            let (stage, _) = testing::grown(no_misses(), style);
            let targets: Vec<String> = planted(&stage).into_iter().map(|v| v.prototype).collect();

            assert!(
                targets
                    .iter()
                    .all(|t| usd_bevy::authoring::prim_exists(&stage, t)),
                "{style:?}"
            );
            assert!(
                targets.iter().any(|t| *t != targets[0]),
                "{style:?}: picks actually vary"
            );
        }
    }

    /// A post at every position the layout solved for one, in both placement
    /// shapes — and, in the export's, one addressable by the slot it stands
    /// in. Unlike a vine, a post is never skipped.
    #[test]
    fn a_pole_stands_at_every_solved_post_position() {
        for style in STYLES {
            let (stage, app) = testing::grown(no_misses(), style);
            let solved = solved_posts(&app);
            assert!(solved.len() > 20, "{style:?}: the fixture solved posts");
            assert_eq!(poles(&stage).len(), solved.len(), "{style:?}");
        }
        let (stage, _) = testing::grown(no_misses(), place::Style::Referenced);
        assert!(
            usd_bevy::authoring::prim_exists(&stage, "/Vineyard/Planting/Row_000/Pole_000"),
            "the first post of the first row is addressable"
        );
    }

    /// Every post is off the line the solver drew — but only just. Both
    /// directions matter: a post that drifted far enough would be standing in
    /// the row rather than in the trellis, and one that didn't drift at all
    /// leaves the parcel looking stamped out, which is the whole reason the
    /// nudges are here.
    #[test]
    fn poles_are_nudged_off_the_layout_without_leaving_it() {
        for style in STYLES {
            let (stage, app) = testing::grown(no_misses(), style);
            let ground = app.world().resource::<Ground>();
            let solved = solved_posts(&app);

            let (mut offsets, mut sinks, mut leans) = (Vec::new(), Vec::new(), Vec::new());
            for post in poles(&stage) {
                let at = post.position();
                offsets.push(
                    solved
                        .iter()
                        .map(|s| s.truncate().distance(at.truncate()))
                        .fold(f32::MAX, f32::min),
                );
                sinks.push(ground.height(at.x, at.y) - at.z);
                // Where the post's own +Z — the axis it was authored standing
                // up — ended up.
                leans.push((post.rotation() * Vec3::Z).angle_between(Vec3::Z));
            }
            let spread = |v: &[f32]| {
                v.iter()
                    .fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(*x), hi.max(*x)))
            };

            // A foot may move in both directions at once, so the two draws
            // compose to a diagonal.
            let (_, furthest) = spread(&offsets);
            assert!(
                furthest <= (POLE_OFFSET * SQRT_2) as f32 + 1e-4,
                "{style:?}: a post landed {furthest} m off the layout"
            );
            assert!(furthest > 0.005, "{style:?}: the posts are all on the line");

            let (shallowest, deepest) = spread(&sinks);
            assert!(
                (-1e-4..=POLE_SINK as f32 + 1e-4).contains(&shallowest)
                    && deepest <= POLE_SINK as f32 + 1e-4,
                "{style:?}: driven {shallowest}..{deepest} m into the ground"
            );
            assert!(
                deepest - shallowest > POLE_SINK as f32 / 2.0,
                "{style:?}: every post was driven to the same depth"
            );

            let (_, worst) = spread(&leans);
            assert!(
                worst <= (POLE_TILT * SQRT_2) as f32 + 1e-3,
                "{style:?}: a post leans {worst} rad off plumb"
            );
            assert!(worst > 0.005, "{style:?}: every post is dead plumb");
        }
    }

    /// The posts draw from a stream of their own, so re-rolling the planting
    /// seed moves both — but changing what the *vines* do must leave the
    /// trellis exactly where it was.
    #[test]
    fn the_trellis_does_not_move_when_the_planting_around_it_changes() {
        let at = |miss_rate: f32| {
            let (stage, _) = testing::grown(
                VineyardParams {
                    planting: PlantingParams {
                        miss_rate,
                        ..default()
                    },
                    ..default()
                },
                place::Style::Referenced,
            );
            poles(&stage)
                .iter()
                .map(Instance::position)
                .collect::<Vec<_>>()
        };
        assert_eq!(at(0.0), at(0.4));
    }

    /// Planting owns its subtree and rewrites it from scratch, so shrinking
    /// the layout must not leave the rows that no longer exist behind.
    #[test]
    fn re_authoring_drops_rows_that_no_longer_exist() {
        let (stage, mut app) = testing::grown(no_misses(), place::Style::Referenced);
        let rows = |stage: &Stage| {
            stage
                .prim(sdf::path(PLANTING).unwrap())
                .child_names()
                .unwrap()
                .len()
        };
        let before = rows(&stage);
        assert!(before > 2);

        app.world_mut()
            .resource_mut::<super::super::parcel::ParcelParams>()
            .row_spacing = 12.0;
        app.update();
        assert!(rows(&stage) < before, "stale rows removed on re-author");
    }
}
