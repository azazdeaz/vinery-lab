//! The palette, and the per-variation jitter that keeps a parcel of clones
//! from reading as one.
//!
//! Colour reaches USD as `primvars:displayColor` on the geometry rather than
//! as a value baked into a material, and that split is deliberate: an
//! attribute composes through every arc unconditionally, where a material
//! binding is a relationship and can be dropped — see
//! [`material`](super::material). It is also the only thing the viewer draws:
//! `usd_bevy` reads `displayColor` into a vertex-colour attribute, but bakes
//! every instanced prototype with a default material. So the hue lives here
//! and on the mesh, and a material only says how the surface responds to
//! light.
//!
//! # Linear, not sRGB
//!
//! `displayColor` is **linear** RGB, and every colour worth choosing is chosen
//! in sRGB — a hex triple off a picker. So the palette is written as hex and
//! run through [`srgb`] on the way to the stage. Authoring the sRGB values
//! directly would wash the scene out: mid-grey `#808080` is 0.5 in sRGB and
//! 0.216 linear, and the error runs the same direction on every dark colour,
//! which is all of them here.

use crate::elements::Rng;

/// Salt splitting a variation's colour off its shape's random stream, so
/// retuning the palette never reshapes the geometry underneath it — the same
/// split [`vine`](crate::elements::vine) keeps between its wood and its
/// shoots. An arbitrary odd constant; only its fixedness matters.
pub const COLOR_STREAM: u64 = 0x6C8E_9CF5_7003_2B31;

// ─── The palette ────────────────────────────────────────────────────
//
// sRGB hex, as drawn. Pass through `srgb` at the point of use.

/// A mature blade, seen from above. Grapevine leaves are a deep, slightly
/// blue-shifted green; the yellower flush of a young one is not modelled.
pub const LEAF: u32 = 0x3E6B2A;

/// This season's growth — the green, unlignified shoot. Lighter and yellower
/// than a blade, which is what separates a stem from the canopy hanging off it
/// without either needing a material of its own.
pub const CANE: u32 = 0x6E8B3D;

/// Permanent wood: trunk, cordons, spurs. Grey-brown shaggy bark.
pub const WOOD: u32 = 0x5A4A38;

/// Bare cultivated ground between the rows. Dry loam, no cover crop yet.
pub const GROUND: u32 = 0x6B5744;

// ─── Conversion ─────────────────────────────────────────────────────

/// An `0xRRGGBB` sRGB triple as the linear RGB `displayColor` wants.
pub fn srgb(hex: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| linear(((hex >> shift) & 0xFF) as f32 / 255.0))
}

/// One channel of the inverse sRGB transfer function.
fn linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

// ─── Variation ──────────────────────────────────────────────────────

/// How far a variation's colour may drift from its palette entry in overall
/// value, as a fraction. Wide enough to read across a row at a glance, narrow
/// enough that no variation stops looking like the thing it is.
const VALUE_JITTER: f64 = 0.18;

/// How far it may drift warm or cool, as a fraction applied in opposite
/// directions to red and blue. Smaller than the value jitter because hue is
/// what says *what* a surface is — a leaf that drifted this far twice over
/// would stop reading as green.
const HUE_JITTER: f64 = 0.06;

/// One variation's take on a palette entry, as linear RGB.
///
/// Two draws, in this order: the overall value, then the warm/cool tilt.
/// Callers pass an [`Rng`] salted with [`COLOR_STREAM`] and their own
/// per-variation seed, so the drift is deterministic and independent of every
/// other stream the element runs.
pub fn shade(base: [f32; 3], rng: &mut Rng) -> [f32; 3] {
    let value = rng.range(1.0 - VALUE_JITTER, 1.0 + VALUE_JITTER) as f32;
    let warm = rng.range(-HUE_JITTER, HUE_JITTER) as f32;
    [
        base[0] * value * (1.0 + warm),
        base[1] * value,
        base[2] * value * (1.0 - warm),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three points the sRGB transfer function is pinned at. Mid-grey is
    /// the one that matters: 0.5 in sRGB is 0.216 linear, so a conversion left
    /// out entirely — or applied the wrong way round — shows up here and
    /// nowhere else in the palette, where every colour is dark enough that the
    /// error just reads as "a bit off".
    #[test]
    fn srgb_lands_on_the_transfer_functions_fixed_points() {
        assert_eq!(srgb(0x000000), [0.0, 0.0, 0.0]);
        for c in srgb(0xFFFFFF) {
            assert!((c - 1.0).abs() < 1e-6, "white stays white, got {c}");
        }
        for c in srgb(0x808080) {
            assert!(
                (c - 0.2158).abs() < 1e-3,
                "mid-grey is 0.216 linear, got {c}"
            );
        }
    }

    /// Channels must not get shuffled on the way through. Every palette entry
    /// is a muted earth tone, so a red/blue swap survives every other check
    /// here while turning the whole scene lurid.
    #[test]
    fn srgb_keeps_its_channels_in_order() {
        let [r, g, b] = srgb(0xFF7F00);
        assert!(r > g && g > b, "got {r}, {g}, {b}");
    }

    fn drift(base: u32, seed: u64) -> [f32; 3] {
        shade(srgb(base), &mut Rng::new(COLOR_STREAM ^ seed))
    }

    #[test]
    fn a_variations_shade_is_deterministic() {
        assert_eq!(drift(LEAF, 3), drift(LEAF, 3));
        assert_ne!(drift(LEAF, 3), drift(LEAF, 4));
    }

    /// The jitter has to be visible without being a recolour: a leaf that
    /// drifted far enough would stop being green, and the whole point is that
    /// a row of clones stops looking like one.
    #[test]
    fn a_shade_stays_recognisably_its_palette_entry() {
        let base = srgb(LEAF);
        for seed in 0..64 {
            let shaded = drift(LEAF, seed);
            for (i, c) in shaded.iter().enumerate() {
                assert!(*c > 0.0, "seed {seed} channel {i} went non-positive: {c}");
                let ratio = c / base[i];
                assert!(
                    (0.7..1.3).contains(&ratio),
                    "seed {seed} channel {i} drifted to {ratio}× the palette"
                );
            }
            // Still a green: more green than either of the other two.
            assert!(
                shaded[1] > shaded[0] && shaded[1] > shaded[2],
                "seed {seed} stopped reading as green: {shaded:?}"
            );
        }
    }

    /// Sixty-four draws off one stream must not collapse onto a handful of
    /// values — the failure a fixed seed, or a stream reset per call, would
    /// produce.
    #[test]
    fn successive_shades_actually_differ() {
        let mut rng = Rng::new(COLOR_STREAM);
        let base = srgb(WOOD);
        let shades: Vec<[f32; 3]> = (0..64).map(|_| shade(base, &mut rng)).collect();
        for (i, a) in shades.iter().enumerate() {
            for b in shades.iter().skip(i + 1) {
                assert_ne!(a, b, "two draws came out identical");
            }
        }
    }
}
