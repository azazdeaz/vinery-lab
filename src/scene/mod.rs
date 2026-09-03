//! The scene graph the generator builds, and the components that tell the
//! exporter what to make of it.
//!
//! Every layer spawns ordinary Bevy entities — a [`Transform`], a [`Name`], a
//! `Mesh3d` where there is geometry — and the viewer renders them with no
//! translation step at all. The components here are *export directives*: they
//! say nothing about how an entity draws, only about what prim it becomes.
//!
//! The one that matters is [`UsdReference`]. An entity carrying it still holds
//! a real `Mesh3d` with a shared handle, which is exactly how Bevy batches
//! tens of thousands of leaves; the component only tells the exporter to emit a
//! reference to the shared part instead of a copy of its points.
//!
//! # The shape of the tree
//!
//! Structural prims are unique and have children. Geometry prims reference a
//! part, carry no children, and are instanceable:
//!
//! ```text
//! /Vineyard/Planting/Row_00/Vine_047     Xform, unique
//!   /Wood                                 -> parts/Vine_3, instanceable
//!   /Shoot_00                             Xform, unique
//!     /Stem                               -> parts/Shoot_11, instanceable
//!     /Leaf_00                            -> parts/Leaf_2, instanceable
//! ```
//!
//! Keeping geometry at the leaves is what makes `instanceable` safe to set
//! unconditionally: an instanceable prim's descendants are not addressable,
//! and these have no descendants to lose.

pub mod doc;
pub mod export;

use std::collections::BTreeMap;
use std::f32::consts::FRAC_PI_2;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

pub fn plugin(app: &mut App) {
    app.init_resource::<Prototypes>()
        .add_systems(Startup, spawn_root);
}

/// The entity every element spawns its prims under — the scene root, and the
/// stage's default prim.
#[derive(Resource, Debug, Clone, Copy)]
pub struct PrimRoot(pub Entity);

/// Takes the scene's `+Z` onto the world's `+Y`, standing a Z-up scene upright
/// for Bevy's Y-up renderer.
///
/// The scene is authored Z-up because that is what the export target needs, and
/// `upAxis` is root-layer-only metadata that does not compose through
/// references — a stage cannot declare Y-up and leave a consumer to correct for
/// it. So the correction belongs to the viewer alone, which is why it sits on a
/// parent of [`PrimRoot`] rather than inside the exported subtree.
pub fn z_up_to_y_up() -> Quat {
    Quat::from_rotation_x(-FRAC_PI_2)
}

/// Spawns the scene root under the Z-up correction, and publishes it as
/// [`PrimRoot`].
fn spawn_root(mut commands: Commands) {
    let upright = commands
        .spawn((
            Transform::from_rotation(z_up_to_y_up()),
            Visibility::default(),
        ))
        .id();
    // The identity `Transform` and `Visibility` are load-bearing: propagation
    // stops at an entity that has neither, which would leave everything below
    // this one unrotated and never drawn.
    //
    // Unnamed parent, named child: the export walk starts at the `Name`, so
    // the correction never reaches the document.
    let root = commands
        .spawn((
            UsdRoot,
            Name::new("Vineyard"),
            Transform::IDENTITY,
            Visibility::default(),
            ChildOf(upright),
        ))
        .id();
    commands.insert_resource(PrimRoot(root));
}

/// Marks the entity the export walk starts from, which becomes the stage's
/// default prim.
///
/// Its parent carries the Z-up correction the viewer needs (see
/// [`viewer`](crate::viewer)) and is deliberately *not* exported: the scene
/// below this entity is already authored in the target's Z-up convention, and
/// the rotation exists only so Bevy's Y-up camera has something upright to
/// look at.
#[derive(Component, Debug)]
pub struct UsdRoot;

/// Draw the [`Part`] of this name instead of inlining geometry.
///
/// The name is a key into [`Prototypes`], not a prim path — the exporter turns
/// it into one. An entity carrying this must have no children.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct UsdReference(pub String);

