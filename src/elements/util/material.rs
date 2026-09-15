//! How each of the scene's surfaces responds to light.
//!
//! The other half of the palette: [`color`](super::color) says what hue a thing
//! is, and this says what it does with the light that lands on it. They are
//! split because they vary independently — a vine's wood is shaded per mesh so
//! two plants are not the same brown, while every piece of bark in the scene is
//! equally rough.
//!
//! Two numbers is the whole of an opaque material here, because nothing in the
//! scene is metallic and nothing is textured yet: `roughness` says how wide the
//! highlight spreads and `reflectance` says how bright it is, and that is all
//! that separates one untextured organic surface from another.
//!
//! # Not `ior`
//!
//! Bevy reads `StandardMaterial::ior` only on the refraction paths — specular
//! and diffuse transmission. On an opaque dielectric the specular level comes
//! from `reflectance`, so that is what a response authors.

use crate::scene::Surface;

/// How a surface responds to light, with the hue left to
/// [`color`](super::color).
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

/// Dry bark: rough, matte, and barely glinting — shaggy enough that what light
/// it does reflect scatters off in every direction.
pub const WOOD: Response = Response {
    roughness: 0.85,
    reflectance: 0.25,
};

/// Leaves and green canes both. They share one response deliberately — both
/// are living tissue under a waxy cuticle, so their roughness and their sheen
/// genuinely match. They still look nothing alike, because their colour
/// differs, and a blade is additionally thin — see [`Response::blade`].
pub const FOLIAGE: Response = Response {
    roughness: 0.5,
    reflectance: 0.55,
};

/// A trellis post. Smoother than bark and rougher than a leaf, which is where
/// both a planed softwood post and a galvanized steel one sit — neither has a
/// highlight worth naming without a texture to break it up.
pub const POLE: Response = Response {
    roughness: 0.7,
    reflectance: 0.45,
};

/// Dry cultivated loam. The roughest and the least reflective thing in the
/// scene: dust has no sheen at any angle.
pub const GROUND: Response = Response {
    roughness: 0.95,
    reflectance: 0.2,
};

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

    /// The palette is ordered, and the order is the point: if two responses
    /// ever landed on the same roughness, the surfaces they describe would be
    /// distinguished by colour alone.
    #[test]
    fn the_palette_runs_from_the_smoothest_thing_to_the_roughest() {
        let ordered = [FOLIAGE, POLE, WOOD, GROUND];
        for pair in ordered.windows(2) {
            assert!(
                pair[0].roughness < pair[1].roughness,
                "{:?} is smoother than {:?}",
                pair[0],
                pair[1]
            );
        }
        assert!(ordered.iter().all(|r| (0.0..=1.0).contains(&r.roughness)));
    }

    /// Roughness and sheen run together across this palette — the rougher a
    /// surface is here, the drier it is, and the less it reflects. The
    /// coincidence is worth pinning because it is what keeps the two knobs from
    /// cancelling each other out and flattening the scene back to uniform.
    #[test]
    fn the_rougher_a_surface_is_the_less_it_reflects() {
        let ordered = [FOLIAGE, POLE, WOOD, GROUND];
        for pair in ordered.windows(2) {
            assert!(
                pair[0].reflectance > pair[1].reflectance,
                "{:?} reflects more than {:?}",
                pair[0],
                pair[1]
            );
        }
        assert!(ordered.iter().all(|r| (0.0..=1.0).contains(&r.reflectance)));
    }

    /// A blade is the one surface with no inside, and the two flags that fall
    /// out of that go together: one keeps a canopy from vanishing when the
    /// camera goes under it, the other makes it glow when the sun is behind it.
    #[test]
    fn only_a_blade_is_lit_from_behind() {
        let color = [0.1, 0.2, 0.3];
        let solid = FOLIAGE.surface(color);
        let blade = FOLIAGE.blade(color);

        assert!(!solid.double_sided);
        assert_eq!(solid.translucency, 0.0);

        assert!(blade.double_sided);
        assert!(blade.translucency > 0.0);
        // Above 0.5 the shaded side would be the brighter one.
        assert!(blade.translucency < 0.5);

        // Thinness changes nothing else about the response.
        assert_eq!(blade.color, color);
        assert_eq!(blade.roughness, FOLIAGE.roughness);
        assert_eq!(blade.reflectance, FOLIAGE.reflectance);
    }
}
