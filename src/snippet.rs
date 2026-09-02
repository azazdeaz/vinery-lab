//! Emits the current parameters as a `vinerylab.isaaclab` config snippet.
//!
//! What the viewer's copy button puts on the clipboard: the Python the user
//! pastes into an Isaac Lab environment to get the scene they just tuned.
//!
//! Only fields that differ from their default are emitted, and a fragment
//! nobody touched is left out entirely. A dump of all forty-odd knobs would
//! be self-describing but unreadable; this way the snippet says what was
//! *decided*, which is also what survives review in a config file.
//!
//! The field lists below are the fifth copy of a list that already lives in
//! the params struct, `python.rs`'s `#[pyo3(signature)]`, `_core.pyi` and the
//! Python cfg classes. Nothing here can stop that, but [`Fields::offered`]
//! records every field this module knows about, so the test at the bottom can
//! compare it against the derived `Debug` output — which names every field of
//! the real struct — and fail the moment the two disagree.

use crate::elements::VineyardParams;
use crate::elements::SceneParams;
use crate::elements::leaf::LeafParams;
use crate::elements::pole::PoleParams;
use crate::elements::shoot::ShootParams;
use crate::elements::terrain::TerrainParams;
use crate::elements::util::parcel::ParcelParams;
use crate::elements::util::planting::PlantingParams;
use crate::elements::vine::VineParams;

/// One fragment's worth of `name=value` arguments.
#[derive(Default)]
struct Fields {
    /// Every field offered, changed or not — the drift test's handle on what
    /// this module covers.
    offered: Vec<&'static str>,
    /// Just the ones that differ from the default, formatted as Python.
    changed: Vec<String>,
}

impl Fields {
    fn float(&mut self, name: &'static str, value: f32, default: f32) {
        self.offered.push(name);
        if value != default {
            self.changed.push(format!("{name}={}", python_float(value)));
        }
    }

