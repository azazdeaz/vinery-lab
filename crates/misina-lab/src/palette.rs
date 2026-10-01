//! Colour and surface response: what a mesh carries besides its shape.
//!
//! Colour travels with the geometry rather than in a material, and the split
//! is deliberate: the hue lives on the mesh, while a [`Response`] says only
//! how the surface responds to light. Both consumers read the colour that way
//! — Bevy draws it, and USD carries it as a `displayColor` primvar — so the
//! viewer and the export agree by construction.
//!
//! The named colours and responses — a leaf, a post, bark — are a generator's
//! own; vinerylab keeps them in `elements/util/{color,material}.rs`. What is
//! here is the machinery those constants go through: [`srgb`] into linear,
//! [`mix`] between two, [`shade`] for the per-variation jitter that keeps a
//! field of clones from reading as one, and [`Response::surface`].
//!
//! # Linear, not sRGB
//!
//! Both consumers want **linear** RGB, and every colour worth choosing is
//! chosen in sRGB — a hex triple off a picker. So a palette is written as hex
//! and run through [`srgb`] on the way out. Using the sRGB values
//! directly would wash the scene out: mid-grey `#808080` is 0.5 in sRGB and
//! 0.216 linear, and the error runs the same direction on every dark colour,
//! which is all of them in a field.
//!
//! # Not `ior`
//!
//! Bevy reads `StandardMaterial::ior` only on the refraction paths — specular
//! and diffuse transmission. On an opaque dielectric the specular level comes
//! from `reflectance`, so that is what a response authors.

use crate::rng::Rng;
use crate::scene::Surface;

/// Salt splitting a variation's colour off its shape's random stream, so
/// retuning the palette never reshapes the geometry underneath it. An
/// arbitrary odd constant; only its fixedness matters.
pub const COLOR_STREAM: u64 = 0x6C8E_9CF5_7003_2B31;

// ─── Conversion ─────────────────────────────────────────────────────

/// An `0xRRGGBB` sRGB triple as the linear RGB both consumers want.
pub fn srgb(hex: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| linear(((hex >> shift) & 0xFF) as f32 / 255.0))
}

/// `a` blended `t` of the way toward `b`, channel by channel. Both linear,
/// which is where a blend is right — mixing sRGB values darkens the middle.
pub fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
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

// ─── Response ───────────────────────────────────────────────────────

/// How a surface responds to light, with the hue left to the mesh.
///
/// Two numbers is the whole of an opaque material here, because nothing in a
/// scene is metallic and nothing is textured yet: `roughness` says how wide
/// the highlight spreads and `reflectance` says how bright it is, and that is
/// all that separates one untextured organic surface from another.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Response {
    /// Microfacet roughness. 0 is a mirror, 1 is chalk.
    pub roughness: f32,
    /// Specular intensity, on a linear 0..1 scale where 0.5 is the 4% every
    /// ordinary dielectric reflects. Low is a dry, dusty surface; high is one
    /// under a wax or a varnish.
    pub reflectance: f32,
}

impl Response {
    /// This response, on a surface of `color`.
    pub fn surface(&self, color: [f32; 3]) -> Surface {
        Surface {
            color,
            roughness: self.roughness,
            reflectance: self.reflectance,
            translucency: 0.0,
            thickness: 0.0,
            double_sided: false,
        }
    }

    /// The same, for a leaf blade: a surface thin enough to have no inside.
    ///
    /// Two consequences, both of them thinness: it is lit and drawn from behind
    /// as well, since a canopy is looked up into as often as down onto, and it
    /// passes light through rather than stopping it, so a backlit blade glows
    /// instead of going black.
    pub fn blade(&self, color: [f32; 3]) -> Surface {
        Surface {
            double_sided: true,
            translucency: BLADE_TRANSLUCENCY,
            thickness: BLADE_THICKNESS,
            ..self.surface(color)
        }
    }
}

/// How much of the light landing on a blade passes through it rather than
/// reflecting off it. Kept under 0.5, above which the shaded side of a leaf
/// would out-glow the lit side — tissue paper, not a canopy.
const BLADE_TRANSLUCENCY: f32 = 0.45;

/// A blade's thickness in meters. Sets how far behind the surface the
/// transmitted lobe is sampled from, which at leaf scale matters only to a
/// nearby point light; it is authored anyway so the number is right when one
/// arrives.
const BLADE_THICKNESS: f32 = 0.000_3;

#[cfg(test)]
mod tests {
    use super::*;

    /// A deep, slightly blue-shifted green: vinerylab's leaf.
    const LEAF: u32 = 0x3E6B2A;
    /// A grey-brown: vinerylab's bark.
    const WOOD: u32 = 0x5A4A38;

    /// The three points the sRGB transfer function is pinned at. Mid-grey is
    /// the one that matters: 0.5 in sRGB is 0.216 linear, so a conversion left
    /// out entirely — or applied the wrong way round — shows up here and
    /// nowhere else in a palette of earth tones, where every colour is dark
    /// enough that the error just reads as "a bit off".
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

    /// Channels must not get shuffled on the way through. Every entry of an
    /// earth-tone palette is muted, so a red/blue swap survives every other
    /// check here while turning the whole scene lurid.
    #[test]
    fn srgb_keeps_its_channels_in_order() {
        let [r, g, b] = srgb(0xFF7F00);
        assert!(r > g && g > b, "got {r}, {g}, {b}");
    }

    #[test]
    fn mix_runs_from_one_end_to_the_other_and_no_further() {
        let (a, b) = (srgb(LEAF), srgb(WOOD));
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 7.0), b, "clamped");
        let mid = mix(a, b, 0.5);
        assert!((0..3).all(|i| (mid[i] - (a[i] + b[i]) / 2.0).abs() < 1e-6));
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

    /// A blade is the one surface with no inside, and the two flags that fall
    /// out of that go together: one keeps a canopy from vanishing when the
    /// camera goes under it, the other makes it glow when the sun is behind it.
    #[test]
    fn only_a_blade_is_lit_from_behind() {
        let foliage = Response {
            roughness: 0.5,
            reflectance: 0.55,
        };
        let color = [0.1, 0.2, 0.3];
        let solid = foliage.surface(color);
        let blade = foliage.blade(color);

        assert!(!solid.double_sided);
        assert_eq!(solid.translucency, 0.0);

        assert!(blade.double_sided);
        assert!(blade.translucency > 0.0);
        // Above 0.5 the shaded side would be the brighter one.
        assert!(blade.translucency < 0.5);

        // Thinness changes nothing else about the response.
        assert_eq!(blade.color, color);
        assert_eq!(blade.roughness, foliage.roughness);
        assert_eq!(blade.reflectance, foliage.reflectance);
    }
}
