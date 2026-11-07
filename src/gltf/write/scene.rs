//! Meshes, nodes, scenes, cameras and lights as glTF JSON.
use super::binary::Binary;
use super::check;
use crate::gltf::GltfError;
use crate::gltf::json::{Members, Value};
use crate::gltf::model::{
    Camera, Gltf, Light, LightKind, Mesh, Node, Projection, Scene, Topology, Transform,
};

use std::f64::consts::FRAC_PI_4;

type Result<T> = std::result::Result<T, GltfError>;

/// A mesh, its attributes and indices added to `binary`.
pub(super) fn mesh(
    mesh: &Mesh,
    at: &str,
    materials: usize,
    binary: &mut Binary,
) -> Result<Value<'static>> {
    if mesh.primitives.is_empty() {
        return Err(GltfError::invalid(at, "a mesh without primitives"));
    }
    let mut primitives = Vec::with_capacity(mesh.primitives.len());
    for (i, primitive) in mesh.primitives.iter().enumerate() {
        let at = format!("{at}.primitives[{i}]");
        let count = primitive.vertex_count();
        if let Some(odd) = primitive
            .attributes
            .iter()
            .find(|a| a.count() != count || a.values.len() % a.kind.width() != 0)
        {
            return Err(GltfError::invalid(
                &at,
                format!("{} doesn't have {count} whole vertices", odd.name),
            ));
        }
        let corners = primitive.topology.corners();
        if primitive.indices.is_empty() || primitive.indices.len() % corners != 0 {
            return Err(GltfError::invalid(
                &at,
                format!(
                    "{} corners don't make whole elements",
                    primitive.indices.len()
                ),
            ));
        }
        if let Some(past) = primitive
            .indices
            .iter()
            .copied()
            .max()
            .filter(|&max| max as usize >= count)
        {
            return Err(GltfError::invalid(
                &at,
                format!("index {past} is past {count} vertices"),
            ));
        }
        if let Some(material) = primitive.material {
            check(&format!("{at}.material"), material, materials)?;
        }
        let mut attributes = Members::new();
        for attribute in &primitive.attributes {
            let accessor = binary.attribute(
                &attribute.values,
                attribute.kind,
                attribute.name == "POSITION",
            );
            attributes = attributes.with_key(attribute.name.clone(), accessor);
        }
        let mode = match primitive.topology {
            Topology::Points => 0u32,
            Topology::Lines => 1,
            Topology::Triangles => 4,
        };
        primitives.push(
            Members::new()
                .with("attributes", attributes)
                .with("indices", binary.indices(&primitive.indices, count))
                .with_some("material", primitive.material)
                .with_changed("mode", mode, 4),
        );
    }
    Ok(Members::new()
        .with_text("name", &mesh.name)
        .with("primitives", primitives)
        .into())
}

pub(super) fn node(node: &Node, at: &str, gltf: &Gltf) -> Result<Value<'static>> {
    for (key, index, len) in [
        ("mesh", node.mesh, gltf.meshes.len()),
        ("camera", node.camera, gltf.cameras.len()),
        ("light", node.light, gltf.lights.len()),
    ] {
        if let Some(index) = index {
            check(&format!("{at}.{key}"), index, len)?;
        }
    }
    for &child in &node.children {
        check(&format!("{at}.children"), child, gltf.nodes.len())?;
    }
    let mut members = Members::new()
        .with_text("name", &node.name)
        .with_if(!node.children.is_empty(), "children", node.children.clone())
        .with_some("mesh", node.mesh)
        .with_some("camera", node.camera);
    members = match &node.transform {
        Transform::Matrix(matrix) => members.with("matrix", *matrix),
        &Transform::Trs {
            translation,
            rotation,
            scale,
        } => members
            .with_changed("translation", translation, [0.0; 3])
            .with_changed("rotation", rotation, [0.0, 0.0, 0.0, 1.0])
            .with_changed("scale", scale, [1.0; 3]),
    };
    let light = node.light.map(|light| {
        Members::new().with("KHR_lights_punctual", Members::new().with("light", light))
    });
    Ok(members.with_some("extensions", light).into())
}

pub(super) fn scene(scene: &Scene, at: &str, nodes: usize) -> Result<Value<'static>> {
    for &node in &scene.nodes {
        check(&format!("{at}.nodes"), node, nodes)?;
    }
    Ok(Members::new()
        .with_text("name", &scene.name)
        .with_if(!scene.nodes.is_empty(), "nodes", scene.nodes.clone())
        .into())
}

pub(super) fn camera(camera: &Camera) -> Value<'static> {
    let (kind, projection) = match camera.projection {
        Projection::Perspective {
            yfov,
            aspect_ratio,
            znear,
            zfar,
        } => (
            "perspective",
            Members::new()
                .with_some("aspectRatio", aspect_ratio)
                .with("yfov", yfov)
                .with_some("zfar", zfar)
                .with("znear", znear),
        ),
        Projection::Orthographic {
            xmag,
            ymag,
            znear,
            zfar,
        } => (
            "orthographic",
            Members::new()
                .with("xmag", xmag)
                .with("ymag", ymag)
                .with("zfar", zfar)
                .with("znear", znear),
        ),
    };
    Members::new()
        .with_text("name", &camera.name)
        .with("type", kind)
        .with(kind, projection)
        .into()
}

pub(super) fn light(light: &Light) -> Value<'static> {
    let (kind, spot) = match light.kind {
        LightKind::Directional => ("directional", None),
        LightKind::Point => ("point", None),
        LightKind::Spot {
            inner_cone_angle,
            outer_cone_angle,
        } => (
            "spot",
            Some(
                Members::new()
                    .with_changed("innerConeAngle", inner_cone_angle, 0.0)
                    .with_changed("outerConeAngle", outer_cone_angle, FRAC_PI_4),
            ),
        ),
    };
    Members::new()
        .with_text("name", &light.name)
        .with_changed("color", light.color, [1.0; 3])
        .with_changed("intensity", light.intensity, 1.0)
        .with_some("range", light.range)
        .with("type", kind)
        .with_some("spot", spot)
        .into()
}
