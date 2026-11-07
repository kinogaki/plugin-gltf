//! Accessors decode every component type, normalized or not, packed or
//! strided, matrices with padded columns, and sparse over zeros.

use crate::common::{primitive, read_bytes};
use kinogaki_gltf::gltf::{Gltf, Kind};

fn values(model: &Gltf, name: &str) -> Vec<f32> {
    model.meshes[0].primitives[0]
        .attribute(name)
        .unwrap_or_else(|| panic!("{name}"))
        .values
        .to_vec()
}

#[test]
fn components_normalize_as_gltf_defines() {
    let mut bytes = Vec::new();
    // POSITION as normalized i16 (KHR_mesh_quantization), three vertices.
    for v in [32767i16, -32768, 0, 0, 16384, 0, -16384, 0, 32767] {
        bytes.extend(v.to_le_bytes());
    }
    bytes.extend([0, 0]); // to four bytes
    // COLOR_0 as normalized u8 RGBA.
    bytes.extend([255, 0, 51, 255, 0, 255, 102, 128, 1, 2, 3, 4]);
    // TEXCOORD_0 as normalized u16, _COUNT as plain u8, indices as u8.
    for v in [0u16, 65535, 32768, 0, 65535, 65535] {
        bytes.extend(v.to_le_bytes());
    }
    bytes.extend([7, 200, 0, 0]);
    bytes.extend([2, 1, 0, 0]);
    let views = r#"{"buffer":0,"byteLength":18},{"buffer":0,"byteOffset":20,"byteLength":12},
        {"buffer":0,"byteOffset":32,"byteLength":12},{"buffer":0,"byteOffset":44,"byteLength":3},
        {"buffer":0,"byteOffset":48,"byteLength":3}"#;
    let accessors = r#"{"bufferView":0,"componentType":5122,"normalized":true,"count":3,"type":"VEC3"},
        {"bufferView":1,"componentType":5121,"normalized":true,"count":3,"type":"VEC4"},
        {"bufferView":2,"componentType":5123,"normalized":true,"count":3,"type":"VEC2"},
        {"bufferView":3,"componentType":5121,"count":3,"type":"SCALAR"},
        {"bufferView":4,"componentType":5121,"count":3,"type":"SCALAR"}"#;
    let text = primitive(
        &bytes,
        views,
        accessors,
        r#""attributes":{"POSITION":0,"COLOR_0":1,"TEXCOORD_0":2,"_COUNT":3},"indices":4"#,
    );
    let model = read_bytes(text.as_bytes()).unwrap();
    let half = 16384.0 / 32767.0;
    assert_eq!(
        values(&model, "POSITION"),
        [1.0, -1.0, 0.0, 0.0, half, 0.0, -half, 0.0, 1.0]
    );
    let colors = values(&model, "COLOR_0");
    assert_eq!(colors[..4], [1.0, 0.0, 0.2, 1.0]);
    assert_eq!(colors[7], 128.0 / 255.0);
    assert_eq!(
        values(&model, "TEXCOORD_0"),
        [0.0, 1.0, 32768.0 / 65535.0, 0.0, 1.0, 1.0]
    );
    assert_eq!(values(&model, "_COUNT"), [7.0, 200.0, 0.0]);
    assert_eq!(*model.meshes[0].primitives[0].indices, [2, 1, 0]);
}

#[test]
fn strided_views_and_padded_matrix_columns_read_element_by_element() {
    let mut bytes = Vec::new();
    // Interleaved POSITION (12 bytes) and a MAT2 of i8 (two columns of two
    // bytes, each padded to four): a 20-byte stride, two vertices.
    for (x, m) in [(1.0f32, [1i8, 2, 3, 4]), (2.0, [-1, -2, -3, -4])] {
        for v in [x, 0.0, 0.0] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.extend([m[0] as u8, m[1] as u8, 0, 0, m[2] as u8, m[3] as u8, 0, 0]);
    }
    let views = r#"{"buffer":0,"byteLength":40,"byteStride":20}"#;
    let accessors = r#"{"bufferView":0,"componentType":5126,"count":2,"type":"VEC3"},
        {"bufferView":0,"byteOffset":12,"componentType":5120,"count":2,"type":"MAT2"}"#;
    let text = primitive(
        &bytes,
        views,
        accessors,
        r#""attributes":{"POSITION":0,"_FRAME":1},"mode":0"#,
    );
    let model = read_bytes(text.as_bytes()).unwrap();
    assert_eq!(values(&model, "POSITION"), [1.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
    assert_eq!(
        values(&model, "_FRAME"),
        [1.0, 2.0, 3.0, 4.0, -1.0, -2.0, -3.0, -4.0]
    );
    assert_eq!(
        model.meshes[0].primitives[0]
            .attribute("_FRAME")
            .unwrap()
            .kind,
        Kind::Mat2
    );
    assert_eq!(*model.meshes[0].primitives[0].indices, [0, 1]);
}

#[test]
fn a_sparse_accessor_without_a_view_replaces_zeros() {
    let mut bytes = vec![3, 0, 0, 0];
    for v in [5.0f32, 6.0, 7.0] {
        bytes.extend(v.to_le_bytes());
    }
    let views = r#"{"buffer":0,"byteLength":2},{"buffer":0,"byteOffset":4,"byteLength":12}"#;
    let accessors = r#"{"componentType":5126,"count":5,"type":"VEC3","sparse":{"count":1,
        "indices":{"bufferView":0,"componentType":5123},"values":{"bufferView":1}}}"#;
    let text = primitive(
        &bytes,
        views,
        accessors,
        r#""attributes":{"POSITION":0},"mode":0"#,
    );
    let model = read_bytes(text.as_bytes()).unwrap();
    let mut expected = vec![0.0; 15];
    expected[9..12].copy_from_slice(&[5.0, 6.0, 7.0]);
    assert_eq!(values(&model, "POSITION"), expected);
}
