//! `vinerylab` — procedural vineyard scenes.
//!
//! The scene is built in Bevy as ordinary meshes and transforms, and comes out
//! as a [`SceneDoc`](misina_lab::scene::doc::SceneDoc): a plain JSON description that
//! `python/vinerylab/usd/build.py` turns into a USD stage. Usable either as an
//! interactive viewer ([`viewer::run`]) or headlessly from Python ([`python`],
//! behind the `python` feature).

pub mod codegen;
pub mod elements;
pub mod generate;
pub mod params;
pub mod perf;
// Pipes frames to an `ffmpeg` child process, which the web has neither of.
#[cfg(not(target_arch = "wasm32"))]
pub mod record;
pub mod scene;
pub mod snippet;
pub mod stats;
pub mod ui;
pub mod viewer;

#[cfg(feature = "python")]
mod python;
