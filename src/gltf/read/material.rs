//! Materials with their KHR_materials extensions, texture references with
//! `KHR_texture_transform`, textures and samplers.
use super::Result;
use super::fields::Obj;
use crate::gltf::model::{
    AlphaMode, Anisotropy, Clearcoat, Iridescence, MagFilter, Material, MinFilter, Sampler, Sheen,
    Specular, Texture, TextureRef, TextureTransform, Transmission, Volume, Wrap,
};

/// The extensions that give a texture an image `source` doesn't, in the
/// order one is chosen when a texture names several.
pub(crate) const IMAGE_EXTENSIONS: [&str; 3] =
    ["KHR_texture_basisu", "EXT_texture_avif", "EXT_texture_webp"];

pub(super) fn sampler(sampler: &Obj<'_, '_>) -> Result<Sampler> {
    let coded = |key: &str, codes: &[usize]| -> Result<Option<usize>> {
        match sampler.opt_count(key)? {
            Some(code) if !codes.contains(&code) => {
                Err(sampler.error(key, format!("{code} isn't one of {codes:?}")))
            }
            code => Ok(code),
        }
    };
    let wrap = |key: &str| -> Result<Wrap> {
        Ok(match coded(key, &[33071, 33648, 10497])? {
            Some(33071) => Wrap::ClampToEdge,
            Some(33648) => Wrap::MirroredRepeat,
            _ => Wrap::Repeat,
        })
    };
    let mag_filter = coded("magFilter", &[9728, 9729])?.map(|code| match code {
        9728 => MagFilter::Nearest,
        _ => MagFilter::Linear,
    });
    let min_filter =
        coded("minFilter", &[9728, 9729, 9984, 9985, 9986, 9987])?.map(|code| match code {
            9728 => MinFilter::Nearest,
            9729 => MinFilter::Linear,
            9984 => MinFilter::NearestMipmapNearest,
            9985 => MinFilter::LinearMipmapNearest,
            9986 => MinFilter::NearestMipmapLinear,
            _ => MinFilter::LinearMipmapLinear,
        });
    Ok(Sampler {
        name: sampler.text("name")?,
        mag_filter,
        min_filter,
        wrap_s: wrap("wrapS")?,
        wrap_t: wrap("wrapT")?,
    })
}

pub(super) fn texture(texture: &Obj<'_, '_>, samplers: usize, images: usize) -> Result<Texture> {
    let mut extension_source = None;
    for name in IMAGE_EXTENSIONS {
        if let Some(extension) = texture.extension(name)? {
            extension_source =
                extension_source.or(Some(extension.required_index("source", images)?));
        }
    }
    let source = texture.index("source", images)?;
    if source.is_none() && extension_source.is_none() {
        return Err(texture.error("source", "missing"));
    }
    Ok(Texture {
        name: texture.text("name")?,
        sampler: texture.index("sampler", samplers)?,
        source,
        extension_source,
    })
}

