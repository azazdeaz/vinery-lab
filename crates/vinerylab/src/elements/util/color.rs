//! The vineyard's colours, as `0xRRGGBB` sRGB triples.
//!
//! Each is passed through [`srgb`] at the point of use, and usually through
//! [`shade`] as well, so two meshes of one element never come out the same
//! brown; both are [`misina_lab::palette`]'s, re-exported here so an element
//! reads its whole colour story off one module. The ground's colour is not
//! here: it travels with [`terrain`](crate::elements::terrain) into the core.

pub use misina_lab::palette::{COLOR_STREAM, mix, shade, srgb};

/// A mature blade, seen from above. Grapevine leaves are a deep, slightly
/// blue-shifted green; the yellower flush of a young one is not modelled.
pub const LEAF: u32 = 0x3E6B2A;

/// This season's growth — the green, unlignified shoot. Lighter and yellower
/// than a blade, which is what separates a stem from the canopy hanging off it
/// without either needing a material of its own.
pub const CANE: u32 = 0x6E8B3D;

/// Permanent wood: trunk, cordons, spurs. Grey-brown shaggy bark.
pub const WOOD: u32 = 0x5A4A38;

/// A dormant cane: last season's shoot after it lignified and dropped its
/// leaves. Tan rather than the grey-brown of old wood, since one-year bark is
/// smooth and has not weathered yet.
pub const DORMANT_CANE: u32 = 0x8B6A45;

/// A trellis post. Grey with the faintest warm cast — weathered softwood and
/// galvanized steel both land here, and at row distance nothing separates
/// them but their silhouette.
pub const POLE: u32 = 0x8C8981;

/// A trellis wire. Galvanized steel, lighter and cooler than a weathered post
/// — enough to read as a separate thing where one crosses another.
pub const WIRE: u32 = 0xA9ADAD;

/// A living sward: the grass of a grassed alley. Lighter and yellower than a
/// vine blade, which is what separates the floor from the canopy above it.
pub const SWARD: u32 = 0x5F8C3C;

/// The same sward dead on its feet — the straw a Mediterranean alley is from
/// June to the autumn rains. What [`SWARD`] is mixed toward as it dries.
pub const STRAW: u32 = 0xB8A46B;

/// Weed foliage. Between the sward and a vine blade, so a plant in the strip
/// reads as neither.
pub const WEED: u32 = 0x4E7A31;
