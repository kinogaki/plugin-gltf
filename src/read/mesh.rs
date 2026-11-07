//! A glTF mesh to one Geometry: each primitive's vertices are points of
//! their own (glTF's vertices, unwelded, so a round trip writes them back
//! as they were), its triangles polygons and its lines two-point polylines,
//! its points points alone; a triangle or line naming a vertex twice is
//! left out. Its material is the primitives' `material` text.
//!
//! Point attributes: `N`, `uv` (`TEXCOORD_0`; `TEXCOORD_n` is `uv<n+1>`,
//! v turned up as MaterialX's coordinates run), `Cd` and `Alpha`
//! (`COLOR_0`, from linear Rec.709 into the working space), [`TANGENT_U`]
//! and [`TANGENT_V`] (`TANGENT` and its bitangent), and the file's own
//! (`_NAME`, kept as named). A primitive lacking one its mesh has takes
//! zeros (white for colors, its triangles' normals for `N`). A mesh
//! without normals shades flat, as glTF says: every edge is `sharp`.
use super::LINEAR_SRGB;
use crate::gltf::{self, Gltf, Primitive, Topology};
use crate::space::Space;
use kinogaki_studio_api::color::ColorService;
use kinogaki_studio_api::geometry::{
    Attribute, AttributeType, Attributes, Column, Domain, Elements, Geometry, Kind, Meaning,
    Primitives, Storage, Text, Values, names,
};
use kinogaki_studio_api::{Diagnostic, Error};
use std::collections::BTreeSet;
use std::sync::Arc;

/// The point attribute holding `TANGENT`'s direction.
pub(crate) const TANGENT_U: &str = "tangentu";
/// The point attribute holding the bitangent, `N × T` times `TANGENT`'s
/// handedness.
pub(crate) const TANGENT_V: &str = "tangentv";

/// The point attribute texture coordinate set `set` is read into: `uv`,
/// then `uv2`, `uv3`…
pub(crate) fn uv_name(set: u32) -> String {
    match set {
        0 => names::UV.to_owned(),
        n => format!("uv{}", n + 1),
    }
}

/// Which materials a primitive with `COLOR_0` reads: their base color is
/// the vertex colors' times their own.
pub(super) fn colored_materials(model: &Gltf) -> Vec<bool> {
    let mut colored = vec![false; model.materials.len()];
    for p in model.meshes.iter().flat_map(|m| &m.primitives) {
        if let Some(m) = p.material
            && p.attribute("COLOR_0").is_some()
        {
            colored[m] = true;
        }
    }
    colored
}

/// The key of glTF material `material` in the materials item, which a
/// primitive's `material` text names.
pub(crate) fn key(material: usize) -> String {
    material.to_string()
}

