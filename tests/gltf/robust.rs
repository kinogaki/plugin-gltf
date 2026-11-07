//! Damaged input is an error naming where, never a panic: cut files,
//! mutated files, structural faults, budgets and cancellation.
use crate::common;

use crate::common::{primitive, read_bytes, samples};
use kinogaki_gltf::gltf::{self, GltfError, ReadOptions};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// The committed samples of at most `size` bytes, with no files beside.
fn small_samples(size: u64) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    for path in samples() {
        let bytes = std::fs::read(&path).unwrap();
        if bytes.len() as u64 <= size && read_bytes(&bytes).is_ok() {
            found.push((
                path.file_name().unwrap().to_string_lossy().into_owned(),
                bytes,
            ));
        }
    }
    assert!(found.len() >= 8, "{}", found.len());
    found
}

/// Reads with a small budget, so mutated counts can't allocate much.
fn read_bounded(bytes: &[u8]) -> Result<gltf::Gltf, GltfError> {
    let options = ReadOptions {
        max_decoded_bytes: 64 << 20,
        ..ReadOptions::default()
    };
    gltf::read(
        bytes,
        &mut |_| Err(std::io::ErrorKind::NotFound.into()),
        &options,
    )
}

#[test]
fn every_cut_of_a_file_is_an_error() {
    for (name, bytes) in small_samples(64 << 10) {
        let whole = bytes.trim_ascii_end().len();
        let step = (whole / 4000).max(1);
        for length in (0..whole).step_by(step) {
            assert!(
                read_bounded(&bytes[..length]).is_err(),
                "{name} cut at {length}"
            );
        }
    }
}

/// A xorshift generator: the same mutations on every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn mutated_files_read_or_fail_without_panicking() {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    let (mut read, mut failed) = (0, 0);
    for (_, bytes) in small_samples(64 << 10) {
        let digits: Vec<usize> = (0..bytes.len())
            .filter(|&i| bytes[i].is_ascii_digit())
            .collect();
        for _ in 0..400 {
            let mut mutated = bytes.clone();
            match random.below(4) {
                // A digit of the JSON changed: indices, counts, offsets, codes.
                0 | 1 => {
                    let at = digits[random.below(digits.len())];
                    mutated[at] = b'0' + random.below(10) as u8;
                }
                // A number made large.
                2 => {
                    let at = digits[random.below(digits.len())];
                    mutated.splice(at..at, *b"99999999");
                }
                // Any byte flipped, the binary chunk included.
                _ => {
                    for _ in 0..1 + random.below(4) {
                        let at = random.below(mutated.len());
                        mutated[at] ^= 1 << random.below(8);
                    }
                }
            }
            match read_bounded(&mutated) {
                Ok(_) => read += 1,
                Err(_) => failed += 1,
            }
        }
    }
    assert!(read > 0 && failed > 0, "{read} read, {failed} failed");
}

/// The triangle of `common::primitive` with `change` made to its JSON.
fn triangle_with(change: impl Fn(String) -> String) -> Result<gltf::Gltf, GltfError> {
    let mut bytes = Vec::new();
    for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bytes.extend(v.to_le_bytes());
    }
    bytes.extend([0, 0, 1, 0, 2, 0]);
    let views = r#"{"buffer":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":6}"#;
    let accessors = r#"{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"},
        {"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}"#;
    let text = primitive(
        &bytes,
        views,
        accessors,
        r#""attributes":{"POSITION":0},"indices":1"#,
    );
    read_bytes(change(text).as_bytes())
}

