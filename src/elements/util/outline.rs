//! Outlines traced in SVG, and the flat meshes filled into them.
//!
//! Some shapes are not worth describing in code. A leaf blade's lobes and
//! teeth are one: drawing one takes a minute in a vector editor and a hundred
//! lines of guesswork here. So the shape is drawn once, committed under
//! `assets/`, and read back through this module — which knows no botany, and
//! only ever sees a closed ring of points to fill.
//!
//! # The frame an outline is drawn in
//!
//! An outline file holds one closed shape drawn **pointing up the page**,
//! hanging by the point it is meant to be attached at — the *bottom* of the
//! drawing. SVG's y-axis runs downward, so that attachment point is the ring's
//! **largest** `y`, at about half the drawing's width.
//!
//! [`Outline::from_svg`] moves the ring into a Z-up frame by putting that
//! point at the origin and turning the drawing a quarter turn clockwise, which
//! leaves the shape running along **+X**, flat on the XY plane, front face
//! toward +Z:
//!
//! ```text
//!   the drawing            the outline
//!        ╱╲                    +Y
//!       ╱  ╲                    ↑
//!       ╲  ╱     becomes        ·──→ +X   ◁═══
//!        ╲╱                   origin
//!        ││                  (the attachment)
//!        ┴┴ ← attachment
//! ```
//!
//! Which axis goes where is not a free choice, and getting it wrong is
//! invisible. Reading the drawing's *y* as the outline's *x* and its *x* as
//! the outline's *y* — both negated about the anchor — is the quarter turn.
//! Dropping either negation is a **reflection** instead, and a reflected
//! outline looks entirely correct until someone notices the shape is its own
//! mirror image. Anchoring the other end of the drawing is worse still: the
//! shape then runs the right way along +X with the wrong end at the origin,
//! and a leaf hangs from its tip with its stalk in the air.
//!
//! # Size
//!
//! Nothing about the drawing's own scale survives. A file may be traced at
//! any size, and [`Outline::with_area`] rescales it to enclose the area asked
//! for — so a library of outlines can be drawn independently and still come
//! out as a set of consistently sized things.

use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use spade::{
    AngleLimit, ConstrainedDelaunayTriangulation, Point2, RefinementParameters, Triangulation,
};
use usvg::tiny_skia_path::PathSegment;

use super::usd::MeshData;

/// How far a flattened curve may stray from the curve it replaces, in the
/// drawing's own units.
///
/// The outlines under `assets/` are traced as polylines, so nothing reaches
/// this today. It is here so that a file drawn with real Béziers in it comes
/// out smooth rather than faceted.
const FLATTEN_TOLERANCE: f64 = 0.25;

/// Angle-based refinement is **off**, and area is left to drive the fill
/// alone.
///
/// Ruppert's algorithm chases a minimum inner angle by inserting points
/// wherever it finds a sharper one, and a traced outline is sharp corners
/// almost everywhere — every tooth along the margin is one. Chasing them
/// costs a great deal and gets nowhere, because a corner *on a constraint
/// edge* cannot be opened up at all: at spade's 30° default a leaf comes out
/// four times the triangles of the same leaf at 0°, all of them crowded into
/// the margin, and `max_triangle_area` stops making any visible difference.
///
/// Turning it off leaves the only knob that matters — how finely the *inside*
/// is divided, which is the part that has no vertices of its own and the part
/// a later deformation pass has to bend. The slivers this leaves at the teeth
/// are the shape of the teeth.
const ANGLE_LIMIT_DEG: f64 = 0.0;

/// A closed ring of points on the XY plane.
///
/// The first point is not repeated at the end; the ring closes implicitly.
/// Winding is whatever the drawing had — [`outline_mesh`] does not depend on
/// it, and neither should anything else.
#[derive(Clone, Debug)]
pub struct Outline {
    pub points: Vec<Point2<f64>>,
}

impl Outline {
    /// Reads the outline out of an SVG document.
    ///
    /// The document may hold anything a vector editor emits — groups, nested
    /// transforms, `<use>`, `<polygon>`, CSS — because `usvg` resolves the
    /// document down to plain paths first, and reading one is then only a
    /// matter of applying the transform it carries. Of the closed subpaths
    /// that survive, the largest wins, so construction marks and stray guides
    /// left in a file are ignored rather than fatal.
    pub fn from_svg(svg: &str) -> Result<Self> {
        let tree = usvg::Tree::from_str(svg, &usvg::Options::default())
            .context("the outline is not a readable SVG document")?;

        let mut rings = Vec::new();
        collect_rings(tree.root(), &mut rings);
        let ring = rings
            .into_iter()
            .max_by(|a, b| ring_area(a).abs().total_cmp(&ring_area(b).abs()))
            .context("the outline holds no closed shape")?;

        let outline = Self {
            points: to_local_frame(&ring),
        };
        if outline.area() <= 0.0 {
            bail!("the outline encloses no area");
        }
        Ok(outline)
    }

