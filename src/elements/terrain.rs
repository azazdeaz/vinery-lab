//! Terrain element — the ground surface everything else sits on.
//!
//! The ground is a Perlin noise field sampled on a regular XY grid. The wave
//! has a size in meters (`feature_size`) and the field is anchored in world
//! space, so growing the ground uncovers more of the same landscape instead
//! of stretching one undulation across it, and the amplitude solved from
//! `max_inclination` holds the steepness at any size. That one grid is both
//! the mesh handed to the exporter and the [`Ground`] resource, the
//! height-field sampler [`parcel`](super::parcel) uses to drape row layouts
//! onto the surface.
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
use crate::scene::doc::TRIANGLE_MESH;
use crate::scene::{Library, PrimRoot};

use super::Grow;
use super::util::mesh::MeshData;
use super::util::{color, material, parcel, planting};

/// The prim this element owns under the scene root.
pub const TERRAIN: &str = "Terrain";

/// The mesh-library prefix this element registers its geometry under.
const PART: &str = "Terrain";

/// Marks the terrain entity, so a rebuild can drop the one before it.
#[derive(Component)]
pub struct Terrain;

/// Fixed seed for the noise field. Terrain has no seed parameter yet, and
/// the generated stage has to be byte-identical across runs for the same
/// params, so the randomness is deterministic rather than sampled.
const SEED: u64 = 0x5EED_1EAF;

/// The largest gradient magnitude [`perlin`] attains, per unit of its input.
/// Dividing by it turns a requested inclination into an elevation amplitude.
///
/// Measured over the field, not derived: the analytic bound is three times
/// looser, and honouring it would flatten the ground to guard against
/// gradient alignments that random angles never produce.
/// `the_noise_stays_under_its_slope_bound` re-measures it.
const MAX_NOISE_SLOPE: f64 = 2.5;

/// Floor on `feature_size`, so nothing divides by a slider left at zero.
const MIN_FEATURE_SIZE: f64 = 0.5;

/// Cap on grid samples per axis. Fine detail over a large field would
/// otherwise build a mesh too heavy to rebuild while a slider is dragged.
const MAX_SAMPLES: usize = 256;

#[derive(Resource, Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct TerrainParams {
    /// Extent along X, in meters. Rows run along it at orientation 0.
    pub length: f32,
    /// Extent along Y, in meters.
    pub width: f32,
    /// Upper bound on the ground's slope, in degrees. The elevation amplitude
    /// is solved from this and `feature_size`, so the same value gives the
    /// same steepness whatever the field's extent or resolution.
    pub max_inclination: f32,
    /// Distance from one hill to the next, in meters. The noise field is
    /// anchored in world space at this size, so changing the extent uncovers
    /// more or less of the same landscape rather than rescaling it.
    pub feature_size: f32,
    /// Grid samples per feature — how finely the mesh follows the noise. The
    /// grid steps `feature_size / detail` meters, capped at [`MAX_SAMPLES`]
    /// samples per axis.
    pub detail: u32,
}

impl Default for TerrainParams {
    fn default() -> Self {
        Self {
            length: 80.0,
            width: 50.0,
            max_inclination: 20.0,
            feature_size: 16.0,
            detail: 8,
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
                build.run_if(resource_changed::<TerrainParams>),
                parcel::author.run_if(
                    resource_changed::<parcel::ParcelParams>.or_else(resource_changed::<Ground>),
                ),
            )
                .chain()
                .in_set(Grow::Terrain),
        )
        // Planting authors every plant's and post's config, so it re-runs
        // whenever the layout moves or any of the params those configs are
        // built from change.
        .add_systems(
            PreUpdate,
            planting::plant.in_set(Grow::Planting).run_if(
                resource_changed::<planting::PlantingParams>
                    .or_else(resource_changed::<parcel::VineyardLayout>)
                    .or_else(resource_changed::<parcel::ParcelParams>)
                    .or_else(resource_changed::<super::vine::VineParams>)
                    .or_else(resource_changed::<super::pole::PoleParams>),
            ),
        );
}

