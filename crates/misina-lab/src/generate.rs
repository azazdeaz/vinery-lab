//! Headless, single-cycle scene generation: no window, no renderer, no
//! async runner — just `App::update()` once, then read the scene graph out.

use bevy::prelude::*;

use crate::scene::doc::SceneDoc;
use crate::scene::export::scene_doc;
use crate::{Generator, Params};

/// Runs one cycle of a minimal headless app and returns the scene document the
/// Python USD builder takes.
///
/// The export path: hand the JSON to `python -m {PACKAGE}.usd` — or to its
/// `build_usd` directly — and it becomes a stage.
pub fn scene<G: Generator>(params: &G::Params) -> anyhow::Result<SceneDoc> {
    scene_doc(grown::<G>(params.clone()).world_mut())
}

/// One build cycle, handing back the app to read the scene graph out of — and
/// the resources it was built from, which is what a placement check in a test
/// is asserted against.
///
/// Deliberately `App::update()` rather than `App::run()`: `run()` hands off to
/// a runner (which, with a windowed app, never returns and may call
/// `process::exit`) — exactly what to avoid when calling this from Python.
/// `update()` runs the schedule once, synchronously, and returns.
pub fn grown<G: Generator>(params: G::Params) -> App {
    let mut app = scene_app::<G>();
    app.add_plugins(G::plugin);
    // After the element plugins, so these override their defaults.
    params.apply(app.world_mut());

    // Let plugins finish deferred setup before the first update, as `run()`
    // would have done for us.
    app.finish();
    app.cleanup();
    app.update();
    app
}

/// A headless app with the scene root, the mesh library and asset storage in
/// place — everything an element's build system needs and nothing else.
///
/// Deliberately `MinimalPlugins` rather than `DefaultPlugins`: `DefaultPlugins`
/// pulls in `LogPlugin`, which installs a *global* `tracing` subscriber, so a
/// second app in one process — a Python host calling twice, or two tests —
/// would panic. `AssetPlugin` on top of it because meshes and materials are
/// assets and the plugins that normally register them are the render ones,
/// which nothing here needs.
pub fn scene_app<G: Generator>() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .add_plugins(crate::scene::plugin::<G>);
    app
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::doc::FORMAT;
    use crate::testing::fixture::{Boxes, BoxesParams};

    fn json() -> String {
        let doc = scene::<Boxes>(&BoxesParams::default()).expect("the default row generates");
        serde_json::to_string(&doc).expect("the document serializes")
    }

    /// The only export target is robotics simulation (Isaac Lab / ROS,
    /// REP-103 right-handed Z-up), so the document has to carry that
    /// convention itself. `upAxis`/`metersPerUnit` are root-layer-only
    /// metadata that do not compose through references, so a consumer cannot
    /// correct for a stage that got them wrong — and USD's unauthored default
    /// is Y-up, which means silence is not neutral, it is wrong.
    #[test]
    fn the_document_declares_z_up_meters_under_the_generators_root() {
        let doc = scene::<Boxes>(&BoxesParams::default()).unwrap();
        assert_eq!(doc.format, FORMAT);
        assert_eq!(doc.up_axis, "Z");
        assert_eq!(doc.meters_per_unit, 1.0);
        assert_eq!(doc.root.name, Boxes::NAME);
        assert_eq!(doc.parts.len(), 1, "the row built its one part");
    }

    /// Python calls the generator from a host process that may already be
    /// running Bevy, and may call it more than once. Both hazards are silent
    /// and process-wide: re-initialized task pools, and the global `tracing`
    /// subscriber `MinimalPlugins` is chosen to avoid.
    ///
    /// Doubles as the reproducibility check — a downstream sim keys its cache
    /// on these bytes.
    #[test]
    fn generating_twice_in_one_process_gives_the_same_scene() {
        let (once, twice) = (json(), json());
        assert_eq!(once.len(), twice.len(), "the same scene both times");
        assert!(once == twice, "and byte for byte the same");
    }
}
