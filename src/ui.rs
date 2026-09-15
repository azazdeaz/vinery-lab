//! Feathers-based parameter panel, docked to the top-left corner of the
//! viewer.
//!
//! The panel itself owns no controls — it just stacks the UI fragment each
//! element publishes next to its own params and author fn. Adding an element
//! to the panel is one line in [`params_panel`].
//!
//! Sliders fire on every [`ValueChange`](bevy::ui_widgets::ValueChange),
//! including mid-drag, so they write a [`Staged`] copy of the params rather
//! than the resources the build systems read. [`commit`] hands the edit over
//! once the slider has stopped moving, which is what keeps the drag itself
//! smooth: one rebuild per drag instead of one per frame.

use bevy::clipboard::Clipboard;
use bevy::feathers::{
    FeathersPlugins,
    containers::{pane, pane_body, pane_header},
    controls::{ButtonVariant, FeathersButton},
    dark_theme::create_dark_theme,
    theme::{ThemeBackgroundColor, ThemedText, UiTheme},
    tokens,
};
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, ScrollArea};

use crate::elements::{Grow, VineyardParams};

use crate::elements::leaf::ui as leaf_ui;
use crate::elements::pole::ui as pole_ui;
use crate::elements::shoot::ui as shoot_ui;
use crate::elements::terrain::ui as terrain_ui;
use crate::elements::ui as scene_ui;
use crate::elements::util::parcel::ui as parcel_ui;
use crate::elements::util::planting::ui as planting_ui;
use crate::elements::vine::ui as vine_ui;

pub fn plugin(app: &mut App) {
    // Every element plugin is added before this one, so the live params are
    // already there to seed the panel from.
    let live = VineyardParams::from_world(app.world());
    app.add_plugins(FeathersPlugins)
        .insert_resource(UiTheme(create_dark_theme()))
        .insert_resource(Staged(live))
        .add_systems(Startup, params_panel_list.spawn())
        .add_systems(PreUpdate, commit.before(Grow::Terrain));
}

/// How long a staged value has to hold still before it reaches the scene.
///
/// Short enough to feel immediate when the pointer pauses, long enough that no
/// frame of a drag commits.
const QUIET: f32 = 0.15;

/// The panel's copy of every element's params, waiting to be handed over.
///
/// Rebuilding a layer costs tens of milliseconds at parcel scale, so a slider
/// that wrote the live resources would spend a drag rebuilding the scene once
/// per frame. Sliders write this instead and [`commit`] copies it across.
///
/// Viewer-only: nothing but [`plugin`] inserts one, so the headless
/// [`generate`](crate::generate) path and the Python bindings see the live
/// resources they always did.
#[derive(Resource, Deref, DerefMut)]
pub struct Staged(pub VineyardParams);

/// Hands the staged params to the live resources once they have stopped moving.
///
/// Exclusive because it writes all of them; [`VineyardParams::apply`] marks
/// only the fragments that actually differ, so a commit rebuilds the layers
/// the drag touched and no others.
///
/// Runs on `Time<Real>` rather than the default time: this measures how long
/// a pointer has been still, which is wall-clock whatever the scene's clock is
/// doing.
fn commit(world: &mut World, mut still: Local<Option<f32>>) {
    if world.resource_ref::<Staged>().is_changed() {
        *still = Some(0.0);
    } else if let Some(elapsed) = still.as_mut() {
        *elapsed += world.resource::<Time<Real>>().delta_secs();
        if *elapsed >= QUIET {
            *still = None;
            let staged = world.resource::<Staged>().0.clone();
            staged.apply(world);
        }
    }
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
            pane_body() Children [
                (
                    Node {
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4),
                        max_height: vh(75),
                        overflow: Overflow::scroll_y(),
                    }
                    ScrollArea
                    Children [
                        scene_ui(),
                        terrain_ui(),
                        parcel_ui(),
                        pole_ui(),
                        vine_ui(),
                        shoot_ui(),
                        leaf_ui(),
                        planting_ui(),
                    ]
                ),
                // Outside the scroll area, so it stays reachable however far
                // down the panel is scrolled.
                copy_cfg_button(),
            ],
        ]]
    }
}

/// Puts the current parameters on the clipboard as an Isaac Lab config.
///
/// The other half of the workflow the viewer exists for: tune the scene here,
/// paste the result into an environment config there.
fn copy_cfg_button() -> impl Scene {
    bsn! {
        Node { margin: UiRect::top(px(8)) }
        Children [ (
            @FeathersButton {
                @caption: bsn! { Text("Copy Isaac Lab cfg") ThemedText },
                @variant: ButtonVariant::Primary,
            }
            // Off `Staged` rather than the live resources, so the snippet is
            // the panel as it reads right now and not as it read before the
            // last edit settled.
            //
            // Logged as well as copied. The clipboard is the point, but it is
            // the part that can fail for reasons outside the app — no backend
            // on a bare Wayland session, no X11 display — and the snippet is
            // worth more than the error.
            on(|_activate: On<Activate>,
                staged: Res<Staged>,
                mut clipboard: ResMut<Clipboard>| {
                let snippet = crate::snippet::vineyard_cfg(&staged.0);
                info!("Isaac Lab config for the current scene:\n\n{snippet}");
                match clipboard.set_text(snippet) {
                    Ok(()) => info!("copied to clipboard"),
                    Err(err) => warn!("could not reach the clipboard: {err}"),
                }
            })
        ) ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::SceneParams;
    use core::time::Duration;

    /// One frame, `by` after the last.
    ///
    /// No `TimePlugin`: it drives `Time` off the wall clock, and this test is
    /// about a duration rather than about how long it takes to run.
    fn advance(app: &mut App, by: Duration) {
        app.world_mut().resource_mut::<Time<Real>>().advance_by(by);
        app.update();
    }

    /// A drag must not reach the scene while it is still moving, and must
    /// reach it once it stops — the whole point of staging the write.
    #[test]
    fn a_staged_value_lands_once_it_stops_moving() {
        let mut app = App::new();
        app.init_resource::<Time<Real>>()
            .init_resource::<SceneParams>()
            .insert_resource(Staged(VineyardParams::default()))
            .add_systems(PreUpdate, commit);

        // Twenty frames of drag, well past `QUIET` end to end.
        for _ in 0..20 {
            app.world_mut().resource_mut::<Staged>().scene.seed += 1;
            advance(&mut app, Duration::from_secs_f32(0.016));
        }
        assert_eq!(app.world().resource::<SceneParams>().seed, 0, "mid-drag");

        advance(&mut app, Duration::from_secs_f32(QUIET * 2.0));
        assert_eq!(app.world().resource::<SceneParams>().seed, 20, "released");
    }
}
