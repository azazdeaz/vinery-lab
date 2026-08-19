//! Terrain element — the ground surface everything else sits on.
//!
//! Builds a NURBS surface by lofting through cross sections: `detail`
//! sections spaced along Y, each a curve over `detail` control points spread
//! along X at random elevations, then tessellates that surface into a mesh.
//!
//! Also owns the scene root (`/Vineyard`, the stage's default prim), so a
//! consumer referencing the generated layer gets the whole scene. It only
//! *defines* that root — the subtree it rewrites is `/Vineyard/Terrain`, which
//! leaves room for sibling elements to place themselves under `/Vineyard`.

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use curvo::prelude::{NurbsCurve3D, NurbsSurface, SurfaceTessellation3D};
use nalgebra::Point4;
use openusd::schemas::geom::{Xform, Xformable};
use openusd::sdf;
use usd_bevy::authoring::{define_prim, remove_prim};
use usd_bevy::live::LiveStage;

use super::usd::{MeshData, author_mesh, reference_prim};
use super::{Grow, grid};

/// The scene root, and the stage's default prim.
pub const ROOT: &str = "/Vineyard";

/// The subtree this element owns and rewrites from scratch.
pub const TERRAIN: &str = "/Vineyard/Terrain";

const SURFACE: &str = "/Vineyard/Terrain/Surface";
const NESTED_GRID: &str = "/Vineyard/Terrain/Grid";

/// Degree of the lofted surface in both directions, clamped down when there
/// are too few control points to support it.
const DEGREE: usize = 3;

/// Tessellated quads per control-point span. The surface is smooth between
/// control points, so it needs subdividing past them to look like anything.
const TESSELLATION: usize = 4;

/// Fixed seed for the elevation field. Terrain has no seed parameter yet, and
/// the generated stage has to be byte-identical across runs for the same
/// params, so the randomness is deterministic rather than sampled.
const SEED: u64 = 0x5EED_1EAF;

#[derive(Resource, Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct TerrainParams {
    /// Extent along X, in meters.
    pub width: f32,
    /// Extent along Y, in meters. (Not vertical — the stage is Z-up; see
    /// [`max_elevation`](Self::max_elevation) for that.)
    pub height: f32,
    /// Upper bound on control-point elevation, in meters. The surface stays
    /// within its control points' convex hull, so it never exceeds this.
    pub max_elevation: f32,
    /// Resolution of the control-point lattice: how many cross sections to
    /// loft through, and how many control points each one spans.
    pub detail: u32,
}

