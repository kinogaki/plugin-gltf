//! glTF 2.0 and GLB, a format plugin for Kinogaki Studio.
//!
//! The [`gltf`] module is the file's own model, its reader and its writer;
//! it uses nothing but the standard library. The format's conversion to
//! Studio's standard values (geometry, MaterialX glTF PBR materials, Image
//! nodes, lights and cameras) builds on it.
pub mod gltf;
