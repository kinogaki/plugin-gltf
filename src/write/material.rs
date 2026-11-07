//! Materials to glTF's: glTF PBR and Surface Unlit as they are recorded
//! (the read's inverse), any other MaterialX model translated into glTF PBR
//! (`standard_surface_to_gltf_pbr`, through Standard Surface from OpenPBR,
//! folded), an engine's own material through its standard form.
//!
//! A glTF PBR input reads a constant, a texture node (glTF's, MaterialX's
//! `image`, USD UV Texture) through `extract` for a packed channel, a
//! `normalmap` or `mix` as the read makes them, or what glTF can't hold,
//! which is said and written as the input's own value. An image is
//! written once: a PNG or JPEG as `source`, KTX2, WebP or AVIF as its
//! extension's; another (an OpenEXR) is left out, said. Fields glTF defines
//! as having no effect are written at their defaults (a volume without
//! transmission, a strength of zero).
use super::Out;
use crate::gltf::{
    self, AlphaMode, MagFilter, MinFilter, Sampler, TextureRef, TextureTransform, Wrap,
};
use kinogaki_render_api::MaterialNetwork;
use kinogaki_studio_api::geometry::image::{ImageFile, ImageSource};
use kinogaki_studio_api::standard::ImportedMaterial;
use kinogaki_studio_api::{Error, Value};
use std::collections::HashMap;

/// The glTF materials, images, textures and samplers made, by what made
/// them.
#[derive(Default)]
pub(super) struct Looks {
    /// Each material key's glTF material (none: written without one).
    made: HashMap<String, Option<usize>>,
    images: Vec<(ImageFile, Option<usize>)>,
    textures: Vec<((usize, Option<usize>), usize)>,
    samplers: Vec<(Sampler, usize)>,
}

/// The texture nodes of glTF's family (MaterialX's and Kinogaki's with a
/// minification filter).
const GLTF_TEXTURES: [&str; 12] = [
    "gltf_image",
    "gltf_colorimage",
    "gltf_normalmap",
    "gltf_anisotropy_image",
    "gltf_iridescence_thickness",
    "kinogaki_gltf_image",
    "kinogaki_gltf_colorimage",
    "kinogaki_gltf_normalmap",
    "kinogaki_gltf_anisotropy_image",
    "kinogaki_gltf_iridescence_thickness",
    "image",
    "UsdUVTexture",
];

