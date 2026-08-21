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
//! Anything that could go either way asks [`Style`] instead of choosing, and
//! [`place`] dispatches. That is what lets the viewer trade addressability for
//! authoring speed while the export keeps it.
//!
//! Nothing here knows about vineyards; the caller decides where instances go.

use bevy::prelude::*;
use openusd::gf::{self, f16};
use openusd::schemas::geom::{Imageable, PointInstancer, Xformable};
use openusd::sdf::{self, Value};
use openusd::usd::{Prim, SchemaBase, SchemaKind, Stage};
use usd_bevy::authoring::define_prim;

use super::usd::reference_prim;

/// Which of the two placement paths an authoring pass takes.
///
/// A resource rather than an argument each caller decides for itself, because
/// it has to be the *same* for every batch in one pass. The combination that
/// breaks is a `PointInstancer` nested inside a reference-placed prototype:
/// its `prototypes` relationship targets the library it draws from, which sits
/// outside the referenced subtree, so the reference's namespace mapping cannot
/// map it and USD drops it — an instancer keeping every instance and losing
/// every prototype, drawing nothing, silently. That is the same failure
/// [`stage::define_parts_library`] guards one level up. One value for the
/// whole pass makes the combination unreachable.
///
/// [`stage::define_parts_library`]: crate::stage
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    /// One prim per instance. What a simulator addresses, and the shape the
    /// generated scene is exported in.
    #[default]
    Referenced,
    /// One `PointInstancer` per batch. What the viewer forces: re-authoring a
    /// parcel becomes four array writes rather than a prim and three
    /// attributes per plant.
    Instanced,
}

/// One placed instance, in the space of whatever prim it is authored under.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub position: Vec3,
    /// Rotation about Z, in radians. The only rotation a ground-planted
    /// prototype needs: elements author their prototypes upright with +X
    /// along the row, so a yaw is all that is ever left to apply.
    pub yaw: f32,
    /// Small rotations about local X and Y, in radians, applied *before* the
    /// yaw — so a prototype tips in its own frame and is then turned into
    /// place.
    ///
    /// Zero for anything planted on the ground: a vine stands upright, and a
    /// yaw is the whole story. It earns its keep in nested placement, where a
    /// handful of prototypes are used over and over within a single parent —
    /// the shoots on a vine's spurs are a dozen copies of four meshes, and a
    /// per-instance lean is what stops them reading as clones.
    pub tilt: Vec2,
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

/// Places `placements` under `parent`, by whichever path `style` names.
///
/// `group` is the name the `PointInstancer` takes *under* `parent`; it goes
/// unused when referencing, where every placement carries its own prim name.
/// Keeping the instancer a child rather than letting it replace `parent` means
/// the caller's own subtree shape — a `Scope` per row, an `Xform` over a
/// vine's wood — is the same either way, and only what hangs below it changes.
#[allow(clippy::too_many_arguments)]
pub fn place(
    stage: &Stage,
    style: Style,
    parent: &str,
    group: &str,
    proto_root: &str,
    variations: usize,
    placements: &[(String, Placement)],
) -> anyhow::Result<()> {
    match style {
        Style::Referenced => place_referenced(stage, parent, proto_root, placements),
        Style::Instanced => {
            // The names are the referenced path's whole point, and the
            // instanced path has nowhere to put them: an instance is a row in
            // four arrays, not a prim.
            let flat: Vec<Placement> = placements.iter().map(|(_, p)| *p).collect();
            place_instanced(
                stage,
                &format!("{parent}/{group}"),
                proto_root,
                variations,
                &flat,
            )
        }
    }
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
    // `rotateXYZ` is Rz·Ry·Rx — X first, then Y, then the yaw last, which is
    // the order the tilt is defined in. One op rather than three keeps the
    // authored stack the same length it was when a yaw was all there was.
    set(
        OP_ROTATE_XYZ,
        "float3",
        Value::Vec3f(gf::vec3f(
            placement.tilt.x.to_degrees(),
            placement.tilt.y.to_degrees(),
            placement.yaw.to_degrees(),
        )),
    )?;
    set(
        OP_SCALE,
        "float3",
        Value::Vec3f(gf::vec3f(placement.scale, placement.scale, placement.scale)),
    )?;

    // USD applies the *last* op first, so this order scales the instance,
    // turns it along its row, and only then moves it onto the ground — rather
    // than scaling it about the origin after it got there.
    Placed(prim).set_xform_op_order([OP_TRANSLATE, OP_ROTATE_XYZ, OP_SCALE])?;
    Ok(())
}

