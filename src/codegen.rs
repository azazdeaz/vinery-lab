//! The generated Python and docs: what the params structs declare, written
//! where Python tooling and readers look for it.
//!
//! Regions marked `# >>> generated: <name>` … `# <<< generated: <name>` in
//! four Python files are rendered here from the params walk in
//! [`crate::params`], and `docs/parameters.md` is rendered whole:
//!
//! - `python/vinerylab/_core.pyi` — the stub the type checker and the IDE read.
//! - `python/vinerylab/__init__.py` — the package's re-exports.
//! - `python/vinerylab/isaaclab/vineyard_cfg.py` — the `@configclass` mirrors.
//! - `python/vinerylab/isaaclab/__init__.py` — their re-exports.
//!
//! `cargo test` fails while any of them is stale, and
//! `cargo test regen_params -- --ignored` rewrites them. Everything outside
//! the markers is hand-written and left alone.

use crate::elements::VineyardParams;
use crate::params::{self, Widget};

/// One generated piece: a file and, within it, the marked region it fills —
/// or the whole file, when it has no region.
pub struct Target {
    pub path: &'static str,
    pub region: Option<&'static str>,
    pub render: fn() -> String,
}

pub const TARGETS: &[Target] = &[
    Target {
        path: "python/vinerylab/_core.pyi",
        region: Some("fragments"),
        render: stub_fragments,
    },
    Target {
        path: "python/vinerylab/_core.pyi",
        region: Some("aggregate"),
        render: stub_aggregate,
    },
    Target {
        path: "python/vinerylab/__init__.py",
        region: Some("exports"),
        render: package_exports,
    },
    Target {
        path: "python/vinerylab/isaaclab/vineyard_cfg.py",
        region: Some("fragments"),
        render: cfg_fragments,
    },
    Target {
        path: "python/vinerylab/isaaclab/vineyard_cfg.py",
        region: Some("aggregate"),
        render: cfg_aggregate,
    },
    Target {
        path: "python/vinerylab/isaaclab/__init__.py",
        region: Some("imports"),
        render: isaaclab_imports,
    },
    Target {
        path: "python/vinerylab/isaaclab/__init__.py",
        region: Some("all"),
        render: isaaclab_all,
    },
    Target {
        path: "docs/parameters.md",
        region: None,
        render: docs,
    },
];

/// `current` with the target's text in place: its region replaced, or the
/// whole file when it has none.
pub fn render(current: &str, target: &Target) -> Result<String, String> {
    let content = (target.render)();
    let Some(region) = target.region else {
        return Ok(content);
    };
    splice(current, region, &content).ok_or_else(|| {
        format!(
            "{}: no `# >>> generated: {region}` … `# <<< generated: {region}` markers",
            target.path
        )
    })
}

/// `text` with everything between the region's marker lines replaced by
/// `content`. The marker lines themselves stay.
fn splice(text: &str, region: &str, content: &str) -> Option<String> {
    let start = text.find(&format!("# >>> generated: {region}"))?;
    let body = start + text[start..].find('\n')? + 1;
    let end = text[body..].find(&format!("# <<< generated: {region}"))? + body;
    let end_line = text[..end].rfind('\n').map_or(0, |at| at + 1);
    Some(format!("{}{}{}", &text[..body], content, &text[end_line..]))
}

// ─── Python ─────────────────────────────────────────────────────────

/// Columns the generated prose wraps at.
const WIDTH: usize = 88;

/// Greedy word wrap.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![String::new()];
    for word in text.split_whitespace() {
        let line = lines.last_mut().unwrap();
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(word.to_string());
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    lines
}

