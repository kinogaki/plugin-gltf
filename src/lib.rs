//! glTF 2.0 and GLB, a format plugin for Kinogaki Studio.
//!
//! The [`gltf`] module is the file's own model, its reader and its writer;
//! it uses nothing but the standard library. The format converts between
//! that model and Studio's standard values at its boundary (`read` and
//! `write`, `space` for where things are): a file's materials as MaterialX
//! glTF PBR networks with an Image node for each image they read, its
//! meshes as polygons (shared ones as instances), its node tree as Parts,
//! and its punctual lights and cameras.
pub mod gltf;
mod read;
mod space;
mod write;

use kinogaki_studio_api::format::options;
use kinogaki_studio_api::params::Schema;
use kinogaki_studio_api::{
    DataTypeId, Error, FileFormat, FormatId, FormatOptions, FormatWriter, Imported, Input, Package,
    PackageId, Purpose, Reading, Value, standard,
};
use std::sync::LazyLock;

/// The package's id.
pub fn id() -> PackageId {
    PackageId::declared("kinogaki.gltf")
}

/// The format's id.
pub fn format() -> FormatId {
    FormatId::declared("kinogaki.format.gltf")
}

/// The package: the format alone.
pub fn package() -> Package {
    Package::new(id(), "glTF").with_format(Format)
}

/// What a read makes and a write takes: materials, polygons, instances of
/// shared meshes, lights and cameras.
static TYPES: LazyLock<[DataTypeId; 5]> = LazyLock::new(|| {
    [
        standard::materials().id(),
        standard::geometry().id(),
        standard::instances().id(),
        standard::light().id(),
        standard::CAMERA,
    ]
});

/// The format: `.gltf` with the files beside it, and `.glb`.
pub struct Format;

impl FileFormat for Format {
    fn id(&self) -> FormatId {
        format()
    }
    fn label(&self) -> &str {
        "glTF"
    }
    fn extensions(&self) -> &[&str] {
        &["gltf", "glb"]
    }
    /// A GLB by its magic.
    fn detects(&self, head: &[u8]) -> bool {
        head.starts_with(b"glTF")
    }
    fn produces(&self) -> &[DataTypeId] {
        &*TYPES
    }
    /// Embed Textures, off: images a `.gltf` names beside it stay paths.
    fn options(&self) -> FormatOptions {
        FormatOptions::from([(options::EMBED.to_owned(), Value::Bool(false))])
    }
    fn schema(&self, chosen: &FormatOptions, _: Purpose) -> Schema {
        let embed = options::option(chosen, options::EMBED, Value::bool).unwrap_or(false);
        options::embed(Schema::default(), embed)
    }
    fn read(
        &self,
        input: &Input<'_>,
        options: &FormatOptions,
        reading: &Reading<'_>,
    ) -> Result<Imported, Error> {
        read::read(input, options, reading)
    }
    fn writer(&self) -> Option<&dyn FormatWriter> {
        Some(&write::Writer)
    }
}

/// The model's error as Studio's, naming `file` (a path, or an input).
fn at_file(file: impl std::fmt::Display, error: gltf::GltfError) -> Error {
    match error {
        gltf::GltfError::Cancelled => Error::cancelled(),
        error => Error::invalid(format!("{file}: {error}")),
    }
}
