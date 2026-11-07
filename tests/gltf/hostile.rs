//! Hostile files: each shape that once asked Studio for far more than a
//! file holds, or reached files outside the file's folder, read as File ▸
//! Import reads one (`read_hostile`: a 2 MiB stack and a budget), ending
//! in its items, a clean error or a warning within a time bound.
use kinogaki_gltf::{Format, gltf};
use kinogaki_studio_api::format::files::Named;
use kinogaki_studio_api::format::{ReadBudget, ReadLimits, options};
use kinogaki_studio_api::geometry::Units;
use kinogaki_studio_api::standard::{self, ImportedMaterial};
use kinogaki_studio_api::{
    Error, FileFormat, FileItem, FormatOptions, Imported, Input, Purpose, Reading, Value, Writing,
};
use kinogaki_studio_api_testing::{Unconverted, read_hostile};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a hostile read may take in a debug build: seconds where the
/// shapes once took minutes, or never ended.
const BOUND: Duration = Duration::from_secs(10);

/// A glTF of the members `rest` after its asset.
fn gltf(rest: &str) -> String {
    format!(r#"{{"asset":{{"version":"2.0"}},{rest}}}"#)
}

/// `text` read as a file held in memory, with `limits`.
fn read(text: &str, limits: ReadLimits) -> (Result<Imported, Error>, Duration) {
    let input = Input::memory("hostile.gltf", text.as_bytes());
    read_hostile(&Format, &input, &Format.options(), limits)
}

/// The read refused, quickly, saying `why`.
fn refused(read: (Result<Imported, Error>, Duration), why: &str) {
    let (read, took) = read;
    let error = read.expect_err("refused");
    assert!(error.message().contains(why), "{error}");
    assert!(took < BOUND, "{took:?}");
}

/// A new folder holding `scene/` (where the file is read) beside a
/// `secret` file, by its canonical path.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kinogaki-gltf-hostile-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("scene/maps")).unwrap();
    std::fs::write(dir.join("secret"), b"key").unwrap();
    dir.canonicalize().unwrap()
}

