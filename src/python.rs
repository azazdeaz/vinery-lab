//! PyO3 wrapper: `SceneParams` is already a `#[pyclass]` (see [`crate::scene`]);
//! this just adds the constructor and the two entry points Python calls.
//!
//! Kept deliberately thin — all the actual work (spawning the headless app,
//! authoring the stage) lives in [`crate::generate`] and [`crate::author`]
//! and is exercised identically by the interactive viewer, so this module
//! has nothing to test on its own beyond "does it call through correctly".

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use crate::generate::generate_stage;
use crate::scene::SceneParams;
use usd_bevy::authoring::save_stage_as;

/// Formats with `{:#}` so `anyhow`'s full context chain reaches the Python
/// traceback, not just the outermost error message.
fn to_py_err(err: anyhow::Error) -> PyErr {
    PyRuntimeError::new_err(format!("{err:#}"))
}

#[pymethods]
impl SceneParams {
    #[new]
    #[pyo3(signature = (rows=10, cols=10, spacing=0.2, cube_size=0.1))]
    fn py_new(rows: u32, cols: u32, spacing: f32, cube_size: f32) -> Self {
        Self { rows, cols, spacing, cube_size }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    /// Generates the scene and returns it as `usda` text.
    ///
    /// Releases the GIL for the duration of the Rust/Bevy work via
    /// `py.detach`: nothing in `generate_stage`/`export_to_string` touches
    /// Python objects, so it's sound, and it lets other Python threads (e.g.
    /// in a host application like Isaac Sim) keep making progress.
    fn generate_usda(&self, py: Python<'_>) -> PyResult<String> {
        py.detach(|| {
            let stage = generate_stage(self)?;
            stage.root_layer().export_to_string()
        })
        .map_err(to_py_err)
    }

    /// Generates the scene and writes it directly to `path` (format chosen
    /// by extension — `.usda`, `.usdc`, `.usd`, `.usdz`).
    fn write_usd(&self, py: Python<'_>, path: &str) -> PyResult<()> {
        py.detach(|| {
            let stage = generate_stage(self)?;
            save_stage_as(&stage, path)
        })
        .map_err(to_py_err)
    }
}

#[pymodule]
fn vinerylab(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<SceneParams>()?;
    Ok(())
}
