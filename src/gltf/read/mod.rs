//! A glTF or GLB file to a [`Gltf`].
//!
//! The JSON is parsed whole, buffers are found (the GLB's binary chunk,
//! `data:` URIs, files beside), then every object is read in dependency
//! order (images, samplers, textures, materials, meshes, cameras, lights,
//! nodes, scenes), each index checked against what it names. Accessors
//! decode when a primitive first uses them.
mod accessor;
mod fields;
mod material;
mod mesh;
mod scene;

use super::json::{self, Value};
use super::model::{Asset, Gltf, Image, ImageData, LeftOut};
use super::uri::{self, Uri};
use super::{GltfError, glb};
use accessor::{Accessor, View};
use fields::Obj;
use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{fs, io};

type Result<T> = std::result::Result<T, GltfError>;

/// The extensions read. A file requiring another is refused; one using
/// another reads without it, which [`Gltf::left_out`] says.
pub const EXTENSIONS: [&str; 17] = [
    "KHR_materials_anisotropy",
    "KHR_materials_clearcoat",
    "KHR_materials_dispersion",
    "KHR_materials_emissive_strength",
    "KHR_materials_ior",
    "KHR_materials_iridescence",
    "KHR_materials_sheen",
    "KHR_materials_specular",
    "KHR_materials_transmission",
    "KHR_materials_unlit",
    "KHR_materials_volume",
    "KHR_texture_transform",
    "KHR_lights_punctual",
    "KHR_mesh_quantization",
    "KHR_texture_basisu",
    "EXT_texture_webp",
    "EXT_texture_avif",
];

/// How to read.
#[derive(Clone, Debug)]
pub struct ReadOptions {
    /// Read images a file names by path into their bytes, as embedded ones.
    pub embed_images: bool,
    /// The most bytes decoded attributes and indices may take: a bound on
    /// what untrusted input allocates.
    pub max_decoded_bytes: u64,
    /// Raised, it stops a read between primitives with
    /// [`GltfError::Cancelled`].
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for ReadOptions {
    fn default() -> Self {
        ReadOptions {
            embed_images: false,
            max_decoded_bytes: 16 << 30,
            cancel: None,
        }
    }
}

/// Reads the file at `path`, finding the files it names beside it.
pub fn read_file(path: &Path, options: &ReadOptions) -> Result<Gltf> {
    let bytes = fs::read(path).map_err(|error| GltfError::Io {
        file: path.display().to_string(),
        error,
    })?;
    let beside = path.parent().unwrap_or(Path::new(""));
    read(
        &bytes,
        &mut |relative| fs::read(beside.join(relative)),
        options,
    )
}

/// Reads a glTF (JSON) or GLB file held in `bytes`. `files` gives the bytes
/// of a file the glTF names by a relative path (percent-decoded); with no
/// files beside (an embedded file), it returns an error.
pub fn read(bytes: &[u8], files: &mut Files<'_>, options: &ReadOptions) -> Result<Gltf> {
    let (json, bin) = if glb::is_glb(bytes) {
        glb::split(bytes)?
    } else {
        (bytes, None)
    };
    let json = json.strip_prefix(b"\xef\xbb\xbf").unwrap_or(json);
    let text = std::str::from_utf8(json).map_err(|e| {
        GltfError::malformed("JSON", format!("not UTF-8 at byte {}", e.valid_up_to()))
    })?;
    let value = json::parse(text).map_err(|e| GltfError::malformed("JSON", e.to_string()))?;
    let root = Obj::new(&value, String::new())?;
    let asset = asset(&root)?;
    let mut left_out = extensions(&root)?;
    let mut reader = Reader::new(&root, bin, files, options)?;
    let images = root.each("images", |image| reader.image(image, files))?;
    let samplers = root.each("samplers", material::sampler)?;
    let textures = root.each("textures", |texture| {
        material::texture(texture, samplers.len(), images.len())
    })?;
    let materials = root.each("materials", |m| material::material(m, textures.len()))?;
    let meshes = root.each("meshes", |mesh| reader.mesh(mesh, materials.len()))?;
    let cameras = root.each("cameras", scene::camera)?;
    let lights = match root.extension("KHR_lights_punctual")? {
        Some(lights) => lights.each("lights", scene::light)?,
        None => Vec::new(),
    };
    let counts = scene::Counts {
        nodes: root.len("nodes"),
        meshes: meshes.len(),
        cameras: cameras.len(),
        lights: lights.len(),
    };
    let nodes = root.each("nodes", |node| scene::node(node, &counts))?;
    let scenes = root.each("scenes", |scene| scene::scene(scene, nodes.len()))?;
    scene::check_trees(&nodes, &scenes)?;
    let scene = root.index("scene", scenes.len())?;
    let counted = [
        (
            reader.morphed_meshes,
            LeftOut::MorphTargets as fn(usize) -> LeftOut,
        ),
        (root.len("animations"), LeftOut::Animations),
        (root.len("skins"), LeftOut::Skins),
        (extras(&value), LeftOut::Extras),
    ];
    left_out.extend(
        counted
            .into_iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, kind)| kind(n)),
    );
    Ok(Gltf {
        asset,
        scene,
        scenes,
        nodes,
        meshes,
        materials,
        textures,
        samplers,
        images,
        cameras,
        lights,
        left_out,
    })
}

