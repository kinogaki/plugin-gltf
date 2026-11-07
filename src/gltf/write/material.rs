//! Materials, textures and samplers as glTF JSON: values at their defaults
//! and extensions at theirs are left unwritten.
use super::check;
use crate::gltf::GltfError;
use crate::gltf::json::{Members, Value};
use crate::gltf::model::{
    AlphaMode, MagFilter, Material, MinFilter, Sampler, Texture, TextureRef, TextureTransform, Wrap,
};
use std::collections::BTreeSet;

type Result<T> = std::result::Result<T, GltfError>;

pub(super) fn material(m: &Material, at: &str, textures: usize) -> Result<Value<'static>> {
    let d = Material::default();
    // A texture reference, and one with its `scale` or `strength`.
    let tex = |r: &Option<TextureRef>, key: &str| texture_ref(r, at, key, textures, None);
    let scaled = |r: &Option<TextureRef>, key: &str, amount: (&'static str, f64)| {
        texture_ref(r, at, key, textures, Some(amount))
    };
    let pbr = Members::new()
        .with_changed("baseColorFactor", m.base_color, d.base_color)
        .with_some(
            "baseColorTexture",
            tex(&m.base_color_texture, "baseColorTexture")?,
        )
        .with_changed("metallicFactor", m.metallic, d.metallic)
        .with_changed("roughnessFactor", m.roughness, d.roughness)
        .with_some(
            "metallicRoughnessTexture",
            tex(&m.metallic_roughness_texture, "metallicRoughnessTexture")?,
        );
    let mut extensions = Members::new();
    if m.emissive_strength != d.emissive_strength {
        let members = Members::new().with("emissiveStrength", m.emissive_strength);
        extensions = extensions.with("KHR_materials_emissive_strength", members);
    }
    if m.unlit {
        extensions = extensions.with("KHR_materials_unlit", Members::new());
    }
    if m.ior != d.ior {
        extensions = extensions.with("KHR_materials_ior", Members::new().with("ior", m.ior));
    }
    if m.dispersion != d.dispersion {
        let members = Members::new().with("dispersion", m.dispersion);
        extensions = extensions.with("KHR_materials_dispersion", members);
    }
    let (t, dt) = (&m.transmission, &d.transmission);
    if t != dt {
        let members = Members::new()
            .with_changed("transmissionFactor", t.factor, dt.factor)
            .with_some(
                "transmissionTexture",
                tex(&t.texture, "transmissionTexture")?,
            );
        extensions = extensions.with("KHR_materials_transmission", members);
    }
    let (v, dv) = (&m.volume, &d.volume);
    if v != dv {
        let members = Members::new()
            .with_changed("thicknessFactor", v.thickness, dv.thickness)
            .with_some(
                "thicknessTexture",
                tex(&v.thickness_texture, "thicknessTexture")?,
            )
            .with_some("attenuationDistance", v.attenuation_distance)
            .with_changed(
                "attenuationColor",
                v.attenuation_color,
                dv.attenuation_color,
            );
        extensions = extensions.with("KHR_materials_volume", members);
    }
    let (c, dc) = (&m.clearcoat, &d.clearcoat);
    if c != dc {
        let normal = ("scale", c.normal_scale);
        let members = Members::new()
            .with_changed("clearcoatFactor", c.factor, dc.factor)
            .with_some("clearcoatTexture", tex(&c.texture, "clearcoatTexture")?)
            .with_changed("clearcoatRoughnessFactor", c.roughness, dc.roughness)
            .with_some(
                "clearcoatRoughnessTexture",
                tex(&c.roughness_texture, "clearcoatRoughnessTexture")?,
            )
            .with_some(
                "clearcoatNormalTexture",
                scaled(&c.normal_texture, "clearcoatNormalTexture", normal)?,
            );
        extensions = extensions.with("KHR_materials_clearcoat", members);
    }
    let (s, ds) = (&m.specular, &d.specular);
    if s != ds {
        let members = Members::new()
            .with_changed("specularFactor", s.factor, ds.factor)
            .with_some("specularTexture", tex(&s.texture, "specularTexture")?)
            .with_changed("specularColorFactor", s.color, ds.color)
            .with_some(
                "specularColorTexture",
                tex(&s.color_texture, "specularColorTexture")?,
            );
        extensions = extensions.with("KHR_materials_specular", members);
    }
    let (s, ds) = (&m.sheen, &d.sheen);
    if s != ds {
        let members = Members::new()
            .with_changed("sheenColorFactor", s.color, ds.color)
            .with_some(
                "sheenColorTexture",
                tex(&s.color_texture, "sheenColorTexture")?,
            )
            .with_changed("sheenRoughnessFactor", s.roughness, ds.roughness)
            .with_some(
                "sheenRoughnessTexture",
                tex(&s.roughness_texture, "sheenRoughnessTexture")?,
            );
        extensions = extensions.with("KHR_materials_sheen", members);
    }
    let (i, di) = (&m.iridescence, &d.iridescence);
    if i != di {
        let members = Members::new()
            .with_changed("iridescenceFactor", i.factor, di.factor)
            .with_some("iridescenceTexture", tex(&i.texture, "iridescenceTexture")?)
            .with_changed("iridescenceIor", i.ior, di.ior)
            .with_changed(
                "iridescenceThicknessMinimum",
                i.thickness_minimum,
                di.thickness_minimum,
            )
            .with_changed(
                "iridescenceThicknessMaximum",
                i.thickness_maximum,
                di.thickness_maximum,
            )
            .with_some(
                "iridescenceThicknessTexture",
                tex(&i.thickness_texture, "iridescenceThicknessTexture")?,
            );
        extensions = extensions.with("KHR_materials_iridescence", members);
    }
    let (a, da) = (&m.anisotropy, &d.anisotropy);
    if a != da {
        let members = Members::new()
            .with_changed("anisotropyStrength", a.strength, da.strength)
            .with_changed("anisotropyRotation", a.rotation, da.rotation)
            .with_some("anisotropyTexture", tex(&a.texture, "anisotropyTexture")?);
        extensions = extensions.with("KHR_materials_anisotropy", members);
    }
    let alpha_mode = match m.alpha_mode {
        AlphaMode::Opaque => None,
        AlphaMode::Mask => Some("MASK"),
        AlphaMode::Blend => Some("BLEND"),
    };
    let material = Members::new()
        .with_text("name", &m.name)
        .with_if(!pbr.is_empty(), "pbrMetallicRoughness", pbr)
        .with_some(
            "normalTexture",
            scaled(
                &m.normal_texture,
                "normalTexture",
                ("scale", m.normal_scale),
            )?,
        )
        .with_some(
            "occlusionTexture",
            scaled(
                &m.occlusion_texture,
                "occlusionTexture",
                ("strength", m.occlusion_strength),
            )?,
        )
        .with_some(
            "emissiveTexture",
            tex(&m.emissive_texture, "emissiveTexture")?,
        )
        .with_changed("emissiveFactor", m.emissive, d.emissive)
        .with_some("alphaMode", alpha_mode)
        // Only MASK reads the cutoff.
        .with_if(
            m.alpha_mode == AlphaMode::Mask && m.alpha_cutoff != d.alpha_cutoff,
            "alphaCutoff",
            m.alpha_cutoff,
        )
        .with_changed("doubleSided", m.double_sided, false)
        .with_if(!extensions.is_empty(), "extensions", extensions);
    Ok(material.into())
}

