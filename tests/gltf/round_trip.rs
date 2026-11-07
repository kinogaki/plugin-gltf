//! What is written reads back as the same model: every sample (so its
//! materials are lossless), a model setting every field, and the writer's
//! own output byte for byte.

use crate::common::{read, read_bytes, samples, written_and_read};
use kinogaki_gltf::gltf::{
    self, AlphaMode, Anisotropy, Attribute, Camera, Clearcoat, Gltf, GltfError, Image, ImageData,
    Iridescence, Kind, Light, LightKind, MagFilter, Material, Mesh, MinFilter, Node, Primitive,
    Projection, Sampler, Scene, Sheen, Specular, Texture, TextureRef, TextureTransform, Topology,
    Transform, Transmission, Volume, Wrap,
};
use std::sync::Arc;

#[test]
fn every_sample_reads_back_the_same_from_a_glb_and_a_gltf() {
    for path in samples() {
        let variant = path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut model = read(&path.file_stem().unwrap().to_string_lossy(), &variant);
        model.left_out.clear();
        for back in written_and_read(&model) {
            assert_eq!(back, model, "{}", path.display());
        }
    }
}

#[test]
fn a_model_setting_every_field_reads_back_the_same() {
    let model = everything();
    for back in written_and_read(&model) {
        assert_eq!(back, model);
    }
}

#[test]
fn writing_what_was_read_gives_the_same_bytes() {
    for model in [
        everything(),
        read("SheenChair", "glTF-Binary"),
        read("TextureTransformTest", "glTF"),
    ] {
        let glb = gltf::to_glb(&model).unwrap();
        assert_eq!(gltf::to_glb(&read_bytes(&glb).unwrap()).unwrap(), glb);
    }
}

#[test]
fn values_shared_between_primitives_are_written_once() {
    let model = everything();
    let positions = &model.meshes[0].primitives[0]
        .attribute("POSITION")
        .unwrap()
        .values;
    assert!(Arc::ptr_eq(
        positions,
        &model.meshes[0].primitives[1]
            .attribute("POSITION")
            .unwrap()
            .values
    ));
    let back = read_bytes(&gltf::to_glb(&model).unwrap()).unwrap();
    let read_back = |p: usize| {
        back.meshes[0].primitives[p]
            .attribute("POSITION")
            .unwrap()
            .values
            .clone()
    };
    assert!(Arc::ptr_eq(&read_back(0), &read_back(1)));
}

#[test]
fn a_model_that_cant_be_written_is_refused_naming_where() {
    let refused = |change: &dyn Fn(&mut Gltf), at: &str| {
        let mut model = everything();
        change(&mut model);
        match gltf::to_glb(&model) {
            Err(GltfError::Invalid { at: where_, .. }) => assert_eq!(where_, at),
            other => panic!("{at}: {:?}", other.map(|glb| glb.len())),
        }
    };
    refused(
        &|m| m.meshes[0].primitives[0].indices[0] = 99,
        "meshes[0].primitives[0]",
    );
    refused(
        &|m| m.meshes[0].primitives[1].indices.push(0),
        "meshes[0].primitives[1]",
    );
    refused(
        &|m| m.materials[0].emissive_texture.as_mut().unwrap().texture = 9,
        "materials[0].emissiveTexture",
    );
    refused(&|m| m.textures[0].sampler = Some(5), "textures[0].sampler");
    refused(&|m| m.textures[1].extension_source = Some(0), "textures[1]");
    refused(&|m| m.nodes[0].children.push(7), "nodes[0].children");
    refused(&|m| m.scene = Some(3), "scene");
    refused(&|m| m.lights[0].intensity = f64::INFINITY, "extensions");
    refused(
        &|m| {
            m.images[0].mime_type = None;
            m.images[0].data = ImageData::Embedded(b"no signature".to_vec());
        },
        "images[0]",
    );
}

