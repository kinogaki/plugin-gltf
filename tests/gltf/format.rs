//! The format on the SDK: every sample imports as Studio's standard values
//! (materials as MaterialX glTF PBR networks with Image nodes, meshes,
//! instances, lights and cameras, in the project's space), and what is
//! written reads back as the same file: its materials lossless, its
//! geometry and placements as they were.
use crate::common;

use crate::common::samples;
use kinogaki_gltf::{Format, gltf};
use kinogaki_studio_api::geometry::image::{ImageColor, ImageFile};
use kinogaki_studio_api::geometry::{Domain, Units, UpAxis, Xform, names};
use kinogaki_studio_api::image_files::{IMAGE_NODE, image_node, image_source};
use kinogaki_studio_api::network::{NetworkSeed, NewNode};
use kinogaki_studio_api::standard::{self, Materials};
use kinogaki_studio_api::{
    FileFormat, FileItem, FormatOptions, Imported, Input, Purpose, Reading, Value, Writing,
};
use std::path::{Path, PathBuf};

/// A project whose working space is glTF's own, linear Rec.709: colors
/// arrive and leave as the file holds them.
#[derive(Debug)]
struct Rec709;

impl kinogaki_studio_api::color::ColorService for Rec709 {
    fn working(&self) -> &str {
        "Linear Rec.709 (sRGB)"
    }
    fn convert(
        &self,
        _: &str,
        _: &str,
        _: &mut [[f64; 3]],
    ) -> Result<(), kinogaki_studio_api::Error> {
        Ok(())
    }
    fn revision(&self) -> u64 {
        0
    }
}

