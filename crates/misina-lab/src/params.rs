//! What a params field declares about itself, and the walk that reads it.
//!
//! Every `*Params` struct derives `Reflect`, so its field names, types, doc
//! comments and `#[reflect(@...)]` attributes can be read at run time. The
//! viewer panel, the config snippet, the Python constructors and the generated
//! Python files are written against that walk rather than against the fields
//! themselves, which is what lets a parameter be declared once, in its struct.
//! `docs/editing-parameters.md` is the authoring guide;
//! [`testing::check_params`](crate::testing::check_params) is what holds a
//! struct to it.
//!
//! A field's doc comment is read in two parts. The **first paragraph** is the
//! user-facing description — the tooltip, the Python docstring and the row in
//! `docs/parameters.md` — so it has to stand on its own and may not use
//! rustdoc link syntax. Anything after it is for a reader of the Rust. A
//! struct's doc comment is user-facing in full: it becomes the class docstring.
//!
//! The aggregate the walk starts from is declared with
//! [`generator!`](crate::generator).

use std::any::TypeId;

use bevy::ecs::component::Mutable;
use bevy::prelude::*;
use bevy::reflect::structs::{Struct, StructInfo};
use bevy::reflect::{NamedField, PartialReflect, ReflectMut, ReflectRef, TypeInfo};

use crate::Params;

/// A numeric field's slider: the range the panel offers and the step it moves
/// in. The display precision follows from the step.
#[derive(Reflect, Clone, Copy, Debug)]
pub struct Slider {
    pub min: f32,
    pub max: f32,
    pub step: f32,
}

/// The caption a field gets in the panel, where its name is not the wording
/// wanted on screen. Absent, the name is shown with its underscores as spaces.
#[derive(Reflect, Clone, Debug)]
pub struct Label(pub &'static str);

/// The fixed list of names a string field is one of. The panel offers them as
/// a dropdown, and Python rejects any other name.
#[derive(Reflect, Clone, Debug)]
#[reflect(opaque)]
pub struct Choices(pub &'static [&'static str]);

/// The control a field gets in the panel, decided by its attributes.
#[derive(Clone, Copy, Debug)]
pub enum Widget {
    Slider(Slider),
    Dropdown(&'static [&'static str]),
    Checkbox,
}

// ─── The aggregate ──────────────────────────────────────────────────

