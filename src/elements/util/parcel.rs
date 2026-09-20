//! Parcel layout — where rows go.
//!
//! Not an element in the sense the other modules in this directory are: it
//! owns no prim subtree and authors no USD. It solves the vineyard's row
//! geometry — how a rectangular planting area divides into parallel rows,
//! and each row into panels and vines — and publishes the result as the
//! [`VineyardLayout`] resource for later elements (trunk, pole, cover-crop)
//! to place their geometry against.
//!
//! Wired from [`terrain`] rather than given its own [`Grow`] slot: it needs
//! the terrain's extent to know what rectangle it's filling, and terrain
//! already owns the [`Ground`] height field rows get draped onto, so
//! chaining after `terrain::author` avoids relying on
//! system-ordering-across-elements for something one element's plugin can
//! just guarantee directly.
//!
//! Rows are solved in plan view (the XY ground plane) and lifted onto
//! [`Ground`] only when a consumer asks for actual 3D positions —
//! [`Row::post_positions`] and [`Row::vine_positions`] do that
//! lifting. That keeps spacing parameters (`row_spacing`, `vine_spacing`,
//! ...) exact in plan view rather than measured along the slope; the error
//! from that simplification stays under 1% below roughly a 15% grade, well
//! past what this terrain generates.
//!
//! [`terrain`]: crate::elements::terrain
//! [`terrain::plugin`]: crate::elements::terrain::plugin
//! [`Grow`]: crate::elements::Grow

use bevy::color::palettes::basic::{GRAY, YELLOW};
use bevy::color::palettes::css::{LIMEGREEN, ORANGE};
use bevy::prelude::*;

use crate::elements::terrain::{Ground, TerrainParams};
use crate::params::Slider;

/// How vineyard rows are laid out across the terrain.
///
/// Solves the positions the planting puts a vine and a post at. `vine_spacing`
/// also sizes the cordons the vines build, and `trellis_height` is how tall
/// the posts stand.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(get_all, set_all, skip_from_py_object)
)]
pub struct ParcelParams {
    /// Row direction, in degrees counter-clockwise from +X.
    #[reflect(@Slider { min: -90.0, max: 90.0, step: 1.0 })]
    pub orientation: f32,
    /// Inset from the terrain's edge left unplanted, in meters — the
    /// turning area machinery needs at each end of a row.
    #[reflect(@Slider { min: 0.0, max: 20.0, step: 0.5 })]
    pub headland: f32,
    /// Distance between neighbouring row centerlines, in meters.
    #[reflect(@Slider { min: 0.8, max: 6.0, step: 0.1 })]
    pub row_spacing: f32,
    /// Distance between neighbouring vines along a row, in meters.
    #[reflect(@Slider { min: 0.5, max: 3.0, step: 0.1 })]
    pub vine_spacing: f32,
    /// Target distance between posts along a row, in meters. Panel count is
    /// solved from this and the row's actual length, so real post spacing
    /// comes out close to this value rather than exactly equal to it.
    #[reflect(@Slider { min: 2.0, max: 12.0, step: 0.5 })]
    pub post_spacing: f32,
    /// Rows shorter than this, after clipping to the headland-inset
    /// rectangle, are dropped rather than planted.
    #[reflect(@Slider { min: 0.0, max: 50.0, step: 1.0 })]
    pub min_row_length: f32,
    /// Height of the trellis above the ground, in meters: how tall the posts
    /// stand, and what the top catch wires hang just under.
    ///
    /// Not consumed by the layout solver itself — it is how tall [`pole`]
    /// builds its posts, and how tall the layout gizmo draws them.
    ///
    /// [`pole`]: crate::elements::pole
    #[reflect(@Slider { min: 0.5, max: 3.0, step: 0.1 })]
    pub trellis_height: f32,
}

impl Default for ParcelParams {
    fn default() -> Self {
        Self {
            orientation: 0.0,
            headland: 6.0,
            row_spacing: 2.4,
            vine_spacing: 1.2,
            post_spacing: 6.0,
            min_row_length: 10.0,
            trellis_height: 1.8,
        }
    }
}

