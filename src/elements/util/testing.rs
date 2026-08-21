//! Test-only helpers shared across the element and util test modules.
//!
//! Two kinds of thing live here. The first is plain deduplication: fixtures
//! that were copied into three or four test modules because there was nowhere
//! to put them.
//!
//! The second is [`instances`], which is why this module exists at all. A
//! batch of plants is a prim each or a `PointInstancer`'s parallel arrays
//! depending on the [`Style`] in force, but *where the plants ended up* is the
//! same either way — so a test that reads them back through here asserts once
//! and covers both paths, instead of being written twice.

use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::ScheduleSystem;
use bevy::prelude::*;
use openusd::schemas::geom::{PointInstancer, Xformable};
use openusd::sdf::{self, Value};
use openusd::usd::Stage;
use usd_bevy::live::LiveStage;

use super::place::{Placed, Style};
use super::usd::MeshData;
use crate::elements::VineyardParams;

/// Both placement styles, for the tests that have to hold either way.
///
/// A plain loop rather than a parameterized-test crate: the suite has no
/// dev-dependencies, and `{style:?}` in the assert messages reads the same in
/// a failure as a generated case name would.
pub const STYLES: [Style; 2] = [Style::Referenced, Style::Instanced];

/// Runs one authoring cycle at `style`, handing back the stage together with
/// the app it was authored from — tests need the solved `VineyardLayout` and
/// `Ground` to check the placed prims against what they were placed from.
pub fn grown(params: VineyardParams, style: Style) -> (Stage, App) {
    let stage = crate::stage::new_stage("testing.usda").unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(crate::elements::plugin);
    params.insert(app.world_mut());
    // After the element plugins, so it overrides `elements::plugin`'s default.
    app.world_mut().insert_resource(style);
    app.world_mut()
        .insert_non_send(LiveStage::new(stage.clone()));
    app.finish();
    app.cleanup();
    app.update();
    (stage, app)
}

/// Lowest and highest coordinate of a mesh's points along `axis`.
pub fn bounds(mesh: &MeshData, axis: usize) -> (f32, f32) {
    mesh.points
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(p[axis]), hi.max(p[axis]))
        })
}

/// The corner indices of each face, walking the `face_vertex_counts` /
/// `face_vertex_indices` pair a [`MeshData`] stores its faces as.
pub fn faces(mesh: &MeshData) -> impl Iterator<Item = &[i32]> {
    let mut cursor = 0usize;
    mesh.face_vertex_counts.iter().map(move |count| {
        let face = &mesh.face_vertex_indices[cursor..cursor + *count as usize];
        cursor += *count as usize;
        face
    })
}

/// A face's normal, from its first three corners. Unnormalized: the winding
/// tests only ever ask which side of something it points, and a zero-area face
/// should fail those rather than produce a NaN direction.
pub fn face_normal(mesh: &MeshData, face: &[i32]) -> Vec3 {
    let [a, b, c] = [0, 1, 2].map(|i| corner(mesh, face[i]));
    (b - a).cross(c - a)
}

/// A face's centroid — the reference point a normal is compared against when
/// "outward" means "away from the middle of the thing".
pub fn face_centroid(mesh: &MeshData, face: &[i32]) -> Vec3 {
    face.iter().map(|i| corner(mesh, *i)).sum::<Vec3>() / face.len() as f32
}

fn corner(mesh: &MeshData, index: i32) -> Vec3 {
    Vec3::from(mesh.points[index as usize])
}

/// One placed instance, read back off the stage.
#[derive(Clone, Debug)]
pub struct Instance {
    /// The prim's name — `None` when it went into a `PointInstancer` and has
    /// no path of its own, which is exactly what the two styles trade.
    pub name: Option<String>,
    /// Where it ended up, relative to the prim it was placed under.
    pub transform: Mat4,
    /// Path of the prototype it draws.
    pub prototype: String,
}

impl Instance {
    /// The prototype's `Var_<i>` index.
    pub fn variation(&self) -> usize {
        self.prototype
            .rsplit_once("Var_")
            .and_then(|(_, i)| i.parse().ok())
            .unwrap_or_else(|| panic!("`{}` is not a Var_<i> path", self.prototype))
    }

    /// Its position, and the scale and direction baked into its transform.
    pub fn position(&self) -> Vec3 {
        self.transform.to_scale_rotation_translation().2
    }

    pub fn scale(&self) -> Vec3 {
        self.transform.to_scale_rotation_translation().0
    }

