//! How each of the scene's surfaces responds to light.
//!
//! The other half of the palette: [`color`](super::color) says what hue a thing
//! is, and this says what it does with the light that lands on it. They are
//! split because they vary independently — a vine's wood is shaded per mesh so
//! two plants are not the same brown, while every piece of bark in the scene is
//! equally rough.
//!
//! Two numbers is the whole of a material here, because nothing in the scene is
//! metallic and nothing is textured yet: `roughness` and `ior` are all that
//! separate one untextured organic surface from another.

use crate::scene::Surface;

/// How a surface responds to light, with the hue left to
/// [`color`](super::color).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Response {
    /// Microfacet roughness. 0 is a mirror, 1 is chalk.
    pub roughness: f32,
    /// Index of refraction, which sets how bright the specular highlight is at
    /// a glancing angle.
    pub ior: f32,
}

impl Response {
    /// This response, on a surface of `color`.
    pub fn surface(&self, color: [f32; 3]) -> Surface {
        Surface {
            color,
            roughness: self.roughness,
            ior: self.ior,
            double_sided: false,
        }
    }

    /// The same, for a surface with no inside — a leaf blade — which has to be
    /// lit and drawn from behind as well, since a canopy is looked up into as
    /// often as down onto.
    pub fn double_sided(&self, color: [f32; 3]) -> Surface {
        Surface {
            double_sided: true,
            ..self.surface(color)
        }
    }
}

/// Dry bark: rough, matte, no sheen at any angle.
pub const WOOD: Response = Response {
    roughness: 0.85,
    ior: 1.5,
};

/// Leaves and green canes both. They share one response deliberately — both
/// are living tissue under a waxy cuticle, so their roughness genuinely
/// matches. They still look nothing alike, because their colour differs.
pub const FOLIAGE: Response = Response {
    roughness: 0.5,
    ior: 1.45,
};

/// A trellis post. Smoother than bark and rougher than a leaf, which is where
/// both a planed softwood post and a galvanized steel one sit — neither has a
/// highlight worth naming without a texture to break it up.
pub const POLE: Response = Response {
    roughness: 0.7,
    ior: 1.5,
};

/// Dry cultivated loam. The roughest thing in the scene.
pub const GROUND: Response = Response {
    roughness: 0.95,
    ior: 1.5,
};

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

    /// A blade is the one surface with no inside, and the flag is what keeps a
    /// canopy from vanishing when the camera goes under it.
    #[test]
    fn only_a_double_sided_surface_is_lit_from_behind() {
        let color = [0.1, 0.2, 0.3];
        assert!(!FOLIAGE.surface(color).double_sided);
        assert!(FOLIAGE.double_sided(color).double_sided);
        assert_eq!(FOLIAGE.double_sided(color).color, color);
        assert_eq!(FOLIAGE.double_sided(color).roughness, FOLIAGE.roughness);
    }
}
