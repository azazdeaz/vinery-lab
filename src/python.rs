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
use crate::elements::shoot::ShootParams;
use crate::elements::terrain::TerrainParams;
use crate::elements::util::parcel::ParcelParams;
use crate::elements::util::planting::PlantingParams;
use crate::elements::vine::VineParams;
use crate::generate::generate_stage;
use usd_bevy::authoring::save_stage_as;

/// Formats with `{:#}` so `anyhow`'s full context chain reaches the Python
/// traceback, not just the outermost error message.
fn to_py_err(err: anyhow::Error) -> PyErr {
    PyRuntimeError::new_err(format!("{err:#}"))
}

#[pymethods]
impl TerrainParams {
    #[new]
    #[pyo3(signature = (width=80.0, height=50.0, max_elevation=3.0, detail=6))]
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
impl ParcelParams {
    #[new]
    #[pyo3(signature = (
        orientation=0.0,
        headland=6.0,
        row_spacing=2.4,
        vine_spacing=1.2,
        post_spacing=6.0,
        min_row_length=10.0,
        trellis_height=1.8,
    ))]
    fn py_new(
        orientation: f32,
        headland: f32,
        row_spacing: f32,
        vine_spacing: f32,
        post_spacing: f32,
        min_row_length: f32,
        trellis_height: f32,
    ) -> Self {
        Self {
            orientation,
            headland,
            row_spacing,
            vine_spacing,
            post_spacing,
            min_row_length,
            trellis_height,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl VineParams {
    #[new]
    #[pyo3(signature = (
        variations=4,
        seed=0,
        trunk_height=0.9,
        trunk_radius=0.035,
        trunk_wobble=0.02,
        arms=2,
        cordon_gap=0.15,
        cordon_radius=0.022,
        spur_spacing=0.12,
        spur_length=0.05,
        shoots_per_spur=1.8,
        roughness=0.14,
        sides=8,
        detail=20,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        variations: u32,
        seed: u64,
        trunk_height: f32,
        trunk_radius: f32,
        trunk_wobble: f32,
        arms: u32,
        cordon_gap: f32,
        cordon_radius: f32,
        spur_spacing: f32,
        spur_length: f32,
        shoots_per_spur: f32,
        roughness: f32,
        sides: u32,
        detail: u32,
    ) -> Self {
        Self {
            variations,
            seed,
            trunk_height,
            trunk_radius,
            trunk_wobble,
            arms,
            cordon_gap,
            cordon_radius,
            spur_spacing,
            spur_length,
            shoots_per_spur,
            roughness,
            sides,
            detail,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl PlantingParams {
    #[new]
    #[pyo3(signature = (seed=0, miss_rate=0.03, young_rate=0.08, young_scale=0.55))]
    fn py_new(seed: u64, miss_rate: f32, young_rate: f32, young_scale: f32) -> Self {
        Self {
            seed,
            miss_rate,
            young_rate,
            young_scale,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl ShootParams {
    #[new]
    #[pyo3(signature = (
        variations=4,
        seed=0,
        length=0.75,
        radius=0.006,
        lean=0.06,
        sides=6,
        detail=40,
    ))]
    fn py_new(
        variations: u32,
        seed: u64,
        length: f32,
        radius: f32,
        lean: f32,
        sides: u32,
        detail: u32,
    ) -> Self {
        Self {
            variations,
            seed,
            length,
            radius,
            lean,
            sides,
            detail,
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
/// generated getter clones, and `params.terrain.detail = 8` would mutate a
/// throwaway copy while the scene silently kept the old value.
#[pyclass(name = "VineyardParams", get_all, set_all)]
pub struct PyVineyardParams {
    pub terrain: Py<TerrainParams>,
    pub parcel: Py<ParcelParams>,
    pub planting: Py<PlantingParams>,
    pub vine: Py<VineParams>,
    pub shoot: Py<ShootParams>,
}

#[pymethods]
impl PyVineyardParams {
    #[new]
    #[pyo3(signature = (terrain=None, parcel=None, planting=None, vine=None, shoot=None))]
    fn py_new(
        py: Python<'_>,
        terrain: Option<Py<TerrainParams>>,
        parcel: Option<Py<ParcelParams>>,
        planting: Option<Py<PlantingParams>>,
        vine: Option<Py<VineParams>>,
        shoot: Option<Py<ShootParams>>,
    ) -> PyResult<Self> {
        Ok(Self {
            terrain: match terrain {
                Some(v) => v,
                None => Py::new(py, TerrainParams::default())?,
            },
            parcel: match parcel {
                Some(v) => v,
                None => Py::new(py, ParcelParams::default())?,
            },
            planting: match planting {
                Some(v) => v,
                None => Py::new(py, PlantingParams::default())?,
            },
            vine: match vine {
                Some(v) => v,
                None => Py::new(py, VineParams::default())?,
            },
            shoot: match shoot {
                Some(v) => v,
                None => Py::new(py, ShootParams::default())?,
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
            terrain: (*self.terrain.borrow(py)).clone(),
            parcel: (*self.parcel.borrow(py)).clone(),
            planting: (*self.planting.borrow(py)).clone(),
            vine: (*self.vine.borrow(py)).clone(),
            shoot: (*self.shoot.borrow(py)).clone(),
        }
    }
}

#[pymodule]
fn vinerylab(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyVineyardParams>()?;
    m.add_class::<TerrainParams>()?;
    m.add_class::<ParcelParams>()?;
    m.add_class::<PlantingParams>()?;
    m.add_class::<VineParams>()?;
    m.add_class::<ShootParams>()?;
    Ok(())
}