/// Writes the model setting every field into `$GLTF_OUT` (a GLB and a
/// `.gltf` with its files), for the glTF-Validator (`tools/validate.mjs`).
/// Its content is made one the validator finds sound: real PNGs, no KTX2
/// (which the validator doesn't read), no volume on a double-sided
/// material, every light used, no textured lines.
#[test]
#[ignore = "hand: writes files for the local validator"]
fn hand_run_write_the_model_setting_every_field_for_the_validator() {
    const PNG: [u8; 70] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0xdf, 0xe0, 0xf0, 0x1f, 0x00, 0x07, 0x00, 0x02, 0xbf, 0x2b, 0xd7, 0xc7, 0xe2, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let mut model = everything();
    for image in &mut model.images {
        (image.mime_type, image.data) =
            (Some("image/png".into()), ImageData::Embedded(PNG.to_vec()));
    }
    model.textures[1].extension_source = None;
    model.textures[2] = Texture {
        source: Some(2),
        extension_source: None,
        ..model.textures[2].clone()
    };
    model.materials[0].double_sided = false;
    model.meshes[0].primitives[1].material = None;
    model.nodes[1].light = Some(1);
    let out = std::path::PathBuf::from(std::env::var("GLTF_OUT").expect("GLTF_OUT, a directory"));
    std::fs::create_dir_all(&out).unwrap();
    for name in ["everything.glb", "everything.gltf"] {
        gltf::write_file(&model, &out.join(name)).unwrap();
    }
}

/// A texture reference to `texture` on set 0.
fn texture(texture: usize) -> Option<TextureRef> {
    Some(TextureRef {
        texture,
        tex_coord: 0,
        transform: TextureTransform::default(),
    })
}

