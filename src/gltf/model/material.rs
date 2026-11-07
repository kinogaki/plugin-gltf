//! Materials (glTF PBR with the ratified KHR_materials extensions),
//! textures, samplers and images.
//!
//! An extension's values sit in a struct whose `Default` is the extension
//! absent; a struct at its default is not written, which changes no look.

/// A material: glTF's metallic-roughness model and its extensions. Names
/// follow MaterialX's `gltf_pbr`.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    /// `baseColorFactor`: linear RGBA.
    pub base_color: [f64; 4],
    pub base_color_texture: Option<TextureRef>,
    /// `metallicFactor`.
    pub metallic: f64,
    /// `roughnessFactor`.
    pub roughness: f64,
    /// Roughness in G, metalness in B.
    pub metallic_roughness_texture: Option<TextureRef>,
    pub normal_texture: Option<TextureRef>,
    /// `normalTexture.scale`.
    pub normal_scale: f64,
    /// Occlusion in R.
    pub occlusion_texture: Option<TextureRef>,
    /// `occlusionTexture.strength`.
    pub occlusion_strength: f64,
    /// `emissiveFactor`: linear RGB.
    pub emissive: [f64; 3],
    pub emissive_texture: Option<TextureRef>,
    /// `KHR_materials_emissive_strength`'s `emissiveStrength`.
    pub emissive_strength: f64,
    pub alpha_mode: AlphaMode,
    /// `alphaCutoff`, which only `Mask` reads.
    pub alpha_cutoff: f64,
    pub double_sided: bool,
    /// `KHR_materials_unlit`: the base color shown as it is.
    pub unlit: bool,
    /// `KHR_materials_ior`'s `ior`.
    pub ior: f64,
    /// `KHR_materials_dispersion`'s `dispersion`.
    pub dispersion: f64,
    pub transmission: Transmission,
    pub volume: Volume,
    pub clearcoat: Clearcoat,
    pub specular: Specular,
    pub sheen: Sheen,
    pub iridescence: Iridescence,
    pub anisotropy: Anisotropy,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            name: String::new(),
            base_color: [1.0; 4],
            base_color_texture: None,
            metallic: 1.0,
            roughness: 1.0,
            metallic_roughness_texture: None,
            normal_texture: None,
            normal_scale: 1.0,
            occlusion_texture: None,
            occlusion_strength: 1.0,
            emissive: [0.0; 3],
            emissive_texture: None,
            emissive_strength: 1.0,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            unlit: false,
            ior: 1.5,
            dispersion: 0.0,
            transmission: Transmission::default(),
            volume: Volume::default(),
            clearcoat: Clearcoat::default(),
            specular: Specular::default(),
            sheen: Sheen::default(),
            iridescence: Iridescence::default(),
            anisotropy: Anisotropy::default(),
        }
    }
}

/// How alpha shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlphaMode {
    /// Alpha ignored.
    #[default]
    Opaque,
    /// Visible where alpha reaches `alpha_cutoff`, else not.
    Mask,
    /// Blended over what is behind.
    Blend,
}

/// `KHR_materials_transmission`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transmission {
    /// `transmissionFactor`.
    pub factor: f64,
    /// In R.
    pub texture: Option<TextureRef>,
}

/// `KHR_materials_volume`.
#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    /// `thicknessFactor`, in the mesh's units.
    pub thickness: f64,
    /// In G.
    pub thickness_texture: Option<TextureRef>,
    /// `attenuationDistance`; none means infinite.
    pub attenuation_distance: Option<f64>,
    pub attenuation_color: [f64; 3],
}

impl Default for Volume {
    fn default() -> Self {
        Volume {
            thickness: 0.0,
            thickness_texture: None,
            attenuation_distance: None,
            attenuation_color: [1.0; 3],
        }
    }
}

/// `KHR_materials_clearcoat`.
#[derive(Clone, Debug, PartialEq)]
pub struct Clearcoat {
    /// `clearcoatFactor`.
    pub factor: f64,
    /// In R.
    pub texture: Option<TextureRef>,
    /// `clearcoatRoughnessFactor`.
    pub roughness: f64,
    /// In G.
    pub roughness_texture: Option<TextureRef>,
    pub normal_texture: Option<TextureRef>,
    /// `clearcoatNormalTexture.scale`.
    pub normal_scale: f64,
}

impl Default for Clearcoat {
    fn default() -> Self {
        Clearcoat {
            factor: 0.0,
            texture: None,
            roughness: 0.0,
            roughness_texture: None,
            normal_texture: None,
            normal_scale: 1.0,
        }
    }
}

