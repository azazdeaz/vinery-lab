//! Cube element — a placeholder prototype, standing in until real vineyard
//! geometry lands.
//!
//! Authors nothing but reusable geometry: `variations` boxes under
//! [`PROTOTYPE`], differing only in size. It knows nothing about who
//! instances them.

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use usd_bevy::authoring::{define_prim, remove_prim};
use usd_bevy::live::LiveStage;

use super::Grow;
use super::usd::{author_mesh, box_mesh};

/// The subtree this element owns, and the contract it offers: after
/// [`Grow::Prototypes`] there is one `Var_<i>` mesh under here per variation.
pub const PROTOTYPE: &str = "/parts/Cube";

#[derive(Resource, Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct CubeParams {
    /// Edge length of the largest variation, in meters.
    pub size: f32,
    /// How many prototypes to author. Variation `i` is scaled down from
    /// `size` so the set is visibly distinguishable.
    pub variations: u32,
}

impl Default for CubeParams {
    fn default() -> Self {
        Self {
            size: 0.1,
            variations: 3,
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<CubeParams>().add_systems(
        PreUpdate,
        author
            .in_set(Grow::Prototypes)
            .run_if(resource_changed::<CubeParams>),
    );
}

fn author(live: NonSend<LiveStage>, params: Res<CubeParams>) -> Result<()> {
    let stage = &live.stage;
    remove_prim(stage, PROTOTYPE)?;
    define_prim(stage, PROTOTYPE, "Scope")?;

    let variations = params.variations.max(1);
    for i in 0..variations {
        let size = params.size * (i + 1) as f32 / variations as f32;
        author_mesh(stage, &format!("{PROTOTYPE}/Var_{i}"), &box_mesh(size))?;
    }
    Ok(())
}

pub fn ui() -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            label_small("Cube size"),
            (
                @FeathersSlider { @min: 0.01, @max: 1.0, @value: 0.1 }
                SliderStep(0.01)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<CubeParams>| {
                    params.size = change.value;
                })
            ),
            label_small("Cube variations"),
            (
                @FeathersSlider { @min: 1.0, @max: 8.0, @value: 3.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<CubeParams>| {
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

    #[test]
    fn authors_one_prototype_per_variation() {
        let params = VineyardParams {
            cube: CubeParams {
                size: 0.2,
                variations: 4,
            },
            ..default()
        };
        let stage = crate::generate::generate_stage(&params).unwrap();

        for i in 0..4 {
            assert!(
                usd_bevy::authoring::prim_exists(&stage, &format!("{PROTOTYPE}/Var_{i}")),
                "Var_{i} authored"
            );
        }
        assert!(
            !usd_bevy::authoring::prim_exists(&stage, &format!("{PROTOTYPE}/Var_4")),
            "no prototype past the requested count"
        );
    }

    /// Re-authoring must not leave prototypes from a larger previous count
    /// behind — the element owns its subtree and clears it first.
    #[test]
    fn shrinking_the_count_drops_stale_prototypes() {
        let stage = crate::stage::new_stage("cube.usda").unwrap();
        let live = LiveStage::new(stage.clone());

        let mut world = World::new();
        world.insert_non_send(live);
        world.insert_resource(CubeParams {
            size: 0.2,
            variations: 4,
        });
        let mut schedule = Schedule::default();
        schedule.add_systems(author);
        schedule.run(&mut world);
        assert!(usd_bevy::authoring::prim_exists(
            &stage,
            &format!("{PROTOTYPE}/Var_3")
        ));

        world.insert_resource(CubeParams {
            size: 0.2,
            variations: 2,
        });
        schedule.run(&mut world);
        assert!(
            !usd_bevy::authoring::prim_exists(&stage, &format!("{PROTOTYPE}/Var_3")),
            "stale prototype removed on re-author"
        );
    }
}