/// The file at `path` imported in a project of `units`.
fn imported(path: &Path, units: Units) -> Imported {
    let reading = Reading::new(Purpose::Objects, units, &Rec709);
    (Format.read(&Input::file(path), &Format.options(), &reading))
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The materials item of `read`, which comes first.
fn materials(read: &Imported) -> &Materials {
    standard::materials()
        .get(read.items[0].data.as_ref().unwrap())
        .expect("the materials item first")
}

#[test]
fn a_file_without_materials_brings_no_materials_item() {
    let read = imported(
        &common::sample("Cameras", "glTF-Embedded"),
        Units::default(),
    );
    let held =
        |i: &FileItem| (i.data.as_ref()).is_some_and(|d| standard::materials().get(d).is_some());
    assert!(!read.items.iter().any(held));
}

/// Node `node`'s MaterialX signature.
fn signature(node: &NewNode) -> Option<&str> {
    match node.parameters.get("signature") {
        Some(Value::Choice(s)) => Some(s),
        _ => None,
    }
}

fn shown(seed: &NetworkSeed) -> &NewNode {
    let shown: Vec<&NewNode> = seed.nodes.iter().filter(|n| n.display).collect();
    assert_eq!(shown.len(), 1);
    shown[0]
}

#[test]
fn a_textured_box_imports_its_material_with_an_image_node_and_its_mesh() {
    for variant in ["glTF", "glTF-Binary", "glTF-Embedded"] {
        let path = common::sample("BoxTextured", variant);
        let read = imported(&path, Units::default());
        let held = materials(&read);
        let [m] = &held.materials[..] else {
            panic!("one material");
        };
        assert_eq!(
            signature(shown(&m.inside)),
            Some("ND_gltf_pbr_surfaceshader")
        );
        let colorimage = (m.inside.nodes.iter())
            .position(|n| signature(n) == Some("ND_kinogaki_gltf_colorimage"))
            .expect("the base color's texture");
        let wire = (m.inside.wires.iter())
            .find(|w| w.to == colorimage && w.input == "file")
            .expect("an Image node into its file");
        let image = &m.inside.nodes[wire.from];
        assert_eq!(
            (image.node_type, wire.output.as_str()),
            (IMAGE_NODE, image_node::IMAGE)
        );
        let source = image_source(&image.parameters).unwrap().unwrap();
        assert_eq!(*source.color(), ImageColor::Auto, "a color, read as tagged");
        match (variant, source.file()) {
            ("glTF", ImageFile::Path(p)) => {
                assert_eq!(
                    Path::new(&**p),
                    path.parent().unwrap().join("CesiumLogoFlat.png")
                );
            }
            ("glTF", file) => panic!("a path beside the file: {file:?}"),
            (_, ImageFile::Embedded(file)) => {
                assert!(
                    file.bytes().starts_with(b"\x89PNG"),
                    "the PNG's bytes, packed"
                );
            }
            (_, file) => panic!("packed: {file:?}"),
        }
        let polygons = (read.items.iter())
            .find_map(|i| standard::geometry().get(i.data.as_ref()?))
            .expect("the box");
        assert_eq!(polygons.primitives().len(), 12, "{variant}");
        let attributes = polygons.attributes();
        for name in [names::N, names::UV] {
            assert!(attributes.get(Domain::Point, name).is_some(), "{name}");
        }
        let key = attributes.text(Domain::Primitive, names::MATERIAL).unwrap();
        assert_eq!(key.get(0), Some(m.key.as_str()));
    }
}

#[test]
fn embed_textures_reads_the_images_beside_into_the_project() {
    let path = common::sample("BoxTextured", "glTF");
    let reading = Reading::new(Purpose::Objects, Units::default(), &Rec709);
    let mut options = Format.options();
    options.insert("embed".into(), Value::Bool(true));
    let read = Format
        .read(&Input::file(&path), &options, &reading)
        .unwrap();
    let m = &materials(&read).materials[0];
    let image = (m.inside.nodes.iter())
        .find(|n| n.node_type == IMAGE_NODE)
        .unwrap();
    let source = image_source(&image.parameters).unwrap().unwrap();
    let ImageFile::Embedded(file) = source.file() else {
        panic!("embedded");
    };
    assert_eq!(file.name(), "CesiumLogoFlat.png");
    let beside = std::fs::read(path.parent().unwrap().join("CesiumLogoFlat.png")).unwrap();
    assert_eq!(file.bytes(), &beside[..]);
}

/// `read` written as `name` (its extension choosing GLB or `.gltf`) in a
/// fresh folder, and that file's model.
fn written(read: &Imported, units: Units, name: &str) -> (gltf::Gltf, Vec<String>) {
    let dir = scratch(name);
    let path = dir.join(name);
    let writing = Writing::new(units, &Rec709);
    let writer = Format.writer().unwrap();
    let said = writer
        .write(&path, &read.items, &writing, &FormatOptions::new())
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    let model = crate::common::read_path(&path);
    std::fs::remove_dir_all(&dir).unwrap();
    (model, said.into_iter().map(|d| d.message).collect())
}

/// A new, empty folder for `name`, by its canonical path (the system's
/// temporary folder may be behind a link, which a relative path a written
/// `.gltf` names can't climb back through).
fn scratch(name: &str) -> PathBuf {
    static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let made = MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "kinogaki-gltf-{}-{made}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

#[test]
fn every_sample_writes_back_with_the_same_materials() {
    for path in samples() {
        let original = common::read_path(&path);
        let read = imported(&path, Units::default());
        for extension in ["glb", "gltf"] {
            let stem = path.file_stem().unwrap().to_string_lossy();
            let (back, said) = written(&read, Units::default(), &format!("{stem}.{extension}"));
            let at = format!("{} as .{extension}", path.display());
            assert_eq!(back.materials.len(), original.materials.len(), "{at}");
            for (a, b) in original.materials.iter().zip(&back.materials) {
                assert_eq!(
                    resolved(&original, a),
                    resolved(&back, b),
                    "{at}: {}",
                    a.name
                );
            }
            assert!(
                said.iter().all(|s| !s.contains("written as")),
                "{at}: {said:?}"
            );
        }
    }
}

/// `m` as a look: its textures resolved to their images' bytes and
/// samplers, its name and the fields glTF gives no effect at their
/// defaults.
fn resolved(model: &gltf::Gltf, m: &gltf::Material) -> String {
    let mut m = m.clone();
    if m.unlit {
        m = gltf::Material {
            name: m.name.clone(),
            base_color: m.base_color,
            base_color_texture: m.base_color_texture.clone(),
            alpha_mode: m.alpha_mode,
            alpha_cutoff: m.alpha_cutoff,
            double_sided: m.double_sided,
            unlit: true,
            ..gltf::Material::default()
        };
    }
    if m.alpha_mode != gltf::AlphaMode::Mask {
        m.alpha_cutoff = 0.5;
    }
    // A name has no look (an unnamed one is named in the library).
    m.name.clear();
    let text = format!("{m:?}");
    // Each texture by what it reads and how.
    let mut out = text.clone();
    for (t, texture) in model.textures.iter().enumerate() {
        let image = |i: Option<usize>| {
            i.map(|i| {
                let image = &model.images[i];
                let bytes = match &image.data {
                    gltf::ImageData::Embedded(b) => b.to_vec(),
                    gltf::ImageData::Path(p) | gltf::ImageData::Elsewhere(p) => {
                        p.as_bytes().to_vec()
                    }
                };
                (image.mime_type.clone(), bytes.len(), checksum(&bytes))
            })
        };
        let sampler = texture
            .sampler
            .map(|s| model.samplers[s].clone())
            .map(|mut s| {
                s.name.clear();
                s
            });
        let sampler = sampler.filter(|s| *s != gltf::Sampler::default());
        let look = format!(
            "{{{:?} {:?} {:?}}}",
            image(texture.source),
            image(texture.extension_source),
            sampler
        );
        out = out.replace(&format!("texture: {t},"), &format!("texture: {look},"));
    }
    out
}

/// A checksum of `bytes`.
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

#[test]
fn a_project_in_millimetres_z_up_holds_the_file_where_its_axes_put_it() {
    let path = common::sample("BoxTextured", "glTF-Binary");
    let metres = imported(&path, Units::default());
    let mm = Units::new(0.001, UpAxis::Z).unwrap();
    let millimetres = imported(&path, mm);
    let world = |read: &Imported| -> Vec<[f64; 3]> {
        let mut at = Vec::new();
        let mut placed = vec![kinogaki_studio_api::geometry::Xform::IDENTITY; read.items.len()];
        for (i, item) in read.items.iter().enumerate() {
            let parent = item.parent.map_or(placed[i], |p| placed[p]);
            placed[i] = parent.compose(&item.placement);
            if let Some(polygons) = item.data.as_ref().and_then(|d| standard::geometry().get(d)) {
                for p in polygons.positions().iter() {
                    at.push(placed[i].apply_point((*p).into()).into());
                }
            }
        }
        at
    };
    let (m, mm_points) = (world(&metres), world(&millimetres));
    assert_eq!(m.len(), mm_points.len());
    for (a, b) in m.iter().zip(&mm_points) {
        let expected = [a[0] * 1000.0, -a[2] * 1000.0, a[1] * 1000.0];
        let close = (0..3).all(|k| (expected[k] - b[k]).abs() < 1e-6);
        assert!(close, "{a:?} at {b:?}");
    }
    // Written from the project, the file holds it as before.
    let (back, _) = written(&millimetres, mm, "Box.glb");
    let (same, _) = written(&metres, Units::default(), "Box.glb");
    let positions = |model: &gltf::Gltf| {
        model.meshes[0].primitives[0]
            .attribute("POSITION")
            .unwrap()
            .values
            .to_vec()
    };
    let (a, b) = (positions(&back), positions(&same));
    assert!(
        a.iter().zip(&b).all(|(a, b)| (a - b).abs() < 1e-6),
        "{a:?}\n{b:?}"
    );
}

#[test]
fn a_mesh_several_nodes_use_is_instances_of_it_held_and_written_once() {
    let path = common::sample("TextureTransformTest", "glTF");
    let original = common::read_path(&path);
    let read = imported(&path, Units::default());
    let held: Vec<&kinogaki_studio_api::Data> = (read.items.iter())
        .filter_map(|i| i.data.as_ref())
        .filter(|d| standard::geometry().get(d).is_some() || standard::instances().get(d).is_some())
        .collect();
    // Each node's mesh: its own polygons, or what the instances it holds place.
    let mesh = |d: &kinogaki_studio_api::Data| match standard::instances().get(d) {
        Some(copies) => {
            assert_eq!(
                copies.xforms().as_slice(),
                [Xform::IDENTITY],
                "where the node is"
            );
            copies.prototypes()[0].identity()
        }
        None => d.identity(),
    };
    let instances = held
        .iter()
        .filter(|d| standard::instances().get(d).is_some());
    assert!(instances.count() > 1, "instances of a shared mesh");
    let distinct: std::collections::BTreeSet<usize> = held.iter().map(|d| mesh(d)).collect();
    assert_eq!(distinct.len(), original.meshes.len());
    let (back, _) = written(&read, Units::default(), "Shared.glb");
    assert_eq!(back.meshes.len(), original.meshes.len());
    let showing = |model: &gltf::Gltf| model.nodes.iter().filter(|n| n.mesh.is_some()).count();
    assert_eq!(
        showing(&back),
        showing(&original),
        "each node its mesh, as read"
    );
}

/// What a file's default scene shows: its elements by kind, and the sums of
/// every corner's place in the world, texture coordinates and color, an
/// element naming a vertex twice left out.
#[derive(Debug, PartialEq)]
struct Shown {
    elements: [usize; 3],
    sums: Vec<f64>,
}

fn shown_by(model: &gltf::Gltf) -> Shown {
    let mut shown = Shown {
        elements: [0; 3],
        sums: vec![0.0; 3 + 2 * 3 + 4],
    };
    let scene = model.scene.or((!model.scenes.is_empty()).then_some(0));
    let mut stack: Vec<(usize, [f64; 16])> = scene
        .map_or_else(Vec::new, |s| model.scenes[s].nodes.clone())
        .into_iter()
        .map(|n| (n, identity()))
        .collect();
    while let Some((n, above)) = stack.pop() {
        let node = &model.nodes[n];
        let world = times(&above, &node.transform.matrix());
        stack.extend(node.children.iter().map(|&c| (c, world)));
        let Some(mesh) = node.mesh else { continue };
        for p in &model.meshes[mesh].primitives {
            let width = p.topology.corners();
            let kind = match p.topology {
                gltf::Topology::Triangles => 0,
                gltf::Topology::Lines => 1,
                gltf::Topology::Points => 2,
            };
            for element in p.indices.chunks_exact(width) {
                if (1..width).any(|i| element[..i].contains(&element[i])) {
                    continue;
                }
                shown.elements[kind] += 1;
                for &v in element {
                    let v = v as usize;
                    let read = |name: &str, k: usize| {
                        p.attribute(name)
                            .map(|a| f64::from(a.values[v * a.kind.width() + k]))
                    };
                    let local = [0, 1, 2].map(|k| read("POSITION", k).unwrap());
                    for (k, sum) in shown.sums[..3].iter_mut().enumerate() {
                        let m = &world;
                        *sum +=
                            m[k] * local[0] + m[4 + k] * local[1] + m[8 + k] * local[2] + m[12 + k];
                    }
                    for set in 0..3 {
                        for k in 0..2 {
                            let t = read(&format!("TEXCOORD_{set}"), k).unwrap_or(0.0);
                            shown.sums[3 + set * 2 + k] += t;
                        }
                    }
                    for k in 0..4 {
                        let c = read("COLOR_0", k).unwrap_or(1.0);
                        shown.sums[9 + k] += c;
                    }
                }
            }
        }
    }
    shown
}

fn identity() -> [f64; 16] {
    std::array::from_fn(|i| if i % 5 == 0 { 1.0 } else { 0.0 })
}

/// `a × b`, column-major.
fn times(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    std::array::from_fn(|i| {
        let (col, row) = (i / 4, i % 4);
        (0..4).map(|k| a[k * 4 + row] * b[col * 4 + k]).sum()
    })
}

#[test]
fn every_sample_writes_back_its_meshes_where_they_were() {
    for path in samples() {
        let original = shown_by(&common::read_path(&path));
        let read = imported(&path, Units::default());
        let stem = path.file_stem().unwrap().to_string_lossy();
        let (back, _) = written(&read, Units::default(), &format!("{stem}.glb"));
        let back = shown_by(&back);
        let at = path.display();
        assert_eq!(back.elements, original.elements, "{at}");
        for (k, (a, b)) in original.sums.iter().zip(&back.sums).enumerate() {
            let near = (a - b).abs() <= 1e-4 * a.abs().max(1.0);
            assert!(near, "{at}: sum {k}: {a} written as {b}");
        }
    }
}

/// The texture node of `m` named `material`'s base color reads.
fn base_texture<'a>(read: &'a Imported, material: &str) -> &'a NewNode {
    let m = (materials(read).materials.iter())
        .find(|m| m.name == material)
        .unwrap_or_else(|| panic!("{material}"));
    (m.inside.nodes.iter())
        .find(|n| signature(n) == Some("ND_kinogaki_gltf_colorimage"))
        .expect("a base color texture")
}