/// `mesh` as one Geometry in `space`; none when it has no primitive with
/// positions (said).
pub(super) fn geometry(
    mesh: &gltf::Mesh,
    space: &Space,
    colors: &dyn ColorService,
    warnings: &mut Vec<Diagnostic>,
) -> Result<Option<Geometry>, Error> {
    let prims: Vec<&Primitive> = (mesh.primitives.iter())
        .filter(|p| p.attribute("POSITION").is_some())
        .collect();
    if prims.len() < mesh.primitives.len() {
        warnings.push(Diagnostic::warning(format!(
            "{}: {} primitives without positions left out",
            named(mesh),
            mesh.primitives.len() - prims.len()
        )));
    }
    if prims.is_empty() {
        return Ok(None);
    }
    let mut bases = Vec::with_capacity(prims.len());
    let mut points = 0usize;
    for p in &prims {
        bases.push(points);
        points += p.vertex_count();
    }
    if u32::try_from(points).is_err() {
        return Err(Error::invalid(format!(
            "{}: too many vertices",
            named(mesh)
        )));
    }
    let mut positions = Vec::with_capacity(points);
    for p in &prims {
        let at = &p.attribute("POSITION").expect("kept").values;
        positions.extend(at.as_chunks::<3>().0.iter().map(|v| space.point(widen(v))));
    }
    let table = Table::of(&prims, &bases);
    if table.dropped > 0 {
        warnings.push(Diagnostic::warning(format!(
            "{}: {} triangles and lines naming a vertex twice left out",
            named(mesh),
            table.dropped
        )));
    }
    let mut attributes = Attributes::new();
    let has = |name: &str| prims.iter().any(|p| p.attribute(name).is_some());
    let normals = has("NORMAL").then(|| normals(&prims, &bases, &table, &positions, space));
    if let Some(n) = &normals {
        let column = Attribute::tuples(n.clone(), Meaning::Normal)?;
        attributes = attributes.with(Domain::Point, names::N, column)?;
    }
    let sets: BTreeSet<u32> = (prims.iter())
        .flat_map(|p| &p.attributes)
        .filter_map(|a| a.name.strip_prefix("TEXCOORD_")?.parse().ok())
        .collect();
    for set in sets {
        let uv: Vec<[f32; 2]> = per_point(&prims, &format!("TEXCOORD_{set}"), [0.0; 2], |v| {
            [v[0], 1.0 - v[1]]
        });
        let column = Attribute::tuples(uv, Meaning::TextureCoord)?;
        attributes = attributes.with(Domain::Point, &uv_name(set), column)?;
    }
    if has("COLOR_0") {
        let rgba: Vec<[f32; 4]> = per_point(&prims, "COLOR_0", [1.0; 4], |v| {
            [v[0], v[1], v[2], v.get(3).copied().unwrap_or(1.0)]
        });
        let mut rgb: Vec<[f64; 3]> = rgba.iter().map(|c| widen(&c[..3])).collect();
        colors.to_working(LINEAR_SRGB, &mut rgb)?;
        let cd: Vec<[f32; 3]> = rgb.iter().map(|c| c.map(|v| v as f32)).collect();
        attributes = attributes.with(
            Domain::Point,
            names::CD,
            Attribute::tuples(cd, Meaning::Color)?,
        )?;
        let alpha = (prims.iter())
            .filter_map(|p| p.attribute("COLOR_0"))
            .any(|a| a.kind == gltf::Kind::Vec4);
        if alpha {
            let a: Vec<f32> = rgba.iter().map(|c| c[3]).collect();
            attributes = attributes.with(
                Domain::Point,
                names::ALPHA,
                Attribute::tuples(a, Meaning::None)?,
            )?;
        }
    }
    if has("TANGENT") {
        let t: Vec<[f32; 4]> = per_point(&prims, "TANGENT", [0.0; 4], |v| [v[0], v[1], v[2], v[3]]);
        let u: Vec<[f32; 3]> = t
            .iter()
            .map(|t| narrow(space.turn(widen(&t[..3]))))
            .collect();
        let v: Vec<[f32; 3]> = match &normals {
            Some(n) => (u.iter().zip(n).zip(&t))
                .map(|((u, n), t)| cross(*n, *u).map(|c| c * t[3]))
                .collect(),
            None => vec![[0.0; 3]; points],
        };
        attributes = attributes
            .with(
                Domain::Point,
                TANGENT_U,
                Attribute::tuples(u, Meaning::Vector)?,
            )?
            .with(
                Domain::Point,
                TANGENT_V,
                Attribute::tuples(v, Meaning::Vector)?,
            )?;
    }
    let own: BTreeSet<&str> = (prims.iter())
        .flat_map(|p| &p.attributes)
        .map(|a| a.name.as_str())
        .filter(|name| name.starts_with('_'))
        .collect();
    for name in own {
        let width = (prims.iter())
            .find_map(|p| p.attribute(name))
            .map_or(1, |a| a.kind.width());
        if width > 4 || names::check_name(name).is_err() || names::standard_type(name).is_some() {
            warnings.push(Diagnostic::warning(format!(
                "{}: attribute {name} left out",
                named(mesh)
            )));
            continue;
        }
        let mut values = Vec::with_capacity(points * width);
        for p in &prims {
            match p.attribute(name).filter(|a| a.kind.width() == width) {
                Some(a) => values.extend_from_slice(&a.values),
                None => values.resize(values.len() + p.vertex_count() * width, 0.0),
            }
        }
        let ty = AttributeType::new(Storage::F32, width as u8, Meaning::None);
        let column = Attribute::new(ty, Values::F32(Column::new(values)))?;
        attributes = attributes.with(Domain::Point, name, column)?;
    }
    if prims.iter().any(|p| p.material.is_some()) {
        attributes =
            attributes.with(Domain::Primitive, names::MATERIAL, table.materials(&prims)?)?;
    }
    let primitives = Primitives::new(
        points,
        Column::new(table.offsets),
        Column::new(table.corners),
        Column::new(table.kinds),
        None,
    )?;
    if normals.is_none() {
        // glTF shades a mesh without normals flat: every edge sharp.
        let edges = Elements::all(primitives.edge_count());
        attributes = attributes.with_group(Domain::Edge, names::SHARP, edges)?;
    }
    Ok(Some(Geometry::new(
        Column::new(positions),
        primitives,
        attributes,
    )?))
}

/// The polygons and polylines of a mesh's primitives, in order.
struct Table {
    offsets: Vec<u32>,
    corners: Vec<u32>,
    kinds: Vec<Kind>,
    /// Each element's primitive.
    from: Vec<u32>,
    /// Triangles and lines naming a vertex twice.
    dropped: usize,
}

