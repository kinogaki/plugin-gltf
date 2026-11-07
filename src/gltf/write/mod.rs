//! A [`Gltf`] to a GLB, or to `.gltf` text with its `.bin` and images.
//!
//! Every attribute and index list becomes a tightly packed buffer view of
//! one buffer: attributes as floats (values shared between primitives
//! written once), indices as 16-bit or 32-bit integers. Embedded images go
//! in the GLB's buffer, or beside the `.gltf` as files; images named by path
//! stay paths. Every index is checked first, so a written file is valid.
mod binary;
mod material;
mod scene;

use super::json::{Members, Value};
use super::model::{Gltf, ImageData};
use super::{GltfError, glb, uri};
use binary::Binary;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

type Result<T> = std::result::Result<T, GltfError>;

/// A `.gltf` and the files it names beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct GltfFiles {
    /// The `.gltf` text.
    pub json: String,
    /// The binary buffer's file: its name and bytes (none when the model
    /// has no binary data).
    pub bin: Option<(String, Vec<u8>)>,
    /// The embedded images' files: names and bytes.
    pub images: Vec<(String, Vec<u8>)>,
}

/// The model as a GLB.
pub fn to_glb(gltf: &Gltf) -> Result<Vec<u8>> {
    let document = document(gltf, None)?;
    Ok(glb::join(&document.json, &document.bin))
}

/// The model as `.gltf` text naming files beside it: `<stem>.bin` and the
/// embedded images, named after themselves.
pub fn to_gltf(gltf: &Gltf, stem: &str) -> Result<GltfFiles> {
    let Document {
        json,
        bin: binary,
        images,
    } = document(gltf, Some(stem))?;
    let bin = (!binary.is_empty()).then(|| (format!("{stem}.bin"), binary));
    Ok(GltfFiles { json, bin, images })
}

/// Writes the model at `path`: a GLB for `.glb`, else `.gltf` text with
/// its files beside it.
pub fn write_file(gltf: &Gltf, path: &Path) -> Result<()> {
    let write = |path: &Path, bytes: &[u8]| {
        fs::write(path, bytes).map_err(|error| GltfError::Io {
            file: path.display().to_string(),
            error,
        })
    };
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("glb"))
    {
        return write(path, &to_glb(gltf)?);
    }
    let stem = path
        .file_stem()
        .map_or("scene".into(), |s| s.to_string_lossy());
    let files = to_gltf(gltf, &stem)?;
    let beside = path.parent().unwrap_or(Path::new(""));
    for (name, bytes) in files.bin.iter().chain(&files.images) {
        write(&beside.join(name), bytes)?;
    }
    write(path, files.json.as_bytes())
}

/// The JSON text, the binary buffer and the image files beside: images go
/// in the buffer for a GLB (`stem` none), else beside, named from `stem`.
fn document(gltf: &Gltf, stem: Option<&str>) -> Result<Document> {
    let mut binary = Binary::default();
    let mut beside = Vec::new();
    // The buffer's file is taken before any image's.
    let mut names: BTreeSet<String> = stem.map(|stem| format!("{stem}.bin")).into_iter().collect();
    let mut images = Vec::with_capacity(gltf.images.len());
    let mut mimes = Vec::with_capacity(gltf.images.len());
    for (i, image) in gltf.images.iter().enumerate() {
        let mime = image.mime_type.as_deref().or(match &image.data {
            ImageData::Embedded(bytes) => uri::sniff(bytes),
            ImageData::Path(path) | ImageData::Elsewhere(path) => uri::by_extension(path),
        });
        mimes.push(mime);
        let members = Members::new().with_text("name", &image.name);
        images.push(match (&image.data, stem) {
            (ImageData::Embedded(bytes), None) => {
                let mime = mime.ok_or_else(|| {
                    GltfError::invalid(format!("images[{i}]"), "an image of unknown type")
                })?;
                members
                    .with("bufferView", binary.view(bytes, None))
                    .with("mimeType", mime)
            }
            (ImageData::Embedded(bytes), Some(stem)) => {
                let name = file_name(
                    stem,
                    &image.name,
                    i,
                    mime.map_or("bin", uri::extension),
                    &mut names,
                );
                beside.push((name.clone(), bytes.to_vec()));
                members
                    .with("uri", uri::percent_encoded(&name))
                    .with_some("mimeType", mime)
            }
            (ImageData::Path(path), _) => members
                .with("uri", uri::percent_encoded(path))
                .with_some("mimeType", mime),
            (ImageData::Elsewhere(text), _) => {
                members.with_text("uri", text).with_some("mimeType", mime)
            }
        });
    }
    let mut required = BTreeSet::new();
    let textures = each("textures", &gltf.textures, |texture, at| {
        material::texture(texture, at, gltf.samplers.len(), &mimes, &mut required)
    })?;
    let materials = each("materials", &gltf.materials, |m, at| {
        material::material(m, at, gltf.textures.len())
    })?;
    let meshes = each("meshes", &gltf.meshes, |mesh, at| {
        scene::mesh(mesh, at, gltf.materials.len(), &mut binary)
    })?;
    let nodes = each("nodes", &gltf.nodes, |node, at| scene::node(node, at, gltf))?;
    let scenes = each("scenes", &gltf.scenes, |scene, at| {
        scene::scene(scene, at, gltf.nodes.len())
    })?;
    if let Some(scene) = gltf.scene {
        check("scene", scene, gltf.scenes.len())?;
    }
    let lights = gltf.lights.iter().map(scene::light).collect::<Vec<_>>();
    let lights = (!lights.is_empty())
        .then(|| Members::new().with("KHR_lights_punctual", Members::new().with("lights", lights)));
    let bin_uri = stem.map(|stem| uri::percent_encoded(&format!("{stem}.bin")));
    let buffers = vec![
        Members::new()
            .with("byteLength", binary.bytes.len())
            .with_some("uri", bin_uri),
    ];
    let some = |items: Vec<Value<'static>>| (!items.is_empty()).then_some(items);
    let content = Value::from(
        Members::new()
            .with_some("scene", gltf.scene)
            .with_some("scenes", some(scenes))
            .with_some("nodes", some(nodes))
            .with_some("meshes", some(meshes))
            .with_some("materials", some(materials))
            .with_some("textures", some(textures))
            .with_some(
                "images",
                some(images.into_iter().map(Value::from).collect()),
            )
            .with_some(
                "samplers",
                some(gltf.samplers.iter().map(material::sampler).collect()),
            )
            .with_some(
                "cameras",
                some(gltf.cameras.iter().map(scene::camera).collect()),
            )
            .with_some("accessors", some(binary.accessors))
            .with_some("bufferViews", some(binary.views))
            .with_if(!binary.bytes.is_empty(), "buffers", buffers)
            .with_some("extensions", lights),
    );
    let used: Vec<String> = used_extensions(&content).into_iter().collect();
    let required: Vec<&str> = required.into_iter().collect();
    let asset = Members::new()
        .with("version", "2.0")
        .with_text("generator", &gltf.asset.generator)
        .with_text("copyright", &gltf.asset.copyright);
    let root = Members::new()
        .with("asset", asset)
        .with_if(!used.is_empty(), "extensionsUsed", used)
        .with_if(!required.is_empty(), "extensionsRequired", required)
        .append(content);
    if let Some(at) = not_finite(&root) {
        return Err(GltfError::invalid(at, "a number that isn't finite"));
    }
    let mut json = String::new();
    Value::from(root).write(&mut json);
    Ok(Document {
        json,
        bin: binary.bytes,
        images: beside,
    })
}