/// Declares a generator's params aggregate from its field list, once.
///
/// ```ignore
/// misina_lab::generator! {
///     /// A plain snapshot of every element's params.
///     pub struct VineyardParams as PyVineyardParams("VineyardParams") for crate::Vineyard {
///         pub scene: SceneParams,
///         pub terrain: misina_lab::terrain::TerrainParams as core,
///         #[reflect(@Label("Weeds"))]
///         pub weed: weed::WeedParams,
///     }
/// }
/// ```
///
/// Emits the struct with `Reflect, Clone, Debug, Default, PartialEq` derived,
/// its [`Params`] implementation, and — under the calling crate's `python`
/// feature — the `#[pyclass]` aggregate Python sees under the quoted name,
/// holding one `Py<T>` per fragment with a keyword-only constructor,
/// `__repr__`, `generate_scene_json` and `write_usd`; a keyword constructor
/// and `__repr__` for every fragment ([`fragment_python!`](crate::fragment_python)); and
/// `fn module(m)`, which registers all of them and `__version__` on the
/// extension module. Every fragment is a `Resource` deriving
/// `Reflect, Clone, Debug, Default, PartialEq`, with the `pyclass` attribute
/// `docs/editing-parameters.md` shows.
///
/// A fragment this crate declares — [`terrain::TerrainParams`](crate::terrain::TerrainParams)
/// — already carries its constructor, and PyO3 accepts a `#[pymethods]` block
/// only in the crate that declared the type; `as core` after the field's
/// type tells the macro not to emit a second one.
///
/// Fragments are held as `Py<T>` rather than by value so attribute access
/// hands back the *same* Python object every time. With plain fields PyO3's
/// generated getter clones, and `params.terrain.detail = 8` would mutate a
/// throwaway copy while the scene silently kept the old value.
#[macro_export]
macro_rules! generator {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident as $py:ident($py_name:literal) for $generator:ty {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $ty:ty $(as $origin:ident)?
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(::bevy::prelude::Reflect, Clone, Debug, Default, PartialEq)]
        $vis struct $name {
            $( $(#[$field_meta])* $field_vis $field: $ty, )*
        }

        impl $crate::Params for $name {
            fn apply(&self, world: &mut ::bevy::prelude::World) {
                $( $crate::params::set(world, &self.$field); )*
            }

            fn read(world: &::bevy::prelude::World) -> Self {
                Self { $( $field: world.resource::<$ty>().clone(), )* }
            }
        }

        #[cfg(feature = "python")]
        #[::pyo3::pyclass(name = $py_name, get_all, set_all)]
        $vis struct $py {
            $( $field_vis $field: ::pyo3::Py<$ty>, )*
        }

        #[cfg(feature = "python")]
        #[::pyo3::pymethods]
        impl $py {
            #[new]
            #[pyo3(signature = ( $( $field = None ),* ))]
            #[allow(clippy::too_many_arguments)]
            fn py_new(
                py: ::pyo3::Python<'_>,
                $( $field: Option<::pyo3::Py<$ty>>, )*
            ) -> ::pyo3::PyResult<Self> {
                Ok(Self {
                    $( $field: match $field {
                        Some(fragment) => fragment,
                        None => ::pyo3::Py::new(py, <$ty>::default())?,
                    }, )*
                })
            }

            fn __repr__(&self, py: ::pyo3::Python<'_>) -> String {
                format!("{:?}", self.fragments(py))
            }

            /// Generates the scene and returns it as a JSON document.
            fn generate_scene_json(&self, py: ::pyo3::Python<'_>) -> ::pyo3::PyResult<String> {
                $crate::python::scene_json::<$generator>(py, &self.snapshot(py)?)
            }

            /// Generates the scene and writes it to `path` as USD.
            fn write_usd(&self, py: ::pyo3::Python<'_>, path: &str) -> ::pyo3::PyResult<()> {
                $crate::python::write_usd::<$generator>(py, &self.snapshot(py)?, path)
            }
        }

        #[cfg(feature = "python")]
        impl $py {
            /// The fragments copied out of their Python objects into the plain
            /// aggregate, so the generation call needs no GIL.
            fn fragments(&self, py: ::pyo3::Python<'_>) -> $name {
                $name { $( $field: (*self.$field.borrow(py)).clone(), )* }
            }

            /// The fragments, checked: every `@Choices` field holds one of
            /// its names.
            fn snapshot(&self, py: ::pyo3::Python<'_>) -> ::pyo3::PyResult<$name> {
                $crate::python::checked(self.fragments(py))
            }
        }

        $( $crate::fragment_python!($ty $(, $origin)?); )*

        /// Registers the aggregate, every fragment and `__version__` on the
        /// extension module.
        ///
        /// The version is part of the cache key the Isaac Lab spawner builds:
        /// the same params authored by a different generator are a different
        /// scene.
        #[cfg(feature = "python")]
        $vis fn module(
            m: &::pyo3::Bound<'_, ::pyo3::types::PyModule>,
        ) -> ::pyo3::PyResult<()> {
            use ::pyo3::types::PyModuleMethods as _;
            m.add("__version__", env!("CARGO_PKG_VERSION"))?;
            m.add_class::<$py>()?;
            $( m.add_class::<$ty>()?; )*
            Ok(())
        }
    };
}

/// What Python sees of one fragment, under the calling crate's `python`
/// feature: a keyword constructor (`PoleParams(height=2.0)`) and `__repr__`.
///
/// [`generator!`](crate::generator) emits it for every field of the aggregate;
/// a fragment declared in this crate emits its own, and its field carries
/// `as core`, which matches the second arm here and emits nothing.
#[macro_export]
macro_rules! fragment_python {
    ($ty:ty) => {
        #[cfg(feature = "python")]
        #[::pyo3::pymethods]
        impl $ty {
            #[new]
            #[pyo3(signature = (**kwargs))]
            fn py_new(
                kwargs: Option<&::pyo3::Bound<'_, ::pyo3::types::PyDict>>,
            ) -> ::pyo3::PyResult<Self> {
                $crate::python::from_kwargs(kwargs)
            }

            fn __repr__(&self) -> String {
                format!("{self:?}")
            }
        }
    };
    ($ty:ty, core) => {};
}

/// One fragment of [`Params::apply`]: inserts the resource if the world has
/// none, and otherwise overwrites it only if the value differs.
pub fn set<T: Resource<Mutability = Mutable> + Clone + PartialEq>(world: &mut World, value: &T) {
    match world.get_resource_mut::<T>() {
        Some(mut live) => {
            live.set_if_neq(value.clone());
        }
        None => world.insert_resource(value.clone()),
    }
}

// ─── The walk ───────────────────────────────────────────────────────

/// The fragments of a params aggregate, in declaration order.
pub fn fragments<P: Params>() -> impl Iterator<Item = &'static NamedField> {
    let TypeInfo::Struct(info) = P::type_info() else {
        unreachable!("a `Params` aggregate is a struct");
    };
    info.iter()
}