    /// The area the ring encloses, by the shoelace formula.
    pub fn area(&self) -> f64 {
        ring_area(&self.points).abs()
    }

    /// The same shape scaled about the origin to enclose `area`.
    ///
    /// Area rather than length or width, because it is what makes a set of
    /// differently-proportioned outlines read as the same size: a long narrow
    /// one and a broad round one matched by length look nothing alike, and
    /// matched by area look like two of the same thing.
    pub fn with_area(mut self, area: f64) -> Self {
        let scale = (area / self.area()).sqrt();
        for p in &mut self.points {
            *p = Point2::new(p.x * scale, p.y * scale);
        }
        self
    }
}

/// Twice-signed area, halved: positive counter-clockwise, negative clockwise.
fn ring_area(ring: &[Point2<f64>]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        / 2.0
}

// ─── Reading SVG ────────────────────────────────────────────────────

/// Every closed subpath in the tree, flattened to polylines, still in the
/// drawing's own coordinates.
fn collect_rings(group: &usvg::Group, out: &mut Vec<Vec<Point2<f64>>>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(inner) => collect_rings(inner, out),
            usvg::Node::Path(path) => {
                // `usvg` keeps a path's data in the coordinates it was
                // written in and the ancestors' transforms *beside* it, so a
                // group transform reaches the geometry only by being applied
                // here. (Its "absolute coordinates" are about SVG's relative
                // path commands having been resolved, not about the frame.)
                // A transform that collapses the shape yields nothing to fill.
                if let Some(data) = path.data().clone().transform(path.abs_transform()) {
                    flatten_path(&data, out);
                }
            }
            _ => {}
        }
    }
}

/// Walks one path's segments, emitting a ring per closed subpath.
///
/// An unclosed subpath is dropped: this module fills areas, and a stroke that
/// never came back to where it started does not bound one.
fn flatten_path(path: &usvg::tiny_skia_path::Path, out: &mut Vec<Vec<Point2<f64>>>) {
    let mut ring: Vec<Point2<f64>> = Vec::new();
    for segment in path.segments() {
        let last = ring.last().copied().unwrap_or(Point2::new(0.0, 0.0));
        match segment {
            PathSegment::MoveTo(p) => {
                ring.clear();
                ring.push(point(p));
            }
            PathSegment::LineTo(p) => ring.push(point(p)),
            PathSegment::QuadTo(c, p) => {
                // As a cubic, so one flattener covers both: a quadratic's
                // control point pulls each cubic handle two thirds of the way.
                let (c, p) = (point(c), point(p));
                let c1 = lerp(last, c, 2.0 / 3.0);
                let c2 = lerp(p, c, 2.0 / 3.0);
                flatten_cubic(last, c1, c2, p, &mut ring);
            }
            PathSegment::CubicTo(c1, c2, p) => {
                flatten_cubic(last, point(c1), point(c2), point(p), &mut ring)
            }
            PathSegment::Close => {
                // The closing point coincides with the opening one and would
                // be a duplicate vertex; the ring closes implicitly instead.
                if ring.len() >= 3 {
                    out.push(std::mem::take(&mut ring));
                } else {
                    ring.clear();
                }
            }
        }
    }
}

fn point(p: usvg::tiny_skia_path::Point) -> Point2<f64> {
    Point2::new(p.x as f64, p.y as f64)
}

fn lerp(a: Point2<f64>, b: Point2<f64>, t: f64) -> Point2<f64> {
    Point2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

/// Appends a cubic's interior, subdivided finely enough to sit within
/// [`FLATTEN_TOLERANCE`] of the curve, plus its end point.
///
/// The step count comes from the control polygon, which bounds the curve: how
/// far a uniform subdivision can stray falls as the square of the step count,
/// so the polygon's length over the tolerance, square-rooted, is the count
/// that clears it.
fn flatten_cubic(
    a: Point2<f64>,
    c1: Point2<f64>,
    c2: Point2<f64>,
    b: Point2<f64>,
    out: &mut Vec<Point2<f64>>,
) {
    let hull = distance(a, c1) + distance(c1, c2) + distance(c2, b);
    let steps = ((hull / FLATTEN_TOLERANCE).sqrt().ceil() as usize).clamp(1, 64);
    for step in 1..=steps {
        let t = step as f64 / steps as f64;
        out.push(lerp(
            lerp(lerp(a, c1, t), lerp(c1, c2, t), t),
            lerp(lerp(c1, c2, t), lerp(c2, b, t), t),
            t,
        ));
    }
}

fn distance(a: Point2<f64>, b: Point2<f64>) -> f64 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
}

