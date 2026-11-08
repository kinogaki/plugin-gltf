//! Studio's standard values to a glTF file: the inverse of the read.
//!
//! Each item is a node, placed in glTF's space (the read's `Conversion`
//! inverted) and holding its
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
use crate::space;
use crate::{TYPES, at_file};
use kinogaki_studio_api::color::{LIN_REC709, settled};
use kinogaki_studio_api::format::Conversion;
use kinogaki_studio_api::format::files::Beside;
use kinogaki_studio_api::standard::{self, ImportedMaterial};
use kinogaki_studio_api::{
    DataTypeId, Diagnostic, Error, FileItem, FormatOptions, FormatWriter, Writing,
};
use std::path::Path;

/// What writes glTF files.
pub(crate) struct Writer;

impl FormatWriter for Writer {
    fn accepts(&self) -> &[DataTypeId] {
        TYPES
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
        let held: Vec<&ImportedMaterial> = (items.iter())
            .filter_map(|i| standard::materials().get(i.data.as_ref()?))
            .flat_map(|m| &m.materials)
            .collect();
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
            to_file: space::into_project(writing.units).inverse(),
            held,
            looks: material::Looks::default(),
            warnings: Vec::new(),
        };
        // Every material of every library, in their order: unused ones too.
        for m in out.held.clone() {
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
    /// The project's space into glTF's, the read's inverse.
    to_file: Conversion,
    /// Every library's materials: what the polygons' keys name.
    held: Vec<&'a ImportedMaterial>,
    looks: material::Looks,
    warnings: Vec<Diagnostic>,
}

impl Out<'_> {
    /// Says `message`.
    fn warn(&mut self, message: String) {
        self.warnings.push(Diagnostic::warning(message));
    }

    /// `rgb`, in the working space, in glTF's linear Rec.709. Moved by the
    /// conversion, it keeps twelve significant digits (eleven places below
    /// 1): a color read and written back is the number it was, not one a
    /// conversion there and back moved in its last bits (glTF's readers hold
    /// it in single precision). Unmoved (a working space of linear Rec.709),
    /// it is as it was.
    fn color(&self, rgb: [f64; 3]) -> Result<[f64; 3], Error> {
        let mut c = [rgb];
        self.writing.colors.working_to(LIN_REC709, &mut c)?;
        Ok(match c[0] == rgb {
            true => rgb,
            false => c[0].map(settled),
        })
    }
}
