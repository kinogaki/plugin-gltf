//! A glTF file to Studio's standard values.
//!
//! The model is read whole (`gltf::read`), then converted: its images to
//! the files Image nodes read, its materials to MaterialX glTF PBR networks
//! (`material`), its meshes to polygons (`mesh`), and the default scene's
//! node tree to items (`scene`): Parts, meshes (a mesh several nodes use as
//! instances of it, held once), lights and cameras, all moved from glTF's
//! metres, +Y up, into the project's space ([`Space`]). The materials item
//! comes first.
mod material;
pub(crate) mod mesh;
pub(crate) mod scene;

use crate::at_file;
use crate::gltf::{self, ImageData, ReadOptions};
use crate::space::Space;
use kinogaki_studio_api::edit::editor::{Placed, gather};
use kinogaki_studio_api::format::options;
use kinogaki_studio_api::geometry::image::{EmbeddedFile, ImageFile};
use kinogaki_studio_api::standard::{self, Materials};
use kinogaki_studio_api::{
    Diagnostic, Error, FileItem, FormatOptions, Imported, Input, Purpose, Reading, Value,
};
use std::path::Path;
use std::{fs, io};

/// The space glTF's colors are in: material factors, vertex colors and
/// lights are linear Rec.709 (sRGB) values.
pub(crate) const LINEAR_SRGB: &str = "Linear Rec.709 (sRGB)";

/// Reads `input` as `chosen` says: Embed Textures reads the images a
/// `.gltf` names beside it into the project.
pub(crate) fn read(
    input: &Input<'_>,
    chosen: &FormatOptions,
    reading: &Reading<'_>,
) -> Result<Imported, Error> {
    reading.check()?;
    let embed = options::option(chosen, options::EMBED, Value::bool)?;
    let folder = input.path().and_then(Path::parent);
    let mut model = {
        let bytes = input.bytes()?;
        let mut beside = |name: &str| match folder {
            Some(folder) => fs::read(folder.join(name)),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "a file held in memory has no files beside it",
            )),
        };
        let options = ReadOptions {
            cancel: Some(reading.cancel.shared_flag()),
            ..ReadOptions::default()
        };
        gltf::read(&bytes, &mut beside, &options).map_err(|e| at_file(input, e))?
    };
    reading.check()?;
    let mut warnings: Vec<Diagnostic> = (model.left_out.iter())
        .map(|left| Diagnostic::warning(left.to_string()))
        .collect();
    let space = Space::of(reading.units);
    let files = image_files(
        std::mem::take(&mut model.images),
        folder,
        embed,
        &mut warnings,
    );
    let colored = mesh::colored_materials(&model);
    let materials = material::materials(&model, &files, &colored, reading.colors, &space)?;
    let meshes = (model.meshes.iter())
        .map(|mesh| {
            reading.check()?;
            let polygons = mesh::geometry(mesh, &space, reading.colors, &mut warnings)?;
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
/// under the image's file name (taken, not copied), a path named beside the
/// file made absolute, and read in when `embed` (one that can't be read
/// stays a path, said).
fn image_files(
    images: Vec<gltf::Image>,
    folder: Option<&Path>,
    embed: bool,
    warnings: &mut Vec<Diagnostic>,
) -> Vec<ImageFile> {
    let mut files = Vec::with_capacity(images.len());
    for (at, image) in images.into_iter().enumerate() {
        let name = file_name(&image, at);
        files.push(match image.data {
            ImageData::Embedded(bytes) => ImageFile::Embedded(EmbeddedFile::new(&name, bytes)),
            ImageData::Path(path) => {
                let path = folder.map_or_else(|| path.clone().into(), |f| f.join(&path));
                let read = embed.then(|| fs::read(&path));
                match read {
                    Some(Ok(bytes)) => ImageFile::Embedded(EmbeddedFile::new(&name, bytes)),
                    Some(Err(e)) => {
                        let why = format!("{}: {e}, kept as a path", path.display());
                        warnings.push(Diagnostic::warning(why));
                        ImageFile::Path(path.display().to_string().into())
                    }
                    None => ImageFile::Path(path.display().to_string().into()),
                }
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
        ImageData::Path(path) => path.rsplit('/').next().unwrap_or_default(),
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
