//! The Khronos sample assets committed under `tests/fixtures/khronos`.
use kinogaki_gltf::gltf::{self, Gltf, GltfError, ImageData};
use kinogaki_studio_api::format::ReadBudget;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Every committed sample file (`.gltf` and `.glb`), sorted.
pub fn samples() -> Vec<PathBuf> {
    let models = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/khronos/Models");
    let mut files = Vec::new();
    for model in std::fs::read_dir(models).unwrap() {
        for variant in std::fs::read_dir(model.unwrap().path()).unwrap() {
            let variant = variant.unwrap().path();
            if !variant.is_dir() {
                continue;
            }
            for file in std::fs::read_dir(variant).unwrap() {
                let file = file.unwrap().path();
                if file.extension().is_some_and(|e| e == "gltf" || e == "glb") {
                    files.push(file);
                }
            }
        }
    }
    files.sort();
    files
}

/// The sample file `<model>/<variant>/<model>.<extension>`.
pub fn sample(model: &str, variant: &str) -> PathBuf {
    let extension = if variant == "glTF-Binary" {
        "glb"
    } else {
        "gltf"
    };
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "tests/fixtures/khronos/Models/{model}/{variant}/{model}.{extension}"
    ))
}

/// The sample read with images embedded.
pub fn read(model: &str, variant: &str) -> Gltf {
    read_path(&sample(model, variant))
}

/// The file at `path` read with images embedded.
pub fn read_path(path: &Path) -> Gltf {
    read_file(path, true).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The model of the file at `path`, a trusted fixture: the files it names
/// read from beside it, and its images read in when `embed`.
pub fn read_file(path: &Path, embed: bool) -> Result<Gltf, GltfError> {
    let bytes = std::fs::read(path).unwrap();
    let folder = path.parent().unwrap();
    let mut files =
        |name: &str| std::fs::read(folder.join(name)).map_err(|e| format!("{name}: {e}"));
    let mut model = gltf::read(&bytes, &mut files, &ReadBudget::default())?;
    for image in model.images.iter_mut().filter(|_| embed) {
        if let ImageData::Path(name) = &image.data
            && let Ok(bytes) = std::fs::read(folder.join(name))
        {
            image.data = ImageData::Embedded(bytes.into());
        }
    }
    Ok(model)
}

/// Bytes held in memory, with no files beside them.
pub fn read_bytes(bytes: &[u8]) -> Result<Gltf, gltf::GltfError> {
    let mut files = |name: &str| Err(format!("{name}: held in memory"));
    gltf::read(bytes, &mut files, &ReadBudget::default())
}

/// `bytes` in base64, for `data:` URIs.
pub fn base64(bytes: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let bits = chunk
            .iter()
            .enumerate()
            .fold(0u32, |bits, (i, &b)| bits | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            text.push(if i <= chunk.len() {
                alphabet[(bits >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    text
}

/// A glTF of one primitive over one buffer of `bytes` (a `data:` URI),
/// with `views`, `accessors` and the primitive's `attributes` and
/// `indices` members as JSON text.
pub fn primitive(bytes: &[u8], views: &str, accessors: &str, primitive: &str) -> String {
    format!(
        r#"{{"asset":{{"version":"2.0"}},"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],
        "meshes":[{{"primitives":[{{{primitive}}}]}}],"accessors":[{accessors}],"bufferViews":[{views}],
        "buffers":[{{"byteLength":{},"uri":"data:application/octet-stream;base64,{}"}}]}}"#,
        bytes.len(),
        base64(bytes)
    )
}

/// `model` written as a GLB and as a `.gltf` with its files, each read back.
pub fn written_and_read(model: &Gltf) -> [Gltf; 2] {
    let glb = read_bytes(&gltf::to_glb(model).unwrap()).unwrap();
    let files = gltf::to_gltf(model, "scene").unwrap();
    let beside: HashMap<String, Vec<u8>> = files.bin.iter().chain(&files.images).cloned().collect();
    let mut open = |name: &str| beside.get(name).cloned().ok_or(format!("{name}: no file"));
    let mut text = gltf::read(files.json.as_bytes(), &mut open, &ReadBudget::default()).unwrap();
    // The images written beside the .gltf were embedded in the model.
    for image in &mut text.images {
        if let ImageData::Path(path) = &image.data
            && let Some(bytes) = beside.get(path)
        {
            image.data = ImageData::Embedded(bytes.as_slice().into());
        }
    }
    [glb, text]
}
