//! Feathers-based parameter panel, docked full-height down the left edge of
//! the viewer.
//!
//! The panel itself owns no controls — it just stacks the UI fragment each
//! element publishes next to its own params and author fn, one [`section`]
//! each. Adding an element to the panel is one line in [`params_panel`].
//!
//! Sliders fire on every [`ValueChange`](bevy::ui_widgets::ValueChange),
//! including mid-drag, so they write a [`Staged`] copy of the params rather
//! than the resources the build systems read. [`commit`] hands the edit over
//! once the slider has stopped moving, which is what keeps the drag itself
//! smooth: one rebuild per drag instead of one per frame.
//!
//! Every control carries a [`Tip`] saying what its parameter means; [`tips`]
//! floats it beside the control while the pointer is over it.

use bevy::clipboard::Clipboard;
use bevy::feathers::{
    FeathersPlugins,
    containers::{group, group_body, group_header, pane, pane_body, pane_header},
    controls::{
        ButtonVariant, FeathersButton, FeathersDisclosureToggle, FeathersMenu, FeathersMenuButton,
        FeathersMenuItem, FeathersMenuPopup,
    },
    dark_theme::create_dark_theme,
    display::label_small,
    theme::{ThemeBackgroundColor, ThemeBorderColor, ThemedText, UiTheme},
    tokens,
};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{Checked, OverrideClip};
use bevy::ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide};
use bevy::ui_widgets::{Activate, ScrollArea, ValueChange, checkbox_self_update};

use crate::elements::{Grow, VineyardParams};

use crate::elements::cover::ui as cover_ui;
use crate::elements::leaf::ui as leaf_ui;
use crate::elements::pole::ui as pole_ui;
use crate::elements::shoot::ui as shoot_ui;
use crate::elements::terrain::ui as terrain_ui;
use crate::elements::ui as scene_ui;
use crate::elements::util::parcel::ui as parcel_ui;
use crate::elements::util::planting::ui as planting_ui;
use crate::elements::vine::ui as vine_ui;
use crate::elements::weed::ui as weed_ui;
use crate::elements::wire::ui as wire_ui;

pub fn plugin(app: &mut App) {
    // Every element plugin is added before this one, so the live params are
    // already there to seed the panel from.
    let live = VineyardParams::from_world(app.world());
    app.add_plugins(FeathersPlugins)
        .insert_resource(UiTheme(create_dark_theme()))
        .insert_resource(Staged(live))
        .add_systems(Startup, params_panel_list.spawn())
        .add_systems(PreUpdate, commit.before(Grow::Terrain))
        .add_systems(Update, (sync_dropdown_captions, tips));
}

/// Marks a dropdown's caption, with the read that keeps it current.
#[derive(Component, Clone, Copy)]
pub struct DropdownCaption(fn(&VineyardParams) -> &str);

/// Blank, and only because a scene template constructs its components from
/// their defaults before patching them; every dropdown patches the read in.
impl Default for DropdownCaption {
    fn default() -> Self {
        Self(|_| "")
    }
}

/// A choice among named options: a menu button showing the current one,
/// opening onto the rest.
///
/// `read` says which option the params hold and `write` stores a pick; both
/// address [`Staged`], like a slider does. The caption is not set by the pick
/// but read back from the params every frame by [`sync_dropdown_captions`],
/// so it is right however the params came to change and needs no walk from
/// a menu item back to the button it belongs to.
pub fn dropdown(
    label: &'static str,
    tip: &'static str,
    options: &'static [&'static str],
    read: fn(&VineyardParams) -> &str,
    write: fn(&mut VineyardParams, &'static str),
) -> impl Scene {
    let items: Vec<_> = options
        .iter()
        .map(|name| {
            let name: &'static str = name;
            bsn! {
                (
                    @FeathersMenuItem { @caption: bsn! { Text(name) ThemedText } }
                    on(move |_activate: On<Activate>, mut params: ResMut<Staged>| {
                        write(&mut params.0, name);
                    })
                )
            }
        })
        .collect();
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            label_small(label),
            (
                @FeathersMenu
                Children [
                    (
                        @FeathersMenuButton {
                            @caption: bsn! { (Text("") ThemedText DropdownCaption(read)) }
                        }
                        Node { flex_grow: 1.0 }
                        Tip(tip)
                    ),
                    (@FeathersMenuPopup Children [ {items} ]),
                ]
            ),
        ]
    }
}

