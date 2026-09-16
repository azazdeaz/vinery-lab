//! Wire element — the trellis wires strung from post to post.
//!
//! What the posts are there to hold up. One **fruiting wire** on the post axis,
//! at the head height the cordons are tied along, and above it `catch_wires`
//! levels of **pairs** stapled either side of the post, that the season's
//! shoots are tucked up between. The topmost pair is the "top wire" a hedger
//! cuts to.
//!
//! # A cylinder, built once
//!
//! A wire is three millimeters of high-tensile steel, and at that gauge the
//! only thing that matters is that it is there from every angle. A flat strip
//! would be cheaper and would vanish edge-on — a robot drives *along* a row as
//! often as across it, and nothing in a static USD turns a billboard to face
//! it. So it is a six-sided [`cylinder_mesh`], twenty triangles, authored once
//! at a one-meter length and stretched to each span by the scale on its
//! geometry prim.
//!
//! # One span per panel
//!
//! A wire prim runs between two neighbouring posts and no further, so a row of
//! them follows the ground the way the posts do: piecewise, at whatever height
//! each post ended up holding. Post tops on a slope differ panel to panel, so
//! one mesh spanning the whole row would need a shear, which a translate /
//! rotate / scale stack cannot express.
//!
//! # Local frame
//!
//! A span is placed **on the anchor it leaves**, running up +Z to the one it
//! reaches — the same convention as every other prototype here, and what lets
//! a collision proxy be added beside the geometry later without moving a prim.
//!
//! # One layer
//!
//! [`planting`](super::util::planting) solves the spans, since it is what holds
//! the post frames, and authors a [`WireConfig`] at each; this element
//! registers the one mesh and hands it to every span. Nothing to quantize: one
//! gauge, one mesh — the exemption [`terrain`](super::terrain) takes for the
//! same reason.
//!
//! [`cylinder_mesh`]: super::util::mesh::cylinder_mesh

use crate::scene::{Library, Surface, configs_changed};
use crate::ui::Staged;
use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};

use super::Grow;
use super::pole;
use super::util::mesh::cylinder_mesh;
use super::util::{color, material};

/// The mesh-library prefix this element registers its geometry under.
pub const PART: &str = "Wire";

/// The prim a span's geometry takes, below the span itself.
///
/// A child rather than the span prim, so the scale that stretches the shared
/// prototype to this span's length stays off the span's own frame — and so a
/// collision proxy has somewhere to go beside it. See [`scene`](crate::scene).
pub const STEEL: &str = "Steel";

/// Vertices around a wire. Six: at 3 mm across, one more would be a triangle
/// nobody ever sees the silhouette of.
const SIDES: usize = 6;

/// How far a catch wire stands off the post's face, in meters. A wire is
/// stapled to the outside of a post, not driven through it.
const STANDOFF: f32 = 0.01;

/// How far below the post tops the top pair runs, in meters.
///
/// A hand's width, and deliberately more than [`POLE_SINK`]: a post driven to
/// the full depth still stands above its own top wire rather than under it.
///
/// [`POLE_SINK`]: super::util::planting::POLE_SINK
const TOP_CLEARANCE: f32 = 0.08;

/// Lowest the top wire will be put, in meters. A trellis height of zero is
/// reachable from Python, and the levels have to stack somewhere.
const MIN_TOP: f32 = 0.1;

/// Thinnest wire we will build. A radius of zero is reachable from Python, and
/// a zero-radius cylinder is a mesh of coincident points.
const MIN_RADIUS: f32 = 0.0002;

// ─── Config ─────────────────────────────────────────────────────────

/// One span of wire, post to post.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct WireConfig {
    /// How far it runs, in meters. The prototype is one meter long and every
    /// span is that mesh scaled to this.
    pub length: f32,
}

// ─── Params ─────────────────────────────────────────────────────────

#[derive(Resource, Clone, Debug, PartialEq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct WireParams {
    /// Levels of catch wires above the fruiting wire. Each level is a *pair*,
    /// one wire either side of the post, and the shoots grow up between them —
    /// two pairs is the usual vertical-shoot-positioned trellis.
    pub catch_wires: u32,
    /// Wire radius, in meters. The default is the 3 mm high-tensile steel a
    /// trellis is strung with.
    pub radius: f32,
}

impl Default for WireParams {
    fn default() -> Self {
        Self {
            catch_wires: 2,
            radius: 0.0015,
        }
    }
}

pub fn plugin(app: &mut App) {
    // `WireParams` is not gated on here although the layout reads it:
    // `planting::plant` gates on it and respawns every span when it moves, so
    // a wire edit arrives as a config change in the same frame.
    app.init_resource::<WireParams>().add_systems(
        PreUpdate,
        build
            .in_set(Grow::Poles)
            .after(pole::build)
            .run_if(configs_changed::<WireConfig>),
    );
}

