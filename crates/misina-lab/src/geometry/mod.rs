//! The geometry kernels: what an element's meshes are built from.
//!
//! - [`mesh`] — the kernels' own mesh type, the bridge to Bevy's, and the
//!   primitives built directly in it.
//! - [`strand`] — skins a polyline of radii into a tube. Knows no botany.
//! - [`outline`] — reads a shape traced in SVG and fills it with triangles.
//!   Knows no botany either.
//! - [`shapes`] — outlines built in code, in the frame a traced one is read
//!   into, for shapes cheaper to describe than to draw.
//! - [`scatter`] — points spread over a band of ground, for everything placed
//!   within a zone rather than along a line.

pub mod mesh;
pub mod outline;
pub mod scatter;
pub mod shapes;
pub mod strand;

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
