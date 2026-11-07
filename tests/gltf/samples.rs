//! The Khronos sample assets read as their READMEs describe them.

use crate::common::{read, read_bytes, sample, samples};
use kinogaki_gltf::gltf::{
    self, AlphaMode, ImageData, Kind, LeftOut, LightKind, MagFilter, MinFilter, Projection,
    ReadOptions, TextureTransform, Topology, Wrap,
};

#[test]
fn every_sample_reads_saying_what_it_leaves_out() {
    for path in samples() {
        let model = gltf::read_file(&path, &ReadOptions::default())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let name = path.file_stem().unwrap().to_string_lossy();
        let expected = match name.as_ref() {
            "SheenChair" => vec![LeftOut::Extension("KHR_materials_variants".into())],
            "TransmissionTest" => vec![LeftOut::Extension("KHR_xmp".into())],
            "SimpleMorph" => vec![LeftOut::MorphTargets(1), LeftOut::Animations(1)],
            "SimpleSkin" => vec![LeftOut::Animations(1), LeftOut::Skins(1)],
            _ => vec![],
        };
        assert_eq!(model.left_out, expected, "{name}");
        assert!(
            !model.meshes.is_empty() && !model.scenes.is_empty(),
            "{name}"
        );
        for mesh in &model.meshes {
            for primitive in &mesh.primitives {
                let count = primitive.vertex_count();
                assert!(
                    primitive.indices.iter().all(|&i| (i as usize) < count),
                    "{name}"
                );
                assert_eq!(
                    primitive.indices.len() % primitive.topology.corners(),
                    0,
                    "{name}"
                );
                assert!(
                    primitive
                        .attribute("POSITION")
                        .is_some_and(|p| p.kind == Kind::Vec3),
                    "{name}"
                );
            }
        }
    }
}

#[test]
fn a_glb_or_a_gltf_of_data_uris_reads_from_memory_alone() {
    for (model, variant) in [
        ("BoxTextured", "glTF-Binary"),
        ("BoxTextured", "glTF-Embedded"),
        ("Cameras", "glTF-Embedded"),
    ] {
        let bytes = std::fs::read(sample(model, variant)).unwrap();
        assert_eq!(
            read_bytes(&bytes).unwrap(),
            read(model, variant),
            "{model} {variant}"
        );
    }
    let bytes = std::fs::read(sample("BoxTextured", "glTF")).unwrap();
    let error = read_bytes(&bytes).unwrap_err();
    assert!(
        matches!(&error, gltf::GltfError::Io { file, .. } if file == "BoxTextured0.bin"),
        "{error}"
    );
}

#[test]
fn images_are_kept_as_their_files_bytes_or_paths() {
    let beside = gltf::read_file(&sample("BoxTextured", "glTF"), &ReadOptions::default()).unwrap();
    assert_eq!(
        beside.images[0].data,
        ImageData::Path("CesiumLogoFlat.png".into())
    );
    assert_eq!(beside.images[0].mime_type.as_deref(), Some("image/png"));
    let png =
        std::fs::read(sample("BoxTextured", "glTF").with_file_name("CesiumLogoFlat.png")).unwrap();
    for variant in ["glTF", "glTF-Binary", "glTF-Embedded"] {
        let image = &read("BoxTextured", variant).images[0];
        assert_eq!(image.data, ImageData::Embedded(png.clone()), "{variant}");
        assert_eq!(image.mime_type.as_deref(), Some("image/png"), "{variant}");
    }
}

#[test]
fn a_sparse_accessor_replaces_its_elements() {
    let model = read("SimpleSparseAccessor", "glTF-Embedded");
    let positions = &model.meshes[0].primitives[0]
        .attribute("POSITION")
        .unwrap()
        .values;
    let mut expected: Vec<f32> = (0..7).flat_map(|x| [x as f32, 0.0, 0.0]).collect();
    expected.extend((0..7).flat_map(|x| [x as f32, 1.0, 0.0]));
    for (vertex, y) in [(8, 2.0), (10, 3.0), (12, 4.0)] {
        expected[vertex * 3 + 1] = y;
    }
    assert_eq!(&positions[..], &expected[..]);
}