/// Overrides the prim type, which is otherwise `Xform`.
///
/// Only worth setting for `Scope`, on a prim that groups without placing —
/// a row of vines carries no transform of its own, because its plants are each
/// draped onto terrain that one row transform could not follow.
#[derive(Component, Clone, Copy, Debug)]
pub struct UsdType(pub &'static str);

/// One entry of the mesh library: the geometry built for a single
/// representative, shared by every organ that drew it.
#[derive(Clone, Debug)]
pub struct Part {
    pub mesh: Handle<Mesh>,
    /// Linear RGB — what [`color::srgb`](crate::elements::util::color::srgb)
    /// hands back. The one channel both consumers read: Bevy draws it, and USD
    /// carries it as `displayColor`, so the viewer and the export agree on
    /// colour by construction.
    pub color: [f32; 3],
    /// Microfacet roughness. 0 is a mirror, 1 is chalk.
    pub roughness: f32,
    /// Index of refraction, which sets how bright the specular highlight is at
    /// a glancing angle.
    pub ior: f32,
    /// A surface with no inside — a leaf blade — which has to be lit and drawn
    /// from behind as well, since a canopy is looked up into as often as down
    /// onto.
    pub double_sided: bool,
}

impl Part {
    /// The viewer's material for this part.
    ///
    /// Untextured, because the colour is the whole material: nothing here is
    /// metallic, and a texture-free surface is distinguished only by how it
    /// responds to light.
    pub fn material(&self) -> StandardMaterial {
        let [red, green, blue] = self.color;
        let mut material = StandardMaterial {
            base_color: Color::linear_rgb(red, green, blue),
            perceptual_roughness: self.roughness,
            ior: self.ior,
            metallic: 0.0,
            ..default()
        };
        if self.double_sided {
            material.double_sided = true;
            // Back faces are culled by default, which would leave a canopy
            // half missing whenever it is looked up into.
            material.cull_mode = None;
        }
        material
    }
}

/// A stable ordering key, assigned in authoring order.
///
/// Bevy's query iteration order is not stable across runs and a codebook has to
/// be a function of its population alone, so every organ records where it was
/// authored and a layer sorts by this before quantizing.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Order(pub u64);

/// Run condition: some organ of this layer was added or re-authored.
pub fn configs_changed<C: Component>(configs: Query<(), Changed<C>>) -> bool {
    !configs.is_empty()
}

/// An organ standing at `position`, tipped by `tilt` in its own frame and then
/// turned to `yaw` along the row.
///
/// Composed explicitly rather than through `Quat::from_euler`, so the order the
/// rotations apply in is on the page: tilt is a lean in the prototype's own
/// frame, and applying it after the yaw would swing it around with the row.
pub fn placed(position: Vec3, yaw: f32, tilt: Vec2, scale: f32) -> Transform {
    Transform {
        translation: position,
        rotation: Quat::from_rotation_z(yaw)
            * Quat::from_rotation_y(tilt.y)
            * Quat::from_rotation_x(tilt.x),
        scale: Vec3::splat(scale),
    }
}

/// How a part looks — a [`Part`] without its geometry.
#[derive(Clone, Copy, Debug)]
pub struct Surface {
    /// Linear RGB, as [`color::srgb`](crate::elements::util::color::srgb)
    /// returns it.
    pub color: [f32; 3],
    /// Microfacet roughness. 0 is a mirror, 1 is chalk.
    pub roughness: f32,
    /// Index of refraction. See
    /// [`material`](crate::elements::util::material).
    pub ior: f32,
    pub double_sided: bool,
}

/// What an organ needs to draw its geometry and export it as a reference.
///
/// Cloned onto every instance that drew the same representative, so they share
/// one mesh handle and one material — which is what lets Bevy batch them.
#[derive(Bundle, Clone)]
pub struct Geometry {
    pub reference: UsdReference,
    pub mesh: Mesh3d,
    pub material: MeshMaterial3d<StandardMaterial>,
}

/// The mesh library and the asset storage behind it, as one system param.
#[derive(SystemParam)]
pub struct Library<'w> {
    prototypes: ResMut<'w, Prototypes>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
}

impl Library<'_> {
    /// Drops everything `prefix` registered, ahead of rebuilding it.
    pub fn clear(&mut self, prefix: &str) {
        self.prototypes.clear_layer(prefix);
    }

    /// Registers `mesh` as `<prefix>_<index>` and hands back what an organ
    /// drawing it needs.
    pub fn part(&mut self, prefix: &str, index: usize, mesh: Mesh, surface: Surface) -> Geometry {
        let part = Part {
            mesh: self.meshes.add(mesh),
            color: surface.color,
            roughness: surface.roughness,
            ior: surface.ior,
            double_sided: surface.double_sided,
        };
        Geometry {
            mesh: Mesh3d(part.mesh.clone()),
            material: MeshMaterial3d(self.materials.add(part.material())),
            reference: UsdReference(self.prototypes.insert(prefix, index, part)),
        }
    }
}

