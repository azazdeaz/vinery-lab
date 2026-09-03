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
use crate::elements::SceneParams;
use crate::elements::leaf::LeafParams;
use crate::elements::pole::PoleParams;
use crate::elements::shoot::ShootParams;
use crate::elements::terrain::TerrainParams;
use crate::elements::util::parcel::ParcelParams;
use crate::elements::util::planting::PlantingParams;
use crate::elements::vine::VineParams;
use crate::generate::generate_scene;

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
impl PoleParams {
    #[new]
    #[pyo3(signature = (radius=0.04, sides=8))]
    fn py_new(radius: f32, sides: u32) -> Self {
        Self { radius, sides }
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
impl SceneParams {
    #[new]
    #[pyo3(signature = (seed=0))]
    fn py_new(seed: u64) -> Self {
        Self { seed }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl PlantingParams {
    #[new]
    #[pyo3(signature = (miss_rate=0.03, young_rate=0.08, young_scale=0.55))]
    fn py_new(miss_rate: f32, young_rate: f32, young_scale: f32) -> Self {
        Self {
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
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (
        variations=4,
        length=0.75,
        radius=0.006,
        lean=0.06,
        sides=6,
        detail=40,
        internode=0.07,
        leaf_droop=0.35,
    ))]
    fn py_new(
        variations: u32,
        length: f32,
        radius: f32,
        lean: f32,
        sides: u32,
        detail: u32,
        internode: f32,
        leaf_droop: f32,
    ) -> Self {
        Self {
            variations,
            length,
            radius,
            lean,
            sides,
            detail,
            internode,
            leaf_droop,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl LeafParams {
    #[new]
    #[pyo3(signature = (variations=5, detail=120))]
    fn py_new(variations: u32, detail: u32) -> Self {
        Self { variations, detail }
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
    pub scene: Py<SceneParams>,
    pub terrain: Py<TerrainParams>,
    pub parcel: Py<ParcelParams>,
    pub planting: Py<PlantingParams>,
    pub pole: Py<PoleParams>,
    pub vine: Py<VineParams>,
    pub shoot: Py<ShootParams>,
    pub leaf: Py<LeafParams>,
}

#[pymethods]
impl PyVineyardParams {
    #[new]
    #[pyo3(signature = (
        scene=None, terrain=None, parcel=None, planting=None, pole=None, vine=None,
        shoot=None, leaf=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        py: Python<'_>,
        scene: Option<Py<SceneParams>>,
        terrain: Option<Py<TerrainParams>>,
        parcel: Option<Py<ParcelParams>>,
        planting: Option<Py<PlantingParams>>,
        pole: Option<Py<PoleParams>>,
        vine: Option<Py<VineParams>>,
        shoot: Option<Py<ShootParams>>,
        leaf: Option<Py<LeafParams>>,
    ) -> PyResult<Self> {
        Ok(Self {
            scene: match scene {
                Some(v) => v,
                None => Py::new(py, SceneParams::default())?,
            },
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
            pole: match pole {
                Some(v) => v,
                None => Py::new(py, PoleParams::default())?,
            },
            vine: match vine {
                Some(v) => v,
                None => Py::new(py, VineParams::default())?,
            },
            shoot: match shoot {
                Some(v) => v,
                None => Py::new(py, ShootParams::default())?,
            },
            leaf: match leaf {
                Some(v) => v,
                None => Py::new(py, LeafParams::default())?,
            },
        })
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        format!("{:?}", self.snapshot(py))
    }

    /// Generates the scene and returns it as a JSON document.
    ///
    /// The whole contract with the USD builder — see `src/scene/doc.rs`. Public
    /// so a caller can cache the bytes, diff two scenes, or build the stage on
    /// another machine.
    ///
    /// Releases the GIL for the Rust/Bevy work via `py.detach`: the params are
    /// copied out first, so nothing inside touches Python objects and other
    /// threads in a host application like Isaac Sim keep making progress.
    fn generate_scene_json(&self, py: Python<'_>) -> PyResult<String> {
        let params = self.snapshot(py);
        py.detach(|| -> anyhow::Result<String> {
            Ok(serde_json::to_string(&generate_scene(&params)?)?)
        })
        .map_err(to_py_err)
    }

    /// Generates the scene and writes it to `path` as USD.
    ///
    /// The extension decides the format: `.usd`/`.usdc` for the binary crate
    /// form (about a third the bytes and roughly 4x faster for USD to parse),
    /// `.usda` for text. The file must not already exist.
    ///
    /// Rust owns the scene and Python owns USD, so this hands the document to
    /// `vinerylab.usd.build_usd` — the only place in the project that knows
    /// what USD is. That import needs `usd-core`, which is a dependency of this
    /// package.
    fn write_usd(&self, py: Python<'_>, path: &str) -> PyResult<()> {
        let document = self.generate_scene_json(py)?;
        let doc = py.import("json")?.call_method1("loads", (document,))?;
        py.import("vinerylab.usd")?
            .call_method1("build_usd", (doc, path))?;
        Ok(())
    }
}

impl PyVineyardParams {
    /// Copies the fragments out of their Python objects into a plain Rust
    /// aggregate, so the generation call needs no GIL.
    fn snapshot(&self, py: Python<'_>) -> VineyardParams {
        VineyardParams {
            scene: (*self.scene.borrow(py)).clone(),
            terrain: (*self.terrain.borrow(py)).clone(),
            parcel: (*self.parcel.borrow(py)).clone(),
            planting: (*self.planting.borrow(py)).clone(),
            pole: (*self.pole.borrow(py)).clone(),
            vine: (*self.vine.borrow(py)).clone(),
            shoot: (*self.shoot.borrow(py)).clone(),
            leaf: (*self.leaf.borrow(py)).clone(),
        }
    }
}

/// The extension module is `vinerylab._core`, re-exported by the Python
/// package's `__init__.py` — PyO3 takes the module's init symbol from this
/// function's name, so it has to match the last component of
/// `module-name` in `pyproject.toml`.
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Part of the cache key `vinerylab.isaaclab` builds: the same params
    // authored by a different generator are a different scene.
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<PyVineyardParams>()?;
    m.add_class::<SceneParams>()?;
    m.add_class::<TerrainParams>()?;
    m.add_class::<ParcelParams>()?;
    m.add_class::<PlantingParams>()?;
    m.add_class::<PoleParams>()?;
    m.add_class::<VineParams>()?;
    m.add_class::<ShootParams>()?;
    m.add_class::<LeafParams>()?;
    Ok(())
}