const OP_TRANSLATE: &str = "xformOp:translate";
const OP_ROTATE_XYZ: &str = "xformOp:rotateXYZ";
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
pub(crate) struct Placed(pub(crate) Prim);

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
        placements.iter().map(orientation).collect(),
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

/// A placement's rotation, as USD types instance orientations.
///
/// `EulerRot::ZYX` composes to Rz·Ry·Rx, the same rotation
/// [`author_transform`]'s `rotateXYZ` op applies — the two placement paths
/// have to agree, or a prototype would lean one way planted and another way
/// instanced.
///
/// `orientations` is `quath[]`, so the components are half floats — about
/// three decimal digits, which is far finer than a row's direction is ever
/// known to.
fn orientation(placement: &Placement) -> gf::Quath {
    let q = Quat::from_euler(
        EulerRot::ZYX,
        placement.yaw,
        placement.tilt.y,
        placement.tilt.x,
    );
    let half16 = |v: f32| f16::from_f32(v);
    gf::quath(half16(q.w), half16(q.x), half16(q.y), half16(q.z))
}

#[cfg(test)]
mod tests {
    use super::super::usd::{author_mesh, box_mesh};
    use super::*;
    use openusd::schemas::geom::{Mesh, PointBased, Xform};
    use usd_bevy::authoring::prim_exists;

    /// A prototype library root inside the scene root, where a real element's
    /// would be — targets that leave the default prim are dropped the moment
    /// the layer is referenced, so the fixtures model the shape that works.
    const THING: &str = "/Vineyard/parts/Thing";

    /// A stage with a two-variation prototype library of plain boxes.
    fn library() -> (Stage, &'static str) {
        let stage = crate::stage::new_stage("place.usda").unwrap();
        for i in 0..2 {
            author_mesh(&stage, &prototype_path(THING, i), &box_mesh(1.0)).unwrap();
        }
        define_prim(&stage, "/World", "Xform").unwrap();
        (stage, THING)
    }

    fn placement(x: f32, variation: usize) -> Placement {
        Placement {
            position: Vec3::new(x, 0.0, 0.0),
            yaw: 0.0,
            tilt: Vec2::ZERO,
            scale: 1.0,
            variation,
        }
    }

