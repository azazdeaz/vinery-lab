//! PyO3 wrapper: each element's params fragment is already a `#[pyclass]`
//! (see `crate::elements`); this adds their constructors and the aggregate
//! Python actually calls.
//!
//! Kept deliberately thin — all the real work (spawning the headless app,
//! authoring the stage) lives in [`crate::generate`] and the element modules,
//! and is exercised identically by the interactive viewer.

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use crate::elements::VineyardParams;
use crate::elements::cube::CubeParams;
use crate::elements::grid::GridParams;
use crate::elements::terrain::TerrainParams;
use crate::generate::generate_stage;
use usd_bevy::authoring::save_stage_as;

/// Formats with `{:#}` so `anyhow`'s full context chain reaches the Python
/// traceback, not just the outermost error message.
fn to_py_err(err: anyhow::Error) -> PyErr {
    PyRuntimeError::new_err(format!("{err:#}"))
}

#[pymethods]
impl CubeParams {
    #[new]
    #[pyo3(signature = (size=0.1, variations=3))]
    fn py_new(size: f32, variations: u32) -> Self {
        Self { size, variations }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl TerrainParams {
    #[new]
    #[pyo3(signature = (width=4.0, height=4.0, max_elevation=0.5, detail=6))]
    fn py_new(width: f32, height: f32, max_elevation: f32, detail: u32) -> Self {
        Self {
            width,
            height,
            max_elevation,
            detail,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl GridParams {
    #[new]
    #[pyo3(signature = (rows=10, cols=10, spacing=0.2, seed=0))]
    fn py_new(rows: u32, cols: u32, spacing: f32, seed: u64) -> Self {
        Self {
            rows,
            cols,
            spacing,
            seed,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

/// The full parameter set, one field per element.
///
/// Fragments are held as `Py<T>` rather than by value so attribute access
/// hands back the *same* Python object every time. With plain fields PyO3's
/// generated getter clones, and `params.cube.size = 0.2` would mutate a
/// throwaway copy while the scene silently kept the old value.
#[pyclass(name = "VineyardParams", get_all, set_all)]
pub struct PyVineyardParams {
    pub cube: Py<CubeParams>,
    pub terrain: Py<TerrainParams>,
    pub grid: Py<GridParams>,
}

#[pymethods]
impl PyVineyardParams {
    #[new]
    #[pyo3(signature = (cube=None, terrain=None, grid=None))]
    fn py_new(
        py: Python<'_>,
        cube: Option<Py<CubeParams>>,
        terrain: Option<Py<TerrainParams>>,
        grid: Option<Py<GridParams>>,
    ) -> PyResult<Self> {
        Ok(Self {
            cube: match cube {
                Some(v) => v,
                None => Py::new(py, CubeParams::default())?,
            },
            terrain: match terrain {
                Some(v) => v,
                None => Py::new(py, TerrainParams::default())?,
            },
            grid: match grid {
                Some(v) => v,
                None => Py::new(py, GridParams::default())?,
            },
        })
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        format!("{:?}", self.snapshot(py))
    }

    /// Generates the scene and returns it as `usda` text.
    ///
    /// Releases the GIL for the duration of the Rust/Bevy work via
    /// `py.detach`: the params are copied out first, so nothing inside
    /// touches Python objects, and other Python threads (e.g. in a host
    /// application like Isaac Sim) keep making progress.
    fn generate_usda(&self, py: Python<'_>) -> PyResult<String> {
        let params = self.snapshot(py);
        py.detach(|| {
            let stage = generate_stage(&params)?;
            stage.root_layer().export_to_string()
        })
        .map_err(to_py_err)
    }

    /// Generates the scene and writes it directly to `path` (format chosen
    /// by extension — `.usda`, `.usdc`, `.usd`, `.usdz`).
    fn write_usd(&self, py: Python<'_>, path: &str) -> PyResult<()> {
        let params = self.snapshot(py);
        py.detach(|| {
            let stage = generate_stage(&params)?;
            save_stage_as(&stage, path)
        })
        .map_err(to_py_err)
    }
}

impl PyVineyardParams {
    /// Copies the fragments out of their Python objects into a plain Rust
    /// aggregate, so the generation call needs no GIL.
    fn snapshot(&self, py: Python<'_>) -> VineyardParams {
        VineyardParams {
            cube: (*self.cube.borrow(py)).clone(),
            terrain: (*self.terrain.borrow(py)).clone(),
            grid: (*self.grid.borrow(py)).clone(),
        }
    }
}

#[pymodule]
fn vinerylab(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyVineyardParams>()?;
    m.add_class::<CubeParams>()?;
    m.add_class::<TerrainParams>()?;
    m.add_class::<GridParams>()?;
    Ok(())
}
