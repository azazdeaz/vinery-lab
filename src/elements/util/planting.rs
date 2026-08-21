//! Planting — what actually stands on the ground, and where.
//!
//! [`parcel`](super::parcel) solves *where* things go; this authors *what* is
//! there. It walks the solved [`VineyardLayout`], draping it onto
//! [`Ground`], and places one prim per plant through
//! [`place`](super::place).
//!
//! Terrain's placement helper, exactly the standing [`parcel`] has as its
//! layout helper: it owns no element identity, has no `/parts` prototypes of
//! its own, and is wired from [`terrain::plugin`] rather than
//! [`elements::plugin`]. What it does own is the `/Vineyard/Planting`
//! subtree — so `terrain` is an element with two subtrees, its surface and
//! everything planted on it.
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
use crate::elements::terrain::Ground;
use crate::elements::vine;

use super::parcel::{Row, VineyardLayout};
use super::place::{self, Placement};

/// The subtree this module owns and rewrites from scratch.
pub const PLANTING: &str = "/Vineyard/Planting";

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
    pub young_rate: f32,
    /// How small the youngest replant is, relative to a mature vine.
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
) -> Result<()> {
    let stage = &live.stage;
    remove_prim(stage, PLANTING)?;
    define_prim(stage, PLANTING, "Xform")?;

    let variations = place::prototype_count(stage, vine::PROTOTYPE).max(1);
    let mut rng = Rng::new(params.seed);

    for (index, row) in layout.rows.iter().enumerate() {
        let group = format!("{PLANTING}/Row_{index:03}");
        // A Scope, not an Xform: a row carries no transform of its own — its
        // plants are already placed in stage space, draped individually onto
        // terrain that a single row transform could not follow.
        define_prim(stage, &group, "Scope")?;
        let vines = row_vines(row, &ground, &params, variations, &mut rng);
        place::place_referenced(stage, &group, vine::PROTOTYPE, &vines)?;
    }
    Ok(())
}

/// The vines of one row, named by planting slot.
///
/// The *draw order* of `rng` is part of this module's output: all three draws
/// happen before the miss test, so the stream stays aligned no matter which
/// slots are skipped. Rolling them lazily instead would make every vine past
/// the first change re-roll whenever `miss_rate` was nudged.
fn row_vines(
    row: &Row,
    ground: &Ground,
    params: &PlantingParams,
    variations: usize,
    rng: &mut Rng,
) -> Vec<(String, Placement)> {
    let yaw = row.direction().to_angle();
    let mut placed = Vec::new();
    for (slot, position) in row.vine_positions(ground).enumerate() {
        let (miss, age, pick) = (rng.unit(), rng.unit(), rng.unit());
        if miss < params.miss_rate as f64 {
            continue;
        }
        placed.push((
            format!("Vine_{slot:03}"),
            Placement {
                position,
                yaw,
                // A vine stands upright; the row's direction is the whole
                // rotation. Tilt is for nested placement — see `Placement`.
                tilt: Vec2::ZERO,
                scale: age_scale(params, age) as f32,
                variation: (pick * variations as f64) as usize % variations,
            },
        ));
    }
    placed
}

