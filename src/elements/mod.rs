//! Elements — the self-contained USD producers the scene is built from.
//!
//! One module per element, each holding its params resource, its `plugin`
//! wiring, its author system and its UI fragment. See the "Elements" section
//! of `README.md` for the rules they follow; the short version is that an
//! element owns exactly one prim subtree, rewrites it from scratch whenever
//! its params change, and composes with other elements by prim path alone.

pub mod cube;
pub mod grid;
pub mod parcel;
pub mod terrain;
pub mod usd;

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
    .add_plugins((cube::plugin, terrain::plugin, grid::plugin));
}

/// A plain snapshot of every element's params.
///
/// The world stores each fragment as its own resource so change detection is
/// per-element; this aggregate exists only to carry a full parameter set
/// across the boundaries where resources aren't available yet — Python calls
/// and headless generation.
#[derive(Clone, Debug, Default)]
pub struct VineyardParams {
    pub cube: cube::CubeParams,
    pub terrain: terrain::TerrainParams,
    pub grid: grid::GridParams,
    pub parcel: parcel::ParcelParams,
}

impl VineyardParams {
    /// Splits the aggregate back into the per-element resources the author
    /// systems actually read.
    pub fn insert(self, world: &mut World) {
        world.insert_resource(self.cube);
        world.insert_resource(self.terrain);
        world.insert_resource(self.grid);
        world.insert_resource(self.parcel);
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
}
