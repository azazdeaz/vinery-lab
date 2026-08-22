//! Pole element — one trellis post.
//!
//! The posts carry the wires the vines are trained onto, so they are what
//! turns a field of plants into a *trellised* vineyard: a row of verticals at
//! an even spacing, standing clear of the canopy. At the distance a robot sees
//! one from, that silhouette is the whole of a post.
//!
//! So a pole is a plain grey cylinder — [`cylinder_mesh`], not
//! [`strand`](super::util::strand). A post is straight by construction, and
//! the tube skinner exists to fit a curve through control points and rough its
//! surface into bark, neither of which a machined post has any use for.
//!
//! # How tall
//!
//! From [`ParcelParams::trellis_height`], not from a parameter here: the wires
//! are at the trellis height by definition, and the posts are what hold them
//! there. It is also what the layout gizmo already draws its posts to, so the
//! geometry and the overlay agree without either being told about the other.
//!
//! # Local frame
//!
//! Authored **standing on the origin**, running up +Z. Same convention as
//! every other ground-planted prototype: the placer supplies a position on the
//! terrain and a yaw along the row, and needs to know nothing else.
//!
//! # One subtree
//!
//! The prototype library under [`PROTOTYPE`] — a single `Var_0`, an `Xform`
//! over one `Pole` mesh plus the `Looks` scope holding its material. Where
//! poles stand is [`planting`](super::util::planting)'s business, and so is
//! the variety they show: a post is a manufactured object, identical to its
//! neighbour except in how it was driven.
//!
//! [`ParcelParams::trellis_height`]: super::util::parcel::ParcelParams::trellis_height
//! [`cylinder_mesh`]: super::util::usd::cylinder_mesh

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use usd_bevy::authoring::{define_prim, remove_prim};
use usd_bevy::live::LiveStage;

use super::Grow;
use super::util::parcel::ParcelParams;
use super::util::usd::{MeshData, author_mesh, cylinder_mesh, set_display_color};
use super::util::{color, material};

/// The prototype library this element owns.
pub const PROTOTYPE: &str = "/Vineyard/parts/Pole";

/// How many prototypes this element authors.
///
/// One, where every other element has a `variations` knob. A post is a
/// manufactured object: two of them differ in how they were driven — a
/// centimeter off line, a degree off plumb, a few centimeters deeper — and
/// that is per *placement*, costs no geometry, and is where
/// [`planting`](super::util::planting) puts it. Authoring four identical
/// cylinders to pick between would buy nothing.
pub const VARIATIONS: usize = 1;

/// The mesh under a variation. Named because the placement tests reach for it,
/// and because a `Var_0` that is an `Xform` has to say what its geometry is.
pub const POLE: &str = "Pole";

/// Where a variation keeps its material, inside the subtree that gets
/// referenced onto the ground — a binding whose target sits outside it is
/// silently dropped, which [`material`](super::util::material) has the full
/// account of.
const LOOKS: &str = "Looks";

/// Shortest post we will build. A trellis height of zero is reachable from
/// Python, and a zero-length cylinder is a mesh with two coincident rings.
const MIN_HEIGHT: f32 = 0.1;

// ─── Params ─────────────────────────────────────────────────────────

#[derive(Resource, Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct PoleParams {
    /// Post radius, in meters — so the default is the 8 cm round softwood
    /// post that is the commonest thing in a European vineyard. A steel
    /// profile post is nearer half as thick.
    pub radius: f32,
    /// Vertices around the post. The silhouette, and the only detail knob a
    /// straight tube has.
    pub sides: u32,
}

impl Default for PoleParams {
    fn default() -> Self {
        Self {
            radius: 0.04,
            sides: 8,
        }
    }
}

pub fn plugin(app: &mut App) {
    // `ParcelParams` is deliberately not initialized here — `terrain::plugin`
    // owns it, and `elements::plugin` adds terrain first.
    app.init_resource::<PoleParams>().add_systems(
        PreUpdate,
        author_prototypes.in_set(Grow::Prototypes).run_if(
            resource_changed::<PoleParams>
                // The posts are as tall as the trellis they carry.
                .or_else(resource_changed::<ParcelParams>),
        ),
    );
}

// ─── Shape ──────────────────────────────────────────────────────────

/// One post, in the prototype's local frame: standing on the origin, running
/// up +Z to the trellis height.
fn pole_mesh(params: &PoleParams, parcel: &ParcelParams) -> MeshData {
    cylinder_mesh(
        params.radius.max(0.001),
        parcel.trellis_height.max(MIN_HEIGHT),
        params.sides as usize,
    )
}

// ─── Authoring ──────────────────────────────────────────────────────

/// Authors the single prototype under [`PROTOTYPE`].
pub fn author_prototypes(
    live: NonSend<LiveStage>,
    params: Res<PoleParams>,
    parcel: Res<ParcelParams>,
) -> Result<()> {
    let stage = &live.stage;
    remove_prim(stage, PROTOTYPE)?;
    define_prim(stage, PROTOTYPE, "Scope")?;

    // An `Xform` over the mesh rather than a bare `Mesh`, so the variation has
    // somewhere to keep the material that has to travel with it.
    let variation = format!("{PROTOTYPE}/Var_0");
    define_prim(stage, &variation, "Xform")?;
    define_prim(stage, &format!("{variation}/{LOOKS}"), "Scope")?;
    let pole_material = format!("{variation}/{LOOKS}/{POLE}");
    material::author_preview_material(stage, &pole_material, material::POLE)?;

    let path = format!("{variation}/{POLE}");
    let pole = author_mesh(stage, &path, &pole_mesh(&params, &parcel))?;
    // No per-variation shade: there is one variation, and a row of posts that
    // came off the same pallet genuinely is one colour.
    set_display_color(&pole, color::srgb(color::POLE))?;
    material::bind_material(stage, &path, &pole_material)?;
    Ok(())
}

