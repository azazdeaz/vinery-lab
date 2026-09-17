//! Everything under `src/elements/` that isn't an element.
//!
//! An element owns a config, a params resource, a build system and a UI
//! fragment (see the "Elements" section of `README.md`). The modules here own
//! none of that on their own account; they are what elements are *built from*:
//!
//! - [`mesh`] — the geometry kernels' own mesh type, the bridge to Bevy's, and
//!   the primitives built directly in it.
//! - [`color`] — the palette, and the per-mesh jitter applied to it. Hands
//!   elements the linear RGB their geometry carries.
//! - [`material`] — the other half of the palette: how each surface responds to
//!   light.
//! - [`strand`] — the geometry kernel that skins a polyline of radii into a
//!   tube. Knows no botany.
//! - [`outline`] — the other geometry kernel: reads a shape traced in SVG and
//!   fills it with triangles. Knows no botany either.
//! - [`shapes`] — outlines built in code, in the frame a traced one is read
//!   into, for shapes cheaper to describe than to draw.
//! - [`scatter`] — points spread over a band of ground, for everything placed
//!   within a zone rather than along a line.
//! - [`parcel`] — the row-layout solver. Publishes [`parcel::VineyardLayout`]
//!   and the bands — rows, alleys — other elements place against, and builds
//!   nothing.
//! - [`planting`] — walks the solved layout and places a config on every plant
//!   and post. Owns the `Planting` subtree, but is driven by
//!   [`terrain`](super::terrain) rather than standing as an element in its own
//!   right — the same arrangement [`parcel`] has.
//!
//! The dividing line is *identity*, not file size: nothing here corresponds to
//! a thing that exists in a vineyard, so nothing here gets a mesh library or a
//! line in [`elements::plugin`](super::plugin).

pub mod color;
pub mod material;
pub mod mesh;
pub mod outline;
pub mod parcel;
pub mod planting;
pub mod scatter;
pub mod shapes;
pub mod strand;
#[cfg(test)]
pub mod testing;

use bevy::tasks::{ComputeTaskPool, ParallelSlice};

/// Maps `items` across the compute pool, handing each call its index.
///
/// Results come back in input order, so a build that maps its representatives
/// through here stays a function of the slice it was given — see the
/// determinism note on [`quantize`](crate::quantize).
///
/// On wasm the pool has no threads and this runs inline: correct, just not
/// faster.
pub fn par_map<T: Sync, R: Send + 'static>(
    items: &[T],
    f: impl Fn(usize, &T) -> R + Send + Sync,
) -> Vec<R> {
    // Paired with their indices up front, because the chunked map hands a
    // chunk its chunk number rather than the offset of the items in it.
    let indexed: Vec<(usize, &T)> = items.iter().enumerate().collect();
    indexed
        .par_splat_map(ComputeTaskPool::get(), None, |_, chunk| {
            chunk
                .iter()
                .map(|(index, item)| f(*index, item))
                .collect::<Vec<R>>()
        })
        .into_iter()
        .flatten()
        .collect()
}
