# glTF

glTF 2.0 and GLB for [Kinogaki Studio](https://github.com/kinogaki): the file's own model, its reader and its writer. The library crate is `kinogaki-gltf`; its `gltf` module knows no SDK and uses nothing but the standard library (its JSON and base64 are its own). The format on Studio's public [studio-sdk](https://github.com/kinogaki/studio-sdk) converts between this model and Studio's standard values: geometry, MaterialX glTF PBR materials, Image nodes, lights and cameras.

```rust
use kinogaki_gltf::gltf::{self, ReadOptions};

let model = gltf::read_file(path, &ReadOptions::default())?;
for material in &model.materials {
    println!("{}: base color {:?}, clearcoat {}", material.name, material.base_color, material.clearcoat.factor);
}
gltf::write_file(&model, &out.join("copy.glb"))?;
```

## The model

`Gltf` keeps glTF's own objects and their indices (scenes, nodes, meshes, materials, textures, samplers, images, cameras, lights), so a file reads and writes back structurally. It hides only the encoding: buffers, buffer views and accessors become typed arrays on the primitives, and images their files' bytes or paths. Material fields carry MaterialX's `gltf_pbr` names (`base_color`, `metallic`, `emissive_strength`, `clearcoat.roughness`), each documented with the glTF property it holds; an extension's fields sit in a struct whose `Default` is the extension absent.

## Reading

- **Files.** `.gltf` text with the files it names beside it (relative paths, percent-decoded), base64 `data:` URIs, and GLB. `read` takes the bytes and a function giving a named file's bytes, so a file held in memory reads as one on disk; `read_file` finds them beside the path. Other URI schemes and absolute paths are refused.
- **Accessors.** Every component type, normalized or not (`u8` 255 is 1.0, as glTF defines), packed or strided, matrices with padded columns, sparse over a buffer view or over zeros; quantized attributes (`KHR_mesh_quantization`) read as their values. Values are `f32` on the attribute; values a file shares between primitives stay one allocation.
- **Primitives.** Points, lines and triangles: strips, fans and loops arrive as lists (a strip's joining degenerate triangles dropped), a primitive without indices as `0..count`. Attributes `POSITION`, `NORMAL`, `TANGENT`, `TEXCOORD_n`, `COLOR_n` and application ones (`_NAME`).
- **Materials.** Metallic-roughness with every ratified material extension glTF PBR carries: `KHR_materials_anisotropy`, `clearcoat`, `dispersion`, `emissive_strength`, `ior`, `iridescence`, `sheen`, `specular`, `transmission`, `unlit` and `volume`. `KHR_texture_transform` folds into the texture reference (its `texCoord` included). Textures keep their samplers (both filters, both wraps) and a `KHR_texture_basisu`, `EXT_texture_webp` or `EXT_texture_avif` source beside `source`.
- **Images.** Their files' bytes, never decoded, with their MIME type (declared, else from the bytes or the path's extension); images named by path stay paths unless `ReadOptions::embed_images`.
- **Scene.** Nodes (a matrix or translation, rotation and scale), scenes, perspective and orthographic cameras, `KHR_lights_punctual` lights.
- **Left out, and said** (`Gltf::left_out`): animations (a file reads at rest), skins (meshes in their bind pose, `JOINTS_n` and `WEIGHTS_n` dropped), morph targets (applied at the mesh's default `weights`), `extras`, and extensions used but not read (`KHR_materials_variants`, for one). A file requiring an extension not read (`KHR_draco_mesh_compression`, `EXT_meshopt_compression`) is refused.
- **Checks.** Every index, range and count, and the node trees (one parent each, no cycles, scenes of roots). A damaged file is a `GltfError` naming the JSON path (`meshes[0].primitives[1].indices`), never a panic. `ReadOptions::max_decoded_bytes` bounds what decoding allocates, and `ReadOptions::cancel` stops a read between primitives.

## Writing

- A GLB (`to_glb`), or `.gltf` text with `<stem>.bin` and the embedded images beside it, named after themselves (`to_gltf`); `write_file` chooses by the extension.
- One buffer: attributes as floats (values shared between primitives written once), `POSITION` with its bounds, indices as 16-bit or 32-bit integers. Embedded images go in the GLB's buffer, or beside the `.gltf`; images named by path stay paths.
- Members at glTF's defaults and extensions at theirs are left out. `extensionsUsed` lists every extension the written JSON names; an image extension without a `source` to fall back on is required.
- Every index in the model is checked first (`GltfError::Invalid` names where), so a written file is valid. What is written reads back as the same model, and writing that again gives the same bytes.

## Code

| File | Role |
|---|---|
| `src/gltf/model/` | `Gltf` and its objects: `mod.rs` (scene, nodes, cameras, lights, what was left out), `mesh.rs`, `material.rs` (materials, textures, samplers, images) |
| `src/gltf/read/` | `mod.rs` (the read, top-down; buffers; images), `fields.rs` (typed JSON members with their paths), `accessor.rs` (views and accessors), `mesh.rs`, `material.rs`, `scene.rs` |
| `src/gltf/write/` | `mod.rs` (the document; image files), `binary.rs` (the buffer, its views and accessors), `material.rs`, `scene.rs` |
| `src/gltf/json/` | JSON text to a value tree (strings without escapes borrowed, scanned eight bytes at a time) and back |
| `src/gltf/base64.rs`, `uri.rs`, `glb.rs` | `data:` URIs, relative paths and image types; the GLB container |

## Tests and data

`cargo test` runs:

- `tests/samples.rs`: every sample reads, saying what it leaves out; sparse accessors, primitive modes, morph targets at default weights, interleaved views, samplers, texture transforms, cameras, lights and every material extension read as the samples' READMEs describe;
- `tests/round_trip.rs`: every sample, and a model setting every field, written as a GLB and as a `.gltf` reads back as the same model (so its materials are lossless); writing is byte-for-byte repeatable; a model that can't be written is refused naming where;
- `tests/accessors.rs`: component types, normalization, strides, padded matrix columns and sparse accessors on hand-made buffers;
- `tests/robust.rs`: every cut of the small samples is an error; 6,400 mutations (digits, large numbers, flipped bits) read or fail without panicking; structural faults name their place; budgets and cancellation.

Two tests run by hand: `hand_run_fetched_samples_read_and_write_back_the_same` reads the samples `tools/fetch_samples.sh` fetches, and `hand_run_write_the_model_setting_every_field_for_the_validator` writes that model into `$GLTF_OUT`.

**Validation.** The Khronos [glTF-Validator](https://github.com/KhronosGroup/glTF-Validator), installed locally from npm (`gltf-validator`) and never a dependency, checks what the writer writes: `examples/rewrite.rs` writes each sample back as a GLB and a `.gltf`, and `tools/validate.mjs` validates them. Every written file has no errors; the only warnings (normal maps on primitives without tangents) are the source files' own. The validator release used (2.0.0-dev.3.10) doesn't know `KHR_texture_basisu`, so the model written for it has no KTX2 image.

**Test data.** `tests/fixtures/khronos/` holds models from the Khronos [glTF-Sample-Assets](https://github.com/KhronosGroup/glTF-Sample-Assets) (commit `8f492390ddc7a2ae221a1369ef560589cf8c70fb`), each with its own `LICENSE.md` and `metadata.json`, and the licence texts they cite in `LICENSES/`. The marks in BoxTextured and MultiUVTest are their owners' and are used under their guidelines; the model licences grant no rights in them.

| Model | Variants | By | Licence |
|---|---|---|---|
| AlphaBlendModeTest | glTF-Binary | Ed Mackey (Analytical Graphics, Inc.) | CC-BY-4.0 |
| AnisotropyBarnLamp | glTF-Binary | Eric Chadwick (Wayfair, LLC) | CC-BY-4.0 |
| AttenuationTest | glTF-Binary | Ed Mackey (Analytical Graphics, Inc.) | CC-BY-4.0 |
| BoxInterleaved | glTF-Binary | Cesium | CC-BY-4.0 |
| BoxTextured | glTF, glTF-Binary, glTF-Embedded | Cesium; the Cesium logo | CC-BY-4.0, LicenseRef-LegalMark-Cesium |
| Cameras | glTF-Embedded | Marco Hutter | CC0-1.0 |
| ClearCoatTest | glTF-Binary | Ed Mackey (Analytical Graphics, Inc.) | CC-BY-4.0 |
| DispersionTest | glTF-Binary | Ed Mackey (AGI) | CC-BY-4.0 |
| EmissiveStrengthTest | glTF-Binary | Ed Mackey (AGI) | CC-BY-4.0 |
| IridescenceLamp | glTF-Binary | Eric Chadwick (Wayfair, LLC) | CC-BY-4.0 |
| LightsPunctualLamp | glTF-Binary | DGG | CC-BY-4.0 |
| MeshPrimitiveModes | glTF-Embedded | Marco Hutter | CC0-1.0 |
| MetalRoughSpheres | glTF-Binary | Ed Mackey (Analytical Graphics, Inc.) | CC-BY-4.0 |
| MultiUVTest | glTF-Binary | Hilo 3D; the Khronos logo | CC-BY-4.0, LicenseRef-LegalMark-Khronos |
| NormalTangentTest | glTF-Binary | Ed Mackey (Analytical Graphics, Inc.) | CC0-1.0 |
| SheenChair | glTF-Binary | Eric Chadwick (Wayfair, LLC) | CC0-1.0 |
| SimpleMorph | glTF-Embedded | Marco Hutter | CC0-1.0 |
| SimpleSkin | glTF-Embedded | Marco Hutter | CC0-1.0 |
| SimpleSparseAccessor | glTF-Embedded | Marco Hutter | CC-BY-4.0 |
| SpecularTest | glTF-Binary | Ed Mackey (Analytical Graphics, Inc.) | CC-BY-4.0 |
| TextureCoordinateTest | glTF-Binary | Ed Mackey (Analytical Graphics, Inc.) | CC0-1.0 |
| TextureSettingsTest | glTF-Binary | Ed Mackey (Analytical Graphics, Inc.) | CC-BY-4.0 |
| TextureTransformMultiTest | glTF-Binary | Ed Mackey (Analytical Graphics, Inc.) | CC-BY-4.0 |
| TextureTransformTest | glTF | Microsoft | CC0-1.0 |
| TransmissionTest | glTF-Binary | Adobe | CC0-1.0 |
| TriangleWithoutIndices | glTF-Embedded | Marco Hutter | CC0-1.0 |
| UnlitTest | glTF-Binary | Ed Mackey (Analytical Graphics, Inc.) | CC-BY-4.0 |
| VertexColorTest | glTF-Binary | Ed Mackey (Analytical Graphics, Inc.) | CC-BY-4.0 |

Two samples are fetched rather than committed, for the hand-run test: DamagedHelmet (CC-BY-4.0 with a CC-BY-NC-4.0 part) and Sponza (the CRYENGINE agreement). `tools/fetch_samples.sh` puts them in `target/khronos-fetched/` from the same commit.

## Performance

`cargo run --release --example bench`, Apple M4 Max under other work, the first of seven runs and the best:

| | First | Best | Budget |
|---|---|---|---|
| A 1M-triangle GLB (positions, normals, UVs; 26 MB) read into the model | 6.1 ms | 2.8 ms | 40 ms |
| The same as a `.gltf` of a base64 `data:` URI (35 MB) read | 16.3 ms | 12.8 ms | 80 ms |
| A GLB of 200 MB of images read (copied, not decoded) | 6.7 ms | 2.8 ms | 30 ms |
| The 1M-triangle model written as a GLB | 4.5 ms | 3.4 ms | 60 ms |

The budgets are for a file into Studio's standard values; these times are the model's share of them.

## Building

The crate builds from this repository alone with the compiler Studio's SDK pins (`rust-toolchain.toml` there; 1.98.1):

```sh
cargo test
cargo run --release --example bench
```

## License

Dual-licensed under the MIT license ([LICENSE-MIT](LICENSE-MIT)) and the Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option. The test data keeps its own licences, above.
