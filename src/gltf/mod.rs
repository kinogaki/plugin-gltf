//! glTF 2.0 and GLB reading and writing.
//!
//! [`Gltf`] is a plain model of what a glTF file holds: its scenes, node
//! trees, meshes with decoded vertex attributes, materials with every
//! ratified `KHR_materials_*` extension MaterialX's glTF PBR carries,
//! textures, samplers, images as their files' bytes, cameras and
//! `KHR_lights_punctual` lights. It depends on nothing of the SDK but the
//! read's budget, which a read checks the file's claims against; the plugin
//! converts it to and from Studio's standard values.
//!
//! Reading takes `.gltf` text with the files it names, `data:` URIs, and GLB.
//! Writing gives a GLB, or `.gltf` text with its `.bin` and image files.
//! Reading what was written gives the same model back.
//!
//! ```
//! use kinogaki_gltf::gltf::{self, Gltf};
//! use kinogaki_studio_api::format::ReadBudget;
//! let model = Gltf { scenes: vec![gltf::Scene::default()], scene: Some(0), ..Gltf::default() };
//! let glb = gltf::to_glb(&model).unwrap();
//! let read = gltf::read(&glb, &mut |_| Err("no files".into()), &ReadBudget::default()).unwrap();
//! assert_eq!(read, model);
//! ```
mod base64;
mod error;
mod glb;
mod json;
mod model;
mod read;
mod uri;
mod write;

pub use error::GltfError;
pub use model::*;
pub use read::{EXTENSIONS, Files, read};
pub use write::{GltfFiles, to_glb, to_gltf, write_file};

/// The media type a file name's extension says (`image/png` of
/// `wood.PNG`): PNG, JPEG, KTX2, WebP or AVIF.
pub fn media_type(name: &str) -> Option<&'static str> {
    uri::by_extension(name)
}

/// The media type an image's first bytes say: PNG, JPEG, KTX2, WebP or
/// AVIF.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    uri::sniff(bytes)
}
