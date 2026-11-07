//! Polygons to a glTF mesh: a primitive of triangles (polygons through the
//! display triangulation every engine shows) for each material its
//! polygons name, one of lines for its polylines' segments; a value of
//! points alone is one of points.
//!
//! A glTF vertex is a point where every attribute written lives on points
//! (what a glTF read gives, so it writes back as it was), else a point with
//! each distinct set of corner values. `N` is the authored normal on
//! points, else the shading normal Studio shows (so a mesh without `N`
//! keeps its look); `uv`, `uv2`… are `TEXCOORD_n` (v turned back down),
//! `Cd` and `Alpha` `COLOR_0` (into linear Rec.709), [`TANGENT_U`] and
//! [`TANGENT_V`] `TANGENT`, and point attributes named `_NAME` the file's
//! own.
use super::Out;
use crate::gltf::{self, Attribute, Kind, Primitive, Topology};
use crate::read::mesh::{TANGENT_U, TANGENT_V, uv_name};
use kinogaki_studio_api::Error;
use kinogaki_studio_api::color::LIN_REC709;
use kinogaki_studio_api::geometry::{Domain, Geometry, Storage, Values, names};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

/// Corner values read by point, polygon vertex, primitive or the whole.
#[derive(Clone, Copy)]
struct At<'a, T> {
    values: &'a [T],
    domain: Domain,
}

impl<T: Copy + Default> At<'_, T> {
    /// The value at a corner; zeros for a point of a value of points
    /// alone, which has no vertex or primitive.
    fn get(&self, point: u32, vertex: u32, primitive: u32) -> T {
        let at = match self.domain {
            Domain::Point => point,
            Domain::Vertex => vertex,
            Domain::Primitive => primitive,
            _ => 0,
        };
        self.values.get(at as usize).copied().unwrap_or_default()
    }
}

/// Attribute `name` of `polygons` as `T`, from the first domain holding it.
fn find<'a, T: kinogaki_studio_api::geometry::Tuple>(
    polygons: &'a Geometry,
    name: &str,
) -> Option<At<'a, T>> {
    let domains = [
        Domain::Vertex,
        Domain::Point,
        Domain::Primitive,
        Domain::Detail,
    ];
    domains.into_iter().find_map(|domain| {
        let values = polygons.attributes().tuples::<T>(domain, name)?;
        Some(At { values, domain })
    })
}

/// One corner of a written element: its point, polygon vertex and
/// primitive ([`LONE`] for a point of a value of points alone).
type Corner = [u32; 3];

/// The elements written, by material and topology in order of first use.
type Groups = Vec<(Option<String>, Topology, Vec<Corner>)>;

/// The vertex and primitive of a point of a value of points alone.
const LONE: u32 = u32::MAX;

/// A tangent and its bitangent at each point.
type Tangents<'a> = (&'a [[f32; 3]], &'a [[f32; 3]]);

/// What a mesh writes at each corner.
struct Columns<'a> {
    normal: Option<At<'a, [f32; 3]>>,
    uvs: Vec<(u32, At<'a, [f32; 2]>)>,
    color: Option<At<'a, [f32; 3]>>,
    alpha: Option<At<'a, f32>>,
    tangent: Option<Tangents<'a>>,
    own: Vec<(String, usize, &'a [f32])>,
}

impl Columns<'_> {
    /// Whether every column lives on points: a glTF vertex is a point.
    fn on_points(&self) -> bool {
        let point = |d: Domain| d == Domain::Point;
        self.normal.is_none_or(|n| point(n.domain))
            && self.uvs.iter().all(|(_, uv)| point(uv.domain))
            && self.color.is_none_or(|c| point(c.domain))
            && self.alpha.is_none_or(|a| point(a.domain))
    }

    /// The values at `corner` as bits: what tells two vertices of one point
    /// apart.
    fn key(&self, [p, v, f]: Corner) -> Vec<u32> {
        let mut key = Vec::new();
        if let Some(n) = &self.normal {
            key.extend(n.get(p, v, f).map(f32::to_bits));
        }
        for (_, uv) in &self.uvs {
            key.extend(uv.get(p, v, f).map(f32::to_bits));
        }
        if let Some(c) = &self.color {
            key.extend(c.get(p, v, f).map(f32::to_bits));
        }
        if let Some(a) = &self.alpha {
            key.push(a.get(p, v, f).to_bits());
        }
        key
    }
}

