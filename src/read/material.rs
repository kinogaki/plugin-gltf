//! glTF's materials as MaterialX glTF PBR networks, field for field (glTF
//! PBR was written for glTF), and unlit ones as Surface Unlit Cutout.
//!
//! A texture is a Kinogaki glTF texture node (`kinogaki_gltf_image`, with
//! glTF's minification filter and an empty filter for an undefined one)
//! holding its sampler and `KHR_texture_transform` (the rotation in
//! degrees), a `texcoord` node wired in for a set other than the first,
//! and an Image node wired into its `file`: one Image node a distinct image
//! and color reading in a material. A channel glTF packs is read through
//! `extract`; occlusion's strength is a `mix` with 1. Colors are converted
//! from linear Rec.709 into the working space, lengths into the project's
//! units.
use super::mesh::key;
use crate::gltf::{self, AlphaMode, Gltf, MagFilter, MinFilter, TextureRef, Wrap};
use kinogaki_studio_api::color::{ColorService, LIN_REC709};
use kinogaki_studio_api::format::Conversion;
use kinogaki_studio_api::geometry::image::{ImageColor, ImageFile, ImageSource};
use kinogaki_studio_api::image_files::image_node;
use kinogaki_studio_api::network::{NetworkSeed, NewNode};
use kinogaki_studio_api::standard::ImportedMaterial;
use kinogaki_studio_api::{Error, Value};
use std::sync::Arc;

/// Each of `model`'s materials, keyed by index ([`key`]): `colored` says
/// which a primitive with vertex colors reads.
pub(super) fn materials(
    model: &Gltf,
    files: &[Option<ImageFile>],
    colored: &[bool],
    colors: &dyn ColorService,
    space: &Conversion,
) -> Result<Vec<ImportedMaterial>, Error> {
    (model.materials.iter().enumerate())
        .map(|(at, m)| {
            let mut seed = Seed {
                model,
                files,
                colors,
                space,
                seed: NetworkSeed::default(),
                images: Vec::new(),
                texcoords: Vec::new(),
            };
            match m.unlit {
                true => seed.unlit(m, colored[at])?,
                false => seed.gltf_pbr(m, colored[at])?,
            };
            Ok(ImportedMaterial {
                key: key(at),
                name: match m.name.trim() {
                    "" => "Material".to_owned(),
                    name => name.to_owned(),
                },
                inside: seed.seed,
                standard: None,
                double_sided: m.double_sided,
            })
        })
        .collect()
}

/// A material's network as it is made.
struct Seed<'a> {
    model: &'a Gltf,
    files: &'a [Option<ImageFile>],
    colors: &'a dyn ColorService,
    space: &'a Conversion,
    seed: NetworkSeed,
    /// The Image nodes made: an image read as data or color, and its node.
    images: Vec<((usize, bool), usize)>,
    /// The `texcoord` nodes made: a set and its node.
    texcoords: Vec<(u32, usize)>,
}

/// What `extract` takes out of a texture's channels.
#[derive(Clone, Copy)]
enum Channel {
    R,
    G,
    A,
}