/// One row of vines: a straight segment in plan view, subdivided into
/// panels (post to post) and, within each panel, evenly spaced vines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub start: Vec2,
    pub end: Vec2,
    /// Number of panels the row is divided into; there are `panels + 1`
    /// posts.
    pub panels: u32,
    /// Vines planted per panel, evenly spaced between (not on) its posts.
    pub vines_per_panel: u32,
}

impl Row {
    fn length(&self) -> f32 {
        self.start.distance(self.end)
    }

    /// Unit vector along the row, in plan view. Public because elements that
    /// place row-aligned geometry — a vine's cordons run *along* the row —
    /// need it to derive their instance orientation.
    pub fn direction(&self) -> Vec2 {
        (self.end - self.start).normalize_or_zero()
    }

    /// Positions of this row's `panels + 1` posts, lifted onto `ground`.
    pub fn post_positions<'a>(&'a self, ground: &'a Ground) -> impl Iterator<Item = Vec3> + 'a {
        let panels = self.panels.max(1);
        let step = self.length() / panels as f32;
        let dir = self.direction();
        (0..=panels).map(move |i| ground.lift(self.start + dir * (step * i as f32)))
    }

    /// Positions of every vine in this row, lifted onto `ground`. Vines sit
    /// at the midpoints of their slot within a panel, so none coincide with
    /// a post.
    pub fn vine_positions<'a>(&'a self, ground: &'a Ground) -> impl Iterator<Item = Vec3> + 'a {
        let panels = self.panels.max(1);
        let vines_per_panel = self.vines_per_panel.max(1);
        let panel_len = self.length() / panels as f32;
        let dir = self.direction();
        let start = self.start;
        (0..panels).flat_map(move |panel| {
            let panel_start = start + dir * (panel_len * panel as f32);
            let vine_step = panel_len / vines_per_panel as f32;
            (0..vines_per_panel)
                .map(move |v| ground.lift(panel_start + dir * (vine_step * (v as f32 + 0.5))))
        })
    }

    /// The strip of ground along this row reaching `half_width` either side
    /// of the trunks — the under-vine strip.
    pub fn band(&self, half_width: f32) -> Band {
        Band {
            start: self.start,
            end: self.end,
            half_width,
        }
    }
}

/// A strip of ground: a centerline in plan view and how far it reaches either
/// side of it.
///
/// A row's under-vine strip is one, the alley between two rows is one, and so
/// is a tile of sward — anything placed *within* a zone rather than *along* a
/// line takes its frame from here. See [`scatter`](super::scatter).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub start: Vec2,
    pub end: Vec2,
    pub half_width: f32,
}

impl Band {
    pub fn length(&self) -> f32 {
        self.start.distance(self.end)
    }

    /// Unit vector along the band, in plan view.
    pub fn direction(&self) -> Vec2 {
        (self.end - self.start).normalize_or_zero()
    }

    /// Unit vector across the band: a quarter turn counter-clockwise from
    /// [`direction`](Self::direction).
    pub fn across(&self) -> Vec2 {
        self.direction().perp()
    }

    /// `p` in the band's own frame: meters along from `start`, meters across
    /// from the centerline.
    pub fn local(&self, p: Vec2) -> Vec2 {
        let d = p - self.start;
        Vec2::new(d.dot(self.direction()), d.dot(self.across()))
    }

    pub fn contains(&self, p: Vec2) -> bool {
        let local = self.local(p);
        (0.0..=self.length()).contains(&local.x) && local.y.abs() <= self.half_width
    }
}

/// The alley between two neighbouring rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Alley {
    /// The row on its one side: alley `k` lies between rows `k` and `k + 1`.
    pub index: usize,
    /// The whole width between the two rows' centerlines. A row's own strip
    /// is the caller's to subtract.
    pub band: Band,
}

