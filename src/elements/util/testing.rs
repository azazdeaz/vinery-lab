//! Test-only helpers shared across the element and util test modules.
//!
//! Fixtures and readers that were copied into three or four test modules
//! because there was nowhere to put them: an app with the whole pipeline in it
//! ([`grown`]), ways to find a prim on the scene graph ([`prim`],
//! [`prim_path`], [`organs`]), and the geometry helpers the kernel tests share.

use bevy::prelude::*;

use super::mesh::MeshData;
use crate::elements::VineyardParams;

/// A headless app with the scene root, the mesh library and asset storage in
/// place — everything an element's build system needs and nothing else.
///
/// `MinimalPlugins` rather than `DefaultPlugins` for the same reason
/// [`generate`](crate::generate) uses it: `DefaultPlugins` installs a *global*
/// `tracing` subscriber, so a second app in one process would panic. `Mesh` and
/// `StandardMaterial` are registered by hand because the plugins that normally
/// do it are the render ones, which nothing here needs.
pub fn scene_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .add_plugins(crate::scene::plugin);
    app
}

/// One organ, read back off the scene graph.
#[derive(Clone, Debug)]
pub struct Organ<C> {
    /// The prim name — `Vine_007`. Repeats across rows; use [`path`](Self::path)
    /// when identity matters.
    pub name: String,
    /// Slash-joined names from the scene root down, `Planting/Row_000/Vine_007`.
    pub path: String,
    pub transform: Transform,
    pub config: C,
}

impl<C> Organ<C> {
    pub fn position(&self) -> Vec3 {
        self.transform.translation
    }

    /// Where the organ's own `+Z` — the axis it was authored standing up on —
    /// ended up.
    pub fn up(&self) -> Vec3 {
        self.transform.rotation * Vec3::Z
    }
}

/// Every organ carrying `C`, in the order planting authored them.
pub fn organs<C: Component + Clone>(world: &mut World) -> Vec<Organ<C>> {
    let mut query = world.query_filtered::<Entity, (With<Name>, With<crate::scene::Order>, With<C>)>();
    let entities: Vec<Entity> = query.iter(world).collect();

    let mut found: Vec<(crate::scene::Order, Organ<C>)> = entities
        .into_iter()
        .map(|entity| {
            let at = world.entity(entity);
            let organ = Organ {
                name: at.get::<Name>().unwrap().as_str().to_string(),
                path: prim_path(world, entity),
                transform: *at.get::<Transform>().unwrap(),
                config: at.get::<C>().unwrap().clone(),
            };
            (*world.entity(entity).get::<crate::scene::Order>().unwrap(), organ)
        })
        .collect();
    found.sort_by_key(|(order, _)| *order);
    found.into_iter().map(|(_, organ)| organ).collect()
}

/// The names from the scene root down to `entity`, slash-joined.
pub fn prim_path(world: &World, entity: Entity) -> String {
    let root = world.resource::<crate::scene::PrimRoot>().0;
    let mut names = Vec::new();
    let mut at = entity;
    loop {
        if at == root {
            break;
        }
        let Some(name) = world.entity(at).get::<Name>() else {
            break;
        };
        names.push(name.as_str().to_string());
        match world.entity(at).get::<ChildOf>() {
            Some(parent) => at = parent.0,
            None => break,
        }
    }
    names.reverse();
    names.join("/")
}

/// The entity at `path` below the scene root — `["Planting", "Row_000"]`.
pub fn prim(world: &mut World, path: &[&str]) -> Option<Entity> {
    let mut at = world.resource::<crate::scene::PrimRoot>().0;
    for name in path {
        at = named_children(world, at)
            .into_iter()
            .find(|(child, _)| child == name)?
            .1;
    }
    Some(at)
}

/// Named children of `entity`, in `Children` order.
pub fn named_children(world: &mut World, entity: Entity) -> Vec<(String, Entity)> {
    let Some(children) = world.entity(entity).get::<Children>() else {
        return Vec::new();
    };
    let children: Vec<Entity> = children.to_vec();
    children
        .into_iter()
        .filter_map(|child| {
            let name = world.entity(child).get::<Name>()?.as_str().to_string();
            Some((name, child))
        })
        .collect()
}

/// Runs one build cycle and hands back the app, so a test can read the scene
/// graph and the resources it was built from — the solved `VineyardLayout` and
/// `Ground` are what a placement check is asserted against.
pub fn grown(params: VineyardParams) -> App {
    let mut app = scene_app();
    app.add_plugins(crate::elements::plugin);
    params.insert(app.world_mut());
    app.finish();
    app.cleanup();
    app.update();
    app
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