/// Puts a ring drawn in SVG user space into the outline's own frame: the
/// attachment point at the origin, the shape running along +X.
///
/// See the module docs for the turn this is, and for what the two ways of
/// getting it wrong look like.
fn to_local_frame(ring: &[Point2<f64>]) -> Vec<Point2<f64>> {
    let along = ring.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
    // A tie is normal rather than pathological: an attachment is drawn as a
    // stalk cut off square, so its end is a short flat run and either corner
    // of it is an arbitrary anchor. The middle of the run is not.
    let tied: Vec<f64> = ring
        .iter()
        .filter(|p| p.y >= along - TIE_TOLERANCE)
        .map(|p| p.x)
        .collect();
    let across = tied.iter().sum::<f64>() / tied.len() as f64;

    ring.iter()
        .map(|p| Point2::new(along - p.y, across - p.x))
        .collect()
}

/// How close to the anchoring point another point has to be to count as level
/// with it. Well under the precision outlines are traced at, and well over
/// the rounding in an SVG's decimal coordinates.
const TIE_TOLERANCE: f64 = 1e-6;

// ─── Filling ────────────────────────────────────────────────────────

type Cdt = ConstrainedDelaunayTriangulation<Point2<f64>>;

/// Fills an outline with triangles, flat on the XY plane at z = 0.
///
/// The ring's own points become constraint edges, so every tooth and notch
/// survives exactly as drawn; `max_triangle_area` then controls how finely
/// the inside is subdivided beyond that. The interior points this inserts are
/// not decoration — they are what a later pass has to work with when it bends
/// the flat shape into something that isn't flat.
///
/// Faces come out counter-clockwise seen from +Z, which under USD's default
/// right-handed orientation puts the front of the shape up.
pub fn outline_mesh(outline: &Outline, max_triangle_area: f64) -> Result<MeshData> {
    let n = outline.points.len();
    let edges: Vec<[usize; 2]> = (0..n).map(|i| [i, (i + 1) % n]).collect();

    // The non-panicking bulk load: a ring that crosses itself is a mistake in
    // the drawing, and it has to arrive as an error an author can read rather
    // than as a panic out of an author system mid-frame.
    let mut crossings = 0usize;
    let mut cdt = Cdt::try_bulk_load_cdt(outline.points.clone(), edges, |_| crossings += 1)
        .context("the outline could not be triangulated")?;
    if crossings > 0 {
        bail!("the outline crosses itself, at {crossings} of its edges");
    }

    // `exclude_outer_faces` is what `triangle -p` does: refine only what the
    // constraint ring encloses, and report everything outside it so it can be
    // dropped. Without it the fill would be the ring's convex hull, webbing
    // over every notch.
    let refinement = cdt.refine(
        RefinementParameters::new()
            .with_angle_limit(AngleLimit::from_deg(ANGLE_LIMIT_DEG))
            .with_max_allowed_area(max_triangle_area)
            .exclude_outer_faces(true),
    );
    let outside: HashSet<_> = refinement.excluded_faces.into_iter().collect();

    // `vertices()` walks the triangulation's vertex storage in order, so a
    // handle's `index()` is its position in this list. That identity is what
    // lets the faces below be emitted as plain indices with no side table.
    let points: Vec<[f32; 3]> = cdt
        .vertices()
        .map(|v| [v.position().x as f32, v.position().y as f32, 0.0])
        .collect();

    let face_vertex_indices: Vec<i32> = cdt
        .inner_faces()
        .filter(|face| !outside.contains(&face.fix()))
        .flat_map(|face| face.vertices().map(|v| v.fix().index() as i32))
        .collect();

    Ok(MeshData {
        face_vertex_counts: vec![3; face_vertex_indices.len() / 3],
        face_vertex_indices,
        points,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::util::testing::{face_normal, faces};

    /// A square, drawn the way the outlines under `assets/` are: standing up
    /// the page and hanging by its bottom edge, in SVG's y-down space.
    const SQUARE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
        <path d="M 4,1 L 6,1 L 6,9 L 4,9 Z"/></svg>"#;

    fn square() -> Outline {
        Outline::from_svg(SQUARE).expect("a square parses")
    }

    #[test]
    fn a_ring_is_read_without_its_closing_duplicate() {
        assert_eq!(square().points.len(), 4, "four corners, not five");
    }

    /// The whole frame contract in one shape: the attachment point lands on
    /// the origin and the drawing runs along +X. Everything that ever places
    /// an outline depends on both.
    #[test]
    fn an_outline_hangs_from_the_origin_and_runs_along_x() {
        let outline = square();

        // The bottom edge's midpoint — x = 5, y = 9 in the drawing — is the
        // anchor, so it lands on the origin and nothing sits behind it.
        assert!(
            outline.points.iter().any(|p| p.x == 0.0 && p.y == 1.0),
            "a corner of the anchoring edge stays level with the origin: {:?}",
            outline.points
        );
        assert!(
            outline.points.iter().all(|p| p.x >= 0.0),
            "the shape runs forward from the anchor: {:?}",
            outline.points
        );

        let reach = outline.points.iter().fold(0.0f64, |m, p| m.max(p.x));
        assert_eq!(reach, 8.0, "and reaches the far edge, eight units up");

        let across = outline.points.iter().fold(0.0f64, |m, p| m.max(p.y.abs()));
        assert_eq!(across, 1.0, "half the drawing's width to either side");
    }

    /// The reflection that would show up only on an asymmetric drawing. A
    /// wedge whose far end leans toward larger `x` — the right of the page —
    /// has to come out leaning toward **-Y**, which is where a clockwise
    /// quarter turn puts the page's right. A mirrored read lands it at +Y and
    /// looks perfectly fine on anything symmetric.
    #[test]
    fn an_asymmetric_outline_is_not_mirrored() {
        // Hangs from a point at the bottom, opens out to the upper right.
        let wedge = Outline::from_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
               <path d="M 5,9 L 9,1 L 6,1 Z"/></svg>"#,
        )
        .unwrap();

        assert!(
            wedge.points.iter().all(|p| p.y <= 0.0),
            "the page's right becomes -Y, got {:?}",
            wedge.points
        );
    }

    #[test]
    fn area_is_the_area_enclosed() {
        assert_eq!(square().area(), 16.0, "two units by eight");
    }

    #[test]
    fn with_area_rescales_to_the_area_asked_for() {
        let scaled = square().with_area(4.0);
        assert!((scaled.area() - 4.0).abs() < 1e-9);
        // Uniformly, so the shape is unchanged: a quarter of the area is half
        // the size in each direction.
        let reach = scaled.points.iter().fold(0.0f64, |m, p| m.max(p.x));
        assert!((reach - 4.0).abs() < 1e-9, "got {reach}");
    }

    /// Every outline is read through `usvg` precisely so that a file does not
    /// have to be hand-normalized first. A group transform is the form that
    /// costs nothing to emit from an editor and would silently offset and
    /// scale the shape if it were ignored.
    #[test]
    fn a_group_transform_is_resolved_rather_than_ignored() {
        let transformed = Outline::from_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
               <g transform="translate(20 30) scale(2)">
               <path d="M 4,1 L 6,1 L 6,9 L 4,9 Z"/></g></svg>"#,
        )
        .unwrap();

        assert_eq!(
            transformed.area(),
            16.0 * 4.0,
            "the scale reaches the ring, and the translate leaves it alone"
        );
    }

    /// Guides and construction marks left in a drawing must not be mistaken
    /// for the shape.
    #[test]
    fn the_largest_closed_shape_wins() {
        let with_marks = Outline::from_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20">
               <path d="M 1,1 L 2,1 L 2,2 Z"/>
               <path d="M 4,1 L 6,1 L 6,9 L 4,9 Z"/></svg>"#,
        )
        .unwrap();
        assert_eq!(with_marks.area(), 16.0);
    }

    #[test]
    fn a_document_with_nothing_closed_in_it_is_an_error() {
        assert!(
            Outline::from_svg(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
                   <path d="M 1,1 L 9,9" stroke="black"/></svg>"#,
            )
            .is_err()
        );
    }

    /// Curves are not in the outlines committed today, but the loader has to
    /// take a file that has them. A disc pins both halves of that: every
    /// point has to land on the circle to within the tolerance the flattener
    /// promises, and the area has to be the disc's rather than that of the
    /// four chords a curve cut across would leave.
    #[test]
    fn curves_are_flattened_to_within_their_tolerance() {
        const R: f64 = 40.0;
        let disc = Outline::from_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
               <circle cx="50" cy="50" r="40"/></svg>"#,
        )
        .unwrap();

        // The topmost point anchors the frame, so the center ends up `R`
        // along +X from the origin.
        let center = Point2::new(R, 0.0);
        let worst = disc
            .points
            .iter()
            .map(|p| (distance(*p, center) - R).abs())
            .fold(0.0f64, f64::max);
        assert!(worst <= FLATTEN_TOLERANCE, "strays {worst} from the circle");

        // A polyline inscribed in a circle always falls short of it, so the
        // area is bounded on one side by the disc and on the other by how far
        // that tolerance can pull a chord in.
        let exact = std::f64::consts::PI * R * R;
        assert!(
            (exact - disc.area()) / exact < 0.01,
            "got {} for a disc of {exact}",
            disc.area()
        );
        assert!(disc.area() < exact, "and never overshoots it");
    }

    // ─── Filling ────────────────────────────────────────────────────

    fn filled(outline: &Outline) -> MeshData {
        outline_mesh(outline, outline.area() / 32.0).expect("a square fills")
    }

    #[test]
    fn a_fill_is_flat_and_covers_the_outline() {
        let outline = square();
        let mesh = filled(&outline);

        assert!(
            mesh.points.iter().all(|p| p[2] == 0.0),
            "every point lies on the XY plane"
        );
        assert_eq!(
            mesh.face_vertex_counts.iter().sum::<i32>() as usize,
            mesh.face_vertex_indices.len(),
        );
        assert!(mesh.face_vertex_counts.iter().all(|c| *c == 3));

        let area: f32 = faces(&mesh)
            .map(|face| face_normal(&mesh, face).length() / 2.0)
            .sum();
        assert!(
            (area - outline.area() as f32).abs() < 1e-3,
            "the triangles add up to the outline's area, got {area}"
        );
    }

    /// Wound the wrong way, a leaf would be invisible from the side it is
    /// meant to be seen from.
    #[test]
    fn every_face_winds_counter_clockwise_seen_from_above() {
        let mesh = filled(&square());
        for (i, face) in faces(&mesh).enumerate() {
            assert!(
                face_normal(&mesh, face).z > 0.0,
                "face {i} faces +Z, got {:?}",
                face_normal(&mesh, face)
            );
        }
    }

    #[test]
    fn a_finer_limit_spends_more_triangles() {
        let outline = square();
        let coarse = outline_mesh(&outline, outline.area() / 8.0).unwrap();
        let fine = outline_mesh(&outline, outline.area() / 200.0).unwrap();
        assert!(
            fine.face_vertex_counts.len() > coarse.face_vertex_counts.len() * 4,
            "{} vs {}",
            fine.face_vertex_counts.len(),
            coarse.face_vertex_counts.len()
        );
    }

    /// A concave shape is the whole reason the fill is constrained rather
    /// than a plain Delaunay triangulation of the ring's points: the notch
    /// must stay empty instead of being webbed over.
    #[test]
    fn a_notch_stays_outside_the_fill() {
        // A chevron: two arms with a deep notch between them, drawn hanging
        // from the middle of its top edge.
        let chevron = Outline::from_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20">
               <path d="M 4,2 L 16,2 L 16,18 L 13,18 L 10,6 L 7,18 L 4,18 Z"/></svg>"#,
        )
        .unwrap();

        let mesh = outline_mesh(&chevron, chevron.area() / 16.0).unwrap();
        let filled: f32 = faces(&mesh)
            .map(|face| face_normal(&mesh, face).length() / 2.0)
            .sum();
        assert!(
            (filled - chevron.area() as f32).abs() < 1e-2,
            "the fill is the outline's area, not its hull: {filled} vs {}",
            chevron.area()
        );
    }

    /// The identity `outline_mesh` emits faces with: a vertex handle's index
    /// is its position in `vertices()`. Nothing in spade's API states it, and
    /// a change to it would scramble every face silently.
    #[test]
    fn vertex_handles_index_the_order_vertices_are_listed_in() {
        let mut cdt = Cdt::new();
        for p in square().points {
            cdt.insert(p).unwrap();
        }
        for (i, v) in cdt.vertices().enumerate() {
            assert_eq!(v.fix().index(), i);
        }
    }

    /// A ring that crosses itself is a mistake in the drawing. It has to come
    /// back as an error, because spade's bulk load panics on one and an
    /// author system is not a place to panic from.
    #[test]
    fn a_self_crossing_outline_is_an_error_rather_than_a_panic() {
        // A bowtie: the two long edges cross in the middle.
        let bowtie = Outline {
            points: vec![
                Point2::new(0.0, -1.0),
                Point2::new(0.0, 1.0),
                Point2::new(4.0, -1.0),
                Point2::new(4.0, 1.0),
            ],
        };
        let err = outline_mesh(&bowtie, 1.0).expect_err("a bowtie cannot be filled");
        assert!(err.to_string().contains("crosses itself"), "got: {err:#}");
    }
}
