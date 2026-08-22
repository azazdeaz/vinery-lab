//! `UsdPreviewSurface` materials, and where a scene made of references is
//! allowed to keep them.
//!
//! # Why a material cannot live in one shared `/Looks` scope
//!
//! `material:binding` is a **relationship**, and relationship targets are
//! namespace-mapped through composition arcs: a target outside the referenced
//! subtree cannot be mapped and USD drops it. That is the same failure
//! [`stage::define_parts_library`] documents for a `PointInstancer`'s
//! `prototypes`, and it bites harder here, because the export places every
//! plant by reference. A material at `/Vineyard/Looks/Wood` bound from
//! `/Vineyard/parts/Vine/Var_0/Wood` looks perfect on the authored stage and
//! loses its binding the moment the vine is planted.
//!
//! So a material lives **inside the subtree that gets referenced**, and
//! [`vine`](crate::elements::vine) — the outermost prototype, the one placed
//! onto the ground — is where the plant's materials go. Everything below it
//! reaches them by inheriting down namespace.
//!
//! # Why the colour comes back out of a primvar
//!
//! `UsdPreviewSurface`'s `diffuseColor` defaults to
//! grey, and `displayColor` is only consulted for prims with *no* bound
//! material — so binding a material without wiring the colour up turns the
//! scene grey in Isaac while the viewer, which ignores bindings entirely,
//! still looks right. Every material here therefore reads `displayColor` back
//! through a `UsdPrimvarReader_float3`, which keeps one source of truth for
//! hue ([`color`](super::color)) and leaves the material to say only how the
//! surface responds to light.
//!
//! [`stage::define_parts_library`]: crate::stage

use openusd::schemas::shade::{Connectable, Material, MaterialBindingAPI, Shader};
use openusd::sdf::{self, Value};
use openusd::usd::Stage;

/// The primvar every material here reads its diffuse colour from.
const COLOR_PRIMVAR: &str = "displayColor";

/// How a surface responds to light, with the hue left to `displayColor`.
///
/// A whole material is these two numbers because nothing else in a
/// texture-free `UsdPreviewSurface` distinguishes one organic surface from
/// another: `metallic` is zero for everything that grows, and the specular
/// workflow is off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    /// Microfacet roughness. 0 is a mirror, 1 is chalk.
    pub roughness: f32,
    /// Index of refraction, which sets how bright the specular highlight is at
    /// a glancing angle.
    pub ior: f32,
}

/// Dry bark: rough, matte, no sheen at any angle.
pub const WOOD: Surface = Surface {
    roughness: 0.85,
    ior: 1.5,
};

/// Leaves and green canes both. They share one material deliberately — both
/// are living tissue under a waxy cuticle, so their roughness genuinely
/// matches, and sharing lets the material sit one level up in the namespace
/// where it is bound once per shoot rather than once per leaf. They still look
/// nothing alike, because their `displayColor` differs.
pub const FOLIAGE: Surface = Surface {
    roughness: 0.5,
    ior: 1.45,
};

/// A trellis post. Smoother than bark and rougher than a leaf, which is where
/// both a planed softwood post and a galvanized steel one sit — neither has a
/// highlight worth naming without a texture to break it up.
pub const POLE: Surface = Surface {
    roughness: 0.7,
    ior: 1.5,
};

/// Dry cultivated loam. The roughest thing in the scene.
pub const GROUND: Surface = Surface {
    roughness: 0.95,
    ior: 1.5,
};

/// Authors a `Material` at `path` whose surface is a `UsdPreviewSurface`
/// taking its diffuse colour from the bound geometry's `displayColor`.
///
/// The network is three prims:
///
/// ```text
/// Material "<name>"
///   outputs:surface.connect → Surface.outputs:surface
///   Shader "Surface"   info:id = "UsdPreviewSurface"
///     inputs:diffuseColor.connect → Color.outputs:result
///   Shader "Color"     info:id = "UsdPrimvarReader_float3"
///     inputs:varname = "displayColor"
/// ```
pub fn author_preview_material(stage: &Stage, path: &str, surface: Surface) -> anyhow::Result<()> {
    let material = Material::define(stage, sdf::path(path)?)?;

    let reader = Shader::define(stage, sdf::path(format!("{path}/Color"))?)?;
    reader
        .create_id_attr()?
        .set(Value::token("UsdPrimvarReader_float3"))?;
    // `token` rather than `string`: the older UsdPreviewSurface spec had this
    // as a string, and Isaac's Sdr registry reads only the current one.
    reader
        .create_input("varname", "token")?
        .set(Value::token(COLOR_PRIMVAR))?;
    let result = reader.create_output("result", "float3")?;

    let shader = Shader::define(stage, sdf::path(format!("{path}/Surface"))?)?;
    shader
        .create_id_attr()?
        .set(Value::token("UsdPreviewSurface"))?;
    // float3 → color3f is a legal UsdShade connection, and is how Pixar's own
    // examples wire a primvar reader into a diffuse input.
    shader
        .create_input("diffuseColor", "color3f")?
        .connect_to(&result)?;
    shader
        .create_input("roughness", "float")?
        .set(Value::Float(surface.roughness))?;
    shader
        .create_input("ior", "float")?
        .set(Value::Float(surface.ior))?;
    // Nothing that grows is metallic, and the specular workflow wants a
    // specular colour we have no way to supply without textures.
    shader
        .create_input("metallic", "float")?
        .set(Value::Float(0.0))?;
    shader
        .create_input("useSpecularWorkflow", "int")?
        .set(Value::Int(0))?;
    let out = shader.create_output("surface", "token")?;

    material.create_surface_output()?.connect_to(&out)?;
    Ok(())
}

