//! The default scene's node tree as items, parents first: a node is its
//! mesh, else its camera or light when it holds nothing else, else a Part;
//! what else it holds is an item under it. A mesh several nodes use is
//! instances of it, one placement where the node is: one value their items
//! share, the mesh held once (and written once).
//!
//! Lights (`KHR_lights_punctual`): a point light is a spot of
//! 180° and 1 mm radius, a spot light a spot of its outer cone softened
//! over the share of it outside the inner one, a directional light a
//! distant one; candela and lux are the intensity's illuminance at a metre,
//! so a sphere's radiance is its candela over its projected area. Cameras:
//! `yfov` on a 24 mm high film back, `xmag` and `ymag` as the view's size.
use crate::gltf::{self, Gltf, LightKind, Projection};
use crate::space::{self, Placed};
use kinogaki_render_api::standard::{Light, Shape};
use kinogaki_render_api::{Camera, Projection as Lens};
use kinogaki_studio_api::color::{ColorService, LIN_REC709};
use kinogaki_studio_api::format::Conversion;
use kinogaki_studio_api::geometry::{Attributes, Xform};
use kinogaki_studio_api::standard::{self, Instances};
use kinogaki_studio_api::{Data, Diagnostic, Error, FileItem};

/// A point or spot light's radius, metres.
pub(crate) const RADIUS: f64 = 0.001;
/// The film back's height a perspective camera's `yfov` is read on, mm.
pub(crate) const FILM_HEIGHT: f64 = 24.0;
/// The far distance of a camera glTF gives none (an infinite one), in the
/// project's units: the farthest a camera clips.
pub(crate) const FAR: f32 = 1e12;

/// Appends the default scene's items to `items` (whose indices their
/// parents name).
pub(super) fn items(
    model: &Gltf,
    meshes: &[Option<Data>],
    space: &Conversion,
    colors: &dyn ColorService,
    items: &mut Vec<FileItem>,
    warnings: &mut Vec<Diagnostic>,
) -> Result<(), Error> {
    let mut ranges = 0;
    // (node, its parent's item), depth first in order.
    let roots = roots(model, warnings);
    let mut stack: Vec<(usize, Option<usize>)> = roots.iter().rev().map(|&n| (n, None)).collect();
    while let Some((n, parent)) = stack.pop() {
        let node = &model.nodes[n];
        let mesh = node.mesh.and_then(|m| Some((m, meshes[m].as_ref()?)));
        let mut held: Vec<(String, Data)> = Vec::new();
        if let Some(c) = node.camera {
            let camera = &model.cameras[c];
            let named = or(&camera.name, "Camera");
            held.push((named, standard::camera().wrap(camera_value(camera, space))?));
        }
        if let Some(l) = node.light {
            let light = &model.lights[l];
            ranges += usize::from(light.range.is_some());
            let named = or(&light.name, "Light");
            let value = light_of(light, space, colors)?;
            held.push((named, standard::light().wrap(value)?));
        }
        let name = match (node.name.trim(), mesh) {
            ("", Some((m, _))) => or(&model.meshes[m].name, "Node"),
            ("", None) => (held.first()).map_or_else(|| "Node".to_owned(), |h| h.0.clone()),
            (name, _) => name.to_owned(),
        };
        let own = mesh.map(|(_, polygons)| polygons.clone());
        let alone = own.is_none() && node.children.is_empty() && held.len() == 1;
        let (data, placed) = match alone {
            true => (Some(held.remove(0).1), Placed::Frame),
            false => (own, Placed::Value),
        };
        let at = items.len();
        items.push(FileItem {
            name,
            parent,
            placement: space::placed(space, &space::xform(&node.transform), placed),
            visible: true,
            data,
        });
        let frame = space.frame(&Xform::IDENTITY);
        for (name, data) in held {
            items.push(FileItem {
                name,
                parent: Some(at),
                placement: frame,
                visible: true,
                data: Some(data),
            });
        }
        stack.extend(node.children.iter().rev().map(|&c| (c, Some(at))));
    }
    if ranges > 0 {
        warnings.push(Diagnostic::warning(format!(
            "the range of {ranges} light{} left out: lights reach everywhere",
            if ranges == 1 { "" } else { "s" }
        )));
    }
    Ok(())
}

