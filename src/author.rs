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

/// Authors `upAxis = "Z"` and `metersPerUnit = 1.0` on `stage`'s root layer.
///
/// The only export target is robotics simulation (Isaac Lab / ROS, both
/// REP-103 right-handed Z-up), so the stage is authored natively in that
/// convention rather than Bevy's Y-up — `upAxis`/`metersPerUnit` are
/// root-layer-only stage metadata that doesn't compose through references or
/// payloads, so declaring Y-up here and hoping a consumer corrects for it on
/// import is not an option; USD also defaults to Y-up when `upAxis` is
/// unauthored, so leaving it unset would silently declare the wrong
/// convention. `usd_bevy`'s viewer projection already rotates a Z-up
/// stage-root onto Bevy's Y-up world (see `stage_up_axis` in
/// `usd_bevy::live`), so this scene still renders upright in the viewer.
///
/// Goes through [`Stage::layer_mut`], the documented escape hatch for
/// editing a layer directly, because `upAxis`/`metersPerUnit` have no
/// dedicated `Stage` setter (unlike `defaultPrim`).
fn set_up_axis_z_meters(stage: &Stage) -> anyhow::Result<()> {
    let root_id = stage.root_layer().identifier().to_string();
    let mut layer = stage
        .layer_mut(&root_id)
        .ok_or_else(|| anyhow::anyhow!("stage root layer `{root_id}` not found"))?;
    layer.edit(|edit| {
        edit.pseudo_root_mut()?.set("upAxis", Value::token("Z"));
        edit.pseudo_root_mut()?.set("metersPerUnit", Value::Double(1.0));
        Ok(())
    })?;
    Ok(())
}

/// Authors one `Cube_<i>` prim per `(Cube, CubeIndex, Transform)` entity onto
/// `stage`, under `/Cubes`. Entities are sorted by `CubeIndex` first so the
/// output doesn't depend on ECS iteration order.
pub fn author_scene(world: &mut World, stage: &Stage) -> anyhow::Result<()> {
    set_up_axis_z_meters(stage)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_up_axis_z_meters_authors_root_layer_metadata() {
        let stage = Stage::builder().in_memory("up_axis_test.usda").unwrap();
        set_up_axis_z_meters(&stage).unwrap();

        assert!(
            matches!(
                stage.stage_metadata("upAxis").unwrap(),
                Some(Value::Token(t)) if t.as_str() == "Z"
            ),
            "upAxis authored as Z"
        );
        assert!(
            matches!(
                stage.stage_metadata("metersPerUnit").unwrap(),
                Some(Value::Double(d)) if (d - 1.0).abs() < 1e-9
            ),
            "metersPerUnit authored as 1.0"
        );

        // Round-trips through export, since it's root-layer metadata (not
        // tied to the in-memory cache).
        let usda = stage.root_layer().export_to_string().unwrap();
        assert!(usda.contains("upAxis = \"Z\""), "export contains upAxis, got:\n{usda}");
    }
}