/// Uniform scale for a vine whose age draw came out at `age`.
///
/// Young vines are shorter *and* thinner, which one uniform scale gives for
/// free. Ramping across the young band rather than stepping means a replanted
/// stretch of row shows a spread of ages, not one clone repeated.
fn age_scale(params: &PlantingParams, age: f64) -> f64 {
    let young_rate = params.young_rate as f64;
    let young_scale = params.young_scale as f64;
    if young_rate <= 0.0 || age >= young_rate {
        return 1.0;
    }
    young_scale + (1.0 - young_scale) * (age / young_rate)
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
    use openusd::schemas::geom::{Imageable, Mesh, PointBased, Visibility, Xformable};
    use openusd::sdf::{self, Value};
    use openusd::usd::Stage;

    /// Runs one authoring cycle, handing back the stage together with the app
    /// it was authored from — tests need the solved [`VineyardLayout`] and
    /// [`Ground`] to check the placed prims against what they were placed
    /// from.
    fn grown(params: VineyardParams) -> (Stage, App) {
        let stage = crate::stage::new_stage("planting.usda").unwrap();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(crate::elements::plugin);
        params.insert(app.world_mut());
        app.world_mut()
            .insert_non_send(LiveStage::new(stage.clone()));
        app.finish();
        app.cleanup();
        app.update();
        (stage, app)
    }

    fn no_misses() -> VineyardParams {
        VineyardParams {
            planting: PlantingParams {
                miss_rate: 0.0,
                ..default()
            },
            ..default()
        }
    }

    /// Every planted prim, as `(path, translate)`.
    fn planted(stage: &Stage) -> Vec<(String, [f64; 3])> {
        let mut out = Vec::new();
        for row in stage.prim(sdf::path(PLANTING).unwrap()).children().unwrap() {
            for vine in row.children().unwrap() {
                let path = vine.path().as_str().to_string();
                let m = Placed(vine).local_to_parent_transform(0.0).unwrap();
                out.push((path, [m.0[12], m.0[13], m.0[14]]));
            }
        }
        out
    }

    /// A bare `Xformable` view, mirroring the one `place` authors through.
    struct Placed(openusd::usd::Prim);
    impl openusd::usd::SchemaBase for Placed {
        const KIND: openusd::usd::SchemaKind = openusd::usd::SchemaKind::ConcreteTyped;
        fn prim(&self) -> &openusd::usd::Prim {
            &self.0
        }
    }
    impl Imageable for Placed {}
    impl Xformable for Placed {}

    #[test]
    fn every_planted_vine_has_its_own_prim_path() {
        let (stage, _) = grown(no_misses());
        assert!(
            usd_bevy::authoring::prim_exists(&stage, "/Vineyard/Planting/Row_000/Vine_000"),
            "the first slot of the first row is addressable"
        );
        assert!(
            planted(&stage).len() > 100,
            "the default parcel plants a lot"
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
        let (stage, _) = grown(no_misses());
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

    /// The nesting the whole element layout rests on, end to end: a shoot
    /// prototype is referenced into a vine prototype, and that vine prototype
    /// is referenced onto the ground. The inner arc has to survive the outer
    /// one.
    ///
    /// It is worth its own test because the failure is silent and one-sided —
    /// `/parts/Vine` would look perfect in isolation while every vine actually
    /// standing in the vineyard came out pruned bare. A relationship target
    /// *would* fail this way (see `stage::define_parts_library`); a reference
    /// does not, because it composes in the layer stack it was authored in.
    #[test]
    fn a_planted_vine_composes_its_shoots() {
        let (stage, _) = grown(no_misses());
        let path = "/Vineyard/Planting/Row_000/Vine_000/Shoot_00_0";

        assert!(
            usd_bevy::authoring::prim_exists(&stage, path),
            "the vine prototype's shoots compose in under the planted vine"
        );
        let shoot = Mesh::get(&stage, sdf::path(path).unwrap())
            .unwrap()
            .expect("and arrive typed as the Mesh they reference");
        assert!(
            matches!(
                shoot.points_attr().get::<Value>().unwrap(),
                Some(Value::Vec3fVec(p)) if !p.is_empty()
            ),
            "with points resolved through both references"
        );
    }

    /// `stage::new_stage` makes the prototype library a `class` so it stays out
    /// of the viewer's traversal. A placed vine references a prototype from
    /// inside that class, and must not pick up its abstractness: `place_referenced`
    /// authors its own `def` first, and the local specifier is what composes.
    /// If that ever regressed, every vine would vanish from viewer and sim alike.
    #[test]
    fn a_placed_vine_is_concrete_despite_referencing_an_abstract_prototype() {
        let (stage, _) = grown(no_misses());
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
        let (stage, app) = grown(no_misses());
        let slots: usize = {
            let layout = app.world().resource::<VineyardLayout>();
            let ground = app.world().resource::<Ground>();
            layout
                .rows
                .iter()
                .map(|r| r.vine_positions(ground).count())
                .sum()
        };
        assert_eq!(planted(&stage).len(), slots);
    }

    /// `Vine_007` has to keep meaning "the eighth slot of this row" whatever
    /// the miss rate is, or an Isaac Lab config keyed on a path silently
    /// repoints at a different plant when the rate is nudged.
    #[test]
    fn a_vines_path_names_its_slot_not_its_rank() {
        let at = |rate: f32| {
            planted(
                &grown(VineyardParams {
                    planting: PlantingParams {
                        miss_rate: rate,
                        ..default()
                    },
                    ..default()
                })
                .0,
            )
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
        let (stage, app) = grown(no_misses());
        let ground = app.world().resource::<Ground>();
        for (path, [x, y, z]) in planted(&stage) {
            assert!(
                (z as f32 - ground.height(x as f32, y as f32)).abs() < 1e-4,
                "{path} is on the height field"
            );
        }
    }

    /// Checked through the composed transform rather than the `rotateZ` value
    /// alone, so the op *order* is under test too: a stack that scaled after
    /// translating would still carry the right angle.
    #[test]
    fn every_vine_faces_along_its_row() {
        let mut params = no_misses();
        params.parcel.orientation = 30.0;
        let (stage, _) = grown(params);

        let direction = Vec2::from_angle(30.0_f32.to_radians());
        let mut checked = 0;
        for row in stage.prim(sdf::path(PLANTING).unwrap()).children().unwrap() {
            for vine in row.children().unwrap() {
                let m = Placed(vine).local_to_parent_transform(0.0).unwrap();
                // Row-vector convention: the first row of the matrix is where
                // the prototype's local +X — the axis its cordons run along —
                // ends up. Normalized, because a young vine's scale is baked
                // into it and only the direction is under test here.
                let along = Vec2::new(m.0[0] as f32, m.0[1] as f32).normalize();
                assert!(
                    (along - direction).length() < 1e-3,
                    "cordons run along the row, got {along:?} against {direction:?}"
                );
                checked += 1;
            }
        }
        assert!(checked > 100);
    }

    #[test]
    fn young_vines_are_scaled_down_uniformly() {
        let p = PlantingParams::default();
        assert_eq!(age_scale(&p, 0.5), 1.0, "a mature vine is full size");
        assert!((age_scale(&p, 0.0) - p.young_scale as f64).abs() < 1e-9);
        assert!(age_scale(&p, 0.04) > p.young_scale as f64);

        let (stage, _) = grown(VineyardParams {
            planting: PlantingParams {
                miss_rate: 0.0,
                young_rate: 0.3,
                ..default()
            },
            ..default()
        });
        let scales: Vec<[f64; 3]> = stage
            .prim(sdf::path(PLANTING).unwrap())
            .children()
            .unwrap()
            .iter()
            .flat_map(|row| row.children().unwrap())
            .map(|vine| {
                let m = Placed(vine).local_to_parent_transform(0.0).unwrap();
                // Diagonal of the linear block: uniform scale, no shear.
                [m.0[0].hypot(m.0[1]), m.0[4].hypot(m.0[5]), m.0[10]]
            })
            .collect();

        assert!(scales.iter().any(|s| s[0] < 0.999), "some vines are young");
        for s in &scales {
            assert!(
                s[0] >= p.young_scale as f64 - 1e-6 && s[0] <= 1.0 + 1e-6,
                "inside the age band, got {s:?}"
            );
            assert!(
                (s[0] - s[1]).abs() < 1e-6 && (s[1] - s[2]).abs() < 1e-6,
                "a young vine is shorter and thinner by the same factor, got {s:?}"
            );
        }
    }

    /// Every vine references a prototype that exists, and the picks actually
    /// vary across the variations on offer.
    #[test]
    fn placed_vines_spread_across_the_prototype_library() {
        let (stage, _) = grown(no_misses());
        let targets: Vec<String> = stage
            .prim(sdf::path(PLANTING).unwrap())
            .children()
            .unwrap()
            .iter()
            .flat_map(|row| row.children().unwrap())
            .map(|vine| {
                assert!(
                    vine.has_composition_arc().unwrap(),
                    "{} composes a reference",
                    vine.path().as_str()
                );
                // The prim stack names every site contributing an opinion;
                // for a referencing prim that includes the target it pulled in.
                vine.prim_stack()
                    .unwrap()
                    .into_iter()
                    .map(|(_, path)| path.as_str().to_string())
                    .find(|path| path.starts_with(vine::PROTOTYPE))
                    .expect("the prototype shows up in the prim stack")
            })
            .collect();

        assert!(
            targets
                .iter()
                .all(|t| usd_bevy::authoring::prim_exists(&stage, t))
        );
        assert!(
            targets.iter().any(|t| *t != targets[0]),
            "picks actually vary"
        );
    }

    /// Planting owns its subtree and rewrites it from scratch, so shrinking
    /// the layout must not leave the rows that no longer exist behind.
    #[test]
    fn re_authoring_drops_rows_that_no_longer_exist() {
        let (stage, mut app) = grown(no_misses());
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
