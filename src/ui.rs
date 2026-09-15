//! Feathers-based parameter panel, docked to the top-left corner of the
//! viewer.
//!
//! The panel itself owns no controls — it just stacks the UI fragment each
//! element publishes next to its own params and author fn. Adding an element
//! to the panel is one line in [`params_panel`].
//!
//! Sliders fire on every [`ValueChange`](bevy::ui_widgets::ValueChange),
//! including mid-drag, so they write a [`Staged`] copy of their params rather
//! than the resource the build systems read. [`commit`] hands the value over
//! once the slider has stopped moving, which is what keeps the drag itself
//! smooth: one rebuild per drag instead of one per frame.

use bevy::clipboard::Clipboard;
use bevy::ecs::component::Mutable;
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

use crate::elements::util::parcel::ParcelParams;
use crate::elements::util::planting::PlantingParams;
use crate::elements::{
    Grow, SceneParams, VineyardParams, leaf::LeafParams, pole::PoleParams, shoot::ShootParams,
    terrain::TerrainParams, vine::VineParams,
};

use crate::elements::leaf::ui as leaf_ui;
use crate::elements::pole::ui as pole_ui;
use crate::elements::shoot::ui as shoot_ui;
use crate::elements::terrain::ui as terrain_ui;
use crate::elements::ui as scene_ui;
use crate::elements::util::parcel::ui as parcel_ui;
use crate::elements::util::planting::ui as planting_ui;
use crate::elements::vine::ui as vine_ui;

pub fn plugin(app: &mut App) {
    app.add_plugins(FeathersPlugins)
        .insert_resource(UiTheme(create_dark_theme()))
        .add_systems(Startup, params_panel_list.spawn())
        // One per element, in the order the panel stacks them.
        .add_plugins((
            staged::<SceneParams>,
            staged::<TerrainParams>,
            staged::<ParcelParams>,
            staged::<PoleParams>,
            staged::<VineParams>,
            staged::<ShootParams>,
            staged::<LeafParams>,
            staged::<PlantingParams>,
        ));
}

/// How long a staged value has to hold still before it reaches the scene.
///
/// Short enough to feel immediate when the pointer pauses, long enough that no
/// frame of a drag commits. Anything reading the live params — the copy button,
/// the save key — is this far behind the panel at worst.
const QUIET: f32 = 0.15;

/// The panel's copy of a params resource, waiting to be handed over.
///
/// Rebuilding a layer costs tens of milliseconds at parcel scale, so a slider
/// that wrote the live resource would spend a drag rebuilding the scene once
/// per frame. Sliders write this instead and [`commit`] copies it across.
///
/// Viewer-only: nothing but [`plugin`] inserts one, so the headless
/// [`generate`](crate::generate) path and the Python bindings see the live
/// resources they always did.
#[derive(Resource, Deref, DerefMut)]
pub struct Staged<T: Resource>(pub T);

/// Gives the panel a staged copy of `T` to write into.
fn staged<T: Resource<Mutability = Mutable> + Clone>(app: &mut App) {
    // Every element plugin is added before this one, so the live resource is
    // already there to seed from.
    let live = app.world().resource::<T>().clone();
    app.insert_resource(Staged(live))
        .add_systems(PreUpdate, commit::<T>.before(Grow::Terrain));
}

/// Copies a staged value into the live resource once it has stopped moving.
fn commit<T: Resource<Mutability = Mutable> + Clone>(
    time: Res<Time>,
    staged: Res<Staged<T>>,
    mut live: ResMut<T>,
    mut still: Local<Option<f32>>,
) {
    // The insert in `staged` reads as a change on this system's first run, and
    // committing it would rebuild the scene a second time at startup.
    if staged.is_added() {
        return;
    }
    if staged.is_changed() {
        *still = Some(0.0);
    } else if let Some(elapsed) = still.as_mut() {
        *elapsed += time.delta_secs();
        if *elapsed >= QUIET {
            *live = (**staged).clone();
            *still = None;
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
            on(|_activate: On<Activate>, mut commands: Commands| {
                // Queued rather than done here: reading every element's params
                // needs the whole world, which rules out taking `Clipboard` as
                // a `ResMut` alongside it.
                commands.queue(copy_cfg_to_clipboard);
            })
        ) ]
    }
}

/// Emits the snippet for the params currently in `world` and copies it.
///
/// Logged as well as copied. The clipboard is the point, but it is the part
/// that can fail for reasons outside the app — no backend on a bare Wayland
/// session, no X11 display — and the snippet is worth more than the error.
fn copy_cfg_to_clipboard(world: &mut World) {
    let snippet = crate::snippet::vineyard_cfg(&VineyardParams::from_world(world));
    info!("Isaac Lab config for the current scene:\n\n{snippet}");
    match world.resource_mut::<Clipboard>().set_text(snippet) {
        Ok(()) => info!("copied to clipboard"),
        Err(err) => warn!("could not reach the clipboard: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    /// One frame, `by` after the last.
    ///
    /// No `TimePlugin`: it drives `Time` off the wall clock, and this test is
    /// about a duration rather than about how long it takes to run.
    fn advance(app: &mut App, by: Duration) {
        app.world_mut().resource_mut::<Time>().advance_by(by);
        app.update();
    }

    /// A drag must not reach the scene while it is still moving, and must
    /// reach it once it stops — the whole point of staging the write.
    #[test]
    fn a_staged_value_lands_once_it_stops_moving() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<SceneParams>()
            .add_plugins(staged::<SceneParams>);

        // Twenty frames of drag, well past `QUIET` end to end.
        for _ in 0..20 {
            app.world_mut().resource_mut::<Staged<SceneParams>>().seed += 1;
            advance(&mut app, Duration::from_secs_f32(0.016));
        }
        assert_eq!(app.world().resource::<SceneParams>().seed, 0, "mid-drag");

        advance(&mut app, Duration::from_secs_f32(QUIET * 2.0));
        assert_eq!(app.world().resource::<SceneParams>().seed, 20, "released");
    }
}
