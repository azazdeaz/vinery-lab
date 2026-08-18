//! `World` -> `Stage` authoring: the pure half of scene generation.
//!
//! This has no dependency on `LiveStage`/`project_stage`, so it works
//! equally from a headless one-shot [`App`](bevy::app::App) (see
//! [`crate::generate`]) and from the interactive viewer, which additionally
//! projects the resulting stage back into entities for rendering.

use bevy::prelude::*;
use openusd::sdf::Value;
use openusd::usd::Stage;
use usd_bevy::authoring::{define_prim, set_attribute};
use usd_bevy::live::author_transform;

use crate::scene::{Cube, CubeIndex};

/// Authors one `Cube_<i>` prim per `(Cube, CubeIndex, Transform)` entity onto
/// `stage`, under `/Cubes`. Entities are sorted by `CubeIndex` first so the
/// output doesn't depend on ECS iteration order.
pub fn author_scene(world: &mut World, stage: &Stage) -> anyhow::Result<()> {
    let mut cubes: Vec<(u32, f32, Vec3)> = {
        let mut q = world.query::<(&Cube, &CubeIndex, &Transform)>();
        q.iter(world)
            .map(|(cube, index, transform)| (index.0, cube.size, transform.translation))
            .collect()
    };
    cubes.sort_by_key(|(i, ..)| *i);

    define_prim(stage, "/Cubes", "Xform")?;
    stage.set_default_prim("Cubes")?;
    for (i, size, translation) in &cubes {
        let path = format!("/Cubes/Cube_{i}");
        define_prim(stage, &path, "Cube")?;
        set_attribute(stage, &path, "size", "double", Value::Double(*size as f64))?;
        author_transform(stage, &path, &Transform::from_translation(*translation))?;
    }
    Ok(())
}