    fn int(&mut self, name: &'static str, value: u64, default: u64) {
        self.offered.push(name);
        if value != default {
            self.changed.push(format!("{name}={value}"));
        }
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

fn scene(p: &SceneParams, out: &mut Fields) {
    let d = SceneParams::default();
    out.int("seed", p.seed, d.seed);
}

fn terrain(p: &TerrainParams, out: &mut Fields) {
    let d = TerrainParams::default();
    out.float("width", p.width, d.width);
    out.float("height", p.height, d.height);
    out.float("max_elevation", p.max_elevation, d.max_elevation);
    out.int("detail", p.detail as u64, d.detail as u64);
}

fn parcel(p: &ParcelParams, out: &mut Fields) {
    let d = ParcelParams::default();
    out.float("orientation", p.orientation, d.orientation);
    out.float("headland", p.headland, d.headland);
    out.float("row_spacing", p.row_spacing, d.row_spacing);
    out.float("vine_spacing", p.vine_spacing, d.vine_spacing);
    out.float("post_spacing", p.post_spacing, d.post_spacing);
    out.float("min_row_length", p.min_row_length, d.min_row_length);
    out.float("trellis_height", p.trellis_height, d.trellis_height);
}

fn planting(p: &PlantingParams, out: &mut Fields) {
    let d = PlantingParams::default();
    out.float("miss_rate", p.miss_rate, d.miss_rate);
    out.float("young_rate", p.young_rate, d.young_rate);
    out.float("young_scale", p.young_scale, d.young_scale);
}

fn pole(p: &PoleParams, out: &mut Fields) {
    let d = PoleParams::default();
    out.float("radius", p.radius, d.radius);
    out.int("sides", p.sides as u64, d.sides as u64);
}

fn vine(p: &VineParams, out: &mut Fields) {
    let d = VineParams::default();
    out.int("variations", p.variations as u64, d.variations as u64);
    out.float("trunk_height", p.trunk_height, d.trunk_height);
    out.float("trunk_radius", p.trunk_radius, d.trunk_radius);
    out.float("trunk_wobble", p.trunk_wobble, d.trunk_wobble);
    out.int("arms", p.arms as u64, d.arms as u64);
    out.float("cordon_gap", p.cordon_gap, d.cordon_gap);
    out.float("cordon_radius", p.cordon_radius, d.cordon_radius);
    out.float("spur_spacing", p.spur_spacing, d.spur_spacing);
    out.float("spur_length", p.spur_length, d.spur_length);
    out.float("shoots_per_spur", p.shoots_per_spur, d.shoots_per_spur);
    out.float("roughness", p.roughness, d.roughness);
    out.int("sides", p.sides as u64, d.sides as u64);
    out.int("detail", p.detail as u64, d.detail as u64);
}

fn shoot(p: &ShootParams, out: &mut Fields) {
    let d = ShootParams::default();
    out.int("variations", p.variations as u64, d.variations as u64);
    out.float("length", p.length, d.length);
    out.float("radius", p.radius, d.radius);
    out.float("lean", p.lean, d.lean);
    out.int("sides", p.sides as u64, d.sides as u64);
    out.int("detail", p.detail as u64, d.detail as u64);
    out.float("internode", p.internode, d.internode);
    out.float("leaf_droop", p.leaf_droop, d.leaf_droop);
}

fn leaf(p: &LeafParams, out: &mut Fields) {
    let d = LeafParams::default();
    out.int("variations", p.variations as u64, d.variations as u64);
    out.int("detail", p.detail as u64, d.detail as u64);
}

/// The eight fragments, as the attribute name and cfg class the snippet uses.
///
/// Same order and same names as `FRAGMENTS` in `vineyard_cfg.py`, which is
/// what makes the emitted keyword arguments land on the right fields.
const FRAGMENTS: [(&str, &str, fn(&VineyardParams, &mut Fields)); 8] = [
    ("scene", "SceneCfg", |p, out| scene(&p.scene, out)),
    ("terrain", "TerrainCfg", |p, out| terrain(&p.terrain, out)),
    ("parcel", "ParcelCfg", |p, out| parcel(&p.parcel, out)),
    ("planting", "PlantingCfg", |p, out| planting(&p.planting, out)),
    ("pole", "PoleCfg", |p, out| pole(&p.pole, out)),
    ("vine", "VineCfg", |p, out| vine(&p.vine, out)),
    ("shoot", "ShootCfg", |p, out| shoot(&p.shoot, out)),
    ("leaf", "LeafCfg", |p, out| leaf(&p.leaf, out)),
];

/// The current params as a paste-ready `VineyardCfg` construction.
pub fn vineyard_cfg(params: &VineyardParams) -> String {
    let touched: Vec<(&str, &str, Vec<String>)> = FRAGMENTS
        .iter()
        .filter_map(|(attr, class, collect)| {
            let mut fields = Fields::default();
            collect(params, &mut fields);
            (!fields.changed.is_empty()).then_some((*attr, *class, fields.changed))
        })
        .collect();

    let mut imports = vec!["VineyardCfg".to_string()];
    imports.extend(touched.iter().map(|(_, class, _)| class.to_string()));

    let mut out = String::new();
    out.push_str(&format!(
        "from vinerylab.isaaclab import {}\n\n",
        imports.join(", ")
    ));

    if touched.is_empty() {
        out.push_str("VINEYARD_CFG = VineyardCfg()\n");
    } else {
        out.push_str("VINEYARD_CFG = VineyardCfg(\n");
        for (attr, class, fields) in &touched {
            out.push_str(&format!("    {attr}={class}({}),\n", fields.join(", ")));
        }
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

    /// The field names a derived `Debug` prints: `"X { a: 1, b: 2 }"` -> `[a, b]`.
    ///
    /// Every params fragment is flat scalars, so there are no nested braces to
    /// confuse the split.
    fn debug_field_names(debug: &str) -> Vec<String> {
        let inner = debug
            .split_once('{')
            .and_then(|(_, rest)| rest.rsplit_once('}'))
            .map(|(inner, _)| inner)
            .unwrap_or_default();
        inner
            .split(',')
            .filter_map(|part| part.split_once(':'))
            .map(|(name, _)| name.trim().to_string())
            .collect()
    }

    /// Every field of every fragment reaches the snippet.
    ///
    /// This is the guard on a list that is copied five times over. `Debug` is
    /// derived, so its field names are the struct's actual ones; a field added
    /// to a params struct and forgotten here fails immediately, rather than
    /// silently dropping out of every snippet the viewer emits.
    #[test]
    fn every_params_field_is_emitted() {
        let params = VineyardParams::default();
        let debugs = [
            format!("{:?}", params.scene),
            format!("{:?}", params.terrain),
            format!("{:?}", params.parcel),
            format!("{:?}", params.planting),
            format!("{:?}", params.pole),
            format!("{:?}", params.vine),
            format!("{:?}", params.shoot),
            format!("{:?}", params.leaf),
        ];

        for ((attr, _, collect), debug) in FRAGMENTS.iter().zip(&debugs) {
            let mut fields = Fields::default();
            collect(&params, &mut fields);
            assert_eq!(
                fields.offered,
                debug_field_names(debug),
                "`{attr}` fragment: the snippet's fields have drifted from the struct's"
            );
        }
    }

    /// A snippet for untouched params still constructs something valid.
    #[test]
    fn defaults_emit_a_bare_cfg() {
        let snippet = vineyard_cfg(&VineyardParams::default());
        assert!(snippet.contains("VINEYARD_CFG = VineyardCfg()"), "{snippet}");
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

    /// Whole numbers keep a decimal point, so a float field reads as a float.
    #[test]
    fn floats_stay_floats() {
        assert_eq!(python_float(2.0), "2.0");
        assert_eq!(python_float(2.8), "2.8");
        assert_eq!(python_float(0.035), "0.035");
    }
}
