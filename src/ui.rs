//! Feathers-based panel for editing [`SceneParams`] live, docked to the
//! top-left corner of the viewer.
//!
//! Each slider writes straight into the [`SceneParams`] resource on every
//! [`ValueChange`], including mid-drag, so dragging a slider gives instant
//! feedback. [`crate::viewer::run`] wires [`despawn_cubes`] into its own
//! system chain (ahead of the USD rebuild) so the cube grid respawns the
//! moment a value lands in the resource.

use bevy::feathers::{
    FeathersPlugins,
    containers::{pane, pane_body, pane_header},
    controls::FeathersSlider,
    dark_theme::create_dark_theme,
    display::label_small,
    theme::{ThemeBackgroundColor, ThemedText, UiTheme},
    tokens,
};
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};

use crate::scene::{Cube, SceneParams};

pub fn plugin(app: &mut App) {
    app.add_plugins(FeathersPlugins)
        .insert_resource(UiTheme(create_dark_theme()))
        .add_systems(Startup, scene_params_panel_list.spawn());
}

/// Despawns every [`Cube`] entity. Paired with [`crate::scene::spawn_scene`]
/// by [`crate::viewer::run`] to fully rebuild the grid whenever
/// [`SceneParams`] changes.
pub(crate) fn despawn_cubes(mut commands: Commands, cubes: Query<Entity, With<Cube>>) {
    for entity in cubes.iter() {
        commands.entity(entity).despawn();
    }
}

fn scene_params_panel_list() -> impl SceneList {
    bsn_list![scene_params_panel()]
}

fn scene_params_panel() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            top: px(10),
            left: px(10),
            width: px(240),
            padding: px(8),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [ pane() Children [
            pane_header() Children [ (Text("Scene Params") ThemedText) ],
            pane_body() Children [
                label_small("Rows"),
                (
                    @FeathersSlider { @min: 1.0, @max: 50.0, @value: 10.0 }
                    SliderStep(1.0)
                    SliderPrecision(0)
                    on(slider_self_update)
                    on(|change: On<ValueChange<f32>>, mut params: ResMut<SceneParams>| {
                        params.rows = change.value.round().max(1.0) as u32;
                    })
                ),
                label_small("Cols"),
                (
                    @FeathersSlider { @min: 1.0, @max: 50.0, @value: 10.0 }
                    SliderStep(1.0)
                    SliderPrecision(0)
                    on(slider_self_update)
                    on(|change: On<ValueChange<f32>>, mut params: ResMut<SceneParams>| {
                        params.cols = change.value.round().max(1.0) as u32;
                    })
                ),
                label_small("Spacing"),
                (
                    @FeathersSlider { @min: 0.05, @max: 2.0, @value: 0.2 }
                    SliderStep(0.05)
                    SliderPrecision(2)
                    on(slider_self_update)
                    on(|change: On<ValueChange<f32>>, mut params: ResMut<SceneParams>| {
                        params.spacing = change.value;
                    })
                ),
                label_small("Cube size"),
                (
                    @FeathersSlider { @min: 0.01, @max: 1.0, @value: 0.1 }
                    SliderStep(0.01)
                    SliderPrecision(2)
                    on(slider_self_update)
                    on(|change: On<ValueChange<f32>>, mut params: ResMut<SceneParams>| {
                        params.cube_size = change.value;
                    })
                ),
            ],
        ]]
    }
}
