//! Items to glTF's nodes, parents first: each item a node placed in glTF's
//! space, holding its mesh (one value several items share written once),
//! light or camera; instances a child node an instance, each prototype's
//! mesh written once (once more for an instance whose `material` stands in
//! for its prototype's own), but one placement where the node is of a mesh
//! in its own material (a mesh several nodes show) the node's mesh. Lights
//! and cameras are the read's inverse (a sphere light a point light, a
//! shaped one a spot light); a rect or disk light is written as a spot light
//! of its intensity facing down its axis, a cylinder as a point light of its
//! intensity across its side, a dome is left out, said.
use super::Out;
use crate::gltf::{self, Camera, LightKind, Node, Projection, Scene};
use crate::read::scene::FAR;
use crate::space::{self, Placed};
use kinogaki_render_api::standard::{Emission, Light, LightShape, blackbody, illuminant};
use kinogaki_render_api::{Projection as Lens, RENDER_SPACE};
use kinogaki_studio_api::color::LIN_REC709;
use kinogaki_studio_api::geometry::{Domain, Geometry, Xform, names};
use kinogaki_studio_api::standard::{self, Instances};
use kinogaki_studio_api::{Data, Error, FileItem};
use std::collections::HashMap;
use std::f64::consts::PI;

/// Writes `items` into `out`'s model as its one scene.
pub(super) fn nodes(items: &[FileItem], out: &mut Out<'_>) -> Result<(), Error> {
    let mut made: Vec<Option<usize>> = Vec::with_capacity(items.len());
    let mut roots = Vec::new();
    let mut meshes = Meshes::default();
    for item in items {
        out.writing.check()?;
        let data = item.data.as_ref();
        if data.is_some_and(|d| standard::materials().get(d).is_some()) {
            made.push(None);
            continue;
        }
        let framed = data.is_some_and(|d| {
            standard::light().get(d).is_some() || standard::camera().get(d).is_some()
        });
        let placed = if framed { Placed::Frame } else { Placed::Value };
        let mut node = Node {
            name: item.name.clone(),
            transform: space::transform(
                space::placed(&out.to_file, &item.placement, placed).rows(),
            ),
            ..Node::default()
        };
        if let Some(data) = data {
            held(data, &item.name, &mut node, &mut meshes, out)?;
        }
        out.model.nodes.push(node);
        let at = out.model.nodes.len() - 1;
        match item.parent.and_then(|p| made.get(p).copied().flatten()) {
            Some(parent) => out.model.nodes[parent].children.push(at),
            None => roots.push(at),
        }
        made.push(Some(at));
    }
    out.model.scenes.push(Scene {
        name: String::new(),
        nodes: roots,
    });
    out.model.scene = Some(0);
    Ok(())
}

/// The meshes made, by the value (an item's or a prototype's) and what
/// stands in for its own materials.
#[derive(Default)]
struct Meshes(HashMap<(usize, Instead), Option<usize>>);

/// What an instance renders its prototype's primitives with in place of
/// their own materials, as a key of the meshes made: one of its own, else
/// its map's pairs (none: theirs).
#[derive(Clone, Default, PartialEq, Eq, Hash)]
struct Instead {
    look: Option<String>,
    map: Vec<(String, String)>,
}

impl Instead {
    /// What instance `i` of `copies` renders with.
    fn of(copies: &Instances, i: usize) -> Self {
        let looks = copies.attributes().text(Domain::Instance, names::MATERIAL);
        let look = looks.and_then(|t| t.get(i)).filter(|k| !k.is_empty());
        let map = standard::material_map(copies.attributes(), i);
        Self {
            look: look.map(str::to_owned),
            map: (map.into_iter())
                .map(|(from, to)| (from.to_owned(), to.to_owned()))
                .collect(),
        }
    }
}

