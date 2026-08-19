//! Interactive viewer: a windowed Bevy app that live-projects the USD stage
//! and lets you inspect/edit it, saving on demand. This is the original
//! `main.rs` behavior, now callable as a library entry point so the
//! headless generator (`crate::generate`) can live in the same crate
//! without pulling in windowing/rendering.

use bevy::prelude::*;
use openusd::usd::Stage;
use usd_bevy::UsdPlugin;
use usd_bevy::authoring::save_stage_as;
use usd_bevy::live::{LiveStage, LiveStagePlugin, PrimEntities, project_stage};

use crate::author::author_scene;
use crate::scene::{SceneParams, spawn_scene};

/// Set by `mark_needs_rebuild` (a normal system, run right after
/// `spawn_scene`) and consumed by `rebuild_usd` (an exclusive system, which
/// can't take a `Changed` query param directly alongside `&mut World`).
#[derive(Resource, Default)]
struct NeedsRebuild(bool);

pub fn run() {
    App::new()
        .add_plugins((DefaultPlugins, UsdPlugin, LiveStagePlugin, crate::ui::plugin))
        .init_resource::<SceneParams>()
        .init_resource::<NeedsRebuild>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                // `SceneParams` starts out "changed" on the very first Update
                // tick too, so this also covers the initial spawn — no
                // separate `Startup` call to `spawn_scene` is needed.
                (crate::ui::despawn_cubes, spawn_scene, mark_needs_rebuild)
                    .chain()
                    .run_if(resource_changed::<SceneParams>),
                rebuild_usd,
                save_usd_on_key,
            )
                .chain(),
        )
        .run();
}

fn setup(world: &mut World) {
    world.spawn((
        Camera3d::default(),
        Transform::from_xyz(4.0, 3.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        AmbientLight {
            brightness: 220.0,
            ..default()
        },
    ));
    world.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    world.resource_mut::<NeedsRebuild>().0 = true;
}

/// Unconditionally flags `NeedsRebuild`. Chained directly after
/// `spawn_scene`, so a panel-driven parameter change rebuilds the USD stage
/// the same frame the cube grid respawns. Plain (non-exclusive) system, so
/// it runs before `spawn_scene`'s commands are actually applied — that's
/// fine here since it just sets a flag rather than reading the respawned
/// cubes; `rebuild_usd`, an exclusive system, forces that sync point right
/// before it reads them.
fn mark_needs_rebuild(mut dirty: ResMut<NeedsRebuild>) {
    dirty.0 = true;
}

/// Fully recreate the USD stage from the current cube entities. Building
/// complex/procedural scenes is usually easier to express as "regenerate
/// everything from scratch" than as an incremental diff, so this tears down
/// the whole previous projection and reprojects the new stage — safely:
///
/// * Only the stage-root entity is despawned. Bevy's despawn is recursive
///   and every prim entity is a (transitive) `ChildOf` the root, so this
///   removes the entire old subtree in one call — no double-despawn of
///   entities reached twice via unordered iteration.
/// * The new stage is projected immediately via `project_stage`, in this
///   same system, instead of waiting for `project_on_load_system` to notice
///   it next frame. That avoids a later system despawning the fresh
///   entities again before they ever reach render extraction.
fn rebuild_usd(world: &mut World) -> Result<()> {
    if !std::mem::take(&mut world.resource_mut::<NeedsRebuild>().0) {
        return Ok(());
    }

    if let Some(_old_live) = world.remove_non_send::<LiveStage>() {
        let old_map = world.remove_resource::<PrimEntities>().unwrap_or_default();
        if let Some(root) = old_map.entity("/") {
            world.despawn(root);
        }
    }

    let stage = Stage::builder().in_memory("scene.usda")?;
    author_scene(world, &stage)?;

    let live = LiveStage::new(stage);
    let mut map = PrimEntities::default();
    project_stage(world, &live, &mut map);
    world.insert_non_send(live);
    world.insert_resource(map);
    Ok(())
}

/// Press `S` to write the live stage's current state out as a `.usda` file.
fn save_usd_on_key(live: NonSend<LiveStage>, keys: Res<ButtonInput<KeyCode>>) -> Result<()> {
    if keys.just_pressed(KeyCode::KeyS) {
        save_stage_as(&live.stage, "scene.usda")?;
        info!("saved scene.usda");
    }
    Ok(())
}
