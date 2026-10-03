//! Reading a built scene back in tests, in this crate and in a generator's.
//!
//! Always compiled, since a generator's `#[cfg(test)]` modules cannot reach a
//! `#[cfg(test)]` item here. Fixtures for a whole pipeline ([`grown`]), ways
//! to find a prim on the scene graph ([`prim`], [`prim_path`], [`organs`]),
//! the [`MeshData`] readers the kernel tests share, and the two checks every
//! params aggregate is held to ([`check_params`], [`nudged`]).

use bevy::prelude::*;

use crate::geometry::mesh::MeshData;

use crate::Params;
use crate::params::{self, Slider, Widget};
use crate::scene::{Order, PrimRoot};

pub use crate::generate::{grown, scene_app};

// ─── Params ─────────────────────────────────────────────────────────

/// Panics on the first field of `P` that does not declare what the panel, the
/// snippet and the generated docs need — see `docs/editing-parameters.md`.
/// A generator calls it from one test.
pub fn check_params<P: Params>() {
    let default = P::default();
    for fragment in params::fragments::<P>() {
        let info = params::fragment_info(fragment);
        let at = params::stem(fragment);
        assert!(
            !params::paragraphs(info.docs()).is_empty(),
            "{at}: the struct has no doc comment, and it is the class docstring"
        );
        assert!(
            !info.docs().unwrap_or_default().contains('['),
            "{at}: the struct docs use rustdoc link syntax, which Python would show verbatim"
        );
        for field in params::fields(fragment) {
            let at = format!("{}.{}", fragment.name(), field.name());
            let summary = params::summary(field);
            assert!(!summary.is_empty(), "{at}: no doc comment");
            assert!(
                !summary.contains('['),
                "{at}: the first paragraph uses rustdoc link syntax; move it to a later one"
            );
            let value = params::get(&default, fragment.name(), field.name()).unwrap();
            match params::widget(field) {
                Widget::Slider(Slider { min, max, step }) => {
                    let kind = params::python_type(field);
                    assert!(
                        kind != "bool" && kind != "str",
                        "{at}: a @Slider on a non-numeric field"
                    );
                    let now = params::number(value).unwrap();
                    assert!(
                        min < max && step > 0.0,
                        "{at}: slider {min}..={max} by {step}"
                    );
                    assert!(
                        (min..=max).contains(&now),
                        "{at}: default {now} outside {min}..={max}"
                    );
                }
                Widget::Dropdown(names) => {
                    let name = value.try_downcast_ref::<String>().expect("a String field");
                    assert!(
                        names.contains(&name.as_str()),
                        "{at}: default {name:?} not in {names:?}"
                    );
                }
                Widget::Checkbox => {}
            }
        }
    }
}

/// A params set with every field moved off its default, for tests that have
/// to see each one go somewhere: numbers up by one step, flags flipped, names
/// on the next choice.
pub fn nudged<P: Params>() -> P {
    let mut params = P::default();
    for fragment in params::fragments::<P>() {
        for field in params::fields(fragment) {
            let widget = params::widget(field);
            let value = params::get_mut(&mut params, fragment.name(), field.name()).unwrap();
            match widget {
                Widget::Slider(slider) => {
                    let now = params::number(value).unwrap();
                    params::set_number(value, now + slider.step);
                }
                Widget::Dropdown(names) => {
                    let name = value.try_downcast_mut::<String>().unwrap();
                    let at = names.iter().position(|n| n == name).unwrap();
                    *name = names[(at + 1) % names.len()].to_string();
                }
                Widget::Checkbox => {
                    let flag = value.try_downcast_mut::<bool>().unwrap();
                    *flag = !*flag;
                }
            }
        }
    }
    params
}

// ─── The scene graph ────────────────────────────────────────────────

/// One organ, read back off the scene graph.
#[derive(Clone, Debug)]
pub struct Organ<C> {
    /// The prim name — `Plant_007`. Repeats across rows; use
    /// [`path`](Self::path) when identity matters.
    pub name: String,
    /// Slash-joined names from the scene root down,
    /// `Planting/Row_000/Plant_007`.
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

/// Every organ carrying `C`, in the order they were authored.
pub fn organs<C: Component + Clone>(world: &mut World) -> Vec<Organ<C>> {
    let mut query = world.query_filtered::<Entity, (With<Name>, With<Order>, With<C>)>();
    let entities: Vec<Entity> = query.iter(world).collect();

    let mut found: Vec<(Order, Organ<C>)> = entities
        .into_iter()
        .map(|entity| {
            let at = world.entity(entity);
            let organ = Organ {
                name: at.get::<Name>().unwrap().as_str().to_string(),
                path: prim_path(world, entity),
                transform: *at.get::<Transform>().unwrap(),
                config: at.get::<C>().unwrap().clone(),
            };
            (*world.entity(entity).get::<Order>().unwrap(), organ)
        })
        .collect();
    found.sort_by_key(|(order, _)| *order);
    found.into_iter().map(|(_, organ)| organ).collect()
}

/// The names from the scene root down to `entity`, slash-joined.
pub fn prim_path(world: &World, entity: Entity) -> String {
    let root = world.resource::<PrimRoot>().0;
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
    let mut at = world.resource::<PrimRoot>().0;
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

// ─── Meshes ─────────────────────────────────────────────────────────

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

// ─── A generator to test the framework with ─────────────────────────

/// The smallest generator that exercises every surface: two fragments, one
/// element, a row of boxes. What this crate's own tests use in place of a real
/// generator, and the shortest example of what a generator declares.
#[cfg(test)]
pub mod fixture {
    use bevy::prelude::*;

    use crate::geometry::mesh::box_mesh;
    use crate::params::{Choices, Label, Slider};
    use crate::rng::Rng;
    use crate::scene::{Library, Order, PrimRoot, Surface};
    use crate::{Build, Generator};

    /// Scene-wide parameters: the seed.
    #[derive(Resource, Reflect, Clone, Debug, Default, PartialEq)]
    #[cfg_attr(
        feature = "python",
        pyo3::pyclass(get_all, set_all, skip_from_py_object)
    )]
    pub struct SceneParams {
        /// The seed the row's jitter is drawn from.
        #[reflect(@Slider { min: 0.0, max: 64.0, step: 1.0 })]
        pub seed: u64,
    }