#[test]
fn one_view_less_accessor_in_hundreds_of_primitives_is_refused_before_it_is_unwelded() {
    // 19 KB once asked for 103 GB: each primitive's vertices are points of
    // its own, so 429 uses of 10M zeros are 4.29G points.
    let primitives = vec![r#"{"attributes":{"POSITION":0}}"#; 429].join(",");
    let text = gltf(&format!(
        r#""accessors":[{{"componentType":5126,"count":9999999,"type":"VEC3"}}],
        "meshes":[{{"primitives":[{primitives}]}}],"nodes":[{{"mesh":0}}],"scenes":[{{"nodes":[0]}}],"scene":0"#
    ));
    assert!(text.len() < 20_000);
    refused(read(&text, ReadLimits::default()), "one allocation takes");
    // Under a small budget the accessor alone is refused.
    let small = ReadLimits {
        total: 64 << 20,
        ..ReadLimits::default()
    };
    refused(read(&text, small), "accessors[0]");
}

#[test]
fn a_view_less_accessor_of_gigabytes_is_refused_before_it_is_zeroed() {
    // 231 B once took 6.1 GB of zeros, then copies.
    let text = gltf(
        r#""accessors":[{"componentType":5126,"count":536870912,"type":"VEC3"}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"nodes":[{"mesh":0}]"#,
    );
    refused(read(&text, ReadLimits::default()), "accessors[0]");
}

#[test]
fn a_scene_listing_a_root_twice_is_refused() {
    // 25 KB once made 4,002,000 items: a root of 2,000 children, listed
    // 2,000 times.
    let children = (1..=2000)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let nodes = std::iter::once(format!(r#"{{"children":[{children}]}}"#))
        .chain(std::iter::repeat_n("{}".to_owned(), 2000))
        .collect::<Vec<_>>()
        .join(",");
    let roots = vec!["0"; 2000].join(",");
    let text = gltf(&format!(
        r#""nodes":[{nodes}],"scenes":[{{"nodes":[{roots}]}}],"scene":0"#
    ));
    refused(read(&text, ReadLimits::default()), "node 0 is listed twice");
}

#[test]
fn a_buffer_out_of_the_folder_is_refused_however_its_path_is_written() {
    let dir = scratch("buffers");
    let file = dir.join("scene/hostile.gltf");
    for uri in [
        "%2Fetc%2Fhosts",
        "../secret",
        "..%2Fsecret",
        "/dev/zero",
        "maps/../../secret",
    ] {
        let text = gltf(&format!(r#""buffers":[{{"uri":"{uri}","byteLength":3}}]"#));
        std::fs::write(&file, &text).unwrap();
        let input = Input::file(&file);
        let read = read_hostile(&Format, &input, &Format.options(), ReadLimits::default());
        refused(read, "outside the folder");
    }
    // Turned on, a name reaches outside, and still never a device.
    let mut anywhere = Format.options();
    anywhere.insert(options::OUTSIDE.into(), Value::Bool(true));
    let text = gltf(r#""buffers":[{"uri":"/dev/zero","byteLength":3}]"#);
    std::fs::write(&file, &text).unwrap();
    let read = read_hostile(
        &Format,
        &Input::file(&file),
        &anywhere,
        ReadLimits::default(),
    );
    refused(read, "not regular files");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_buffer_that_is_a_pipe_is_never_opened() {
    let dir = scratch("pipe");
    let pipe = dir.join("scene/data.bin");
    let made = std::process::Command::new("mkfifo")
        .arg(&pipe)
        .status()
        .unwrap();
    assert!(made.success());
    let file = dir.join("scene/hostile.gltf");
    std::fs::write(
        &file,
        gltf(r#""buffers":[{"uri":"data.bin","byteLength":3}]"#),
    )
    .unwrap();
    let read = read_hostile(
        &Format,
        &Input::file(&file),
        &Format.options(),
        ReadLimits::default(),
    );
    refused(read, "not regular files");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A glTF whose material reads each of `uris` as an image, through
/// textures.
fn textured(uris: &[&str]) -> String {
    let images = (uris.iter())
        .map(|u| format!(r#"{{"uri":"{u}"}}"#))
        .collect::<Vec<_>>();
    let textures = (0..uris.len())
        .map(|i| format!(r#"{{"source":{i}}}"#))
        .collect::<Vec<_>>();
    let materials = (0..uris.len())
        .map(|i| format!(r#"{{"pbrMetallicRoughness":{{"baseColorTexture":{{"index":{i}}}}}}}"#))
        .collect::<Vec<_>>();
    gltf(&format!(
        r#""images":[{}],"textures":[{}],"materials":[{}]"#,
        images.join(","),
        textures.join(","),
        materials.join(",")
    ))
}

/// Every Image node's file in `read`'s materials, as text.
fn image_files(read: &Imported) -> String {
    let held = standard::materials()
        .get(read.items[0].data.as_ref().unwrap())
        .unwrap();
    let networks: Vec<String> = (held.materials.iter())
        .map(|m: &ImportedMaterial| format!("{:?}", m.inside))
        .collect();
    networks.join("\n")
}

#[test]
fn images_out_of_reach_or_elsewhere_are_left_out_with_a_warning() {
    let dir = scratch("images");
    std::fs::write(dir.join("scene/maps/wood.png"), b"\x89PNG wood").unwrap();
    let pipe = dir.join("scene/maps/pipe.png");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap()
            .success()
    );
    let file = dir.join("scene/hostile.gltf");
    let uris = [
        "%2Fetc%2Fhosts",
        "../secret",
        "http://example.com/a.png",
        "maps/pipe.png",
        "maps/wood.png",
    ];
    std::fs::write(&file, textured(&uris)).unwrap();
    let mut embed = Format.options();
    embed.insert(options::EMBED.into(), Value::Bool(true));
    let (read, took) = read_hostile(&Format, &Input::file(&file), &embed, ReadLimits::default());
    let read = read.unwrap();
    assert!(took < BOUND);
    let said: Vec<&str> = read.warnings.iter().map(|w| w.message.as_str()).collect();
    let outside = said
        .iter()
        .find(|w| w.contains("outside the folder"))
        .expect("said");
    assert!(outside.ends_with("../secret, /etc/hosts"), "{said:?}");
    assert!(
        said.iter()
            .any(|w| w.contains("http://example.com/a.png") && w.contains("left out")),
        "{said:?}"
    );
    assert!(
        said.iter()
            .any(|w| w.ends_with("never read: maps/pipe.png")),
        "{said:?}"
    );
    let files = image_files(&read);
    for gone in ["hosts", "secret", "example.com", "pipe"] {
        assert!(!files.contains(gone), "{gone}: {files}");
    }
    assert!(files.contains("wood"), "the one in reach, embedded");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A GLB of `json` and the binary chunk `bin`.
fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut json = json.as_bytes().to_vec();
    json.resize(json.len().next_multiple_of(4), b' ');
    let mut bin = bin.to_vec();
    bin.resize(bin.len().next_multiple_of(4), 0);
    let length = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(length);
    out.extend(b"glTF");
    out.extend(2u32.to_le_bytes());
    out.extend((length as u32).to_le_bytes());
    out.extend((json.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(json);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(b"BIN\0");
    out.extend(bin);
    out
}

#[test]
fn images_in_one_buffer_view_share_its_bytes() {
    // A 4 MB GLB of 500 images on one view once made 500 copies (2.1 GB).
    let view = vec![0x89u8; 4 << 20];
    let images = vec![r#"{"bufferView":0,"mimeType":"image/png"}"#; 500].join(",");
    let json = gltf(&format!(
        r#""buffers":[{{"byteLength":{}}}],"bufferViews":[{{"buffer":0,"byteLength":{}}}],"images":[{images}]"#,
        view.len(),
        view.len()
    ));
    let bytes = glb(&json, &view);
    let model = gltf::read(&bytes, &mut |_| Err("none".into()), &ReadBudget::default()).unwrap();
    let gltf::ImageData::Embedded(first) = &model.images[0].data else {
        panic!()
    };
    for image in &model.images {
        let gltf::ImageData::Embedded(bytes) = &image.data else {
            panic!()
        };
        assert!(Arc::ptr_eq(first, bytes));
    }
    let input = Input::memory("shared.glb", &bytes);
    let (read, took) = read_hostile(&Format, &input, &Format.options(), ReadLimits::default());
    assert!(read.is_ok() && took < BOUND, "{took:?}");
}

/// A glTF of one point with `count` attributes of its own besides its
/// position.
fn attributed(count: usize) -> String {
    let names: Vec<String> = (0..count).map(|n| format!(r#""_A{n}":0"#)).collect();
    gltf(&format!(
        r#""buffers":[{{"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAA","byteLength":12}}],
        "bufferViews":[{{"buffer":0,"byteLength":12}}],
        "accessors":[{{"bufferView":0,"componentType":5126,"count":1,"type":"VEC3"}}],
        "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,{}}},"mode":0}}]}}],"nodes":[{{"mesh":0}}]"#,
        names.join(",")
    ))
}

#[test]
fn a_primitive_of_many_attributes_reads_in_linear_time() {
    // 829 KB of attribute names once took 24 s: each looked up by a scan.
    let text = attributed(90_000);
    assert!(text.len() > 800_000);
    let start = Instant::now();
    let model = gltf::read(
        text.as_bytes(),
        &mut |_| Err("none".into()),
        &ReadBudget::default(),
    );
    assert_eq!(
        model.unwrap().meshes[0].primitives[0].attributes.len(),
        90_001
    );
    assert!(start.elapsed() < BOUND, "{:?}", start.elapsed());
    // Each a point attribute of the mesh.
    let (read, took) = read(&attributed(4_000), ReadLimits::default());
    assert_eq!(read.unwrap().items.len(), 1);
    assert!(took < BOUND, "{took:?}");
}

#[test]
fn normals_of_many_primitives_are_found_in_linear_time() {
    // 227 KB once took 8.6 s: each primitive without normals scanned every
    // element of the mesh.
    let primitives = std::iter::once(r#"{"attributes":{"POSITION":0,"NORMAL":0}}"#)
        .chain(std::iter::repeat_n(
            r#"{"attributes":{"POSITION":0}}"#,
            8000,
        ))
        .collect::<Vec<_>>()
        .join(",");
    let text = gltf(&format!(
        r#""buffers":[{{"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA","byteLength":36}}],
        "bufferViews":[{{"buffer":0,"byteLength":36}}],
        "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}}],
        "meshes":[{{"primitives":[{primitives}]}}],"nodes":[{{"mesh":0}}]"#
    ));
    let (read, took) = read(&text, ReadLimits::default());
    assert!(read.is_ok(), "{read:?}");
    assert!(took < BOUND, "{took:?}");
}

/// The materials and one mesh of BoxTextured's GLB (its image embedded),
/// as Export hands them.
fn box_textured() -> Vec<FileItem> {
    let path = crate::common::sample("BoxTextured", "glTF-Binary");
    let reading = Reading::new(Purpose::Objects, Units::default(), &Unconverted);
    Format
        .read(&Input::file(&path), &Format.options(), &reading)
        .unwrap()
        .items
}

#[test]
fn a_gltf_written_beside_other_files_never_overwrites_them() {
    let dir = scratch("export");
    let scene = dir.join("scene");
    let writer = Format.writer().unwrap();
    let items = box_textured();
    let writing = Writing::new(Units::default(), &Unconverted);
    writer
        .write(
            &scene.join("first.gltf"),
            &items,
            &writing,
            &FormatOptions::new(),
        )
        .unwrap();
    let written: Vec<PathBuf> = std::fs::read_dir(scene.join("first_textures"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(
        written.len(),
        1,
        "the image beside, in the stem's textures folder"
    );
    let name = written[0].file_name().unwrap().to_owned();
    // Files of that name beside a second export, holding other bytes.
    let decoys = [
        scene.join(&name),
        scene.join("second_textures").join(&name),
        scene.join("image0.png"),
    ];
    for decoy in &decoys {
        std::fs::create_dir_all(decoy.parent().unwrap()).unwrap();
        std::fs::write(decoy, b"mine").unwrap();
    }
    writer
        .write(
            &scene.join("second.gltf"),
            &items,
            &writing,
            &FormatOptions::new(),
        )
        .unwrap();
    for decoy in &decoys {
        assert_eq!(
            std::fs::read(decoy).unwrap(),
            b"mine",
            "{}",
            decoy.display()
        );
    }
    // The second names its own copy, numbered past the decoy, and reads back.
    let text = std::fs::read_to_string(scene.join("second.gltf")).unwrap();
    assert!(
        text.contains("second_textures/") && text.contains("%202."),
        "{text}"
    );
    let back = read_hostile(
        &Format,
        &Input::file(&scene.join("second.gltf")),
        &Format.options(),
        ReadLimits::default(),
    );
    assert!(back.0.unwrap().warnings.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_file_held_in_memory_names_no_file_on_disk() {
    let text = textured(&["maps/wood.png", "../wood.png"]);
    let input = Input::memory("held.gltf", text.as_bytes());
    let reading = Reading::new(Purpose::Objects, Units::default(), &Unconverted);
    let named = Named::new(&input, &Format.options(), &reading);
    assert!(named.folder().is_none());
    let (read, _) = read_hostile(&Format, &input, &Format.options(), ReadLimits::default());
    let read = read.unwrap();
    let files = image_files(&read);
    assert!(
        files.contains("maps/wood.png") && !files.contains("../wood.png"),
        "{files}"
    );
    assert!(
        read.warnings
            .iter()
            .any(|w| w.message.contains("held in memory"))
    );
    let buffer = gltf(r#""buffers":[{"uri":"data.bin","byteLength":3}]"#);
    refused(
        read_hostile(
            &Format,
            &Input::memory("held.gltf", buffer.as_bytes()),
            &Format.options(),
            ReadLimits::default(),
        ),
        "held in memory",
    );
}