/// A source of the files a glTF names beside it.
type Files<'f> = dyn FnMut(&str) -> io::Result<Vec<u8>> + 'f;

/// A read's binary data: buffers, views and accessors, decoded on use
/// within the budget.
pub(super) struct Reader<'a, 'o> {
    buffers: Vec<Cow<'a, [u8]>>,
    views: Vec<View>,
    accessors: Vec<Accessor>,
    /// Decoded attributes by accessor, shared by the primitives using them.
    decoded: Vec<Option<Arc<[f32]>>>,
    options: &'o ReadOptions,
    budget: u64,
    /// Meshes whose morph targets were applied.
    morphed_meshes: usize,
}

impl<'a, 'o> Reader<'a, 'o> {
    fn new(
        root: &Obj<'_, '_>,
        bin: Option<&'a [u8]>,
        files: &mut Files<'_>,
        options: &'o ReadOptions,
    ) -> Result<Self> {
        let mut buffers = Vec::new();
        for (i, buffer) in root.objects("buffers")?.iter().enumerate() {
            let length = buffer.required_count("byteLength")?;
            let bytes = match buffer.opt_str("uri")? {
                Some(uri) => Cow::Owned(load(buffer, uri, files)?),
                None if i == 0 => Cow::Borrowed(
                    bin.ok_or_else(|| buffer.error("uri", "missing, with no GLB binary chunk"))?,
                ),
                None => return Err(buffer.error("uri", "missing")),
            };
            if bytes.len() < length {
                return Err(buffer.error(
                    "byteLength",
                    format!("{length}, but the data holds {}", bytes.len()),
                ));
            }
            buffers.push(bytes);
        }
        let views = root.each("bufferViews", |view| View::read(view, &buffers))?;
        let accessors = root.each("accessors", |accessor| {
            Accessor::read(accessor, views.len())
        })?;
        Ok(Reader {
            decoded: vec![None; accessors.len()],
            buffers,
            views,
            accessors,
            budget: options.max_decoded_bytes,
            options,
            morphed_meshes: 0,
        })
    }

    /// Stops when the cancel flag is raised.
    fn check(&self) -> Result<()> {
        match &self.options.cancel {
            Some(flag) if flag.load(Ordering::Relaxed) => Err(GltfError::Cancelled),
            _ => Ok(()),
        }
    }

    /// Spends `bytes` of the decoding budget.
    fn spend(&mut self, bytes: u64, at: &str) -> Result<()> {
        self.budget = self.budget.checked_sub(bytes).ok_or_else(|| {
            GltfError::Limit(format!(
                "{at}: decoding exceeds the {} bytes allowed",
                self.options.max_decoded_bytes
            ))
        })?;
        Ok(())
    }

    /// The bytes of buffer view `view`.
    fn view(&self, view: usize) -> &[u8] {
        let view = &self.views[view];
        &self.buffers[view.buffer][view.offset..view.offset + view.length]
    }