/// Shows every dropdown the option its params currently hold.
fn sync_dropdown_captions(staged: Res<Staged>, mut captions: Query<(&DropdownCaption, &mut Text)>) {
    for (caption, mut text) in &mut captions {
        let current = (caption.0)(&staged.0);
        if text.0 != current {
            text.0 = current.to_string();
        }
    }
}

/// What a control's parameter means, in a line or two.
///
/// Goes on the control rather than on its caption: every feathers slider,
/// checkbox and menu button already carries [`Hovered`], so a tip is the whole
/// per-parameter cost of a tooltip — no wrapper node, no lookup table.
#[derive(Component, Clone, Copy, Default)]
pub struct Tip(pub &'static str);

/// The card [`tips`] spawns, so it can find it again to take it down.
#[derive(Component, Clone, Default)]
struct TipPopup;

/// Floats a control's [`Tip`] beside it while the pointer is over it.
///
/// `Hovered` is immutable and reinserted only when the pointer crosses the
/// control's bounds, so `Changed` is an exact enter/leave edge: one card is
/// spawned on enter and despawned on leave, and none exist in between.
fn tips(
    mut commands: Commands,
    crossed: Query<(Entity, &Hovered, &Tip), Changed<Hovered>>,
    shown: Query<(Entity, &ChildOf), With<TipPopup>>,
) {
    for (control, hovered, tip) in &crossed {
        let card = shown
            .iter()
            .find(|(_, of)| of.parent() == control)
            .map(|(card, _)| card);
        match (hovered.get(), card) {
            (true, None) => {
                commands
                    .spawn_scene(tip_popup(tip.0))
                    .insert(ChildOf(control));
            }
            (false, Some(card)) => commands.entity(card).despawn(),
            _ => {}
        }
    }
}

/// The card itself. [`Popover`] anchors it to the control it is a child of and
/// flips it to whichever side has room, so it clears the panel's right edge.
fn tip_popup(text: &'static str) -> impl Scene {
    bsn! {
        Node {
            // `position_popover` sets this itself, but only after a frame of
            // layout — without it the card is in flow once and shoves the row.
            position_type: PositionType::Absolute,
            max_width: px(200),
            padding: UiRect::axes(px(8), px(5)),
            border: px(1),
            border_radius: {BorderRadius::all(px(4))},
        }
        TipPopup
        ThemeBackgroundColor(tokens::MENU_BG)
        ThemeBorderColor(tokens::MENU_BORDER)
        GlobalZIndex(100)
        // The panel body scrolls, and a scroll clip reaches every descendant
        // whatever its `position_type`. This is the only way out of one.
        OverrideClip
        // The card overlaps the control it belongs to, and a hit on it would
        // read as a hover leave — which would flicker the card away.
        Pickable::IGNORE
        Popover {
            positions: vec![
                PopoverPlacement {
                    side: PopoverSide::Right,
                    align: PopoverAlign::Start,
                    gap: 8.0,
                },
                PopoverPlacement {
                    side: PopoverSide::Left,
                    align: PopoverAlign::Start,
                    gap: 8.0,
                },
            ],
            window_margin: 10.0,
        }
        Children [ label_small(text) ]
    }
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
            top: px(0),
            left: px(0),
            bottom: px(0),
            width: px(260),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        ParamsPanel
        Interaction
        ThemeBackgroundColor(tokens::WINDOW_BG)
        // `min_height` on both: a flex item refuses to shrink below its own
        // content by default, which would push the scroll area past the
        // bottom of the window instead of letting it scroll.
        Children [ pane() Node { flex_grow: 1.0, min_height: px(0) } Children [
            pane_header() Children [ (Text("Vineyard") ThemedText) ],
            pane_body() Node { flex_grow: 1.0, min_height: px(0) } Children [
                (
                    Node {
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6),
                        flex_grow: 1.0,
                        min_height: px(0),
                        overflow: Overflow::scroll_y(),
                    }
                    ScrollArea
                    Children [
                        section("Scene", bsn_list![scene_ui()]),
                        section("Terrain", bsn_list![terrain_ui()]),
                        section("Parcel", bsn_list![parcel_ui()]),
                        section("Pole", bsn_list![pole_ui()]),
                        section("Wire", bsn_list![wire_ui()]),
                        section("Vine", bsn_list![vine_ui()]),
                        section("Shoot", bsn_list![shoot_ui()]),
                        section("Leaf", bsn_list![leaf_ui()]),
                        section("Planting", bsn_list![planting_ui()]),
                        section("Cover", bsn_list![cover_ui()]),
                        section("Weeds", bsn_list![weed_ui()]),
                    ]
                ),
                // Outside the scroll area, so it stays reachable however far
                // down the panel is scrolled.
                copy_cfg_button(),
            ],
        ]]
    }
}