#[test]
fn structural_faults_are_errors_naming_where() {
    assert!(triangle_with(|text| text).is_ok());
    let cases: [(&str, &str, &str); 14] = [
        (
            r#""nodes":[{"mesh":0}]"#,
            r#""nodes":[{"mesh":0},{"children":[2]},{"children":[1]}]"#,
            "nodes[1]",
        ),
        (
            r#""nodes":[{"mesh":0}]"#,
            r#""nodes":[{"mesh":0,"children":[1]},{},{"children":[1]}]"#,
            "nodes[2].children",
        ),
        (r#""nodes":[0]"#, r#""nodes":[1]"#, "scenes[0].nodes"),
        (
            r#""nodes":[{"mesh":0}]"#,
            r#""nodes":[{"mesh":1}]"#,
            "nodes[0].mesh",
        ),
        (
            r#""nodes":[{"mesh":0}]"#,
            r#""nodes":[{"mesh":0,"matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],"scale":[1,1,1]}]"#,
            "nodes[0].matrix",
        ),
        (
            r#""indices":1"#,
            r#""indices":1,"material":0"#,
            "meshes[0].primitives[0].material",
        ),
        (
            r#""count":3,"type":"SCALAR""#,
            r#""count":4,"type":"SCALAR""#,
            "accessors[1]",
        ),
        (
            r#""byteOffset":36,"byteLength":6"#,
            r#""byteOffset":40,"byteLength":6"#,
            "bufferViews[1].byteLength",
        ),
        (
            r#""POSITION":0"#,
            r#""POSITION":1"#,
            "meshes[0].primitives[0].attributes.POSITION",
        ),
        (
            r#""POSITION":0"#,
            r#""POSITION":0,"TEXCOORD_0":0"#,
            "meshes[0].primitives[0].attributes.TEXCOORD_0",
        ),
        (
            r#""indices":1"#,
            r#""indices":1,"mode":1"#,
            "meshes[0].primitives[0].mode",
        ),
        (
            r#""uri":"data:"#,
            r#""uri":"http://example.com/a.bin#"#,
            "buffers[0].uri",
        ),
        (r#""version":"2.0""#, r#""version":"1.0""#, "asset.version"),
        (
            r#""componentType":5123"#,
            r#""componentType":5124"#,
            "accessors[1].componentType",
        ),
    ];
    for (from, to, at) in cases {
        match triangle_with(|t| t.replace(from, to)) {
            Err(GltfError::Malformed { at: found, .. }) => assert_eq!(found, at, "{to}"),
            other => panic!("{to}: {:?}", other.map(|_| "read")),
        }
    }
    // Index 2 made 4 in the binary data, past the three vertices.
    let past = triangle_with(|t| t.replace("AAABAAIA", "AAABAAQA")).unwrap_err();
    assert!(
        matches!(&past, GltfError::Malformed { at, message } if at == "meshes[0].primitives[0].indices" && message.contains('4'))
    );
}

#[test]
fn a_required_extension_not_read_is_refused_and_a_used_one_said() {
    let required = r#""extensionsRequired":["KHR_draco_mesh_compression"],"extensionsUsed":["KHR_draco_mesh_compression"],"asset""#;
    let error = triangle_with(|t| t.replacen(r#""asset""#, required, 1)).unwrap_err();
    assert!(matches!(&error, GltfError::Unsupported(name) if name == "KHR_draco_mesh_compression"));
    let used = triangle_with(|t| {
        t.replacen(
            r#""asset""#,
            r#""extensionsUsed":["EXT_mesh_gpu_instancing"],"asset""#,
            1,
        )
    });
    assert_eq!(
        used.unwrap().left_out,
        [gltf::LeftOut::Extension("EXT_mesh_gpu_instancing".into())]
    );
    let extras =
        triangle_with(|t| t.replace(r#""mesh":0"#, r#""mesh":0,"extras":{"extras":1}"#)).unwrap();
    assert_eq!(extras.left_out, [gltf::LeftOut::Extras(1)]);
}

#[test]
fn decoding_past_the_budget_is_refused_before_allocating() {
    let huge = triangle_with(|t| {
        t.replace(
            r#"{"bufferView":0,"componentType":5126,"count":3"#,
            r#"{"componentType":5126,"count":1099511627776"#,
        )
    });
    assert!(matches!(huge, Err(GltfError::Limit(_))), "{huge:?}");
    let options = ReadOptions {
        max_decoded_bytes: 40,
        ..ReadOptions::default()
    };
    let bytes = std::fs::read(common::sample("BoxTextured", "glTF-Binary")).unwrap();
    let small = gltf::read(
        &bytes,
        &mut |_| Err(std::io::ErrorKind::NotFound.into()),
        &options,
    );
    assert!(matches!(small, Err(GltfError::Limit(_))));
}

#[test]
fn a_raised_flag_cancels_a_read() {
    let bytes = std::fs::read(common::sample("BoxTextured", "glTF-Binary")).unwrap();
    let flag = Arc::new(AtomicBool::new(true));
    let options = ReadOptions {
        cancel: Some(flag.clone()),
        ..ReadOptions::default()
    };
    let read = gltf::read(
        &bytes,
        &mut |_| Err(std::io::ErrorKind::NotFound.into()),
        &options,
    );
    assert!(matches!(read, Err(GltfError::Cancelled)));
    flag.store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(
        gltf::read(
            &bytes,
            &mut |_| Err(std::io::ErrorKind::NotFound.into()),
            &options
        )
        .is_ok()
    );
}
