//! `misina-lab` — what a scene generator is built from.
//!
//! A generator such as `vinerylab` builds its scene in Bevy as ordinary meshes
//! and transforms and exports it as a [`SceneDoc`](scene::doc::SceneDoc), the
//! JSON its Python package turns into a USD stage. This crate holds the parts
//! of that which know nothing about what is being grown: the geometry kernels,
//! the quantizer, the seeded random stream, the scene graph and its export, the
//! viewer with its parameter panel, and the surfaces read off a params struct —
//! the Python classes, the config snippet, the generated stub and docs.
//!
//! A generator is one type implementing [`Generator`]: a marker naming its
//! params aggregate, its scene and its Python package, and the plugin that
//! adds its element plugins. Everything here is keyed on it —
//! [`viewer::app`], [`generate::scene`], [`ui::plugin`], [`snippet::cfg`],
//! [`codegen::targets`], [`testing::grown`]. The aggregate itself is declared
//! once, with [`generator!`], which writes the apply/read-back and the PyO3
//! glue from the field list.

pub mod codegen;
pub mod generate;
pub mod geometry;
pub mod palette;
pub mod params;
pub mod perf;
#[cfg(feature = "python")]
pub mod python;
pub mod quantize;
// Pipes frames to an `ffmpeg` child process, which the web has neither of.
#[cfg(not(target_arch = "wasm32"))]
pub mod record;
pub mod rng;
pub mod scene;
pub mod snippet;
pub mod stats;
pub mod terrain;
pub mod testing;
pub mod ui;
pub mod viewer;

use bevy::prelude::*;
use bevy::reflect::Typed;
use bevy::reflect::structs::Struct;

/// The one thing a generator implements. A zero-sized marker is enough:
///
/// ```ignore
/// pub struct Vineyard;
/// impl Generator for Vineyard {
///     type Params = elements::VineyardParams;
///     const NAME: &'static str = "Vineyard";
///     const PACKAGE: &'static str = "vinerylab";
///     fn plugin(app: &mut App) { elements::plugin(app) }
/// }
/// ```
pub trait Generator: 'static {
    /// The aggregate params struct, one field per fragment — see
    /// [`generator!`].
    type Params: Params;
    /// What the scene is called: the root prim, and the stem of the Python
    /// names — `Vineyard` gives `VineyardParams`, `VineyardCfg`,
    /// `VINEYARD_CFG` and `vineyard_cfg.py`.
    const NAME: &'static str;
    /// The Python package, which re-exports the `_core` extension module and
    /// holds `{PACKAGE}.usd` and `{PACKAGE}.isaaclab`.
    const PACKAGE: &'static str;
    /// Adds the element plugins. Their build systems sit in the generator's
    /// own stage sets, chained inside [`Build`].
    fn plugin(app: &mut App);

    /// The `{PACKAGE}_{name}` environment variable: `VINERYLAB_PERF` for
    /// vinerylab's `PERF`.
    fn env(name: &str) -> Option<std::ffi::OsString> {
        std::env::var_os(format!("{}_{name}", Self::PACKAGE.to_uppercase()))
    }
}

/// The aggregate of every element's params, as one plain value.
///
/// The world stores each fragment as its own resource so change detection is
/// per element; this is the whole parameter set for everything that has to
/// hold one — Python calls, headless generation, the panel's staged copy. A
/// reflected struct whose every field is a fragment, which is what the walk in
/// [`params`] reads. [`generator!`] writes the implementation.
pub trait Params: Struct + Typed + Clone + Default + PartialEq + std::fmt::Debug {
    /// Splits the aggregate into the per-element resources the build systems
    /// read. A fragment that already holds its value is left alone, so applying
    /// a set in which one slider moved re-runs that layer and no other.
    fn apply(&self, world: &mut World);
    /// Reads every fragment resource back out of `world`; the inverse of
    /// [`apply`](Self::apply). Not `from_world`, which Bevy's `FromWorld`
    /// already gives every `Default` type.
    fn read(world: &World) -> Self;
}

/// Everything that authors the scene, in `PreUpdate`. A generator chains its
/// own stage sets inside it; the panel commits before it and the perf marks
/// bracket it.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Build;