/// Builds the ground surface and publishes the height field under it.
///
/// The one layer with nothing to quantize: there is a single ground, so it is
/// its own single representative and the mesh library gets exactly one entry
/// from here. It still goes through [`Prototypes`] rather than inlining its
/// geometry, so that every element reaches the export by the same route.
pub(crate) fn build(
    mut commands: Commands,
    mut library: Library,
    params: Res<TerrainParams>,
    root: Res<PrimRoot>,
    mut ground: ResMut<Ground>,
    existing: Query<Entity, With<Terrain>>,
) -> Result<()> {
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    library.clear(PART);

    let field = terrain_grid(&params);
    let geometry = library.part(
        PART,
        0,
        mesh_data(&field).to_mesh(),
        // Unjittered: there is one ground, so nothing for a per-mesh drift to
        // tell apart.
        material::GROUND.surface(color::srgb(color::GROUND)),
    );

    // The ground is the one part whose mesh is also what a robot stands on, so
    // it collides as itself rather than through a proxy: exact triangles,
    // which is legal because nothing in the scene is a rigid body.
    library.collide(&geometry, TRIANGLE_MESH);
    // The ground is also a height field in xy, and says so: a backend that
    // collides one rasterizes the mesh at its own grid spacing rather than
    // approximating it -- MuJoCo collides a mesh as its convex hull, which
    // turns every hollow in the ground into a lid over it.
    library.heightfield(&geometry, field.finest_spacing());

    commands.spawn((Terrain, Name::new(TERRAIN), geometry, ChildOf(root.0)));

    *ground = field;
    Ok(())
}

/// Samples the noise field over the parcel's extent: the grid that is both
/// the ground mesh and the height-field sampler.
fn terrain_grid(params: &TerrainParams) -> Ground {
    let feature = (params.feature_size as f64).max(MIN_FEATURE_SIZE);
    let spacing = feature / params.detail.max(1) as f64;
    let xs = axis(params.length as f64, spacing);
    let ys = axis(params.width as f64, spacing);

    // The noise field's own slope is at most `MAX_NOISE_SLOPE` per unit of
    // input, so stretched over `feature` meters an amplitude `a` tilts the
    // ground by at most `a * MAX_NOISE_SLOPE / feature`. Solving that for the
    // requested angle makes steepness a property of the wave rather than of
    // how much ground there is -- which is the point of sampling a field
    // anchored in world space instead of fitting one to the extent.
    let slope = params.max_inclination.clamp(0.0, 89.0).to_radians().tan() as f64;
    let amplitude = slope * feature / MAX_NOISE_SLOPE;

    let heights = xs
        .iter()
        .flat_map(|&x| {
            ys.iter().map(move |&y| {
                (amplitude * perlin(x as f64 / feature, y as f64 / feature)) as f32
            })
        })
        .collect();
    Ground { xs, ys, heights }
}

/// Sample coordinates across `extent`, centered on 0 and about `spacing`
/// apart.
///
/// The ends land exactly on the extent, so the spacing is rounded to whole
/// spans: one at least, and never more than [`MAX_SAMPLES`] allows.
fn axis(extent: f64, spacing: f64) -> Vec<f32> {
    let spans = ((extent / spacing).round() as usize).clamp(1, MAX_SAMPLES - 1);
    (0..=spans)
        .map(|i| (extent * (i as f64 / spans as f64 - 0.5)) as f32)
        .collect()
}

