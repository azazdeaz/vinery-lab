//! `misina-lab` — what a scene generator is built from.
//!
//! A generator such as `vinerylab` builds its scene in Bevy as ordinary meshes
//! and transforms; this crate holds the parts of that which know nothing about
//! what is being grown: the geometry kernels, the quantizer, the seeded random
//! stream and the JSON document a scene is exported as.

pub mod geometry;
pub mod quantize;
pub mod rng;
pub mod scene;
pub mod testing;