#[test]
fn texture_transforms_and_samplers_are_the_texture_nodes_values() {
    let read = imported(
        &common::sample("TextureTransformTest", "glTF"),
        Units::default(),
    );
    let rotated = base_texture(&read, "Rotation");
    let degrees = rotated.parameters["rotate"].number().unwrap();
    assert!(
        (degrees - 22.5).abs() < 1e-9,
        "radians become degrees: {degrees}"
    );
    let all = base_texture(&read, "All");
    assert_eq!(all.parameters["offset"], Value::Numbers(vec![-0.2, -0.1]));
    assert_eq!(all.parameters["scale"], Value::Numbers(vec![1.5, 1.5]));
    let clamped = &all.parameters;
    for mode in ["uaddressmode", "vaddressmode"] {
        assert_eq!(clamped[mode], Value::Choice("clamp".into()), "{mode}");
    }
    assert_eq!(clamped["minfilter"], Value::Choice("linear".into()));
    assert!(clamped.get("filtertype").is_none(), "linear, the default");
    let read = imported(
        &common::sample("TextureSettingsTest", "glTF-Binary"),
        Units::default(),
    );
    let mirrored = (materials(&read).materials.iter())
        .flat_map(|m| &m.inside.nodes)
        .find(|n| n.parameters.get("vaddressmode") == Some(&Value::Choice("mirror".into())));
    let mirrored = mirrored.expect("a texture mirrored along V");
    assert_eq!(
        mirrored.parameters["minfilter"],
        Value::Choice("nearest_mipmap_linear".into())
    );
}