#[test]
fn strips_fans_and_loops_read_as_lists() {
    let model = read("MeshPrimitiveModes", "glTF-Embedded");
    let read: Vec<(Topology, usize)> = model
        .meshes
        .iter()
        .map(|m| (m.primitives[0].topology, m.primitives[0].indices.len()))
        .collect();
    use Topology::*;
    // Points 7, lines 12, a loop of 7, a strip of 7, triangles 18, a strip
    // of 6 and a fan of 8 corners.
    assert_eq!(
        read,
        [
            (Points, 7),
            (Lines, 12),
            (Lines, 14),
            (Lines, 12),
            (Triangles, 18),
            (Triangles, 12),
            (Triangles, 18)
        ]
    );
}

#[test]
fn morph_targets_apply_at_the_default_weights() {
    let model = read("SimpleMorph", "glTF-Embedded");
    let positions = &model.meshes[0].primitives[0]
        .attribute("POSITION")
        .unwrap()
        .values;
    assert_eq!(
        &positions[..],
        &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.5, 1.5, 0.0]
    );
}

#[test]
fn a_primitive_without_indices_reads_its_vertices_in_order() {
    let model = read("TriangleWithoutIndices", "glTF-Embedded");
    assert_eq!(model.meshes[0].primitives[0].indices, [0, 1, 2]);
}

#[test]
fn interleaved_attributes_read_at_their_stride() {
    let primitive = &read("BoxInterleaved", "glTF-Binary").meshes[0].primitives[0];
    let positions = &primitive.attribute("POSITION").unwrap().values;
    let normals = &primitive.attribute("NORMAL").unwrap().values;
    assert_eq!((positions.len(), normals.len()), (72, 72));
    assert!(positions.iter().all(|p| p.abs() == 0.5));
    for normal in normals.as_chunks::<3>().0 {
        assert_eq!(normal.iter().map(|n| n * n).sum::<f32>(), 1.0);
    }
}

#[test]
fn samplers_keep_their_filters_and_wraps() {
    let model = read("TextureSettingsTest", "glTF-Binary");
    let wraps: Vec<(Wrap, Wrap)> = model
        .samplers
        .iter()
        .map(|s| (s.wrap_s, s.wrap_t))
        .collect();
    use Wrap::*;
    assert_eq!(
        wraps,
        [
            (Repeat, Repeat),
            (Repeat, ClampToEdge),
            (ClampToEdge, Repeat),
            (Repeat, MirroredRepeat),
            (MirroredRepeat, Repeat)
        ]
    );
    for sampler in &model.samplers {
        assert_eq!(
            (sampler.mag_filter, sampler.min_filter),
            (
                Some(MagFilter::Linear),
                Some(MinFilter::NearestMipmapLinear)
            )
        );
    }
    assert_eq!(model.samplers[1].name, "RepeatSClampTSampler");
}

#[test]
fn texture_transforms_fold_into_their_references() {
    let model = read("TextureTransformTest", "glTF");
    let transform = |m: usize| {
        model.materials[m]
            .base_color_texture
            .as_ref()
            .unwrap()
            .transform
    };
    let at = |offset, rotation, scale| TextureTransform {
        offset,
        rotation,
        scale,
    };
    assert_eq!(transform(0), at([0.5, 0.0], 0.0, [1.0, 1.0]));
    assert_eq!(
        transform(3),
        at([0.0, 0.0], std::f64::consts::FRAC_PI_8, [1.0, 1.0])
    );
    assert_eq!(transform(5), at([-0.2, -0.1], 0.3, [1.5, 1.5]));
    assert_eq!(transform(6), TextureTransform::default());
    let multi = read("MultiUVTest", "glTF-Binary");
    let sets: Vec<u32> = (multi.materials[0]
        .emissive_texture
        .iter()
        .chain(&multi.materials[0].base_color_texture))
    .map(|r| r.tex_coord)
    .collect();
    assert!(sets.contains(&1), "{sets:?}");
}