pub(super) fn material(material: &Obj<'_, '_>, textures: usize) -> Result<Material> {
    let texture = |parent: &Obj<'_, '_>, key: &str| texture_ref(parent, key, textures);
    let defaults = Material::default();
    let mut read = Material {
        name: material.text("name")?,
        normal_texture: texture(material, "normalTexture")?,
        normal_scale: scale(material, "normalTexture", "scale")?,
        occlusion_texture: texture(material, "occlusionTexture")?,
        occlusion_strength: scale(material, "occlusionTexture", "strength")?,
        emissive: material.numbers("emissiveFactor", defaults.emissive)?,
        emissive_texture: texture(material, "emissiveTexture")?,
        alpha_mode: match material.opt_str("alphaMode")? {
            None | Some("OPAQUE") => AlphaMode::Opaque,
            Some("MASK") => AlphaMode::Mask,
            Some("BLEND") => AlphaMode::Blend,
            Some(other) => {
                return Err(material.error("alphaMode", format!("{other:?} is no alpha mode")));
            }
        },
        alpha_cutoff: material.number("alphaCutoff", defaults.alpha_cutoff)?,
        double_sided: material.flag("doubleSided", false)?,
        unlit: material.extension("KHR_materials_unlit")?.is_some(),
        ..defaults
    };
    if let Some(pbr) = material.object("pbrMetallicRoughness")? {
        read.base_color = pbr.numbers("baseColorFactor", read.base_color)?;
        read.base_color_texture = texture(&pbr, "baseColorTexture")?;
        read.metallic = pbr.number("metallicFactor", read.metallic)?;
        read.roughness = pbr.number("roughnessFactor", read.roughness)?;
        read.metallic_roughness_texture = texture(&pbr, "metallicRoughnessTexture")?;
    }
    if let Some(e) = material.extension("KHR_materials_emissive_strength")? {
        read.emissive_strength = e.number("emissiveStrength", read.emissive_strength)?;
    }
    if let Some(e) = material.extension("KHR_materials_ior")? {
        read.ior = e.number("ior", read.ior)?;
    }
    if let Some(e) = material.extension("KHR_materials_dispersion")? {
        read.dispersion = e.number("dispersion", read.dispersion)?;
    }
    if let Some(e) = material.extension("KHR_materials_transmission")? {
        read.transmission = Transmission {
            factor: e.number("transmissionFactor", 0.0)?,
            texture: texture(&e, "transmissionTexture")?,
        };
    }
    if let Some(e) = material.extension("KHR_materials_volume")? {
        let d = Volume::default();
        read.volume = Volume {
            thickness: e.number("thicknessFactor", d.thickness)?,
            thickness_texture: texture(&e, "thicknessTexture")?,
            attenuation_distance: e.opt_number("attenuationDistance")?,
            attenuation_color: e.numbers("attenuationColor", d.attenuation_color)?,
        };
    }
    if let Some(e) = material.extension("KHR_materials_clearcoat")? {
        let d = Clearcoat::default();
        read.clearcoat = Clearcoat {
            factor: e.number("clearcoatFactor", d.factor)?,
            texture: texture(&e, "clearcoatTexture")?,
            roughness: e.number("clearcoatRoughnessFactor", d.roughness)?,
            roughness_texture: texture(&e, "clearcoatRoughnessTexture")?,
            normal_texture: texture(&e, "clearcoatNormalTexture")?,
            normal_scale: scale(&e, "clearcoatNormalTexture", "scale")?,
        };
    }
    if let Some(e) = material.extension("KHR_materials_specular")? {
        let d = Specular::default();
        read.specular = Specular {
            factor: e.number("specularFactor", d.factor)?,
            texture: texture(&e, "specularTexture")?,
            color: e.numbers("specularColorFactor", d.color)?,
            color_texture: texture(&e, "specularColorTexture")?,
        };
    }
    if let Some(e) = material.extension("KHR_materials_sheen")? {
        read.sheen = Sheen {
            color: e.numbers("sheenColorFactor", [0.0; 3])?,
            color_texture: texture(&e, "sheenColorTexture")?,
            roughness: e.number("sheenRoughnessFactor", 0.0)?,
            roughness_texture: texture(&e, "sheenRoughnessTexture")?,
        };
    }
    if let Some(e) = material.extension("KHR_materials_iridescence")? {
        let d = Iridescence::default();
        read.iridescence = Iridescence {
            factor: e.number("iridescenceFactor", d.factor)?,
            texture: texture(&e, "iridescenceTexture")?,
            ior: e.number("iridescenceIor", d.ior)?,
            thickness_minimum: e.number("iridescenceThicknessMinimum", d.thickness_minimum)?,
            thickness_maximum: e.number("iridescenceThicknessMaximum", d.thickness_maximum)?,
            thickness_texture: texture(&e, "iridescenceThicknessTexture")?,
        };
    }
    if let Some(e) = material.extension("KHR_materials_anisotropy")? {
        read.anisotropy = Anisotropy {
            strength: e.number("anisotropyStrength", 0.0)?,
            rotation: e.number("anisotropyRotation", 0.0)?,
            texture: texture(&e, "anisotropyTexture")?,
        };
    }
    Ok(read)
}

/// The texture reference `key` of `parent`, its transform folded in.
fn texture_ref(parent: &Obj<'_, '_>, key: &str, textures: usize) -> Result<Option<TextureRef>> {
    let Some(info) = parent.object(key)? else {
        return Ok(None);
    };
    let tex_coord = |obj: &Obj<'_, '_>| -> Result<Option<u32>> {
        match obj.opt_count("texCoord")? {
            Some(set) => u32::try_from(set)
                .map(Some)
                .map_err(|_| obj.error("texCoord", "too large")),
            None => Ok(None),
        }
    };
    let mut reference = TextureRef {
        texture: info.required_index("index", textures)?,
        tex_coord: tex_coord(&info)?.unwrap_or(0),
        transform: TextureTransform::default(),
    };
    if let Some(transform) = info.extension("KHR_texture_transform")? {
        let identity = TextureTransform::default();
        reference.transform = TextureTransform {
            offset: transform.numbers("offset", identity.offset)?,
            rotation: transform.number("rotation", identity.rotation)?,
            scale: transform.numbers("scale", identity.scale)?,
        };
        reference.tex_coord = tex_coord(&transform)?.unwrap_or(reference.tex_coord);
    }
    Ok(Some(reference))
}

/// A texture reference's `scale` or `strength`, 1 without the reference.
fn scale(parent: &Obj<'_, '_>, key: &str, member: &str) -> Result<f64> {
    match parent.object(key)? {
        Some(info) => info.number(member, 1.0),
        None => Ok(1.0),
    }
}
