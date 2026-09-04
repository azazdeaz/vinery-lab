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
//! # One layer
//!
//! [`planting`](super::util::planting) authors a [`PoleConfig`] per post and
//! places it; this element turns the distinct configs into meshes. The variety
//! a row shows is per placement, not per shape: a post is a manufactured
//! object, identical to its neighbour except in how it was driven.
//!
//! [`ParcelParams::trellis_height`]: super::util::parcel::ParcelParams::trellis_height
//! [`cylinder_mesh`]: super::util::mesh::cylinder_mesh

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use crate::quantize::{Metric, farthest_first};
use crate::scene::{COLLISION, Geometry, Library, Order, Surface, capsule, configs_changed};

use super::Grow;
use super::util::parcel::ParcelParams;
use super::util::mesh::{MeshData, cylinder_mesh};
use super::util::{color, material};

/// The mesh-library prefix this element registers its geometry under.
pub const PART: &str = "Pole";

/// The prim a post's geometry takes, below the post itself.
///
/// A child rather than the post prim, because the post also carries a
/// collision proxy and geometry prims carry no children — see
/// [`scene`](crate::scene).
pub const POST: &str = "Post";

/// How many distinct post meshes the scene may hold.
///
/// One, where every other element has a `variations` knob. A post is a
/// manufactured object: two of them differ in how they were driven — a
/// centimeter off line, a degree off plumb, a few centimeters deeper — and
/// that is per *placement*, costs no geometry, and is where
/// [`planting`](super::util::planting) puts it. Raise it if a post's *shape*
/// ever varies down a row.
pub const VARIATIONS: usize = 1;

/// Shortest post we will build. A trellis height of zero is reachable from
/// Python, and a zero-length cylinder is a mesh with two coincident rings.
const MIN_HEIGHT: f32 = 0.1;

// ─── Config ─────────────────────────────────────────────────────────

/// One post's shape.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PoleConfig {
    pub radius: f32,
    /// How far it stands above the ground.
    pub height: f32,
    /// Vertices around the post.
    pub sides: u32,
}

impl PoleConfig {
    /// The post a trellis of this height calls for.
    ///
    /// Clamped here rather than in the mesh builder, so the config a metric
    /// compares is the shape that actually gets built.
    pub fn new(params: &PoleParams, parcel: &ParcelParams) -> Self {
        Self {
            radius: params.radius.max(0.001),
            height: parcel.trellis_height.max(MIN_HEIGHT),
            sides: params.sides.max(3),
        }
    }

    /// One post, standing on the origin and running up +Z.
    fn mesh(&self) -> MeshData {
        cylinder_mesh(self.radius, self.height, self.sides as usize)
    }
}

/// Two posts share a mesh when they are close in every dimension a post has.
///
/// The weights convert each field into roughly how far apart it *looks*: a
/// centimetre of radius shows on the silhouette, ten centimetres of height
/// barely does, and a facet either way is close to invisible past the first
/// few.
pub struct PoleMetric;

impl Metric<PoleConfig> for PoleMetric {
    fn distance(&self, a: &PoleConfig, b: &PoleConfig) -> f32 {
        [
            a.radius - b.radius,
            (a.height - b.height) * 0.1,
            (a.sides as f32 - b.sides as f32) * 0.01,
        ]
        .iter()
        .map(|d| d * d)
        .sum::<f32>()
        .sqrt()
    }
}

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
        build
            .in_set(Grow::Poles)
            .run_if(configs_changed::<PoleConfig>),
    );
}

// ─── Building ───────────────────────────────────────────────────────

/// Builds one mesh per distinct post and gives it to every post that drew it.
///
/// Every post gets two prims: the shared mesh, and a capsule standing in for
/// it in physics. Both are sized from the *representative* the post drew, so
/// the proxy matches the geometry beside it rather than the shape this post
/// asked for and did not get.
pub fn build(
    mut commands: Commands,
    mut library: Library,
    posts: Query<(Entity, &Order, &PoleConfig)>,
) {
    library.clear(PART);

    let mut posts: Vec<(Order, Entity, PoleConfig)> = posts
        .iter()
        .map(|(entity, order, config)| (*order, entity, *config))
        .collect();
    posts.sort_by_key(|(order, ..)| *order);

    let configs: Vec<PoleConfig> = posts.iter().map(|(_, _, config)| *config).collect();
    let book = farthest_first(&configs, VARIATIONS, 0.0, &PoleMetric);

    let geometry: Vec<Geometry> = book
        .representatives
        .iter()
        .enumerate()
        .map(|(i, config)| library.part(PART, i, config.mesh().to_mesh(), surface()))
        .collect();

    for ((_, entity, _), drew) in posts.iter().zip(&book.assignment) {
        let drew = *drew as usize;
        let built = &book.representatives[drew];
        let mut post = commands.entity(*entity);
        // The layer owns everything below a post, so a rebuild replaces what
        // the last one hung there rather than doubling it.
        post.despawn_children();
        post.with_child((Name::new(POST), geometry[drew].clone()));
        // A post stands on the origin, so its proxy spans the same 0..height
        // the mesh does.
        post.with_child((Name::new(COLLISION), capsule(built.radius, 0.0, built.height)));
    }
}

