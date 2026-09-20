//! The viewer's footer: what the scene currently weighs, counted off the ECS.
//!
//! Nothing here is estimated or recorded while building. Every figure is a
//! query over the entities and the mesh library as they stand, so the footer
//! cannot disagree with what is on screen. [`tally`] is the only thing that
//! counts and [`SceneStats`] the only thing the UI reads, so another figure is
//! one field, one line in [`tally`] and one [`field`] in [`footer`].
//!
//! The footer also carries the controls that say how the scene is *drawn*,
//! which is not what the params panel is for — it says what the scene *is*.

use std::collections::HashMap;

use bevy::feathers::controls::FeathersCheckbox;
use bevy::feathers::display::label;
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::mesh::Indices;
use bevy::pbr::wireframe::WireframeConfig;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{ValueChange, checkbox_self_update};

use crate::scene::{Cable, Collider, Prototypes, UsdReference};
use crate::ui::{BlocksCamera, PANEL_WIDTH, Tip, TipAbove};

pub fn plugin(app: &mut App) {
    app.init_resource::<SceneStats>()
        .add_systems(Startup, footer_list.spawn())
        .add_systems(
            Update,
            // In `Update` rather than beside the build systems in `PreUpdate`:
            // a layer spawns through `Commands`, so its entities only exist
            // once those have been applied.
            //
            // Gated on the library, which every layer clears before rebuilding
            // — so a walk over a hundred thousand instances happens on the
            // frames something actually changed, not on all of them.
            (
                tally.run_if(resource_changed::<Prototypes>),
                show.run_if(resource_changed::<SceneStats>),
            )
                .chain(),
        );
}

/// What the scene currently weighs. Written by [`tally`], read by [`show`].
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct SceneStats {
    /// Named entities — one prim each in the exported stage.
    pub prims: usize,
    /// Prims drawing a shared part instead of geometry of their own.
    pub instances: usize,
    /// Entries in the mesh library.
    pub parts: usize,
    /// Triangles in the library, built once each.
    pub unique_triangles: usize,
    /// Triangles standing in the scene, counting every instance of a part.
    pub drawn_triangles: usize,
    /// Capsule collision proxies. The ground collides as its own mesh and is
    /// not one of these.
    pub capsules: usize,
    /// Rod segments across every flexible organ — one rigid body each, once a
    /// solver imports the curve.
    pub segments: usize,
}

/// A mesh's triangle count. Everything the library holds is indexed; a mesh
/// that is not falls back to its points, which a triangle list groups in
/// threes.
fn triangles_in(mesh: &Mesh) -> usize {
    mesh.indices().map_or(mesh.count_vertices(), Indices::len) / 3
}

fn tally(
    mut stats: ResMut<SceneStats>,
    prototypes: Res<Prototypes>,
    meshes: Res<Assets<Mesh>>,
    prims: Query<(), With<Name>>,
    instances: Query<&UsdReference>,
    capsules: Query<(), With<Collider>>,
    cables: Query<&Cable>,
) {
    let triangles: HashMap<&str, usize> = prototypes
        .iter()
        .map(|(name, part)| {
            let count = meshes.get(&part.mesh).map_or(0, triangles_in);
            (name.as_str(), count)
        })
        .collect();

    *stats = SceneStats {
        prims: prims.iter().count(),
        instances: instances.iter().count(),
        parts: prototypes.len(),
        unique_triangles: triangles.values().sum(),
        drawn_triangles: instances
            .iter()
            .filter_map(|reference| triangles.get(reference.0.as_str()))
            .sum(),
        capsules: capsules.iter().count(),
        // One body per edge between the points, which is what a solver's cable
        // importer builds out of the curve.
        segments: cables
            .iter()
            .map(|cable| cable.0.points.len().saturating_sub(1))
            .sum(),
    };
}

/// A count at a glance: three digits and a magnitude. A footer is read while a
/// slider is moving, and `91,605,918` is not.
fn short(n: usize) -> String {
    let n = n as f64;
    if n < 1e4 {
        format!("{n:.0}")
    } else if n < 1e6 {
        format!("{:.0}k", n / 1e3)
    } else {
        format!("{:.1}M", n / 1e6)
    }
}