impl Default for TerrainParams {
    fn default() -> Self {
        Self {
            width: 4.0,
            height: 4.0,
            max_elevation: 0.5,
            detail: 6,
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<TerrainParams>().add_systems(
        PreUpdate,
        author
            .in_set(Grow::Terrain)
            .run_if(resource_changed::<TerrainParams>),
    );
}

fn author(live: NonSend<LiveStage>, params: Res<TerrainParams>) -> Result<()> {
    let stage = &live.stage;
    define_prim(stage, ROOT, "Xform")?;
    stage.set_default_prim("Vineyard")?;

    remove_prim(stage, TERRAIN)?;
    define_prim(stage, TERRAIN, "Xform")?;
    author_mesh(stage, SURFACE, &terrain_mesh(&params)?)?;

    // Temporary: nests the grid element's subtree above the highest possible
    // point of the terrain, to exercise composition through a reference. Goes
    // away with the grid.
    Xform::define(stage, sdf::path(NESTED_GRID)?)?
        .set_translate([0.0, 0.0, params.max_elevation as f64].into())?;
    reference_prim(stage, NESTED_GRID, grid::ROOT)?;
    Ok(())
}

/// Lofts the terrain surface and tessellates it into a mesh.
fn terrain_mesh(params: &TerrainParams) -> anyhow::Result<MeshData> {
    // A curve needs more control points than its degree, and a loft needs at
    // least two sections, so one span is the floor for both directions.
    let count = (params.detail as usize).max(2);
    let degree = DEGREE.min(count - 1);
    let knots = clamped_uniform_knots(count, degree);
    let mut elevations = Elevations::new(SEED, params.max_elevation as f64);

    let sections = (0..count)
        .map(|section| {
            let y = spread(params.height as f64, section, count);
            // Homogeneous control points: curvo's 3D curve carries a weight
            // per point, and an unweighted point is `w = 1`.
            let control_points = (0..count)
                .map(|i| {
                    Point4::new(
                        spread(params.width as f64, i, count),
                        y,
                        elevations.next(),
                        1.0,
                    )
                })
                .collect();
            NurbsCurve3D::try_new(degree, control_points, knots.clone())
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let surface = NurbsSurface::try_loft(&sections, Some(DEGREE))?;
    let divisions = (count - 1) * TESSELLATION;
    Ok(mesh_data(&surface.regular_tessellate(divisions, divisions)))
}

/// Position `i` of `count` evenly spaced across `extent`, centered on 0.
fn spread(extent: f64, i: usize, count: usize) -> f64 {
    extent * (i as f64 / (count - 1) as f64 - 0.5)
}

/// A clamped uniform knot vector for `count` control points of `degree`:
/// `degree + 1` repeats at each end (so the curve reaches its first and last
/// control point) and single interior knots between.
///
/// Every cross section shares this vector, which is what lets the loft
/// combine them without first refining them into a common one.
fn clamped_uniform_knots(count: usize, degree: usize) -> Vec<f64> {
    let spans = (count - degree) as f64;
    std::iter::repeat_n(0.0, degree + 1)
        .chain((1..spans as usize).map(|i| i as f64))
        .chain(std::iter::repeat_n(spans, degree + 1))
        .collect()
}

/// Converts a tessellated surface into USD's mesh layout.
///
/// curvo's triangles already wind counter-clockwise seen from above, which is
/// what USD's default right-handed orientation needs for the ground to face
/// up, so the indices carry over unchanged.
fn mesh_data(tessellation: &SurfaceTessellation3D<f64>) -> MeshData {
    MeshData {
        points: tessellation
            .points()
            .iter()
            .map(|p| [p.x as f32, p.y as f32, p.z as f32])
            .collect(),
        face_vertex_counts: vec![3; tessellation.faces().len()],
        face_vertex_indices: tessellation
            .faces()
            .iter()
            .flat_map(|[a, b, c]| [*a as i32, *b as i32, *c as i32])
            .collect(),
    }
}

/// The random elevation field, as a stream of heights in `0..=max`.
///
/// Uses the same inlined SplitMix64 as the variation picker (see
/// [`super::split_mix_64`]) for the same reason: a fixed algorithm is what
/// makes the scene reproducible across machines and crate versions.
struct Elevations {
    state: u64,
    max: f64,
}

impl Elevations {
    fn new(seed: u64, max: f64) -> Self {
        Self { state: seed, max }
    }

    fn next(&mut self) -> f64 {
        // Top 53 bits, the mantissa width of an f64, for a uniform unit float.
        let unit = (super::split_mix_64(&mut self.state) >> 11) as f64 / (1u64 << 53) as f64;
        unit * self.max
    }
}

pub fn ui() -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            label_small("Terrain width"),
            (
                @FeathersSlider { @min: 0.5, @max: 50.0, @value: 4.0 }
                SliderStep(0.5)
                SliderPrecision(1)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<TerrainParams>| {
                    params.width = change.value;
                })
            ),
            label_small("Terrain height"),
            (
                @FeathersSlider { @min: 0.5, @max: 50.0, @value: 4.0 }
                SliderStep(0.5)
                SliderPrecision(1)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<TerrainParams>| {
                    params.height = change.value;
                })
            ),
            label_small("Max elevation"),
            (
                @FeathersSlider { @min: 0.0, @max: 5.0, @value: 0.5 }
                SliderStep(0.05)
                SliderPrecision(2)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<TerrainParams>| {
                    params.max_elevation = change.value;
                })
            ),
            label_small("Terrain detail"),
            (
                @FeathersSlider { @min: 2.0, @max: 24.0, @value: 6.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<TerrainParams>| {
                    params.detail = change.value.round().max(2.0) as u32;
                })
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::VineyardParams;
    use openusd::schemas::geom::{Mesh, PointBased, PointInstancer};
    use openusd::sdf::Value;

    fn mesh(params: &TerrainParams) -> MeshData {
        terrain_mesh(params).expect("terrain lofts")
    }

    #[test]
    fn detail_drives_the_tessellation_size() {
        let m = mesh(&TerrainParams {
            detail: 4,
            ..default()
        });
        let divisions = (4 - 1) * TESSELLATION;
        assert_eq!(m.points.len(), (divisions + 1) * (divisions + 1));
        assert_eq!(m.face_vertex_counts.len(), divisions * divisions * 2);
        assert_eq!(m.face_vertex_indices.len(), m.face_vertex_counts.len() * 3);
    }

    /// The lowest usable detail must still loft: two sections of two control
    /// points each, which forces the degree down to 1 in both directions.
    #[test]
    fn the_coarsest_terrain_still_builds() {
        assert!(
            !mesh(&TerrainParams {
                detail: 1,
                ..default()
            })
            .points
            .is_empty()
        );
    }

    #[test]
    fn the_surface_spans_the_requested_extent() {
        let params = TerrainParams {
            width: 8.0,
            height: 3.0,
            max_elevation: 1.5,
            detail: 5,
        };
        let m = mesh(&params);
        let bound = |axis: usize| {
            m.points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                (lo.min(p[axis]), hi.max(p[axis]))
            })
        };
        let (x0, x1) = bound(0);
        let (y0, y1) = bound(1);
        let (z0, z1) = bound(2);
        assert!((x1 - x0 - params.width).abs() < 1e-4, "width {x0}..{x1}");
        assert!((y1 - y0 - params.height).abs() < 1e-4, "height {y0}..{y1}");
        assert!(
            z0 >= -1e-4 && z1 <= params.max_elevation + 1e-4,
            "elevation {z0}..{z1} stays within 0..={}",
            params.max_elevation
        );
    }

    /// Every triangle must wind counter-clockwise seen from above, or the
    /// ground renders as a hole under USD's default right-handed orientation.
    #[test]
    fn faces_wind_upward() {
        let m = mesh(&TerrainParams::default());
        for (face, indices) in m.face_vertex_indices.chunks(3).enumerate() {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(m.points[indices[i] as usize]));
            assert!(
                (b - a).cross(c - a).z > 0.0,
                "face {face} normal points up"
            );
        }
    }

    #[test]
    fn authors_the_scene_root_and_its_default_prim() {
        let stage = crate::generate::generate_stage(&VineyardParams::default()).unwrap();
        let usda = stage.root_layer().export_to_string().unwrap();

        assert!(usda.contains("defaultPrim = \"Vineyard\""), "got:\n{usda}");
        assert!(
            matches!(
                Mesh::get(&stage, sdf::path(SURFACE).unwrap())
                    .unwrap()
                    .expect("terrain mesh authored")
                    .points_attr()
                    .get::<Value>()
                    .unwrap(),
                Some(Value::Vec3fVec(p)) if !p.is_empty()
            ),
            "the terrain mesh has points"
        );
    }

    /// The nested grid is composed in by reference, not copied — so the
    /// instancer authored under the grid's own subtree has to show up under
    /// the terrain, still pointing at the cube prototypes it was authored
    /// against (those live outside the referenced subtree, so composition has
    /// to leave their paths alone).
    #[test]
    fn nests_the_grid_by_reference() {
        let stage = crate::generate::generate_stage(&VineyardParams::default()).unwrap();
        let path = sdf::path(format!("{NESTED_GRID}/Cubes")).unwrap();
        let instancer = PointInstancer::get(&stage, path)
            .unwrap()
            .expect("grid subtree composes in under the terrain");
        assert!(
            instancer
                .prototypes_rel()
                .targets()
                .unwrap()
                .iter()
                .all(|p| p.as_str().starts_with(crate::elements::cube::PROTOTYPE)),
            "prototype targets survive the reference"
        );
    }

    /// Editing the grid must show through the reference without the terrain
    /// re-authoring anything: the viewer only re-runs the element whose params
    /// changed, so a stale composition here would freeze the nested subtree.
    #[test]
    fn grid_edits_show_through_the_reference() {
        let stage = crate::stage::new_stage("live.usda").unwrap();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(crate::elements::plugin);
        app.world_mut().insert_non_send(LiveStage::new(stage.clone()));
        app.finish();
        app.cleanup();
        app.update();

        let instances = || {
            let path = sdf::path(format!("{NESTED_GRID}/Cubes")).unwrap();
            match PointInstancer::get(&stage, path)
                .unwrap()
                .expect("composed instancer")
                .positions_attr()
                .get::<Value>()
                .unwrap()
            {
                Some(Value::Vec3fVec(positions)) => positions.len(),
                other => panic!("positions not authored: {other:?}"),
            }
        };
        let before = instances();

        app.world_mut()
            .resource_mut::<crate::elements::grid::GridParams>()
            .rows = 3;
        app.update();

        assert_ne!(instances(), before, "the re-authored grid composes through");
    }

    /// Re-authoring the terrain must not disturb the sibling subtrees under
    /// `/Vineyard`: the element defines the scene root but only owns
    /// `/Vineyard/Terrain`.
    #[test]
    fn re_authoring_keeps_the_scene_root_intact() {
        let stage = crate::stage::new_stage("terrain.usda").unwrap();
        define_prim(&stage, "/Vineyard/Sibling", "Xform").unwrap();

        let mut world = World::new();
        world.insert_non_send(LiveStage::new(stage.clone()));
        world.insert_resource(TerrainParams::default());
        let mut schedule = Schedule::default();
        schedule.add_systems(author);
        schedule.run(&mut world);
        schedule.run(&mut world);

        assert!(usd_bevy::authoring::prim_exists(
            &stage,
            "/Vineyard/Sibling"
        ));
        assert!(usd_bevy::authoring::prim_exists(&stage, SURFACE));
    }
}
