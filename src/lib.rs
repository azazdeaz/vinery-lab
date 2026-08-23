//! `vinerylab` — procedural vineyard scenes, authored to USD via
//! [`usd_bevy`], usable either as an interactive Bevy viewer
//! ([`viewer::run`]) or headlessly from Python ([`python`], behind the
//! `python` feature) to generate a `.usda`/`.usd` file in one shot.

pub mod elements;
pub mod generate;
pub mod perf;
pub mod snippet;
pub mod stage;
pub mod ui;
pub mod viewer;

#[cfg(feature = "python")]
mod python;