/// The solved row layout: a set of parallel rows filling the headland-inset
/// planting rectangle. 2D intent only — no heights, no geometry — so it
/// stays cheap to re-solve on every parameter tweak and agnostic to which
/// element ends up placing trunks, poles or cover crop against it.
#[derive(Resource, Clone, Debug, Default)]
pub struct VineyardLayout {
    /// The planting rectangle: the terrain's extent, inset by `headland`.
    pub bounds: Rect,
    pub rows: Vec<Row>,
    /// The spacing the rows were solved at, in meters. What says whether two
    /// consecutive rows are neighbours — see [`alleys`](Self::alleys).
    pub row_spacing: f32,
}

impl VineyardLayout {
    /// The alley between every pair of neighbouring rows.
    ///
    /// Rows are listed in the order they were swept, so neighbours are
    /// consecutive — except across a row that was dropped for being too
    /// short, which leaves a gap wider than the spacing and no alley in it.
    ///
    /// On a parcel cut at an angle the two rows are clipped at different
    /// points, so the centerline joins the midpoints of their ends and runs a
    /// little past the shorter row; anything placed in a band still checks
    /// [`bounds`](Self::bounds).
    pub fn alleys(&self) -> Vec<Alley> {
        self.rows
            .windows(2)
            .enumerate()
            .filter_map(|(index, pair)| {
                let [a, b] = pair else { return None };
                let width = (b.start - a.start).dot(a.direction().perp()).abs();
                if width > 1.5 * self.row_spacing {
                    return None;
                }
                Some(Alley {
                    index,
                    band: Band {
                        start: (a.start + b.start) / 2.0,
                        end: (a.end + b.end) / 2.0,
                        half_width: width / 2.0,
                    },
                })
            })
            .collect()
    }
}

/// Re-solves [`VineyardLayout`] from `parcel` and `terrain`. Called from
/// [`terrain::plugin`], chained after `terrain::author`; see the module docs
/// for why it lives there instead of its own [`Grow`] slot.
pub fn author(
    parcel: Res<ParcelParams>,
    terrain: Res<TerrainParams>,
    mut layout: ResMut<VineyardLayout>,
) {
    *layout = solve(&parcel, &terrain);
}

/// Solves the row layout for a rectangular terrain of `terrain.length` by
/// `terrain.width`, centered on the origin. Pure function of its
/// parameters, so it's the whole surface worth testing.
fn solve(parcel: &ParcelParams, terrain: &TerrainParams) -> VineyardLayout {
    let half = Vec2::new(
        (terrain.length / 2.0).max(0.0),
        (terrain.width / 2.0).max(0.0),
    );
    let extent = Rect::from_center_half_size(Vec2::ZERO, half);
    let bounds = extent.inflate(-parcel.headland.max(0.0));
    if bounds.is_empty() {
        return VineyardLayout {
            bounds: Rect::from_center_size(extent.center(), Vec2::ZERO),
            rows: Vec::new(),
            row_spacing: parcel.row_spacing,
        };
    }

    let theta = parcel.orientation.to_radians();
    let dir = Vec2::new(theta.cos(), theta.sin());
    let normal = Vec2::new(-theta.sin(), theta.cos());
    let center = bounds.center();

    // Half-extent of the planting rectangle projected onto `normal`, so the
    // sweep of row offsets covers it regardless of orientation.
    let corners = [
        bounds.min,
        Vec2::new(bounds.max.x, bounds.min.y),
        bounds.max,
        Vec2::new(bounds.min.x, bounds.max.y),
    ];
    let half_extent = corners
        .iter()
        .map(|c| (*c - center).dot(normal).abs())
        .fold(0.0_f32, f32::max);

    let row_spacing = parcel.row_spacing.max(0.01);
    // Capped defensively: a pathologically small spacing on a large parcel
    // would otherwise try to solve thousands of rows per frame.
    let count = ((half_extent / row_spacing).floor() as i64).clamp(0, 2000) as i32;

    let rows = (-count..=count)
        .filter_map(|i| {
            let offset = center + normal * (i as f32 * row_spacing);
            let (t0, t1) = clip_line(offset, dir, bounds)?;
            let start = offset + dir * t0;
            let end = offset + dir * t1;
            if start.distance(end) < parcel.min_row_length {
                return None;
            }
            Some(subdivide(start, end, parcel))
        })
        .collect();

    VineyardLayout {
        bounds,
        rows,
        row_spacing,
    }
}

