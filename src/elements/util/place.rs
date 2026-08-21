//! The two ways a prototype gets put on the ground.
//!
//! Both take the same [`Placement`] list and the same `/parts/<Name>`
//! prototype root, and differ only in what they author:
//!
//! - [`place_referenced`] gives every instance its **own prim**, an internal
//!   reference to the prototype carrying a `translate`/`rotateZ`/`scale`
//!   stack. Each one has a prim path, which is what a simulator needs to
//!   attach a semantic label, a rigid body, or a randomization handle to an
//!   individual plant.
//! - [`place_instanced`] authors **one** `PointInstancer` prim holding
//!   parallel arrays for all of them. No instance has a path, but the stage
//!   cost is flat no matter how many there are.
//!
//! Use the first for anything a consumer might address — vines, posts. Use
//! the second for scatter that is only ever looked at in aggregate — weeds,
//! leaves, grapes — where per-instance prims would run to five figures.
//!
//! Nothing here knows about vineyards; the caller decides where instances go.

use bevy::prelude::*;
use openusd::gf::{self, f16};
use openusd::schemas::geom::{Imageable, PointInstancer, Xformable};
use openusd::sdf::{self, Value};
use openusd::usd::{Prim, SchemaBase, SchemaKind, Stage};
use usd_bevy::authoring::define_prim;

use super::usd::reference_prim;

/// One placed instance, in the space of whatever prim it is authored under.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub position: Vec3,
    /// Rotation about Z, in radians. The only rotation a ground-planted
    /// prototype needs: elements author their prototypes upright with +X
    /// along the row, so a yaw is all that is ever left to apply.
    pub yaw: f32,
    /// Uniform scale. Uniform rather than per-axis because the thing it
    /// expresses is age — a young plant is shorter *and* thinner.
    pub scale: f32,
    /// Which `Var_<i>` of the prototype library this instance uses.
    pub variation: usize,
}

/// Path of variation `i` in the prototype library rooted at `root`.
///
/// The `Var_<i>` convention is the contract between an element that authors
/// prototypes and whoever places them — see the "Variations" section of
/// `README.md`.
pub fn prototype_path(root: &str, variation: usize) -> String {
    format!("{root}/Var_{variation}")
}

/// How many variations the library at `root` currently holds.
///
/// Counted off the stage rather than read from the owning element's params,
/// because elements compose by prim path only. `Grow::Prototypes` runs ahead
/// of every placement set, so the count is always the current one.
pub fn prototype_count(stage: &Stage, root: &str) -> usize {
    (0..)
        .take_while(|i| usd_bevy::authoring::prim_exists(stage, &prototype_path(root, *i)))
        .count()
}

/// Authors one prim per placement under `parent`, each an internal reference
/// to its variation's prototype.
///
/// `placements` pairs each instance with the prim name it gets. Names are the
/// caller's business precisely because they are the point of this function:
/// they have to stay stable across re-authoring, or a downstream config keyed
/// on a path silently starts pointing at a different plant.
pub fn place_referenced(
    stage: &Stage,
    parent: &str,
    proto_root: &str,
    placements: &[(String, Placement)],
) -> anyhow::Result<()> {
    for (name, placement) in placements {
        let target = prototype_path(proto_root, placement.variation);
        let path = format!("{parent}/{name}");
        define_prim(stage, &path, &reference_type(stage, &target))?;
        reference_prim(stage, &path, &target)?;

        author_transform(stage.prim(sdf::path(&path)?), placement)?;
    }
    Ok(())
}

/// The `translate` / `rotateZ` / `scale` ops for one placement, plus the
/// `xformOpOrder` naming them.
///
/// The op values are written straight onto the prim rather than through
/// [`Xformable::set_translate`] and friends, and the order is set once at the
/// end rather than appended to three times. Those setters each *read* the
/// current `xformOpOrder` before rewriting it, and at one prim per plant that
/// read-modify-write is the single most expensive thing a planting pass does —
/// skipping it takes a default parcel's re-author from 58 ms to 34 ms. The
/// authored result is identical.
fn author_transform(prim: Prim, placement: &Placement) -> anyhow::Result<()> {
    let set = |name: &str, type_name: &str, value: Value| -> anyhow::Result<()> {
        prim.create_attribute(name, type_name)?
            .set_custom(false)?
            .set(value)?;
        Ok(())
    };
    set(
        OP_TRANSLATE,
        "double3",
        Value::Vec3d(gf::vec3d(
            placement.position.x as f64,
            placement.position.y as f64,
            placement.position.z as f64,
        )),
    )?;
    set(
        OP_ROTATE_Z,
        "float",
        Value::Float(placement.yaw.to_degrees()),
    )?;
    set(
        OP_SCALE,
        "float3",
        Value::Vec3f(gf::vec3f(placement.scale, placement.scale, placement.scale)),
    )?;

    // USD applies the *last* op first, so this order scales the instance,
    // turns it along its row, and only then moves it onto the ground — rather
    // than scaling it about the origin after it got there.
    Placed(prim).set_xform_op_order([OP_TRANSLATE, OP_ROTATE_Z, OP_SCALE])?;
    Ok(())
}