/// The fields of one fragment, in declaration order.
pub fn fields(fragment: &NamedField) -> impl Iterator<Item = &'static NamedField> {
    fragment_info(fragment).iter()
}

/// The struct behind a fragment field.
pub fn fragment_info(fragment: &NamedField) -> &'static StructInfo {
    match fragment.type_info() {
        Some(TypeInfo::Struct(info)) => info,
        _ => panic!("fragment `{}` is not a reflected struct", fragment.name()),
    }
}

/// The name a fragment goes by everywhere: `Pole` for `PoleParams`, which
/// Python spells `PoleParams` and the Isaac Lab cfg `PoleCfg`.
pub fn stem(fragment: &NamedField) -> &'static str {
    let name = fragment_info(fragment).type_path_table().short_path();
    name.strip_suffix("Params")
        .unwrap_or_else(|| panic!("{name}: a fragment's type is named `<Stem>Params`"))
}

/// The panel caption of a field, or the section title of a fragment.
pub fn label(field: &NamedField) -> String {
    match field.get_attribute::<Label>() {
        Some(Label(text)) => (*text).to_string(),
        None => {
            let mut text = field.name().replace('_', " ");
            if let Some(first) = text.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            text
        }
    }
}

pub fn widget(field: &NamedField) -> Widget {
    if let Some(slider) = field.get_attribute::<Slider>() {
        Widget::Slider(*slider)
    } else if let Some(Choices(names)) = field.get_attribute::<Choices>() {
        Widget::Dropdown(names)
    } else if field.type_id() == TypeId::of::<bool>() {
        Widget::Checkbox
    } else {
        panic!(
            "`{}`: a numeric field carries a `@Slider`, a string field a `@Choices`",
            field.name()
        )
    }
}

// ─── Docs ───────────────────────────────────────────────────────────

/// A doc comment as paragraphs of plain prose: rustdoc's leading space and the
/// line wrapping dropped, one string per paragraph.
pub fn paragraphs(docs: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in docs.unwrap_or_default().lines().map(str::trim) {
        if line.is_empty() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// The user-facing description of a field: the first paragraph of its docs.
pub fn summary(field: &NamedField) -> String {
    paragraphs(field.docs())
        .into_iter()
        .next()
        .unwrap_or_default()
}

// ─── Values ─────────────────────────────────────────────────────────

/// A field's value on a params set, by fragment and field name.
pub fn get<'a>(
    params: &'a dyn Struct,
    fragment: &str,
    field: &str,
) -> Option<&'a dyn PartialReflect> {
    match params.field(fragment)?.reflect_ref() {
        ReflectRef::Struct(fragment) => fragment.field(field),
        _ => None,
    }
}

pub fn get_mut<'a>(
    params: &'a mut dyn Struct,
    fragment: &str,
    field: &str,
) -> Option<&'a mut dyn PartialReflect> {
    match params.field_mut(fragment)?.reflect_mut() {
        ReflectMut::Struct(fragment) => fragment.field_mut(field),
        _ => None,
    }
}