/// What `data` puts on `node`.
fn held(
    data: &Data,
    name: &str,
    node: &mut Node,
    meshes: &mut Meshes,
    out: &mut Out<'_>,
) -> Result<(), Error> {
    if let Some(light) = standard::light().get(data) {
        node.light = light_of(light, name, out)?;
    } else if let Some(camera) = standard::camera().get(data) {
        out.model.cameras.push(camera_of(camera, name, out));
        node.camera = Some(out.model.cameras.len() - 1);
    } else if let Some(copies) = standard::instances().get(data) {
        match here(copies) {
            Some(prototype) => held(prototype, name, node, meshes, out)?,
            None => node.children = instances(copies, name, meshes, out)?,
        }
    } else if let Some(&mesh) = meshes.0.get(&(data.identity(), Instead::default())) {
        // A value several items share (a glTF mesh several nodes used).
        node.mesh = mesh;
    } else if let Some(polygons) = polygons(data, out) {
        node.mesh = super::mesh::mesh(&polygons, name, out)?;
        meshes
            .0
            .insert((data.identity(), Instead::default()), node.mesh);
    } else {
        out.warn(format!("{name}: glTF holds no {}; left out", data.ty()));
    }
    Ok(())
}

/// The one prototype `copies` places where it is, in its own materials:
/// what the node holds itself.
fn here(copies: &Instances) -> Option<&Data> {
    let here = copies.xforms().as_slice() == [Xform::IDENTITY];
    (here && Instead::of(copies, 0) == Instead::default())
        .then(|| &copies.prototypes()[copies.prototype_index(0)])
}

/// `data` as polygons: its own, or as the installed mesher makes them.
fn polygons(data: &Data, out: &Out<'_>) -> Option<Geometry> {
    match standard::geometry().get(data) {
        Some(polygons) => Some(polygons.clone()),
        None => (out.writing.meshed)(data),
    }
}

/// A child node an instance of `copies`, each placing its prototype's mesh.
fn instances(
    copies: &Instances,
    name: &str,
    meshes: &mut Meshes,
    out: &mut Out<'_>,
) -> Result<Vec<usize>, Error> {
    let attributes = copies.attributes();
    let names = attributes.text(Domain::Instance, names::NAME);
    let mut children = Vec::with_capacity(copies.len());
    for (i, xform) in copies.xforms().iter().enumerate() {
        out.writing.check()?;
        let prototype = copies.prototype_index(i);
        let named = names.and_then(|t| t.get(i)).filter(|n| !n.is_empty());
        let named = named.map_or_else(|| format!("{name} {i}"), str::to_owned);
        let mut node = Node {
            name: named.clone(),
            transform: space::transform(out.to_file.placement(xform).rows()),
            ..Node::default()
        };
        let data = &copies.prototypes()[prototype];
        if let Some(nested) = standard::instances().get(data) {
            node.children = instances(nested, &named, meshes, out)?;
        } else {
            let instead = Instead::of(copies, i);
            let key = (data.identity(), instead.clone());
            node.mesh = match meshes.0.get(&key) {
                Some(&mesh) => mesh,
                None => {
                    let mesh = match polygons(data, out) {
                        Some(polygons) => {
                            let rendered =
                                standard::instance_materials(&polygons, copies.attributes(), i)?;
                            super::mesh::mesh(rendered.as_ref().unwrap_or(&polygons), &named, out)?
                        }
                        None => None,
                    };
                    meshes.0.insert(key, mesh);
                    mesh
                }
            };
        }
        out.model.nodes.push(node);
        children.push(out.model.nodes.len() - 1);
    }
    Ok(children)
}