/// `polygons` as a glTF mesh named `name`, its index in the model; none for
/// a value with nothing to write.
pub(super) fn mesh(
    polygons: &Geometry,
    name: &str,
    out: &mut Out<'_>,
) -> Result<Option<usize>, Error> {
    let table = polygons.primitives();
    let attributes = polygons.attributes();
    let shading;
    let normal = match find::<[f32; 3]>(polygons, names::N) {
        Some(n) if n.domain == Domain::Point => Some(n),
        _ if table.polygons().is_empty() => None,
        _ => {
            shading = polygons.shading_normals();
            Some(At {
                values: shading.vertices().as_slice(),
                domain: Domain::Vertex,
            })
        }
    };
    let mut uvs = Vec::new();
    for set in 0..8 {
        if let Some(uv) = find::<[f32; 2]>(polygons, &uv_name(set)) {
            uvs.push((set, uv));
        }
    }
    let tangent = attributes
        .tuples::<[f32; 3]>(Domain::Point, TANGENT_U)
        .zip(attributes.tuples::<[f32; 3]>(Domain::Point, TANGENT_V));
    let mut own = Vec::new();
    for (domain, named, attribute) in attributes.iter() {
        let width = usize::from(attribute.ty().width);
        if domain != Domain::Point || !named.starts_with('_') || width > 4 {
            continue;
        }
        if let (Storage::F32, Values::F32(values)) = (attribute.ty().storage, attribute.values()) {
            own.push((named.to_owned(), width, values.as_slice()));
        }
    }
    let columns = Columns {
        normal,
        uvs,
        color: find::<[f32; 3]>(polygons, names::CD),
        alpha: find::<f32>(polygons, names::ALPHA),
        tangent: tangent.filter(|_| normal.is_some()),
        own,
    };
    let keys = attributes.text(Domain::Primitive, names::MATERIAL);
    let material = |p: u32| -> Option<&str> {
        keys.and_then(|t| t.get(p as usize))
            .filter(|k| !k.is_empty())
    };
    // The triangles and the segments, by material in order of first use.
    let mut groups: Groups = Vec::new();
    let group = |groups: &mut Groups, key: Option<&str>, topology: Topology| -> usize {
        let at = groups
            .iter()
            .position(|(k, t, _)| k.as_deref() == key && *t == topology);
        at.unwrap_or_else(|| {
            groups.push((key.map(str::to_owned), topology, Vec::new()));
            groups.len() - 1
        })
    };
    let offsets = table.offsets();
    let mut last: Option<(u32, usize)> = None;
    let mut triangle = |p: u32, corners: [Corner; 3], groups: &mut Groups| {
        let at = match last {
            Some((q, at)) if q == p => at,
            _ => group(groups, material(p), Topology::Triangles),
        };
        last = Some((p, at));
        groups[at].2.extend(corners);
    };
    let triangles = table.uniform() == Some(kinogaki_studio_api::geometry::Kind::POLYGON)
        && offsets.as_slice().windows(2).all(|w| w[1] - w[0] == 3);
    if triangles {
        // Triangles alone, as a glTF read gives them: each as it is.
        let vertices = table.vertices();
        for p in 0..table.len() as u32 {
            let first = offsets[p as usize];
            let corner = |k: u32| [vertices[(first + k) as usize], first + k, p];
            triangle(p, [corner(0), corner(1), corner(2)], &mut groups);
        }
    } else {
        let triangulation = polygons.triangulation();
        let (points, vertices, owners) = (
            triangulation.triangles(),
            triangulation.corners(),
            triangulation.primitives(),
        );
        for t in 0..triangulation.len() {
            let p = owners[t];
            let corners = [0, 1, 2].map(|k| [points[t][k], vertices[t][k], p]);
            triangle(p, corners, &mut groups);
        }
    }
    for p in 0..table.len() {
        if table.kind(p).is_polygon() {
            continue;
        }
        let at = group(&mut groups, material(p as u32), Topology::Lines);
        let first = offsets[p];
        let line = table.primitive(p);
        for (k, pair) in line.windows(2).enumerate() {
            let k = first + k as u32;
            groups[at]
                .2
                .extend([[pair[0], k, p as u32], [pair[1], k + 1, p as u32]]);
        }
    }
    if table.is_empty() {
        let lone = (0..table.points() as u32)
            .map(|q| [q, LONE, LONE])
            .collect();
        groups.push((None, Topology::Points, lone));
    }
    if groups.is_empty() {
        return Ok(None);
    }
    let mut primitives = Vec::with_capacity(groups.len());
    for (key, topology, corners) in groups {
        out.writing.check()?;
        let material = match key {
            Some(key) => out.material(&key)?,
            None => None,
        };
        primitives.push(primitive(
            polygons, &columns, &corners, topology, material, out,
        )?);
    }
    out.model.meshes.push(gltf::Mesh {
        name: name.to_owned(),
        primitives,
    });
    Ok(Some(out.model.meshes.len() - 1))
}