/// A docstring at `indent`: on one line when a single paragraph fits, else
/// wrapped, paragraphs a blank line apart, the closing quotes on their own
/// line.
fn docstring(paragraphs: &[String], indent: usize) -> String {
    let pad = " ".repeat(indent);
    let width = WIDTH - indent - 3;
    if let [only] = paragraphs
        && only.len() <= width
    {
        return format!("{pad}\"\"\"{only}\"\"\"\n");
    }
    let mut lines: Vec<String> = Vec::new();
    for (i, paragraph) in paragraphs.iter().enumerate() {
        if i > 0 {
            lines.push(String::new());
        }
        lines.extend(wrap(paragraph, width));
    }
    let mut out = format!("{pad}\"\"\"");
    for (i, line) in lines.iter().enumerate() {
        if i > 0 && !line.is_empty() {
            out.push_str(&pad);
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&format!("{pad}\"\"\"\n"));
    out
}

/// The `Literal` alias a `@Choices` field gets in the stub: `CoverKind`.
fn alias(fragment: &bevy::reflect::NamedField, field: &bevy::reflect::NamedField) -> String {
    let mut name = field.name().to_string();
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    format!("{}{name}", params::stem(fragment))
}

/// A field's type in the stub, where a choice is its `Literal` alias.
fn stub_type(fragment: &bevy::reflect::NamedField, field: &bevy::reflect::NamedField) -> String {
    match params::widget(field) {
        Widget::Dropdown(_) => alias(fragment, field),
        _ => params::python_type(field).to_string(),
    }
}

/// The class names, sorted the way the import sorter wants them.
fn sorted_classes(suffix: &str) -> Vec<String> {
    let mut names: Vec<String> = params::fragments()
        .map(|fragment| format!("{}{suffix}", params::stem(fragment)))
        .collect();
    names.push(format!("Vineyard{suffix}"));
    names.sort();
    names
}

/// `_core.pyi`: a `Literal` per choice field, then one class per fragment
/// with every field documented and a keyword-only constructor carrying the
/// defaults.
fn stub_fragments() -> String {
    let default = VineyardParams::default();
    let mut out = String::new();
    for fragment in params::fragments() {
        for field in params::fields(fragment) {
            if let Widget::Dropdown(names) = params::widget(field) {
                let quoted: Vec<String> = names.iter().map(|name| format!("{name:?}")).collect();
                out += &format!(
                    "{} = Literal[{}]\n",
                    alias(fragment, field),
                    quoted.join(", ")
                );
                out += &docstring(
                    &[format!(
                        "What `{}Params.{}` may be set to.",
                        params::stem(fragment),
                        field.name()
                    )],
                    0,
                );
                out.push('\n');
            }
        }
    }
    for fragment in params::fragments() {
        let stem = params::stem(fragment);
        out += &format!("class {stem}Params:\n");
        out += &docstring(
            &params::paragraphs(params::fragment_info(fragment).docs()),
            4,
        );
        out.push('\n');
        for field in params::fields(fragment) {
            out += &format!("    {}: {}\n", field.name(), stub_type(fragment, field));
            out += &docstring(&[params::summary(field)], 4);
        }
        out += "\n    def __init__(\n        self,\n        *,\n";
        for field in params::fields(fragment) {
            let value = params::get(&default, fragment.name(), field.name()).unwrap();
            out += &format!(
                "        {}: {} = {},\n",
                field.name(),
                stub_type(fragment, field),
                params::python_literal(value)
            );
        }
        out += "    ) -> None: ...\n    def __repr__(self) -> str: ...\n\n";
    }
    out
}

/// `_core.pyi`, inside `class VineyardParams`: one attribute per fragment and
/// the constructor that takes them.
fn stub_aggregate() -> String {
    let mut out = String::new();
    for fragment in params::fragments() {
        out += &format!(
            "    {}: {}Params\n",
            fragment.name(),
            params::stem(fragment)
        );
    }
    out += "\n    def __init__(\n        self,\n";
    for fragment in params::fragments() {
        out += &format!(
            "        {}: {}Params | None = None,\n",
            fragment.name(),
            params::stem(fragment)
        );
    }
    out += "    ) -> None: ...\n";
    out
}

/// `vinerylab/__init__.py`: the re-export of every class in `_core`.
fn package_exports() -> String {
    let names = sorted_classes("Params");
    let mut out = String::from("from ._core import (\n");
    for name in &names {
        out += &format!("    {name},\n");
    }
    out += "    __version__,\n)\n\n__all__ = [\n";
    for name in &names {
        out += &format!("    {name:?},\n");
    }
    out += "    \"__version__\",\n]\n";
    out
}

/// `vineyard_cfg.py`: one `@configclass` per fragment, and the `FRAGMENTS`
/// table the spawner walks.
fn cfg_fragments() -> String {
    let default = VineyardParams::default();
    let mut out = String::new();
    for fragment in params::fragments() {
        let stem = params::stem(fragment);
        out += &format!("@configclass\nclass {stem}Cfg:\n");
        out += &docstring(
            &params::paragraphs(params::fragment_info(fragment).docs()),
            4,
        );
        out.push('\n');
        for field in params::fields(fragment) {
            let value = params::get(&default, fragment.name(), field.name()).unwrap();
            out += &format!(
                "    {}: {} = {}\n",
                field.name(),
                params::python_type(field),
                params::python_literal(value)
            );
            out += &docstring(&[params::summary(field)], 4);
        }
        out += "\n\n";
    }
    out += "FRAGMENTS: tuple[tuple[str, type], ...] = (\n";
    for fragment in params::fragments() {
        out += &format!(
            "    ({:?}, {}Cfg),\n",
            fragment.name(),
            params::stem(fragment)
        );
    }
    out += ")\n";
    out += &docstring(
        &[
            "The geometry fragments, in the order `VineyardParams` takes them.".to_string(),
            "The single list both the pyclass conversion and the cache key walk. Everything on \
             `VineyardCfg` that is *not* in this list is applied to the spawned prim rather than \
             baked into the USD, and so must not take part in the cache key."
                .to_string(),
        ],
        0,
    );
    out
}

/// `vineyard_cfg.py`, inside `class VineyardCfg`: one fragment field each.
fn cfg_aggregate() -> String {
    params::fragments()
        .map(|fragment| {
            let stem = params::stem(fragment);
            format!("    {}: {stem}Cfg = {stem}Cfg()\n", fragment.name())
        })
        .collect()
}

/// `isaaclab/__init__.py`: the import of every cfg class. The blank line
/// after it is where the import sorter wants the block to end.
fn isaaclab_imports() -> String {
    let mut out = String::from("from .vineyard_cfg import (\n");
    for name in sorted_classes("Cfg") {
        out += &format!("    {name},\n");
    }
    out += ")\n\n";
    out
}

/// `isaaclab/__init__.py`, inside `__all__`: the cfg classes' entries.
fn isaaclab_all() -> String {
    sorted_classes("Cfg")
        .iter()
        .map(|name| format!("    {name:?},\n"))
        .collect()
}

// ─── Docs ───────────────────────────────────────────────────────────

/// `docs/parameters.md`: every fragment's docs and a table of its fields.
fn docs() -> String {
    let default = VineyardParams::default();
    let mut out = String::from(
        "<!-- Generated from the Rust params structs by `cargo test regen_params -- --ignored`.\n\
         \x20    Edit the structs, not this file: docs/editing-parameters.md says how. -->\n\n\
         # Parameters\n\n\
         Every parameter is reachable three ways under one name: as a control in the viewer\n\
         (`cargo run --release`), as an attribute of a `vinerylab.VineyardParams` fragment,\n\
         and as a field of the matching Isaac Lab `VineyardCfg` fragment.\n\
         `params.pole.radius = 0.05` and `VineyardCfg(pole=PoleCfg(radius=0.05))` set the\n\
         same thing. The slider range is what the viewer offers; Python takes any value the\n\
         generator can build. Lengths are in meters.\n\n",
    );
    for fragment in params::fragments() {
        let stem = params::stem(fragment);
        out += &format!(
            "## {}\n\n`{stem}Params` in Python, `{stem}Cfg` in Isaac Lab.\n\n",
            params::label(fragment)
        );
        for paragraph in params::paragraphs(params::fragment_info(fragment).docs()) {
            out += &paragraph;
            out += "\n\n";
        }
        out +=
            "| Parameter | Type | Default | Slider range | Description |\n|---|---|---|---|---|\n";
        for field in params::fields(fragment) {
            let value = params::get(&default, fragment.name(), field.name()).unwrap();
            let range = match params::widget(field) {
                Widget::Slider(slider) => format!("{} to {}", slider.min, slider.max),
                Widget::Dropdown(names) => names
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                Widget::Checkbox => String::new(),
            };
            out += &format!(
                "| `{}` | {} | `{}` | {} | {} |\n",
                field.name(),
                params::python_type(field),
                params::python_literal(value),
                range,
                params::summary(field)
            );
        }
        out.push('\n');
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// Every generated region reads as the structs say now.
    #[test]
    fn generated_python_and_docs_are_fresh() {
        let mut stale = BTreeSet::new();
        for target in TARGETS {
            let current = std::fs::read_to_string(root().join(target.path)).unwrap_or_default();
            let fresh = render(&current, target).unwrap_or_else(|err| panic!("{err}"));
            if fresh != current {
                stale.insert(target.path);
            }
        }
        assert!(
            stale.is_empty(),
            "stale: {stale:?} — run `cargo test regen_params -- --ignored`"
        );
    }

    /// Rewrites the generated files from the structs.
    ///
    /// A dev tool rather than a test, and `#[ignore]`d for the same reason
    /// `generate::dump_scene` is: it writes files and asserts nothing.
    #[test]
    #[ignore]
    fn regen_params() {
        for target in TARGETS {
            let path = root().join(target.path);
            let current = std::fs::read_to_string(&path).unwrap_or_default();
            let fresh = render(&current, target).unwrap_or_else(|err| panic!("{err}"));
            if fresh != current {
                std::fs::write(&path, fresh).unwrap();
            }
        }
    }

    #[test]
    fn splice_replaces_the_region_and_nothing_else() {
        let text = "a\n    # >>> generated: x\n    old\n    # <<< generated: x\nb\n";
        assert_eq!(
            splice(text, "x", "    new\n").unwrap(),
            "a\n    # >>> generated: x\n    new\n    # <<< generated: x\nb\n"
        );
        assert_eq!(
            splice(text, "y", ""),
            None,
            "a missing region is an error, not a no-op"
        );
    }

    #[test]
    fn a_docstring_closes_on_its_line_only_when_it_fits() {
        assert_eq!(docstring(&["Short.".into()], 4), "    \"\"\"Short.\"\"\"\n");
        let long = docstring(&["a ".repeat(60).trim().to_string(), "b".into()], 4);
        assert!(long.starts_with("    \"\"\"a a a"));
        assert!(long.contains("\n\n    b\n    \"\"\"\n"), "{long}");
        assert!(long.lines().all(|line| line.len() <= WIDTH), "{long}");
    }
}
