//! Everything under `src/elements/` that isn't an element.
//!
//! An element owns a prim subtree, a params resource, an author system and a UI
//! fragment (see the "Elements" section of `README.md`). The modules here own
//! none of that on their own account; they are what elements are *built from*:
//!
//! - [`usd`] — authoring plumbing over `openusd`'s typed schemas: meshes,
//!   internal references, mesh merging.
//! - [`color`] — the palette, and the per-variation jitter applied to it.
//!   Authors no USD; hands elements the linear RGB their meshes carry.
//! - [`material`] — `UsdPreviewSurface` networks and the binding helper, plus
//!   the composition rule that decides where in the namespace they may live.
//! - [`strand`] — the geometry kernel that skins a polyline of radii into a
//!   tube. Knows no botany and authors no USD.
//! - [`outline`] — the other geometry kernel: reads a shape traced in SVG and
//!   fills it with triangles. Knows no botany either.
//! - [`parcel`] — the row-layout solver. Publishes [`parcel::VineyardLayout`]
//!   and authors nothing.
//! - [`place`] — the two ways an element's prototypes get put on the ground:
//!   one addressable prim each, or one `PointInstancer` for all of them.
//! - [`planting`] — walks the solved layout and places the plants. Owns
//!   `/Vineyard/Planting`, but is driven by [`terrain`](super::terrain) rather
//!   than standing as an element in its own right — the same arrangement
//!   [`parcel`] has.
//!
//! The dividing line is *identity*, not file size: nothing here corresponds to
//! a thing that exists in a vineyard, so nothing here gets a `/parts/<Name>`
//! prototype or a line in [`elements::plugin`](super::plugin).

pub mod color;
pub mod material;
pub mod outline;
pub mod parcel;
pub mod place;
pub mod planting;
pub mod strand;
#[cfg(test)]
pub mod testing;
pub mod usd;
