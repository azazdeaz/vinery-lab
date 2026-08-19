//! Feathers-based parameter panel, docked to the top-left corner of the
//! viewer.
//!
//! The panel itself owns no controls — it just stacks the UI fragment each
//! element publishes next to its own params and author fn. Adding an element
//! to the panel is one line in [`params_panel`].
//!
//! Sliders write straight into their params resource on every
//! [`ValueChange`](bevy::ui_widgets::ValueChange), including mid-drag, so a
//! drag re-authors that element's subtree every frame it moves.

use bevy::feathers::{
    FeathersPlugins,
    containers::{pane, pane_body, pane_header},
    dark_theme::create_dark_theme,
    theme::{ThemeBackgroundColor, ThemedText, UiTheme},
    tokens,
};
use bevy::prelude::*;

use crate::elements::cube::ui as cube_ui;
use crate::elements::grid::ui as grid_ui;

pub fn plugin(app: &mut App) {
    app.add_plugins(FeathersPlugins)
        .insert_resource(UiTheme(create_dark_theme()))
        .add_systems(Startup, params_panel_list.spawn());
}

fn params_panel_list() -> impl SceneList {
    bsn_list![params_panel()]
}

fn params_panel() -> impl Scene {
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
            pane_header() Children [ (Text("Vineyard") ThemedText) ],
            pane_body() Children [
                grid_ui(),
                cube_ui(),
            ],
        ]]
    }
}
