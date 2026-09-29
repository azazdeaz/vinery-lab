//! Where a vineyard rebuild's time goes, layer by layer: the marks
//! [`misina_lab::perf`] reports, placed between this generator's build
//! systems, and the bench that reads them headlessly.
//!
//! `VINERYLAB_PERF=1 cargo run` turns on both halves — see `main.rs`. The
//! bench runs the same schedule with the asset storage present, so the
//! geometry really gets built, and reports the same breakdown without a
//! window:
//!
//! ```text
//! cargo test perf::bench -- --ignored --nocapture
//! ```

use bevy::prelude::*;
use misina_lab::perf::{Perf, mark, report};

use crate::elements::util::{parcel, planting};
use crate::elements::{SceneParams, cover, leaf, pole, shoot, terrain, vine, weed, wire};

/// A mark after every build system, and the note of which params changed
/// that the report reads. Added beside [`misina_lab::perf::plugin`].
pub fn plugin(app: &mut App) {
    app.add_systems(
        PreUpdate,
        (
            mark("author:terrain")
                .after(terrain::build)
                .before(parcel::author),
            mark("author:parcel")
                .after(parcel::author)
                .before(planting::plant),
            mark("author:planting")
                .after(planting::plant)
                .before(pole::build),
            mark("author:pole").after(pole::build).before(vine::build),
            mark("author:vine").after(vine::build).before(shoot::build),
            mark("author:shoot").after(shoot::build).before(leaf::build),
            // The floor layers share `Grow::Scatter` with the leaves and
            // have no order among themselves; the marks impose one.
            mark("author:leaf").after(leaf::build).before(cover::author),
            mark("author:cover")
                .after(cover::build)
                .before(weed::author),
            mark("author:weed").after(weed::build),
        ),
    )
    .add_systems(Last, note_changed_params.before(report));
}

/// Which params resources are marked changed as of `Last`.
///
/// The build systems are gated on exactly these, so this says *why* a frame
/// re-authored — including the case nobody expects, where a resource is being
/// touched every frame by something other than a slider.
#[allow(clippy::too_many_arguments)]
fn note_changed_params(
    mut perf: ResMut<Perf>,
    scene_p: Res<SceneParams>,
    terrain_p: Res<terrain::TerrainParams>,
    parcel_p: Res<parcel::ParcelParams>,
    planting_p: Res<planting::PlantingParams>,
    pole_p: Res<pole::PoleParams>,
    wire_p: Res<wire::WireParams>,
    vine_p: Res<vine::VineParams>,
    shoot_p: Res<shoot::ShootParams>,
    leaf_p: Res<leaf::LeafParams>,
    cover_p: Res<cover::CoverParams>,
    weed_p: Res<weed::WeedParams>,
    ground: Res<terrain::Ground>,
    layout: Res<parcel::VineyardLayout>,
) {
    let flags: [(&'static str, bool); 13] = [
        ("SceneParams", scene_p.is_changed()),
        ("TerrainParams", terrain_p.is_changed()),
        ("ParcelParams", parcel_p.is_changed()),
        ("PlantingParams", planting_p.is_changed()),
        ("PoleParams", pole_p.is_changed()),
        ("WireParams", wire_p.is_changed()),
        ("VineParams", vine_p.is_changed()),
        ("ShootParams", shoot_p.is_changed()),
        ("LeafParams", leaf_p.is_changed()),
        ("CoverParams", cover_p.is_changed()),
        ("WeedParams", weed_p.is_changed()),
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

#[cfg(test)]
mod bench {
    use super::*;
    use crate::Vineyard;

    /// The viewer's app, headless: the same plugins, with the asset storage
    /// present so the build systems actually produce geometry.
    fn viewer_like() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            // Errors only: the breakdown goes to stdout, and Bevy's startup
            // chatter would bury it.
            .add_plugins(bevy::log::LogPlugin {
                filter: "error".to_string(),
                level: bevy::log::Level::ERROR,
                ..default()
            })
            .add_plugins(AssetPlugin::default())
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .add_plugins(misina_lab::scene::plugin::<Vineyard>)
            .add_plugins(crate::elements::plugin)
            .add_plugins((misina_lab::perf::plugin, super::plugin));
        app.finish();
        app.cleanup();
        app
    }

    /// What the build produced. The entity count matters most: a layer
    /// despawns and respawns everything below it on every rebuild, so it is
    /// the floor under a param change that only moved where things stand.
    fn scene_size(app: &mut App) -> String {
        let entities = app.world_mut().query::<Entity>().iter(app.world()).count();
        let parts = app
            .world()
            .resource::<misina_lab::scene::Prototypes>()
            .len();
        let meshes = app.world().resource::<Assets<Mesh>>().len();
        let materials = app.world().resource::<Assets<StandardMaterial>>().len();
        format!(
            "{entities} entities, {parts} parts, \
             {meshes} mesh assets, {materials} material assets"
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

    /// One more frame, which must not re-author anything.
    ///
    /// Every row of the bench has to be read from a quiet scene, or it
    /// measures the tail of the previous rebuild instead of its own edit. A
    /// rebuild reaches every layer within the frame that triggered it, so one
    /// quiet frame is the whole settle — a second frame of authoring means a
    /// run condition is firing a frame late (see the `or_eager` note in
    /// `terrain::plugin`).
    fn settle(app: &mut App) {
        app.update();
        let authored = app.world().resource::<Perf>().total("author:");
        assert!(authored < 0.5, "rebuilt again: {}", breakdown(app));
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
        settle(&mut app);

        // Each of these is a slider a user would drag. They are deliberately
        // spread across the dependency graph: a leaf param re-authors
        // everything downstream of it, while a planting param re-places only.
        #[expect(
            clippy::type_complexity,
            reason = "a list of named closures, read once"
        )]
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
            ("scene.seed", |w| {
                w.resource_mut::<SceneParams>().seed += 1;
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
            settle(&mut app);
        }
        println!("\nscene:               {}\n", scene_size(&mut app));
    }
}
