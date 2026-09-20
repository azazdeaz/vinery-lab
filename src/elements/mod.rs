//! Elements — the things a vineyard is made of, one module each.
//!
//! An element holds its params resource, its `plugin` wiring and its build
//! system; the viewer panel, the config snippet and the Python classes are
//! read off the params struct (see `docs/editing-parameters.md`). See the
//! "Elements" section of `DEVELOPMENT.md` for the rules they follow; the
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
//! [`Order`]: crate::scene::Order

pub mod cover;
pub mod leaf;
pub mod pole;
pub mod shoot;
pub mod terrain;
pub mod util;
pub mod vine;
pub mod weed;
pub mod wire;

use bevy::ecs::component::Mutable;
use bevy::prelude::*;

use crate::params::{Label, Slider};

/// Build order. Every layer's build system goes in exactly one of these, and
/// they run chained in `PreUpdate`.
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
            .chain(),
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

/// A plain snapshot of every element's params.
///
/// The world stores each fragment as its own resource so change detection is
/// per-element; this aggregate is the whole parameter set as one value, for
/// everything that has to hold one — Python calls, headless generation, and
/// the viewer panel's [`Staged`] copy.
#[derive(Reflect, Clone, Debug, Default, PartialEq)]
pub struct VineyardParams {
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

impl VineyardParams {
    /// Splits the aggregate back into the per-element resources the author
    /// systems actually read.
    ///
    /// A fragment that already holds its value is left alone rather than
    /// rewritten, so applying a set in which one slider moved re-runs that
    /// layer and no other.
    pub fn apply(&self, world: &mut World) {
        set(world, &self.scene);
        set(world, &self.terrain);
        set(world, &self.parcel);
        set(world, &self.planting);
        set(world, &self.pole);
        set(world, &self.wire);
        set(world, &self.vine);
        set(world, &self.shoot);
        set(world, &self.leaf);
        set(world, &self.cover);
        set(world, &self.weed);
    }

    /// Reads every element's params resource back out of `world`.
    ///
    /// The inverse of [`apply`](Self::apply), for the one caller that has a
    /// live world and needs a plain snapshot: the viewer's panel, seeding the
    /// copy its sliders write.
    pub fn from_world(world: &World) -> Self {
        Self {
            scene: world.resource::<SceneParams>().clone(),
            terrain: world.resource::<terrain::TerrainParams>().clone(),
            parcel: world.resource::<util::parcel::ParcelParams>().clone(),
            planting: world.resource::<util::planting::PlantingParams>().clone(),
            pole: world.resource::<pole::PoleParams>().clone(),
            wire: world.resource::<wire::WireParams>().clone(),
            vine: world.resource::<vine::VineParams>().clone(),
            shoot: world.resource::<shoot::ShootParams>().clone(),
            leaf: world.resource::<leaf::LeafParams>().clone(),
            cover: world.resource::<cover::CoverParams>().clone(),
            weed: world.resource::<weed::WeedParams>().clone(),
        }
    }
}

/// One fragment of [`VineyardParams::apply`]: inserts the resource if the
/// world has none, and otherwise overwrites it only if the value differs.
fn set<T: Resource<Mutability = Mutable> + Clone + PartialEq>(world: &mut World, value: &T) {
    match world.get_resource_mut::<T>() {
        Some(mut live) => {
            live.set_if_neq(value.clone());
        }
        None => world.insert_resource(value.clone()),
    }
}

/// SplitMix64. Inlined rather than pulled from `rand` because the only
/// requirement is that the same seed gives the same scene on every machine
/// and every run, which a fixed algorithm guarantees and a crate's default
/// generator does not promise across versions.
fn split_mix_64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Spreads a counter across the whole 64-bit range, so neighbouring indices
/// seed unrelated streams instead of the same one shifted by a step.
///
/// What a layer salts its streams with: a representative's mesh is built from
/// `seed ^ salt(index)`, and an instance's placement draws from
/// `seed ^ LAYER_STREAM ^ salt(order)`.
pub fn salt(index: u64) -> u64 {
    index.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// A deterministic stream of floats, for the shape and scatter randomness
/// elements need beyond picking a variation index.
///
/// Same inlined [`split_mix_64`] as the variation picker, for the same
/// reason: a fixed algorithm is what makes the scene reproducible across
/// machines and crate versions.
///
/// The *draw order* of a stream is part of an element's output. Inserting a
/// draw in the middle of a build re-rolls everything downstream of it, so
/// elements document their draw order where a reader would otherwise be
/// tempted to reorder it.
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next draw, uniform in `0.0..1.0`.
    pub fn unit(&mut self) -> f64 {
        // Top 53 bits, the mantissa width of an f64, for a uniform unit float.
        (split_mix_64(&mut self.state) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// The next draw, uniform in `lo..hi`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.unit() * (hi - lo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `apply` reaches every fragment: a set with every field moved lands in
    /// a bare world and reads back out whole. A fragment `apply` skipped would
    /// be missing from the world, which `from_world` panics on.
    #[test]
    fn apply_and_from_world_round_trip_every_fragment() {
        let params = crate::params::nudged();
        let mut world = World::new();
        params.apply(&mut world);
        assert_eq!(VineyardParams::from_world(&world), params);
    }

    fn draws(seed: u64, n: usize) -> Vec<f64> {
        let mut rng = Rng::new(seed);
        (0..n).map(|_| rng.unit()).collect()
    }

    #[test]
    fn rng_produces_the_same_stream_for_the_same_seed() {
        assert_eq!(draws(7, 32), draws(7, 32));
        assert_ne!(draws(7, 32), draws(8, 32));
    }

    #[test]
    fn rng_stays_within_its_range() {
        let mut rng = Rng::new(3);
        assert!((0..256).all(|_| (0.0..1.0).contains(&rng.unit())));
        assert!((0..256).all(|_| (-2.0..5.0).contains(&rng.range(-2.0, 5.0))));
    }

    /// A stream has to actually spread out, not sit near one value — a
    /// broken shift or divisor would still pass the range check above.
    #[test]
    fn rng_draws_spread_across_the_unit_interval() {
        let d = draws(11, 512);
        let mean = d.iter().sum::<f64>() / d.len() as f64;
        assert!((mean - 0.5).abs() < 0.05, "mean {mean} is near 0.5");
        assert!(d.iter().any(|v| *v < 0.05) && d.iter().any(|v| *v > 0.95));
    }
}
