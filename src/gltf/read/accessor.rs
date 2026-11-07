//! Buffer views and accessors: typed elements out of a buffer's bytes,
//! strided or packed, sparse or dense, normalized or not.
use super::fields::Obj;
use super::{Reader, Result};
use crate::gltf::GltfError;
use crate::gltf::model::Kind;
use std::borrow::Cow;
use std::sync::Arc;

/// A buffer view: a range of a buffer, with a stride for vertex data.
pub(super) struct View {
    pub buffer: usize,
    pub offset: usize,
    pub length: usize,
    pub stride: Option<usize>,
}

impl View {
    pub fn read(view: &Obj<'_, '_>, buffers: &[Cow<'_, [u8]>]) -> Result<View> {
        let buffer = view.required_index("buffer", buffers.len())?;
        let (offset, length) = (
            view.count("byteOffset", 0)?,
            view.required_count("byteLength")?,
        );
        if offset
            .checked_add(length)
            .is_none_or(|end| end > buffers[buffer].len())
        {
            return Err(view.error(
                "byteLength",
                format!("{offset} + {length} runs past buffer {buffer}"),
            ));
        }
        let stride = view.opt_count("byteStride")?;
        if stride.is_some_and(|s| !(4..=252).contains(&s) || s % 4 != 0) {
            return Err(view.error("byteStride", "outside 4–252 or not a multiple of 4"));
        }
        Ok(View {
            buffer,
            offset,
            length,
            stride,
        })
    }
}

/// An accessor's component type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Component {
    I8,
    U8,
    I16,
    U16,
    U32,
    F32,
}

impl Component {
    fn coded(code: usize) -> Option<Component> {
        Some(match code {
            5120 => Component::I8,
            5121 => Component::U8,
            5122 => Component::I16,
            5123 => Component::U16,
            5125 => Component::U32,
            5126 => Component::F32,
            _ => return None,
        })
    }

    pub fn size(self) -> usize {
        match self {
            Component::I8 | Component::U8 => 1,
            Component::I16 | Component::U16 => 2,
            Component::U32 | Component::F32 => 4,
        }
    }
}

/// An accessor as declared; decoded on use.
pub(super) struct Accessor {
    at: String,
    kind: Kind,
    component: Component,
    normalized: bool,
    count: usize,
    view: Option<usize>,
    offset: usize,
    sparse: Option<Sparse>,
}

struct Sparse {
    count: usize,
    indices: (usize, usize, Component),
    values: (usize, usize),
}

