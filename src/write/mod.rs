//! Studio's standard values to a glTF file: the inverse of the read.
//!
//! Each item is a node, placed in glTF's space ([`Space`]) and holding its
//! mesh (`mesh`; a B-rep through the installed mesher), its instances (a
//! child node an instance), its light or its camera (`scene`); the
//! materials the polygons name are glTF PBR (`material`). A `.glb` holds
//! its images; a `.gltf` names them through `format::files::Beside`, a file
//! on disk by its path relative to the `.gltf` and an embedded one written
//! into `<stem>_textures/` beside it, so no file there is overwritten. What
//! glTF can't hold is said.
mod material;
mod mesh;
mod scene;

use crate::gltf::{self, Asset, Gltf};
use crate::read::LINEAR_SRGB;
use crate::space::Space;
use crate::{TYPES, at_file};
use kinogaki_studio_api::color::settled;
use kinogaki_studio_api::format::files::Beside;
use kinogaki_studio_api::standard::{self, Materials};
use kinogaki_studio_api::{
    DataTypeId, Diagnostic, Error, FileItem, FormatOptions, FormatWriter, Writing,
};
use std::path::Path;

/// What writes glTF files.
pub(crate) struct Writer;

impl FormatWriter for Writer {
    fn accepts(&self) -> &[DataTypeId] {
        &*TYPES
    }

    /// `items` as a GLB for `.glb`, else `.gltf` text with its buffer and
    /// images beside it.
    fn write(
        &self,
        path: &Path,
        items: &[FileItem],
        writing: &Writing<'_>,
        _: &FormatOptions,
    ) -> Result<Vec<Diagnostic>, Error> {
        writing.check()?;
        let held = (items.iter())
            .filter_map(|i| i.data.as_ref())
            .find_map(|d| standard::materials().get(d));
        let gltf_text = !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("glb"));
        let mut out = Out {
            beside: gltf_text.then(|| Beside::new(path)),
            model: Gltf {
                asset: Asset {
                    generator: "Kinogaki Studio".to_owned(),
                    copyright: String::new(),
                },
                ..Gltf::default()
            },
            writing,
            space: Space::of(writing.units),
            held,
            looks: material::Looks::default(),
            warnings: Vec::new(),
        };
        // Every material the item holds, in its order: Export hands the
        // ones the items name.
        for m in held.map_or(&[][..], |h| &h.materials) {
            out.material(&m.key)?;
        }
        scene::nodes(items, &mut out)?;
        writing.check()?;
        gltf::write_file(&out.model, path).map_err(|e| at_file(path.display(), e))?;
        Ok(out.warnings)
    }
}

/// A write as it goes: the model made so far and what made it.
pub(crate) struct Out<'a> {
    model: Gltf,
    /// Where a `.gltf`'s images go: a file on disk named by its path
    /// relative to the `.gltf`, an embedded one written beside it, never
    /// over another file; none for a GLB, which holds its images.
    beside: Option<Beside>,
    writing: &'a Writing<'a>,
    space: Space,
    /// The materials item: what the polygons' keys name.
    held: Option<&'a Materials>,
    looks: material::Looks,
    warnings: Vec<Diagnostic>,
}

impl Out<'_> {
    /// Says `message`.
    fn warn(&mut self, message: String) {
        self.warnings.push(Diagnostic::warning(message));
    }

    /// `rgb`, in the working space, in glTF's linear Rec.709. Converted, it
    /// keeps twelve significant digits (eleven places below 1): a color read
    /// and written back is the number it was, not one a conversion there and
    /// back moved in its last bits (glTF's readers hold it in single
    /// precision).
    fn color(&self, rgb: [f64; 3]) -> Result<[f64; 3], Error> {
        let colors = self.writing.colors;
        if colors.working() == LINEAR_SRGB {
            return Ok(rgb);
        }
        let mut c = [rgb];
        colors.working_to(LINEAR_SRGB, &mut c)?;
        Ok(c[0].map(settled))
    }
}
