//! Elements — the things a vineyard is made of, one module each.
//!
//! An element holds its params resource, its `plugin` wiring and its build
//! system; the viewer panel, the config snippet and the Python classes are
//! read off the params struct (see `docs/editing-parameters.md`). See the
//! "Elements" section of `docs/architecture.md` for the rules they follow; the
//! short version is that every element is one layer of the same pipeline:
//!
//! 1. **Collect** every config of its own kind, sorted by [`Order`].
//! 2. **Cluster** them to `params.variations` representatives.
//! 3. **Build** each representative's mesh once, into the shared library.
//! 4. **Assign** each instance the geometry of the representative it drew.
//! 5. **Expand** — author a *unique* config for the layer below at every frame
//!    the representative offers.
//!
//! Step 5 is where the layers meet: the frames come from the representative,
//! because a shoot has to sit on a spur that actually got built, while the
//! configs authored at them are per instance, so two plants off one mesh do not
//! carry the same canopy.
//!
//! What elements are built *from* — the geometry kernels, the palette, the
//! layout solver, the planting walk — lives in [`util`], which holds everything
//! under this directory that isn't an element.
//!
//! [`Order`]: misina_lab::scene::Order

pub mod cover;
pub mod leaf;
pub mod pole;
pub mod shoot;
pub mod terrain;
pub mod util;
pub mod vine;
pub mod weed;
pub mod wire;

use bevy::prelude::*;
use misina_lab::Build;
use misina_lab::params::{Label, Slider};
pub use misina_lab::rng::{Rng, salt};

/// Build order. Every layer's build system goes in exactly one of these, and
/// they run chained in `PreUpdate`, inside [`Build`].
///
/// The chain is what makes the pipeline a pipeline: a layer places the configs
/// the next one down clusters, so each set must have finished spawning before
/// the one after it queries.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum Grow {
    /// The ground surface, and the field other elements sample to sit on it.
    Terrain,
    /// Where things go: rows, planting positions.
    Layout,
    /// One config per plant and post, placed on the ground.
    Planting,
    /// Quantizes the posts and builds their meshes, and the wires between them.
    Poles,
    /// Quantizes the plants, builds their wood, and hangs a shoot config on
    /// every bud.
    Vines,
    /// Quantizes the shoots, builds their stems, and hangs a leaf config on
    /// every node.
    Shoots,
    /// High-count scatter: leaves, grapes, cover tiles, weeds. The floor
    /// layers author themselves here from the layout rather than from a
    /// layer above.
    Scatter,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<SceneParams>();

    app.configure_sets(
        PreUpdate,
        (
            Grow::Terrain,
            Grow::Layout,
            Grow::Planting,
            Grow::Poles,
            Grow::Vines,
            Grow::Shoots,
            Grow::Scatter,
        )
            .chain()
            .in_set(Build),
    )
    .add_plugins((
        terrain::plugin,
        pole::plugin,
        wire::plugin,
        shoot::plugin,
        vine::plugin,
        leaf::plugin,
        cover::plugin,
        weed::plugin,
    ));
}

/// Scene-wide parameters, owned by no element: one seed and one date for
/// everything on the ground.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct SceneParams {
    /// The one seed the whole scene is generated from. Every layer salts it
    /// with a constant of its own, so nudging one layer's knobs never re-rolls
    /// another: a new seed is a different vineyard, not a different trunk on
    /// the same one.
    ///
    /// See [`salt`] and the `*_STREAM` constants each element keeps. One seed
    /// rather than one per element because a scene is reproduced as a whole:
    /// a downstream simulator keys its cache on the params, and "which of
    /// three seeds moved" is not a question anyone was asking.
    #[reflect(@Slider { min: 0.0, max: 64.0, step: 1.0 })]
    pub seed: u64,
    /// Where in the growing season the scene is: `0.0` at budbreak, `1.0` at
    /// harvest. Today only the weeds read it, for which species are up and
    /// whether a bolter has bolted; the canopy does not yet.
    ///
    /// Scene-wide rather than an element's, because it is one date for
    /// everything on the ground.
    #[reflect(@Slider { min: 0.0, max: 1.0, step: 0.05 })]
    pub season: f32,
}

impl Default for SceneParams {
    fn default() -> Self {
        Self {
            seed: 0,
            season: 0.5,
        }
    }
}