    fn image(&self, image: &Obj<'_, '_>, files: &mut Files<'_>) -> Result<Image> {
        let declared = image.opt_str("mimeType")?;
        let (data, sniffed) = match (
            image.index("bufferView", self.views.len())?,
            image.opt_str("uri")?,
        ) {
            (Some(view), None) => {
                if declared.is_none() {
                    return Err(image.error("mimeType", "missing for an image in a buffer view"));
                }
                (ImageData::Embedded(self.view(view).to_vec()), None)
            }
            (None, Some(text)) => match uri::sort(text) {
                Some(Uri::Data { mime, base64 }) => {
                    let bytes =
                        uri::data(base64).ok_or_else(|| image.error("uri", "bad base64"))?;
                    (
                        ImageData::Embedded(bytes),
                        (!mime.is_empty()).then_some(mime),
                    )
                }
                Some(Uri::Path(path)) if self.options.embed_images => (
                    ImageData::Embedded(load(image, text, files)?),
                    uri::by_extension(&path),
                ),
                Some(Uri::Path(path)) => {
                    let by_extension = uri::by_extension(&path);
                    (ImageData::Path(path), by_extension)
                }
                None => {
                    return Err(
                        image.error("uri", format!("{text:?} names no file beside the glTF"))
                    );
                }
            },
            _ => return Err(image.error("uri", "an image needs one of uri and bufferView")),
        };
        let mime = declared.or(sniffed).or(match &data {
            ImageData::Embedded(bytes) => uri::sniff(bytes),
            ImageData::Path(_) => None,
        });
        Ok(Image {
            name: image.text("name")?,
            mime_type: mime.map(str::to_owned),
            data,
        })
    }
}

/// The `asset` object, whose `version` must be 2.x and `minVersion` at
/// most 2.0.
fn asset(root: &Obj<'_, '_>) -> Result<Asset> {
    let asset = root
        .object("asset")?
        .ok_or_else(|| root.error("asset", "missing"))?;
    let version = asset
        .opt_str("version")?
        .ok_or_else(|| asset.error("version", "missing"))?;
    if !version.starts_with("2.") {
        return Err(asset.error("version", format!("{version}, where 2.x is read")));
    }
    if let Some(min) = asset.opt_str("minVersion")?.filter(|&min| min != "2.0") {
        return Err(asset.error("minVersion", format!("{min}, where 2.0 is read")));
    }
    Ok(Asset {
        generator: asset.text("generator")?,
        copyright: asset.text("copyright")?,
    })
}

/// Refuses a required extension not read; names the used ones not read.
fn extensions(root: &Obj<'_, '_>) -> Result<Vec<LeftOut>> {
    let names = |key: &str| -> Result<Vec<String>> {
        let Some(Value::Array(items)) = root.get(key) else {
            return match root.get(key) {
                None => Ok(Vec::new()),
                Some(_) => Err(root.error(key, "expected an array of names")),
            };
        };
        items
            .iter()
            .map(|item| match item {
                Value::String(name) => Ok(name.to_string()),
                _ => Err(root.error(key, "expected an array of names")),
            })
            .collect()
    };
    if let Some(required) = names("extensionsRequired")?
        .into_iter()
        .find(|n| !EXTENSIONS.contains(&n.as_str()))
    {
        return Err(GltfError::Unsupported(required));
    }
    let used = names("extensionsUsed")?
        .into_iter()
        .filter(|n| !EXTENSIONS.contains(&n.as_str()));
    Ok(used.map(LeftOut::Extension).collect())
}

/// The bytes a `uri` names.
fn load(at: &Obj<'_, '_>, text: &str, files: &mut Files<'_>) -> Result<Vec<u8>> {
    match uri::sort(text) {
        Some(Uri::Data { base64, .. }) => {
            uri::data(base64).ok_or_else(|| at.error("uri", "bad base64"))
        }
        Some(Uri::Path(path)) => files(&path).map_err(|error| GltfError::Io { file: path, error }),
        None => Err(at.error("uri", format!("{text:?} names no file beside the glTF"))),
    }
}

/// How many objects in `value` (itself included) carry `extras`.
fn extras(value: &Value<'_>) -> usize {
    match value {
        Value::Array(items) => items.iter().map(extras).sum(),
        Value::Object(members) => members
            .iter()
            .map(|(k, v)| if k == "extras" { 1 } else { extras(v) })
            .sum(),
        _ => 0,
    }
}