/// `KHR_materials_specular`.
#[derive(Clone, Debug, PartialEq)]
pub struct Specular {
    /// `specularFactor`.
    pub factor: f64,
    /// In A.
    pub texture: Option<TextureRef>,
    /// `specularColorFactor`.
    pub color: [f64; 3],
    /// In RGB.
    pub color_texture: Option<TextureRef>,
}

impl Default for Specular {
    fn default() -> Self {
        Specular {
            factor: 1.0,
            texture: None,
            color: [1.0; 3],
            color_texture: None,
        }
    }
}

/// `KHR_materials_sheen`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sheen {
    /// `sheenColorFactor`.
    pub color: [f64; 3],
    /// In RGB.
    pub color_texture: Option<TextureRef>,
    /// `sheenRoughnessFactor`.
    pub roughness: f64,
    /// In A.
    pub roughness_texture: Option<TextureRef>,
}

/// `KHR_materials_iridescence`.
#[derive(Clone, Debug, PartialEq)]
pub struct Iridescence {
    /// `iridescenceFactor`.
    pub factor: f64,
    /// In R.
    pub texture: Option<TextureRef>,
    /// `iridescenceIor`.
    pub ior: f64,
    /// `iridescenceThicknessMinimum`, nanometres.
    pub thickness_minimum: f64,
    /// `iridescenceThicknessMaximum`, nanometres.
    pub thickness_maximum: f64,
    /// In G, between the minimum and maximum.
    pub thickness_texture: Option<TextureRef>,
}

impl Default for Iridescence {
    fn default() -> Self {
        Iridescence {
            factor: 0.0,
            texture: None,
            ior: 1.3,
            thickness_minimum: 100.0,
            thickness_maximum: 400.0,
            thickness_texture: None,
        }
    }
}

/// `KHR_materials_anisotropy`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Anisotropy {
    /// `anisotropyStrength`.
    pub strength: f64,
    /// `anisotropyRotation`, radians counter-clockwise from the tangent.
    pub rotation: f64,
    /// Direction in RG, strength in B.
    pub texture: Option<TextureRef>,
}

/// A material's use of a texture.
#[derive(Clone, Debug, PartialEq)]
pub struct TextureRef {
    /// Into [`Gltf::textures`](super::Gltf::textures).
    pub texture: usize,
    /// The `TEXCOORD_n` set: `KHR_texture_transform`'s `texCoord` when it
    /// gives one, else the reference's own.
    pub tex_coord: u32,
    /// `KHR_texture_transform`; the identity when absent.
    pub transform: TextureTransform,
}

/// `KHR_texture_transform`: UVs scaled, then rotated, then offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextureTransform {
    pub offset: [f64; 2],
    /// Radians, counter-clockwise in UV space.
    pub rotation: f64,
    pub scale: [f64; 2],
}

impl Default for TextureTransform {
    fn default() -> Self {
        TextureTransform {
            offset: [0.0; 2],
            rotation: 0.0,
            scale: [1.0; 2],
        }
    }
}

/// A texture: an image and how it is sampled.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Texture {
    pub name: String,
    /// None samples with repeat wrapping and the renderer's filters.
    pub sampler: Option<usize>,
    /// `source`: a PNG or JPEG image.
    pub source: Option<usize>,
    /// `KHR_texture_basisu`'s, `EXT_texture_webp`'s or `EXT_texture_avif`'s
    /// `source`, which readers decoding it prefer over `source` (then a
    /// fallback, or none). Its image's MIME type names the extension.
    pub extension_source: Option<usize>,
}

/// How a texture is filtered and wrapped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sampler {
    pub name: String,
    /// None leaves magnification to the renderer.
    pub mag_filter: Option<MagFilter>,
    /// None leaves minification to the renderer.
    pub min_filter: Option<MinFilter>,
    /// Along U.
    pub wrap_s: Wrap,
    /// Along V.
    pub wrap_t: Wrap,
}

/// Magnification filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MagFilter {
    Nearest,
    Linear,
}

/// Minification filters, with mipmaps or without.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MinFilter {
    Nearest,
    Linear,
    NearestMipmapNearest,
    LinearMipmapNearest,
    NearestMipmapLinear,
    LinearMipmapLinear,
}

/// Wrap modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wrap {
    ClampToEdge,
    MirroredRepeat,
    #[default]
    Repeat,
}

/// An image, kept as the file's bytes or the path it names; never decoded.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub name: String,
    /// `mimeType`, else what the bytes or the path's extension say; none
    /// when neither says.
    pub mime_type: Option<String>,
    pub data: ImageData,
}

/// Where an image's file is.
#[derive(Clone, Debug, PartialEq)]
pub enum ImageData {
    /// In the file (a buffer view or a `data:` URI), or read from beside it
    /// when reading embeds images.
    Embedded(Vec<u8>),
    /// A path relative to the glTF file, percent-decoded.
    Path(String),
}
