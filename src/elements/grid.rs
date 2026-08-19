//! Grid element — the placeholder layout, standing in until terrain and rows
//! land.
//!
//! Owns the scene root (`/Vineyard`, the stage's default prim) and scatters
//! cube prototypes across it on a regular lattice. It discovers what to
//! instance by listing the children of [`cube::PROTOTYPE`] rather than being
//! told, so adding a cube variation needs no change here.

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use openusd::schemas::geom::PointInstancer;
use openusd::sdf::{self, Value};
use openusd::usd::Stage;
use usd_bevy::authoring::{define_prim, remove_prim};
use usd_bevy::live::LiveStage;

use super::cube::{self, CubeParams};
use super::{Grow, variation_indices};

/// The subtree this element owns. Also the stage's default prim, so a
/// consumer referencing the generated layer gets the whole scene.
pub const ROOT: &str = "/Vineyard";

const INSTANCER: &str = "/Vineyard/Cubes";

#[derive(Resource, Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct GridParams {
    pub rows: u32,
    pub cols: u32,
    /// Distance between neighbouring instances, in meters.
    pub spacing: f32,
    /// Drives which prototype variation each instance gets. Changing it
    /// re-rolls the scene without rebuilding any geometry.
    pub seed: u64,
}

impl Default for GridParams {
    fn default() -> Self {
        Self {
            rows: 10,
            cols: 10,
            spacing: 0.2,
            seed: 0,
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<GridParams>().add_systems(
        PreUpdate,
        author.in_set(Grow::Layout).run_if(
            resource_changed::<GridParams>.or_else(resource_changed::<CubeParams>),
        ),
    );
}

fn author(live: NonSend<LiveStage>, params: Res<GridParams>) -> Result<()> {
    let stage = &live.stage;
    remove_prim(stage, ROOT)?;
    define_prim(stage, ROOT, "Xform")?;
    stage.set_default_prim("Vineyard")?;

    let prototypes = prototype_paths(stage, cube::PROTOTYPE)?;
    if prototypes.is_empty() {
        return Ok(());
    }

    let positions = lattice(params.rows, params.cols, params.spacing);
    let indices = variation_indices(params.seed, positions.len(), prototypes.len());

    let instancer = PointInstancer::define(stage, sdf::path(INSTANCER)?)?;
    instancer.create_prototypes_rel()?.set_targets(prototypes)?;
    instancer
        .create_positions_attr()?
        .set(Value::Vec3fVec(
            positions.into_iter().map(Into::into).collect(),
        ))?;
    instancer
        .create_proto_indices_attr()?
        .set(Value::IntVec(indices))?;
    Ok(())
}

/// The prototype prims published under `root`, in a stable order.
///
/// Sorted by name so the generated stage is reproducible; which variation
/// lands at which index doesn't matter, only that it doesn't drift between
/// runs.
fn prototype_paths(stage: &Stage, root: &str) -> anyhow::Result<Vec<sdf::Path>> {
    let mut names: Vec<String> = stage
        .prim(sdf::path(root)?)
        .child_names()?
        .iter()
        .map(|name| name.as_str().to_string())
        .collect();
    names.sort();
    names
        .iter()
        .map(|name| sdf::path(format!("{root}/{name}")))
        .collect()
}

/// A `rows` x `cols` lattice on the ground plane, centered on the origin.
/// Z-up, so the grid spans XY (see [`crate::stage`] for the convention).
fn lattice(rows: u32, cols: u32, spacing: f32) -> Vec<[f32; 3]> {
    let offset = |n: u32| (n.saturating_sub(1)) as f32 * spacing / 2.0;
    let (x0, y0) = (offset(cols), offset(rows));
    (0..cols)
        .flat_map(move |x| {
            (0..rows).map(move |y| {
                [
                    x as f32 * spacing - x0,
                    y as f32 * spacing - y0,
                    0.0,
                ]
            })
        })
        .collect()
}

pub fn ui() -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            label_small("Rows"),
            (
                @FeathersSlider { @min: 1.0, @max: 50.0, @value: 10.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<GridParams>| {
                    params.rows = change.value.round().max(1.0) as u32;
                })
            ),
            label_small("Cols"),
            (
                @FeathersSlider { @min: 1.0, @max: 50.0, @value: 10.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<GridParams>| {
                    params.cols = change.value.round().max(1.0) as u32;
                })
            ),
            label_small("Spacing"),
            (
                @FeathersSlider { @min: 0.05, @max: 2.0, @value: 0.2 }
                SliderStep(0.05)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<GridParams>| {
                    params.spacing = change.value;
                })
            ),
            label_small("Variation seed"),
            (
                @FeathersSlider { @min: 0.0, @max: 64.0, @value: 0.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<GridParams>| {
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

    #[test]
    fn lattice_is_centered_on_the_origin() {
        let points = lattice(3, 3, 1.0);
        assert_eq!(points.len(), 9);
        let sum: [f32; 3] = points.iter().fold([0.0; 3], |acc, p| {
            [acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]]
        });
        assert!(sum.iter().all(|c| c.abs() < 1e-5), "centroid at origin");
    }

    #[test]
    fn a_single_instance_sits_at_the_origin() {
        assert_eq!(lattice(1, 1, 0.5), vec![[0.0, 0.0, 0.0]]);
    }

    #[test]
    fn instances_every_cube_variation() {
        let params = VineyardParams {
            cube: CubeParams {
                size: 0.2,
                variations: 3,
            },
            grid: GridParams {
                rows: 4,
                cols: 5,
                spacing: 0.3,
                seed: 1,
            },
        };
        let stage = crate::generate::generate_stage(&params).unwrap();

        let instancer = PointInstancer::get(&stage, sdf::path(INSTANCER).unwrap())
            .unwrap()
            .expect("PointInstancer authored");
        assert_eq!(
            instancer.prototypes_rel().targets().unwrap().len(),
            3,
            "one prototype target per cube variation"
        );
        assert!(
            matches!(
                instancer.proto_indices_attr().get().unwrap(),
                Some(Value::IntVec(v)) if v.len() == 20
            ),
            "one prototype choice per lattice point"
        );
    }

    /// The generated stage must be byte-identical across runs for the same
    /// params, so a downstream sim gets a reproducible scene.
    #[test]
    fn generation_is_reproducible() {
        let params = VineyardParams::default();
        let a = crate::generate::generate_stage(&params)
            .unwrap()
            .root_layer()
            .export_to_string()
            .unwrap();
        let b = crate::generate::generate_stage(&params)
            .unwrap()
            .root_layer()
            .export_to_string()
            .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn reseeding_changes_the_variation_assignment() {
        let mut params = VineyardParams::default();
        let a = crate::generate::generate_stage(&params).unwrap();
        params.grid.seed = 99;
        let b = crate::generate::generate_stage(&params).unwrap();

        let indices = |stage: &Stage| {
            PointInstancer::get(stage, sdf::path(INSTANCER).unwrap())
                .unwrap()
                .unwrap()
                .proto_indices_attr()
                .get::<Value>()
                .unwrap()
        };
        assert_ne!(indices(&a), indices(&b));
    }
}