    #[test]
    fn prototype_count_stops_at_the_first_gap() {
        let (stage, root) = library();
        assert_eq!(prototype_count(&stage, root), 2);
        assert_eq!(prototype_count(&stage, "/Vineyard/parts/Nothing"), 0);
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
            "/Vineyard/parts/Nothing",
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
                    tilt: Vec2::ZERO,
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
                "xformOp:rotateXYZ".to_string(),
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

    /// A tilt has to tip the prototype in its *own* frame and only then be
    /// turned by the yaw. Authored the other way round, a shoot leaning
    /// "outward" would lean outward in the vine's frame rather than its own,
    /// and every shoot on a spur would lean the same way.
    #[test]
    fn a_tilt_leans_the_prototype_before_the_yaw_turns_it() {
        let (stage, root) = library();
        let tilt = 0.2_f32;
        place_referenced(
            &stage,
            "/World",
            root,
            &[(
                "A".to_string(),
                Placement {
                    yaw: std::f32::consts::FRAC_PI_2,
                    tilt: Vec2::new(tilt, 0.0),
                    ..placement(0.0, 0)
                },
            )],
        )
        .unwrap();

        let mesh = Mesh::get(&stage, sdf::path("/World/A").unwrap())
            .unwrap()
            .unwrap();
        let m = mesh.local_to_parent_transform(0.0).unwrap();
        let column = |i: usize| {
            Vec3::new(m.0[i * 4] as f32, m.0[i * 4 + 1] as f32, m.0[i * 4 + 2] as f32)
        };

        // Local +Z is off vertical by exactly the tilt...
        let up = column(2);
        assert!(
            (up.angle_between(Vec3::Z) - tilt).abs() < 1e-5,
            "local +Z leans by the tilt, got {up:?}"
        );
        // ...and the yaw still turns local +X onto +Y, which is what says the
        // tilt was applied first rather than in the parent's frame.
        let along = column(0);
        assert!(
            (along - Vec3::Y).length() < 1e-5,
            "the yaw survives the tilt, got {along:?}"
        );
    }

    /// Both placement paths have to apply the same rotation, or a prototype
    /// would lean one way planted and another way instanced.
    #[test]
    fn instanced_and_referenced_placements_agree_on_the_rotation() {
        let p = Placement {
            yaw: 0.7,
            tilt: Vec2::new(0.15, -0.1),
            ..placement(0.0, 0)
        };
        let q = orientation(&p);
        let instanced = Quat::from_xyzw(
            q.x.to_f32(),
            q.y.to_f32(),
            q.z.to_f32(),
            q.w.to_f32(),
        )
        .normalize();

        let (stage, root) = library();
        place_referenced(&stage, "/World", root, &[("A".to_string(), p)]).unwrap();
        let m = Mesh::get(&stage, sdf::path("/World/A").unwrap())
            .unwrap()
            .unwrap()
            .local_to_parent_transform(0.0)
            .unwrap();
        let referenced = Mat4::from_cols_array(&m.0.map(|v| v as f32))
            .to_scale_rotation_translation()
            .1;

        // Half floats, so the instanced quat is only good to ~3 digits.
        assert!(
            (referenced * Vec3::Z - instanced * Vec3::Z).length() < 1e-2,
            "{referenced:?} vs {instanced:?}"
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
        let targets = instancer.prototypes_rel().targets().unwrap();
        assert_eq!(targets.len(), 2, "every variation is a target");
        // `prototypes` is a relationship, and relationship targets are
        // namespace-mapped through composition arcs: one pointing outside the
        // default prim cannot be mapped and USD drops it, leaving an instancer
        // with every instance and no prototype. Opened directly that stage
        // looks perfect, so the check has to live here.
        assert!(
            targets
                .iter()
                .all(|t| t.as_str().starts_with(crate::stage::ROOT)),
            "prototype targets stay under the default prim, got {targets:?}"
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

    /// The dispatcher's whole contract: same batch, same prototypes, two
    /// shapes on the stage — a prim each under `parent`, or one instancer
    /// beside them named `group`.
    #[test]
    fn place_switches_between_a_prim_each_and_one_instancer() {
        let batch = [
            ("A".to_string(), placement(0.0, 0)),
            ("B".to_string(), placement(1.0, 1)),
        ];
        let author = |style| {
            let (stage, root) = library();
            place(&stage, style, "/World", "Batch", root, 2, &batch).unwrap();
            stage
        };

        let referenced = author(Style::Referenced);
        assert!(prim_exists(&referenced, "/World/A") && prim_exists(&referenced, "/World/B"));
        assert!(
            !prim_exists(&referenced, "/World/Batch"),
            "no instancer when every instance is its own prim"
        );

        let instanced = author(Style::Instanced);
        assert!(
            !prim_exists(&instanced, "/World/A") && !prim_exists(&instanced, "/World/B"),
            "the names are dropped — an instance is a row in four arrays"
        );
        let instancer = PointInstancer::get(&instanced, sdf::path("/World/Batch").unwrap())
            .unwrap()
            .expect("the batch is one instancer");
        assert!(
            matches!(
                instancer.proto_indices_attr().get::<Value>().unwrap(),
                Some(Value::IntVec(v)) if v == vec![0, 1]
            ),
            "carrying both placements, each still on its own variation"
        );
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
