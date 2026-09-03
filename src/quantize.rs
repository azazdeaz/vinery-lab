//! Vector quantization: reducing a population of configs to `k` representatives
//! that still cover it.
//!
//! Each layer authors one config per organ, and each distinct config would
//! otherwise need a mesh of its own. The population is reduced to a budget of
//! representatives instead, and every organ draws the mesh of the one it is
//! closest to.
//!
//! # Minimize the maximum, not the average
//!
//! This solves metric **k-center** — smallest worst-case distance — rather than
//! k-means or k-medoids, which minimize a sum. Do not swap it for one of those.
//! A sum-based objective weights a variant by how many organs share it, so a
//! config held by a tenth of a percent of the population contributes a tenth of
//! a percent of the error and is rounded away however different it looks.
//! k-center is density-blind: one config far from everything costs as much as a
//! cluster of ten thousand. That is what keeps a handful of diseased leaves in
//! a healthy vineyard — provided the [`Metric`] puts them far away, which is
//! the metric's business and not this module's.
//!
//! k-center is NP-hard; Gonzalez's farthest-first traversal is a
//! 2-approximation, and nothing polynomial does better unless P=NP.
//!
//! # Determinism
//!
//! The same params must give the same scene on every machine and every run — a
//! downstream Isaac Lab cache is keyed on it. [`farthest_first`] starts from
//! index 0 and breaks ties toward the lowest index, so the codebook is a
//! function of the input slice alone.
//!
//! The slice's *order* is the caller's to fix, and Bevy's query iteration order
//! is not stable across runs. **Collect with an explicit key and sort before
//! calling in here.**

/// A distance over `T`, deciding which configs count as similar enough to
/// share a mesh.
///
/// Implemented on a separate type rather than on the config, so that one
/// config can be measured several ways — a loose metric for a distant
/// level of detail, a strict one for the export — and so the weights have
/// somewhere to live when they stop being constants.
pub trait Metric<T> {
    /// The distance between two configs.
    ///
    /// Only the *ordering* of the returned values matters to
    /// [`farthest_first`], so a squared distance is a legitimate
    /// implementation — but then [`Codebook::radius`] comes back in squared
    /// units, and the 2-approximation guarantee needs a real metric:
    /// symmetric, zero only on equal inputs, and obeying the triangle
    /// inequality.
    ///
    /// The recipe that gives one of those for free is to warp each field with
    /// a monotone function, take the absolute difference, and combine the
    /// axes with L2. The warp is where a field stops being a number and
    /// becomes a decision: a step at zero turns "has any disease at all" into
    /// a categorical jump, which is what makes k-center spend a slot on it.
    fn distance(&self, a: &T, b: &T) -> f32;
}

/// The result of quantizing a population: the representatives, and which one
/// each input drew.
#[derive(Clone, Debug)]
pub struct Codebook<T> {
    /// The chosen representatives, **in selection order**.
    ///
    /// Prefixes are valid smaller codebooks — `representatives[..j]` is what a
    /// budget of `j` would have produced, because the traversal is greedy and
    /// never revisits a pick. Nothing uses that yet; it is what a
    /// level-of-detail ladder would be built from without a second pass.
    pub representatives: Vec<T>,
    /// For each input, by index, which representative it drew.
    pub assignment: Vec<u32>,
    /// The largest distance from any input to the representative it drew.
    ///
    /// The honest readout of what a budget cost: no organ in the scene ended
    /// up further than this from the one that was actually built. Tuning
    /// against it beats tuning against `k`, because it is in the metric's own
    /// units rather than in slots.
    pub radius: f32,
}

impl<T> Codebook<T> {
    pub fn len(&self) -> usize {
        self.representatives.len()
    }

    pub fn is_empty(&self) -> bool {
        self.representatives.is_empty()
    }

    /// The representative input `index` drew.
    pub fn representative(&self, index: usize) -> &T {
        &self.representatives[self.assignment[index] as usize]
    }
}

