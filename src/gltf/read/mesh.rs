//! Meshes and their primitives: attributes decoded and checked, strips,
//! fans and loops made lists, morph targets applied at the default weights.
use super::fields::Obj;
use super::{Indices, Reader, Result};
use crate::gltf::model::{Attribute, Kind, Mesh, Primitive, Topology};
use std::collections::HashMap;

impl Reader<'_> {
    pub(super) fn mesh(&mut self, mesh: &Obj<'_, '_>, materials: usize) -> Result<Mesh> {
        let primitives = mesh.objects("primitives")?;
        if primitives.is_empty() {
            return Err(mesh.error("primitives", "a mesh needs a primitive"));
        }
        let weights = mesh.number_list("weights")?;
        let mut read = Vec::with_capacity(primitives.len());
        for primitive in &primitives {
            self.check()?;
            read.push(self.primitive(primitive, materials, &weights)?);
        }
        if primitives.iter().any(|p| p.has("targets")) {
            self.morphed_meshes += 1;
        }
        Ok(Mesh {
            name: mesh.text("name")?,
            primitives: read,
        })
    }

    fn primitive(
        &mut self,
        primitive: &Obj<'_, '_>,
        materials: usize,
        weights: &[f64],
    ) -> Result<Primitive> {
        let named = primitive
            .object("attributes")?
            .ok_or_else(|| primitive.error("attributes", "missing"))?;
        let mut attributes = Vec::new();
        for (name, index) in named.indexed(self.accessors.len()) {
            // Skins are left out, and with them their joints and weights.
            if name.starts_with("JOINTS_") || name.starts_with("WEIGHTS_") {
                continue;
            }
            let index = index?;
            let kind = self.accessors[index].kind();
            if !fits(name, kind) {
                return Err(named.error(name, format!("{name} can't be {}", kind.name())));
            }
            attributes.push(Attribute {
                name: name.to_owned(),
                kind,
                values: self.floats(index)?,
            });
        }
        let count = attributes.first().map_or(0, Attribute::count);
        if let Some(odd) = attributes.iter().find(|a| a.count() != count) {
            let message = format!(
                "{} has {} vertices where {} has {count}",
                odd.name,
                odd.count(),
                attributes[0].name
            );
            return Err(primitive.error("attributes", message));
        }
        if u32::try_from(count).is_err() {
            return Err(primitive.error("attributes", "more vertices than 32-bit indices reach"));
        }
        let which = match primitive.index("indices", self.accessors.len())? {
            Some(index) => Indices::Accessor(index),
            None => Indices::Sequential(count),
        };
        let (corners, max) = self.index_list(which)?;
        if let Some(past) = max.filter(|&max| max as usize >= count) {
            return Err(
                primitive.error("indices", format!("index {past} is past {count} vertices"))
            );
        }
        let mode = primitive.count("mode", 4)?;
        let (topology, indices) = match lists(mode, &corners) {
            Ok((topology, None)) => (topology, corners),
            Ok((topology, Some(made))) => {
                self.spend(made.len().checked_mul(4), &primitive.at)?;
                (topology, made.into())
            }
            Err(message) => return Err(primitive.error("mode", message)),
        };
        let targets = primitive.objects("targets")?;
        if !targets.is_empty() {
            let by_name: HashMap<String, usize> = (attributes.iter().enumerate())
                .map(|(at, a)| (a.name.clone(), at))
                .collect();
            for (target, deltas) in targets.iter().enumerate() {
                let weight = weights.get(target).copied().unwrap_or(0.0) as f32;
                if weight != 0.0 {
                    self.morph(deltas, weight, &by_name, &mut attributes)?;
                }
            }
        }
        Ok(Primitive {
            topology,
            indices,
            attributes,
            material: primitive.index("material", materials)?,
        })
    }

    /// Adds `weight` times the target's deltas to the attributes it moves.
    fn morph(
        &mut self,
        target: &Obj<'_, '_>,
        weight: f32,
        by_name: &HashMap<String, usize>,
        attributes: &mut [Attribute],
    ) -> Result<()> {
        for (name, index) in target.indexed(self.accessors.len()) {
            let index = index?;
            let Some(&at) = by_name.get(name) else {
                continue;
            };
            let attribute = &mut attributes[at];
            let width = self.accessors[index].kind().width();
            if self.accessors[index].count() != attribute.count() || width > attribute.kind.width()
            {
                return Err(target.error(name, format!("doesn't match the primitive's {name}")));
            }
            let deltas = self.unshared_floats(index)?;
            self.spend(attribute.values.len().checked_mul(4), &target.at)?;
            let mut values = attribute.values.to_vec();
            for (vertex, delta) in values
                .chunks_exact_mut(attribute.kind.width())
                .zip(deltas.chunks_exact(width))
            {
                for (value, delta) in vertex.iter_mut().zip(delta) {
                    *value += weight * delta;
                }
            }
            attribute.values = values.into();
        }
        Ok(())
    }
}

