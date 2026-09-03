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
//! - [`parcel`] — the row-layout solver. Publishes [`parcel::VineyardLayout`]
//!   and builds nothing.
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
pub mod strand;
#[cfg(test)]
pub mod testing;
