//! Times reading and writing against the format's budgets:
//!
//! ```sh
//! cargo run --release --example bench
//! ```
//!
//! - a 1M-triangle GLB (positions, normals, UVs) read into the model, and
//!   written from it;
//! - the same as a `.gltf` whose buffer is a base64 `data:` URI, read;
//! - a GLB holding 200 MB of images, read (the images copied, not decoded).
use kinogaki_gltf::gltf::{
    self, Attribute, Gltf, Image, ImageData, Kind, Mesh, Node, Primitive, ReadOptions, Scene,
    Topology,
};
use std::hint::black_box;
use std::time::{Duration, Instant};

fn main() {
    let grid = grid(708);
    let triangles = grid.meshes[0].primitives[0].indices.len() / 3;
    let glb = gltf::to_glb(&grid).unwrap();
    report(
        &format!(
            "write a GLB of {triangles} triangles ({} MB)",
            glb.len() >> 20
        ),
        60,
        || {
            black_box(gltf::to_glb(&grid).unwrap());
        },
    );
    report(&format!("read a GLB of {triangles} triangles"), 40, || {
        black_box(read(&glb));
    });
    let files = gltf::to_gltf(&grid, "grid").unwrap();
    let (name, bin) = files.bin.unwrap();
    let uri = format!("data:application/octet-stream;base64,{}", base64(&bin));
    let text = files.json.replace(
        &format!("\"uri\":\"{name}\""),
        &format!("\"uri\":\"{uri}\""),
    );
    report(
        &format!(
            "read a base64 .gltf of {triangles} triangles ({} MB)",
            text.len() >> 20
        ),
        80,
        || {
            black_box(read(text.as_bytes()));
        },
    );
    let pictures = pictures(50, 4 << 20);
    report(
        &format!("read a GLB of {} MB of images", pictures.len() >> 20),
        30,
        || {
            black_box(read(&pictures));
        },
    );
}

fn read(bytes: &[u8]) -> Gltf {
    gltf::read(
        bytes,
        &mut |_| Err(std::io::ErrorKind::NotFound.into()),
        &ReadOptions::default(),
    )
    .unwrap()
}

/// Prints the first of seven runs of `run` (memory not yet reused) and the
/// best, beside the budget.
fn report(what: &str, budget_ms: u64, mut run: impl FnMut()) {
    let times: Vec<Duration> = (0..7)
        .map(|_| {
            let start = Instant::now();
            run();
            start.elapsed()
        })
        .collect();
    let ms = |d: &Duration| d.as_secs_f64() * 1e3;
    let best = times.iter().min().unwrap_or(&Duration::ZERO);
    println!(
        "{what}: first {:.1} ms, best {:.1} ms (budget {budget_ms} ms)",
        ms(&times[0]),
        ms(best)
    );
}

/// A `side` × `side` grid of vertices in two triangles a cell.
fn grid(side: u32) -> Gltf {
    let (mut positions, mut normals, mut uvs) = (Vec::new(), Vec::new(), Vec::new());
    for y in 0..side {
        for x in 0..side {
            let (u, v) = (x as f32 / (side - 1) as f32, y as f32 / (side - 1) as f32);
            positions.extend([u, (u * 40.0).sin() * 0.01, v]);
            normals.extend([0.0, 1.0, 0.0]);
            uvs.extend([u, v]);
        }
    }
    let mut indices = Vec::new();
    for y in 0..side - 1 {
        for x in 0..side - 1 {
            let corner = y * side + x;
            indices.extend([
                corner,
                corner + side,
                corner + 1,
                corner + 1,
                corner + side,
                corner + side + 1,
            ]);
        }
    }
    let attribute = |name: &str, kind, values: Vec<f32>| Attribute {
        name: name.into(),
        kind,
        values: values.into(),
    };
    let primitive = Primitive {
        topology: Topology::Triangles,
        indices,
        attributes: vec![
            attribute("POSITION", Kind::Vec3, positions),
            attribute("NORMAL", Kind::Vec3, normals),
            attribute("TEXCOORD_0", Kind::Vec2, uvs),
        ],
        material: None,
    };
    Gltf {
        scene: Some(0),
        scenes: vec![Scene {
            name: String::new(),
            nodes: vec![0],
        }],
        nodes: vec![Node {
            mesh: Some(0),
            ..Node::default()
        }],
        meshes: vec![Mesh {
            name: "Grid".into(),
            primitives: vec![primitive],
        }],
        ..Gltf::default()
    }
}

/// A GLB of `count` images of `size` bytes each.
fn pictures(count: usize, size: usize) -> Vec<u8> {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let images = (0..count)
        .map(|i| {
            let mut bytes: Vec<u8> = (0..size)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    state as u8
                })
                .collect();
            bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
            Image {
                name: format!("picture {i}"),
                mime_type: Some("image/png".into()),
                data: ImageData::Embedded(bytes),
            }
        })
        .collect();
    gltf::to_glb(&Gltf {
        images,
        ..Gltf::default()
    })
    .unwrap()
}

fn base64(bytes: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len() / 3 * 4 + 4);
    for chunk in bytes.chunks(3) {
        let bits = chunk
            .iter()
            .enumerate()
            .fold(0u32, |bits, (i, &b)| bits | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            text.push(if i <= chunk.len() {
                alphabet[(bits >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    text
}
