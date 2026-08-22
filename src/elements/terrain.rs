//! Terrain element — the ground surface everything else sits on.
//!
//! Builds a NURBS surface by lofting through cross sections: `detail`
//! sections spaced along Y, each a curve over `detail` control points spread
//! along X at random elevations, then tessellates that surface into a mesh.
//! The same tessellation also feeds the [`Ground`] resource, a height-field
//! sampler that [`parcel`](super::parcel) uses to drape row layouts onto the
//! surface.
//!
//! The scene root `/Vineyard` is not this element's to define —
//! [`crate::stage::new_stage`] authors it, along with the default prim it
//! becomes. This element only rewrites subtrees beneath it, which leaves room
//! for sibling elements to place themselves under `/Vineyard` too.
//!
//! # Two subtrees
//!
//! Terrain also owns `/Vineyard/Planting`, through
//! [`planting`](super::util::planting) — everything standing *on* the ground,
//! placed against the rows [`parcel`](super::util::parcel) solves. Both
//! helpers are wired from this element's [`plugin`] rather than given
//! [`Grow`] slots of their own, because both need the terrain's extent and
//! the [`Ground`] field, and chaining them here guarantees the ordering that
//! system-ordering-across-elements would only imply.

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, ValueChange, slider_self_update};
use curvo::prelude::{NurbsSurface, SurfaceTessellation3D};
use nalgebra::Point4;
use usd_bevy::authoring::{define_prim, remove_prim};
use usd_bevy::live::LiveStage;

use super::Grow;
use super::util::usd::{MeshData, author_mesh, set_display_color};
use super::util::{color, material, parcel, planting};

/// The subtree this element owns and rewrites from scratch.
pub const TERRAIN: &str = "/Vineyard/Terrain";

const SURFACE: &str = "/Vineyard/Terrain/Surface";

/// Where this element keeps its material. Inside [`TERRAIN`] rather than in a
/// scene-wide `/Vineyard/Looks`, so that `remove_prim(TERRAIN)` still rewrites
/// the whole subtree in one call — and for the same reason every other
/// material is namespaced under what it shades, which
/// [`material`](super::util::material) explains at length.
const LOOKS: &str = "/Vineyard/Terrain/Looks";

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
            width: 80.0,
            height: 50.0,
            max_elevation: 3.0,
            detail: 6,
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<TerrainParams>()
        .init_resource::<Ground>()
        .init_resource::<parcel::ParcelParams>()
        .init_resource::<parcel::VineyardLayout>()
        .init_resource::<planting::PlantingParams>()
        .add_systems(
            PreUpdate,
            (
                author.run_if(resource_changed::<TerrainParams>),
                parcel::author.run_if(
                    resource_changed::<parcel::ParcelParams>.or_else(resource_changed::<Ground>),
                ),
            )
                .chain()
                .in_set(Grow::Terrain),
        )
        // `VineParams` is a coarse trigger: planting reads no vine params, but
        // it does count the prototypes on the stage, and changing
        // `variations` is what changes that count. When a second element
        // needs planting, replace this with a registry the prototype authors
        // push into.
        .add_systems(
            PreUpdate,
            planting::author.in_set(Grow::Plants).run_if(
                resource_changed::<planting::PlantingParams>
                    .or_else(resource_changed::<parcel::VineyardLayout>)
                    .or_else(resource_changed::<super::vine::VineParams>)
                    .or_else(resource_changed::<super::util::place::Style>),
            ),
        );
}

fn author(
    live: NonSend<LiveStage>,
    params: Res<TerrainParams>,
    mut ground: ResMut<Ground>,
) -> Result<()> {
    let stage = &live.stage;
    remove_prim(stage, TERRAIN)?;
    define_prim(stage, TERRAIN, "Xform")?;
    define_prim(stage, LOOKS, "Scope")?;
    let ground_material = format!("{LOOKS}/Ground");
    material::author_preview_material(stage, &ground_material, material::GROUND)?;

    let (tessellation, divisions) = terrain_tessellation(&params)?;
    let surface = author_mesh(stage, SURFACE, &mesh_data(&tessellation))?;
    // Unjittered: there is one ground, so there is nothing for a per-variation
    // drift to tell apart.
    set_display_color(&surface, color::srgb(color::GROUND))?;
    material::bind_material(stage, SURFACE, &ground_material)?;

    *ground = Ground::from_tessellation(&tessellation, divisions);
    Ok(())
}

