//! What the Python side of [`generator!`](crate::generator) calls: the
//! keyword constructor every fragment shares, the choice check, and the two
//! entry points that generate a scene.
//!
//! A fragment's constructor is keyword-only and generic — `PoleParams(radius=0.05)`
//! sets fields by name through reflection — so a field added to a struct is
//! accepted here with nothing to update. The typed signature Python tooling
//! sees is in the generator's `_core.pyi`, generated from the same structs by
//! [`codegen`](crate::codegen).
//!
//! Kept deliberately thin — all the real work (spawning the headless app,
//! authoring the stage) lives in [`generate`](crate::generate) and the
//! generator's element modules, and is exercised identically by the viewer.

use bevy::reflect::structs::Struct;
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::params::{self, Choices};
use crate::{Generator, Params};

/// Formats with `{:#}` so `anyhow`'s full context chain reaches the Python
/// traceback, not just the outermost error message.
fn to_py_err(err: anyhow::Error) -> PyErr {
    PyRuntimeError::new_err(format!("{err:#}"))
}

/// `Params(**kwargs)`: every keyword names a field of the fragment. An unknown
/// name, or a value of the wrong type, is the `TypeError` Python would raise.
pub fn from_kwargs<T: Struct + Default>(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<T> {
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

/// `params`, checked: every `@Choices` field holds one of its names.
///
/// The one place a name a Python caller typed is rejected. Past here an
/// unknown one is read as the default with a warning, which is right for a
/// build system and wrong for a caller who misspelt a config.
pub fn checked<P: Params>(params: P) -> PyResult<P> {
    for fragment in params::fragments::<P>() {
        for field in params::fields(fragment) {
            let Some(Choices(names)) = field.get_attribute::<Choices>() else {
                continue;
            };
            let value = params::get(&params, fragment.name(), field.name())
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
    Ok(params)
}

/// Generates the scene and returns it as a JSON document.
///
/// The whole contract with the USD builder — see [`doc`](crate::scene::doc).
/// Public on the Python class so a caller can cache the bytes, diff two
/// scenes, or build the stage on another machine.
///
/// Releases the GIL for the Rust/Bevy work via `py.detach`: the params are a
/// plain copy, so nothing inside touches Python objects and other threads in
/// a host application like Isaac Sim keep making progress.
pub fn scene_json<G: Generator>(py: Python<'_>, params: &G::Params) -> PyResult<String> {
    py.detach(|| -> anyhow::Result<String> {
        Ok(serde_json::to_string(&crate::generate::scene::<G>(
            params,
        )?)?)
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
/// `{PACKAGE}.usd.build_usd` — the only place in a generator that knows what
/// USD is. That import needs `usd-core`, which is a dependency of the
/// package.
pub fn write_usd<G: Generator>(py: Python<'_>, params: &G::Params, path: &str) -> PyResult<()> {
    let document = scene_json::<G>(py, params)?;
    let doc = py.import("json")?.call_method1("loads", (document,))?;
    py.import("misina_lab.usd")?
        .call_method1("build_usd", (doc, path))?;
    Ok(())
}
