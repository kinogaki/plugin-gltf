//! The one buffer a written file has: buffer views on four-byte boundaries
//! and the accessors over them.
use crate::gltf::json::{Members, Value};
use crate::gltf::model::Kind;
use std::collections::HashMap;
use std::sync::Arc;

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;

#[derive(Default)]
pub(super) struct Binary {
    pub bytes: Vec<u8>,
    pub views: Vec<Value<'static>>,
    pub accessors: Vec<Value<'static>>,
    /// Accessors already written for shared attribute values, by address.
    shared: HashMap<(*const f32, usize), usize>,
}

impl Binary {
    /// A buffer view of `data`; its index.
    pub fn view(&mut self, data: &[u8], target: Option<u32>) -> usize {
        let offset = self.align();
        self.bytes.extend_from_slice(data);
        self.push_view(offset, data.len(), target)
    }

    fn align(&mut self) -> usize {
        self.bytes.resize(self.bytes.len().next_multiple_of(4), 0);
        self.bytes.len()
    }

    fn push_view(&mut self, offset: usize, length: usize, target: Option<u32>) -> usize {
        let view = Members::new()
            .with("buffer", 0usize)
            .with_if(offset > 0, "byteOffset", offset)
            .with("byteLength", length)
            .with_some("target", target);
        self.views.push(view.into());
        self.views.len() - 1
    }

    /// An accessor of vertex attribute values; the same values (one
    /// allocation) give the same accessor. `bounds` adds `min` and `max`.
    pub fn attribute(&mut self, values: &Arc<[f32]>, kind: Kind, bounds: bool) -> usize {
        let key = (values.as_ptr(), values.len());
        if let Some(&accessor) = self.shared.get(&key) {
            return accessor;
        }
        let offset = self.align();
        self.bytes.resize(offset + values.len() * 4, 0);
        for (bytes, value) in self.bytes[offset..]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(values.iter())
        {
            *bytes = value.to_le_bytes();
        }
        let view = self.push_view(offset, values.len() * 4, Some(ARRAY_BUFFER));
        let width = kind.width();
        let mut accessor = Members::new()
            .with("bufferView", view)
            .with("componentType", FLOAT)
            .with("count", values.len() / width)
            .with("type", kind.name());
        if bounds && !values.is_empty() {
            let mut min = vec![f32::INFINITY; width];
            let mut max = vec![f32::NEG_INFINITY; width];
            for element in values.chunks_exact(width) {
                for (i, &v) in element.iter().enumerate() {
                    min[i] = min[i].min(v);
                    max[i] = max[i].max(v);
                }
            }
            let numbers = |v: Vec<f32>| v.into_iter().map(f64::from).collect::<Vec<_>>();
            accessor = accessor.with("min", numbers(min)).with("max", numbers(max));
        }
        self.accessors.push(accessor.into());
        self.shared.insert(key, self.accessors.len() - 1);
        self.accessors.len() - 1
    }

    /// An accessor of vertex indices: 16-bit when every vertex fits below
    /// 65535 (the largest value is reserved), else 32-bit.
    pub fn indices(&mut self, indices: &[u32], vertices: usize) -> usize {
        let offset = self.align();
        let short = vertices <= 65535;
        let size = if short { 2 } else { 4 };
        self.bytes.resize(offset + indices.len() * size, 0);
        let bytes = &mut self.bytes[offset..];
        if short {
            for (bytes, &index) in bytes.as_chunks_mut::<2>().0.iter_mut().zip(indices) {
                *bytes = (index as u16).to_le_bytes();
            }
        } else {
            for (bytes, &index) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(indices) {
                *bytes = index.to_le_bytes();
            }
        }
        let view = self.push_view(offset, indices.len() * size, Some(ELEMENT_ARRAY_BUFFER));
        let accessor = Members::new()
            .with("bufferView", view)
            .with(
                "componentType",
                if short { UNSIGNED_SHORT } else { UNSIGNED_INT },
            )
            .with("count", indices.len())
            .with("type", "SCALAR");
        self.accessors.push(accessor.into());
        self.accessors.len() - 1
    }
}