/// A written document: its JSON, its binary buffer and the image files
/// beside it.
struct Document {
    json: String,
    bin: Vec<u8>,
    images: Vec<(String, Vec<u8>)>,
}

/// `write` over each of `items`, given its path (`"meshes[2]"`).
fn each<T>(
    what: &str,
    items: &[T],
    mut write: impl FnMut(&T, &str) -> Result<Value<'static>>,
) -> Result<Vec<Value<'static>>> {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| write(item, &format!("{what}[{i}]")))
        .collect()
}

/// An index into a list of `len`, or the error naming where it is.
pub(super) fn check(at: &str, index: usize, len: usize) -> Result<()> {
    match index < len {
        true => Ok(()),
        false => Err(GltfError::invalid(
            at,
            format!("{index} is past the {len} there are"),
        )),
    }
}

/// The first top-level member holding a number JSON can't (infinite or NaN).
fn not_finite(root: &Members) -> Option<String> {
    fn finite(value: &Value<'_>) -> bool {
        match value {
            Value::Number(n) => n.is_finite(),
            Value::Array(items) => items.iter().all(finite),
            Value::Object(members) => members.iter().all(|(_, v)| finite(v)),
            _ => true,
        }
    }
    root.0
        .iter()
        .find(|(_, v)| !finite(v))
        .map(|(k, _)| k.to_string())
}

/// Every extension named in an `extensions` object, sorted.
fn used_extensions(value: &Value<'_>) -> BTreeSet<String> {
    let mut used = BTreeSet::new();
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        match value {
            Value::Array(items) => stack.extend(items),
            Value::Object(members) => {
                for (key, value) in members {
                    if let (true, Value::Object(extensions)) = (key == "extensions", value) {
                        used.extend(extensions.iter().map(|(name, _)| name.to_string()));
                    }
                    stack.push(value);
                }
            }
            _ => {}
        }
    }
    used
}

/// An image's file name beside the `.gltf`: its own name made safe, else
/// the stem and its index, numbered on until unique among `taken` (kept
/// lowercase, for file systems that ignore case).
fn file_name(
    stem: &str,
    name: &str,
    index: usize,
    extension: &str,
    taken: &mut BTreeSet<String>,
) -> String {
    let name = name.strip_suffix(&format!(".{extension}")).unwrap_or(name);
    let safe: String = (name.chars())
        .map(|c| match c.is_alphanumeric() || "-_ .".contains(c) {
            true => c,
            false => '_',
        })
        .collect();
    let base = match safe.trim_matches(['.', ' ']) {
        "" => format!("{stem}_{index}"),
        safe => safe.to_owned(),
    };
    let mut file = format!("{base}.{extension}");
    for n in 1.. {
        if taken.insert(file.to_lowercase()) {
            break;
        }
        file = format!("{base}_{n}.{extension}");
    }
    file
}

#[cfg(test)]
mod tests {
    use super::file_name;
    use std::collections::BTreeSet;

    #[test]
    fn image_files_are_named_after_their_images_and_never_twice() {
        let mut taken = BTreeSet::from(["scene.bin".to_owned()]);
        let mut name = |image: &str, index: usize, extension: &str| {
            file_name("scene", image, index, extension, &mut taken)
        };
        assert_eq!(name("wood albedo.png", 0, "png"), "wood albedo.png");
        assert_eq!(name("Wood Albedo", 1, "png"), "Wood Albedo_1.png");
        assert_eq!(name("", 2, "jpg"), "scene_2.jpg");
        assert_eq!(name("../a/b:c", 3, "png"), "_a_b_c.png");
        assert_eq!(name("scene", 4, "bin"), "scene_1.bin");
        assert_eq!(name("...", 5, "png"), "scene_5.png");
    }
}
