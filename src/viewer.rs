//! Interactive viewer: a windowed Bevy app that live-projects the USD stage
//! and lets you edit its parameters, saving on demand.
//!
//! The stage is created once at startup and never recreated. Element author
//! systems edit it in place during `PreUpdate`, and `LiveStagePlugin`'s
//! `Update` systems project the result — the first frame in full, every
//! frame after that as a diff against what changed.

use bevy::prelude::*;
use bevy_panorbit_camera::{PanOrbitCamera, PanOrbitCameraPlugin};
use usd_bevy::UsdPlugin;
use usd_bevy::authoring::save_stage_as;
use usd_bevy::live::{LiveStage, LiveStagePlugin};

use crate::elements::parcel;
use crate::ui::ParamsPanel;

pub fn run() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            UsdPlugin,
            LiveStagePlugin,
            PanOrbitCameraPlugin,
            crate::elements::plugin,
            crate::ui::plugin,
            // Gizmos need `GizmoPlugin` (from `DefaultPlugins`), which the
            // headless generation path's `MinimalPlugins` doesn't provide —
            // see `parcel::debug_plugin`'s docs for why it's kept separate
            // from `crate::elements::plugin`.
            parcel::debug_plugin,
        ))
        // `open_stage` must land before the first `PreUpdate`, where the
        // element author systems expect a `LiveStage` to already exist.
        .add_systems(Startup, (open_stage, setup))
        .add_systems(Update, (save_usd_on_key, sync_camera_enabled_with_ui))
        .run();
}

fn open_stage(world: &mut World) -> Result<()> {
    let stage = crate::stage::new_stage("scene.usda")?;
    world.insert_non_send(LiveStage::new(stage));
    Ok(())
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

/// Press `S` to write the live stage's current state out as a `.usda` file.
fn save_usd_on_key(live: NonSend<LiveStage>, keys: Res<ButtonInput<KeyCode>>) -> Result<()> {
    if keys.just_pressed(KeyCode::KeyS) {
        save_stage_as(&live.stage, "scene.usda")?;
        info!("saved scene.usda");
    }
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