const OP_TRANSLATE: &str = "xformOp:translate";
const OP_ROTATE_Z: &str = "xformOp:rotateZ";
const OP_SCALE: &str = "xformOp:scale";

/// The type name to define a referencing prim with: the prototype's own.
///
/// A reference is a *weaker* opinion than a local `typeName`, so defining an
/// `Xform` over a `Mesh` prototype yields an `Xform` carrying `points` — which
/// no renderer will draw, because dispatch is by prim type. Matching the
/// target's type keeps the composed prim renderable, and keeps working when a
/// prototype later grows from a single merged mesh into an `Xform` with canes
/// and leaves under it.
///
/// Falls back to `Xform`, which is the right guess for a prototype root that
/// has not been authored yet — the reference will resolve to nothing, but the
/// namespace stays well-formed.
fn reference_type(stage: &Stage, target: &str) -> String {
    sdf::path(target)
        .ok()
        .and_then(|path| stage.prim(path).type_name().ok().flatten())
        .map(|token| token.to_string())
        .unwrap_or_else(|| "Xform".to_string())
}

/// A bare [`Xformable`] view of a prim of unknown type.
///
/// [`place_referenced`] has to author a transform stack onto whatever type the
/// prototype turned out to be, and the typed views (`Xform::get`, `Mesh::get`,
/// …) each resolve only their own type. `Xformable` needs nothing but the prim
/// itself, so this supplies exactly that.
struct Placed(Prim);

impl SchemaBase for Placed {
    const KIND: SchemaKind = SchemaKind::ConcreteTyped;

    fn prim(&self) -> &Prim {
        &self.0
    }
}
impl Imageable for Placed {}
impl Xformable for Placed {}