impl Out<'_> {
    /// The glTF material the polygons' key `key` names, made once; none for a
    /// key naming no material of the materials item.
    pub(super) fn material(&mut self, key: &str) -> Result<Option<usize>, Error> {
        if let Some(&made) = self.looks.made.get(key) {
            return Ok(made);
        }
        let imported = self
            .held
            .and_then(|h| h.materials.iter().find(|m| m.key == key));
        let made = match imported {
            Some(m) => Some(self.write_material(m)?),
            None => {
                self.warn(format!("{key}: no such material; written without one"));
                None
            }
        };
        self.looks.made.insert(key.to_owned(), made);
        Ok(made)
    }

    fn write_material(&mut self, m: &ImportedMaterial) -> Result<usize, Error> {
        let mut material = gltf::Material {
            name: m.name.clone(),
            double_sided: m.double_sided,
            ..gltf::Material::default()
        };
        let network = MaterialNetwork::try_from(&m.inside)
            .ok()
            .or_else(|| MaterialNetwork::try_from(m.standard.as_ref()?).ok());
        let surface = network.as_ref().and_then(|n| Some((n, n.surface()?)));
        match surface {
            None => self.warn(format!(
                "{}: no MaterialX form; written as glTF's default material",
                m.name
            )),
            Some((n, s)) => match node(n, s) {
                "gltf_pbr" => self.gltf_pbr(n, s, &mut material)?,
                "surface_unlit" | "kinogaki_surface_unlit" => self.unlit(n, s, &mut material)?,
                model => match n.translated("gltf_pbr") {
                    Some(t) => {
                        let s = t.surface().expect("translated");
                        self.gltf_pbr(&t, s, &mut material)?;
                        self.warn(format!("{}: {model} written as glTF PBR", m.name));
                    }
                    None => self.warn(format!(
                        "{}: {model} has no glTF PBR form; written as glTF's default material",
                        m.name
                    )),
                },
            },
        }
        self.model.materials.push(material);
        Ok(self.model.materials.len() - 1)
    }

    /// glTF PBR's inputs into `m`.
    fn gltf_pbr(
        &mut self,
        n: &MaterialNetwork,
        s: u32,
        m: &mut gltf::Material,
    ) -> Result<(), Error> {
        self.base(n, s, ("base_color", "alpha"), m)?;
        let (metal, metallic) = self.channel(n, s, "metallic");
        let (rough, roughness) = self.channel(n, s, "roughness");
        m.metallic = metallic;
        m.roughness = roughness;
        m.metallic_roughness_texture = self.texture(n, metal.or(rough));
        (m.normal_texture, m.normal_scale) = self.normal(n, s, "normal");
        (m.occlusion_texture, m.occlusion_strength) = match driver(n, s, "occlusion") {
            Some((mix, _)) if node(n, mix) == "mix" => {
                let fg = driver(n, mix, "fg").map(|d| d.0);
                (self.texture(n, fg), number(n, mix, "mix"))
            }
            Some((t, _)) if is_texture(n, t) => (self.texture(n, Some(t)), 1.0),
            _ => (None, 1.0),
        };
        let (texture, emissive) = self.rgb(n, s, "emissive")?;
        (m.emissive_texture, m.emissive) = (texture, emissive);
        m.emissive_strength = number(n, s, "emissive_strength");
        if m.emissive_strength == 0.0 {
            (m.emissive_texture, m.emissive, m.emissive_strength) = (None, [0.0; 3], 1.0);
        }
        m.alpha_mode = alpha_mode(integer(n, s, "alpha_mode"));
        m.alpha_cutoff = number(n, s, "alpha_cutoff");
        m.ior = number(n, s, "ior");
        m.dispersion = number(n, s, "dispersion");
        let (t, factor) = self.channel(n, s, "transmission");
        m.transmission = gltf::Transmission {
            factor,
            texture: self.texture(n, t),
        };
        let (t, thickness) = self.channel(n, s, "thickness");
        let distance = number(n, s, "attenuation_distance");
        m.volume = gltf::Volume {
            thickness: self.space.metres(thickness),
            thickness_texture: self.texture(n, t),
            attenuation_distance: (distance > 0.0).then(|| self.space.metres(distance)),
            attenuation_color: self.color(color(n, s, "attenuation_color"))?,
        };
        let (t, factor) = self.channel(n, s, "clearcoat");
        let (r, roughness) = self.channel(n, s, "clearcoat_roughness");
        let (normal_texture, normal_scale) = self.normal(n, s, "clearcoat_normal");
        m.clearcoat = gltf::Clearcoat {
            factor,
            texture: self.texture(n, t),
            roughness,
            roughness_texture: self.texture(n, r),
            normal_texture,
            normal_scale,
        };
        let (t, factor) = self.channel(n, s, "specular");
        let (color_texture, color) = self.rgb(n, s, "specular_color")?;
        m.specular = gltf::Specular {
            factor,
            texture: self.texture(n, t),
            color,
            color_texture,
        };
        let (color_texture, color) = self.rgb(n, s, "sheen_color")?;
        let (r, roughness) = self.channel(n, s, "sheen_roughness");
        m.sheen = gltf::Sheen {
            color,
            color_texture,
            roughness,
            roughness_texture: self.texture(n, r),
        };
        let (t, factor) = self.channel(n, s, "iridescence");
        let mut iridescence = gltf::Iridescence {
            factor,
            texture: self.texture(n, t),
            ior: number(n, s, "iridescence_ior"),
            thickness_maximum: number(n, s, "iridescence_thickness"),
            ..gltf::Iridescence::default()
        };
        if let Some((t, _)) = driver(n, s, "iridescence_thickness").filter(|d| is_texture(n, d.0)) {
            iridescence.thickness_minimum = number(n, t, "thicknessMin");
            iridescence.thickness_maximum = number(n, t, "thicknessMax");
            iridescence.thickness_texture = self.texture(n, Some(t));
        }
        m.iridescence = iridescence;
        m.anisotropy = match driver(n, s, "anisotropy_strength") {
            Some((a, _)) if is_texture(n, a) => gltf::Anisotropy {
                strength: number(n, a, "anisotropy_strength"),
                rotation: number(n, a, "anisotropy_rotation"),
                texture: self.texture(n, Some(a)),
            },
            _ => gltf::Anisotropy {
                strength: number(n, s, "anisotropy_strength"),
                rotation: number(n, s, "anisotropy_rotation"),
                texture: None,
            },
        };
        without_effect(m);
        Ok(())
    }

    /// Surface Unlit (Cutout): `KHR_materials_unlit` of its emission.
    fn unlit(&mut self, n: &MaterialNetwork, s: u32, m: &mut gltf::Material) -> Result<(), Error> {
        self.base(n, s, ("emission_color", "opacity"), m)?;
        let emission = number(n, s, "emission");
        for c in &mut m.base_color[..3] {
            *c *= emission;
        }
        m.unlit = true;
        // MaterialX's own Surface Unlit blends where its opacity says.
        let blends = driver(n, s, "opacity").is_some() || number(n, s, "opacity") < 1.0;
        (m.alpha_mode, m.alpha_cutoff) = match node(n, s) {
            "kinogaki_surface_unlit" => (
                alpha_mode(integer(n, s, "alpha_mode")),
                number(n, s, "alpha_cutoff"),
            ),
            _ if blends => (AlphaMode::Blend, 0.5),
            _ => (AlphaMode::Opaque, 0.5),
        };
        Ok(())
    }

    /// The base color and alpha from inputs `color` and `alpha` of `s`.
    fn base(
        &mut self,
        n: &MaterialNetwork,
        s: u32,
        (color_in, alpha_in): (&str, &str),
        m: &mut gltf::Material,
    ) -> Result<(), Error> {
        let mut alpha = number(n, s, alpha_in);
        let rgb = match driver(n, s, color_in) {
            Some((c, _)) if node(n, c).ends_with("gltf_colorimage") => {
                m.base_color_texture = self.texture(n, Some(c));
                let factor = numbers(n, c, "color");
                alpha = factor.get(3).copied().unwrap_or(1.0);
                [0, 1, 2].map(|k| factor.get(k).copied().unwrap_or(1.0))
            }
            Some((times, _)) if node(n, times) == "multiply" => {
                let held = |input| driver(n, times, input).map(|d| node(n, d.0));
                if held("in1") != Some("geomcolor") {
                    self.warn("a base color glTF can't hold written as its constant".to_owned());
                }
                color(n, times, "in2")
            }
            Some((t, _)) if is_texture(n, t) => {
                m.base_color_texture = self.texture(n, Some(t));
                factor(n, t)
            }
            Some(_) => {
                self.warn("a base color glTF can't hold written as its constant".to_owned());
                color(n, s, color_in)
            }
            None => color(n, s, color_in),
        };
        let [r, g, b] = self.color(rgb)?;
        m.base_color = [r, g, b, alpha];
        Ok(())
    }

    /// A one-channel input: the texture node it reads and the channel's
    /// factor, else none and its value.
    fn channel(&mut self, n: &MaterialNetwork, s: u32, input: &str) -> (Option<u32>, f64) {
        let held = number(n, s, input);
        let Some((from, _)) = driver(n, s, input) else {
            return (None, held);
        };
        let (texture, at) = match node(n, from) {
            "extract" => match driver(n, from, "in") {
                Some((t, _)) if is_texture(n, t) => (t, integer(n, from, "index") as usize),
                _ => return self.unheld(input, held),
            },
            _ if is_texture(n, from) => (from, 0),
            _ => return self.unheld(input, held),
        };
        let factors = numbers(n, texture, "factor");
        let factor = match factors.len() {
            0 => 1.0,
            1 => factors[0],
            _ => factors.get(at).copied().unwrap_or(1.0),
        };
        (Some(texture), factor)
    }

    /// An input glTF can't hold: said, its own value written.
    fn unheld(&mut self, input: &str, held: f64) -> (Option<u32>, f64) {
        self.warn(format!(
            "{input}: what drives it isn't a glTF texture; its value is written"
        ));
        (None, held)
    }

    /// A color input: its texture and factor (linear Rec.709), else its
    /// value.
    fn rgb(
        &mut self,
        n: &MaterialNetwork,
        s: u32,
        input: &str,
    ) -> Result<(Option<TextureRef>, [f64; 3]), Error> {
        let (texture, rgb) = match driver(n, s, input) {
            Some((t, _)) if is_texture(n, t) => (self.texture(n, Some(t)), factor(n, t)),
            Some(_) => {
                self.unheld(input, 0.0);
                (None, color(n, s, input))
            }
            None => (None, color(n, s, input)),
        };
        Ok((texture, self.color(rgb)?))
    }

    /// A normal input: its texture and scale, through `normalmap` or a glTF
    /// normal map.
    fn normal(&mut self, n: &MaterialNetwork, s: u32, input: &str) -> (Option<TextureRef>, f64) {
        match driver(n, s, input) {
            Some((map, _)) if node(n, map) == "normalmap" => {
                let image = driver(n, map, "in").map(|d| d.0);
                (self.texture(n, image), number(n, map, "scale"))
            }
            Some((t, _)) if is_texture(n, t) => (self.texture(n, Some(t)), 1.0),
            _ => (None, 1.0),
        }
    }

    /// Texture node `at` as a reference to a glTF texture; none when it reads
    /// no image glTF holds.
    fn texture(&mut self, n: &MaterialNetwork, at: Option<u32>) -> Option<TextureRef> {
        let at = at.filter(|&t| is_texture(n, t))?;
        let kind = node(n, at);
        let (image, extended) = self.image(n.nodes()[at as usize].images.get("file")?)?;
        let text = |input: &str| choice(n, at, input);
        let (u, v, filter) = match kind {
            "UsdUVTexture" => (text("wrapS"), text("wrapT"), String::new()),
            _ => (
                text("uaddressmode"),
                text("vaddressmode"),
                text("filtertype"),
            ),
        };
        let sampler = Sampler {
            name: String::new(),
            mag_filter: match filter.as_str() {
                "closest" => Some(MagFilter::Nearest),
                "linear" | "cubic" => Some(MagFilter::Linear),
                _ => None,
            },
            min_filter: minification(&text("minfilter")),
            wrap_s: wrap(&u),
            wrap_t: wrap(&v),
        };
        let sampler = (sampler != Sampler::default()).then(|| self.sampler(sampler));
        let texture = match (self.looks.textures.iter()).find(|(k, _)| *k == (image, sampler)) {
            Some(&(_, t)) => t,
            None => {
                self.model.textures.push(gltf::Texture {
                    name: String::new(),
                    sampler,
                    source: (!extended).then_some(image),
                    extension_source: extended.then_some(image),
                });
                let t = self.model.textures.len() - 1;
                self.looks.textures.push(((image, sampler), t));
                t
            }
        };
        let gltf_family = kind.contains("gltf");
        let transform = match gltf_family {
            true => TextureTransform {
                offset: pair(&numbers(n, at, "offset"), 0.0),
                rotation: number(n, at, "rotate").to_radians(),
                scale: pair(&numbers(n, at, "scale"), 1.0),
            },
            false => TextureTransform::default(),
        };
        let tex_coord = match driver(
            n,
            at,
            if kind == "UsdUVTexture" {
                "st"
            } else {
                "texcoord"
            },
        ) {
            Some((t, _)) if node(n, t) == "texcoord" => integer(n, t, "index").max(0) as u32,
            _ => 0,
        };
        Some(TextureRef {
            texture,
            tex_coord,
            transform,
        })
    }

    /// The glTF image of `source`, made once, and whether it is an
    /// extension's (KTX2, WebP, AVIF); none for an image glTF doesn't hold
    /// or that can't be read, said.
    fn image(&mut self, source: &ImageSource) -> Option<(usize, bool)> {
        let file = source.file();
        if let Some((_, made)) = self.looks.images.iter().find(|(f, _)| f == file) {
            return made.map(|i| (i, extended(&self.model.images[i])));
        }
        let read = match file {
            ImageFile::Embedded(file) => Ok(file.bytes().to_vec()),
            ImageFile::Path(path) => std::fs::read(&**path).map_err(|e| format!("{path}: {e}")),
        };
        let name = source.name().to_owned();
        let made = match read {
            Err(why) => {
                self.warn(format!("{why}; the texture is left out"));
                None
            }
            Ok(bytes) => match gltf::sniff(&bytes).or_else(|| gltf::media_type(&name)) {
                Some(mime) => {
                    let stem = std::path::Path::new(&name).file_stem();
                    let stem = stem.map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                    self.model.images.push(gltf::Image {
                        name: stem,
                        mime_type: Some(mime.to_owned()),
                        data: gltf::ImageData::Embedded(bytes),
                    });
                    Some(self.model.images.len() - 1)
                }
                None => {
                    self.warn(format!(
                        "{name}: glTF holds PNG, JPEG, KTX2, WebP and AVIF images; the texture is left out"
                    ));
                    None
                }
            },
        };
        self.looks.images.push((file.clone(), made));
        made.map(|i| (i, extended(&self.model.images[i])))
    }

    /// The glTF sampler `sampler`, made once.
    fn sampler(&mut self, sampler: Sampler) -> usize {
        if let Some(&(_, s)) = self.looks.samplers.iter().find(|(k, _)| *k == sampler) {
            return s;
        }
        self.model.samplers.push(sampler.clone());
        let s = self.model.samplers.len() - 1;
        self.looks.samplers.push((sampler, s));
        s
    }
}

