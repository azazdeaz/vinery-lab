//! Points scattered over a [`Band`] of ground.
//!
//! One rule for everything placed *within* a zone rather than along a line:
//! ground-cover tiles down an alley, weed slots in the strip under a row,
//! blades inside a tile. A jittered grid rather than dart throwing — it is
//! `O(n)`, never lands two points on one spot at a jitter under half a cell,
//! and gives every point a stable slot to be named by.

use bevy::math::Vec2;

use crate::rng::Rng;

/// A strip of ground: a centerline in plan view and how far it reaches either
/// side of it.
///
/// The strip under a row is one, the alley between two rows is one, and so
/// is a tile of ground cover — anything placed *within* a zone rather than
/// *along* a line takes its frame from here. See [`jittered_grid`].
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

/// One grid cell's point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    /// The cell, counted along the band first and across it second. Stable
    /// for a given band and spacing, so an element can name a point by it.
    pub index: u32,
    /// Where the point landed, in plan view.
    pub position: Vec2,
}

/// A grid of about `spacing` over `band`, each point nudged off its cell's
/// centre by up to `jitter` cells along and across.
///
/// Cells are stretched to fill the band exactly, so the spacing is a target
/// rather than a promise. Two draws per cell, along then across, taken for
/// every cell whether or not the caller keeps it — so thinning a scatter
/// leaves the survivors where they were. At a jitter of a half or less every
/// point stays inside its own cell, and so inside the band.
pub fn jittered_grid(band: &Band, spacing: f32, jitter: f32, rng: &mut Rng) -> Vec<Slot> {
    let spacing = spacing.max(1e-3);
    let along_cells = ((band.length() / spacing).floor() as u32).max(1);
    let across_cells = ((2.0 * band.half_width / spacing).round() as u32).max(1);
    let step = Vec2::new(
        band.length() / along_cells as f32,
        2.0 * band.half_width / across_cells as f32,
    );
    let (dir, across) = (band.direction(), band.across());
    let jitter = jitter as f64;

    let mut slots = Vec::with_capacity((along_cells * across_cells) as usize);
    for i in 0..along_cells {
        for j in 0..across_cells {
            let nudge_along = rng.range(-jitter, jitter) as f32;
            let nudge_across = rng.range(-jitter, jitter) as f32;
            let along = (i as f32 + 0.5 + nudge_along) * step.x;
            let off = (j as f32 + 0.5 + nudge_across) * step.y - band.half_width;
            slots.push(Slot {
                index: i * across_cells + j,
                position: band.start + dir * along + across * off,
            });
        }
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band() -> Band {
        Band {
            start: Vec2::new(1.0, 2.0),
            end: Vec2::new(11.0, 2.0),
            half_width: 1.0,
        }
    }

    #[test]
    fn the_grid_fills_the_band_at_about_the_spacing() {
        let slots = jittered_grid(&band(), 0.5, 0.4, &mut Rng::new(1));
        // Ten metres by two, at half-metre cells.
        assert_eq!(slots.len(), 20 * 4);
        assert!(slots.iter().all(|s| band().contains(s.position)));
        let indices: Vec<u32> = slots.iter().map(|s| s.index).collect();
        assert_eq!(indices, (0..80).collect::<Vec<_>>(), "slots count in order");
    }

    #[test]
    fn a_jittered_point_stays_in_its_own_cell() {
        let still = jittered_grid(&band(), 0.5, 0.0, &mut Rng::new(1));
        let moved = jittered_grid(&band(), 0.5, 0.5, &mut Rng::new(1));
        for (a, b) in still.iter().zip(&moved) {
            let d = (a.position - b.position).abs();
            assert!(d.x <= 0.25 + 1e-6 && d.y <= 0.25 + 1e-6, "{a:?} vs {b:?}");
        }
        assert!(
            still
                .iter()
                .zip(&moved)
                .any(|(a, b)| a.position != b.position),
            "the jitter moves something"
        );
    }

    #[test]
    fn the_scatter_is_deterministic_and_follows_the_band() {
        let a = jittered_grid(&band(), 0.5, 0.3, &mut Rng::new(9));
        let b = jittered_grid(&band(), 0.5, 0.3, &mut Rng::new(9));
        assert_eq!(a, b);

        let turned = Band {
            start: Vec2::ZERO,
            end: Vec2::new(0.0, 10.0),
            half_width: 1.0,
        };
        let slots = jittered_grid(&turned, 0.5, 0.3, &mut Rng::new(9));
        assert!(slots.iter().all(|s| turned.contains(s.position)));
        assert!(slots.iter().all(|s| s.position.x.abs() <= 1.0));
    }

    /// A band too short or narrow for a whole cell still gets one, on its
    /// centreline: nothing placed in it is better than a division by zero.
    #[test]
    fn a_tiny_band_gets_a_single_cell() {
        let tiny = Band {
            start: Vec2::ZERO,
            end: Vec2::new(0.1, 0.0),
            half_width: 0.05,
        };
        let slots = jittered_grid(&tiny, 1.0, 0.0, &mut Rng::new(1));
        assert_eq!(slots.len(), 1);
        assert!((slots[0].position - Vec2::new(0.05, 0.0)).length() < 1e-6);
    }
}