/// `meshes` with each that several nodes use as instances of it, one
/// placement where the node is.
pub(super) fn shared(model: &Gltf, meshes: Vec<Option<Data>>) -> Result<Vec<Option<Data>>, Error> {
    let mut uses = vec![0_usize; meshes.len()];
    for m in model.nodes.iter().filter_map(|n| n.mesh) {
        uses[m] += 1;
    }
    (meshes.into_iter().zip(uses))
        .map(|(mesh, uses)| match mesh {
            Some(polygons) if uses > 1 => {
                let here = vec![Xform::IDENTITY].into();
                let one = Instances::new(vec![polygons], here, None, Attributes::new())?;
                Ok(Some(standard::instances().wrap(one)))
            }
            mesh => Ok(mesh),
        })
        .collect()
}

/// The nodes the default scene shows (`scene`, else the first), else every
/// root; other scenes are said.
fn roots(model: &Gltf, warnings: &mut Vec<Diagnostic>) -> Vec<usize> {
    let others = model.scenes.len().saturating_sub(1);
    if others > 0 {
        warnings.push(Diagnostic::warning(format!(
            "{others} scene{} besides the default left out",
            if others == 1 { "" } else { "s" }
        )));
    }
    match model.scene.or((!model.scenes.is_empty()).then_some(0)) {
        Some(scene) => model.scenes[scene].nodes.clone(),
        None => {
            let mut child = vec![false; model.nodes.len()];
            for &c in model.nodes.iter().flat_map(|n| &n.children) {
                child[c] = true;
            }
            (0..model.nodes.len()).filter(|&n| !child[n]).collect()
        }
    }
}

/// `name`, else `fallback`.
fn or(name: &str, fallback: &str) -> String {
    match name.trim() {
        "" => fallback.to_owned(),
        name => name.to_owned(),
    }
}

/// `light` as a standard light in `space`, its color in the working space.
fn light_of(
    light: &gltf::Light,
    space: &Conversion,
    colors: &dyn ColorService,
) -> Result<Light, Error> {
    let mut color = [light.color];
    colors.to_working(LIN_REC709, &mut color)?;
    let radius = space.length(RADIUS) as f32;
    let sphere = std::f64::consts::PI * RADIUS * RADIUS;
    let (shape, intensity) = match light.kind {
        LightKind::Directional => (LightShape::Distant { angle: 0.0 }, light.intensity),
        LightKind::Point => (
            LightShape::Spot {
                radius,
                cone_angle: 180.0,
                cone_softness: 0.0,
            },
            light.intensity / sphere,
        ),
        LightKind::Spot {
            inner_cone_angle,
            outer_cone_angle,
        } => (
            LightShape::Spot {
                radius,
                cone_angle: outer_cone_angle.to_degrees() as f32,
                cone_softness: match outer_cone_angle > 0.0 {
                    true => ((outer_cone_angle - inner_cone_angle) / outer_cone_angle) as f32,
                    false => 0.0,
                },
            },
            light.intensity / sphere,
        ),
    };
    Ok(Light {
        shape,
        color: color[0].map(|c| c as f32),
        intensity: intensity as f32,
        normalize: false,
        ..Light::default()
    })
}

/// `camera` as a standard camera's lens in `space`.
fn camera_value(camera: &gltf::Camera, space: &Conversion) -> Camera {
    let near = |znear: f64| (space.length(znear) as f32).max(f32::MIN_POSITIVE);
    match camera.projection {
        Projection::Perspective {
            yfov,
            aspect_ratio,
            znear,
            zfar,
        } => Camera {
            projection: Lens::Perspective,
            focal_length: (FILM_HEIGHT / 2.0 / (yfov / 2.0).tan()) as f32,
            horizontal_aperture: (FILM_HEIGHT * aspect_ratio.unwrap_or(1.5)) as f32,
            vertical_aperture: FILM_HEIGHT as f32,
            clipping_range: [near(znear), zfar.map_or(FAR, |f| space.length(f) as f32)],
            ..Camera::default()
        },
        Projection::Orthographic {
            xmag,
            ymag,
            znear,
            zfar,
        } => Camera {
            projection: Lens::Orthographic,
            horizontal_aperture: (xmag * 2000.0) as f32,
            vertical_aperture: (ymag * 2000.0) as f32,
            clipping_range: [near(znear), space.length(zfar) as f32],
            ..Camera::default()
        },
    }
}