/// Whether attribute `name` may hold elements of `kind`.
fn fits(name: &str, kind: Kind) -> bool {
    match name {
        "POSITION" | "NORMAL" => kind == Kind::Vec3,
        "TANGENT" => kind == Kind::Vec4,
        _ if name.starts_with("TEXCOORD_") => kind == Kind::Vec2,
        _ if name.starts_with("COLOR_") => matches!(kind, Kind::Vec3 | Kind::Vec4),
        _ => true,
    }
}

/// A primitive mode's corners as points, line pairs or triangle triples:
/// the corners as they are (`None`), or the lists made of them. Strips
/// drop the degenerate triangles that join their runs.
type Lists = (Topology, Option<Vec<u32>>);

fn lists(mode: usize, v: &[u32]) -> std::result::Result<Lists, String> {
    let n = v.len();
    Ok(match mode {
        0 => (Topology::Points, None),
        1 if n.is_multiple_of(2) => (Topology::Lines, None),
        2 | 3 => {
            let mut lines: Vec<u32> = v.windows(2).flatten().copied().collect();
            if mode == 2 && n > 2 {
                lines.extend([v[n - 1], v[0]]);
            }
            (Topology::Lines, Some(lines))
        }
        4 if n.is_multiple_of(3) => (Topology::Triangles, None),
        5 => {
            let triangles = (0..n.saturating_sub(2))
                .map(|i| {
                    if i % 2 == 0 {
                        [v[i], v[i + 1], v[i + 2]]
                    } else {
                        [v[i], v[i + 2], v[i + 1]]
                    }
                })
                .filter(|[a, b, c]| a != b && b != c && c != a);
            (Topology::Triangles, Some(triangles.flatten().collect()))
        }
        6 => (
            Topology::Triangles,
            Some(
                (1..n.saturating_sub(1))
                    .flat_map(|i| [v[i], v[i + 1], v[0]])
                    .collect(),
            ),
        ),
        1 | 4 => return Err(format!("{n} corners don't make whole elements")),
        _ => return Err(format!("{mode} is no primitive mode")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_fans_and_loops_become_lists() {
        let v = [0, 1, 2, 3, 4];
        let made = |mode, v: &[u32]| lists(mode, v).map(|(t, l)| (t, l.unwrap_or(v.to_vec())));
        assert_eq!(
            made(5, &v).unwrap(),
            (Topology::Triangles, vec![0, 1, 2, 1, 3, 2, 2, 3, 4])
        );
        assert_eq!(
            made(6, &v).unwrap(),
            (Topology::Triangles, vec![1, 2, 0, 2, 3, 0, 3, 4, 0])
        );
        assert_eq!(
            made(3, &v).unwrap(),
            (Topology::Lines, vec![0, 1, 1, 2, 2, 3, 3, 4])
        );
        assert_eq!(
            made(2, &v[..3]).unwrap(),
            (Topology::Lines, vec![0, 1, 1, 2, 2, 0])
        );
        assert_eq!(
            made(5, &[0, 1, 2, 2, 3, 3, 4, 5]).unwrap(),
            (Topology::Triangles, vec![0, 1, 2, 3, 5, 4])
        );
        assert_eq!(
            lists(4, &v[..3]).unwrap(),
            (Topology::Triangles, None),
            "as they are"
        );
        assert!(lists(4, &v).is_err());
        assert!(lists(1, &v).is_err());
        assert!(lists(7, &v).is_err());
        assert_eq!(made(5, &[0, 1]).unwrap(), (Topology::Triangles, vec![]));
    }
}
