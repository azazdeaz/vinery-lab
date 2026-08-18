//! Headless, single-cycle scene generation: no window, no renderer, no
//! async runner — just `App::update()` once and then read the `World`.

use bevy::prelude::*;
use openusd::usd::Stage;

use crate::author::author_scene;
use crate::scene::{spawn_scene, SceneParams};

/// Runs one `Startup` + `Update` cycle of a minimal headless app and returns
/// the resulting USD `Stage`.
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
/// asset loading entirely, none of which scene generation needs.
pub fn generate_stage(params: &SceneParams) -> anyhow::Result<Stage> {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(params.clone())
        .add_systems(Startup, spawn_scene);

    // Let plugins finish deferred setup before the first update, as `run()`
    // would have done for us.
    app.finish();
    app.cleanup();
    app.update();

    let stage = Stage::builder().in_memory("scene.usda")?;
    author_scene(app.world_mut(), &stage)?;
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
        let stage = generate_stage(&SceneParams::default()).unwrap();
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
}
