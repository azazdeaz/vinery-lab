//! Stage lifecycle: building the in-memory stage and authoring the
//! root-layer metadata that isn't any single element's business.
//!
//! Elements own prim subtrees; nobody owns stage metadata, so it lives here
//! and is applied once at creation. Both entry points ([`crate::viewer`] and
//! [`crate::generate`]) start from [`new_stage`], so they share one
//! convention.

use openusd::schemas::geom::{Imageable, Scope};
use openusd::sdf::Value;
use openusd::usd::Stage;

/// Root of the prototype library. Elements author their reusable geometry
/// under `/parts/<Element>` and instance each other's by path.
pub const PARTS: &str = "/parts";

/// Creates an empty in-memory stage with the project's coordinate convention
/// and prototype library root already in place.
pub fn new_stage(identifier: &str) -> anyhow::Result<Stage> {
    let stage = Stage::builder().in_memory(identifier)?;
    set_up_axis_z_meters(&stage)?;
    define_parts_library(&stage)?;
    Ok(stage)
}

/// Defines `/parts` and hides it.
///
/// Prototypes are ordinary defined prims, so the viewer would otherwise
/// project them as a pile of stray geometry sitting at the origin alongside
/// the real scene. Instances are unaffected: they hang off their instancer,
/// not off `/parts`, so Bevy's visibility inheritance never reaches them.
fn define_parts_library(stage: &Stage) -> anyhow::Result<()> {
    let parts = Scope::define(stage, openusd::sdf::path(PARTS)?)?;
    parts
        .create_visibility_attr()?
        .set(Value::token("invisible"))?;
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stage_authors_root_layer_metadata() {
        let stage = new_stage("up_axis_test.usda").unwrap();

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
        assert!(
            usda.contains("upAxis = \"Z\""),
            "export contains upAxis, got:\n{usda}"
        );
    }

    #[test]
    fn new_stage_hides_the_parts_library() {
        let stage = new_stage("parts_test.usda").unwrap();
        let usda = stage.root_layer().export_to_string().unwrap();

        assert!(usda.contains("def Scope \"parts\""), "got:\n{usda}");
        assert!(
            usda.contains("token visibility = \"invisible\""),
            "got:\n{usda}"
        );
        assert!(
            !usda.contains("custom token visibility"),
            "visibility is a UsdGeomImageable attribute, not a custom one; got:\n{usda}"
        );
    }
}