/// One element's fragment, under a header whose chevron folds it away.
///
/// Sections start open. Collapsing every element but the one being tuned is
/// what keeps a panel of forty-odd sliders on screen without scrolling.
///
/// The group's children are `[header, body]` in that order, and the toggle
/// sits in the header — [`fold_section`] walks that shape.
fn section(title: &'static str, body: impl SceneList) -> impl Scene {
    bsn! {
        group()
        Children [
            group_header() Children [
                (Text(title) ThemedText),
                (
                    @FeathersDisclosureToggle
                    Checked
                    on(checkbox_self_update)
                    on(fold_section)
                ),
            ],
            group_body() Children [ {body} ],
        ]
    }
}

/// Shows or hides the body of the section whose chevron was just toggled.
fn fold_section(
    change: On<ValueChange<bool>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    mut nodes: Query<&mut Node>,
) {
    let Ok(group) = parents
        .get(change.source)
        .and_then(|header| parents.get(header.parent()))
    else {
        return;
    };
    let Some(body) = children
        .get(group.parent())
        .ok()
        .and_then(|kids| kids.get(1))
    else {
        return;
    };
    if let Ok(mut node) = nodes.get_mut(*body) {
        node.display = if change.value {
            Display::Flex
        } else {
            Display::None
        };
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

    fn one_section() -> impl SceneList {
        bsn_list![section("Test", bsn_list![Node])]
    }

    /// Everything a section's scene touches while spawning, and nothing else:
    /// no window, no renderer, no theme.
    fn panel_app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::scene::ScenePlugin,
        ))
        .init_asset::<bevy::text::Font>()
        .init_asset::<Image>()
        .add_systems(Startup, one_section.spawn());
        app.update();
        app
    }

    /// How many nodes are currently folded away.
    fn hidden(world: &mut World) -> usize {
        world
            .query::<&Node>()
            .iter(world)
            .filter(|node| node.display == Display::None)
            .count()
    }

    /// The chevron folds its own section's body, and unfolds it again. The
    /// walk from the toggle up to the group and back down to the body is the
    /// one thing here that can silently land on the wrong entity.
    #[test]
    fn a_chevron_folds_the_body_of_its_section() {
        let mut app = panel_app();
        let world = app.world_mut();
        let toggle = world
            .query_filtered::<Entity, With<bevy::ui_widgets::Checkbox>>()
            .single(world)
            .expect("the section spawned one chevron");

        assert_eq!(hidden(world), 0, "sections start open");
        for (value, folded) in [(false, 1), (true, 0)] {
            world.trigger(ValueChange {
                source: toggle,
                value,
                is_final: true,
            });
            world.flush();
            assert_eq!(hidden(world), folded, "after toggling to {value}");
        }
    }

    fn one_dropdown() -> impl SceneList {
        bsn_list![dropdown(
            "Kind",
            "Which sward is sown in the alleys.",
            &crate::elements::cover::Kind::NAMES,
            |params| &params.cover.kind,
            |params, name| params.cover.kind = name.to_string(),
        )]
    }

    /// The caption a dropdown shows, in a world with the params it reads.
    fn caption(world: &mut World) -> String {
        world
            .query_filtered::<&Text, With<DropdownCaption>>()
            .single(world)
            .expect("one caption")
            .0
            .clone()
    }

    /// A dropdown's two halves, neither of which a compile can check: the
    /// caption follows the params, and activating an item writes them.
    #[test]
    fn a_dropdown_shows_the_current_choice_and_writes_a_pick() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::scene::ScenePlugin,
        ))
        .init_asset::<bevy::text::Font>()
        .init_asset::<Image>()
        .insert_resource(Staged(VineyardParams::default()))
        .add_systems(Startup, one_dropdown.spawn())
        .add_systems(Update, sync_dropdown_captions);
        app.update();
        app.update();
        assert_eq!(
            caption(app.world_mut()),
            "spontaneous",
            "the default reads through"
        );

        // The item reading "sown": its label hangs somewhere under the item
        // that carries the observer.
        let world = app.world_mut();
        let label = world
            .query::<(Entity, &Text)>()
            .iter(world)
            .find(|(_, text)| text.0 == "sown")
            .map(|(entity, _)| entity)
            .expect("an item reads sown");
        let mut item = label;
        while !world.entity(item).contains::<bevy::ui_widgets::MenuItem>() {
            item = world
                .entity(item)
                .get::<ChildOf>()
                .expect("the label hangs under an item")
                .parent();
        }
        world.trigger(Activate { entity: item });
        world.flush();
        assert_eq!(world.resource::<Staged>().cover.kind, "sown");

        app.update();
        assert_eq!(caption(app.world_mut()), "sown", "and the caption follows");
    }

    /// A tip appears while the pointer is over its control and leaves with it.
    ///
    /// `Hovered` is immutable, so the pointer crossing a control shows up here
    /// as a re-insert — which is also what makes `Changed` an exact edge.
    #[test]
    fn a_tip_follows_the_pointer_onto_its_control_and_off_again() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::scene::ScenePlugin,
        ))
        .init_asset::<bevy::text::Font>()
        .add_systems(Update, tips);
        let control = app
            .world_mut()
            .spawn((Tip("Post radius."), Hovered(false)))
            .id();

        let cards = |app: &mut App| {
            app.world_mut()
                .query_filtered::<(), With<TipPopup>>()
                .iter(app.world())
                .count()
        };

        app.update();
        assert_eq!(cards(&mut app), 0, "nothing shows unhovered");

        app.world_mut().entity_mut(control).insert(Hovered(true));
        app.update();
        assert_eq!(cards(&mut app), 1, "the pointer arrives");
        app.update();
        assert_eq!(cards(&mut app), 1, "and one card is enough");

        app.world_mut().entity_mut(control).insert(Hovered(false));
        app.update();
        assert_eq!(cards(&mut app), 0, "the pointer leaves");
    }

    /// Every control in the panel carries a tip.
    ///
    /// The tips are a copy of the field list, the way `snippet.rs` keeps one,
    /// and they go in by hand at each control: add a slider without a tip and
    /// the two counts part company here rather than in the running panel.
    #[test]
    fn every_control_in_the_panel_carries_a_tip() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::scene::ScenePlugin,
        ))
        .init_asset::<bevy::text::Font>()
        .init_asset::<Image>()
        .insert_resource(Staged(VineyardParams::default()))
        .add_systems(Startup, params_panel_list.spawn());
        app.update();

        let world = app.world_mut();
        // The section chevrons are checkboxes too, and fold rather than
        // configure — they are the one control with nothing to explain.
        let controls = world
            .query_filtered::<(), (
                Or<(
                    With<bevy::ui_widgets::Slider>,
                    With<bevy::ui_widgets::Checkbox>,
                    With<bevy::ui_widgets::MenuButton>,
                )>,
                Without<FeathersDisclosureToggle>,
            )>()
            .iter(world)
            .count();
        let tipped = world.query_filtered::<(), With<Tip>>().iter(world).count();
        assert!(controls > 50, "the whole panel spawned, not a fragment");
        assert_eq!(tipped, controls, "a control was added without a Tip");
    }
}
