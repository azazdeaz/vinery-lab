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
    use crate::elements::util::place::Style;
    use crate::elements::util::testing::{self, STYLES};

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
    ///
    /// Both placement styles, because they reach the stage by different
    /// routes: a prim and a reference each, or one ordering of four parallel
    /// arrays.
    #[test]
    fn generation_is_reproducible() {
        for style in STYLES {
            let export = || {
                testing::grown(VineyardParams::default(), style)
                    .0
                    .root_layer()
                    .export_to_string()
                    .unwrap()
            };
            assert_eq!(export(), export(), "{style:?}");
        }
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

    /// Every prim on the stage, including the abstract prototype library —
    /// `Prim::children` is unfiltered, unlike a default traversal predicate.
    fn every_prim(stage: &openusd::usd::Stage) -> Vec<openusd::sdf::Path> {
        let mut found = Vec::new();
        let mut stack = vec![openusd::sdf::Path::abs_root()];
        while let Some(path) = stack.pop() {
            for child in stage.prim(path).children().unwrap() {
                stack.push(child.path().clone());
                found.push(child.path().clone());
            }
        }
        found
    }

    /// No relationship target may leave the default prim.
    ///
    /// Relationship targets are namespace-mapped through composition arcs, and
    /// a target outside the arc's scope cannot be mapped, so USD drops it. The
    /// failure is invisible from in here: opened directly the stage is perfect,
    /// and only a consumer *referencing* the layer sees the damage — a
    /// `PointInstancer` keeping all its instances and losing every prototype,
    /// drawing nothing. Composition arcs may point anywhere; targets may not.
    ///
    /// This guards the whole stage rather than any one element, because the
    /// rule binds every relationship an element might author later — material
    /// bindings, physics joint bodies, collision groups — not just
    /// `prototypes`.
    ///
    /// Run at both placement styles because `place_instanced` authors the only
    /// relationship on the stage today: reference-placed alone, this walks a
    /// scene with nothing to check.
    #[test]
    fn no_relationship_target_escapes_the_default_prim() {
        let root = crate::stage::ROOT;
        let inside = |p: &str| p == root || p.starts_with(&format!("{root}/"));

        for style in STYLES {
            let (stage, _) = testing::grown(VineyardParams::default(), style);
            let prims = every_prim(&stage);
            assert!(
                prims.iter().any(|p| p.as_str() == crate::stage::PARTS),
                "the walk reaches the abstract prototype library, so a stage whose \
                 only escaping targets live in there cannot pass by being skipped"
            );

            let escapes: Vec<String> = prims
                .iter()
                .flat_map(|path| stage.prim(path.clone()).relationships().unwrap())
                .flat_map(|rel| {
                    let from = rel.path().as_str().to_string();
                    rel.targets()
                        .unwrap()
                        .into_iter()
                        .map(move |t| (from.clone(), t))
                })
                .filter(|(_, target)| !inside(target.as_str()))
                .map(|(from, target)| format!("{from} -> {}", target.as_str()))
                .collect();

            assert!(
                escapes.is_empty(),
                "{style:?}: these targets would be silently dropped when the \
                 layer is referenced:\n  {}",
                escapes.join("\n  ")
            );
        }
    }

    /// The viewer's save key calls this from inside a system of the app it is
    /// already running, so the nesting has to hold: a second `App` with its own
    /// `MinimalPlugins` and its own single `update()`, while the outer one is
    /// mid-schedule.
    ///
    /// Worth a test because the two hazards are silent and process-wide —
    /// re-initialized task pools, and the global `tracing` subscriber
    /// `MinimalPlugins` is chosen to avoid. And because what comes back has to
    /// be the *export* shape even though the app it was called from is
    /// previewing an instanced one.
    #[test]
    fn generate_stage_runs_from_inside_a_running_app() {
        // The exported text rather than the stage, because `Stage` is `!Send`
        // and cannot be a resource — and because text is what the save key
        // ultimately puts on disk.
        #[derive(Resource)]
        struct Saved(String);

        fn save(world: &mut World) {
            let params = VineyardParams::from_world(world);
            let stage = generate_stage(&params).unwrap();
            let usda = stage.root_layer().export_to_string().unwrap();
            world.insert_resource(Saved(usda));
        }

        let previewed = crate::stage::new_stage("outer.usda").unwrap();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(crate::elements::plugin)
            .insert_resource(Style::Instanced)
            .add_systems(Update, save);
        app.world_mut()
            .insert_non_send(LiveStage::new(previewed.clone()));
        app.finish();
        app.cleanup();
        app.update();

        let saved = &app.world().resource::<Saved>().0;
        assert!(
            saved.contains("\"Vine_000\"") && !saved.contains("PointInstancer"),
            "what gets written is reference-placed"
        );
        assert!(
            usd_bevy::authoring::prim_exists(&previewed, "/Vineyard/Planting/Row_000/Vines"),
            "while the stage on screen stays instanced"
        );
    }

    /// The default `generate_stage` gives the export shape, not the viewer's.
    /// Every vine has to arrive with a prim path of its own, whatever the
    /// viewer does to go faster.
    #[test]
    fn the_generated_scene_is_reference_placed() {
        assert_eq!(Style::default(), Style::Referenced);

        let stage = generate_stage(&VineyardParams::default()).unwrap();
        assert!(usd_bevy::authoring::prim_exists(
            &stage,
            "/Vineyard/Planting/Row_000/Vine_000"
        ));
        assert!(
            !stage
                .root_layer()
                .export_to_string()
                .unwrap()
                .contains("PointInstancer"),
            "nothing an Isaac Lab config could key on is hidden in an array"
        );
    }
}

