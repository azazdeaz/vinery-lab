//! Everything under `elements/` that isn't an element.
//!
//! An element owns a config, a params resource, a build system and a UI
//! fragment (see the "Elements" section of `README.md`). The modules here own
//! none of that on their own account; they are what elements are *built from*:
//!
//! - [`color`] — the vineyard's colours, and the per-mesh jitter applied to
//!   them. Hands elements the linear RGB their geometry carries.
//! - [`material`] — the other half of the palette: how each surface responds to
//!   light.
//! - [`parcel`] — the row-layout solver. Publishes [`parcel::VineyardLayout`]
//!   and the bands — rows, alleys — other elements place against, and builds
//!   nothing.
//! - [`planting`] — walks the solved layout and places a config on every plant
//!   and post. Owns the `Planting` subtree, but is wired from
//!   [`elements::plugin`](super::plugin) after the ground rather than standing
//!   as an element in its own right — the same arrangement [`parcel`] has.
//!
//! The geometry kernels the elements build their meshes with — `mesh`,
//! `strand`, `outline`, `shapes`, `scatter` — know no botany at all and live in
//! [`misina_lab::geometry`], as does the machinery under the palette,
//! [`misina_lab::palette`]; they are re-exported here so an element reaches
//! them as `util::strand` and `color::srgb`.
//!
//! The dividing line is *identity*, not file size: nothing here corresponds to
//! a thing that exists in a vineyard, so nothing here gets a mesh library or a
//! line in [`elements::plugin`](super::plugin).

pub mod color;
pub mod material;
pub mod parcel;
pub mod planting;
#[cfg(test)]
pub mod testing;

pub use misina_lab::geometry::{mesh, outline, par_map, scatter, shapes, strand};