/// The texture reference `r` (member `key` of the material at `at`): its
/// index, set, `scale` or `strength` (`amount`) and transform.
fn texture_ref(
    r: &Option<TextureRef>,
    at: &str,
    key: &str,
    textures: usize,
    amount: Option<(&'static str, f64)>,
) -> Result<Option<Value<'static>>> {
    let Some(r) = r else { return Ok(None) };
    check(&format!("{at}.{key}"), r.texture, textures)?;
    let (t, identity) = (&r.transform, TextureTransform::default());
    let transform = (*t != identity).then(|| {
        let members = Members::new()
            .with_changed("offset", t.offset, identity.offset)
            .with_changed("rotation", t.rotation, identity.rotation)
            .with_changed("scale", t.scale, identity.scale);
        Members::new().with("KHR_texture_transform", members)
    });
    let mut info = Members::new()
        .with("index", r.texture)
        .with_changed("texCoord", r.tex_coord, 0);
    if let Some((key, value)) = amount {
        info = info.with_changed(key, value, 1.0);
    }
    Ok(Some(info.with_some("extensions", transform).into()))
}

/// A texture: its extension source written under the extension its image's
/// MIME type names, which `required` takes when there is no `source` to fall
/// back on.
pub(super) fn texture(
    t: &Texture,
    at: &str,
    samplers: usize,
    mimes: &[Option<&str>],
    required: &mut BTreeSet<&'static str>,
) -> Result<Value<'static>> {
    if let Some(sampler) = t.sampler {
        check(&format!("{at}.sampler"), sampler, samplers)?;
    }
    for source in t.source.iter().chain(&t.extension_source) {
        check(&format!("{at}.source"), *source, mimes.len())?;
    }
    let mut members = Members::new()
        .with_text("name", &t.name)
        .with_some("sampler", t.sampler)
        .with_some("source", t.source);
    if let Some(image) = t.extension_source {
        let name = match mimes[image] {
            Some("image/ktx2") => "KHR_texture_basisu",
            Some("image/webp") => "EXT_texture_webp",
            Some("image/avif") => "EXT_texture_avif",
            other => {
                return Err(GltfError::invalid(
                    at,
                    format!("an extension source of type {other:?}"),
                ));
            }
        };
        if t.source.is_none() {
            required.insert(name);
        }
        members = members.with(
            "extensions",
            Members::new().with(name, Members::new().with("source", image)),
        );
    } else if t.source.is_none() {
        return Err(GltfError::invalid(at, "a texture without an image"));
    }
    Ok(members.into())
}

pub(super) fn sampler(s: &Sampler) -> Value<'static> {
    let mag = s.mag_filter.map(|f| match f {
        MagFilter::Nearest => 9728u32,
        MagFilter::Linear => 9729,
    });
    let min = s.min_filter.map(|f| match f {
        MinFilter::Nearest => 9728u32,
        MinFilter::Linear => 9729,
        MinFilter::NearestMipmapNearest => 9984,
        MinFilter::LinearMipmapNearest => 9985,
        MinFilter::NearestMipmapLinear => 9986,
        MinFilter::LinearMipmapLinear => 9987,
    });
    let wrap = |w: Wrap| match w {
        Wrap::ClampToEdge => 33071u32,
        Wrap::MirroredRepeat => 33648,
        Wrap::Repeat => 10497,
    };
    Members::new()
        .with_text("name", &s.name)
        .with_some("magFilter", mag)
        .with_some("minFilter", min)
        .with_changed("wrapS", wrap(s.wrap_s), wrap(Wrap::Repeat))
        .with_changed("wrapT", wrap(s.wrap_t), wrap(Wrap::Repeat))
        .into()
}