/// Marks a footer figure: its label, and the read that keeps it current.
#[derive(Component, Clone, Copy)]
struct Field(&'static str, fn(&SceneStats) -> String);

/// Blank, and only because a scene template builds its components from their
/// defaults before patching them; every field patches both halves in.
impl Default for Field {
    fn default() -> Self {
        Self("", |_| String::new())
    }
}

/// Shows every figure what the last tally counted.
fn show(stats: Res<SceneStats>, mut fields: Query<(&Field, &mut Text)>) {
    for (field, mut text) in &mut fields {
        let current = format!("{} {}", field.0, (field.1)(&stats));
        if text.0 != current {
            text.0 = current;
        }
    }
}

fn footer_list() -> impl SceneList {
    bsn_list![footer()]
}

/// A bar along the bottom of the viewport, starting where the params panel
/// ends.
fn footer() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            bottom: px(0),
            left: px(PANEL_WIDTH),
            right: px(0),
            align_items: AlignItems::Center,
            column_gap: px(16),
            padding: UiRect::axes(px(12), px(2)),
        }
        BlocksCamera
        Interaction
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [
            field(
                "Prims",
                "Prims the export writes: one per organ, plus the transforms grouping them. The size of the stage, not of the render.",
                |stats| short(stats.prims),
            ),
            field(
                "Instances",
                "Prims drawing a shared part rather than points of their own. Bevy batches each part into one draw however many reference it.",
                |stats| short(stats.instances),
            ),
            field(
                "Parts",
                "Meshes in the shared library, one per representative. What a variations slider buys: unique geometry, not more of it.",
                |stats| short(stats.parts),
            ),
            field(
                "Tris",
                "Triangles built, then triangles standing in the scene. The gap between them is the instancing — a detail slider moves the first, a parcel slider the second.",
                |stats| format!("{} / {}", short(stats.unique_triangles), short(stats.drawn_triangles)),
            ),
            field(
                "Capsules",
                "Capsule proxies a physics engine collides with. The ground collides as its own mesh and is not counted here.",
                |stats| short(stats.capsules),
            ),
            field(
                "Rods",
                "Capsule bodies a solver builds from the flexible shoots — the one figure here that reaches the physics step. Zero unless stray shoots are bendable.",
                |stats| short(stats.segments),
            ),
            // Holds the view controls at the far end, away from the figures.
            (Node { flex_grow: 1.0 }),
            wireframe_checkbox(),
        ]
    }
}

/// One figure, under a node the pointer can rest on so its [`Tip`] can say what
/// it counts.
///
/// A wrapper rather than the label itself: [`tips`](crate::ui) hangs its card
/// off the hovered entity as a child, and a `Text`'s children are its spans.
fn field(caption: &'static str, tip: &'static str, read: fn(&SceneStats) -> String) -> impl Scene {
    bsn! {
        Node
        Hovered
        Tip(tip)
        TipAbove
        Children [ (label("") Field(caption, read)) ]
    }
}

fn wireframe_checkbox() -> impl Scene {
    bsn! {
        @FeathersCheckbox { @caption: bsn! { (Text("Wireframe") ThemedText) } }
        Tip("Draw every mesh as its edges.")
        TipAbove
        on(checkbox_self_update)
        on(|change: On<ValueChange<bool>>, mut config: ResMut<WireframeConfig>| {
            config.global = change.value;
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Part, Surface, cable, capsule};

    fn part(mesh: Handle<Mesh>) -> Part {
        Part {
            mesh,
            color: [0.5, 0.5, 0.5],
            roughness: 0.8,
            reflectance: 0.5,
            translucency: 0.0,
            thickness: 0.0,
            double_sided: false,
            collision: None,
            heightfield_resolution: None,
        }
    }

    fn surface() -> Surface {
        Surface {
            color: [0.5, 0.4, 0.2],
            roughness: 0.8,
            reflectance: 0.5,
            translucency: 0.0,
            thickness: 0.0,
            double_sided: false,
        }
    }

    /// Two instances of one part, so the two triangle figures have to differ:
    /// with a single instance, counting the library and counting the scene give
    /// the same answer and neither is pinned.
    #[test]
    fn a_part_is_built_once_and_drawn_by_every_instance() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_resource::<Prototypes>()
            .init_resource::<SceneStats>()
            .add_systems(Update, tally);

        let world = app.world_mut();
        // A cuboid is 12 triangles, and indexed the way the library's own
        // meshes are.
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Mesh::from(Cuboid::default()));
        world
            .resource_mut::<Prototypes>()
            .insert("Leaf", 0, part(mesh));
        for index in 0..2 {
            world.spawn((
                Name::new(format!("Leaf_{index:02}")),
                UsdReference("Leaf_0".into()),
            ));
        }
        world.spawn((Name::new("Collision"), capsule(0.05, 0.0, 0.6)));
        world.spawn((
            Name::new("Cable"),
            cable(
                vec![[0.0; 3], [0.0, 0.0, 0.1], [0.0, 0.0, 0.2]],
                vec![0.01; 3],
                0.01,
                surface(),
            ),
        ));
        app.update();

        assert_eq!(
            *app.world().resource::<SceneStats>(),
            SceneStats {
                prims: 4,
                instances: 2,
                parts: 1,
                unique_triangles: 12,
                drawn_triangles: 24,
                capsules: 1,
                // Three points, so two edges.
                segments: 2,
            }
        );
    }

    /// The footer is read at a glance, so every magnitude has to land on three
    /// digits or fewer.
    #[test]
    fn counts_are_shortened_to_three_digits() {
        for (n, expected) in [(0, "0"), (917, "917"), (9999, "9999"), (101_000, "101k")] {
            assert_eq!(short(n), expected);
        }
        assert_eq!(short(91_605_918), "91.6M");
    }
}
