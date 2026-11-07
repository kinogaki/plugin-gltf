//! glTF and GLB as a Studio plugin library (`formats/gltf.dylib`).
use kinogaki_studio_api::export_package;

kinogaki_shared::link!();
export_package!(kinogaki_gltf::package);
