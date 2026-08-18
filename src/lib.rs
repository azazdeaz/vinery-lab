//! `vinerylab` — procedural vineyard scenes, authored to USD via
//! [`usd_bevy`], usable either as an interactive Bevy viewer
//! ([`viewer::run`]) or headlessly from Python ([`python`], behind the
//! `python` feature) to generate a `.usda`/`.usd` file in one shot.

pub mod author;
pub mod generate;
pub mod scene;
pub mod viewer;

#[cfg(feature = "python")]
mod python;