/// The mesh library: every representative any layer has built, by name.
///
/// A layer registers its representatives under `<Layer>_<index>`, and
/// downstream layers read them from here — elements compose by typed value, not
/// by prim path.
///
/// A `BTreeMap` rather than a hash map, so iteration is in name order and an
/// exported document is byte-identical across runs without a sort.
#[derive(Resource, Default, Debug)]
pub struct Prototypes {
    parts: BTreeMap<String, Part>,
}

impl Prototypes {
    /// Names the part `<prefix>_<index>` and registers it.
    pub fn insert(&mut self, prefix: &str, index: usize, part: Part) -> String {
        let name = format!("{prefix}_{index}");
        self.parts.insert(name.clone(), part);
        name
    }

    pub fn get(&self, name: &str) -> Option<&Part> {
        self.parts.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Part)> {
        self.parts.iter()
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Drops everything one layer registered, ahead of it rebuilding.
    ///
    /// A rebuild may produce fewer representatives than the last one; without
    /// this the leftovers stay in the library, exported and unreferenced.
    pub fn clear_layer(&mut self, prefix: &str) {
        let head = format!("{prefix}_");
        self.parts.retain(|name, _| !name.starts_with(&head));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part() -> Part {
        Part {
            mesh: Handle::default(),
            color: [0.5, 0.5, 0.5],
            roughness: 0.8,
            ior: 1.5,
            double_sided: false,
        }
    }

    #[test]
    fn parts_are_named_by_layer_and_index() {
        let mut prototypes = Prototypes::default();
        assert_eq!(prototypes.insert("Leaf", 2, part()), "Leaf_2");
        assert!(prototypes.get("Leaf_2").is_some());
    }

    /// A rebuild that produces fewer representatives must not leave the extras
    /// behind, or the library grows every time a budget is lowered.
    #[test]
    fn clearing_a_layer_leaves_the_others_alone() {
        let mut prototypes = Prototypes::default();
        for i in 0..4 {
            prototypes.insert("Leaf", i, part());
        }
        prototypes.insert("Vine", 0, part());

        prototypes.clear_layer("Leaf");

        assert_eq!(prototypes.len(), 1);
        assert!(prototypes.get("Vine_0").is_some());
    }

    /// `LeafBlade_0` is not a `Leaf` part, and clearing one layer must not
    /// take the other with it.
    #[test]
    fn clearing_a_layer_matches_whole_names_only() {
        let mut prototypes = Prototypes::default();
        prototypes.insert("Leaf", 0, part());
        prototypes.insert("LeafBlade", 0, part());

        prototypes.clear_layer("Leaf");

        assert_eq!(prototypes.len(), 1);
        assert!(prototypes.get("LeafBlade_0").is_some());
    }

    /// The correction has to actually reach what is spawned under the root, not
    /// just be right in isolation: propagation stops at an entity with no
    /// `Transform`, and the scene below a broken link renders lying on its side.
    #[test]
    fn the_correction_reaches_everything_under_the_root() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, super::plugin));
        app.update();

        // A metre up in scene space, which is a metre up in world space too.
        let root = app.world().resource::<PrimRoot>().0;
        let marker = app
            .world_mut()
            .spawn((Transform::from_xyz(0.0, 0.0, 1.0), ChildOf(root)))
            .id();
        app.update();

        let placed = app.world().entity(marker).get::<GlobalTransform>().unwrap();
        assert!(
            (placed.translation() - Vec3::Y).length() < 1e-6,
            "scene +Z lands on world +Y, got {}",
            placed.translation()
        );
    }

    /// The rotation belongs to the viewer, so it must sit above the prim the
    /// export starts from — inside it, every prim in the file would be tipped.
    #[test]
    fn the_correction_sits_above_the_exported_root() {
        let mut app = App::new();
        app.add_plugins(super::plugin);
        app.update();

        let root = app.world().resource::<PrimRoot>().0;
        let entity = app.world().entity(root);
        assert!(entity.contains::<UsdRoot>());
        assert_eq!(
            entity.get::<Transform>().copied().unwrap_or_default(),
            Transform::IDENTITY,
            "the exported root is untransformed"
        );

        let parent = entity.get::<ChildOf>().expect("the root has a parent").0;
        assert_eq!(
            app.world().entity(parent).get::<Transform>().unwrap().rotation,
            z_up_to_y_up()
        );
    }

    /// The library iterates in name order, which is what makes an exported
    /// document reproducible without sorting it on the way out.
    #[test]
    fn the_library_iterates_in_name_order() {
        let mut prototypes = Prototypes::default();
        for i in [3, 0, 2, 1] {
            prototypes.insert("Vine", i, part());
        }
        let names: Vec<&str> = prototypes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Vine_0", "Vine_1", "Vine_2", "Vine_3"]);
    }
}
