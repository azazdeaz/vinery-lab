//! How each of the vineyard's surfaces responds to light.
//!
//! The other half of the palette: [`color`](super::color) says what hue a
//! thing is, and this says what it does with the light that lands on it. They
//! are split because they vary independently — a vine's wood is shaded per
//! mesh so two plants are not the same brown, while every piece of bark in the
//! scene is equally rough. [`Response`] itself is
//! [`misina_lab::palette`]'s; the ground's response travels with
//! [`terrain`](crate::elements::terrain) into the core.

pub use misina_lab::palette::Response;

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

#[cfg(test)]
mod tests {
    use super::*;
    use misina_lab::terrain::GROUND;

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
}
