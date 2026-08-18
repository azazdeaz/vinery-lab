//! Scene description: the fixed parameters that drive procedural generation,
//! and the Bevy system that spawns entities from them.
//!
//! [`SceneParams`] does double duty as a Bevy [`Resource`] (read by
//! [`spawn_scene`]) and, behind the `python` feature, a `#[pyclass]`
//! constructible directly from Python. A plain data struct satisfies both:
//! `Resource` needs `Send + Sync + 'static`, `pyclass` needs `Send`.

use bevy::prelude::*;

#[derive(Resource, Clone, Debug)]
#[cfg_attr(feature = "python", pyo3::pyclass(get_all, set_all, from_py_object))]
pub struct SceneParams {
    pub rows: u32,
    pub cols: u32,
    pub spacing: f32,
    pub cube_size: f32,
}

impl Default for SceneParams {
    fn default() -> Self {
        Self {
            rows: 10,
            cols: 10,
            spacing: 0.2,
            cube_size: 0.1,
        }
    }
}

#[derive(Component)]
pub struct Cube {
    pub size: f32,
}

/// Stable spawn order, independent of ECS iteration order. [`author_scene`]
/// sorts on this so the generated `.usda` is byte-identical across runs for
/// the same [`SceneParams`].
#[derive(Component)]
pub struct CubeIndex(pub u32);

/// Populates the world from `params`. Shared by the headless generator
/// ([`crate::generate`]) and the interactive viewer ([`crate::viewer`]), so
/// both produce the same scene from the same parameters.
pub fn spawn_scene(mut commands: Commands, params: Res<SceneParams>) {
    let mut i = 0u32;
    for x in 0..params.cols {
        for y in 0..params.rows {
            commands.spawn((
                Cube { size: params.cube_size },
                CubeIndex(i),
                Transform::from_xyz(x as f32 * params.spacing, y as f32 * params.spacing, 0.0),
            ));
            i += 1;
        }
    }
}
