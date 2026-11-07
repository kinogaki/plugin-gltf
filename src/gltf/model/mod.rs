//! The plain data a glTF file holds, its binary data decoded.
//!
//! The model keeps glTF's own objects and indices (scenes, nodes, meshes,
//! materials, textures, samplers, images, cameras, lights), so it reads and
//! writes back structurally, and it hides only the encoding: buffers, buffer
//! views and accessors become typed arrays on the primitives, and images
//! their files' bytes. Material values carry MaterialX's glTF PBR names
//! (`base_color`, `metallic`, `emissive_strength`), each documented with the
//! glTF property it holds.
mod material;
mod mesh;

pub use material::{
    AlphaMode, Anisotropy, Clearcoat, Image, ImageData, Iridescence, MagFilter, Material,
    MinFilter, Sampler, Sheen, Specular, Texture, TextureRef, TextureTransform, Transmission,
    Volume, Wrap,
};
pub use mesh::{Attribute, Kind, Mesh, Primitive, Topology};
use std::fmt;

/// Everything read from one glTF or GLB file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gltf {
    pub asset: Asset,
    /// The scene shown first (`scene`).
    pub scene: Option<usize>,
    pub scenes: Vec<Scene>,
    pub nodes: Vec<Node>,
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    pub samplers: Vec<Sampler>,
    pub images: Vec<Image>,
    pub cameras: Vec<Camera>,
    /// `KHR_lights_punctual`'s lights.
    pub lights: Vec<Light>,
    /// What reading left out, to be said once each. Writing ignores it.
    pub left_out: Vec<LeftOut>,
}

/// The `asset` object's descriptive members (its `version` is always 2.0).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Asset {
    /// The tool that wrote the file; empty when not given.
    pub generator: String,
    /// The content's copyright; empty when not given.
    pub copyright: String,
}

/// A scene: the roots of its node trees.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    /// Empty when the file gives none (as for every `name` here).
    pub name: String,
    pub nodes: Vec<usize>,
}

/// A node of the scene's trees. Every node has at most one parent and no
/// node is its own ancestor (reading checks both).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub name: String,
    pub transform: Transform,
    pub children: Vec<usize>,
    pub mesh: Option<usize>,
    pub camera: Option<usize>,
    /// `KHR_lights_punctual`'s `light`.
    pub light: Option<usize>,
}

/// A node's placement relative to its parent, as the file gives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Transform {
    /// `matrix`: sixteen numbers, column by column.
    Matrix([f64; 16]),
    /// `translation`, `rotation` (a unit quaternion x, y, z, w) and `scale`,
    /// applied scale first.
    Trs {
        translation: [f64; 3],
        rotation: [f64; 4],
        scale: [f64; 3],
    },
}

impl Default for Transform {
    fn default() -> Self {
        Transform::Trs {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }
    }
}

/// A camera looking down its node's −Z, +Y up.
#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    pub name: String,
    pub projection: Projection,
}

/// A camera's projection, distances in the file's metres.
#[derive(Clone, Debug, PartialEq)]
pub enum Projection {
    /// `yfov` in radians; no `aspect_ratio` means the viewport's; no `zfar`
    /// means infinite.
    Perspective {
        yfov: f64,
        aspect_ratio: Option<f64>,
        znear: f64,
        zfar: Option<f64>,
    },
    /// Half the view's width and height.
    Orthographic {
        xmag: f64,
        ymag: f64,
        znear: f64,
        zfar: f64,
    },
}

/// A `KHR_lights_punctual` light, shining down its node's −Z.
#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    pub name: String,
    pub kind: LightKind,
    /// Linear RGB.
    pub color: [f64; 3],
    /// Candela for point and spot lights, lux for directional ones.
    pub intensity: f64,
    /// Where the light's influence ends, in metres; none means unbounded.
    pub range: Option<f64>,
}

/// What a light is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LightKind {
    Directional,
    Point,
    /// Cone angles from the axis, in radians.
    Spot {
        inner_cone_angle: f64,
        outer_cone_angle: f64,
    },
}

/// Something a file holds that reading leaves out, to be said once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeftOut {
    /// Animations, by count: a file reads at rest.
    Animations(usize),
    /// Skins, by count: meshes read in their bind pose, `JOINTS_n` and
    /// `WEIGHTS_n` dropped.
    Skins(usize),
    /// Meshes with morph targets, by count: each reads with its targets
    /// applied at its default `weights`, and node `weights` are ignored.
    MorphTargets(usize),
    /// An extension the file uses that isn't read.
    Extension(String),
    /// Objects carrying `extras`, by count.
    Extras(usize),
}

impl fmt::Display for LeftOut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plural = |n: &usize| if *n == 1 { "" } else { "s" };
        match self {
            LeftOut::Animations(n) => write!(
                f,
                "{n} animation{} left out: the file reads at rest",
                plural(n)
            ),
            LeftOut::Skins(n) => write!(
                f,
                "{n} skin{} left out: meshes read in their bind pose",
                plural(n)
            ),
            LeftOut::MorphTargets(n) => {
                write!(
                    f,
                    "morph targets of {n} mesh{} applied at their default weights",
                    if *n == 1 { "" } else { "es" }
                )
            }
            LeftOut::Extension(name) => write!(f, "{name} left out"),
            LeftOut::Extras(n) => write!(f, "extras of {n} object{} left out", plural(n)),
        }
    }
}