impl Table {
    fn of(prims: &[&Primitive], bases: &[usize]) -> Table {
        let elements: usize = (prims.iter())
            .map(|p| p.indices.len() / p.topology.corners())
            .sum();
        let mut table = Table {
            offsets: Vec::with_capacity(elements + 1),
            corners: Vec::with_capacity(elements * 3),
            kinds: Vec::with_capacity(elements),
            from: Vec::with_capacity(elements),
            dropped: 0,
        };
        table.offsets.push(0);
        for (at, (p, &base)) in prims.iter().zip(bases).enumerate() {
            let base = base as u32;
            let kind = match p.topology {
                Topology::Points => continue,
                Topology::Lines => Kind::POLYLINE,
                Topology::Triangles => Kind::POLYGON,
            };
            for element in p.indices.chunks_exact(p.topology.corners()) {
                let repeats = (1..element.len()).any(|i| element[..i].contains(&element[i]));
                if repeats {
                    table.dropped += 1;
                    continue;
                }
                table.corners.extend(element.iter().map(|&i| i + base));
                table.offsets.push(table.corners.len() as u32);
                table.kinds.push(kind);
                table.from.push(at as u32);
            }
        }
        table
    }

    /// Each element's material key; empty for a primitive with none.
    fn materials(&self, prims: &[&Primitive]) -> Result<Attribute, Error> {
        let mut keys: Vec<Arc<str>> = Vec::new();
        let slots: Vec<u32> = (prims.iter())
            .map(|p| {
                let key: Arc<str> = p.material.map_or_else(|| Arc::from(""), |m| key(m).into());
                match keys.iter().position(|k| *k == key) {
                    Some(at) => at as u32,
                    None => {
                        keys.push(key);
                        (keys.len() - 1) as u32
                    }
                }
            })
            .collect();
        let indices = self.from.iter().map(|&p| slots[p as usize]).collect();
        Ok(Attribute::text(Text::new(
            Column::new(indices),
            keys.into(),
        )?))
    }
}

/// Every point's `N`: a primitive's own, turned, else the area-weighted
/// normals of its triangles (glTF's flat normals, smoothed where the
/// primitive shares vertices).
fn normals(
    prims: &[&Primitive],
    bases: &[usize],
    table: &Table,
    positions: &[[f64; 3]],
    space: &Space,
) -> Vec<[f32; 3]> {
    let mut n = Vec::with_capacity(positions.len());
    for (at, (p, &base)) in prims.iter().zip(bases).enumerate() {
        if let Some(own) = p.attribute("NORMAL") {
            let turned = own.values.as_chunks::<3>().0.iter();
            n.extend(turned.map(|v| narrow(space.turn(widen(v)))));
            continue;
        }
        let mut sum = vec![[0.0f64; 3]; p.vertex_count()];
        let elements = (0..table.kinds.len()).filter(|&e| table.from[e] == at as u32);
        for e in elements.filter(|&e| table.kinds[e] == Kind::POLYGON) {
            let corners = &table.corners[table.offsets[e] as usize..table.offsets[e + 1] as usize];
            let [a, b, c] = [0, 1, 2].map(|i| positions[corners[i] as usize]);
            let face = cross64(sub(b, a), sub(c, a));
            for &corner in corners {
                let s = &mut sum[corner as usize - base];
                *s = [s[0] + face[0], s[1] + face[1], s[2] + face[2]];
            }
        }
        n.extend(sum.into_iter().map(|s| {
            let length = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt();
            match length > 0.0 {
                true => narrow(s.map(|c| c / length)),
                false => [0.0, 0.0, 1.0],
            }
        }));
    }
    n
}

/// One value a point of attribute `name` across `prims`: each value of a
/// primitive having it through `read`, `missing` for one lacking it.
fn per_point<T: Copy>(
    prims: &[&Primitive],
    name: &str,
    missing: T,
    read: impl Fn(&[f32]) -> T,
) -> Vec<T> {
    let mut out = Vec::with_capacity(prims.iter().map(|p| p.vertex_count()).sum());
    for p in prims {
        match p.attribute(name) {
            Some(a) => out.extend(a.values.chunks_exact(a.kind.width()).map(&read)),
            None => out.extend(std::iter::repeat_n(missing, p.vertex_count())),
        }
    }
    out
}

/// What a message calls `mesh`.
fn named(mesh: &gltf::Mesh) -> String {
    match mesh.name.as_str() {
        "" => "a mesh".to_owned(),
        name => format!("mesh {name}"),
    }
}

fn widen(v: &[f32]) -> [f64; 3] {
    [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
}

fn narrow(v: [f64; 3]) -> [f32; 3] {
    v.map(|c| c as f32)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross64(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