/// A numeric field's value as a slider holds it.
pub fn number(value: &dyn PartialReflect) -> Option<f32> {
    value
        .try_downcast_ref::<f32>()
        .copied()
        .or_else(|| value.try_downcast_ref::<u32>().map(|v| *v as f32))
        .or_else(|| value.try_downcast_ref::<u64>().map(|v| *v as f32))
}

/// Writes a slider's value to a numeric field, rounded for an integer one.
pub fn set_number(field: &mut dyn PartialReflect, value: f32) {
    if let Some(v) = field.try_downcast_mut::<f32>() {
        *v = value;
    } else if let Some(v) = field.try_downcast_mut::<u32>() {
        *v = value.round().max(0.0) as u32;
    } else if let Some(v) = field.try_downcast_mut::<u64>() {
        *v = value.round().max(0.0) as u64;
    }
}

// ─── Python ─────────────────────────────────────────────────────────

/// The Python type a field's value has.
pub fn python_type(field: &NamedField) -> &'static str {
    let id = field.type_id();
    if id == TypeId::of::<f32>() {
        "float"
    } else if id == TypeId::of::<u32>() || id == TypeId::of::<u64>() {
        "int"
    } else if id == TypeId::of::<bool>() {
        "bool"
    } else if id == TypeId::of::<String>() {
        "str"
    } else {
        panic!(
            "`{}`: a params field is an f32, u32, u64, bool or String",
            field.name()
        )
    }
}

/// A field's value as a Python literal.
pub fn python_literal(value: &dyn PartialReflect) -> String {
    if let Some(v) = value.try_downcast_ref::<f32>() {
        python_float(*v)
    } else if let Some(v) = value.try_downcast_ref::<u32>() {
        v.to_string()
    } else if let Some(v) = value.try_downcast_ref::<u64>() {
        v.to_string()
    } else if let Some(v) = value.try_downcast_ref::<bool>() {
        (if *v { "True" } else { "False" }).to_string()
    } else if let Some(v) = value.try_downcast_ref::<String>() {
        format!("{v:?}")
    } else {
        panic!("a params field is an f32, u32, u64, bool or String")
    }
}

/// Formats an `f32` as a Python float literal.
///
/// Rust's `Display` for `f32` already gives the shortest text that round-trips
/// through an `f32`, so a slider left at 2.8 prints `2.8` rather than the
/// `2.799999952316284` its `f64` widening would. A whole number needs the
/// trailing `.0` put back, so the literal still reads as a float.
fn python_float(value: f32) -> String {
    let text = format!("{value}");
    if text.contains(['.', 'e', 'E']) {
        text
    } else {
        format!("{text}.0")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixture::BoxesParams;
    use crate::testing::nudged;

    #[test]
    fn paragraphs_join_wrapped_lines_and_split_on_blank_ones() {
        let docs = Some(" First line,\n wrapped.\n\n Second.\n");
        assert_eq!(paragraphs(docs), ["First line, wrapped.", "Second."]);
        assert_eq!(paragraphs(None), Vec::<String>::new());
    }

    /// Whole numbers keep a decimal point, so a float field reads as a float.
    #[test]
    fn python_literals_read_as_their_python_type() {
        assert_eq!(python_literal(&2.0f32), "2.0");
        assert_eq!(python_literal(&2.8f32), "2.8");
        assert_eq!(python_literal(&0.035f32), "0.035");
        assert_eq!(python_literal(&8u32), "8");
        assert_eq!(python_literal(&true), "True");
        assert_eq!(python_literal(&"sown".to_string()), "\"sown\"");
    }

    /// Every field moves, and moves back through the same names.
    #[test]
    fn nudged_moves_every_field_off_its_default() {
        let (moved, default) = (nudged::<BoxesParams>(), BoxesParams::default());
        for fragment in fragments::<BoxesParams>() {
            for field in fields(fragment) {
                let (a, b) = (
                    get(&moved, fragment.name(), field.name()).unwrap(),
                    get(&default, fragment.name(), field.name()).unwrap(),
                );
                assert_ne!(
                    a.reflect_partial_eq(b),
                    Some(true),
                    "{}.{}",
                    fragment.name(),
                    field.name()
                );
            }
        }
    }
}