/// Whether `image` is an extension's: KTX2, WebP or AVIF.
fn extended(image: &gltf::Image) -> bool {
    !matches!(image.mime_type.as_deref(), Some("image/png" | "image/jpeg"))
}

/// What glTF defines as having no effect, at its defaults: a volume without
/// transmission, a clearcoat, sheen, iridescence or anisotropy of nothing.
fn without_effect(m: &mut gltf::Material) {
    if m.transmission.factor == 0.0 && m.transmission.texture.is_none() {
        m.volume = gltf::Volume::default();
    }
    if m.clearcoat.factor == 0.0 && m.clearcoat.texture.is_none() {
        m.clearcoat = gltf::Clearcoat::default();
    }
    if m.sheen.color == [0.0; 3] && m.sheen.color_texture.is_none() {
        m.sheen = gltf::Sheen::default();
    }
    if m.iridescence.factor == 0.0 && m.iridescence.texture.is_none() {
        m.iridescence = gltf::Iridescence::default();
    }
    if m.anisotropy.strength == 0.0 && m.anisotropy.texture.is_none() {
        m.anisotropy = gltf::Anisotropy::default();
    }
}

/// The MaterialX node `at` is (`gltf_pbr`); empty for another.
fn node(n: &MaterialNetwork, at: u32) -> &str {
    (n.nodes()[at as usize].def.mtlx()).map_or("", |d| d.node.as_str())
}

