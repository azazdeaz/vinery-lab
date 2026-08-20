//! Headless, single-cycle scene generation: no window, no renderer, no
//! async runner — just `App::update()` once and then read the stage.

use bevy::prelude::*;
use openusd::usd::Stage;
use usd_bevy::live::LiveStage;

use crate::elements::VineyardParams;

/// Runs one cycle of a minimal headless app and returns the authored USD
/// `Stage`.
///
/// Deliberately uses `App::update()` instead of `App::run()`: `run()` hands
/// off to a runner (which, with a windowed app, never returns and may call
/// `process::exit`) — exactly what we want to avoid when calling this from
/// Python. `update()` just runs the schedule once, synchronously, and
/// returns control to the caller.
///
/// Also deliberately uses `MinimalPlugins` rather than `DefaultPlugins`:
/// `DefaultPlugins` pulls in `LogPlugin`, which installs a *global*
/// `tracing` subscriber — calling this function twice in one process would
/// panic on the second call. `MinimalPlugins` skips windowing, rendering and
/// asset loading entirely, none of which authoring needs.
///
/// `LiveStagePlugin` is left out too: the stage is the output here, so
/// there's nothing to project it into. The `LiveStage` wrapper is still used
/// (it holds the stage for the author systems, which take it either way), it
/// just never gets drained.
pub fn generate_stage(params: &VineyardParams) -> anyhow::Result<Stage> {
    let stage = crate::stage::new_stage("scene.usda")?;

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(crate::elements::plugin);
    // After the element plugins, so these override their defaults.
    params.clone().insert(app.world_mut());
    app.world_mut().insert_non_send(LiveStage::new(stage.clone()));

    // Let plugins finish deferred setup before the first update, as `run()`
    // would have done for us.
    app.finish();
    app.cleanup();
    app.update();

    Ok(stage)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The only export target is robotics simulation (Isaac Lab / ROS,
    /// REP-103 right-handed Z-up), so the generated stage must declare that
    /// convention itself rather than relying on a consumer to correct for
    /// Y-up — `upAxis`/`metersPerUnit` don't compose through references, so
    /// an unauthored (default Y-up) root stage would be silently wrong.
    #[test]
    fn generated_stage_declares_z_up_meters() {
        let stage = generate_stage(&VineyardParams::default()).unwrap();
        assert!(
            matches!(
                stage.stage_metadata("upAxis").unwrap(),
                Some(openusd::sdf::Value::Token(t)) if t.as_str() == "Z"
            ),
            "generated stage must author upAxis = Z"
        );
        assert!(
            matches!(
                stage.stage_metadata("metersPerUnit").unwrap(),
                Some(openusd::sdf::Value::Double(d)) if (d - 1.0).abs() < 1e-9
            ),
            "generated stage must author metersPerUnit = 1.0"
        );
    }

    /// The generated stage must be byte-identical across runs for the same
    /// params, so a downstream sim gets a reproducible scene.
    #[test]
    fn generation_is_reproducible() {
        let params = VineyardParams::default();
        let export = || {
            generate_stage(&params)
                .unwrap()
                .root_layer()
                .export_to_string()
                .unwrap()
        };
        assert_eq!(export(), export());
    }

    #[test]
    fn generated_stage_has_a_default_prim() {
        let stage = generate_stage(&VineyardParams::default()).unwrap();
        let usda = stage.root_layer().export_to_string().unwrap();
        assert!(
            usda.contains("defaultPrim = \"Vineyard\""),
            "got:\n{usda}"
        );
    }
}
