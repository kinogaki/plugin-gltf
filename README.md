# glTF

glTF 2.0 and GLB for [Kinogaki Studio](https://github.com/kinogaki), a format plugin on Studio's public [studio-sdk](https://github.com/kinogaki/studio-sdk): File ▸ Import and Export of `.gltf` and `.glb`, and the File node. The library crate `kinogaki-gltf` holds the file's own model, its reader and its writer (the `gltf` module, which uses nothing of the SDK but the read's budget, which it checks the file's claims against; its JSON and base64 are its own) and the format, which converts between that model and Studio's standard values: polygons, MaterialX glTF PBR materials with Image nodes, lights and cameras. `plugin/` (`kinogaki-plugin-gltf`) is the library Studio loads, `formats/gltf.dylib`; the package is `kinogaki.gltf`, the format `kinogaki.format.gltf`.

```rust
use kinogaki_gltf::gltf::{self, ReadOptions};

let model = gltf::read_file(path, &ReadOptions::default())?;
for material in &model.materials {
    println!("{}: base color {:?}, clearcoat {}", material.name, material.base_color, material.clearcoat.factor);
}
gltf::write_file(&model, &out.join("copy.glb"))?;
```

## In Studio

**Import** gives the materials item first (when the file has materials), then the default scene's node tree, parents first. Everything moves from glTF's metres, +Y up, into the project's units and up axis: positions and lengths scaled, directions turned, and each placement turned so it carries no scale of units.

- **Materials** become a Material Library: a Material each, double sided as the file says, holding a MaterialX network. glTF PBR (`ND_gltf_pbr_surfaceshader`) takes every field and ratified extension in the input of the same meaning; `KHR_materials_unlit` is Surface Unlit Cutout (`kinogaki_surface_unlit`, with glTF's alpha mode and cutoff). A texture is a Kinogaki glTF texture node (`kinogaki_gltf_image`, `kinogaki_gltf_colorimage`, …, MaterialX's with glTF's minification filter and an undefined filter) holding its sampler's wraps and filters and its `KHR_texture_transform` (the rotation in degrees), a `texcoord` node wired in for a set past the first, and an Image node wired into its `file`: one Image node a distinct image and color reading in a material, a color texture read as tagged and the others as data. A channel glTF packs is read through `extract` (metallic in B, roughness in G, specular in A…), a normal texture through `normalmap` with its scale, occlusion through a `mix` with 1 at its strength, and the base color times the vertex colors through `geomcolor` where a primitive has `COLOR_0`. Colors move from linear Rec.709 into the working space.
- **Images** packed in the file (a GLB's buffer, a `data:` URI) are embedded, their bytes copied out of the file once a buffer view however many images share it (so the file's geometry isn't kept with them); images a `.gltf` names stay paths, absolute, or are read in with **Embed Textures**. Nothing is decoded: KTX2, WebP and AVIF stay embedded as they are, and a texture with both reads its PNG or JPEG source.
- **Files the file names** (buffers and images, their URIs percent-decoded first) are reached through the SDK's `Named`: in the file's folder and below, unless **Read Files Outside the Folder** is on; regular files only, never a device or a pipe; read within the read's budget. An image out of reach, or named by a URI of another scheme (`http:`), is left out and said; a buffer out of reach fails the read, saying why. A file held in memory names nothing on disk.
- **Meshes** become polygons, one value a mesh: each primitive's vertices points of their own, triangles polygons, lines two-point polylines, points alone points; a triangle naming a vertex twice is left out, said. Point attributes: `N`, `uv` (`TEXCOORD_n` is `uv<n+1>`, v turned up as MaterialX's coordinates run), `Cd` and `Alpha` (`COLOR_0`), `tangentu` and `tangentv` (`TANGENT` and its bitangent), and the file's own `_NAME` attributes; the primitive `material` names each face's Material. A mesh without normals has every edge sharp, so it shades flat as glTF says. A mesh several nodes use is instances of it, one placement where each node is: one value their items share, which Import makes Instances objects of, the mesh held once.
- **Nodes** are Parts, or the object of their mesh, camera or light. `KHR_lights_punctual`: a point light is a spot light of 180° and 1 mm radius, a spot light a spot of its outer cone softened over the share outside its inner one, a directional light a distant light; candela and lux are illuminance at a metre (a sphere's radiance is its candela over its projected area); a light's range is left out, said. Cameras: `yfov` on a 24 mm high film back, `xmag` and `ymag` as the view's size; no far distance is the farthest a camera clips.
- **Left out, and said:** animations (a file reads at rest), skins (bind pose), morph targets (applied at their default weights), `extras`, `KHR_materials_variants`, other scenes than the default.

**Export** is the inverse: each item a node (a value several items share written once; instances a child node an instance, but one placement where the node is of a mesh in its own material the node's mesh), a mesh a primitive of triangles for each material its polygons name (triangles as they are, other polygons through the display triangulation every engine shows) and one of lines; a glTF vertex is a point where every attribute written lives on points, else a point with each distinct set of corner values. `N` is written as authored on points, else as the shading normals Studio shows. glTF PBR and Surface Unlit are written as recorded; another MaterialX model is translated into glTF PBR (`standard_surface_to_gltf_pbr`, through Standard Surface from OpenPBR, folded), an engine's own material through its standard form, said. Images are written once each, PNG and JPEG as a texture's `source`, KTX2, WebP and AVIF as its extension's; another (an OpenEXR) is left out, said. A converted color keeps twelve significant digits, so one read and written back is the number it was. An area light is written as a spot light, a dome is left out, said. `.glb` holds its images and buffer; `.gltf` names its images through the SDK's `Beside` (a file on disk by its path relative to the `.gltf`, an embedded one written into `<stem>_textures/` beside it, numbered past a file of other bytes), so no file beside it is overwritten, and its buffer is `<stem>.bin`. The writer sets `asset.generator` to "Kinogaki Studio".

## The model

The `gltf` module's `Gltf` keeps glTF's own objects and their indices (scenes, nodes, meshes, materials, textures, samplers, images, cameras, lights), so a file reads and writes back structurally. It hides only the encoding: buffers, buffer views and accessors become typed arrays on the primitives, and images their files' bytes or paths. Material fields carry MaterialX's `gltf_pbr` names (`base_color`, `metallic`, `emissive_strength`, `clearcoat.roughness`), each documented with the glTF property it holds; an extension's fields sit in a struct whose `Default` is the extension absent.

## Reading

- **Files.** `.gltf` text with the files it names (paths, percent-decoded), base64 `data:` URIs, and GLB. `read` takes the bytes, a function giving a named file's bytes (the format's reaches them through the SDK's `Named`), and the read's budget; it reaches no disk itself. A buffer named by another URI scheme is refused; an image so named is kept as written (`ImageData::Elsewhere`).
- **Accessors.** Every component type, normalized or not (`u8` 255 is 1.0, as glTF defines), packed or strided, matrices with padded columns, sparse over a buffer view or over zeros; quantized attributes (`KHR_mesh_quantization`) read as their values. Values are `f32` on the attribute; values and index lists a file shares between primitives are decoded once and stay one allocation.
- **Primitives.** Points, lines and triangles: strips, fans and loops arrive as lists (a strip's joining degenerate triangles dropped), a primitive without indices as `0..count`. Attributes `POSITION`, `NORMAL`, `TANGENT`, `TEXCOORD_n`, `COLOR_n` and application ones (`_NAME`).
- **Materials.** Metallic-roughness with every ratified material extension glTF PBR carries: `KHR_materials_anisotropy`, `clearcoat`, `dispersion`, `emissive_strength`, `ior`, `iridescence`, `sheen`, `specular`, `transmission`, `unlit` and `volume`. `KHR_texture_transform` folds into the texture reference (its `texCoord` included). Textures keep their samplers (both filters, both wraps) and a `KHR_texture_basisu`, `EXT_texture_webp` or `EXT_texture_avif` source beside `source`.
- **Images.** Their files' bytes (shared by the images of one buffer view), never decoded, with their MIME type (declared, else from the bytes or the path's extension); images named by path stay paths.
- **Scene.** Nodes (a matrix or translation, rotation and scale), scenes, perspective and orthographic cameras, `KHR_lights_punctual` lights.
- **Left out, and said** (`Gltf::left_out`): animations (a file reads at rest), skins (meshes in their bind pose, `JOINTS_n` and `WEIGHTS_n` dropped), morph targets (applied at the mesh's default `weights`), `extras`, and extensions used but not read (`KHR_materials_variants`, for one). A file requiring an extension not read (`KHR_draco_mesh_compression`, `EXT_meshopt_compression`) is refused.
- **Checks.** Every index, range and count, and the node trees (one parent each, no cycles, scenes of roots each listed once). A damaged file is a `GltfError` naming the JSON path (`meshes[0].primitives[1].indices`), never a panic. The read's budget (the SDK's `ReadBudget`) bounds the JSON's depth and what decoding allocates, and its cancellation stops a read between primitives.

## Writing

- A GLB (`to_glb`), or `.gltf` text with `<stem>.bin` and the embedded images beside it, named after themselves (`to_gltf`); `write_file` chooses by the extension.
- One buffer: attributes as floats (values shared between primitives written once), `POSITION` with its bounds, indices as 16-bit or 32-bit integers. Embedded images go in the GLB's buffer, or beside the `.gltf`; images named by path stay paths.
- Members at glTF's defaults and extensions at theirs are left out. `extensionsUsed` lists every extension the written JSON names; an image extension without a `source` to fall back on is required.
- Every index in the model is checked first (`GltfError::Invalid` names where), so a written file is valid. What is written reads back as the same model, and writing that again gives the same bytes.

## Code

| File | Role |
|---|---|
| `src/lib.rs` | The package and the format: ids, options (Embed Textures, Read Files Outside the Folder), the reader and writer |
| `src/read/` | The model to standard values: `mod.rs` (the read, images), `material.rs` (glTF PBR and Surface Unlit Cutout networks), `mesh.rs` (polygons and their attributes), `scene.rs` (the node tree, lights, cameras) |
| `src/write/` | Standard values to the model: `mod.rs` (the write), `material.rs` (networks to glTF PBR, translated where needed; images, textures, samplers), `mesh.rs` (primitives and their vertices), `scene.rs` (nodes, instances, lights, cameras) |
| `src/space.rs` | glTF's metres, +Y up, and the project's space, for values and placements |
| `src/gltf/model/` | `Gltf` and its objects: `mod.rs` (scene, nodes, cameras, lights, what was left out), `mesh.rs`, `material.rs` (materials, textures, samplers, images) |
| `src/gltf/read/` | `mod.rs` (the read, top-down; buffers; images), `fields.rs` (typed JSON members with their paths), `accessor.rs` (views and accessors), `mesh.rs`, `material.rs`, `scene.rs` |
| `src/gltf/write/` | `mod.rs` (the document; image files), `binary.rs` (the buffer, its views and accessors), `material.rs`, `scene.rs` |
| `src/gltf/json/` | JSON text to a value tree (strings without escapes borrowed, scanned eight bytes at a time) and back |
| `src/gltf/base64.rs`, `uri.rs`, `glb.rs` | `data:` URIs, relative paths and image types; the GLB container |
| `plugin/` | The dylib Studio loads |

## Tests and data

`cargo test` runs one binary of tests, `tests/gltf/` (`common.rs` the samples it reads):

- `tests/gltf/format.rs`, the format on the SDK: a textured box imports its glTF PBR network with an Image node into the base color's texture (a path, packed bytes, a `data:` URI) and its mesh; Embed Textures reads the image beside into the project; every sample written as a GLB and as a `.gltf` reads back with the same materials (each texture by its image's bytes and sampler) and the same meshes where they were; a project in millimetres, Z up, holds the file where its axes put it and writes it back as it was; a mesh several nodes use is instances of one value, written once and back on each node; texture transforms and samplers are the texture nodes' values; unlit materials are Surface Unlit Cutout; lights and cameras import as standard values and write back; a File node reads one value; damaged files are errors and a cancelled read stops;
- `tests/gltf/samples.rs`: every sample reads, saying what it leaves out; sparse accessors, primitive modes, morph targets at default weights, interleaved views, samplers, texture transforms, cameras, lights and every material extension read as the samples' READMEs describe;
- `tests/gltf/round_trip.rs`: every sample, and a model setting every field, written as a GLB and as a `.gltf` reads back as the same model; writing is byte-for-byte repeatable; a model that can't be written is refused naming where;
- `tests/gltf/accessors.rs`: component types, normalization, strides, padded matrix columns and sparse accessors on hand-made buffers;
- `tests/gltf/robust.rs`: every cut of the small samples is an error; 6,400 mutations (digits, large numbers, flipped bits) read or fail without panicking; structural faults name their place; budgets and cancellation;
- `tests/gltf/hostile.rs`: the hostile files that once asked for far more than they hold, read as Import reads (a 2 MiB stack and a budget): a view-less accessor shared by 429 primitives, one of gigabytes, a scene listing a root twice, buffers and images out of the folder however written (`%2Fetc%2Fhosts`, `..`), pipes, images in one buffer view, 90,000 attributes, normals of 8,000 primitives; a `.gltf` exported beside other files overwrites none.

Three tests run by hand, with outside data (Studio's `tools/tests/outside.toml`; its `tools/fetch_data.py` brings it to `~/.kinogaki/data`): `hand_run_fetched_samples_read_and_write_back_the_same` reads the samples in `$GLTF_SAMPLES`, and `hand_run_the_model_setting_every_field_passes_the_validator` and `hand_run_every_sample_written_through_the_format_passes_the_validator` check what they write with the glTF-Validator in `$GLTF_VALIDATOR`. Studio's tree imports every sample through the editor, exports and reads them again, and renders an imported textured asset (its `workflows/`).

**Validation.** The Khronos [glTF-Validator](https://github.com/KhronosGroup/glTF-Validator), its macOS program from release 2.0.0-dev.3.10 (`~/.kinogaki/data/gltf-validator/gltf_validator`), never a dependency, checks what the writer writes: the hand-run tests write the model setting every field and each sample through Studio's values and validate them, and `examples/rewrite.rs` writes each sample back as a GLB and a `.gltf` from the model for `gltf_validator --messages DIR` to check. Every written file has no errors; the only warnings (normal maps on primitives without tangents) are the source files' own. That release doesn't know `KHR_texture_basisu`, so the model written for it has no KTX2 image.

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

Two samples are fetched rather than committed, for the hand-run test: DamagedHelmet (CC-BY-4.0 with a CC-BY-NC-4.0 part) and Sponza (the CRYENGINE agreement). Studio's `tools/fetch_data.py` fetches them from the same commit into `~/.kinogaki/data/gltf-samples`.

## Performance

`cargo run --release -p kinogaki-gltf --example bench` in Studio's tree, Apple M4 Max under other sessions' load (load average 30 to 55), the first of seven runs and the best. The budgets are for a file into Studio's standard values and back; the model's share is the bytes to the model and back.

| | First | Best | The model's share, best | Budget |
|---|---|---|---|---|
| A 1M-triangle GLB (positions, normals, UVs; 26 MB) read from its file into standard values | 13.7 ms | 11.5 ms | 3.1 ms | 40 ms |
| The same as a `.gltf` of a base64 `data:` URI (35 MB) | 27.5 ms | 22.8 ms | 13.3 ms | 80 ms |
| A GLB of 200 MB of images (copied, not decoded) | 16.6 ms | 15.1 ms | 3.2 ms | 30 ms |
| The 1M-triangle polygons written to a GLB file | 21.5 ms | 19.7 ms | 3.3 ms | 60 ms |

A first run under the heaviest load read the GLB in up to 57 ms; the best stays at 11.5 ms.

## Building

The format builds from this repository and the public SDK's alone, checked out side by side as Studio's tree lays them out (`sdk/` and `plugins/gltf/`), with the compiler the SDK pins (`sdk/rust-toolchain.toml`; 1.98.1):

```sh
git clone https://github.com/kinogaki/studio-sdk sdk
git clone https://github.com/kinogaki/plugin-gltf plugins/gltf
cd plugins/gltf
cargo test
```

`plugin/` is the library Studio loads. It links Studio's shared library (`kinogaki_shared`), so it is built in Studio's tree, or against a Studio release's plugin kit as the SDK's guides describe.

## License

Dual-licensed under the MIT license ([LICENSE-MIT](LICENSE-MIT)) and the Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option. The test data keeps its own licences, above.