/// Binds the material at `material` to the prim at `path`.
///
/// A binding inherits down namespace and a descendant's own binding wins over
/// it, which is what lets one binding on a shoot cover the stem and every leaf
/// hanging off it. `path` must already be defined: applying the API schema
/// opens the prim as an `over`, which on an undefined prim would leave a
/// binding on a prim that never gets a type.
pub fn bind_material(stage: &Stage, path: &str, material: &str) -> anyhow::Result<()> {
    MaterialBindingAPI::apply(stage, sdf::path(path)?)?.bind(sdf::path(material)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::VineyardParams;
    use crate::elements::util::testing;
    use crate::elements::vine;
    use openusd::tf::Token;
    use usd_bevy::authoring::prim_exists;

    /// All-purpose, which is what everything here binds at.
    const ANY: &str = "";

    /// Path of the first replant a generated parcel planted.
    ///
    /// Found rather than named: which slots come out young is the planting
    /// seed's business, and a fixed `Vine_007` would quietly stop testing a
    /// young vine the moment the stream moved.
    fn a_planted_replant(stage: &Stage) -> String {
        stage
            .prim(sdf::path("/Vineyard/Planting").unwrap())
            .children()
            .unwrap()
            .iter()
            .find_map(|row| {
                let row = row.path().as_str().to_string();
                testing::instances(stage, &row)
                    .into_iter()
                    .find(|i| i.prototype.starts_with(vine::YOUNG_PROTOTYPE))
                    .and_then(|i| i.name)
                    .map(|name| format!("{row}/{name}"))
            })
            .expect("the default planting holds some replants")
    }

    fn stage_with_material() -> Stage {
        let stage = crate::stage::new_stage("material.usda").unwrap();
        author_preview_material(&stage, "/Vineyard/Looks/Test", FOLIAGE).unwrap();
        stage
    }

    /// Walk the authored network the way a renderer does — terminal, to
    /// surface shader, to whatever drives its diffuse — rather than asserting
    /// on the exported text. A misspelled `info:id` or a connection left
    /// dangling still produces perfectly plausible-looking usda.
    #[test]
    fn a_material_drives_its_diffuse_from_the_display_color_primvar() {
        let stage = stage_with_material();
        let material = Material::get(&stage, sdf::path("/Vineyard/Looks/Test").unwrap())
            .unwrap()
            .expect("a Material prim");

        let surface = material
            .compute_surface_source()
            .unwrap()
            .expect("the surface terminal resolves to a shader");
        assert_eq!(surface.id().unwrap().as_deref(), Some("UsdPreviewSurface"));

        let sources = surface.input("diffuseColor").connections().unwrap();
        let [source] = sources.as_slice() else {
            panic!("diffuseColor is connected to exactly one source, got {sources:?}");
        };
        let reader = Shader::get(&stage, source.prim_path())
            .unwrap()
            .expect("diffuseColor is driven by a Shader");
        assert_eq!(
            reader.id().unwrap().as_deref(),
            Some("UsdPrimvarReader_float3"),
            "…and that shader is a primvar reader"
        );
        assert_eq!(
            reader.input("varname").get::<Token>().unwrap().as_deref(),
            Some(COLOR_PRIMVAR),
            "…reading the primvar the geometry actually carries"
        );
    }

    /// `diffuseColor` unconnected would leave `UsdPreviewSurface` on its grey
    /// default, and `displayColor` is only consulted for prims with *no* bound
    /// material — so the scene would go grey in Isaac while the viewer, which
    /// ignores bindings entirely, still looked correct. Nothing else here can
    /// catch that.
    #[test]
    fn the_surface_carries_the_settings_a_texture_free_look_has_left() {
        let stage = stage_with_material();
        let surface = Material::get(&stage, sdf::path("/Vineyard/Looks/Test").unwrap())
            .unwrap()
            .unwrap()
            .compute_surface_source()
            .unwrap()
            .unwrap();

        assert_eq!(
            surface.input("roughness").get::<f32>().unwrap(),
            Some(FOLIAGE.roughness)
        );
        assert_eq!(
            surface.input("ior").get::<f32>().unwrap(),
            Some(FOLIAGE.ior)
        );
        assert_eq!(surface.input("metallic").get::<f32>().unwrap(), Some(0.0));
        assert_eq!(
            surface.input("useSpecularWorkflow").get::<i32>().unwrap(),
            Some(0),
            "the specular workflow wants a specular colour no texture-free \
             material can supply"
        );
    }

    #[test]
    fn binding_names_the_material_on_the_prim() {
        let stage = stage_with_material();
        usd_bevy::authoring::define_prim(&stage, "/Vineyard/Thing", "Xform").unwrap();
        bind_material(&stage, "/Vineyard/Thing", "/Vineyard/Looks/Test").unwrap();

        let api = MaterialBindingAPI::get(&stage, sdf::path("/Vineyard/Thing").unwrap())
            .unwrap()
            .expect("the API schema is applied, which is what usdchecker looks for");
        assert_eq!(
            api.direct_binding(ANY)
                .unwrap()
                .map(|p| p.as_str().to_string()),
            Some("/Vineyard/Looks/Test".to_string())
        );
    }

    /// **The rule this module exists for.**
    ///
    /// A `material:binding` is a relationship, and relationship targets are
    /// namespace-mapped through composition arcs. The export places every
    /// plant by internal reference, so a binding only survives if its target
    /// sits inside the referenced subtree — which is the whole reason the
    /// materials live in the vine prototype rather than in one scene-wide
    /// `/Vineyard/Looks`.
    ///
    /// The failure this guards is silent in both directions: the authored
    /// prototype resolves perfectly, and the viewer ignores bindings outright,
    /// so nothing short of resolving a *placed* prim would ever notice that
    /// every plant in the parcel had come out unshaded.
    #[test]
    fn a_binding_survives_being_referenced_onto_the_ground() {
        let stage = crate::generate::generate_stage(&VineyardParams::default()).unwrap();
        let row = "/Vineyard/Planting/Row_000";
        let (vine, pole) = (format!("{row}/Vine_000"), format!("{row}/Pole_000"));
        let young = a_planted_replant(&stage);

        // Every kind of thing the export places, with the prims under it that
        // have to come out shaded. A vine's wood binds directly and its stem
        // and blades inherit from the shoot they hang off, which is what saves
        // a relationship on each of six figures' worth of leaves; a post is
        // one mesh, and binds its own.
        for (placed, prims) in [
            (
                &vine,
                vec![
                    format!("{vine}/Wood"),
                    format!("{vine}/Shoot_00_0"),
                    format!("{vine}/Shoot_00_0/Stem"),
                    format!("{vine}/Shoot_00_0/Leaf_00"),
                ],
            ),
            // A replant is placed from a library of its own, so its binding
            // is a second one that has to survive the same trip — and the only
            // one where the material and the geometry it shades arrive through
            // *different* references.
            (
                &young,
                vec![
                    format!("{young}/Shoot"),
                    format!("{young}/Shoot/Stem"),
                    format!("{young}/Shoot/Leaf_00"),
                ],
            ),
            (&pole, vec![format!("{pole}/Pole")]),
        ] {
            assert!(prim_exists(&stage, placed), "the fixture placed {placed}");
            for prim in prims {
                assert!(prim_exists(&stage, &prim), "{prim} composes in");
                let resolved = MaterialBindingAPI::apply(&stage, sdf::path(&prim).unwrap())
                    .unwrap()
                    .compute_bound_material(ANY)
                    .unwrap()
                    .unwrap_or_else(|| panic!("{prim} resolves no material"))
                    .as_str()
                    .to_string();

                // Remapped through the arc onto the prim it was placed as —
                // not still pointing into `/parts`, which is the shape that
                // composes away to nothing the moment a consumer references
                // this layer.
                assert!(
                    resolved.starts_with(&format!("{placed}/")),
                    "{prim} resolved to `{resolved}`, outside the {placed} it was placed as"
                );
                assert!(
                    prim_exists(&stage, &resolved),
                    "{prim} resolved to `{resolved}`, which is not on the stage"
                );
            }
        }
    }
}
