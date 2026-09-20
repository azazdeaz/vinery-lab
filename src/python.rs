//! PyO3 wrapper: each element's params fragment is already a `#[pyclass]`
//! (see `crate::elements`); this adds their constructors and the aggregate
//! Python actually calls.
//!
//! A fragment's constructor is keyword-only and generic — `PoleParams(radius=0.05)`
//! sets fields by name through reflection — so a field added to a struct is
//! accepted here with nothing to update. The typed signature Python tooling
//! sees is in `_core.pyi`, generated from the same structs by
//! [`crate::codegen`].
//!
//! Kept deliberately thin — all the real work (spawning the headless app,
//! authoring the stage) lives in [`crate::generate`] and the element modules,
//! and is exercised identically by the interactive viewer.

use bevy::reflect::structs::Struct;
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::elements::SceneParams;
use crate::elements::VineyardParams;
use crate::elements::cover::CoverParams;
use crate::elements::leaf::LeafParams;
use crate::elements::pole::PoleParams;
use crate::elements::shoot::ShootParams;
use crate::elements::terrain::TerrainParams;
use crate::elements::util::parcel::ParcelParams;
use crate::elements::util::planting::PlantingParams;
use crate::elements::vine::VineParams;
use crate::elements::weed::WeedParams;
use crate::elements::wire::WireParams;
use crate::generate::generate_scene;
use crate::params::{self, Choices};

/// Formats with `{:#}` so `anyhow`'s full context chain reaches the Python
/// traceback, not just the outermost error message.
fn to_py_err(err: anyhow::Error) -> PyErr {
    PyRuntimeError::new_err(format!("{err:#}"))
}

/// `Params(**kwargs)`: every keyword names a field of the fragment. An unknown
/// name, or a value of the wrong type, is the `TypeError` Python would raise.
fn from_kwargs<T: Struct + Default>(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<T> {
    let mut params = T::default();
    let class = std::any::type_name::<T>()
        .rsplit("::")
        .next()
        .unwrap_or_default();
    for (key, value) in kwargs.into_iter().flat_map(|kwargs| kwargs.iter()) {
        let name: String = key.extract()?;
        let Some(field) = params.field_mut(&name) else {
            return Err(PyTypeError::new_err(format!(
                "{class}() got an unexpected keyword argument {name:?}"
            )));
        };
        let set = if let Some(f) = field.try_downcast_mut::<f32>() {
            value.extract().map(|v| *f = v)
        } else if let Some(i) = field.try_downcast_mut::<u32>() {
            value.extract().map(|v| *i = v)
        } else if let Some(i) = field.try_downcast_mut::<u64>() {
            value.extract().map(|v| *i = v)
        } else if let Some(b) = field.try_downcast_mut::<bool>() {
            value.extract().map(|v| *b = v)
        } else if let Some(s) = field.try_downcast_mut::<String>() {
            value.extract().map(|v| *s = v)
        } else {
            unreachable!("a params field is an f32, u32, u64, bool or String")
        };
        set.map_err(|err| PyTypeError::new_err(format!("{class}() argument {name:?}: {err}")))?;
    }
    Ok(params)
}

/// The Python face of every fragment — the keyword-only constructor and a
/// `__repr__` — and the one list the module registers them from.
macro_rules! py_params {
    ($($ty:ident),* $(,)?) => {
        $(
            #[pymethods]
            impl $ty {
                #[new]
                #[pyo3(signature = (**kwargs))]
                fn py_new(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                    from_kwargs(kwargs)
                }

                fn __repr__(&self) -> String {
                    format!("{self:?}")
                }
            }
        )*

        fn add_fragments(m: &Bound<'_, PyModule>) -> PyResult<()> {
            $( m.add_class::<$ty>()?; )*
            Ok(())
        }
    };
}

py_params!(
    SceneParams,
    TerrainParams,
    ParcelParams,
    PlantingParams,
    PoleParams,
    WireParams,
    VineParams,
    ShootParams,
    LeafParams,
    CoverParams,
    WeedParams,
);

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
    pub wire: Py<WireParams>,
    pub vine: Py<VineParams>,
    pub shoot: Py<ShootParams>,
    pub leaf: Py<LeafParams>,
    pub cover: Py<CoverParams>,
    pub weed: Py<WeedParams>,
}

#[pymethods]
impl PyVineyardParams {
    #[new]
    #[pyo3(signature = (
        scene=None, terrain=None, parcel=None, planting=None, pole=None, wire=None,
        vine=None, shoot=None, leaf=None, cover=None, weed=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        py: Python<'_>,
        scene: Option<Py<SceneParams>>,
        terrain: Option<Py<TerrainParams>>,
        parcel: Option<Py<ParcelParams>>,
        planting: Option<Py<PlantingParams>>,
        pole: Option<Py<PoleParams>>,
        wire: Option<Py<WireParams>>,
        vine: Option<Py<VineParams>>,
        shoot: Option<Py<ShootParams>>,
        leaf: Option<Py<LeafParams>>,
        cover: Option<Py<CoverParams>>,
        weed: Option<Py<WeedParams>>,
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
            wire: match wire {
                Some(v) => v,
                None => Py::new(py, WireParams::default())?,
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
            cover: match cover {
                Some(v) => v,
                None => Py::new(py, CoverParams::default())?,
            },
            weed: match weed {
                Some(v) => v,
                None => Py::new(py, WeedParams::default())?,
            },
        })
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        format!("{:?}", self.fragments(py))
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
        let params = self.snapshot(py)?;
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
    fn fragments(&self, py: Python<'_>) -> VineyardParams {
        VineyardParams {
            scene: (*self.scene.borrow(py)).clone(),
            terrain: (*self.terrain.borrow(py)).clone(),
            parcel: (*self.parcel.borrow(py)).clone(),
            planting: (*self.planting.borrow(py)).clone(),
            pole: (*self.pole.borrow(py)).clone(),
            wire: (*self.wire.borrow(py)).clone(),
            vine: (*self.vine.borrow(py)).clone(),
            shoot: (*self.shoot.borrow(py)).clone(),
            leaf: (*self.leaf.borrow(py)).clone(),
            cover: (*self.cover.borrow(py)).clone(),
            weed: (*self.weed.borrow(py)).clone(),
        }
    }

    /// The fragments, checked: every `@Choices` field holds one of its names.
    ///
    /// The one place a name a Python caller typed is rejected. Past here an
    /// unknown one is read as the default with a warning, which is right for
    /// a build system and wrong for a caller who misspelt a config.
    fn snapshot(&self, py: Python<'_>) -> PyResult<VineyardParams> {
        let set = self.fragments(py);
        for fragment in params::fragments() {
            for field in params::fields(fragment) {
                let Some(Choices(names)) = field.get_attribute::<Choices>() else {
                    continue;
                };
                let value = params::get(&set, fragment.name(), field.name())
                    .and_then(|value| value.try_downcast_ref::<String>())
                    .expect("a @Choices field is a String");
                if !names.contains(&value.as_str()) {
                    return Err(PyValueError::new_err(format!(
                        "{}.{} is {value:?}, which is none of {names:?}",
                        fragment.name(),
                        field.name()
                    )));
                }
            }
        }
        Ok(set)
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
    add_fragments(m)
}