#[test]
fn unlit_materials_are_surface_unlit_cutout() {
    let read = imported(
        &common::sample("UnlitTest", "glTF-Binary"),
        Units::default(),
    );
    for m in &materials(&read).materials {
        let surface = shown(&m.inside);
        assert_eq!(
            signature(surface),
            Some("ND_kinogaki_surface_unlit"),
            "{}",
            m.name
        );
        assert!(
            surface.parameters.contains_key("alpha_mode"),
            "always set: it blends by default"
        );
    }
}

#[test]
fn lights_and_cameras_import_as_standard_values_and_write_back() {
    let path = common::sample("LightsPunctualLamp", "glTF-Binary");
    let original = common::read_path(&path);
    let read = imported(&path, Units::default());
    let lights = (read.items.iter())
        .filter_map(|i| standard::light().get(i.data.as_ref()?))
        .count();
    assert_eq!(lights, original.lights.len());
    let (back, _) = written(&read, Units::default(), "Lamp.glb");
    let sorted = |model: &gltf::Gltf| {
        let mut lights = model.lights.clone();
        lights.sort_by(|a, b| {
            (a.intensity, a.color[1])
                .partial_cmp(&(b.intensity, b.color[1]))
                .unwrap()
        });
        lights
    };
    for (light, written) in sorted(&original).iter().zip(sorted(&back)) {
        assert_eq!(written.kind, light.kind, "{}", light.name);
        let near = |a: f64, b: f64| (a - b).abs() <= 1e-5 * a.abs().max(1.0);
        assert!(
            near(written.intensity, light.intensity),
            "{} {}",
            written.intensity,
            light.intensity
        );
        assert!(
            (0..3).all(|k| near(written.color[k], light.color[k])),
            "{:?}",
            written.color
        );
    }
    let path = common::sample("Cameras", "glTF-Embedded");
    let original = common::read_path(&path);
    let read = imported(&path, Units::default());
    let cameras: Vec<_> = (read.items.iter())
        .filter_map(|i| standard::camera().get(i.data.as_ref()?))
        .collect();
    assert_eq!(cameras.len(), 2);
    assert!(
        (cameras[1].horizontal_aperture - 2000.0).abs() < 1e-3,
        "xmag 1 m is a 2 m view"
    );
    let (back, _) = written(&read, Units::default(), "Cameras.gltf");
    for (a, b) in original.cameras.iter().zip(&back.cameras) {
        match (&a.projection, &b.projection) {
            (
                gltf::Projection::Perspective {
                    yfov, znear, zfar, ..
                },
                gltf::Projection::Perspective {
                    yfov: y,
                    znear: n,
                    zfar: f,
                    ..
                },
            ) => {
                assert!((yfov - y).abs() < 1e-6 && (znear - n).abs() < 1e-6);
                assert!((zfar.unwrap() - f.unwrap()).abs() < 1e-4);
            }
            (
                gltf::Projection::Orthographic { xmag, ymag, .. },
                gltf::Projection::Orthographic {
                    xmag: x, ymag: y, ..
                },
            ) => assert!((xmag - x).abs() < 1e-6 && (ymag - y).abs() < 1e-6),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn a_file_node_reads_the_file_as_one_value() {
    let path = common::sample("TextureTransformTest", "glTF");
    let reading = Reading::new(Purpose::Value, Units::default(), &Rec709);
    let read = Format
        .read(&Input::file(&path), &Format.options(), &reading)
        .unwrap();
    let [item] = &read.items[..] else {
        panic!("one item");
    };
    let polygons = standard::geometry()
        .get(item.data.as_ref().unwrap())
        .unwrap();
    let model = common::read_path(&path);
    assert_eq!(polygons.primitives().len(), shown_by(&model).elements[0]);
}

#[test]
fn damaged_files_are_errors_and_a_cancelled_read_stops() {
    let path = common::sample("BoxTextured", "glTF-Binary");
    let bytes = std::fs::read(&path).unwrap();
    let reading = Reading::new(Purpose::Objects, Units::default(), &Rec709);
    for cut in [0, 12, 100, bytes.len() / 2, bytes.len() - 1] {
        let input = Input::memory("Box.glb", &bytes[..cut]);
        let read = Format.read(&input, &Format.options(), &reading);
        let why = read
            .expect_err("a cut file is an error")
            .message()
            .to_owned();
        assert!(why.starts_with("Box.glb"), "{why}");
    }
    let stop = kinogaki_studio_api::CancelToken::default();
    stop.cancel();
    let cancelled = Reading {
        budget: kinogaki_studio_api::format::ReadBudget::default().cancelled_by(stop),
        ..Reading::new(Purpose::Objects, Units::default(), &Rec709)
    };
    let read = Format.read(
        &Input::memory("Box.glb", &bytes),
        &Format.options(),
        &cancelled,
    );
    assert_eq!(read.unwrap_err(), kinogaki_studio_api::Error::cancelled());
}

/// Writes every sample through the format, as Studio exports it, into
/// `$GLTF_OUT` as a GLB and a `.gltf`, for the local validator
/// (`tools/validate.mjs`).
#[test]
#[ignore = "hand: writes files for the local validator"]
fn hand_run_write_every_sample_through_the_format_for_the_validator() {
    let out = PathBuf::from(std::env::var("GLTF_OUT").expect("GLTF_OUT, a directory"));
    let writing = Writing::new(Units::default(), &Rec709);
    let writer = Format.writer().unwrap();
    for path in samples() {
        let read = imported(&path, Units::default());
        let stem = path.file_stem().unwrap().to_string_lossy();
        let variant = path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy();
        for extension in ["glb", "gltf"] {
            let dir = out.join(format!("{stem}-{variant}-{extension}"));
            std::fs::create_dir_all(&dir).unwrap();
            let file = dir.join(format!("{stem}.{extension}"));
            writer
                .write(&file, &read.items, &writing, &FormatOptions::new())
                .unwrap();
        }
    }
}

#[test]
fn a_mesh_of_primitives_with_and_without_a_material_reads() {
    let text = r#"{"asset":{"version":"2.0"},
        "buffers":[{"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA","byteLength":36}],
        "bufferViews":[{"buffer":0,"byteLength":36}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}],
        "materials":[{}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0},{"attributes":{"POSITION":0}}]}],
        "nodes":[{"mesh":0}]}"#;
    let reading = Reading::new(Purpose::Objects, Units::default(), &Rec709);
    let input = Input::memory("mixed.gltf", text.as_bytes());
    let read = Format.read(&input, &Format.options(), &reading).unwrap();
    let mesh = standard::geometry()
        .get(read.items[1].data.as_ref().unwrap())
        .unwrap();
    let keys = (mesh.attributes())
        .text(Domain::Primitive, names::MATERIAL)
        .unwrap();
    assert_eq!((keys.get(0), keys.get(1)), (Some("0"), None));
}