// ─── UI ─────────────────────────────────────────────────────────────

pub fn ui() -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            label_small("Pole radius"),
            (
                @FeathersSlider { @min: 0.01, @max: 0.1, @value: 0.04 }
                SliderStep(0.005)
                SliderPrecision(3)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<PoleParams>| {
                    params.radius = change.value;
                })
            ),
            label_small("Pole sides"),
            (
                @FeathersSlider { @min: 3.0, @max: 16.0, @value: 8.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<PoleParams>| {
                    params.sides = change.value.round().max(3.0) as u32;
                })
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::VineyardParams;
    use crate::elements::util::place::prototype_count;
    use crate::elements::util::testing::{Authoring, bounds};
    use openusd::schemas::shade::MaterialBindingAPI;
    use openusd::sdf;

    fn parcel() -> ParcelParams {
        ParcelParams::default()
    }

    fn authored(params: PoleParams, parcel: ParcelParams) -> Authoring {
        let mut authoring = Authoring::new("pole.usda", author_prototypes);
        authoring.insert(params).insert(parcel).run();
        authoring
    }

    /// The placement contract: a post stands *on* the origin rather than
    /// straddling it, and reaches the trellis height the layout solved its
    /// wires for. Authored centered, every post in the parcel would be sunk
    /// half its length into the ground.
    #[test]
    fn a_pole_stands_on_the_origin_and_reaches_the_trellis_height() {
        let params = PoleParams::default();
        for trellis_height in [0.9, 1.8, 3.0] {
            let mesh = pole_mesh(
                &params,
                &ParcelParams {
                    trellis_height,
                    ..parcel()
                },
            );
            assert_eq!(bounds(&mesh, 2), (0.0, trellis_height));
            for axis in [0, 1] {
                assert_eq!(
                    bounds(&mesh, axis),
                    (-params.radius, params.radius),
                    "a post is as wide as it is round, on the axis it stands on"
                );
            }
        }
    }

    /// Both knobs reach the geometry — `sides` is the one that would look
    /// plausible while doing nothing, since a coarser post is still a post.
    #[test]
    fn the_shape_knobs_reach_the_mesh() {
        let at = |radius, sides| pole_mesh(&PoleParams { radius, sides }, &parcel());
        let default = PoleParams::default();

        assert!(
            at(default.radius, 16).points.len() > at(default.radius, 4).points.len(),
            "more sides is a rounder post"
        );
        assert!(bounds(&at(0.09, default.sides), 0).1 > bounds(&at(0.02, default.sides), 0).1);
    }

    /// Python can set either end of both knobs, including the ones the
    /// viewer's sliders can't reach. Nothing may come back degenerate.
    #[test]
    fn a_post_asked_for_at_the_stops_still_builds() {
        for (params, parcel) in [
            (
                PoleParams {
                    radius: 0.0,
                    sides: 0,
                },
                ParcelParams {
                    trellis_height: 0.0,
                    ..parcel()
                },
            ),
            (
                PoleParams {
                    radius: 1.0,
                    sides: 64,
                },
                ParcelParams {
                    trellis_height: 12.0,
                    ..parcel()
                },
            ),
        ] {
            let mesh = pole_mesh(&params, &parcel);
            assert!(mesh.points.iter().flatten().all(|c| c.is_finite()));
            let (z0, z1) = bounds(&mesh, 2);
            let height = z1 - z0;
            assert!(height >= MIN_HEIGHT, "{params:?} came out {height} m tall");
        }
    }

    #[test]
    fn authors_one_prototype() {
        let stage = crate::generate::generate_stage(&VineyardParams::default()).unwrap();
        assert_eq!(prototype_count(&stage, PROTOTYPE), VARIATIONS);
    }

    /// The material has to sit *inside* the variation, because that subtree is
    /// what gets referenced onto the ground and a binding pointing out of it
    /// is silently dropped — see [`material`](super::super::util::material).
    /// Resolved rather than read off the prim, so a target that composed away
    /// to nothing fails here.
    #[test]
    fn a_pole_carries_its_own_material() {
        let authoring = authored(PoleParams::default(), parcel());
        let path = format!("{PROTOTYPE}/Var_0/{POLE}");
        let resolved = MaterialBindingAPI::apply(&authoring.stage, sdf::path(&path).unwrap())
            .unwrap()
            .compute_bound_material("")
            .unwrap()
            .expect("the post resolves a material")
            .as_str()
            .to_string();

        assert_eq!(resolved, format!("{PROTOTYPE}/Var_0/{LOOKS}/{POLE}"));
        assert!(authoring.has(&resolved));
    }

    /// The element owns its subtree and rewrites it from scratch, so a second
    /// pass must leave exactly what the first one did.
    #[test]
    fn re_authoring_rewrites_rather_than_accumulates() {
        let mut authoring = authored(PoleParams::default(), parcel());
        assert!(authoring.has(&format!("{PROTOTYPE}/Var_0/{POLE}")));

        authoring.insert(PoleParams::default()).run();
        assert_eq!(prototype_count(&authoring.stage, PROTOTYPE), VARIATIONS);
    }
}
