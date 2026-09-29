//! `vinerylab` — procedural vineyard scenes.
//!
//! The scene is built in Bevy as ordinary meshes and transforms, and comes out
//! as a [`SceneDoc`](misina_lab::scene::doc::SceneDoc): a plain JSON
//! description that `python/misina-lab/misina_lab/usd/build.py` turns into a USD stage.
//! Usable either as an interactive viewer (`cargo run`) or headlessly from
//! Python (the `_core` module, behind the `python` feature).
//!
//! The vineyard itself is [`elements`]. Everything around it — the panel, the
//! scene graph and its export, the config snippet, the generated Python and
//! docs — is [`misina_lab`], keyed on [`Vineyard`].

pub mod elements;
pub mod perf;

use bevy::prelude::*;
use misina_lab::Generator;

/// The generator, as `misina_lab` sees it.
pub struct Vineyard;

impl Generator for Vineyard {
    type Params = elements::VineyardParams;
    const NAME: &'static str = "Vineyard";
    const PACKAGE: &'static str = "vinerylab";

    fn plugin(app: &mut App) {
        elements::plugin(app);
    }
}

/// The extension module is `vinerylab._core`, re-exported by the Python
/// package's `__init__.py` — PyO3 takes the module's init symbol from this
/// function's name, so it has to match the last component of `module-name`
/// in `pyproject.toml`. What it holds is written by the `generator!` call in
/// [`elements`].
#[cfg(feature = "python")]
#[pyo3::pymodule]
fn _core(m: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
    elements::module(m)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use misina_lab::codegen;

    /// The parameters page, relative to this crate; the Python targets are
    /// under `python/vinerylab/` and need no naming.
    const DOCS: &str = "../../docs/parameters.md";

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// Every generated region reads as the structs say now.
    #[test]
    fn generated_python_and_docs_are_fresh() {
        let stale: Vec<PathBuf> = codegen::stale::<Vineyard>(&root(), DOCS)
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert!(
            stale.is_empty(),
            "stale: {stale:?} — run `cargo test regen_params -- --ignored`"
        );
    }

    /// Rewrites the generated files from the structs.
    ///
    /// A dev tool rather than a test, and `#[ignore]`d for the same reason
    /// `dump_scene` is: it writes files and asserts nothing.
    #[test]
    #[ignore]
    fn regen_params() {
        for (path, fresh) in codegen::stale::<Vineyard>(&root(), DOCS) {
            std::fs::write(path, fresh).unwrap();
        }
    }

    /// Writes the current scene document out, for eyeballing the export or
    /// building a stage from it by hand:
    ///
    /// ```text
    /// cargo test dump_scene -- --ignored --nocapture
    /// python -m vinerylab.usd scene.json scene.usda --force
    /// ```
    #[test]
    #[ignore]
    fn dump_scene() {
        let doc =
            misina_lab::generate::scene::<Vineyard>(&elements::VineyardParams::default()).unwrap();
        let json = serde_json::to_string_pretty(&doc).unwrap();
        std::fs::write("scene.json", &json).unwrap();
        println!(
            "wrote scene.json: {} parts, {} bytes",
            doc.parts.len(),
            json.len()
        );
    }
}