/// Whether node `at` is a texture node glTF reads.
fn is_texture(n: &MaterialNetwork, at: u32) -> bool {
    GLTF_TEXTURES.contains(&node(n, at))
}

/// What drives input `input` of `at`: a node and its output.
fn driver<'n>(n: &'n MaterialNetwork, at: u32, input: &str) -> Option<(u32, &'n str)> {
    (n.links().iter())
        .find(|l| l.to == at && &*l.input == input)
        .map(|l| (l.from, &*l.output))
}

fn number(n: &MaterialNetwork, at: u32, input: &str) -> f64 {
    (n.nodes()[at as usize].values.get(input))
        .and_then(Value::number)
        .unwrap_or(0.0)
}

fn numbers(n: &MaterialNetwork, at: u32, input: &str) -> Vec<f64> {
    match n.nodes()[at as usize].values.get(input) {
        Some(Value::Numbers(v)) => v.clone(),
        Some(Value::Color(c)) => c.to_vec(),
        Some(Value::Number(v)) => vec![*v],
        _ => Vec::new(),
    }
}

fn color(n: &MaterialNetwork, at: u32, input: &str) -> [f64; 3] {
    let v = numbers(n, at, input);
    [0, 1, 2].map(|k| v.get(k).or(v.first()).copied().unwrap_or(0.0))
}

