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
use kinogaki_studio_api::format::ReadBudget;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

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

/// Reads a glTF (JSON) or GLB file held in `bytes` within `budget`:
/// what the file claims (its JSON's depth, its decoded attributes and
/// indices) is checked against it, and a cancelled budget stops the read
/// with [`GltfError::Cancelled`]. `files` gives the bytes of a file the
/// glTF names by a path (percent-decoded), or why it can't.
pub fn read(bytes: &[u8], files: &mut Files<'_>, budget: &ReadBudget) -> Result<Gltf> {
    let (json, bin) = if glb::is_glb(bytes) {
        glb::split(bytes)?
    } else {
        (bytes, None)
    };
    let json = json.strip_prefix(b"\xef\xbb\xbf").unwrap_or(json);
    let text = std::str::from_utf8(json).map_err(|e| {
        GltfError::malformed("JSON", format!("not UTF-8 at byte {}", e.valid_up_to()))
    })?;
    let depth = budget.limits().depth;
    let value =
        json::parse(text, depth).map_err(|e| GltfError::malformed("JSON", e.to_string()))?;
    let root = Obj::new(&value, String::new())?;
    let asset = asset(&root)?;
    let mut left_out = extensions(&root)?;
    let mut reader = Reader::new(&root, bin, files, budget)?;
    let images = root.each("images", |image| reader.image(image))?;
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

/// A source of the files a glTF names: a path's bytes, or why it has none.
pub type Files<'f> = dyn FnMut(&str) -> std::result::Result<Vec<u8>, String> + 'f;

/// A read's binary data: buffers, views and accessors, decoded on use
/// within the budget.
pub(super) struct Reader<'a> {
    buffers: Vec<Cow<'a, [u8]>>,
    views: Vec<View>,
    accessors: Vec<Accessor>,
    /// Decoded attributes by accessor, shared by the primitives using them.
    decoded: Vec<Option<Arc<[f32]>>>,
    /// Decoded indices by accessor, and `0..count` lists by count: each
    /// with its largest index, shared by the primitives using them.
    indices: HashMap<Indices, (Arc<[u32]>, Option<u32>)>,
    /// Each buffer view's bytes as the images in it hold them, copied once.
    pictures: HashMap<usize, Arc<[u8]>>,
    budget: ReadBudget,
    /// Meshes whose morph targets were applied.
    morphed_meshes: usize,
}

/// Which index list: an accessor's, or `0..count`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Indices {
    Accessor(usize),
    Sequential(usize),
}

impl<'a> Reader<'a> {
    fn new(
        root: &Obj<'_, '_>,
        bin: Option<&'a [u8]>,
        files: &mut Files<'_>,
        budget: &ReadBudget,
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
            indices: HashMap::new(),
            pictures: HashMap::new(),
            buffers,
            views,
            accessors,
            budget: budget.clone(),
            morphed_meshes: 0,
        })
    }

    /// Stops when the read is cancelled.
    fn check(&self) -> Result<()> {
        self.budget.check().map_err(GltfError::from)
    }

    /// Charges an allocation of `bytes` the file sized (`None`: past any
    /// size) to the budget.
    fn spend(&self, bytes: Option<usize>, at: &str) -> Result<()> {
        let bytes = bytes.map_or(u64::MAX, |b| b as u64);
        self.budget
            .allocate(bytes)
            .map_err(|e| match e.is_cancelled() {
                true => GltfError::Cancelled,
                false => GltfError::Limit(format!("{at}: {}", e.message())),
            })
    }

    /// The bytes of buffer view `view`.
    fn view(&self, view: usize) -> &[u8] {
        let view = &self.views[view];
        &self.buffers[view.buffer][view.offset..view.offset + view.length]
    }

    fn image(&mut self, image: &Obj<'_, '_>) -> Result<Image> {
        let declared = image.opt_str("mimeType")?;
        let (data, sniffed) = match (
            image.index("bufferView", self.views.len())?,
            image.opt_str("uri")?,
        ) {
            (Some(view), None) => {
                if declared.is_none() {
                    return Err(image.error("mimeType", "missing for an image in a buffer view"));
                }
                let bytes = match self.pictures.get(&view) {
                    Some(bytes) => bytes.clone(),
                    None => {
                        let bytes: Arc<[u8]> = self.view(view).into();
                        self.pictures.insert(view, bytes.clone());
                        bytes
                    }
                };
                (ImageData::Embedded(bytes), None)
            }
            (None, Some(text)) => match uri::sort(text) {
                Some(Uri::Data { mime, base64 }) => {
                    let bytes =
                        uri::data(base64).ok_or_else(|| image.error("uri", "bad base64"))?;
                    (
                        ImageData::Embedded(bytes.into()),
                        (!mime.is_empty()).then_some(mime),
                    )
                }
                Some(Uri::Path(path)) => {
                    let by_extension = uri::by_extension(&path);
                    (ImageData::Path(path), by_extension)
                }
                Some(Uri::Other) | None => (ImageData::Elsewhere(text.to_owned()), None),
            },
            _ => return Err(image.error("uri", "an image needs one of uri and bufferView")),
        };
        let mime = declared.or(sniffed).or(match &data {
            ImageData::Embedded(bytes) => uri::sniff(bytes),
            ImageData::Path(_) | ImageData::Elsewhere(_) => None,
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

/// The bytes a buffer's `uri` names.
fn load(at: &Obj<'_, '_>, text: &str, files: &mut Files<'_>) -> Result<Vec<u8>> {
    match uri::sort(text) {
        Some(Uri::Data { base64, .. }) => {
            uri::data(base64).ok_or_else(|| at.error("uri", "bad base64"))
        }
        Some(Uri::Path(path)) => files(&path).map_err(|why| at.error("uri", why)),
        Some(Uri::Other) | None => Err(at.error("uri", format!("{text:?} names no file"))),
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
