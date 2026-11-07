//! Nodes, scenes, cameras and `KHR_lights_punctual` lights.
use super::Result;
use super::fields::Obj;
use crate::gltf::GltfError;
use crate::gltf::model::{Camera, Light, LightKind, Node, Projection, Scene, Transform};

/// What nodes may name.
pub(super) struct Counts {
    pub nodes: usize,
    pub meshes: usize,
    pub cameras: usize,
    pub lights: usize,
}

pub(super) fn node(node: &Obj<'_, '_>, counts: &Counts) -> Result<Node> {
    let transform = match node.opt_numbers::<16>("matrix")? {
        Some(matrix) => {
            if ["translation", "rotation", "scale"]
                .iter()
                .any(|key| node.has(key))
            {
                return Err(node.error("matrix", "given beside translation, rotation or scale"));
            }
            Transform::Matrix(matrix)
        }
        None => Transform::Trs {
            translation: node.numbers("translation", [0.0; 3])?,
            rotation: node.numbers("rotation", [0.0, 0.0, 0.0, 1.0])?,
            scale: node.numbers("scale", [1.0; 3])?,
        },
    };
    let light = match node.extension("KHR_lights_punctual")? {
        Some(light) => Some(light.required_index("light", counts.lights)?),
        None => None,
    };
    Ok(Node {
        name: node.text("name")?,
        transform,
        children: node.indices("children", counts.nodes)?,
        mesh: node.index("mesh", counts.meshes)?,
        camera: node.index("camera", counts.cameras)?,
        light,
    })
}

pub(super) fn scene(scene: &Obj<'_, '_>, nodes: usize) -> Result<Scene> {
    Ok(Scene {
        name: scene.text("name")?,
        nodes: scene.indices("nodes", nodes)?,
    })
}

/// Nodes form trees: none has two parents or is its own ancestor, and a
/// scene's nodes are roots, each listed once.
pub(super) fn check_trees(nodes: &[Node], scenes: &[Scene]) -> Result<()> {
    let mut parent = vec![None; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        for &child in &node.children {
            if parent[child].replace(index).is_some() {
                return Err(GltfError::malformed(
                    format!("nodes[{index}].children"),
                    format!("node {child} has two parents"),
                ));
            }
        }
    }
    let mut reached = vec![false; nodes.len()];
    let mut stack: Vec<usize> = (0..nodes.len()).filter(|&n| parent[n].is_none()).collect();
    while let Some(node) = stack.pop() {
        reached[node] = true;
        stack.extend(&nodes[node].children);
    }
    if let Some(cycle) = reached.iter().position(|reached| !reached) {
        return Err(GltfError::malformed(
            format!("nodes[{cycle}]"),
            "the node is its own ancestor",
        ));
    }
    let mut listed = vec![usize::MAX; nodes.len()];
    for (index, scene) in scenes.iter().enumerate() {
        let at = || format!("scenes[{index}].nodes");
        if let Some(&child) = scene.nodes.iter().find(|&&n| parent[n].is_some()) {
            return Err(GltfError::malformed(
                at(),
                format!("node {child} isn't a root"),
            ));
        }
        // Each root once: a scene listing one again would show its tree twice.
        for &root in &scene.nodes {
            if std::mem::replace(&mut listed[root], index) == index {
                return Err(GltfError::malformed(
                    at(),
                    format!("node {root} is listed twice"),
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn camera(camera: &Obj<'_, '_>) -> Result<Camera> {
    let projection = match camera.opt_str("type")? {
        Some("perspective") => {
            let p = camera
                .object("perspective")?
                .ok_or_else(|| camera.error("perspective", "missing"))?;
            Projection::Perspective {
                yfov: p.required_number("yfov")?,
                aspect_ratio: p.opt_number("aspectRatio")?,
                znear: p.required_number("znear")?,
                zfar: p.opt_number("zfar")?,
            }
        }
        Some("orthographic") => {
            let o = camera
                .object("orthographic")?
                .ok_or_else(|| camera.error("orthographic", "missing"))?;
            Projection::Orthographic {
                xmag: o.required_number("xmag")?,
                ymag: o.required_number("ymag")?,
                znear: o.required_number("znear")?,
                zfar: o.required_number("zfar")?,
            }
        }
        other => return Err(camera.error("type", format!("{other:?} is no camera type"))),
    };
    Ok(Camera {
        name: camera.text("name")?,
        projection,
    })
}

pub(super) fn light(light: &Obj<'_, '_>) -> Result<Light> {
    let kind = match light.opt_str("type")? {
        Some("directional") => LightKind::Directional,
        Some("point") => LightKind::Point,
        Some("spot") => {
            let spot = light
                .object("spot")?
                .ok_or_else(|| light.error("spot", "missing"))?;
            LightKind::Spot {
                inner_cone_angle: spot.number("innerConeAngle", 0.0)?,
                outer_cone_angle: spot.number("outerConeAngle", std::f64::consts::FRAC_PI_4)?,
            }
        }
        other => return Err(light.error("type", format!("{other:?} is no light type"))),
    };
    Ok(Light {
        name: light.text("name")?,
        kind,
        color: light.numbers("color", [1.0; 3])?,
        intensity: light.number("intensity", 1.0)?,
        range: light.opt_number("range")?,
    })
}
