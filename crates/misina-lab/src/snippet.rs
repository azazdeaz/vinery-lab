//! Emits the current parameters as a `{PACKAGE}.isaaclab` config snippet.
//!
//! What the viewer's copy button puts on the clipboard: the Python the user
//! pastes into an Isaac Lab environment to get the scene they just tuned.
//!
//! Only fields that differ from their default are emitted, and a fragment
//! nobody touched is left out entirely. A dump of all sixty-odd knobs would
//! be self-describing but unreadable; this way the snippet says what was
//! *decided*, which is also what survives review in a config file.
//!
//! The fields come from the params walk in [`params`](crate::params), so a
//! field added to a struct is in the snippet. A fragment's cfg class is its
//! stem with `Cfg` on the end — `PoleCfg` for `PoleParams` — which is the name
//! the generated `{name}_cfg.py` defines; the aggregate's is `{NAME}Cfg`.

use crate::Generator;
use crate::params;

/// The current params as a paste-ready `{NAME}Cfg` construction.
pub fn cfg<G: Generator>(current: &G::Params) -> String {
    let default = G::Params::default();
    let class = format!("{}Cfg", G::NAME);
    let var = format!("{}_CFG", G::NAME.to_uppercase());
    let mut imports = vec![class.clone()];
    let mut lines = Vec::new();
    for fragment in params::fragments::<G::Params>() {
        let changed: Vec<String> = params::fields(fragment)
            .filter_map(|field| {
                let now = params::get(current, fragment.name(), field.name())?;
                let was = params::get(&default, fragment.name(), field.name())?;
                (now.reflect_partial_eq(was) != Some(true))
                    .then(|| format!("{}={}", field.name(), params::python_literal(now)))
            })
            .collect();
        if changed.is_empty() {
            continue;
        }
        let class = format!("{}Cfg", params::stem(fragment));
        lines.push(format!(
            "    {}={class}({}),\n",
            fragment.name(),
            changed.join(", ")
        ));
        imports.push(class);
    }

    let mut out = format!(
        "from {}.isaaclab import {}\n\n",
        G::PACKAGE,
        imports.join(", ")
    );
    if lines.is_empty() {
        out.push_str(&format!("{var} = {class}()\n"));
    } else {
        out.push_str(&format!("{var} = {class}(\n"));
        out.extend(lines);
        out.push_str(")\n");
    }
    out.push_str(&format!(
        "\n# Spawn it directly:\n\
         #     {var}.func(\"/World/{name}\", {var})\n\
         # or put it in a scene config:\n\
         #     {lower} = AssetBaseCfg(\n\
         #         prim_path=\"/World/{name}\", spawn={var})\n",
        name = G::NAME,
        lower = G::NAME.to_lowercase(),
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixture::{Boxes, BoxesParams};
    use crate::testing::nudged;

    /// A snippet for untouched params still constructs something valid.
    #[test]
    fn defaults_emit_a_bare_cfg() {
        let snippet = cfg::<Boxes>(&BoxesParams::default());
        assert!(snippet.contains("BOXES_CFG = BoxesCfg()"), "{snippet}");
        assert!(
            !snippet.contains("BoxCfg"),
            "an untouched fragment is not imported: {snippet}"
        );
        assert!(snippet.contains("prim_path=\"/World/Boxes\""), "{snippet}");
    }

    /// Only what moved is emitted, and only the fragments it moved in.
    #[test]
    fn only_changed_fields_are_emitted() {
        let mut params = BoxesParams::default();
        params.boxes.size = 0.8;
        params.boxes.count = 1;

        let snippet = cfg::<Boxes>(&params);
        assert!(
            snippet.contains("boxes=BoxCfg(count=1, size=0.8)"),
            "shortest round-tripping literal, not the f64 widening: {snippet}"
        );
        assert!(
            !snippet.contains("gap"),
            "an untouched field in a touched fragment stays out: {snippet}"
        );
        assert!(
            snippet.contains("from boxlab.isaaclab import BoxesCfg, BoxCfg"),
            "imports cover exactly what is used: {snippet}"
        );
    }

    /// Every field reaches the snippet once it has moved, under its own name
    /// and its fragment's cfg class.
    #[test]
    fn every_moved_field_is_emitted() {
        let snippet = cfg::<Boxes>(&nudged::<BoxesParams>());
        for fragment in params::fragments::<BoxesParams>() {
            let class = format!("{}Cfg", params::stem(fragment));
            assert!(
                snippet.contains(&format!("{}={class}(", fragment.name())),
                "{snippet}"
            );
            for field in params::fields(fragment) {
                assert!(
                    snippet.contains(&format!("{}=", field.name())),
                    "{}.{} is missing from:\n{snippet}",
                    fragment.name(),
                    field.name()
                );
            }
        }
    }

    /// A name is quoted and a flag is capitalised, the way Python spells them.
    #[test]
    fn strings_and_flags_are_python_literals() {
        let mut params = BoxesParams::default();
        params.boxes.material = "stone".into();
        params.boxes.open = false;
        let snippet = cfg::<Boxes>(&params);
        assert!(
            snippet.contains("boxes=BoxCfg(material=\"stone\", open=False)"),
            "{snippet}"
        );
    }
}
