//! The export document — the whole contract between this crate and the USD
//! builder in `python/vinerylab/usd/build.py`.
//!
//! Rust owns the *scene*: what geometry exists, where it goes, and what
//! references what. Python owns *USD*: prim types, schemas, composition arcs,
//! stage metadata. This module is the line between them. Keep it a plain serde
//! struct — it is the only thing both test suites can assert against, and it
//! has to stay readable when a scene comes out wrong.
//!
//! # Conventions the builder relies on
//!
//! - Coordinates are **Z-up, meters**, already in the convention the export
//!   targets. Nothing downstream rotates anything.
//! - Every mesh is a **triangle list**, so USD's `faceVertexCounts` is
//!   `[3; indices.len() / 3]` and is not transmitted.
//! - Rotations are **quaternions in xyzw order**, authored as
//!   `xformOp:orient`. Do not switch to Euler triples: USD's `rotateXYZ` and
//!   Bevy's `EulerRot` disagree about intrinsic versus extrinsic composition,
//!   and a mismatch gives a scene that is plausibly wrong rather than obviously
//!   wrong.
//! - A [`Node`] with a `reference` draws the [`PartEntry`] of that name and
//!   has **no children** of its own — see [`Node::instanceable`].

use serde::{Deserialize, Serialize};

/// Bumped when the shape of this document changes incompatibly. The builder
/// refuses anything it does not recognise, so a stale cached scene fails
/// loudly instead of composing into something subtly wrong.
pub const FORMAT: u32 = 1;

/// One generated scene, ready to be turned into a USD stage.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SceneDoc {
    pub format: u32,
    /// Always `"Z"` — the export target is robotics simulation (Isaac Lab and
    /// ROS, both REP-103 right-handed Z-up). Transmitted rather than assumed
    /// so the builder authors stage metadata from the document rather than
    /// from a constant of its own.
    pub up_axis: String,
    pub meters_per_unit: f64,
    /// The mesh library, sorted by name. Becomes `/Vineyard/parts`.
    pub parts: Vec<PartEntry>,
    /// The scene root, which becomes the stage's default prim.
    pub root: Node,
}

/// One entry of the mesh library: the geometry of a single representative,
/// referenced by every organ that drew it.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PartEntry {
    /// Unique within the document, and the name a [`Node::reference`] uses.
    /// By convention `<Layer>_<representative index>` — `Leaf_2`, `Vine_11`.
    pub name: String,
    pub points: Vec<[f32; 3]>,
    /// Flat triangle list into `points`.
    pub indices: Vec<u32>,
    /// Per-point normals. Absent when the mesh carries none, in which case the
    /// builder leaves `normals` unauthored and a renderer computes its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normals: Option<Vec<[f32; 3]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uvs: Option<Vec<[f32; 2]>>,
    /// Linear RGB, authored as a constant-interpolation `displayColor`.
    pub display_color: [f32; 3],
    /// A surface with no inside — a leaf blade — which has to be lit and drawn
    /// from behind as well, since a canopy is looked up into as often as down
    /// onto.
    pub double_sided: bool,
}

/// One prim.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Node {
    /// The prim name. The path is this joined onto the ancestors' names, so
    /// these have to stay stable across regeneration — a downstream Isaac Lab
    /// config keyed on a path silently starts pointing at a different plant
    /// otherwise.
    pub name: String,
    /// `"Xform"` or `"Scope"`.
    pub type_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xform: Option<Xform>,
    /// The [`PartEntry`] this prim draws, by name. A prim with a reference is
    /// a leaf of the tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Whether USD may treat this prim as an instance of what it references.
    ///
    /// Set on every referencing prim, which is what makes tens of thousands of
    /// unique leaf paths affordable: the prims stay individually addressable
    /// while the renderer draws one prototype per part. It is safe precisely
    /// because a referencing prim has no children — an instanceable prim's
    /// descendants are not addressable, and these have no descendants to lose.
    #[serde(default, skip_serializing_if = "is_false")]
    pub instanceable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

/// A prim's transform, as the `translate` / `orient` / `scale` op stack.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    pub translate: [f32; 3],
    /// Quaternion in **xyzw** order — Bevy's `Quat` layout. USD's `Gf.Quatf`
    /// takes the real part first, so the builder reorders on the way in.
    pub orient: [f32; 4],
    pub scale: [f32; 3],
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Node {
    /// A structural prim: a name, a type, and whatever hangs below it.
    pub fn group(name: impl Into<String>, type_name: &str) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.to_string(),
            xform: None,
            reference: None,
            instanceable: false,
            children: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> SceneDoc {
        let mut leaf = Node::group("Leaf_00", "Xform");
        leaf.reference = Some("Leaf_2".into());
        leaf.instanceable = true;
        leaf.xform = Some(Xform {
            translate: [0.1, 0.2, 0.3],
            orient: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        });

        let mut root = Node::group("Vineyard", "Xform");
        root.children.push(leaf);

        SceneDoc {
            format: FORMAT,
            up_axis: "Z".into(),
            meters_per_unit: 1.0,
            parts: vec![PartEntry {
                name: "Leaf_2".into(),
                points: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                indices: vec![0, 1, 2],
                normals: None,
                uvs: None,
                display_color: [0.2, 0.5, 0.1],
                double_sided: true,
            }],
            root,
        }
    }

    #[test]
    fn a_document_round_trips_through_json() {
        let json = serde_json::to_string(&doc()).unwrap();
        let back: SceneDoc = serde_json::from_str(&json).unwrap();

        assert_eq!(back.format, FORMAT);
        assert_eq!(back.up_axis, "Z");
        assert_eq!(back.parts[0].name, "Leaf_2");
        assert_eq!(back.root.children[0].reference.as_deref(), Some("Leaf_2"));
        assert!(back.root.children[0].instanceable);
    }

    /// At tens of thousands of prims the defaults are most of the bytes, so
    /// the empty ones have to stay out of the file.
    #[test]
    fn defaulted_fields_are_left_out_of_the_json() {
        let json = serde_json::to_string(&doc()).unwrap();

        assert!(!json.contains("\"normals\""), "got:\n{json}");
        assert!(!json.contains("\"uvs\""), "got:\n{json}");
        // The root has no transform, no reference and is not instanceable;
        // the leaf has all three, so each key appears exactly once.
        assert_eq!(json.matches("\"xform\"").count(), 1, "got:\n{json}");
        assert_eq!(json.matches("\"instanceable\"").count(), 1, "got:\n{json}");
        assert_eq!(json.matches("\"children\"").count(), 1, "got:\n{json}");
    }
}
