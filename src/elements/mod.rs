//! Elements — the self-contained USD producers the scene is built from.
//!
//! One module per element, each holding its params resource, its `plugin`
//! wiring, its author system and its UI fragment. See the "Elements" section
//! of `README.md` for the rules they follow; the short version is that an
//! element owns exactly one prim subtree, rewrites it from scratch whenever
//! its params change, and composes with other elements by prim path alone.
//!
//! What they are built *from* — authoring plumbing, the geometry kernel, the
//! layout solver, the placement helpers — lives in [`util`], which holds
//! everything under this directory that isn't an element.

pub mod leaf;
pub mod pole;
pub mod shoot;
pub mod terrain;
pub mod util;
pub mod vine;

use bevy::prelude::*;

/// Authoring order. Every element's author system goes in exactly one of
/// these; they run chained in `PreUpdate`, ahead of `LiveStagePlugin`'s
/// projection systems in `Update`.
///
/// `Prototypes` runs first so an element that instances another always finds
/// the prototype prims already on the stage — the path contract holds because
/// of this ordering, not by luck.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum Grow {
    /// Reusable geometry under `/parts`, instanced by everything downstream.
    Prototypes,
    /// The ground surface, and the field other elements sample to sit on it.
    Terrain,
    /// Where things go: rows, planting positions.
    Layout,
    /// Poles, trunks, vines placed along the layout.
    Plants,
    /// High-count scatter: leaves, grapes, weeds.
    Scatter,
    /// Re-rolls per-instance prototype choices without touching geometry.
    Randomize,
}

pub fn plugin(app: &mut App) {
    // Defaults to the reference-placed export shape, which is the one a
    // consumer can address. The viewer overrides it — see `viewer::run`.
    app.init_resource::<util::place::Style>();

    app.configure_sets(
        PreUpdate,
        (
            Grow::Prototypes,
            Grow::Terrain,
            Grow::Layout,
            Grow::Plants,
            Grow::Scatter,
            Grow::Randomize,
        )
            .chain(),
    )
    .add_plugins((
        terrain::plugin,
        pole::plugin,
        shoot::plugin,
        vine::plugin,
        leaf::plugin,
    ));
}

/// A plain snapshot of every element's params.
///
/// The world stores each fragment as its own resource so change detection is
/// per-element; this aggregate exists only to carry a full parameter set
/// across the boundaries where resources aren't available yet — Python calls
/// and headless generation.
#[derive(Clone, Debug, Default)]
pub struct VineyardParams {
    pub terrain: terrain::TerrainParams,
    pub parcel: util::parcel::ParcelParams,
    pub planting: util::planting::PlantingParams,
    pub pole: pole::PoleParams,
    pub vine: vine::VineParams,
    pub shoot: shoot::ShootParams,
    pub leaf: leaf::LeafParams,
}

impl VineyardParams {
    /// Splits the aggregate back into the per-element resources the author
    /// systems actually read.
    pub fn insert(self, world: &mut World) {
        world.insert_resource(self.terrain);
        world.insert_resource(self.parcel);
        world.insert_resource(self.planting);
        world.insert_resource(self.pole);
        world.insert_resource(self.vine);
        world.insert_resource(self.shoot);
        world.insert_resource(self.leaf);
    }

    /// Reads every element's params resource back out of `world`.
    ///
    /// The inverse of [`insert`](Self::insert), for the one caller that has a
    /// live world and needs a plain snapshot: the viewer's save key, which
    /// re-generates the scene headlessly rather than saving the stage it is
    /// previewing.
    pub fn from_world(world: &World) -> Self {
        Self {
            terrain: world.resource::<terrain::TerrainParams>().clone(),
            parcel: world.resource::<util::parcel::ParcelParams>().clone(),
            planting: world.resource::<util::planting::PlantingParams>().clone(),
            pole: world.resource::<pole::PoleParams>().clone(),
            vine: world.resource::<vine::VineParams>().clone(),
            shoot: world.resource::<shoot::ShootParams>().clone(),
            leaf: world.resource::<leaf::LeafParams>().clone(),
        }
    }
}

// ─── Variation picking ──────────────────────────────────────────────

/// Picks a prototype variation for each instance, deterministically from
/// `seed`.
///
/// The result is a `protoIndices` array: rewriting it is an attribute-only
/// edit, so re-rolling variations reprojects without resyncing the stage or
/// rebuilding any geometry.
pub fn variation_indices(seed: u64, instances: usize, variations: usize) -> Vec<i32> {
    if variations <= 1 {
        return vec![0; instances];
    }
    let mut state = seed;
    (0..instances)
        .map(|_| (split_mix_64(&mut state) % variations as u64) as i32)
        .collect()
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

    #[test]
    fn variation_indices_are_deterministic_and_in_range() {
        let a = variation_indices(42, 64, 3);
        let b = variation_indices(42, 64, 3);
        assert_eq!(a, b, "same seed gives the same picks");
        assert_eq!(a.len(), 64);
        assert!(a.iter().all(|i| (0..3).contains(i)), "indices stay in range");
        assert!(a.iter().any(|i| *i != a[0]), "picks actually vary");
    }

    #[test]
    fn a_single_variation_needs_no_randomness() {
        assert_eq!(variation_indices(7, 4, 1), vec![0; 4]);
        assert_eq!(variation_indices(7, 4, 0), vec![0; 4]);
    }

    #[test]
    fn reseeding_changes_the_picks() {
        assert_ne!(variation_indices(1, 64, 4), variation_indices(2, 64, 4));
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