/// The grid as one quad per cell, in USD's mesh layout.
///
/// Corners are wound counter-clockwise seen from above, which is what USD's
/// default right-handed orientation needs for the ground to face up.
fn mesh_data(ground: &Ground) -> MeshData {
    let (nx, ny) = (ground.xs.len(), ground.ys.len());
    let points = (0..nx)
        .flat_map(|ix| (0..ny).map(move |iy| (ix, iy)))
        .map(|(ix, iy)| [ground.xs[ix], ground.ys[iy], ground.heights[ix * ny + iy]])
        .collect();

    let mut face_vertex_indices = Vec::with_capacity((nx - 1) * (ny - 1) * 4);
    for ix in 0..nx - 1 {
        for iy in 0..ny - 1 {
            let (a, b) = ((ix * ny + iy) as i32, ((ix + 1) * ny + iy) as i32);
            face_vertex_indices.extend([a, b, b + 1, a + 1]);
        }
    }

    MeshData {
        points,
        face_vertex_counts: vec![4; face_vertex_indices.len() / 4],
        face_vertex_indices,
    }
}

/// The terrain's height field, sampled on a rectilinear XY grid.
///
/// The same grid the ground mesh is built from, so height lookup is two
/// binary searches and a bilinear blend, and it agrees exactly with the
/// collision geometry at the grid points.
///
/// Rows are drawn in plan view and lifted onto this field afterwards, rather
/// than following the ground as they go: see [`parcel`](super::parcel) for
/// where that happens.
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
    /// The narrowest gap between adjacent grid lines, in meters. `0.0` if the
    /// grid hasn't been built yet.
    ///
    /// Each axis rounds its own spacing to whole spans, so this is not simply
    /// `feature_size / detail`. Resampling the field -- which is what
    /// [`Library::heightfield`](crate::scene::Library::heightfield) has a
    /// consumer do -- has to match the narrower of the two to resolve every
    /// span the mesh carries.
    pub fn finest_spacing(&self) -> f32 {
        if self.xs.len() < 2 || self.ys.len() < 2 {
            return 0.0;
        }
        let narrowest = |axis: &[f32]| {
            axis.windows(2)
                .map(|pair| pair[1] - pair[0])
                .fold(f32::INFINITY, f32::min)
        };
        narrowest(&self.xs).min(narrowest(&self.ys))
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

/// The unit vector at lattice corner `(ix, iy)`, at an angle hashed from the
/// corner's coordinates.
fn gradient(ix: i64, iy: i64) -> (f64, f64) {
    // Odd multipliers, so the two coordinates cannot cancel each other out.
    let mut state = SEED
        ^ (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (iy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    // Top 53 bits, the mantissa width of an f64, for a uniform unit float.
    let unit = (super::split_mix_64(&mut state) >> 11) as f64 / (1u64 << 53) as f64;
    let angle = unit * std::f64::consts::TAU;
    (angle.cos(), angle.sin())
}

/// Perlin gradient noise over the unit lattice, in about `-0.7..=0.7`.
///
/// Written out rather than pulled from a crate, for the same reason the
/// variation picker inlines SplitMix64 (see [`super::split_mix_64`]): the
/// generated stage has to be byte-identical across machines and across
/// dependency updates, and a noise crate is free to change what it returns
/// between versions.
fn perlin(x: f64, y: f64) -> f64 {
    let (cx, cy) = (x.floor(), y.floor());
    let (fx, fy) = (x - cx, y - cy);
    // Each corner's gradient, dotted with the offset from that corner.
    let corner = |dx: i64, dy: i64| {
        let (gx, gy) = gradient(cx as i64 + dx, cy as i64 + dy);
        gx * (fx - dx as f64) + gy * (fy - dy as f64)
    };
    // Quintic fade: zero first and second derivatives at the lattice lines, so
    // neighbouring cells meet without a visible crease.
    let fade = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let lerp = |a: f64, b: f64, t: f64| a + t * (b - a);
    let (u, v) = (fade(fx), fade(fy));
    lerp(
        lerp(corner(0, 0), corner(1, 0), u),
        lerp(corner(0, 1), corner(1, 1), u),
        v,
    )
}

pub fn ui() -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Column, row_gap: px(4) }
        Children [
            label_small("Terrain length"),
            (
                @FeathersSlider { @min: 5.0, @max: 200.0, @value: 80.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<TerrainParams>| {
                    params.length = change.value;
                })
            ),
            label_small("Terrain width"),
            (
                @FeathersSlider { @min: 5.0, @max: 200.0, @value: 50.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<TerrainParams>| {
                    params.width = change.value;
                })
            ),
            label_small("Max inclination (deg)"),
            (
                @FeathersSlider { @min: 0.0, @max: 45.0, @value: 20.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<TerrainParams>| {
                    params.max_inclination = change.value;
                })
            ),
            label_small("Feature size (m)"),
            (
                @FeathersSlider { @min: 2.0, @max: 60.0, @value: 16.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<TerrainParams>| {
                    params.feature_size = change.value;
                })
            ),
            label_small("Terrain detail"),
            (
                @FeathersSlider { @min: 2.0, @max: 16.0, @value: 8.0 }
                SliderStep(1.0)
                SliderPrecision(0)
                on(slider_self_update)
                on(|change: On<ValueChange<f32>>, mut params: ResMut<TerrainParams>| {
                    params.detail = change.value.round().max(1.0) as u32;
                })
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::util::testing::{bounds, face_normal, faces, scene_app};
    use crate::scene::doc::SceneDoc;
    use crate::scene::export::scene_doc;

    /// Builds the terrain once, and hands back the app together with what the
    /// export would make of it.
    fn built(params: TerrainParams) -> (App, SceneDoc) {
        let mut app = scene_app();
        app.insert_resource(params)
            .init_resource::<Ground>()
            .add_systems(Update, build);
        app.update();
        let doc = scene_doc(app.world_mut()).unwrap();
        (app, doc)
    }

    fn mesh(params: &TerrainParams) -> MeshData {
        mesh_data(&terrain_grid(params))
    }

    /// The grid steps `feature_size / detail` meters, and carries one quad
    /// per cell.
    #[test]
    fn the_grid_steps_feature_size_over_detail() {
        let params = TerrainParams {
            length: 40.0,
            width: 20.0,
            feature_size: 8.0,
            detail: 4,
            ..default()
        };
        let ground = terrain_grid(&params);
        // 2 m steps: 20 spans across 40 m, 10 across 20 m.
        assert_eq!((ground.xs.len(), ground.ys.len()), (21, 11));
        assert!((ground.finest_spacing() - 2.0).abs() < 1e-4);

        let m = mesh(&params);
        assert_eq!(m.points.len(), 21 * 11);
        assert_eq!(m.face_vertex_counts.len(), 20 * 10);
        assert_eq!(m.face_vertex_indices.len(), m.face_vertex_counts.len() * 4);
    }

    /// Neither end of a slider may divide by zero or build a mesh nothing can
    /// carry: a feature size and a detail of zero both floor, and a fine grid
    /// over a large field stops at the sample cap.
    #[test]
    fn the_grid_survives_both_extremes() {
        assert!(
            !mesh(&TerrainParams {
                feature_size: 0.0,
                detail: 0,
                ..default()
            })
            .points
            .is_empty()
        );

        let dense = terrain_grid(&TerrainParams {
            length: 200.0,
            feature_size: 0.5,
            detail: 16,
            ..default()
        });
        assert_eq!(dense.xs.len(), MAX_SAMPLES);
    }

    #[test]
    fn the_surface_spans_the_requested_extent() {
        let params = TerrainParams {
            length: 8.0,
            width: 3.0,
            feature_size: 4.0,
            ..default()
        };
        let m = mesh(&params);
        let (x0, x1) = bounds(&m, 0);
        let (y0, y1) = bounds(&m, 1);
        assert!((x1 - x0 - params.length).abs() < 1e-4, "length {x0}..{x1}");
        assert!((y1 - y0 - params.width).abs() < 1e-4, "width {y0}..{y1}");
    }

    /// The steepest slope between two adjacent grid samples.
    fn steepest_slope(ground: &Ground) -> f64 {
        let (nx, ny) = (ground.xs.len(), ground.ys.len());
        let height = |ix: usize, iy: usize| ground.heights[ix * ny + iy] as f64;
        let mut steepest = 0.0_f64;
        for ix in 0..nx {
            for iy in 0..ny {
                let steps = [
                    (ix + 1 < nx).then(|| {
                        let run = (ground.xs[ix + 1] - ground.xs[ix]) as f64;
                        (height(ix + 1, iy) - height(ix, iy), run)
                    }),
                    (iy + 1 < ny).then(|| {
                        let run = (ground.ys[iy + 1] - ground.ys[iy]) as f64;
                        (height(ix, iy + 1) - height(ix, iy), run)
                    }),
                ];
                for (rise, run) in steps.into_iter().flatten() {
                    steepest = steepest.max(rise.abs() / run);
                }
            }
        }
        steepest
    }

    /// Slope is a property of the ground, not of how much of it there is.
    /// Elevation used to be capped in meters over a lattice fitted to the
    /// extent, which made the same cap on a smaller field mean steeper
    /// ground; the cap must now hold at any size.
    #[test]
    fn inclination_bounds_the_slope_at_any_field_size() {
        let steepest = |length, width| {
            steepest_slope(&terrain_grid(&TerrainParams {
                length,
                width,
                max_inclination: 20.0,
                ..default()
            }))
        };

        let (large, small) = (steepest(80.0, 50.0), steepest(8.0, 5.0));
        let cap = 20.0_f64.to_radians().tan();
        assert!(small > 0.0, "20 degrees is not flat ground: {small}");
        assert!(large <= cap && small <= cap, "{large} and {small} under {cap}");
    }

    /// The undulation is anchored in the world rather than fitted to the
    /// field: two grounds differing only in extent agree wherever they
    /// overlap. Stretching one wave across the whole field is what made a
    /// smaller ground come out wavier.
    #[test]
    fn the_wave_does_not_stretch_with_the_field() {
        // A whole meter per sample, and both extents are whole multiples of
        // it, so the two grids share these sample points exactly.
        let ground = |length, width| {
            terrain_grid(&TerrainParams {
                length,
                width,
                detail: 16,
                ..default()
            })
        };
        let (large, small) = (ground(80.0, 50.0), ground(20.0, 20.0));
        for (x, y) in [(0.0, 0.0), (3.0, -2.0), (-5.0, 4.0)] {
            assert_eq!(large.height(x, y), small.height(x, y), "at ({x}, {y})");
        }
    }

    /// `MAX_NOISE_SLOPE` is measured rather than derived, so it is worth
    /// re-measuring: the inclination cap is only a cap while the noise field
    /// stays under it, and only useful while it stays near it.
    #[test]
    fn the_noise_stays_under_its_slope_bound() {
        let h = 1e-5;
        let mut steepest = 0.0_f64;
        // 30 cells square -- more of the lattice than the largest field at
        // the smallest feature size reaches.
        for i in 0..400 {
            for j in 0..400 {
                let (x, y) = (i as f64 * 0.075 - 15.0, j as f64 * 0.075 - 15.0);
                let dx = (perlin(x + h, y) - perlin(x - h, y)) / (2.0 * h);
                let dy = (perlin(x, y + h) - perlin(x, y - h)) / (2.0 * h);
                steepest = steepest.max(dx.hypot(dy));
            }
        }
        assert!(steepest < MAX_NOISE_SLOPE, "measured {steepest}");
        assert!(
            steepest > MAX_NOISE_SLOPE * 0.5,
            "not so loose it flattens the ground: {steepest}"
        );
    }

    /// Every face must wind counter-clockwise seen from above, or the ground
    /// renders as a hole under USD's default right-handed orientation.
    #[test]
    fn faces_wind_upward() {
        let m = mesh(&TerrainParams::default());
        for (i, face) in faces(&m).enumerate() {
            assert!(face_normal(&m, face).z > 0.0, "face {i} normal points up");
        }
    }

    #[test]
    fn ground_height_matches_the_mesh_at_grid_points() {
        let ground = terrain_grid(&TerrainParams {
            length: 8.0,
            width: 3.0,
            feature_size: 4.0,
            ..default()
        });
        for [x, y, z] in mesh_data(&ground).points {
            assert!(
                (ground.height(x, y) - z).abs() < 1e-4,
                "grid point ({x}, {y}) samples back to its own height"
            );
        }
    }

    /// Exercises the bilinear math directly against a hand-built grid rather
    /// than a real terrain's: a midpoint check like this needs the field to
    /// be exactly linear between neighbouring samples, which noise is only
    /// approximately.
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
        let ground = terrain_grid(&params);

        let far_below = ground.height(-1e6, -1e6);
        let corner = ground.height(
            (-(params.length as f64) / 2.0) as f32,
            (-(params.width as f64) / 2.0) as f32,
        );
        assert!((far_below - corner).abs() < 1e-3, "clamps to the near edge");
    }

    #[test]
    fn an_unbuilt_ground_samples_to_zero() {
        assert_eq!(Ground::default().height(1.0, 2.0), 0.0);
    }

    #[test]
    fn the_ground_reaches_the_export_as_a_referenced_part() {
        let (_, doc) = built(TerrainParams::default());

        let terrain = doc
            .root
            .children
            .iter()
            .find(|child| child.name == TERRAIN)
            .expect("the terrain hangs off the scene root");
        let name = terrain
            .reference
            .as_deref()
            .expect("its geometry comes from the mesh library");

        let part = doc
            .parts
            .iter()
            .find(|part| part.name == name)
            .unwrap_or_else(|| panic!("the library holds `{name}`"));
        assert!(!part.points.is_empty(), "the ground has points");
        assert!(
            part.normals.is_some(),
            "and normals, which the USD export used to leave to a consumer's guess"
        );
        // Exact triangles: the ground is what a robot stands on, and its mesh
        // is the shape to stand on. Instancing goes in exchange, so that the
        // collider lands on a real prim rather than inside a prototype.
        assert_eq!(part.collision.as_deref(), Some(TRIANGLE_MESH));
        // Sampled at the mesh's narrowest span, so a height field built from it
        // resolves every span of a grid that is rectilinear but uneven.
        let narrowest = |axis: usize| {
            let mut coords: Vec<f32> = part.points.iter().map(|point| point[axis]).collect();
            coords.sort_by(f32::total_cmp);
            coords
                .windows(2)
                .map(|pair| pair[1] - pair[0])
                .filter(|gap| *gap > 1e-3) // the same grid line, off in its last bits
                .fold(f32::INFINITY, f32::min)
        };
        let spacing = part
            .heightfield_resolution
            .expect("the ground says how finely it is sampled");
        assert!(
            (spacing - narrowest(0).min(narrowest(1))).abs() < 1e-3,
            "authored {spacing}, mesh spans {} and {}",
            narrowest(0),
            narrowest(1)
        );
        assert!(!terrain.instanceable);
    }

    /// A rebuild has to *replace* what the last one made. Left alone, a slider
    /// drag would stack a ground on every frame it moved and fill the mesh
    /// library with geometry nothing references.
    #[test]
    fn rebuilding_replaces_what_it_built_before() {
        let (mut app, _) = built(TerrainParams::default());
        app.insert_resource(TerrainParams {
            detail: 8,
            ..default()
        });
        app.update();

        let doc = scene_doc(app.world_mut()).unwrap();
        assert_eq!(doc.root.children.len(), 1, "one ground, not two");
        assert_eq!(doc.parts.len(), 1, "and one entry in the library");
    }
}