/// A model whose every field is away from its default.
fn everything() -> Gltf {
    let positions: Arc<[f32]> = Arc::from(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    let attribute = |name: &str, kind: Kind, values: Vec<f32>| Attribute {
        name: name.into(),
        kind,
        values: values.into(),
    };
    let position = Attribute {
        name: "POSITION".into(),
        kind: Kind::Vec3,
        values: positions,
    };
    let triangle = Primitive {
        topology: Topology::Triangles,
        indices: vec![0, 1, 2],
        attributes: vec![
            position.clone(),
            attribute("NORMAL", Kind::Vec3, [0.0, 0.0, 1.0].repeat(3)),
            attribute("TANGENT", Kind::Vec4, [1.0, 0.0, 0.0, -1.0].repeat(3)),
            attribute("TEXCOORD_0", Kind::Vec2, vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
            attribute("TEXCOORD_1", Kind::Vec2, vec![0.5; 6]),
            attribute("COLOR_0", Kind::Vec4, [1.0, 0.5, 0.25, 1.0].repeat(3)),
            attribute("_TEMPERATURE", Kind::Scalar, vec![20.0, 21.5, -3.0]),
            attribute("_FRAME", Kind::Mat2, [1.0, 0.0, 0.0, 1.0].repeat(3)),
        ],
        material: Some(0),
    };
    let lines = Primitive {
        topology: Topology::Lines,
        indices: vec![0, 1, 1, 2],
        attributes: vec![position.clone()],
        material: Some(1),
    };
    let points = Primitive {
        topology: Topology::Points,
        indices: vec![2, 0],
        attributes: vec![position],
        material: None,
    };
    let transform = TextureTransform {
        offset: [0.25, -0.5],
        rotation: 0.3,
        scale: [2.0, 3.0],
    };
    let reference = |texture: usize, tex_coord: u32, transform| {
        Some(TextureRef {
            texture,
            tex_coord,
            transform,
        })
    };
    let material = Material {
        name: "Every Field".into(),
        base_color: [0.8, 0.2, 0.1, 0.5],
        base_color_texture: reference(0, 1, transform),
        metallic: 0.25,
        roughness: 0.75,
        metallic_roughness_texture: texture(1),
        normal_texture: texture(0),
        normal_scale: 0.5,
        occlusion_texture: texture(1),
        occlusion_strength: 0.3,
        emissive: [1.0, 0.5, 0.0],
        emissive_texture: texture(2),
        emissive_strength: 5.0,
        alpha_mode: AlphaMode::Mask,
        alpha_cutoff: 0.25,
        double_sided: true,
        unlit: false,
        ior: 1.33,
        dispersion: 0.1,
        transmission: Transmission {
            factor: 0.9,
            texture: texture(0),
        },
        volume: Volume {
            thickness: 0.2,
            thickness_texture: texture(1),
            attenuation_distance: Some(0.5),
            attenuation_color: [0.9, 0.5, 0.1],
        },
        clearcoat: Clearcoat {
            factor: 1.0,
            texture: texture(0),
            roughness: 0.1,
            roughness_texture: texture(1),
            normal_texture: texture(0),
            normal_scale: 0.7,
        },
        specular: Specular {
            factor: 0.5,
            texture: texture(0),
            color: [0.9, 0.9, 1.0],
            color_texture: texture(1),
        },
        sheen: Sheen {
            color: [0.2, 0.1, 0.3],
            color_texture: texture(0),
            roughness: 0.4,
            roughness_texture: texture(1),
        },
        iridescence: Iridescence {
            factor: 1.0,
            texture: texture(0),
            ior: 1.8,
            thickness_minimum: 200.0,
            thickness_maximum: 800.0,
            thickness_texture: texture(1),
        },
        anisotropy: Anisotropy {
            strength: 0.6,
            rotation: 1.2,
            texture: texture(1),
        },
    };
    let unlit = Material {
        name: "Unlit".into(),
        base_color_texture: reference(
            2,
            0,
            TextureTransform {
                rotation: 0.0,
                ..transform
            },
        ),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..Material::default()
    };
    let png = b"\x89PNG\r\n\x1a\n not decoded".to_vec();
    let ktx2 = b"\xabKTX 20\xbb\r\n\x1a\n not decoded".to_vec();
    let image = |name: &str, mime: &str, data: ImageData| Image {
        name: name.into(),
        mime_type: Some(mime.into()),
        data,
    };
    Gltf {
        asset: gltf::Asset {
            generator: "Kinogaki Studio".into(),
            copyright: "2026 Kinogaki".into(),
        },
        scene: Some(1),
        scenes: vec![
            Scene {
                name: "Empty".into(),
                nodes: vec![],
            },
            Scene {
                name: "Main".into(),
                nodes: vec![0, 2],
            },
        ],
        nodes: vec![
            Node {
                name: "Root".into(),
                transform: Transform::Trs {
                    translation: [1.0, 2.0, 3.0],
                    rotation: [0.0, 0.6, 0.0, 0.8],
                    scale: [2.0; 3],
                },
                children: vec![1, 3],
                mesh: Some(0),
                ..Node::default()
            },
            Node {
                name: "Camera".into(),
                camera: Some(0),
                ..Node::default()
            },
            Node {
                name: "Lamp".into(),
                transform: Transform::Matrix([
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 5.0, 6.0, 7.0, 1.0,
                ]),
                light: Some(2),
                camera: Some(1),
                ..Node::default()
            },
            Node {
                light: Some(0),
                ..Node::default()
            },
        ],
        meshes: vec![Mesh {
            name: "Shapes".into(),
            primitives: vec![triangle, lines, points],
        }],
        materials: vec![material, unlit],
        textures: vec![
            Texture {
                name: "Albedo".into(),
                sampler: Some(0),
                source: Some(0),
                extension_source: None,
            },
            Texture {
                name: String::new(),
                sampler: None,
                source: Some(1),
                extension_source: Some(2),
            },
            Texture {
                name: "Compressed".into(),
                sampler: Some(1),
                source: None,
                extension_source: Some(2),
            },
        ],
        samplers: vec![
            Sampler {
                name: "Pixels".into(),
                mag_filter: Some(MagFilter::Nearest),
                min_filter: Some(MinFilter::LinearMipmapNearest),
                wrap_s: Wrap::ClampToEdge,
                wrap_t: Wrap::MirroredRepeat,
            },
            Sampler::default(),
        ],
        images: vec![
            image("albedo", "image/png", ImageData::Embedded(png.clone())),
            image(
                "",
                "image/jpeg",
                ImageData::Path("textures/rough ness.jpg".into()),
            ),
            image("compressed", "image/ktx2", ImageData::Embedded(ktx2)),
        ],
        cameras: vec![
            Camera {
                name: "Lens".into(),
                projection: Projection::Perspective {
                    yfov: 0.6,
                    aspect_ratio: Some(1.5),
                    znear: 0.1,
                    zfar: None,
                },
            },
            Camera {
                name: String::new(),
                projection: Projection::Orthographic {
                    xmag: 2.0,
                    ymag: 1.0,
                    znear: 0.0,
                    zfar: 50.0,
                },
            },
        ],
        lights: vec![
            Light {
                name: "Sun".into(),
                kind: LightKind::Directional,
                color: [1.0, 0.9, 0.8],
                intensity: 3.0,
                range: None,
            },
            Light {
                name: "Bulb".into(),
                kind: LightKind::Point,
                color: [1.0; 3],
                intensity: 1.0,
                range: Some(10.0),
            },
            Light {
                name: "Spot".into(),
                kind: LightKind::Spot {
                    inner_cone_angle: 0.2,
                    outer_cone_angle: 0.5,
                },
                color: [0.5, 0.5, 1.0],
                intensity: 200.0,
                range: None,
            },
        ],
        left_out: vec![],
    }
}