/// No per-post shade: a row of posts off the same pallet is one colour.
fn surface() -> Surface {
    material::POLE.surface(color::srgb(color::POLE))
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
    use crate::elements::util::testing::{self, bounds, named_children, organs};
    use crate::scene::{Collider, Prototypes};

    fn config(params: PoleParams, trellis_height: f32) -> PoleConfig {
        PoleConfig::new(
            &params,
            &ParcelParams {
                trellis_height,
                ..default()
            },
        )
    }

    /// Runs the layer over `posts`, and hands back the mesh library it filled
    /// together with which post drew what.
    fn built(posts: &[PoleConfig], budget: usize) -> (Vec<String>, Vec<u32>) {
        let book = farthest_first(posts, budget, 0.0, &PoleMetric);
        let names = (0..book.len()).map(|i| format!("{PART}_{i}")).collect();
        (names, book.assignment)
    }

    /// The placement contract: a post stands *on* the origin rather than
    /// straddling it, and reaches the trellis height the layout solved its
    /// wires for. Authored centered, every post would be sunk half its length
    /// into the ground.
    #[test]
    fn a_pole_stands_on_the_origin_and_reaches_the_trellis_height() {
        let params = PoleParams::default();
        for trellis_height in [0.9, 1.8, 3.0] {
            let mesh = config(params.clone(), trellis_height).mesh();
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
        let at = |radius, sides| config(PoleParams { radius, sides }, 1.8).mesh();
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
        for (params, trellis_height) in [
            (PoleParams { radius: 0.0, sides: 0 }, 0.0),
            (PoleParams { radius: 1.0, sides: 64 }, 12.0),
        ] {
            let mesh = config(params.clone(), trellis_height).mesh();
            assert!(mesh.points.iter().flatten().all(|c| c.is_finite()));
            let (z0, z1) = bounds(&mesh, 2);
            let height = z1 - z0;
            assert!(height >= MIN_HEIGHT, "{params:?} came out {height} m tall");
        }
    }

    /// Posts are identical by construction today, so the whole parcel shares
    /// one mesh however many posts stand in it.
    #[test]
    fn identical_posts_share_one_mesh() {
        let posts = vec![config(PoleParams::default(), 1.8); 40];
        let (names, drew) = built(&posts, VARIATIONS);

        assert_eq!(names.len(), 1, "one distinct post, one mesh");
        assert!(drew.iter().all(|d| *d == 0), "and every post drew it");
    }

    /// The budget is what makes that true, not luck: given posts that really
    /// differ, the metric has to tell them apart.
    #[test]
    fn posts_that_differ_are_told_apart() {
        let posts = vec![
            config(PoleParams::default(), 1.8),
            config(PoleParams::default(), 1.8),
            config(PoleParams { radius: 0.2, sides: 8 }, 1.8),
        ];
        let (names, drew) = built(&posts, 2);

        assert_eq!(names.len(), 2);
        assert_eq!(drew[0], drew[1], "the two matching posts share a mesh");
        assert_ne!(drew[0], drew[2], "the thick one gets its own");
    }

    /// A post is two prims: the mesh every post shares, and the capsule a robot
    /// bumps into. The proxy stands on the same origin the mesh does and
    /// reaches the same top — a collider that missed either would leave a post
    /// a robot walks through, or trips on nothing beside.
    #[test]
    fn a_post_is_a_mesh_and_a_proxy_that_covers_it() {
        let mut app = testing::grown(VineyardParams::default());
        let post = testing::prim(app.world_mut(), &["Planting", "Row_000", "Pole_000"])
            .expect("the first post of the first row");
        let built = *app.world().entity(post).get::<PoleConfig>().unwrap();

        let children = named_children(app.world_mut(), post);
        let names: Vec<&str> = children.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, [POST, COLLISION]);

        let at = app.world().entity(children[1].1);
        let shape = at.get::<Collider>().unwrap().0;
        let z = at.get::<Transform>().unwrap().translation.z;
        let reach = shape.height / 2.0 + shape.radius;

        assert_eq!(shape.radius, built.radius);
        assert!((z - reach).abs() < 1e-6, "stands on the origin, like the mesh");
        assert!((z + reach - built.height).abs() < 1e-6, "and reaches the wire");
    }

    /// End to end through the schedule: every post the parcel planted comes out
    /// carrying geometry, and the library holds exactly what they drew.
    #[test]
    fn every_planted_post_draws_a_mesh_from_the_library() {
        let mut app = testing::grown(VineyardParams::default());
        let posts = organs::<PoleConfig>(app.world_mut());
        assert!(posts.len() > 20, "the fixture planted posts");

        let library = app.world().resource::<Prototypes>();
        assert!(library.get(&format!("{PART}_0")).is_some());
        assert_eq!(
            (0..)
                .take_while(|i| library.get(&format!("{PART}_{i}")).is_some())
                .count(),
            VARIATIONS
        );
    }
}