    /// A row of boxes along X.
    #[derive(Resource, Reflect, Clone, Debug, PartialEq)]
    #[cfg_attr(
        feature = "python",
        pyo3::pyclass(get_all, set_all, skip_from_py_object)
    )]
    pub struct BoxParams {
        /// How many boxes stand in the row.
        #[reflect(@Slider { min: 1.0, max: 8.0, step: 1.0 })]
        pub count: u32,
        /// Edge length of a box, in meters.
        #[reflect(@Slider { min: 0.1, max: 2.0, step: 0.1 })]
        pub size: f32,
        /// The gap between neighbours, as a fraction of `size`.
        #[reflect(@Slider { min: 0.0, max: 1.0, step: 0.05 })]
        pub gap: f32,
        /// What the boxes are made of.
        #[reflect(@Choices(&MATERIALS))]
        pub material: String,
        /// Whether a box is open at the top.
        pub open: bool,
    }

    pub const MATERIALS: [&str; 2] = ["wood", "stone"];

    impl Default for BoxParams {
        fn default() -> Self {
            Self {
                count: 3,
                size: 0.5,
                gap: 0.5,
                material: "wood".into(),
                open: true,
            }
        }
    }

    crate::generator! {
        /// Every knob of the box row.
        pub struct BoxesParams as PyBoxesParams("BoxesParams") for Boxes {
            pub scene: SceneParams,
            #[reflect(@Label("The boxes"))]
            pub boxes: BoxParams,
        }
    }

    pub struct Boxes;

    impl Generator for Boxes {
        type Params = BoxesParams;
        const NAME: &'static str = "Boxes";
        const PACKAGE: &'static str = "boxlab";

        fn plugin(app: &mut App) {
            app.init_resource::<SceneParams>()
                .init_resource::<BoxParams>()
                .add_systems(
                    PreUpdate,
                    build
                        .run_if(
                            resource_changed::<BoxParams>.or_eager(resource_changed::<SceneParams>),
                        )
                        .in_set(Build),
                );
        }
    }

    /// Marks the row, so a rebuild can take the last one down.
    #[derive(Component)]
    pub struct Row;

    /// The prim name of the one part.
    pub const PART: &str = "Box";

    fn build(
        mut commands: Commands,
        params: Res<BoxParams>,
        scene: Res<SceneParams>,
        root: Res<PrimRoot>,
        mut library: Library,
        standing: Query<Entity, With<Row>>,
    ) {
        for row in &standing {
            commands.entity(row).despawn();
        }
        library.clear(PART);
        let surface = Surface {
            color: [0.5, 0.35, 0.2],
            roughness: 0.8,
            reflectance: 0.4,
            translucency: 0.0,
            thickness: 0.0,
            double_sided: false,
        };
        let geometry = library.part(PART, 0, box_mesh(params.size).to_mesh(), surface);
        let row = commands
            .spawn((
                Row,
                Name::new("Row"),
                Transform::IDENTITY,
                Visibility::default(),
                ChildOf(root.0),
            ))
            .id();
        let mut rng = Rng::new(scene.seed);
        let pitch = params.size * (1.0 + params.gap);
        for i in 0..params.count {
            let jitter = rng.range(-0.1, 0.1) as f32 * params.size;
            commands.spawn((
                Name::new(format!("Box_{i:02}")),
                Order(i as u64),
                Transform::from_xyz(i as f32 * pitch, jitter, params.size / 2.0),
                Visibility::default(),
                geometry.clone(),
                ChildOf(row),
            ));
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::Params;
        use crate::testing::{check_params, grown, nudged, organs, prim};

        #[test]
        fn the_fixture_declares_what_every_generator_must() {
            check_params::<BoxesParams>();
        }

        /// The macro's `apply` reaches every fragment: a set with every field
        /// moved lands in a bare world and reads back out whole.
        #[test]
        fn apply_and_read_round_trip_every_fragment() {
            let params = nudged::<BoxesParams>();
            let mut world = World::new();
            params.apply(&mut world);
            assert_eq!(BoxesParams::read(&world), params);
        }

        #[test]
        fn the_row_stands_under_the_root_and_rebuilds_on_a_change() {
            let mut app = grown::<Boxes>(BoxesParams::default());
            assert_eq!(organs::<Order>(app.world_mut()).len(), 3);
            assert!(prim(app.world_mut(), &["Row", "Box_02"]).is_some());

            app.world_mut().resource_mut::<BoxParams>().count = 5;
            app.update();
            assert_eq!(organs::<Order>(app.world_mut()).len(), 5);
        }
    }
}