impl Seed<'_> {
    /// A node of MaterialX's or Kinogaki's definition `signature`.
    fn node(&mut self, signature: &str) -> usize {
        let node = NewNode::materialx(signature).expect("the library defines it");
        self.seed.node(node)
    }

    /// Sets input `name` of node `at` to `value`.
    fn set(&mut self, at: usize, name: &str, value: impl Into<Value>) {
        self.seed.nodes[at]
            .parameters
            .insert(name.to_owned(), value.into());
    }

    /// Sets `name` to `value` unless it is glTF's default, `default`.
    fn number(&mut self, at: usize, name: &str, value: f64, default: f64) {
        if value != default {
            self.set(at, name, value);
        }
    }

    /// `rgb`, linear Rec.709, in the working space.
    fn color(&self, rgb: [f64; 3]) -> Result<[f64; 3], Error> {
        let mut c = [rgb];
        self.colors.to_working(LIN_REC709, &mut c)?;
        Ok(c[0])
    }

    /// Sets color `name` to `rgb` (linear Rec.709) unless it is `default`.
    fn color_input(
        &mut self,
        at: usize,
        name: &str,
        rgb: [f64; 3],
        default: [f64; 3],
    ) -> Result<(), Error> {
        if rgb != default {
            let c = self.color(rgb)?;
            self.set(at, name, Value::Color(c));
        }
        Ok(())
    }

    /// The Image node reading image `image` as data or as color; none for
    /// an image left out.
    fn image(&mut self, image: usize, data: bool) -> Option<usize> {
        if let Some(&(_, at)) = self.images.iter().find(|(k, _)| *k == (image, data)) {
            return Some(at);
        }
        let color = match data {
            true => ImageColor::Data,
            false => ImageColor::Auto,
        };
        let source = ImageSource::new(self.files[image].clone()?, color);
        let stem = std::path::Path::new(source.name()).file_stem();
        let name = stem.map_or_else(|| "Image".into(), |s| s.to_string_lossy().into_owned());
        let at = self.seed.node(image_node::reading(&source).named(name));
        self.images.push(((image, data), at));
        Some(at)
    }

    /// The `texcoord` node reading set `set`.
    fn texcoord(&mut self, set: u32) -> usize {
        if let Some(&(_, at)) = self.texcoords.iter().find(|(s, _)| *s == set) {
            return at;
        }
        let at = self.node("ND_texcoord_vector2");
        self.set(at, "index", Value::Integer(i64::from(set)));
        self.texcoords.push((set, at));
        at
    }

    /// A texture node of `signature` reading `r`: its image, sampler,
    /// transform and coordinate set.
    fn texture(&mut self, r: &TextureRef, signature: &str, data: bool) -> usize {
        let at = self.node(signature);
        let texture = &self.model.textures[r.texture];
        // The image a decoder reads: `source` (PNG or JPEG), else the
        // extension's (KHR_texture_basisu, EXT_texture_webp or avif).
        let image = texture.source.or(texture.extension_source).expect("read");
        if let Some(file) = self.image(image, data) {
            self.seed.wire(file, image_node::IMAGE, at, "file");
        }
        let sampler = texture.sampler.map(|s| self.model.samplers[s].clone());
        let sampler = sampler.unwrap_or_default();
        for (name, wrap) in [
            ("uaddressmode", sampler.wrap_s),
            ("vaddressmode", sampler.wrap_t),
        ] {
            if wrap != Wrap::Repeat {
                self.set(at, name, choice(address(wrap)));
            }
        }
        if sampler.mag_filter != Some(MagFilter::Linear) {
            self.set(at, "filtertype", choice(mag(sampler.mag_filter)));
        }
        if let Some(min) = sampler.min_filter {
            self.set(at, "minfilter", choice(minification(min)));
        }
        let t = r.transform;
        if t.offset != [0.0; 2] {
            self.set(at, "offset", Value::Numbers(t.offset.to_vec()));
        }
        if t.rotation != 0.0 {
            self.set(at, "rotate", t.rotation.to_degrees());
        }
        if t.scale != [1.0; 2] {
            self.set(at, "scale", Value::Numbers(t.scale.to_vec()));
        }
        if r.tex_coord != 0 {
            let set = self.texcoord(r.tex_coord);
            self.seed.wire(set, "out", at, "texcoord");
        }
        at
    }

    /// A one-channel glTF texture `r` times `factor` into `input` of `to`,
    /// else `factor` itself unless it is `default`.
    fn channel(
        &mut self,
        to: usize,
        input: &str,
        r: Option<&TextureRef>,
        channel: Channel,
        factor: f64,
        default: f64,
    ) {
        let Some(r) = r else {
            return self.number(to, input, factor, default);
        };
        let (signature, extract, factors) = match channel {
            Channel::R => ("ND_kinogaki_gltf_image_float", None, Value::Number(factor)),
            Channel::G => (
                "ND_kinogaki_gltf_image_color3",
                Some(("ND_extract_color3", 1)),
                Value::Color([1.0, factor, 1.0]),
            ),
            Channel::A => (
                "ND_kinogaki_gltf_image_color4",
                Some(("ND_extract_color4", 3)),
                Value::Numbers(vec![1.0, 1.0, 1.0, factor]),
            ),
        };
        let image = self.texture(r, signature, true);
        if factor != 1.0 {
            self.set(image, "factor", factors);
        }
        let out = match extract {
            Some((signature, index)) => {
                let e = self.node(signature);
                self.set(e, "index", Value::Integer(index));
                self.seed.wire(image, "out", e, "in");
                e
            }
            None => image,
        };
        self.seed.wire(out, "out", to, input);
    }

    /// An RGB glTF texture `r` times `factor` (a color, linear Rec.709)
    /// into `input` of `to`, else `factor` unless it is `default`.
    fn rgb(
        &mut self,
        to: usize,
        input: &str,
        r: Option<&TextureRef>,
        factor: [f64; 3],
        default: [f64; 3],
    ) -> Result<(), Error> {
        let Some(r) = r else {
            return self.color_input(to, input, factor, default);
        };
        let image = self.texture(r, "ND_kinogaki_gltf_image_color3", false);
        if factor != [1.0; 3] {
            let c = self.color(factor)?;
            self.set(image, "factor", Value::Color(c));
        }
        self.seed.wire(image, "out", to, input);
        Ok(())
    }

    /// A normal texture `r` scaled by `scale` into `input` of `to`.
    fn normal(&mut self, to: usize, input: &str, r: Option<&TextureRef>, scale: f64) {
        let Some(r) = r else { return };
        let image = self.texture(r, "ND_kinogaki_gltf_image_vector3", true);
        self.set(image, "default", Value::Numbers(vec![0.5, 0.5, 1.0]));
        let map = self.node("ND_normalmap_float");
        self.number(map, "scale", scale, 1.0);
        self.seed.wire(image, "out", map, "in");
        self.seed.wire(map, "out", to, input);
    }

    /// The base color and alpha into `color` and `alpha` of `to`: a
    /// `gltf_colorimage` of the texture times the factor (and the vertex
    /// colors when `colored`), else the vertex colors times the factor's
    /// color, else the factor.
    fn base(
        &mut self,
        m: &gltf::Material,
        colored: bool,
        to: usize,
        (color, alpha): (&str, &str),
    ) -> Result<(), Error> {
        let [r, g, b, a] = m.base_color;
        match &m.base_color_texture {
            Some(t) => {
                let image = self.texture(t, "ND_kinogaki_gltf_colorimage", false);
                if m.base_color != [1.0; 4] {
                    let [r, g, b] = self.color([r, g, b])?;
                    self.set(image, "color", Value::Numbers(vec![r, g, b, a]));
                }
                if colored {
                    let vertex = self.node("ND_geomcolor_color4");
                    self.seed.wire(vertex, "out", image, "geomcolor");
                }
                self.seed.wire(image, "outcolor", to, color);
                self.seed.wire(image, "outa", to, alpha);
            }
            None if colored => {
                let vertex = self.node("ND_geomcolor_color3");
                let times = self.node("ND_multiply_color3");
                self.seed.wire(vertex, "out", times, "in1");
                let c = self.color([r, g, b])?;
                self.set(times, "in2", Value::Color(c));
                self.seed.wire(times, "out", to, color);
                self.number(to, alpha, a, 1.0);
            }
            None => {
                self.color_input(to, color, [r, g, b], [1.0; 3])?;
                self.number(to, alpha, a, 1.0);
            }
        }
        Ok(())
    }

    /// Alpha mode and cutoff onto `to`, always for Surface Unlit Cutout
    /// (whose mode defaults to blending).
    fn alpha(&mut self, m: &gltf::Material, to: usize, always: bool) {
        let mode = match m.alpha_mode {
            AlphaMode::Opaque => 0,
            AlphaMode::Mask => 1,
            AlphaMode::Blend => 2,
        };
        if always || mode != 0 {
            self.set(to, "alpha_mode", Value::Integer(mode));
        }
        self.number(to, "alpha_cutoff", m.alpha_cutoff, 0.5);
    }

    /// `KHR_materials_unlit`: Surface Unlit Cutout of the base color.
    fn unlit(&mut self, m: &gltf::Material, colored: bool) -> Result<(), Error> {
        let s = self.node("ND_kinogaki_surface_unlit");
        self.seed.nodes[s].display = true;
        self.base(m, colored, s, ("emission_color", "opacity"))?;
        self.alpha(m, s, true);
        Ok(())
    }

    /// glTF PBR, every field in its input.
    fn gltf_pbr(&mut self, m: &gltf::Material, colored: bool) -> Result<(), Error> {
        let s = self.node("ND_gltf_pbr_surfaceshader");
        self.seed.nodes[s].display = true;
        self.base(m, colored, s, ("base_color", "alpha"))?;
        match &m.metallic_roughness_texture {
            Some(t) => {
                let image = self.texture(t, "ND_kinogaki_gltf_image_color3", true);
                if (m.roughness, m.metallic) != (1.0, 1.0) {
                    self.set(
                        image,
                        "factor",
                        Value::Color([1.0, m.roughness, m.metallic]),
                    );
                }
                for (index, input) in [(2, "metallic"), (1, "roughness")] {
                    let e = self.node("ND_extract_color3");
                    self.set(e, "index", Value::Integer(index));
                    self.seed.wire(image, "out", e, "in");
                    self.seed.wire(e, "out", s, input);
                }
            }
            None => {
                self.number(s, "metallic", m.metallic, 1.0);
                self.number(s, "roughness", m.roughness, 1.0);
            }
        }
        self.normal(s, "normal", m.normal_texture.as_ref(), m.normal_scale);
        if let Some(t) = &m.occlusion_texture {
            let image = self.texture(t, "ND_kinogaki_gltf_image_float", true);
            let mix = self.node("ND_mix_float");
            self.seed.wire(image, "out", mix, "fg");
            self.set(mix, "bg", 1.0);
            self.set(mix, "mix", m.occlusion_strength);
            self.seed.wire(mix, "out", s, "occlusion");
        }
        self.rgb(
            s,
            "emissive",
            m.emissive_texture.as_ref(),
            m.emissive,
            [0.0; 3],
        )?;
        self.number(s, "emissive_strength", m.emissive_strength, 1.0);
        self.alpha(m, s, false);
        self.number(s, "ior", m.ior, 1.5);
        self.number(s, "dispersion", m.dispersion, 0.0);
        let t = &m.transmission;
        self.channel(
            s,
            "transmission",
            t.texture.as_ref(),
            Channel::R,
            t.factor,
            0.0,
        );
        let v = &m.volume;
        let thickness = self.space.length(v.thickness);
        let texture = v.thickness_texture.as_ref();
        self.channel(s, "thickness", texture, Channel::G, thickness, 0.0);
        if let Some(d) = v.attenuation_distance {
            self.set(s, "attenuation_distance", self.space.length(d));
        }
        self.color_input(s, "attenuation_color", v.attenuation_color, [1.0; 3])?;
        let c = &m.clearcoat;
        self.channel(
            s,
            "clearcoat",
            c.texture.as_ref(),
            Channel::R,
            c.factor,
            0.0,
        );
        let rough = c.roughness_texture.as_ref();
        self.channel(
            s,
            "clearcoat_roughness",
            rough,
            Channel::G,
            c.roughness,
            0.0,
        );
        self.normal(
            s,
            "clearcoat_normal",
            c.normal_texture.as_ref(),
            c.normal_scale,
        );
        let sp = &m.specular;
        self.channel(
            s,
            "specular",
            sp.texture.as_ref(),
            Channel::A,
            sp.factor,
            1.0,
        );
        let texture = sp.color_texture.as_ref();
        self.rgb(s, "specular_color", texture, sp.color, [1.0; 3])?;
        let sh = &m.sheen;
        self.rgb(
            s,
            "sheen_color",
            sh.color_texture.as_ref(),
            sh.color,
            [0.0; 3],
        )?;
        let rough = sh.roughness_texture.as_ref();
        self.channel(s, "sheen_roughness", rough, Channel::A, sh.roughness, 0.0);
        self.iridescence(s, &m.iridescence);
        self.anisotropy(s, &m.anisotropy);
        Ok(())
    }

    /// `KHR_materials_iridescence`: its thickness the maximum without a
    /// texture, else a `gltf_iridescence_thickness` between the two.
    fn iridescence(&mut self, s: usize, i: &gltf::Iridescence) {
        if *i == gltf::Iridescence::default() {
            return;
        }
        self.channel(
            s,
            "iridescence",
            i.texture.as_ref(),
            Channel::R,
            i.factor,
            0.0,
        );
        self.number(s, "iridescence_ior", i.ior, 1.3);
        match &i.thickness_texture {
            Some(t) => {
                let image = self.texture(t, "ND_kinogaki_gltf_iridescence_thickness", true);
                self.number(image, "thicknessMin", i.thickness_minimum, 100.0);
                self.number(image, "thicknessMax", i.thickness_maximum, 400.0);
                self.seed.wire(image, "out", s, "iridescence_thickness");
            }
            None => self.number(s, "iridescence_thickness", i.thickness_maximum, 100.0),
        }
    }

    /// `KHR_materials_anisotropy`, through `gltf_anisotropy_image` with a
    /// texture.
    fn anisotropy(&mut self, s: usize, a: &gltf::Anisotropy) {
        let Some(t) = &a.texture else {
            self.number(s, "anisotropy_strength", a.strength, 0.0);
            return self.number(s, "anisotropy_rotation", a.rotation, 0.0);
        };
        let image = self.texture(t, "ND_kinogaki_gltf_anisotropy_image", true);
        self.number(image, "anisotropy_strength", a.strength, 1.0);
        self.number(image, "anisotropy_rotation", a.rotation, 0.0);
        for output in ["anisotropy_strength", "anisotropy_rotation"] {
            self.seed.wire(image, &format!("{output}_out"), s, output);
        }
    }
}

/// A choice's value.
fn choice(key: &str) -> Value {
    Value::Choice(Arc::from(key))
}

/// MaterialX's address mode of a glTF wrap.
pub(crate) fn address(wrap: Wrap) -> &'static str {
    match wrap {
        Wrap::ClampToEdge => "clamp",
        Wrap::MirroredRepeat => "mirror",
        Wrap::Repeat => "periodic",
    }
}

/// MaterialX's filter of a glTF magnification filter; empty for none.
pub(crate) fn mag(filter: Option<MagFilter>) -> &'static str {
    match filter {
        None => "",
        Some(MagFilter::Nearest) => "closest",
        Some(MagFilter::Linear) => "linear",
    }
}

/// Kinogaki's `minfilter` of a glTF minification filter.
pub(crate) fn minification(filter: MinFilter) -> &'static str {
    match filter {
        MinFilter::Nearest => "nearest",
        MinFilter::Linear => "linear",
        MinFilter::NearestMipmapNearest => "nearest_mipmap_nearest",
        MinFilter::LinearMipmapNearest => "linear_mipmap_nearest",
        MinFilter::NearestMipmapLinear => "nearest_mipmap_linear",
        MinFilter::LinearMipmapLinear => "linear_mipmap_linear",
    }
}
