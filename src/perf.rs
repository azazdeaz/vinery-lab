//! TEMPORARY — instrumentation for the slider-drag latency investigation.
//!
//! Two ways in:
//!
//! - [`plugin`], which [`viewer::run`](crate::viewer::run) adds when
//!   [`ENV`] is set, brackets every author system and the `Update` /
//!   `PostUpdate` schedules with marker systems and logs a breakdown on any
//!   frame that actually re-authored something:
//!
//!   ```text
//!   VINERYLAB_PERF=1 cargo run
//!   ```
//!
//! - `bench` (test-only, below) runs the same schedule headlessly with the
//!   render `Assets` present, so the reprojection cost is real, and reports
//!   the same breakdown without needing a window — which is what makes it
//!   usable as a before/after when changing either this crate's authoring or
//!   `usd_bevy`'s projection:
//!
//!   ```text
//!   cargo test perf::bench -- --ignored --nocapture
//!   ```
//!
//! The marks are wall-clock deltas between systems, so each one is "whatever
//! ran since the previous mark" rather than a true per-system total. That is
//! precise enough here because every author system holds the non-send
//! `LiveStage` and so runs on the main thread in a fixed order.
//!
//! Delete this module, its `mod perf;` line and the `perf::plugin` block in
//! `viewer::run` when it has outlived its use.

use std::time::Instant;

use bevy::prelude::*;

use crate::elements::util::{parcel, planting};
use crate::elements::{Grow, leaf, pole, shoot, terrain, vine};

/// Set this (to anything) to turn the viewer's instrumentation on.
pub const ENV: &str = "VINERYLAB_PERF";

/// Elapsed-time marks for the frame in progress.
#[derive(Resource)]
pub struct Perf {
    last: Instant,
    pub marks: Vec<(&'static str, f64)>,
    pub frames: u64,
    /// Params resources marked changed this frame — the gate every author
    /// system's `run_if` reads.
    pub changed: String,
}

impl Default for Perf {
    fn default() -> Self {
        Self {
            last: Instant::now(),
            marks: Vec::new(),
            frames: 0,
            changed: String::new(),
        }
    }
}

impl Perf {
    fn mark(&mut self, label: &'static str) {
        let now = Instant::now();
        self.marks
            .push((label, (now - self.last).as_secs_f64() * 1e3));
        self.last = now;
    }

    fn reset(&mut self) {
        self.marks.clear();
        self.last = Instant::now();
        self.frames += 1;
    }

    /// Total of the marks whose label starts with `prefix`.
    pub fn total(&self, prefix: &str) -> f64 {
        self.marks
            .iter()
            .filter(|(l, _)| l.starts_with(prefix))
            .map(|(_, ms)| ms)
            .sum()
    }

