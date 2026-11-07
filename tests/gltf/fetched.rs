//! The Khronos samples whose licences keep them out of this repository,
//! from the folder `GLTF_SAMPLES` names (Studio's `tools/fetch_data.py`
//! fetches them): they read and write back the same.
use crate::common;

use std::path::PathBuf;

#[test]
#[ignore = "hand: against the fetched samples in GLTF_SAMPLES"]
fn hand_run_fetched_samples_read_and_write_back_the_same() {
    let fetched = PathBuf::from(
        std::env::var_os("GLTF_SAMPLES")
            .expect("GLTF_SAMPLES: the fetched samples (Studio's tools/fetch_data.py)"),
    );
    for file in [
        "DamagedHelmet/glTF-Binary/DamagedHelmet.glb",
        "DamagedHelmet/glTF/DamagedHelmet.gltf",
        "Sponza/glTF/Sponza.gltf",
    ] {
        let model = common::read_path(&fetched.join(file));
        assert!(
            model.left_out.is_empty() && !model.images.is_empty(),
            "{file}"
        );
        for back in common::written_and_read(&model) {
            assert_eq!(back, model, "{file}");
        }
    }
}