// ─── Layout ─────────────────────────────────────────────────────────

/// Where a post carries its wires, in that post's own frame.
///
/// Local `+X` runs along the row and `+Y` across it, so a pair straddles the
/// post rather than standing one behind the other. Heights are measured from
/// **the ground the post stands on** rather than from the post's own base —
/// see [`row_wires`] for the sink that distinction pays for.
///
/// Bottom-up: index `k` here is the `k` in the `Wire_<panel>_<k>` prim name,
/// so `0` is the fruiting wire.
///
/// [`row_wires`]: super::util::planting
pub fn anchors(params: &WireParams, pole_radius: f32, fruiting: f32, trellis: f32) -> Vec<Vec3> {
    let top = (trellis - TOP_CLEARANCE).max(MIN_TOP);
    // Python can ask for a head above the post tops. The levels collapse onto
    // the top wire rather than running back down the post.
    let fruiting = fruiting.clamp(0.0, top);
    let offset = pole_radius.max(0.0) + STANDOFF;

    let mut stops = vec![Vec3::new(0.0, 0.0, fruiting)];
    for level in 1..=params.catch_wires {
        let z = fruiting + (top - fruiting) * level as f32 / params.catch_wires as f32;
        stops.push(Vec3::new(0.0, offset, z));
        stops.push(Vec3::new(0.0, -offset, z));
    }
    stops
}

// ─── Building ───────────────────────────────────────────────────────

/// Builds the one wire mesh and hangs it, stretched, on every span.
pub fn build(
    mut commands: Commands,
    mut library: Library,
    params: Res<WireParams>,
    spans: Query<(Entity, &WireConfig)>,
) {
    library.clear(PART);
    // A part nothing references must not be authored: the export writes every
    // registered prototype, and an unused one is a prim with no instances.
    if spans.is_empty() {
        return;
    }

    let radius = params.radius.max(MIN_RADIUS);
    let geometry = library.part(
        PART,
        0,
        cylinder_mesh(radius, radius, 1.0, SIDES).to_mesh(),
        surface(),
    );

    for (entity, config) in &spans {
        let mut span = commands.entity(entity);
        // The layer owns everything below a span, so a rebuild replaces what
        // the last one hung there rather than doubling it.
        span.despawn_children();
        span.with_child((
            Name::new(STEEL),
            geometry.clone(),
            // The prototype is a meter long, so the scale *is* the span.
            Transform::from_scale(Vec3::new(1.0, 1.0, config.length)),
        ));
    }
}

/// No per-span shade: a trellis is strung from one reel of wire.
fn surface() -> Surface {
    material::POLE.surface(color::srgb(color::WIRE))
}

// ─── UI ─────────────────────────────────────────────────────────────