    pub fn rotation(&self) -> Quat {
        self.transform.to_scale_rotation_translation().1
    }
}

/// Every instance placed under `parent`, whichever style placed it.
///
/// Child prims are read as reference-placed instances; a child that is a
/// `PointInstancer` is expanded into one entry per row of its arrays. A parent
/// may hold both — a vine prototype's `Wood` sits beside its shoots — so
/// anything without a prototype behind it is skipped rather than reported as
/// an instance of nothing.
pub fn instances(stage: &Stage, parent: &str) -> Vec<Instance> {
    let mut out = Vec::new();
    for child in stage.prim(sdf::path(parent).unwrap()).children().unwrap() {
        if let Some(instancer) = PointInstancer::get(stage, child.path().clone()).unwrap() {
            out.extend(instancer_rows(&instancer));
            continue;
        }
        // The prim stack names every site contributing an opinion; for a
        // referencing prim that includes the target it pulled in.
        let Some(prototype) = child
            .prim_stack()
            .unwrap()
            .into_iter()
            .map(|(_, path)| path.as_str().to_string())
            .find(|path| path.starts_with(crate::stage::PARTS))
        else {
            continue;
        };
        let m = Placed(child.clone())
            .local_to_parent_transform(0.0)
            .unwrap();
        out.push(Instance {
            name: child.path().name().map(str::to_string),
            transform: Mat4::from_cols_array(&m.0.map(|v| v as f32)),
            prototype,
        });
    }
    out
}

/// One entry per row of an instancer's parallel arrays.
fn instancer_rows(instancer: &PointInstancer) -> Vec<Instance> {
    let prototypes: Vec<String> = instancer
        .prototypes_rel()
        .targets()
        .unwrap()
        .iter()
        .map(|t| t.as_str().to_string())
        .collect();
    let Some(Value::Vec3fVec(positions)) = instancer.positions_attr().get::<Value>().unwrap()
    else {
        panic!("an instancer without positions");
    };
    let orientations = match instancer.orientations_attr().get::<Value>().unwrap() {
        Some(Value::QuathVec(v)) => v,
        other => panic!("orientations not authored as quath[]: {other:?}"),
    };
    let Some(Value::Vec3fVec(scales)) = instancer.scales_attr().get::<Value>().unwrap() else {
        panic!("an instancer without scales");
    };
    let Some(Value::IntVec(indices)) = instancer.proto_indices_attr().get::<Value>().unwrap()
    else {
        panic!("an instancer without protoIndices");
    };

    (0..positions.len())
        .map(|i| {
            let q = orientations[i];
            Instance {
                name: None,
                transform: Mat4::from_scale_rotation_translation(
                    Vec3::new(scales[i].x, scales[i].y, scales[i].z),
                    Quat::from_xyzw(q.x.to_f32(), q.y.to_f32(), q.z.to_f32(), q.w.to_f32())
                        .normalize(),
                    Vec3::new(positions[i].x, positions[i].y, positions[i].z),
                ),
                prototype: prototypes[indices[i] as usize].clone(),
            }
        })
        .collect()
}

/// A world and a schedule running one author system directly, for the tests
/// that drive an element's authoring rather than a whole app.
///
/// Running it twice is the point: every author fn owns its subtree and
/// rewrites it from scratch, and what those tests check is what the *second*
/// run leaves behind.
pub struct Authoring {
    pub stage: Stage,
    world: World,
    schedule: Schedule,
}

impl Authoring {
    pub fn new<M>(identifier: &str, system: impl IntoScheduleConfigs<ScheduleSystem, M>) -> Self {
        let stage = crate::stage::new_stage(identifier).unwrap();
        let mut world = World::new();
        world.insert_non_send(LiveStage::new(stage.clone()));
        let mut schedule = Schedule::default();
        schedule.add_systems(system);
        Self {
            stage,
            world,
            schedule,
        }
    }

    /// Inserts a resource the system under test reads. Overwrites, so a second
    /// call is how a test changes params between runs.
    pub fn insert<R: Resource>(&mut self, resource: R) -> &mut Self {
        self.world.insert_resource(resource);
        self
    }

    pub fn run(&mut self) -> &mut Self {
        self.schedule.run(&mut self.world);
        self
    }

    pub fn has(&self, path: &str) -> bool {
        usd_bevy::authoring::prim_exists(&self.stage, path)
    }
}