/// Gonzalez's farthest-first traversal: pick any config, then repeatedly pick
/// the one furthest from every representative chosen so far.
///
/// Stops at `k` representatives, or earlier once no input is further than
/// `max_radius` from the one it drew. Pass `0.0` for `max_radius` to spend the
/// whole budget — it still stops early when everything left is an exact
/// duplicate of something already chosen, which is the difference between a
/// codebook of `k` entries and a codebook of `k` *useful* ones.
///
/// `O(n·k)` with one [`Metric::distance`] call per pair, and the inner loop is
/// a plain scan: at a few tens of thousands of configs against a budget in the
/// single digits this is well under a millisecond, which is why it is not
/// parallelized. The place to start if that changes is the inner loop, as a
/// `rayon` map-reduce per round — the tie-breaking rule below survives it,
/// since a tree reduction that keeps the left operand on a tie still lands on
/// the lowest index.
pub fn farthest_first<T, M>(items: &[T], k: usize, max_radius: f32, metric: &M) -> Codebook<T>
where
    T: Clone,
    M: Metric<T>,
{
    let k = k.min(items.len());
    if k == 0 {
        // A real state, not a caller error: a parcel can solve to no rows, and
        // a row can draw no plants.
        return Codebook {
            representatives: Vec::new(),
            assignment: Vec::new(),
            radius: 0.0,
        };
    }
    let max_radius = max_radius.max(0.0);

    let mut chosen: Vec<usize> = Vec::with_capacity(k);
    let mut assignment = vec![0u32; items.len()];
    // Each input's distance to the nearest representative chosen so far.
    let mut nearest = vec![f32::INFINITY; items.len()];
    let mut radius = f32::INFINITY;

    // Index 0 rather than a random draw — see the module docs on determinism.
    let mut next = 0usize;

    for rank in 0..k {
        chosen.push(next);
        let representative = &items[next];

        let mut farthest = 0usize;
        let mut farthest_distance = f32::NEG_INFINITY;
        for (i, item) in items.iter().enumerate() {
            let d = metric.distance(representative, item);
            if d < nearest[i] {
                nearest[i] = d;
                assignment[i] = rank as u32;
            }
            // Strictly greater, so a tie leaves `farthest` on the lower index.
            if nearest[i] > farthest_distance {
                farthest_distance = nearest[i];
                farthest = i;
            }
        }

        radius = farthest_distance;
        if radius <= max_radius {
            break;
        }
        next = farthest;
    }

    Codebook {
        representatives: chosen.into_iter().map(|i| items[i].clone()).collect(),
        assignment,
        radius,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for an element config: two fields that vary continuously and
    /// one that is categorically far, which is the shape every real metric in
    /// this crate has.
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Sample {
        size: f32,
        age: f32,
        disease: f32,
    }

    struct SampleMetric;

    impl Metric<Sample> for SampleMetric {
        fn distance(&self, a: &Sample, b: &Sample) -> f32 {
            // Any disease at all is a categorical jump, an order of magnitude
            // past the whole range the healthy fields span; beyond the jump
            // the amount matters continuously.
            let warp = |x: f32| if x > 0.0 { 10.0 + x } else { 0.0 };
            [
                a.size - b.size,
                a.age - b.age,
                warp(a.disease) - warp(b.disease),
            ]
            .iter()
            .map(|d| d * d)
            .sum::<f32>()
            .sqrt()
        }
    }

    /// `healthy` samples spread across the unit square of size and age, plus
    /// `sick` diseased ones — the rare variant a density-weighted method
    /// would round away.
    fn population(healthy: usize, sick: usize) -> Vec<Sample> {
        let mut items: Vec<Sample> = (0..healthy)
            .map(|i| {
                let f = i as f32 / healthy as f32;
                Sample {
                    size: f,
                    age: (f * 7.0).fract(),
                    disease: 0.0,
                }
            })
            .collect();
        items.extend((0..sick).map(|i| Sample {
            size: i as f32 / sick.max(1) as f32,
            age: 0.5,
            disease: 0.4,
        }));
        items
    }

    /// Brute-force distance from each input to the nearest representative —
    /// what the incremental `nearest` array must agree with.
    fn covering(items: &[Sample], book: &Codebook<Sample>) -> f32 {
        items
            .iter()
            .map(|item| {
                book.representatives
                    .iter()
                    .map(|r| SampleMetric.distance(r, item))
                    .fold(f32::INFINITY, f32::min)
            })
            .fold(f32::NEG_INFINITY, f32::max)
    }

    /// The reason this module is k-center and not k-means. One diseased sample
    /// in a thousand is a tenth of a percent of the population and must still
    /// take a slot, because the metric puts it far away.
    #[test]
    fn a_rare_variant_wins_a_representative() {
        let items = population(1000, 1);
        let book = farthest_first(&items, 8, 0.0, &SampleMetric);

        assert_eq!(book.len(), 8);
        assert!(
            book.representatives.iter().any(|r| r.disease > 0.0),
            "the one diseased sample in a thousand took a slot"
        );
        assert!(
            book.representatives.iter().filter(|r| r.disease > 0.0).count() == 1,
            "and only the one, since there is only one to cover"
        );
    }

    /// Every diseased sample must draw a diseased representative — winning a
    /// slot is worth nothing if the assignment still sends it to a healthy one.
    #[test]
    fn a_rare_variant_draws_its_own_representative() {
        let items = population(1000, 3);
        let book = farthest_first(&items, 8, 0.0, &SampleMetric);

        for (i, item) in items.iter().enumerate() {
            assert_eq!(
                book.representative(i).disease > 0.0,
                item.disease > 0.0,
                "sample {i} drew across the categorical jump"
            );
        }
    }

    #[test]
    fn the_same_population_gives_the_same_codebook() {
        let items = population(200, 2);
        let a = farthest_first(&items, 12, 0.0, &SampleMetric);
        let b = farthest_first(&items, 12, 0.0, &SampleMetric);

        assert_eq!(a.representatives, b.representatives);
        assert_eq!(a.assignment, b.assignment);
        assert_eq!(a.radius, b.radius);
    }

    /// Nothing may draw a representative that isn't its nearest — the
    /// incremental update has to match what a brute-force search would say.
    #[test]
    fn every_input_draws_its_nearest_representative() {
        let items = population(300, 2);
        let book = farthest_first(&items, 10, 0.0, &SampleMetric);
        assert_eq!(book.assignment.len(), items.len());

        for (i, item) in items.iter().enumerate() {
            let drew = SampleMetric.distance(book.representative(i), item);
            let best = book
                .representatives
                .iter()
                .map(|r| SampleMetric.distance(r, item))
                .fold(f32::INFINITY, f32::min);
            assert!(
                (drew - best).abs() < 1e-6,
                "sample {i} drew at {drew}, nearest was {best}"
            );
        }
    }

    /// `radius` is the tuning knob, so it has to be the true covering radius
    /// rather than whatever the last round happened to leave behind.
    #[test]
    fn radius_is_the_true_covering_radius() {
        let items = population(300, 2);
        for k in [1, 4, 16] {
            let book = farthest_first(&items, k, 0.0, &SampleMetric);
            let brute = covering(&items, &book);
            assert!(
                (book.radius - brute).abs() < 1e-5,
                "k={k}: reported {}, actual {brute}",
                book.radius
            );
        }
    }

    /// The traversal is greedy and never revisits a pick, so a small budget's
    /// codebook is the prefix of a large one's. A level-of-detail ladder is
    /// built on this.
    #[test]
    fn a_smaller_budget_is_a_prefix_of_a_larger_one() {
        let items = population(300, 2);
        let small = farthest_first(&items, 4, 0.0, &SampleMetric);
        let large = farthest_first(&items, 16, 0.0, &SampleMetric);

        assert_eq!(small.representatives, large.representatives[..4]);
    }

    /// Spending slots on exact duplicates would leave a scene with `k`
    /// prototypes and fewer than `k` shapes.
    #[test]
    fn duplicates_never_take_a_second_slot() {
        let distinct = population(3, 0);
        let items: Vec<Sample> = distinct.iter().cycle().take(90).copied().collect();

        let book = farthest_first(&items, 10, 0.0, &SampleMetric);
        assert_eq!(book.len(), 3, "one slot per distinct sample and no more");
        assert_eq!(book.radius, 0.0, "and the cover is exact");
    }

    /// The other way to spend a budget: ask for a tolerance instead of a
    /// count, and take however many representatives that needs.
    #[test]
    fn max_radius_stops_the_traversal_early() {
        let items = population(500, 0);
        let loose = farthest_first(&items, 64, 0.25, &SampleMetric);

        assert!(loose.len() < 64, "stopped short of the budget");
        assert!(
            loose.radius <= 0.25,
            "and stopped because the tolerance was met, at {}",
            loose.radius
        );
    }

    #[test]
    fn an_empty_population_yields_an_empty_codebook() {
        let book = farthest_first::<Sample, _>(&[], 8, 0.0, &SampleMetric);
        assert!(book.is_empty() && book.assignment.is_empty());
    }

    /// A budget past the population size is not an error — a row with two
    /// plants in it does not need eight prototypes.
    #[test]
    fn a_budget_larger_than_the_population_is_clamped() {
        let items = population(3, 0);
        let book = farthest_first(&items, 50, 0.0, &SampleMetric);
        assert_eq!(book.len(), 3);
    }
}
