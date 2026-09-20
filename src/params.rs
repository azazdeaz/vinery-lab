//! What a params field declares about itself, and the walk that reads it.
//!
//! Every `*Params` struct derives `Reflect`, so its field names, types, doc
//! comments and `#[reflect(@...)]` attributes can be read at run time. The
//! viewer panel, the config snippet, the Python constructors and the generated
//! Python files are written against that walk rather than against the fields
//! themselves, which is what lets a parameter be declared once, in its struct.
//! `docs/editing-parameters.md` is the authoring guide; the test at the bottom
//! is what holds a struct to it.
//!
//! A field's doc comment is read in two parts. The **first paragraph** is the
//! user-facing description — the tooltip, the Python docstring and the row in
//! `docs/parameters.md` — so it has to stand on its own and may not use
//! rustdoc link syntax. Anything after it is for a reader of the Rust. A
//! struct's doc comment is user-facing in full: it becomes the class docstring.

use std::any::TypeId;

use bevy::prelude::*;
use bevy::reflect::structs::{Struct, StructInfo};
use bevy::reflect::{NamedField, PartialReflect, ReflectMut, ReflectRef, TypeInfo, Typed};

use crate::elements::VineyardParams;

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

// ─── The walk ───────────────────────────────────────────────────────

/// The fragments of [`VineyardParams`], in declaration order.
pub fn fragments() -> impl Iterator<Item = &'static NamedField> {
    let TypeInfo::Struct(info) = VineyardParams::type_info() else {
        unreachable!("VineyardParams is a struct");
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
    params: &'a VineyardParams,
    fragment: &str,
    field: &str,
) -> Option<&'a dyn PartialReflect> {
    match params.field(fragment)?.reflect_ref() {
        ReflectRef::Struct(fragment) => fragment.field(field),
        _ => None,
    }
}

pub fn get_mut<'a>(
    params: &'a mut VineyardParams,
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

/// A params set with every field moved off its default, for tests that have
/// to see each one go somewhere: numbers up by one step, flags flipped, names
/// on the next choice.
#[cfg(test)]
pub fn nudged() -> VineyardParams {
    let mut params = VineyardParams::default();
    for fragment in fragments() {
        for field in fields(fragment) {
            let widget = widget(field);
            let value = get_mut(&mut params, fragment.name(), field.name()).unwrap();
            match widget {
                Widget::Slider(slider) => {
                    let now = number(value).unwrap();
                    set_number(value, now + slider.step);
                }
                Widget::Dropdown(names) => {
                    let name = value.try_downcast_mut::<String>().unwrap();
                    let at = names.iter().position(|n| n == name).unwrap();
                    *name = names[(at + 1) % names.len()].to_string();
                }
                Widget::Checkbox => {
                    let flag = value.try_downcast_mut::<bool>().unwrap();
                    *flag = !*flag;
                }
            }
        }
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What every field has to declare for the panel, the snippet and the
    /// generated docs to be built from it — see `docs/editing-parameters.md`.
    #[test]
    fn every_field_declares_what_the_panel_and_the_docs_need() {
        let default = VineyardParams::default();
        for fragment in fragments() {
            let info = fragment_info(fragment);
            let at = format!("{}::{}", stem(fragment), "");
            assert!(
                !paragraphs(info.docs()).is_empty(),
                "{at}: the struct has no doc comment, and it is the class docstring"
            );
            assert!(
                !info.docs().unwrap_or_default().contains('['),
                "{at}: the struct docs use rustdoc link syntax, which Python would show verbatim"
            );
            for field in fields(fragment) {
                let at = format!("{}.{}", fragment.name(), field.name());
                let summary = summary(field);
                assert!(!summary.is_empty(), "{at}: no doc comment");
                assert!(
                    !summary.contains('['),
                    "{at}: the first paragraph uses rustdoc link syntax; move it to a later one"
                );
                let value = get(&default, fragment.name(), field.name()).unwrap();
                match widget(field) {
                    Widget::Slider(Slider { min, max, step }) => {
                        assert!(
                            python_type(field) != "bool" && python_type(field) != "str",
                            "{at}: a @Slider on a non-numeric field"
                        );
                        let now = number(value).unwrap();
                        assert!(
                            min < max && step > 0.0,
                            "{at}: slider {min}..={max} by {step}"
                        );
                        assert!(
                            (min..=max).contains(&now),
                            "{at}: default {now} outside {min}..={max}"
                        );
                    }
                    Widget::Dropdown(names) => {
                        let name = value.try_downcast_ref::<String>().expect("a String field");
                        assert!(
                            names.contains(&name.as_str()),
                            "{at}: default {name:?} not in {names:?}"
                        );
                    }
                    Widget::Checkbox => {}
                }
            }
        }
    }

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
        let (moved, default) = (nudged(), VineyardParams::default());
        for fragment in fragments() {
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
