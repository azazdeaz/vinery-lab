//! Stage lifecycle: building the in-memory stage and authoring the
//! root-layer metadata that isn't any single element's business.
//!
//! Elements own prim subtrees; nobody owns stage metadata, so it lives here
//! and is applied once at creation. Both entry points ([`crate::viewer`] and
//! [`crate::generate`]) start from [`new_stage`], so they share one
//! convention.

use openusd::schemas::geom::Scope;
use openusd::sdf::Value;
use openusd::usd::Stage;

/// The scene root, and the stage's default prim.
///
/// Everything the scene is made of hangs off this one prim — placed geometry
/// and the prototype library alike — so a consumer referencing the generated
/// layer gets a complete asset. See [`define_parts_library`] for why that is
/// not merely tidy.
pub const ROOT: &str = "/Vineyard";

/// Root of the prototype library. Elements author their reusable geometry
/// under `<ROOT>/parts/<Element>` and instance each other's by path.
pub const PARTS: &str = "/Vineyard/parts";

/// Creates an empty in-memory stage with the project's coordinate convention,
/// scene root and prototype library already in place.
pub fn new_stage(identifier: &str) -> anyhow::Result<Stage> {
    let stage = Stage::builder().in_memory(identifier)?;
    set_up_axis_z_meters(&stage)?;
    define_root(&stage)?;
    define_parts_library(&stage)?;
    Ok(stage)
}

/// Defines [`ROOT`] and names it the stage's default prim.
///
/// Runs ahead of [`define_parts_library`] because authoring a prim implicitly
/// creates its missing ancestors as `over` specs: nesting the library under an
/// undefined root would leave `defaultPrim` pointing at a non-defining prim
/// until whichever element got there first upgraded it.
fn define_root(stage: &Stage) -> anyhow::Result<()> {
    usd_bevy::authoring::define_prim(stage, ROOT, "Xform")?;
    stage.set_default_prim("Vineyard")?;
    Ok(())
}

/// Defines the prototype library as a `class`, nested inside [`ROOT`].
///
/// Both halves of that are load-bearing.
///
/// *Inside the root*, because a `PointInstancer`'s `prototypes` is a
/// **relationship**, and relationship targets are namespace-mapped through
/// composition arcs: a target outside the referenced subtree cannot be mapped
/// and is dropped, leaving an instancer holding every instance and no
/// prototypes — which draws nothing, silently, and only once the layer is
/// referenced rather than opened directly. References may leave the default
/// prim; relationship targets may not.
///
/// A *`class`*, because prototypes are otherwise ordinary defined prims and
/// the viewer would project them as a pile of stray geometry at the origin.
/// `class` makes the subtree abstract, which `usd_bevy`'s traversal predicate
/// skips, while instancing still resolves prototypes by path. It replaces a
/// `visibility = "invisible"` opinion that did the same job less well: now
/// that the library composes into every consumer, that opinion would ride
/// along with it.
fn define_parts_library(stage: &Stage) -> anyhow::Result<()> {
    Scope::define(stage, openusd::sdf::path(PARTS)?)?;
    stage.prim(openusd::sdf::path(PARTS)?).set_metadata(
        openusd::sdf::FieldKey::Specifier.as_str(),
        Value::Specifier(openusd::sdf::Specifier::Class),
    )?;
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

    /// `new_stage` owns the scene root, so the layer is a referenceable asset
    /// from the moment it exists — before any element has authored anything.
    #[test]
    fn new_stage_defines_the_root_as_the_default_prim() {
        let stage = new_stage("root_test.usda").unwrap();
        let usda = stage.root_layer().export_to_string().unwrap();

        assert!(
            usda.contains("defaultPrim = \"Vineyard\""),
            "got:\n{usda}"
        );
        assert!(usda.contains("def Xform \"Vineyard\""), "got:\n{usda}");
        assert!(
            matches!(
                stage.prim(openusd::sdf::path(ROOT).unwrap()).specifier(),
                Ok(Some(openusd::sdf::Specifier::Def))
            ),
            "the root is a defining prim, not the `over` an implicit ancestor \
             would have produced"
        );
    }

    /// The library is a `class` nested under the root: abstract so the viewer
    /// skips it, and inside the default prim so `prototypes` relationship
    /// targets survive being referenced.
    #[test]
    fn new_stage_encapsulates_the_parts_library() {
        let stage = new_stage("parts_test.usda").unwrap();
        let usda = stage.root_layer().export_to_string().unwrap();

        assert!(usda.contains("class Scope \"parts\""), "got:\n{usda}");
        assert!(
            !usda.contains("visibility"),
            "`class` replaces the visibility opinion, which would otherwise \
             compose into every consumer; got:\n{usda}"
        );
        assert!(
            PARTS.starts_with(&format!("{ROOT}/")),
            "the library must live under the default prim, got `{PARTS}`"
        );
    }
}
