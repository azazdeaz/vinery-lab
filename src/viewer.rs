//! Interactive viewer: a windowed Bevy app that draws the generated scene and
//! lets you edit its parameters, saving on demand.
//!
//! Element build systems spawn ordinary Bevy entities during `PreUpdate` and
//! Bevy renders them directly. Saving exports the same entities as a scene
//! document for the Python builder to author.
//!
//! The scene is authored Z-up and stood upright by the scene root's parent (see
//! [`scene::z_up_to_y_up`](crate::scene)), so the camera below works in Bevy's
//! ordinary Y-up world.

use bevy::camera::Exposure;
#[cfg(not(target_arch = "wasm32"))]
use bevy::input::common_conditions::input_just_pressed;
use bevy::light::{
    Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfigBuilder, GlobalAmbientLight,
    atmosphere::ScatteringMedium, light_consts::lux,
};
use bevy::pbr::AtmosphereSettings;
use bevy::prelude::*;
use bevy_panorbit_camera::{PanOrbitCamera, PanOrbitCameraPlugin};

use crate::elements::util::parcel;
use crate::ui::ParamsPanel;

/// Where the save key writes the scene document.
#[cfg(not(target_arch = "wasm32"))]
const SCENE_PATH: &str = "scene.json";

pub fn run() {
    let mut app = App::new();
    // The sky is the scene's ambient light (see `setup`), so the flat
    // hemisphere-wide term `LightPlugin` inserts would only wash it out. Left
    // in, it also adds a uniform specular lobe to every surface at every angle,
    // which is what makes an unlit scene look like it is all made of one shiny
    // material.
    app.insert_resource(GlobalAmbientLight::NONE);
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                // The web build draws into this canvas and tracks its size.
                // Both fields are ignored off the web.
                canvas: Some("#viewer".into()),
                fit_canvas_to_parent: true,
                ..default()
            }),
            ..default()
        }),
        PanOrbitCameraPlugin,
        crate::scene::plugin,
        crate::elements::plugin,
        crate::ui::plugin,
        // Gizmos need `GizmoPlugin` (from `DefaultPlugins`), which the
        // headless generation path's `MinimalPlugins` doesn't provide —
        // see `parcel::debug_plugin`'s docs for why it's kept separate
        // from `crate::elements::plugin`.
        parcel::debug_plugin,
    ))
    .add_systems(Startup, setup)
    .add_systems(Update, sync_camera_enabled_with_ui);

    // No filesystem on the web, and this system's `Err` would take the app
    // down rather than log it: a `BevyError` defaults to `Severity::Panic`.
    // **Copy Isaac Lab cfg** is the export path there.
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(
        Update,
        save_scene_on_key.run_if(input_just_pressed(KeyCode::KeyS)),
    );

    // Off by default: it logs a line per re-authored frame, which during a
    // slider drag is every frame. See [`crate::perf`].
    if std::env::var_os(crate::perf::ENV).is_some() {
        app.add_plugins(crate::perf::plugin);
    }

    app.run();
}

/// How far from the camera shadows are still drawn, in meters. Past the engine
/// default of 150, which the framing below overruns: the camera starts 114 m
/// out and the far corner of an 80x50 m parcel is another 50 beyond that, so at
/// the default the row furthest from the camera would sit unshadowed. The
/// cascade splits are left alone — they stay fine-grained near the camera,
/// which is what orbiting in to inspect a single vine wants.
const SHADOW_DISTANCE: f32 = 200.0;

fn setup(mut commands: Commands, mut mediums: ResMut<Assets<ScatteringMedium>>) {
    // A physically scattered sky, which is both the backdrop and — through
    // `AtmosphereEnvironmentMapLight` below — the scene's ambient light. It
    // places itself one earth radius under the origin on its own, so the
    // parcel sits on the planet's surface with no transform to author.
    commands.spawn(Atmosphere::earth(
        mediums.add(ScatteringMedium::earth(256, 256)),
    ));

    // Framed for `TerrainParams::default()`'s 80x50m extent, not the 4x4m
    // placeholder scale the defaults used before rows landed.
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(70.0, 55.0, 70.0).looking_at(Vec3::ZERO, Vec3::Y),
        PanOrbitCamera::default(),
        AtmosphereSettings::default(),
        // Lights the scene off the sky instead of off a constant: blue from
        // above, warm bounce from the ground, and a reflection whose spread
        // follows each surface's roughness. Without it every material's
        // roughness is invisible, because a constant ambient term looks the
        // same however wide the lobe sampling it is.
        AtmosphereEnvironmentMapLight::default(),
        // Raw sunlight is orders of magnitude past what a display covers, so
        // the camera stops down to meet it. Tied to the light's illuminance
        // below: raise one and this has to follow.
        Exposure { ev100: 13.0 },
    ));
    commands.spawn((
        DirectionalLight {
            // Off by default, and the single biggest thing between this scene
            // and a lit one: with nothing casting, a canopy has no form.
            shadow_maps_enabled: true,
            // Sunlight *before* the atmosphere filters it, which is what the
            // atmosphere above wants as input. The other `lux` constants
            // already have scattering baked in and would be counted twice.
            illuminance: lux::RAW_SUNLIGHT,
            ..default()
        },
        Transform::from_xyz(40.0, 80.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder {
            maximum_distance: SHADOW_DISTANCE,
            ..default()
        }
        .build(),
    ));
}

/// Press `S` to write the scene out as a document for the Python USD builder.
///
/// Exports the entities on screen: the viewer and the export draw from one
/// scene graph, so there is no preview shape and export shape to keep in step.
#[cfg(not(target_arch = "wasm32"))]
fn save_scene_on_key(world: &mut World) -> Result<()> {
    std::fs::write(SCENE_PATH, crate::scene::export::scene_json(world)?)?;
    info!("saved {SCENE_PATH} — build it with `python -m vinerylab.usd {SCENE_PATH} scene.usd`");
    Ok(())
}

/// Disables orbit/pan/zoom while the pointer is over the params panel, so
/// dragging a slider there doesn't also drag the camera underneath it.
fn sync_camera_enabled_with_ui(
    panel: Query<&Interaction, With<ParamsPanel>>,
    mut cameras: Query<&mut PanOrbitCamera>,
) {
    let over_panel = panel
        .iter()
        .any(|interaction| *interaction != Interaction::None);
    for mut camera in &mut cameras {
        camera.enabled = !over_panel;
    }
}