misina_lab::generator! {
    /// A plain snapshot of every element's params.
    ///
    /// The world stores each fragment as its own resource so change detection
    /// is per-element; this aggregate is the whole parameter set as one value,
    /// for everything that has to hold one — Python calls, headless
    /// generation, and the viewer panel's staged copy.
    pub struct VineyardParams as PyVineyardParams("VineyardParams") for crate::Vineyard {
        pub scene: SceneParams,
        pub terrain: terrain::TerrainParams,
        pub parcel: util::parcel::ParcelParams,
        pub planting: util::planting::PlantingParams,
        pub pole: pole::PoleParams,
        pub wire: wire::WireParams,
        pub vine: vine::VineParams,
        pub shoot: shoot::ShootParams,
        pub leaf: leaf::LeafParams,
        pub cover: cover::CoverParams,
        #[reflect(@Label("Weeds"))]
        pub weed: weed::WeedParams,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use bevy::ecs::component::Mutable;
    use misina_lab::Params;
    use misina_lab::scene::doc::{Node, SceneDoc};
    use misina_lab::scene::export::scene_doc;
    use misina_lab::testing::{check_params, nudged};

    use super::*;
    use crate::Vineyard;

    /// What every field has to declare for the panel, the snippet and the
    /// generated docs to be built from it — see `docs/editing-parameters.md`.
    #[test]
    fn every_field_declares_what_the_panel_and_the_docs_need() {
        check_params::<VineyardParams>();
    }

    /// `apply` reaches every fragment: a set with every field moved lands in
    /// a bare world and reads back out whole. A fragment `apply` skipped would
    /// be missing from the world, which `read` panics on.
    #[test]
    fn apply_and_read_round_trip_every_fragment() {
        let params = nudged::<VineyardParams>();
        let mut world = World::new();
        params.apply(&mut world);
        assert_eq!(VineyardParams::read(&world), params);
    }

    fn generate(params: &VineyardParams) -> SceneDoc {
        misina_lab::generate::scene::<Vineyard>(params).expect("the parcel generates")
    }

    fn scene() -> SceneDoc {
        generate(&VineyardParams::default())
    }

    fn json(doc: &SceneDoc) -> String {
        serde_json::to_string(doc).expect("the document serializes")
    }

    /// The scene a default one already standing reaches when `change` is
    /// written to its `P` resource, beside the scene the same change generates
    /// from scratch.
    fn edited<P: Resource<Mutability = Mutable>>(
        pick: impl FnOnce(&mut VineyardParams) -> &mut P,
        change: impl Fn(&mut P),
    ) -> (String, String) {
        let mut want = VineyardParams::default();
        change(pick(&mut want));

        let mut app = util::testing::grown(VineyardParams::default());
        change(&mut app.world_mut().resource_mut::<P>());
        app.update();

        let live = scene_doc(app.world_mut()).expect("the edited scene exports");
        (json(&live), json(&generate(&want)))
    }

    /// An edit to a scene already standing has to land exactly where
    /// generating those params from scratch would have.
    ///
    /// A layer re-applies its own params to the configs already placed rather
    /// than having the layer above re-author them — see `leaf::reauthor` and
    /// `shoot::reauthor` — so the two paths are different code, and every
    /// other check here only ever exercises the second.
    #[test]
    fn an_edit_lands_where_generating_would_have() {
        for (what, (edited, generated)) in [
            (
                "a leaf edit",
                edited(|p| &mut p.leaf, |leaf| leaf.curl = 0.35),
            ),
            (
                "a shoot edit",
                edited(|p| &mut p.shoot, |shoot| shoot.length = 0.5),
            ),
            (
                "a vine edit",
                edited(|p| &mut p.vine, |vine| vine.trunk_height = 1.1),
            ),
            (
                "a seed edit",
                edited(|p| &mut p.scene, |scene| scene.seed = 7),
            ),
        ] {
            assert_eq!(edited, generated, "{what} drifted");
        }
    }

    /// Every prim in the document, depth first.
    fn walk<'a>(node: &'a Node, into: &mut Vec<(String, &'a Node)>, at: &str) {
        let path = format!("{at}/{}", node.name);
        for child in &node.children {
            walk(child, into, &path);
        }
        into.push((path, node));
    }

    fn prims(doc: &SceneDoc) -> Vec<(String, &Node)> {
        let mut found = Vec::new();
        walk(&doc.root, &mut found, "");
        found
    }

    /// Nothing may reach a consumer untinted. `displayColor` is the one
    /// channel both consumers read — the viewer draws it and USD carries it —
    /// so a part without one renders grey everywhere.
    ///
    /// And no two parts *of one layer* may share a shade: two different
    /// elements landing on the same colour is a palette choice, while two
    /// meshes of one element landing on it is a jitter stream wired to a
    /// constant seed, which every other check here would pass.
    ///
    /// Two layers are exceptions, both deliberate. A blade is shaded off the
    /// drawing it was cut from rather than off which mesh it came out as, so a
    /// budget spent on curls of one drawing shares that drawing's green — see
    /// [`leaf::surface`]. And every tube of a cane is the shade of the shoot
    /// mesh it stands in for, so that layer holds one shade per shoot
    /// representative rather than one per tube.
    #[test]
    fn every_part_is_tinted_and_no_layer_repeats_a_shade() {
        let doc = scene();
        assert!(doc.parts.len() > 5, "the walk found parts to check");

        let mut by_layer: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for part in &doc.parts {
            assert!(
                part.display_color.iter().any(|c| *c > 0.0),
                "{} is untinted",
                part.name
            );
            let layer = part.name.rsplit_once('_').expect("<Layer>_<index>").0;
            by_layer.entry(layer).or_default().push(&part.name);
        }

        // One shoot part per representative, which is what a cane's tubes are
        // shaded by.
        let representatives = by_layer.get(shoot::PART).map_or(0, Vec::len);

        for (layer, names) in &by_layer {
            let shades: BTreeSet<[u32; 3]> = names
                .iter()
                .map(|name| {
                    let part = doc.parts.iter().find(|p| &p.name == name).unwrap();
                    part.display_color.map(|c| c.to_bits())
                })
                .collect();
            let expected = if *layer == leaf::PART {
                names.len().min(leaf::SHAPES)
            } else if *layer == shoot::CANE {
                representatives
            } else {
                names.len()
            };
            assert_eq!(
                shades.len(),
                expected,
                "{layer}: two of {names:?} came out the same shade"
            );
        }
    }

    /// Every reference has to resolve, and a referencing prim has to be a leaf
    /// of the tree. Both are silent failures in USD: a dangling reference
    /// composes to an empty prim, and an `instanceable` prim's authored
    /// children are simply unreachable.
    ///
    /// A prim is instanceable unless the part it draws is its own collider —
    /// the ground, which has one instance and so shares nothing by giving
    /// instancing up.
    #[test]
    fn every_reference_resolves_to_a_part_and_carries_no_children() {
        let doc = scene();
        let parts: BTreeSet<&str> = doc.parts.iter().map(|p| p.name.as_str()).collect();

        let prims = prims(&doc);
        let referencing = prims.iter().filter(|(_, n)| n.reference.is_some()).count();
        assert!(
            referencing > 1000,
            "the scene draws geometry, got {referencing}"
        );

        for (path, node) in &prims {
            let Some(reference) = &node.reference else {
                continue;
            };
            assert!(
                parts.contains(reference.as_str()),
                "{path} draws a missing {reference}"
            );
            assert!(
                node.children.is_empty(),
                "{path} references and has children"
            );
            let solid = doc
                .parts
                .iter()
                .any(|part| &part.name == reference && part.collision.is_some());
            assert_eq!(
                node.instanceable,
                !solid,
                "{path} draws {reference}, which {} a collider",
                if solid { "carries" } else { "does not carry" }
            );
        }
    }

    /// A downstream Isaac Lab config is keyed on prim paths, so two prims may
    /// never share one — a name collision would silently repoint it.
    #[test]
    fn every_prim_path_is_unique() {
        let doc = scene();
        let prims = prims(&doc);
        let paths: BTreeSet<&str> = prims.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths.len(), prims.len(), "some prim path repeats");
    }

    /// A downstream sim keys its cache on these bytes, so the vineyard has to
    /// come out the same every time — no element may iterate a hash map into
    /// the document.
    #[test]
    fn generating_twice_gives_the_same_scene() {
        let (once, twice) = (json(&scene()), json(&scene()));
        assert!(once == twice, "byte for byte the same");
    }
}
