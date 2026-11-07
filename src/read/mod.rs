//! A glTF file to Studio's standard values.
//!
//! The model is read whole (`gltf::read`), then converted: its images to
//! the files Image nodes read, its materials to MaterialX glTF PBR networks
//! (`material`), its meshes to polygons (`mesh`), and the default scene's
//! node tree to items (`scene`): Parts, meshes (a mesh several nodes use as
//! instances of it, held once), lights and cameras, all moved from glTF's
//! metres, +Y up, into the project's space (the SDK's `Conversion`). The materials item
//! comes first.
mod material;
pub(crate) mod mesh;
pub(crate) mod scene;

use crate::at_file;
use crate::gltf::{self, ImageData};
use crate::space;
use kinogaki_studio_api::edit::editor::{Placed, gather};
use kinogaki_studio_api::format::files::Named;
use kinogaki_studio_api::geometry::image::{EmbeddedFile, ImageFile};
use kinogaki_studio_api::standard::{self, Materials};
use kinogaki_studio_api::{
    Diagnostic, Error, FileItem, FormatOptions, Imported, Input, Purpose, Reading,
};
use std::collections::HashMap;
use std::path::Path;

/// Reads `input` as `chosen` says: the files a `.gltf` names are reached
/// through [`Named`] (in its folder and below, unless Read Files Outside
/// the Folder), and Embed Textures reads its images into the project.
pub(crate) fn read(
    input: &Input<'_>,
    chosen: &FormatOptions,
    reading: &Reading<'_>,
) -> Result<Imported, Error> {
    reading.check()?;
    let named = Named::new(input, chosen, reading);
    let mut model = {
        let bytes = input.bytes()?;
        let mut files = |name: &str| match named.file(name, None) {
            Some(path) => named.read(&path).map_err(|e| e.message().to_owned()),
            None => Err(named
                .warnings()
                .into_iter()
                .map(|w| w.message)
                .collect::<Vec<_>>()
                .join("; ")),
        };
        gltf::read(&bytes, &mut files, &reading.budget).map_err(|e| at_file(input, e))?
    };
    reading.check()?;
    let mut warnings: Vec<Diagnostic> = (model.left_out.iter())
        .map(|left| Diagnostic::warning(left.to_string()))
        .collect();
    let space = space::into_project(reading.units);
    let files = image_files(std::mem::take(&mut model.images), &named, &mut warnings);
    warnings.extend(named.warnings());
    let colored = mesh::colored_materials(&model);
    let materials = material::materials(&model, &files, &colored, reading.colors, &space)?;
    let meshes = (model.meshes.iter())
        .map(|mesh| {
            reading.check()?;
            let polygons = mesh::geometry(mesh, &space, reading, &mut warnings)?;
            Ok(polygons.map(|p| standard::geometry().wrap(p)))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let materials = FileItem::top(
        "Materials",
        standard::materials().wrap(Materials {
            materials,
            images: Vec::new(),
        }),
    );
    if reading.purpose == Purpose::Value && reading.wants == Some(standard::materials().id()) {
        return Ok(Imported {
            items: vec![materials],
            warnings,
        });
    }
    // The materials item first, when the file has any.
    let mut items = match model.materials.is_empty() {
        true => Vec::new(),
        false => vec![materials],
    };
    let colors = reading.colors;
    let meshes = scene::shared(&model, meshes)?;
    scene::items(&model, &meshes, &space, colors, &mut items, &mut warnings)?;
    let items = match reading.purpose {
        Purpose::Objects => items,
        Purpose::Value => vec![gathered(&items, input)?],
    };
    Ok(Imported { items, warnings })
}

/// Every mesh of `items` where it is placed (instances realized), as one
/// Geometry named after the file: what a File node reads.
fn gathered(items: &[FileItem], input: &Input<'_>) -> Result<FileItem, Error> {
    let mut world = Vec::with_capacity(items.len());
    let mut placed = Vec::new();
    for item in items {
        let parent = item
            .parent
            .map_or(kinogaki_studio_api::geometry::Xform::IDENTITY, |p| world[p]);
        let at = parent.compose(&item.placement);
        world.push(at);
        let Some(data) = &item.data else { continue };
        if let Some(polygons) = standard::geometry().get(data) {
            placed.push((polygons.clone(), at));
        } else if let Some(copies) = standard::instances().get(data) {
            for (i, xform) in copies.xforms().iter().enumerate() {
                let prototype = &copies.prototypes()[copies.prototype_index(i)];
                if let Some(polygons) = standard::geometry().get(prototype) {
                    placed.push((polygons.clone(), at.compose(xform)));
                }
            }
        }
    }
    let placed: Vec<Placed<'_>> = (placed.iter())
        .map(|(value, at)| Placed { value, at: *at })
        .collect();
    let name = Path::new(input.name()).file_stem();
    let name = name.map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    Ok(FileItem::top(
        name,
        standard::geometry().wrap(gather(&placed)?),
    ))
}

/// Each of `images` as the file an Image node reads: packed bytes embedded
/// under the image's file name, one file a buffer view however many images
/// share it; a path as [`Named::image`] finds it (read in with Embed
/// Textures); none for an image out of reach or named by a URI of another
/// scheme, said.
fn image_files(
    images: Vec<gltf::Image>,
    named: &Named,
    warnings: &mut Vec<Diagnostic>,
) -> Vec<Option<ImageFile>> {
    let mut packed: HashMap<*const u8, EmbeddedFile> = HashMap::new();
    let mut files = Vec::with_capacity(images.len());
    for (at, image) in images.into_iter().enumerate() {
        let name = file_name(&image, at);
        files.push(match &image.data {
            ImageData::Embedded(bytes) => {
                let file = packed
                    .entry(bytes.as_ptr())
                    .or_insert_with(|| EmbeddedFile::new(&name, bytes.to_vec()));
                Some(ImageFile::Embedded(file.named(&name)))
            }
            ImageData::Path(path) => named.image(path, None),
            ImageData::Elsewhere(uri) => {
                warnings.push(Diagnostic::warning(format!(
                    "image {at} ({uri}): no file is read from there, left out"
                )));
                None
            }
        });
    }
    files
}

/// The file name image `at` is kept under: its name (its path's last part
/// for one named by path), else `image<at>`, ending in the extension its
/// media type says.
fn file_name(image: &gltf::Image, at: usize) -> String {
    let named = match &image.data {
        ImageData::Path(path) | ImageData::Elsewhere(path) => {
            path.rsplit('/').next().unwrap_or_default()
        }
        ImageData::Embedded(_) => image.name.as_str(),
    };
    let named = named.replace(['/', '\\'], "_");
    let stem = match named.trim() {
        "" => format!("image{at}"),
        named => named.to_owned(),
    };
    match image.extension() {
        Some(extension) if gltf::media_type(&stem) != image.mime_type.as_deref() => {
            format!("{stem}.{extension}")
        }
        _ => stem,
    }
}
