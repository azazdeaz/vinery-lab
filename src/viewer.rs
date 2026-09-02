//! Interactive viewer: a windowed Bevy app that draws the generated scene and
//! lets you edit its parameters, saving on demand.
//!
//! Element build systems spawn ordinary Bevy entities during `PreUpdate` and
//! Bevy renders them directly. Saving exports the same entities as a scene
//! document for the Python builder to author.
//!
//! The scene is authored Z-up and stood upright by the scene root's parent (see
//! [`scene::z_up_to_y_up`](crate::scene)), so the camera below works in Bevy's
//! ordinary Y-up world.

use bevy::input::common_conditions::input_just_pressed;
use bevy::prelude::*;
use bevy_panorbit_camera::{PanOrbitCamera, PanOrbitCameraPlugin};

use crate::elements::util::parcel;
use crate::ui::ParamsPanel;

/// Where the save key writes the scene document.
const SCENE_PATH: &str = "scene.json";

pub fn run() {
    let mut app = App::new();
    app.add_plugins((
            DefaultPlugins,
            PanOrbitCameraPlugin,
            crate::scene::plugin,
            crate::elements::plugin,
            crate::ui::plugin,
            // Gizmos need `GizmoPlugin` (from `DefaultPlugins`), which the
            // headless generation path's `MinimalPlugins` doesn't provide —
            // see `parcel::debug_plugin`'s docs for why it's kept separate
            // from `crate::elements::plugin`.
            parcel::debug_plugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                save_scene_on_key.run_if(input_just_pressed(KeyCode::KeyS)),
                sync_camera_enabled_with_ui,
            ),
        );

    // Off by default: it logs a line per re-authored frame, which during a
    // slider drag is every frame. See [`crate::perf`].
    if std::env::var_os(crate::perf::ENV).is_some() {
        app.add_plugins(crate::perf::plugin);
    }

    app.run();
}

fn setup(mut commands: Commands) {
    // Framed for `TerrainParams::default()`'s 80x50m extent, not the 4x4m
    // placeholder scale the defaults used before rows landed.
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(70.0, 55.0, 70.0).looking_at(Vec3::ZERO, Vec3::Y),
        PanOrbitCamera::default(),
        AmbientLight {
            brightness: 220.0,
            ..default()
        },
    ));
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(40.0, 80.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Press `S` to write the scene out as a document for the Python USD builder.
///
/// Exports the entities on screen: the viewer and the export draw from one
/// scene graph, so there is no preview shape and export shape to keep in step.
fn save_scene_on_key(world: &mut World) -> Result<()> {
    std::fs::write(SCENE_PATH, crate::scene::export::scene_json(world)?)?;
    info!("saved {SCENE_PATH} — build it with `python -m vinerylab.usd {SCENE_PATH} scene.usd`");
    Ok(())
}

/// Disables orbit/pan/zoom while the pointer is over the params panel, so
/// dragging a slider there doesn't also drag the camera underneath it.
fn sync_camera_enabled_with_ui(
    panel: Query<&Interaction, With<ParamsPanel>>,
    mut cameras: Query<&mut PanOrbitCamera>,
) {
    let over_panel = panel
        .iter()
        .any(|interaction| *interaction != Interaction::None);
    for mut camera in &mut cameras {
        camera.enabled = !over_panel;
    }
}
