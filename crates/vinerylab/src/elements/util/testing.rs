//! The framework's test helpers — see [`misina_lab::testing`] — with the two
//! app fixtures bound to the vineyard, so an element's test needs no type
//! argument.

use bevy::prelude::*;

use crate::Vineyard;
use crate::elements::VineyardParams;
pub use misina_lab::testing::*;

/// One build cycle of the whole vineyard, handing back the app so a test can
/// read the scene graph and the resources it was built from — the solved
/// `VineyardLayout` and `Ground` are what a placement check is asserted
/// against.
pub fn grown(params: VineyardParams) -> App {
    misina_lab::testing::grown::<Vineyard>(params)
}

/// A headless app with the scene root and the mesh library in place, and no
/// elements — for a test that wires one build system by hand.
pub fn scene_app() -> App {
    misina_lab::testing::scene_app::<Vineyard>()
}