/// `light` as a `KHR_lights_punctual` light (its index), the read's
/// inverse; none for a dome, said.
fn light_of(light: &Light, name: &str, out: &mut Out<'_>) -> Result<Option<usize>, Error> {
    let gain = f64::from(light.intensity) * 2f64.powf(f64::from(light.exposure));
    let to_file = out.to_file;
    let metres = |length: f32| to_file.length(f64::from(length));
    let (kind, intensity) = match light.shape {
        LightShape::Dome { .. } => {
            out.warn(format!("{name}: glTF has no dome light; left out"));
            return Ok(None);
        }
        LightShape::Distant { angle } => {
            let half = f64::from(angle).to_radians() / 2.0;
            let disk = match angle > 0.0 && !light.normalize {
                true => PI * half.sin().powi(2),
                false => 1.0,
            };
            (LightKind::Directional, gain * disk)
        }
        LightShape::Sphere { radius, .. } => {
            // Its radiant intensity: radiance × πr², normalized radiance / 4.
            let intensity = match light.normalize {
                true => gain / 4.0,
                false => gain * PI * metres(radius).powi(2),
            };
            let kind = match &light.shaping {
                Some(s) if s.cone_angle < 180.0 => {
                    let outer = f64::from(s.cone_angle).to_radians();
                    LightKind::Spot {
                        inner_cone_angle: outer * (1.0 - f64::from(s.cone_softness)),
                        outer_cone_angle: outer.min(PI / 2.0),
                    }
                }
                _ => LightKind::Point,
            };
            (kind, intensity)
        }
        LightShape::Cylinder { length, radius, .. } => {
            out.warn(format!("{name}: a cylinder light written as a point light"));
            let area = 2.0 * PI * f64::from(radius) * f64::from(length);
            let across = metres(length) * 2.0 * metres(radius);
            let radiance = match light.normalize {
                true => gain / area,
                false => gain,
            };
            (LightKind::Point, radiance * across)
        }
        LightShape::Rect { width, height, .. } => {
            out.warn(format!("{name}: an area light written as a spot light"));
            let area = f64::from(width) * f64::from(height);
            area_light(light, gain, area, metres(width) * metres(height))
        }
        LightShape::Disk { radius } => {
            out.warn(format!("{name}: an area light written as a spot light"));
            let area = PI * f64::from(radius).powi(2);
            area_light(light, gain, area, PI * metres(radius).powi(2))
        }
    };
    let color = match &light.emission {
        Emission::Color => out.color(light.color.map(f64::from))?,
        others => {
            let mut c = [match others {
                Emission::Illuminant(named) => illuminant(named).unwrap_or([1.0; 3]),
                _ => blackbody(f64::from(light.color_temperature)),
            }];
            (out.writing.colors).convert(RENDER_SPACE, LIN_REC709, &mut c)?;
            c[0]
        }
    };
    out.model.lights.push(gltf::Light {
        name: name.to_owned(),
        kind,
        color,
        intensity,
        range: None,
    });
    Ok(Some(out.model.lights.len() - 1))
}

/// An area light as a spot light of a quarter turn facing down its axis:
/// its luminous intensity along the axis is its radiance times its area.
fn area_light(light: &Light, gain: f64, area: f64, metres: f64) -> (LightKind, f64) {
    let radiance = match light.normalize {
        true => gain / area,
        false => gain,
    };
    let kind = LightKind::Spot {
        inner_cone_angle: 0.0,
        outer_cone_angle: PI / 2.0,
    };
    (kind, radiance * metres)
}

/// `camera` as a glTF camera, the read's inverse.
fn camera_of(camera: &kinogaki_render_api::Camera, name: &str, out: &Out<'_>) -> Camera {
    let metres = |length: f32| out.to_file.length(f64::from(length));
    let [near, far] = camera.clipping_range;
    let zfar = (far < FAR).then(|| metres(far));
    let projection = match camera.projection {
        Lens::Perspective => {
            let height = f64::from(camera.vertical_aperture);
            let yfov = 2.0 * (height / 2.0 / f64::from(camera.focal_length)).atan();
            Projection::Perspective {
                yfov,
                aspect_ratio: Some(f64::from(camera.horizontal_aperture) / height),
                znear: metres(near),
                zfar,
            }
        }
        Lens::Orthographic => Projection::Orthographic {
            xmag: f64::from(camera.horizontal_aperture) / 2000.0,
            ymag: f64::from(camera.vertical_aperture) / 2000.0,
            znear: if near <= f32::MIN_POSITIVE {
                0.0
            } else {
                metres(near)
            },
            zfar: zfar.unwrap_or(metres(far)),
        },
    };
    Camera {
        name: name.to_owned(),
        projection,
    }
}
