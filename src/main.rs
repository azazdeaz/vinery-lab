use bevy::prelude::*;
use openusd::sdf::Value;
use openusd::usd::Stage;
use usd_bevy::UsdPlugin;
use usd_bevy::live::{LiveStage, LiveStagePlugin, PrimEntities, author_transform, project_stage};
use usd_bevy::authoring::{define_prim, save_stage_as, set_attribute};

#[derive(Component)]
struct Cube {
    size: f32,
}

/// Set by `mark_dirty` (a normal system, so `Changed<Transform>` tracks
/// correctly across frames) and consumed by `rebuild_usd` (an exclusive
/// system, which can't take a `Changed` query param directly alongside
/// `&mut World`).
#[derive(Resource, Default)]
struct NeedsRebuild(bool);

fn main() {
    App::new()
    .add_plugins((DefaultPlugins, UsdPlugin, LiveStagePlugin))
    .init_resource::<NeedsRebuild>()
    .add_systems(Startup, (add_cubes, setup))
    .add_systems(Update, (mark_dirty, rebuild_usd, save_usd_on_key).chain())
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


fn add_cubes(mut commands: Commands) {
    for x in 0..10 {
        for y in 0..10 {
            commands.spawn((
                Cube { size: 0.1 }, 
                Transform::from_xyz(x as f32 * 0.2, y as f32 * 0.2, 0.0),
            ));
        }
    }
}

/// Flags `NeedsRebuild` whenever a cube's transform changes. Plain system
/// (not exclusive), so `Changed<Transform>` correctly tracks state across
/// frames via the query's cached `QueryState`.
fn mark_dirty(mut dirty: ResMut<NeedsRebuild>, changed: Query<(), Changed<Transform>>) {
    if !changed.is_empty() {
        dirty.0 = true;
    }
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

    let cubes: Vec<(f32, Vec3)> = {
        let mut q = world.query::<(&Cube, &Transform)>();
        q.iter(world)
            .map(|(cube, transform)| (cube.size, transform.translation))
            .collect()
    };

    if let Some(_old_live) = world.remove_non_send::<LiveStage>() {
        let old_map = world.remove_resource::<PrimEntities>().unwrap_or_default();
        if let Some(root) = old_map.entity("/") {
            world.despawn(root);
        }
    }

    let stage = Stage::builder().in_memory("scene.usda")?;
    define_prim(&stage, "/Cubes", "Xform")?;
    for (i, (size, translation)) in cubes.iter().enumerate() {
        let path = format!("/Cubes/Cube_{i}");
        define_prim(&stage, &path, "Cube")?;
        set_attribute(&stage, &path, "size", "double", Value::Double(*size as f64))?;
        author_transform(&stage, &path, &Transform::from_translation(*translation))?;
    }

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
