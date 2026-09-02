//! `vinerylab` — procedural vineyard scenes.
//!
//! The scene is built in Bevy as ordinary meshes and transforms, and comes out
//! as a [`SceneDoc`](scene::doc::SceneDoc): a plain JSON description that
//! `python/vinerylab/usd/build.py` turns into a USD stage. Usable either as an
//! interactive viewer ([`viewer::run`]) or headlessly from Python ([`python`],
//! behind the `python` feature).

pub mod elements;
pub mod generate;
pub mod perf;
pub mod quantize;
pub mod scene;
pub mod snippet;
pub mod ui;
pub mod viewer;

#[cfg(feature = "python")]
mod python;
