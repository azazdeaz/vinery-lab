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
use bevy::ui_widgets::ScrollArea;

use crate::elements::parcel::ui as parcel_ui;
use crate::elements::terrain::ui as terrain_ui;
use crate::elements::vine::ui as vine_ui;

pub fn plugin(app: &mut App) {
    app.add_plugins(FeathersPlugins)
        .insert_resource(UiTheme(create_dark_theme()))
        .add_systems(Startup, params_panel_list.spawn());
}

/// Marks the params panel's root node so other systems (e.g. the viewer
/// camera) can tell whether the pointer is over the panel.
#[derive(Component, Clone, Default)]
pub struct ParamsPanel;

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
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        ParamsPanel
        Interaction
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [ pane() Children [
            pane_header() Children [ (Text("Vineyard") ThemedText) ],
            pane_body() Children [ (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    max_height: vh(75),
                    overflow: Overflow::scroll_y(),
                }
                ScrollArea
                Children [
                    terrain_ui(),
                    parcel_ui(),
                    vine_ui(),
                ]
            ) ],
        ]]
    }
}