    pub fn line(&self) -> String {
        self.marks
            .iter()
            .filter(|(_, ms)| *ms > 0.05)
            .map(|(l, ms)| format!("{l} {ms:.1}ms"))
            .collect::<Vec<_>>()
            .join("  ")
    }
}

fn mark(label: &'static str) -> impl Fn(ResMut<Perf>) + Clone + Send + Sync + 'static {
    move |mut perf: ResMut<Perf>| perf.mark(label)
}

fn reset(mut perf: ResMut<Perf>) {
    perf.reset();
}

/// Brackets every author system, plus the `Update` and `PostUpdate`
/// schedules. The marks are deltas, so each one is the cost of whatever ran
/// since the previous mark.
pub fn plugin(app: &mut App) {
    // The marks are wall-clock points inside the schedule, so they only
    // attribute cost to one system if nothing else runs alongside them.
    app.edit_schedule(PreUpdate, |s| {
        s.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    });
    app.init_resource::<Perf>()
        // `Grow::Prototypes` holds four author systems that are unordered
        // relative to each other, so pin an order here — otherwise a mark
        // cannot sit between two of them.
        .add_systems(
            PreUpdate,
            (
                reset.before(Grow::Prototypes),
                mark("author:leaf")
                    .after(leaf::author_prototypes)
                    .before(shoot::author_prototypes),
                mark("author:shoot")
                    .after(shoot::author_prototypes)
                    .before(vine::author_prototypes),
                mark("author:vine")
                    .after(vine::author_prototypes)
                    .before(pole::author_prototypes),
                mark("author:pole")
                    .after(pole::author_prototypes)
                    .before(terrain::author),
                mark("author:terrain")
                    .after(terrain::author)
                    .before(parcel::author),
                mark("author:parcel")
                    .after(parcel::author)
                    .before(planting::author),
                mark("author:planting").after(planting::author),
            ),
        )
        // Schedule-level brackets, so no ordering against `usd_bevy`'s
        // private systems is needed: `RunFixedMainLoop` runs between
        // `PreUpdate` and `Update`, so the next mark covers all of `Update`
        // — which is where `LiveStagePlugin` reprojects.
        .add_systems(bevy::app::RunFixedMainLoop, mark("author:tail"))
        .add_systems(PostUpdate, mark("update(reproject)"))
        .add_systems(Last, (mark("postupdate"), report).chain())
        .add_systems(Last, note_changed_params.before(report));
}

/// Which params resources are marked changed as of `Last`.
///
/// The author systems are gated on exactly these, so this says *why* a frame
/// re-authored — including the case nobody expects, where a resource is being
/// touched every frame by something other than a slider.
#[allow(clippy::too_many_arguments)]
fn note_changed_params(
    mut perf: ResMut<Perf>,
    terrain_p: Res<terrain::TerrainParams>,
    parcel_p: Res<parcel::ParcelParams>,
    planting_p: Res<planting::PlantingParams>,
    pole_p: Res<pole::PoleParams>,
    vine_p: Res<vine::VineParams>,
    shoot_p: Res<shoot::ShootParams>,
    leaf_p: Res<leaf::LeafParams>,
    ground: Res<terrain::Ground>,
    layout: Res<parcel::VineyardLayout>,
) {
    let flags: [(&'static str, bool); 9] = [
        ("TerrainParams", terrain_p.is_changed()),
        ("ParcelParams", parcel_p.is_changed()),
        ("PlantingParams", planting_p.is_changed()),
        ("PoleParams", pole_p.is_changed()),
        ("VineParams", vine_p.is_changed()),
        ("ShootParams", shoot_p.is_changed()),
        ("LeafParams", leaf_p.is_changed()),
        ("Ground", ground.is_changed()),
        ("VineyardLayout", layout.is_changed()),
    ];
    perf.changed = flags
        .iter()
        .filter(|(_, c)| *c)
        .map(|(n, _)| *n)
        .collect::<Vec<_>>()
        .join(",");
}

/// Logs the breakdown on frames that actually re-authored something.
fn report(perf: Res<Perf>) {
    let authored = perf.total("author:");
    if authored < 0.5 {
        return;
    }
    let total: f64 = perf.marks.iter().map(|(_, ms)| ms).sum();
    info!(
        "frame {}: {total:.1}ms total | author {authored:.1}ms | changed [{}] | {}",
        perf.frames,
        perf.changed,
        perf.line()
    );
}

#[cfg(test)]
mod bench {
    use super::*;
    use crate::elements::util::place;
    use usd_bevy::live::{LiveStage, LiveStagePlugin, PrimEntities};

    /// The viewer's app, headless: same plugins, same `Style::Instanced`, and
    /// the render `Assets` present so the mesh/instancer routes actually
    /// build geometry instead of bailing out.
    fn viewer_like() -> App {
        let stage = crate::stage::new_stage("perf.usda").unwrap();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            // Errors only: the breakdown goes to stdout, and Bevy's startup
            // chatter would bury it. Widen the filter (e.g. `usd_bevy=debug`)
            // when the projection side needs a closer look.
            .add_plugins(bevy::log::LogPlugin {
                filter: "error".to_string(),
                level: bevy::log::Level::ERROR,
                ..default()
            })
            .add_plugins(AssetPlugin::default())
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            // As the viewer has it: `UsdPlugin` is what installs the mesh
            // intern cache, so leaving it out would measure a slower app than
            // the one being debugged.
            .add_plugins(usd_bevy::UsdPlugin)
            .add_plugins(LiveStagePlugin)
            .add_plugins(crate::elements::plugin)
            .add_plugins(super::plugin)
            .insert_resource(place::Style::Instanced);
        app.world_mut().insert_non_send(LiveStage::new(stage));
        app.finish();
        app.cleanup();
        app
    }

    /// What the projection produced. The entity count is the number that
    /// matters most: an instancer despawns and respawns all of them on every
    /// re-projection, so it is the floor under a param change that only moved
    /// where things stand.
    fn scene_size(app: &mut App) -> String {
        let prims = app.world().resource::<PrimEntities>().iter().count();
        let live = app.world_mut().query::<Entity>().iter(app.world()).count();
        let meshes = app.world().resource::<Assets<Mesh>>().len();
        let materials = app.world().resource::<Assets<StandardMaterial>>().len();
        let interned = app
            .world()
            .resource::<usd_bevy::route::cache::ProjectionCache>()
            .len();
        let mut q = app
            .world_mut()
            .query::<&usd_bevy::route::instancer::UsdInstance>();
        let instances = q.iter(app.world()).count();
        format!(
            "{prims} prims, {live} entities ({instances} instancer instances), \
             {meshes} mesh assets ({interned} interned), {materials} material assets"
        )
    }

    fn breakdown(app: &App) -> String {
        let perf = app.world().resource::<Perf>();
        let total: f64 = perf.marks.iter().map(|(_, ms)| ms).sum();
        format!(
            "{total:7.1}ms total | changed [{}] | {}",
            perf.changed,
            perf.line()
        )
    }

    /// Not an assertion — a measurement. Run with
    /// `cargo test perf::bench -- --ignored --nocapture`.
    #[test]
    #[ignore = "measurement, not a test"]
    fn a_param_change_costs() {
        let mut app = viewer_like();

        app.update();
        println!("\ninitial build:       {}", breakdown(&app));
        println!("scene:               {}\n", scene_size(&mut app));

        // Several, not one: a resource written during `PreUpdate` is still
        // "changed" to a run condition evaluated the following frame, so the
        // authoring settles a couple of frames after the last edit. The last
        // of these is the real floor.
        for i in 0..4 {
            app.update();
            println!("idle frame {i}:        {}", breakdown(&app));
        }
        println!();

        // Each of these is a slider a user would drag. They are deliberately
        // spread across the dependency graph: a leaf param re-authors
        // everything downstream of it, while a planting param re-places only.
        let nudges: Vec<(&str, fn(&mut World))> = vec![
            ("leaf.detail", |w| {
                w.resource_mut::<leaf::LeafParams>().detail += 1;
            }),
            ("shoot.length", |w| {
                w.resource_mut::<shoot::ShootParams>().length += 0.01;
            }),
            ("vine.trunk_radius", |w| {
                w.resource_mut::<vine::VineParams>().trunk_radius += 0.001;
            }),
            ("planting.seed", |w| {
                w.resource_mut::<planting::PlantingParams>().seed += 1;
            }),
            ("parcel.row_spacing", |w| {
                w.resource_mut::<parcel::ParcelParams>().row_spacing += 0.01;
            }),
            ("terrain.<any>", |w| {
                w.resource_mut::<terrain::TerrainParams>().set_changed();
            }),
        ];

        for (name, nudge) in nudges {
            nudge(app.world_mut());
            app.update();
            println!("{name:<20} {}", breakdown(&app));
            // Back to quiet before the next one, so each row is one edit's
            // cost and not the tail of the previous one.
            for _ in 0..3 {
                app.update();
            }
        }
        println!("\nscene:               {}\n", scene_size(&mut app));
    }
}