impl Accessor {
    pub fn read(accessor: &Obj<'_, '_>, views: usize) -> Result<Accessor> {
        let code = accessor.required_count("componentType")?;
        let component = Component::coded(code).ok_or_else(|| {
            accessor.error("componentType", format!("{code} is no component type"))
        })?;
        let kind = accessor
            .opt_str("type")?
            .ok_or_else(|| accessor.error("type", "missing"))?;
        let kind = Kind::named(kind)
            .ok_or_else(|| accessor.error("type", format!("{kind:?} is no accessor type")))?;
        let normalized = accessor.flag("normalized", false)?;
        if normalized && matches!(component, Component::U32 | Component::F32) {
            return Err(accessor.error("normalized", "only 8- and 16-bit integers normalize"));
        }
        let sparse = match accessor.object("sparse")? {
            None => None,
            Some(sparse) => {
                let indices = sparse
                    .object("indices")?
                    .ok_or_else(|| sparse.error("indices", "missing"))?;
                let values = sparse
                    .object("values")?
                    .ok_or_else(|| sparse.error("values", "missing"))?;
                let code = indices.required_count("componentType")?;
                let component = Component::coded(code)
                    .filter(|c| matches!(c, Component::U8 | Component::U16 | Component::U32))
                    .ok_or_else(|| {
                        indices.error("componentType", format!("{code} is no index type"))
                    })?;
                Some(Sparse {
                    count: sparse.required_count("count")?,
                    indices: (
                        indices.required_index("bufferView", views)?,
                        indices.count("byteOffset", 0)?,
                        component,
                    ),
                    values: (
                        values.required_index("bufferView", views)?,
                        values.count("byteOffset", 0)?,
                    ),
                })
            }
        };
        Ok(Accessor {
            kind,
            component,
            normalized,
            count: accessor.required_count("count")?,
            view: accessor.index("bufferView", views)?,
            offset: accessor.count("byteOffset", 0)?,
            sparse,
            at: accessor.at.clone(),
        })
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

impl Reader<'_, '_> {
    /// Accessor `index`'s numbers as `f32`, shared with every earlier and
    /// later use of the same accessor.
    pub(super) fn floats(&mut self, index: usize) -> Result<Arc<[f32]>> {
        if let Some(values) = &self.decoded[index] {
            return Ok(values.clone());
        }
        let values: Arc<[f32]> = self.unshared_floats(index)?.into();
        self.decoded[index] = Some(values.clone());
        Ok(values)
    }

    /// Accessor `index`'s numbers as `f32`, decoded anew.
    pub(super) fn unshared_floats(&mut self, index: usize) -> Result<Vec<f32>> {
        let normalized = self.accessors[index].normalized;
        match (self.accessors[index].component, normalized) {
            (Component::F32, _) => {
                self.gather(index, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            }
            (Component::I8, true) => {
                self.gather(index, |b| (f32::from(b[0] as i8) / 127.0).max(-1.0))
            }
            (Component::U8, true) => self.gather(index, |b| f32::from(b[0]) / 255.0),
            (Component::I16, true) => self.gather(index, |b| {
                (f32::from(i16::from_le_bytes([b[0], b[1]])) / 32767.0).max(-1.0)
            }),
            (Component::U16, true) => self.gather(index, |b| {
                f32::from(u16::from_le_bytes([b[0], b[1]])) / 65535.0
            }),
            (Component::I8, false) => self.gather(index, |b| f32::from(b[0] as i8)),
            (Component::U8, false) => self.gather(index, |b| f32::from(b[0])),
            (Component::I16, false) => {
                self.gather(index, |b| f32::from(i16::from_le_bytes([b[0], b[1]])))
            }
            (Component::U16, false) => {
                self.gather(index, |b| f32::from(u16::from_le_bytes([b[0], b[1]])))
            }
            (Component::U32, false) => self.gather(index, |b| {
                u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32
            }),
            (Component::U32, true) => unreachable!("refused when declared"),
        }
    }

    /// Accessor `index`'s unsigned integers (vertex indices).
    pub(super) fn integers(&mut self, index: usize) -> Result<Vec<u32>> {
        let accessor = &self.accessors[index];
        if accessor.kind != Kind::Scalar || accessor.normalized {
            return Err(GltfError::malformed(
                &accessor.at,
                "indices need unnormalized scalars",
            ));
        }
        match accessor.component {
            Component::U8 => self.gather(index, |b| u32::from(b[0])),
            Component::U16 => self.gather(index, |b| u32::from(u16::from_le_bytes([b[0], b[1]]))),
            Component::U32 => self.gather(index, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            _ => Err(GltfError::malformed(
                &accessor.at,
                "indices need unsigned integers",
            )),
        }
    }

    /// Every component of accessor `index` through `read`, which takes the
    /// component's bytes: the dense elements (zeros without a buffer view),
    /// then the sparse ones over them.
    fn gather<T: Copy + Default>(
        &mut self,
        index: usize,
        read: impl Fn(&[u8]) -> T + Copy,
    ) -> Result<Vec<T>> {
        let accessor = &self.accessors[index];
        let numbers = accessor.count.checked_mul(accessor.kind.width());
        let bytes = numbers.and_then(|n| n.checked_mul(size_of::<T>()));
        let at = accessor.at.clone();
        self.spend(bytes.map_or(u64::MAX, |b| b as u64), &at)?;
        let accessor = &self.accessors[index];
        let layout = Layout::of(accessor);
        let mut values = match accessor.view {
            Some(view) => self.elements(
                &at,
                view,
                accessor.offset,
                accessor.count,
                &layout,
                true,
                read,
            )?,
            None => vec![T::default(); numbers.unwrap_or(0)],
        };
        if let Some(sparse) = &accessor.sparse {
            if sparse.count > accessor.count {
                return Err(GltfError::malformed(
                    &at,
                    "more sparse elements than elements",
                ));
            }
            let (view, offset, component) = sparse.indices;
            let list = Layout {
                size: component.size(),
                rows: 1,
                columns: 1,
                column: component.size(),
            };
            let targets: Vec<u32> = match component {
                Component::U8 => {
                    self.elements(&at, view, offset, sparse.count, &list, false, |b| {
                        u32::from(b[0])
                    })?
                }
                Component::U16 => {
                    self.elements(&at, view, offset, sparse.count, &list, false, |b| {
                        u32::from(u16::from_le_bytes([b[0], b[1]]))
                    })?
                }
                _ => self.elements(&at, view, offset, sparse.count, &list, false, |b| {
                    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
                })?,
            };
            let (view, offset) = sparse.values;
            let replacements =
                self.elements(&at, view, offset, sparse.count, &layout, false, read)?;
            let width = layout.rows * layout.columns;
            for (target, replacement) in targets.iter().zip(replacements.chunks_exact(width)) {
                let target = *target as usize;
                if target >= accessor.count {
                    return Err(GltfError::malformed(
                        &at,
                        format!("sparse index {target} is past {} elements", accessor.count),
                    ));
                }
                values[target * width..(target + 1) * width].copy_from_slice(replacement);
            }
        }
        Ok(values)
    }

    /// `count` elements of `layout` from buffer view `view` at `offset`, at
    /// the view's stride when `strided`, else packed.
    #[allow(clippy::too_many_arguments)]
    fn elements<T>(
        &self,
        at: &str,
        view: usize,
        offset: usize,
        count: usize,
        layout: &Layout,
        strided: bool,
        read: impl Fn(&[u8]) -> T,
    ) -> Result<Vec<T>> {
        let element = layout.element();
        let stride = match self.views[view].stride {
            Some(stride) if strided => stride,
            _ => element,
        };
        if stride < element {
            return Err(GltfError::malformed(
                at,
                format!("a {element}-byte element in a {stride}-byte stride"),
            ));
        }
        let bytes = self.view(view);
        let end = match count {
            0 => Some(offset),
            _ => (count - 1)
                .checked_mul(stride)
                .and_then(|s| s.checked_add(offset + element)),
        };
        if end.is_none_or(|end| end > bytes.len()) {
            return Err(GltfError::malformed(
                at,
                format!("{count} elements run past buffer view {view}"),
            ));
        }
        if stride == element && layout.column == layout.rows * layout.size {
            let packed = &bytes[offset..offset + count * element];
            return Ok(packed.chunks_exact(layout.size).map(read).collect());
        }
        let mut values = Vec::with_capacity(count * layout.rows * layout.columns);
        for e in 0..count {
            let start = offset + e * stride;
            for c in 0..layout.columns {
                for r in 0..layout.rows {
                    let at = start + c * layout.column + r * layout.size;
                    values.push(read(&bytes[at..at + layout.size]));
                }
            }
        }
        Ok(values)
    }
}

/// Where an element's components are: matrix columns of 1- and 2-byte
/// components start on four-byte boundaries.
struct Layout {
    size: usize,
    rows: usize,
    columns: usize,
    /// Bytes from one column to the next.
    column: usize,
}

impl Layout {
    fn of(accessor: &Accessor) -> Layout {
        let (size, rows) = (accessor.component.size(), accessor.kind.rows());
        let matrix = matches!(accessor.kind, Kind::Mat2 | Kind::Mat3 | Kind::Mat4);
        let column = if matrix {
            (rows * size).next_multiple_of(4)
        } else {
            rows * size
        };
        Layout {
            size,
            rows,
            columns: accessor.kind.width() / rows,
            column,
        }
    }

    /// Bytes from one packed element to the next.
    fn element(&self) -> usize {
        self.column * self.columns
    }
}