/// The glTF primitive of `corners` (in groups of `topology`).
fn primitive(
    polygons: &Geometry,
    columns: &Columns<'_>,
    corners: &[Corner],
    topology: Topology,
    material: Option<usize>,
    out: &mut Out<'_>,
) -> Result<Primitive, Error> {
    let (firsts, indices) = vertices(polygons.positions().len(), columns, corners);
    let to_file = out.to_file;
    let positions = polygons.positions();
    let mut attributes = Vec::new();
    let mut column = |name: &str, kind: Kind, values: Vec<f32>| {
        attributes.push(Attribute {
            name: name.to_owned(),
            kind,
            values: Arc::from(values),
        });
    };
    column(
        "POSITION",
        Kind::Vec3,
        flat(&firsts, |[p, ..]| {
            to_file
                .point(positions[p as usize].into())
                .to_array()
                .map(|c| c as f32)
        }),
    );
    if let Some(n) = &columns.normal {
        let unturned = |[p, v, f]: Corner| {
            let n = n.get(p, v, f).map(f64::from);
            to_file.direction(n.into()).to_array().map(|c| c as f32)
        };
        column("NORMAL", Kind::Vec3, flat(&firsts, unturned));
    }
    if let (Some((u, v)), Some(n)) = (columns.tangent, &columns.normal) {
        let tangent = |[p, vertex, f]: Corner| {
            let (t, b, n) = (u[p as usize], v[p as usize], n.get(p, vertex, f));
            let w = match dot(cross(n, t), b) < 0.0 {
                true => -1.0,
                false => 1.0,
            };
            let [x, y, z] = to_file
                .direction(t.map(f64::from).into())
                .to_array()
                .map(|c| c as f32);
            [x, y, z, w]
        };
        column("TANGENT", Kind::Vec4, flat(&firsts, tangent));
    }
    for (set, uv) in &columns.uvs {
        let turned = |[p, v, f]: Corner| {
            let [s, t] = uv.get(p, v, f);
            [s, 1.0 - t]
        };
        column(
            &format!("TEXCOORD_{set}"),
            Kind::Vec2,
            flat(&firsts, turned),
        );
    }
    if let Some(c) = &columns.color {
        let mut rgb: Vec<[f64; 3]> = (firsts.iter())
            .map(|&[p, v, f]| c.get(p, v, f).map(f64::from))
            .collect();
        out.writing.colors.working_to(LIN_REC709, &mut rgb)?;
        let rgb: Vec<[f32; 3]> = rgb.iter().map(|c| c.map(|v| v as f32)).collect();
        match &columns.alpha {
            Some(a) => {
                let rgba = (rgb.iter().zip(&firsts))
                    .flat_map(|(c, &[p, v, f])| [c[0], c[1], c[2], a.get(p, v, f)]);
                column("COLOR_0", Kind::Vec4, rgba.collect());
            }
            None => column("COLOR_0", Kind::Vec3, rgb.into_flattened()),
        }
    }
    for (name, width, values) in &columns.own {
        let kind = [Kind::Scalar, Kind::Vec2, Kind::Vec3, Kind::Vec4][width - 1];
        let mut own = Vec::with_capacity(firsts.len() * width);
        for &[p, ..] in &firsts {
            own.extend_from_slice(&values[p as usize * width..(p as usize + 1) * width]);
        }
        column(name, kind, own);
    }
    Ok(Primitive {
        topology,
        indices: indices.into(),
        attributes,
        material,
    })
}

/// The glTF vertices of `corners` over `points` points: each vertex's first
/// corner, in point order, and each corner's vertex. A vertex is a point
/// where every column lives on points, else a point with one set of corner
/// values.
fn vertices(points: usize, columns: &Columns<'_>, corners: &[Corner]) -> (Vec<Corner>, Vec<u32>) {
    if columns.on_points() {
        let mut index = vec![u32::MAX; points];
        for c in corners {
            index[c[0] as usize] = 0;
        }
        let mut firsts = Vec::new();
        for (p, at) in index.iter_mut().enumerate() {
            if *at == 0 {
                *at = firsts.len() as u32;
                firsts.push([p as u32, LONE, LONE]);
            }
        }
        let indices = corners.iter().map(|c| index[c[0] as usize]).collect();
        return (firsts, indices);
    }
    let keyed: Vec<(u32, Vec<u32>)> = corners.iter().map(|&c| (c[0], columns.key(c))).collect();
    let mut order: Vec<usize> = (0..keyed.len()).collect();
    order.sort_by_key(|&at| (keyed[at].0, at));
    let mut index: HashMap<&(u32, Vec<u32>), u32> = HashMap::new();
    let mut firsts = Vec::new();
    for &at in &order {
        if let Entry::Vacant(new) = index.entry(&keyed[at]) {
            new.insert(firsts.len() as u32);
            firsts.push(corners[at]);
        }
    }
    let indices = keyed.iter().map(|key| index[key]).collect();
    (firsts, indices)
}

/// `each` of every vertex's first corner, flattened.
fn flat<const W: usize>(firsts: &[Corner], each: impl Fn(Corner) -> [f32; W]) -> Vec<f32> {
    let mut out = Vec::with_capacity(firsts.len() * W);
    for &c in firsts {
        out.extend_from_slice(&each(c));
    }
    out
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
