//! Reads glTF files and writes each back as a GLB and as a `.gltf` with its
//! files, for the Khronos glTF-Validator (`tools/validate.mjs`) to check:
//!
//! ```sh
//! cargo run --release --example rewrite -- <out dir> tests/fixtures/khronos/Models/*/*/*.gl*
//! ```
use kinogaki_gltf::gltf::{self, Gltf, ImageData};
use kinogaki_studio_api::format::ReadBudget;
use std::path::Path;

/// The file at `path` with the files it names beside it, its images read
/// in.
fn read(path: &Path) -> Result<Gltf, gltf::GltfError> {
    let folder = path.parent().unwrap_or(Path::new(""));
    let mut files =
        |name: &str| std::fs::read(folder.join(name)).map_err(|e| format!("{name}: {e}"));
    let bytes = std::fs::read(path).map_err(|error| gltf::GltfError::Io {
        file: path.display().to_string(),
        error,
    })?;
    let mut model = gltf::read(&bytes, &mut files, &ReadBudget::default())?;
    for image in &mut model.images {
        if let ImageData::Path(name) = &image.data
            && let Ok(bytes) = std::fs::read(folder.join(name))
        {
            image.data = ImageData::Embedded(bytes.into());
        }
    }
    Ok(model)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("an output directory, then glTF files");
    for file in args {
        let path = Path::new(&file);
        let model = read(path).unwrap_or_else(|e| panic!("{file}: {e}"));
        let stem = path.file_stem().unwrap().to_string_lossy();
        let variant = path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy();
        let dir = Path::new(&out).join(format!("{stem}.{variant}"));
        std::fs::create_dir_all(&dir).unwrap();
        for name in [format!("{stem}.glb"), format!("{stem}.gltf")] {
            gltf::write_file(&model, &dir.join(&name))
                .unwrap_or_else(|e| panic!("{file} as {name}: {e}"));
        }
    }
}