#[test]
fn cameras_and_lights_read_with_their_nodes() {
    let model = read("Cameras", "glTF-Embedded");
    assert_eq!(
        model.cameras[0].projection,
        Projection::Perspective {
            yfov: 0.7,
            aspect_ratio: Some(1.0),
            znear: 0.01,
            zfar: Some(100.0)
        }
    );
    assert_eq!(
        model.cameras[1].projection,
        Projection::Orthographic {
            xmag: 1.0,
            ymag: 1.0,
            znear: 0.01,
            zfar: 100.0
        }
    );
    assert_eq!(model.nodes.iter().filter(|n| n.camera.is_some()).count(), 2);
    let lamp = read("LightsPunctualLamp", "glTF-Binary");
    assert_eq!(lamp.lights.len(), 5);
    assert!(
        lamp.lights
            .iter()
            .all(|l| l.kind == LightKind::Point && l.range.is_none())
    );
    assert_eq!(
        (lamp.lights[0].name.as_str(), lamp.lights[0].intensity),
        ("Point", 15.0)
    );
    assert_eq!(
        lamp.lights[0].color,
        [1.0, 0.6318749785423279, 0.23909975588321689]
    );
    assert_eq!(lamp.lights[1].color, [1.0; 3]);
    assert_eq!(lamp.nodes.iter().filter(|n| n.light.is_some()).count(), 5);
}

#[test]
fn every_ratified_material_extension_reads() {
    let any = |model: &str, test: &dyn Fn(&gltf::Material) -> bool| {
        assert!(
            read(model, "glTF-Binary").materials.iter().any(test),
            "{model}"
        );
    };
    any("ClearCoatTest", &|m| {
        m.clearcoat.factor > 0.0 && m.clearcoat.normal_texture.is_some()
    });
    any("ClearCoatTest", &|m| {
        m.clearcoat.roughness_texture.is_some()
    });
    any("TransmissionTest", &|m| {
        m.transmission.factor > 0.0 && m.transmission.texture.is_some()
    });
    any("AttenuationTest", &|m| {
        m.volume.attenuation_distance.is_some() && m.volume.thickness > 0.0
    });
    any("AttenuationTest", &|m| {
        m.volume.attenuation_color != [1.0; 3]
    });
    any("SpecularTest", &|m| m.specular.factor != 1.0);
    any("SpecularTest", &|m| m.specular.color != [1.0; 3]);
    any("SheenChair", &|m| {
        m.sheen.color != [0.0; 3] && m.sheen.roughness > 0.0
    });
    any("IridescenceLamp", &|m| {
        m.iridescence.factor > 0.0 && m.iridescence.thickness_texture.is_some()
    });
    any("AnisotropyBarnLamp", &|m| {
        m.anisotropy.strength > 0.0 && m.anisotropy.texture.is_some()
    });
    any("DispersionTest", &|m| m.dispersion > 0.0 && m.ior != 1.5);
    any("UnlitTest", &|m| m.unlit);
    let strengths: Vec<f64> = read("EmissiveStrengthTest", "glTF-Binary")
        .materials
        .iter()
        .map(|m| m.emissive_strength)
        .collect();
    assert_eq!(strengths, [4.0, 1.0, 2.0, 1.0, 8.0, 16.0]);
    let modes = read("AlphaBlendModeTest", "glTF-Binary").materials;
    assert!(modes.iter().any(|m| m.alpha_mode == AlphaMode::Blend));
    assert!(
        modes
            .iter()
            .any(|m| m.alpha_mode == AlphaMode::Mask && m.alpha_cutoff != 0.5)
    );
    assert!(
        read("NormalTangentTest", "glTF-Binary").materials[0]
            .normal_texture
            .is_some()
    );
    let colors = read("VertexColorTest", "glTF-Binary");
    assert_eq!(
        colors.meshes[1].primitives[0]
            .attribute("COLOR_0")
            .unwrap()
            .kind,
        Kind::Vec4
    );
}