/// A texture node's factor as a color: its `factor`'s, else white.
fn factor(n: &MaterialNetwork, at: u32) -> [f64; 3] {
    let v = numbers(n, at, "factor");
    [0, 1, 2].map(|k| v.get(k).or(v.first()).copied().unwrap_or(1.0))
}

fn integer(n: &MaterialNetwork, at: u32, input: &str) -> i64 {
    (n.nodes()[at as usize].values.get(input))
        .and_then(Value::integer)
        .unwrap_or(0)
}

fn choice(n: &MaterialNetwork, at: u32, input: &str) -> String {
    match n.nodes()[at as usize].values.get(input) {
        Some(Value::Choice(c)) => c.to_string(),
        Some(Value::Text(t)) => t.clone(),
        _ => String::new(),
    }
}

/// Two numbers, `fill` where `v` has none.
fn pair(v: &[f64], fill: f64) -> [f64; 2] {
    [
        v.first().copied().unwrap_or(fill),
        v.get(1).copied().unwrap_or(fill),
    ]
}

fn alpha_mode(mode: i64) -> AlphaMode {
    match mode {
        1 => AlphaMode::Mask,
        2 => AlphaMode::Blend,
        _ => AlphaMode::Opaque,
    }
}

/// glTF's wrap of a MaterialX address mode or USD wrap: periodic repeats,
/// mirror mirrors, the rest clamp.
fn wrap(mode: &str) -> Wrap {
    match mode {
        "periodic" | "repeat" | "" => Wrap::Repeat,
        "mirror" => Wrap::MirroredRepeat,
        _ => Wrap::ClampToEdge,
    }
}

/// glTF's minification filter of Kinogaki's `minfilter`.
fn minification(filter: &str) -> Option<MinFilter> {
    Some(match filter {
        "nearest" => MinFilter::Nearest,
        "linear" => MinFilter::Linear,
        "nearest_mipmap_nearest" => MinFilter::NearestMipmapNearest,
        "linear_mipmap_nearest" => MinFilter::LinearMipmapNearest,
        "nearest_mipmap_linear" => MinFilter::NearestMipmapLinear,
        "linear_mipmap_linear" => MinFilter::LinearMipmapLinear,
        _ => return None,
    })
}
