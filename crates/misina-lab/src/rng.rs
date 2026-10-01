//! The seeded random stream every element draws from.

/// SplitMix64. Inlined rather than pulled from `rand` because the only
/// requirement is that the same seed gives the same scene on every machine
/// and every run, which a fixed algorithm guarantees and a crate's default
/// generator does not promise across versions.
fn split_mix_64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Spreads a counter across the whole 64-bit range, so neighbouring indices
/// seed unrelated streams instead of the same one shifted by a step.
///
/// What a layer salts its streams with: a representative's mesh is built from
/// `seed ^ salt(index)`, and an instance's placement draws from
/// `seed ^ LAYER_STREAM ^ salt(order)`.
pub fn salt(index: u64) -> u64 {
    index.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// A deterministic stream of floats, for the shape and scatter randomness
/// elements need beyond picking a variation index.
///
/// Same inlined [`split_mix_64`] as the variation picker, for the same
/// reason: a fixed algorithm is what makes the scene reproducible across
/// machines and crate versions.
///
/// The *draw order* of a stream is part of an element's output. Inserting a
/// draw in the middle of a build re-rolls everything downstream of it, so
/// elements document their draw order where a reader would otherwise be
/// tempted to reorder it.
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next draw, uniform in `0.0..1.0`.
    pub fn unit(&mut self) -> f64 {
        // Top 53 bits, the mantissa width of an f64, for a uniform unit float.
        (split_mix_64(&mut self.state) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// The next draw, uniform in `lo..hi`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.unit() * (hi - lo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draws(seed: u64, n: usize) -> Vec<f64> {
        let mut rng = Rng::new(seed);
        (0..n).map(|_| rng.unit()).collect()
    }

    #[test]
    fn rng_produces_the_same_stream_for_the_same_seed() {
        assert_eq!(draws(7, 32), draws(7, 32));
        assert_ne!(draws(7, 32), draws(8, 32));
    }

    #[test]
    fn rng_stays_within_its_range() {
        let mut rng = Rng::new(3);
        assert!((0..256).all(|_| (0.0..1.0).contains(&rng.unit())));
        assert!((0..256).all(|_| (-2.0..5.0).contains(&rng.range(-2.0, 5.0))));
    }

    /// A stream has to actually spread out, not sit near one value — a
    /// broken shift or divisor would still pass the range check above.
    #[test]
    fn rng_draws_spread_across_the_unit_interval() {
        let d = draws(11, 512);
        let mean = d.iter().sum::<f64>() / d.len() as f64;
        assert!((mean - 0.5).abs() < 0.05, "mean {mean} is near 0.5");
        assert!(d.iter().any(|v| *v < 0.05) && d.iter().any(|v| *v > 0.95));
    }
}
