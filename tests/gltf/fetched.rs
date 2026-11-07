//! The Khronos samples whose licences keep them out of this repository,
//! fetched by `tools/fetch_samples.sh`: they read and write back the same.
use crate::common;

use kinogaki_gltf::gltf::{self, ReadOptions};
use std::path::Path;

#[test]
#[ignore = "hand: needs tools/fetch_samples.sh"]
fn hand_run_fetched_samples_read_and_write_back_the_same() {
    let fetched = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/khronos-fetched");
    for file in [
        "DamagedHelmet/glTF-Binary/DamagedHelmet.glb",
        "DamagedHelmet/glTF/DamagedHelmet.gltf",
        "Sponza/glTF/Sponza.gltf",
    ] {
        let options = ReadOptions {
            embed_images: true,
            ..ReadOptions::default()
        };
        let model = gltf::read_file(&fetched.join(file), &options)
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        assert!(
            model.left_out.is_empty() && !model.images.is_empty(),
            "{file}"
        );
        for back in common::written_and_read(&model) {
            assert_eq!(back, model, "{file}");
        }
    }
}