pub fn ui() -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            label_small("Catch wires"),
            (
                @FeathersSlider { @min: 0.0, @max: 4.0, @value: 2.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.wire.catch_wires = change.value.round().max(0.0) as u32;
                })
            ),
            label_small("Wire radius"),
            (
                @FeathersSlider { @min: 0.001, @max: 0.004, @value: 0.0015 }
                SliderStep(0.0005)
                SliderPrecision(4)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<Staged>| {
                    params.wire.radius = change.value;
                })
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::VineyardParams;
    use crate::elements::terrain::Ground;
    use crate::elements::util::parcel::VineyardLayout;
    use crate::elements::util::planting::POLE_SINK;
    use crate::elements::util::testing::{self, Organ, named_children, organs};
    use crate::scene::{Prototypes, UsdReference};

    /// The stack a post carries: the fruiting wire on its axis at the head
    /// height, then pairs straddling it, the last a hand below the post top.
    #[test]
    fn anchors_stack_from_the_fruiting_wire_to_below_the_post_top() {
        let (radius, fruiting, trellis) = (0.04, 0.9, 1.8);
        for catch_wires in [0, 1, 2, 4] {
            let stops = anchors(
                &WireParams {
                    catch_wires,
                    ..default()
                },
                radius,
                fruiting,
                trellis,
            );

            assert_eq!(stops.len(), 1 + 2 * catch_wires as usize);
            assert_eq!(
                stops[0],
                Vec3::new(0.0, 0.0, fruiting),
                "the cordons are tied along this one"
            );
            for pair in stops[1..].chunks(2) {
                assert_eq!(pair[0].y, radius + STANDOFF, "clear of the post's face");
                assert_eq!(pair[1].y, -pair[0].y, "a pair straddles the post");
                assert_eq!(pair[0].z, pair[1].z);
            }
            assert!(stops.windows(2).all(|w| w[1].z >= w[0].z), "bottom up");
            if catch_wires > 0 {
                let top = stops.last().unwrap().z;
                assert!((top - (trellis - TOP_CLEARANCE)).abs() < 1e-6, "{top}");
            }
        }
        assert!(
            TOP_CLEARANCE > POLE_SINK as f32,
            "the deepest-driven post still stands above its own top wire"
        );
    }

    /// Python can set any of these to anything. Nothing may come back
    /// inverted, underground or NaN.
    #[test]
    fn anchors_at_the_stops_stay_finite_and_ordered() {
        for (catch_wires, pole_radius, fruiting, trellis) in [
            (0, 0.0, 0.0, 0.0),
            // A head asked for above the post tops.
            (2, 0.04, 3.0, 1.8),
            (50, 1.0, 0.9, 12.0),
        ] {
            let params = WireParams {
                catch_wires,
                ..default()
            };
            let stops = anchors(&params, pole_radius, fruiting, trellis);

            assert_eq!(stops.len(), 1 + 2 * catch_wires as usize);
            assert!(stops.iter().all(|a| a.is_finite() && a.z >= 0.0));
            assert!(stops.windows(2).all(|w| w[1].z >= w[0].z), "{stops:?}");
        }
    }

    /// End to end: every span leaves one post and reaches the next, standing
    /// at the height above the ground its anchor asked for.
    #[test]
    fn every_span_runs_post_to_post_at_its_anchor() {
        let params = VineyardParams::default();
        let stops = anchors(
            &params.wire,
            params.pole.radius,
            params.vine.trunk_height,
            params.parcel.trellis_height,
        );
        let mut app = testing::grown(params);
        let ground = app.world().resource::<Ground>().clone();
        let rows = app.world().resource::<VineyardLayout>().rows.len();

        let posts: Vec<Organ<pole::PoleConfig>> = organs(app.world_mut());
        let spans: Vec<Organ<WireConfig>> = organs(app.world_mut());
        assert!(!spans.is_empty(), "the fixture strung wires");
        // A row of n posts has n-1 panels, and every panel carries every stop.
        assert_eq!(spans.len(), (posts.len() - rows) * stops.len());

        for span in &spans {
            let (row, name) = span.path.rsplit_once('/').unwrap();
            let (panel, level) = name
                .trim_start_matches("Wire_")
                .split_once('_')
                .expect("`Wire_<panel>_<k>`");
            let (panel, level): (usize, usize) = (panel.parse().unwrap(), level.parse().unwrap());
            let reach = span.transform.rotation * Vec3::Z * span.config.length;

            for (at, slot) in [
                (span.position(), panel),
                (span.position() + reach, panel + 1),
            ] {
                let foot = posts
                    .iter()
                    .find(|post| post.path == format!("{row}/Pole_{slot:03}"))
                    .unwrap_or_else(|| panic!("`{name}` hangs on a post that exists"))
                    .position();
                // Against the ground under the *post's* foot, not under the
                // anchor: on a slope those differ by more than the sink this
                // is checking. The post's lean costs under a millimeter.
                let above = at.z - ground.height(foot.x, foot.y);
                assert!(
                    (above - stops[level].z).abs() < 2e-3,
                    "`{name}` ends {above} m over the ground, not {}",
                    stops[level].z
                );
                assert!(
                    at.truncate().distance(foot.truncate()) < stops[level].y.abs() + 0.06,
                    "`{name}` ends away from the post it is stapled to"
                );
            }
        }
    }

    /// One gauge, one mesh: every span in the parcel draws the same prototype,
    /// stretched to its own length rather than built at it.
    #[test]
    fn every_span_draws_the_one_mesh_stretched_to_its_length() {
        let mut app = testing::grown(VineyardParams::default());

        let library = app.world().resource::<Prototypes>();
        assert!(library.get(&format!("{PART}_0")).is_some());
        assert!(library.get(&format!("{PART}_1")).is_none());

        let span = testing::prim(app.world_mut(), &["Planting", "Row_000", "Wire_000_0"])
            .expect("the fruiting wire of the first panel");
        let length = app.world().entity(span).get::<WireConfig>().unwrap().length;
        assert!(length > 0.1, "a span reaches the next post");

        let children = named_children(app.world_mut(), span);
        let names: Vec<&str> = children.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, [STEEL]);

        let steel = app.world().entity(children[0].1);
        assert_eq!(steel.get::<UsdReference>().unwrap().0, format!("{PART}_0"));
        assert_eq!(
            steel.get::<Transform>().unwrap().scale,
            Vec3::new(1.0, 1.0, length),
            "the meter-long prototype, stretched along its own axis"
        );
    }
}
