//! Headless, single-cycle scene generation: no window, no renderer, no
//! async runner — just `App::update()` once, then read the scene graph out.

use bevy::prelude::*;

use crate::elements::VineyardParams;
use crate::scene::doc::SceneDoc;
use crate::scene::export::scene_doc;

/// Runs one cycle of a minimal headless app and returns the scene document the
/// Python USD builder takes.
///
/// The export path: hand the JSON to `python -m vinerylab.usd` — or to
/// [`build_usd`](../python/vinerylab/usd/build.py) directly — and it becomes a
/// stage.
pub fn generate_scene(params: &VineyardParams) -> anyhow::Result<SceneDoc> {
    scene_doc(grow(params)?.world_mut())
}

/// One build cycle, handing back the app to read the scene graph out of.
///
/// Deliberately `App::update()` rather than `App::run()`: `run()` hands off to
/// a runner (which, with a windowed app, never returns and may call
/// `process::exit`) — exactly what to avoid when calling this from Python.
/// `update()` runs the schedule once, synchronously, and returns.
///
/// Deliberately `MinimalPlugins` rather than `DefaultPlugins` too:
/// `DefaultPlugins` pulls in `LogPlugin`, which installs a *global* `tracing`
/// subscriber, so calling this twice in one process would panic on the second
/// call. `AssetPlugin` on top of it because meshes and materials are assets and
/// the plugins that normally register them are the render ones, which nothing
/// here needs.
fn grow(params: &VineyardParams) -> anyhow::Result<App> {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .add_plugins((crate::scene::plugin, crate::elements::plugin));
    // After the element plugins, so these override their defaults.
    params.clone().insert(app.world_mut());

    // Let plugins finish deferred setup before the first update, as `run()`
    // would have done for us.
    app.finish();
    app.cleanup();
    app.update();

    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::doc::{FORMAT, Node};
    use std::collections::{BTreeMap, BTreeSet};

    /// Writes the current scene document out, for eyeballing the export or
    /// building a stage from it by hand:
    ///
    /// ```text
    /// cargo test dump_scene -- --ignored --nocapture
    /// python -m vinerylab.usd scene.json scene.usda --force
    /// ```
    ///
    /// A dev tool rather than a test, and `#[ignore]`d for the same reason
    /// [`perf::bench`](crate::perf) is: it writes a file and asserts nothing.
    #[test]
    #[ignore]
    fn dump_scene() {
        let doc = generate_scene(&VineyardParams::default()).unwrap();
        let json = serde_json::to_string_pretty(&doc).unwrap();
        std::fs::write("scene.json", &json).unwrap();
        println!(
            "wrote scene.json: {} parts, {} bytes",
            doc.parts.len(),
            json.len()
        );
    }

    fn scene() -> SceneDoc {
        generate_scene(&VineyardParams::default()).expect("the default parcel generates")
    }

    /// Every prim in the document, depth first.
    fn walk<'a>(node: &'a Node, into: &mut Vec<(String, &'a Node)>, at: &str) {
        let path = format!("{at}/{}", node.name);
        for child in &node.children {
            walk(child, into, &path);
        }
        into.push((path, node));
    }

    fn prims(doc: &SceneDoc) -> Vec<(String, &Node)> {
        let mut found = Vec::new();
        walk(&doc.root, &mut found, "");
        found
    }

    /// The only export target is robotics simulation (Isaac Lab / ROS,
    /// REP-103 right-handed Z-up), so the document has to carry that
    /// convention itself. `upAxis`/`metersPerUnit` are root-layer-only
    /// metadata that do not compose through references, so a consumer cannot
    /// correct for a stage that got them wrong — and USD's unauthored default
    /// is Y-up, which means silence is not neutral, it is wrong.
    #[test]
    fn the_document_declares_z_up_meters() {
        let doc = scene();
        assert_eq!(doc.format, FORMAT);
        assert_eq!(doc.up_axis, "Z");
        assert_eq!(doc.meters_per_unit, 1.0);
    }

    /// Nothing may reach a consumer untinted. `displayColor` is the one
    /// channel both consumers read — the viewer draws it and USD carries it —
    /// so a part without one renders grey everywhere.
    ///
    /// And no two parts *of one layer* may share a shade: two different
    /// elements landing on the same colour is a palette choice, while two
    /// meshes of one element landing on it is a jitter stream wired to a
    /// constant seed, which every other check here would pass.
    #[test]
    fn every_part_is_tinted_and_no_layer_repeats_a_shade() {
        let doc = scene();
        assert!(doc.parts.len() > 5, "the walk found parts to check");

        let mut by_layer: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for part in &doc.parts {
            assert!(
                part.display_color.iter().any(|c| *c > 0.0),
                "{} is untinted",
                part.name
            );
            let layer = part.name.rsplit_once('_').expect("<Layer>_<index>").0;
            by_layer.entry(layer).or_default().push(&part.name);
        }

        for (layer, names) in &by_layer {
            let shades: BTreeSet<[u32; 3]> = names
                .iter()
                .map(|name| {
                    let part = doc.parts.iter().find(|p| &p.name == name).unwrap();
                    part.display_color.map(|c| c.to_bits())
                })
                .collect();
            assert_eq!(
                shades.len(),
                names.len(),
                "{layer}: two of {names:?} came out the same shade"
            );
        }
    }

    /// Every reference has to resolve, and a referencing prim has to be a leaf
    /// of the tree. Both are silent failures in USD: a dangling reference
    /// composes to an empty prim, and an `instanceable` prim's authored
    /// children are simply unreachable.
    #[test]
    fn every_reference_resolves_to_a_part_and_carries_no_children() {
        let doc = scene();
        let parts: BTreeSet<&str> = doc.parts.iter().map(|p| p.name.as_str()).collect();

        let prims = prims(&doc);
        let referencing = prims.iter().filter(|(_, n)| n.reference.is_some()).count();
        assert!(referencing > 1000, "the scene draws geometry, got {referencing}");

        for (path, node) in &prims {
            let Some(reference) = &node.reference else {
                continue;
            };
            assert!(parts.contains(reference.as_str()), "{path} draws a missing {reference}");
            assert!(node.children.is_empty(), "{path} references and has children");
            assert!(node.instanceable, "{path} references without being instanceable");
        }
    }

    /// A downstream Isaac Lab config is keyed on prim paths, so two prims may
    /// never share one — a name collision would silently repoint it.
    #[test]
    fn every_prim_path_is_unique() {
        let doc = scene();
        let prims = prims(&doc);
        let paths: BTreeSet<&str> = prims.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths.len(), prims.len(), "some prim path repeats");
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
        let once = serde_json::to_string(&scene()).unwrap();
        let twice = serde_json::to_string(&scene()).unwrap();
        assert_eq!(once.len(), twice.len(), "the same scene both times");
        assert!(once == twice, "and byte for byte the same");
    }
}
