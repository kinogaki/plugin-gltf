//! Reads glTF files and writes each back as a GLB and as a `.gltf` with its
//! files, for the Khronos glTF-Validator (`tools/validate.mjs`) to check:
//!
//! ```sh
//! cargo run --release --example rewrite -- <out dir> tests/fixtures/khronos/Models/*/*/*.gl*
//! ```
use kinogaki_gltf::gltf::{self, ReadOptions};
use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("an output directory, then glTF files");
    let options = ReadOptions {
        embed_images: true,
        ..ReadOptions::default()
    };
    for file in args {
        let path = Path::new(&file);
        let model = gltf::read_file(path, &options).unwrap_or_else(|e| panic!("{file}: {e}"));
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
