//! Emits the current parameters as a `vinerylab.isaaclab` config snippet.
//!
//! What the viewer's copy button puts on the clipboard: the Python the user
//! pastes into an Isaac Lab environment to get the scene they just tuned.
//!
//! Only fields that differ from their default are emitted, and a fragment
//! nobody touched is left out entirely. A dump of all sixty-odd knobs would
//! be self-describing but unreadable; this way the snippet says what was
//! *decided*, which is also what survives review in a config file.
//!
//! The fields come from the params walk in [`crate::params`], so a field
//! added to a struct is in the snippet. A fragment's cfg class is its stem
//! with `Cfg` on the end — `PoleCfg` for `PoleParams` — which is the name
//! `vineyard_cfg.py` is generated to define.

use crate::elements::VineyardParams;
use crate::params;

/// The current params as a paste-ready `VineyardCfg` construction.
pub fn vineyard_cfg(current: &VineyardParams) -> String {
    let default = VineyardParams::default();
    let mut imports = vec!["VineyardCfg".to_string()];
    let mut lines = Vec::new();
    for fragment in params::fragments() {
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

    let mut out = format!("from vinerylab.isaaclab import {}\n\n", imports.join(", "));
    if lines.is_empty() {
        out.push_str("VINEYARD_CFG = VineyardCfg()\n");
    } else {
        out.push_str("VINEYARD_CFG = VineyardCfg(\n");
        out.extend(lines);
        out.push_str(")\n");
    }
    out.push_str(
        "\n# Spawn it directly:\n\
         #     VINEYARD_CFG.func(\"/World/Vineyard\", VINEYARD_CFG)\n\
         # or put it in a scene config:\n\
         #     vineyard = AssetBaseCfg(\n\
         #         prim_path=\"/World/Vineyard\", spawn=VINEYARD_CFG)\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A snippet for untouched params still constructs something valid.
    #[test]
    fn defaults_emit_a_bare_cfg() {
        let snippet = vineyard_cfg(&VineyardParams::default());
        assert!(
            snippet.contains("VINEYARD_CFG = VineyardCfg()"),
            "{snippet}"
        );
        assert!(
            !snippet.contains("TerrainCfg"),
            "an untouched fragment is not imported: {snippet}"
        );
    }

    /// Only what moved is emitted, and only the fragments it moved in.
    #[test]
    fn only_changed_fields_are_emitted() {
        let mut params = VineyardParams::default();
        params.parcel.row_spacing = 2.8;
        params.vine.arms = 1;

        let snippet = vineyard_cfg(&params);
        assert!(
            snippet.contains("parcel=ParcelCfg(row_spacing=2.8)"),
            "shortest round-tripping literal, not the f64 widening: {snippet}"
        );
        assert!(snippet.contains("vine=VineCfg(arms=1)"), "{snippet}");
        assert!(
            !snippet.contains("vine_spacing"),
            "an untouched field in a touched fragment stays out: {snippet}"
        );
        assert!(
            snippet.contains("import VineyardCfg, ParcelCfg, VineCfg"),
            "imports cover exactly what is used: {snippet}"
        );
    }

    /// Every field reaches the snippet once it has moved, under its own name
    /// and its fragment's cfg class.
    #[test]
    fn every_moved_field_is_emitted() {
        let snippet = vineyard_cfg(&params::nudged());
        for fragment in params::fragments() {
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
        let mut params = VineyardParams::default();
        params.cover.kind = "cereal".into();
        params.cover.alternate = true;
        let snippet = vineyard_cfg(&params);
        assert!(
            snippet.contains("cover=CoverCfg(kind=\"cereal\", alternate=True)"),
            "{snippet}"
        );
    }
}