/// Builds the terrain surface and tessellates it, returning the tessellation
/// alongside the (square) division count used to build it — [`Ground`] needs
/// that count to index back into the point grid.
///
/// Built directly via [`NurbsSurface::new`] with one shared, explicit knot
/// vector in both directions, rather than [`NurbsSurface::try_loft`]'s
/// per-column chord-length interpolation: `try_loft` derives each column's
/// fit from the Euclidean distance between its (randomly elevated) points,
/// so with elevation baked into the lofted curves themselves, different
/// columns end up fit against slightly different implicit parametrizations
/// even though they share one knot vector on the resulting surface — which
/// makes the surface's `x`/`y` only *approximately* separable, undermining
/// the whole point of [`Ground`]. Every control point's x depends only on
/// its column index and y only on its row index here (both spread evenly by
/// construction), so building the grid with one explicit shared knot vector
/// keeps x and y exactly separable regardless of the random elevation —
/// see `the_tessellation_grid_is_rectilinear_in_xy` below.
fn terrain_tessellation(
    params: &TerrainParams,
) -> anyhow::Result<(SurfaceTessellation3D<f64>, usize)> {
    // A curve needs more control points than its degree, and a surface
    // needs at least two rows/columns, so one span is the floor for both
    // directions.
    let count = (params.detail as usize).max(2);
    let degree = DEGREE.min(count - 1);
    let knots = clamped_uniform_knots(count, degree);
    let mut elevations = Elevations::new(SEED, params.max_elevation as f64);
    // Drawn row-by-row (`j` outer, `i` inner) so reseeding keeps assigning
    // the same draw to the same (i, j) grid cell regardless of how the grid
    // is later built up.
    let heights: Vec<f64> = (0..count * count).map(|_| elevations.next()).collect();

    // control_points[i][j]: column `i` spread along x, row `j` spread along
    // y — matching curvo's own `[u][v]` control-point layout, so this slots
    // straight into `NurbsSurface::new` without curvo needing to infer
    // anything about the grid's shape.
    let control_points: Vec<Vec<_>> = (0..count)
        .map(|i| {
            let x = spread(params.width as f64, i, count);
            (0..count)
                .map(|j| {
                    let y = spread(params.height as f64, j, count);
                    Point4::new(x, y, heights[j * count + i], 1.0)
                })
                .collect()
        })
        .collect();

    let surface = NurbsSurface::new(degree, degree, knots.clone(), knots, control_points);
    let divisions = (count - 1) * TESSELLATION;
    Ok((surface.regular_tessellate(divisions, divisions), divisions))
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

/// The terrain's height field, sampled on a rectilinear XY grid.
///
/// `regular_tessellate` lays its points out u-major (`points[iu * m + iv]`),
/// and because every control point's x comes from [`spread`] over `i` alone
/// and its y from the section index alone, the loft's x depends only on u
/// and its y only on v — so the tessellated grid is rectilinear in XY even
/// though the surface itself is a general NURBS loft. That turns height
/// lookup into two binary searches and a bilinear blend instead of a NURBS
/// parameter inversion, and — since it samples the same grid the mesh is
/// built from — it agrees exactly with the collision geometry at the grid
/// points.
///
/// Rows are drawn in plan view and lifted onto this field afterwards, rather
/// than following the surface's own parameterization: see
/// [`parcel`](super::parcel) for where that happens.
#[derive(Resource, Clone, Debug, Default)]
pub struct Ground {
    /// Strictly increasing x coordinates of the grid columns.
    xs: Vec<f32>,
    /// Strictly increasing y coordinates of the grid rows.
    ys: Vec<f32>,
    /// Heights, indexed `[ix * ys.len() + iy]`.
    heights: Vec<f32>,
}

impl Ground {
    /// Builds the sampler from a tessellation known to have `divs + 1`
    /// points along each parametric direction, laid out u-major.
    fn from_tessellation(tessellation: &SurfaceTessellation3D<f64>, divs: usize) -> Self {
        let n = divs + 1;
        let points = tessellation.points();
        if points.len() != n * n {
            // Defensive: a mismatch here means the u/v layout assumption
            // above no longer holds, and any grid built from it would just
            // be wrong. Fall back to "no terrain" rather than sample garbage.
            return Self::default();
        }
        Self {
            xs: (0..n).map(|iu| points[iu * n].x as f32).collect(),
            ys: (0..n).map(|iv| points[iv].y as f32).collect(),
            heights: points.iter().map(|p| p.z as f32).collect(),
        }
    }

    /// Height at `(x, y)`, bilinearly interpolated and clamped to the grid
    /// at the edges. `0.0` if the grid hasn't been built yet.
    pub fn height(&self, x: f32, y: f32) -> f32 {
        if self.xs.is_empty() || self.ys.is_empty() {
            return 0.0;
        }
        let (ix0, ix1, tx) = segment(&self.xs, x);
        let (iy0, iy1, ty) = segment(&self.ys, y);
        let m = self.ys.len();
        let at = |ix: usize, iy: usize| self.heights[ix * m + iy];
        let h0 = at(ix0, iy0).lerp(at(ix0, iy1), ty);
        let h1 = at(ix1, iy0).lerp(at(ix1, iy1), ty);
        h0.lerp(h1, tx)
    }

    /// `p`, lifted from the XY ground plane onto this height field.
    pub fn lift(&self, p: Vec2) -> Vec3 {
        p.extend(self.height(p.x, p.y))
    }
}

/// Locates `v` within `axis`, returning the bracketing indices and the
/// interpolation parameter between them (`0.0` at the lower index, `1.0` at
/// the upper). Clamps `v` to the axis's extent, and degenerates to a single
/// index pair when the axis has fewer than two entries.
fn segment(axis: &[f32], v: f32) -> (usize, usize, f32) {
    let n = axis.len();
    if n < 2 {
        return (0, 0, 0.0);
    }
    if v <= axis[0] {
        return (0, 1, 0.0);
    }
    if v >= axis[n - 1] {
        return (n - 2, n - 1, 1.0);
    }
    let i = axis
        .partition_point(|&a| a <= v)
        .saturating_sub(1)
        .min(n - 2);
    let t = (v - axis[i]) / (axis[i + 1] - axis[i]);
    (i, i + 1, t)
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
                @FeathersSlider { @min: 5.0, @max: 200.0, @value: 80.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<TerrainParams>| {
                    params.width = change.value;
                })
            ),
            label_small("Terrain height"),
            (
                @FeathersSlider { @min: 5.0, @max: 200.0, @value: 50.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<TerrainParams>| {
                    params.height = change.value;
                })
            ),
            label_small("Max elevation"),
            (
                @FeathersSlider { @min: 0.0, @max: 15.0, @value: 3.0 }
                SliderStep(0.1)
                SliderPrecision(1)
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
    use crate::elements::util::testing::{Authoring, bounds, face_normal, faces};
    use openusd::schemas::geom::{Mesh, PointBased};
    use openusd::sdf::{self, Value};

    fn mesh(params: &TerrainParams) -> MeshData {
        let (tessellation, _) = terrain_tessellation(params).expect("terrain lofts");
        mesh_data(&tessellation)
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
        let (x0, x1) = bounds(&m, 0);
        let (y0, y1) = bounds(&m, 1);
        let (z0, z1) = bounds(&m, 2);
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
        for (i, face) in faces(&m).enumerate() {
            assert!(face_normal(&m, face).z > 0.0, "face {i} normal points up");
        }
    }

    /// The rectilinear-grid assumption `Ground` relies on: every point in a
    /// tessellation column shares an x, and every point in a row shares a y.
    /// If curvo ever laid the loft's u/v out the other way round, this is
    /// the test that would catch it — `Ground::from_tessellation` would
    /// otherwise silently sample a transposed field.
    #[test]
    fn the_tessellation_grid_is_rectilinear_in_xy() {
        let (tessellation, divisions) = terrain_tessellation(&TerrainParams {
            width: 8.0,
            height: 3.0,
            max_elevation: 1.5,
            detail: 5,
        })
        .unwrap();
        let n = divisions + 1;
        let points = tessellation.points();
        assert_eq!(points.len(), n * n);

        for iu in 0..n {
            let x0 = points[iu * n].x;
            for iv in 1..n {
                assert!(
                    (points[iu * n + iv].x - x0).abs() < 1e-9,
                    "column {iu} shares one x"
                );
            }
        }
        for iv in 0..n {
            let y0 = points[iv].y;
            for iu in 1..n {
                assert!(
                    (points[iu * n + iv].y - y0).abs() < 1e-9,
                    "row {iv} shares one y"
                );
            }
        }
    }

    #[test]
    fn ground_height_matches_the_mesh_at_grid_points() {
        let params = TerrainParams {
            width: 8.0,
            height: 3.0,
            max_elevation: 1.5,
            detail: 5,
        };
        let (tessellation, divisions) = terrain_tessellation(&params).unwrap();
        let ground = Ground::from_tessellation(&tessellation, divisions);

        for point in tessellation.points() {
            let (x, y, z) = (point.x as f32, point.y as f32, point.z as f32);
            assert!(
                (ground.height(x, y) - z).abs() < 1e-4,
                "grid point ({x}, {y}) samples back to its own height"
            );
        }
    }

    /// Exercises the bilinear math directly against a hand-built grid,
    /// rather than a real terrain's — real elevation is independently random
    /// per control point, so nothing says a real height field is anywhere
    /// near linear between two neighbouring grid samples, which is exactly
    /// what a midpoint check like this needs to hold.
    #[test]
    fn ground_height_interpolates_between_grid_points() {
        let ground = Ground {
            xs: vec![0.0, 1.0],
            ys: vec![0.0, 1.0],
            // (0,0) -> 0.0, (1,0) -> 2.0, (0,1) -> 4.0, (1,1) -> 6.0
            heights: vec![0.0, 4.0, 2.0, 6.0],
        };

        assert!((ground.height(0.0, 0.0) - 0.0).abs() < 1e-6);
        assert!((ground.height(1.0, 0.0) - 2.0).abs() < 1e-6);
        assert!((ground.height(0.0, 1.0) - 4.0).abs() < 1e-6);
        assert!((ground.height(1.0, 1.0) - 6.0).abs() < 1e-6);
        // The center of a bilinear patch is the average of its four corners.
        assert!((ground.height(0.5, 0.5) - 3.0).abs() < 1e-6);
        // Halfway along one edge is the average of that edge's two corners.
        assert!((ground.height(0.5, 0.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn ground_height_clamps_outside_the_grid() {
        let params = TerrainParams::default();
        let (tessellation, divisions) = terrain_tessellation(&params).unwrap();
        let ground = Ground::from_tessellation(&tessellation, divisions);

        let far_below = ground.height(-1e6, -1e6);
        let corner = ground.height(
            (-(params.width as f64) / 2.0) as f32,
            (-(params.height as f64) / 2.0) as f32,
        );
        assert!((far_below - corner).abs() < 1e-3, "clamps to the near edge");
    }

    #[test]
    fn an_unbuilt_ground_samples_to_zero() {
        assert_eq!(Ground::default().height(1.0, 2.0), 0.0);
    }

    #[test]
    fn authors_the_terrain_surface_under_the_scene_root() {
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

    /// Re-authoring the terrain must not disturb the sibling subtrees under
    /// `/Vineyard`: the element owns `/Vineyard/Terrain`, not the root itself.
    #[test]
    fn re_authoring_keeps_the_scene_root_intact() {
        let mut authoring = Authoring::new("terrain.usda", author);
        define_prim(&authoring.stage, "/Vineyard/Sibling", "Xform").unwrap();
        authoring
            .insert(TerrainParams::default())
            .insert(Ground::default())
            .run()
            .run();

        assert!(authoring.has("/Vineyard/Sibling"));
        assert!(authoring.has(SURFACE));
    }
}