/// Authors every placement into a single `PointInstancer` at `path`.
///
/// The stage cost is four arrays regardless of instance count, which is what
/// makes this the only workable option for scatter. The trade is that no
/// instance has a prim path, so nothing downstream can address one.
pub fn place_instanced(
    stage: &Stage,
    path: &str,
    proto_root: &str,
    variations: usize,
    placements: &[Placement],
) -> anyhow::Result<()> {
    let variations = variations.max(1);
    let instancer = PointInstancer::define(stage, sdf::path(path)?)?;
    instancer.create_prototypes_rel()?.set_targets(
        (0..variations)
            .map(|i| sdf::path(prototype_path(proto_root, i)))
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    instancer.create_positions_attr()?.set(Value::Vec3fVec(
        placements
            .iter()
            .map(|p| gf::vec3f(p.position.x, p.position.y, p.position.z))
            .collect(),
    ))?;
    instancer.create_orientations_attr()?.set(Value::QuathVec(
        placements.iter().map(|p| yaw_about_z(p.yaw)).collect(),
    ))?;
    instancer.create_scales_attr()?.set(Value::Vec3fVec(
        placements
            .iter()
            .map(|p| gf::vec3f(p.scale, p.scale, p.scale))
            .collect(),
    ))?;
    instancer.create_proto_indices_attr()?.set(Value::IntVec(
        placements
            .iter()
            .map(|p| (p.variation % variations) as i32)
            .collect(),
    ))?;
    Ok(())
}

/// A rotation of `yaw` radians about Z, as USD types instance orientations.
///
/// `orientations` is `quath[]`, so the components are half floats — about
/// three decimal digits, which is far finer than a row's direction is ever
/// known to.
fn yaw_about_z(yaw: f32) -> gf::Quath {
    let half = yaw * 0.5;
    let half16 = |v: f32| f16::from_f32(v);
    gf::quath(
        half16(half.cos()),
        half16(0.0),
        half16(0.0),
        half16(half.sin()),
    )
}

#[cfg(test)]
mod tests {
    use super::super::usd::{author_mesh, box_mesh};
    use super::*;
    use openusd::schemas::geom::{Mesh, PointBased, Xform};
    use usd_bevy::authoring::prim_exists;

    /// A stage with a two-variation prototype library of plain boxes.
    fn library() -> (Stage, &'static str) {
        let stage = crate::stage::new_stage("place.usda").unwrap();
        for i in 0..2 {
            author_mesh(&stage, &prototype_path("/parts/Thing", i), &box_mesh(1.0)).unwrap();
        }
        define_prim(&stage, "/World", "Xform").unwrap();
        (stage, "/parts/Thing")
    }

    fn placement(x: f32, variation: usize) -> Placement {
        Placement {
            position: Vec3::new(x, 0.0, 0.0),
            yaw: 0.0,
            scale: 1.0,
            variation,
        }
    }

    #[test]
    fn prototype_count_stops_at_the_first_gap() {
        let (stage, root) = library();
        assert_eq!(prototype_count(&stage, root), 2);
        assert_eq!(prototype_count(&stage, "/parts/Nothing"), 0);
    }

    /// The trap this function exists to avoid: a reference is a *weaker*
    /// opinion than a local `typeName`, so defining an `Xform` over a `Mesh`
    /// prototype composes to an `Xform` carrying stray `points` — geometry no
    /// renderer draws, because dispatch is by prim type.
    #[test]
    fn a_referencing_prim_takes_its_prototypes_type() {
        let (stage, root) = library();
        place_referenced(
            &stage,
            "/World",
            root,
            &[("A".to_string(), placement(0.0, 0))],
        )
        .unwrap();

        assert!(
            Xform::get(&stage, sdf::path("/World/A").unwrap())
                .unwrap()
                .is_none(),
            "not an Xform — that would swallow the mesh"
        );
        let mesh = Mesh::get(&stage, sdf::path("/World/A").unwrap())
            .unwrap()
            .expect("typed as the Mesh it references");
        assert!(
            matches!(
                mesh.points_attr().get::<Value>().unwrap(),
                Some(Value::Vec3fVec(p)) if p.len() == 24
            ),
            "the box's points compose in"
        );
    }

    /// An unauthored prototype root still has to leave a well-formed
    /// namespace behind, rather than failing the whole placement pass.
    #[test]
    fn referencing_a_missing_prototype_falls_back_to_an_xform() {
        let (stage, _) = library();
        place_referenced(
            &stage,
            "/World",
            "/parts/Nothing",
            &[("A".to_string(), placement(0.0, 0))],
        )
        .unwrap();
        assert!(prim_exists(&stage, "/World/A"));
    }

    /// USD applies the *last* listed op first, so this order scales the
    /// instance, turns it along its row, and only then moves it onto the
    /// ground — rather than scaling it about the origin after it got there.
    #[test]
    fn the_transform_stack_is_scale_then_rotate_then_translate() {
        let (stage, root) = library();
        place_referenced(
            &stage,
            "/World",
            root,
            &[(
                "A".to_string(),
                Placement {
                    position: Vec3::new(3.0, 5.0, 7.0),
                    yaw: std::f32::consts::FRAC_PI_2,
                    scale: 2.0,
                    variation: 0,
                },
            )],
        )
        .unwrap();

        let mesh = Mesh::get(&stage, sdf::path("/World/A").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            mesh.xform_op_order().unwrap(),
            Some(vec![
                "xformOp:translate".to_string(),
                "xformOp:rotateZ".to_string(),
                "xformOp:scale".to_string(),
            ])
        );

        let m = mesh.local_to_parent_transform(0.0).unwrap();
        // The translation survives the scale unmultiplied, which is what says
        // the scale is applied *before* the move rather than after it.
        assert_eq!([m.0[12], m.0[13], m.0[14]], [3.0, 5.0, 7.0]);
        // And local +X lands on +Y, scaled by 2.
        let along = [m.0[0], m.0[1]];
        assert!(
            along[0].abs() < 1e-6 && (along[1] - 2.0).abs() < 1e-6,
            "got {along:?}"
        );
    }

    #[test]
    fn place_instanced_authors_one_prim_for_every_instance() {
        let (stage, root) = library();
        let placements: Vec<Placement> = (0..5).map(|i| placement(i as f32, i % 2)).collect();
        place_instanced(&stage, "/World/Scatter", root, 2, &placements).unwrap();

        let instancer = PointInstancer::get(&stage, sdf::path("/World/Scatter").unwrap())
            .unwrap()
            .expect("the instancer is authored");
        assert_eq!(
            instancer.prototypes_rel().targets().unwrap().len(),
            2,
            "every variation is a target"
        );
        match instancer.positions_attr().get::<Value>().unwrap() {
            Some(Value::Vec3fVec(v)) => {
                assert_eq!(v.len(), 5);
                assert_eq!(v[3].x, 3.0);
            }
            other => panic!("positions not authored: {other:?}"),
        }
        match instancer.proto_indices_attr().get::<Value>().unwrap() {
            Some(Value::IntVec(v)) => assert_eq!(v, vec![0, 1, 0, 1, 0]),
            other => panic!("protoIndices not authored: {other:?}"),
        }
    }

    /// `orientations` is `quath[]`, so a yaw round-trips through half floats.
    #[test]
    fn place_instanced_orients_around_z() {
        let (stage, root) = library();
        place_instanced(
            &stage,
            "/World/Scatter",
            root,
            1,
            &[Placement {
                yaw: std::f32::consts::FRAC_PI_2,
                ..placement(0.0, 0)
            }],
        )
        .unwrap();

        let instancer = PointInstancer::get(&stage, sdf::path("/World/Scatter").unwrap())
            .unwrap()
            .unwrap();
        match instancer.orientations_attr().get::<Value>().unwrap() {
            Some(Value::QuathVec(v)) => {
                let q = Quat::from_xyzw(
                    v[0].x.to_f32(),
                    v[0].y.to_f32(),
                    v[0].z.to_f32(),
                    v[0].w.to_f32(),
                )
                .normalize();
                let along = q * Vec3::X;
                assert!((along - Vec3::Y).length() < 1e-2, "got {along:?}");
            }
            other => panic!("orientations not authored as quath[]: {other:?}"),
        }
    }
}
