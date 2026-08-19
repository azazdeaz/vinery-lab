//! Interactive viewer: a windowed Bevy app that live-projects the USD stage
//! and lets you edit its parameters, saving on demand.
//!
//! The stage is created once at startup and never recreated. Element author
//! systems edit it in place during `PreUpdate`, and `LiveStagePlugin`'s
//! `Update` systems project the result — the first frame in full, every
//! frame after that as a diff against what changed.

use bevy::prelude::*;
use usd_bevy::UsdPlugin;
use usd_bevy::authoring::save_stage_as;
use usd_bevy::live::{LiveStage, LiveStagePlugin};

pub fn run() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            UsdPlugin,
            LiveStagePlugin,
            crate::elements::plugin,
            crate::ui::plugin,
        ))
        // `open_stage` must land before the first `PreUpdate`, where the
        // element author systems expect a `LiveStage` to already exist.
        .add_systems(Startup, (open_stage, setup))
        .add_systems(Update, save_usd_on_key)
        .run();
}

fn open_stage(world: &mut World) -> Result<()> {
    let stage = crate::stage::new_stage("scene.usda")?;
    world.insert_non_send(LiveStage::new(stage));
    Ok(())
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(4.0, 3.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        AmbientLight {
            brightness: 220.0,
            ..default()
        },
    ));
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
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