/// Divides a row into panels close to `post_spacing` long, then each panel
/// into vines close to `vine_spacing` apart. Rounding to a whole panel count
/// first, then a whole vine count per panel, is what keeps posts landing on
/// an even multiple of the vine spacing without validating it as a separate
/// constraint.
fn subdivide(start: Vec2, end: Vec2, parcel: &ParcelParams) -> Row {
    let length = start.distance(end);
    let panels = ((length / parcel.post_spacing.max(0.01)).round().max(1.0)) as u32;
    let panel_len = length / panels as f32;
    let vines_per_panel = ((panel_len / parcel.vine_spacing.max(0.01)).round().max(1.0)) as u32;
    Row {
        start,
        end,
        panels,
        vines_per_panel,
    }
}

/// Clips the infinite line through `origin` in direction `dir` (assumed
/// unit length) to the axis-aligned `rect`, by intersecting the line's
/// parameter range against each axis's slab. Returns `None` if the line
/// misses the rectangle entirely.
fn clip_line(origin: Vec2, dir: Vec2, rect: Rect) -> Option<(f32, f32)> {
    let mut t0 = f32::NEG_INFINITY;
    let mut t1 = f32::INFINITY;

    for (o, d, lo, hi) in [
        (origin.x, dir.x, rect.min.x, rect.max.x),
        (origin.y, dir.y, rect.min.y, rect.max.y),
    ] {
        if d.abs() < 1e-9 {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (mut near, mut far) = ((lo - o) / d, (hi - o) / d);
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        t0 = t0.max(near);
        t1 = t1.min(far);
        if t0 > t1 {
            return None;
        }
    }
    Some((t0, t1))
}

/// Toggles the debug gizmo overlay. Viewer-only: see [`debug_plugin`].
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct ShowLayout(pub bool);

/// Draws [`VineyardLayout`] with `Gizmos`. Kept out of [`plugin`] because
/// `Gizmos` needs `GizmoPlugin`, which the headless generation path's
/// `MinimalPlugins` doesn't provide — see [`crate::generate::generate_stage`].
pub fn debug_plugin(app: &mut App) {
    app.init_resource::<ShowLayout>()
        .add_systems(Update, draw_gizmos.run_if(|show: Res<ShowLayout>| show.0));
}

/// Caps how many vine ticks get drawn, so a dense parcel doesn't tank the
/// viewer's frame rate. Posts are always drawn in full — there are orders of
/// magnitude fewer of them.
const MAX_VINE_GIZMOS: usize = 20_000;

fn draw_gizmos(
    layout: Res<VineyardLayout>,
    parcel: Res<ParcelParams>,
    ground: Res<Ground>,
    mut gizmos: Gizmos,
) {
    let corners = [
        layout.bounds.min,
        Vec2::new(layout.bounds.max.x, layout.bounds.min.y),
        layout.bounds.max,
        Vec2::new(layout.bounds.min.x, layout.bounds.max.y),
    ]
    .map(|p| to_bevy(ground.lift(p)));
    for i in 0..corners.len() {
        gizmos.line(corners[i], corners[(i + 1) % corners.len()], GRAY);
    }

    let mut vines_drawn = 0usize;
    for row in &layout.rows {
        gizmos.linestrip(sample_row(row, &ground).into_iter().map(to_bevy), YELLOW);

        for post in row.post_positions(&ground) {
            gizmos.line(
                to_bevy(post),
                to_bevy(post + Vec3::Z * parcel.trellis_height),
                ORANGE,
            );
        }

        for vine in row.vine_positions(&ground) {
            if vines_drawn >= MAX_VINE_GIZMOS {
                break;
            }
            gizmos.line(to_bevy(vine), to_bevy(vine + Vec3::Z * 0.2), LIMEGREEN);
            vines_drawn += 1;
        }
    }
}

/// Maps a point from the stage's Z-up space onto Bevy's Y-up world: `-90°`
/// about X, the same rotation [`scene::z_up_to_y_up`](crate::scene) applies
/// so gizmos line up with the projected mesh.
fn to_bevy(p: Vec3) -> Vec3 {
    Vec3::new(p.x, p.z, -p.y)
}

/// Samples a row's centerline roughly every metre, draped onto `ground`, for
/// a gizmo linestrip smooth enough to show terrain relief along the row.
fn sample_row(row: &Row, ground: &Ground) -> Vec<Vec3> {
    let length = row.length();
    let steps = (length.max(0.01).ceil() as u32).max(1);
    let dir = row.direction();
    (0..=steps)
        .map(|i| {
            let t = length * (i as f32 / steps as f32);
            ground.lift(row.start + dir * t)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> ParcelParams {
        ParcelParams::default()
    }

    fn terrain() -> TerrainParams {
        TerrainParams {
            length: 80.0,
            width: 50.0,
            ..default()
        }
    }

    #[test]
    fn rows_are_parallel_and_row_spacing_apart() {
        let layout = solve(&params(), &terrain());
        assert!(layout.rows.len() > 1, "expected multiple rows");

        let dir = layout.rows[0].direction();
        for row in &layout.rows {
            assert!(
                row.direction().distance(dir) < 1e-4,
                "all rows share one direction"
            );
        }

        let mut offsets: Vec<f32> = layout
            .rows
            .iter()
            .map(|row| {
                let normal = Vec2::new(-dir.y, dir.x);
                row.start.dot(normal)
            })
            .collect();
        offsets.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for pair in offsets.windows(2) {
            assert!(
                (pair[1] - pair[0] - params().row_spacing).abs() < 1e-3,
                "consecutive rows are one row_spacing apart, got {:?}",
                pair
            );
        }
    }

    #[test]
    fn a_rotated_parcel_yields_rows_of_differing_length() {
        let mut parcel = params();
        parcel.orientation = 30.0;
        let layout = solve(&parcel, &terrain());

        let lengths: Vec<f32> = layout.rows.iter().map(Row::length).collect();
        let (min, max) = lengths
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &l| (lo.min(l), hi.max(l)));
        assert!(
            max - min > 1.0,
            "a rotated sweep over an axis-aligned rectangle produces rows of varied length, got {min}..{max}"
        );
    }

    #[test]
    fn the_layout_is_symmetric_about_the_origin_at_zero_orientation() {
        let layout = solve(&params(), &terrain());
        let centers: Vec<Vec2> = layout
            .rows
            .iter()
            .map(|row| (row.start + row.end) / 2.0)
            .collect();
        let sum = centers.iter().fold(Vec2::ZERO, |acc, c| acc + *c);
        assert!(
            (sum / centers.len() as f32).length() < 1e-3,
            "row centers average out to the origin"
        );
    }

    #[test]
    fn a_headland_larger_than_the_parcel_yields_no_rows() {
        let mut parcel = params();
        parcel.headland = 1000.0;
        let layout = solve(&parcel, &terrain());
        assert!(layout.rows.is_empty());
    }

    #[test]
    fn solve_is_deterministic() {
        let a = solve(&params(), &terrain());
        let b = solve(&params(), &terrain());
        assert_eq!(a.rows, b.rows);
    }

    #[test]
    fn panels_and_vines_are_close_to_their_targets() {
        let layout = solve(&params(), &terrain());
        let parcel = params();
        for row in &layout.rows {
            let panel_len = row.length() / row.panels as f32;
            assert!(
                (panel_len - parcel.post_spacing).abs() <= parcel.post_spacing / 2.0,
                "panel length {panel_len} stays close to the {} target",
                parcel.post_spacing
            );
            let vine_step = panel_len / row.vines_per_panel as f32;
            assert!(
                (vine_step - parcel.vine_spacing).abs() <= parcel.vine_spacing / 2.0,
                "vine step {vine_step} stays close to the {} target",
                parcel.vine_spacing
            );
        }
    }

    #[test]
    fn row_post_and_vine_counts_match_their_panel_structure() {
        let row = Row {
            start: Vec2::new(0.0, 0.0),
            end: Vec2::new(12.0, 0.0),
            panels: 3,
            vines_per_panel: 2,
        };
        let ground = Ground::default();
        assert_eq!(row.post_positions(&ground).count(), 4);
        assert_eq!(row.vine_positions(&ground).count(), 6);
    }

    #[test]
    fn clip_line_returns_none_when_the_line_misses_the_rect() {
        let rect = Rect::from_center_half_size(Vec2::ZERO, Vec2::splat(1.0));
        assert!(clip_line(Vec2::new(10.0, 10.0), Vec2::X, rect).is_none());
    }

    #[test]
    fn clip_line_clips_to_the_rect_bounds() {
        let rect = Rect::from_center_half_size(Vec2::ZERO, Vec2::splat(1.0));
        let (t0, t1) = clip_line(Vec2::ZERO, Vec2::X, rect).unwrap();
        assert!((t0 - (-1.0)).abs() < 1e-5);
        assert!((t1 - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_band_measures_points_in_its_own_frame() {
        let band = Band {
            start: Vec2::new(0.0, 0.0),
            end: Vec2::new(0.0, 10.0),
            half_width: 1.0,
        };
        // Along +Y, so "across" is a quarter turn on: -X.
        assert!((band.across() - Vec2::new(-1.0, 0.0)).length() < 1e-6);
        let local = band.local(Vec2::new(-0.5, 4.0));
        assert!((local - Vec2::new(4.0, 0.5)).length() < 1e-6);
        assert!(band.contains(Vec2::new(0.9, 9.9)));
        assert!(!band.contains(Vec2::new(1.1, 5.0)));
        assert!(!band.contains(Vec2::new(0.0, 10.1)));
    }

    #[test]
    fn an_alley_lies_between_every_pair_of_neighbouring_rows() {
        let layout = solve(&params(), &terrain());
        let alleys = layout.alleys();
        assert_eq!(alleys.len(), layout.rows.len() - 1);
        for (alley, pair) in alleys.iter().zip(layout.rows.windows(2)) {
            assert!((alley.band.half_width * 2.0 - params().row_spacing).abs() < 1e-4);
            let mid = (pair[0].start + pair[1].start) / 2.0;
            assert!((alley.band.start - mid).length() < 1e-4);
            assert!(alley.band.contains(pair[0].start.lerp(pair[1].start, 0.5)));
            let edge = alley.band.local(pair[0].start).y.abs();
            assert!(
                (edge - alley.band.half_width).abs() < 1e-4,
                "a row is the edge"
            );
        }
    }

    /// A row dropped for being too short leaves its two neighbours twice the
    /// spacing apart, and that gap is not an alley anything should be placed in.
    #[test]
    fn a_dropped_row_leaves_no_alley() {
        let row = |y: f32| Row {
            start: Vec2::new(0.0, y),
            end: Vec2::new(10.0, y),
            panels: 1,
            vines_per_panel: 1,
        };
        let layout = VineyardLayout {
            bounds: Rect::from_center_half_size(Vec2::new(5.0, 2.0), Vec2::new(5.0, 3.0)),
            rows: vec![row(0.0), row(1.0), row(3.0)],
            row_spacing: 1.0,
        };
        let alleys = layout.alleys();
        assert_eq!(alleys.len(), 1);
        assert_eq!(alleys[0].index, 0);
    }
}
