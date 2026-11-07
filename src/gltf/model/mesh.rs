//! Meshes: primitives of points, lines or triangles over vertex attributes.
use std::sync::Arc;

/// A mesh: its primitives, each drawn with its own material.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub name: String,
    pub primitives: Vec<Primitive>,
}

/// One primitive: corners into its vertices, in groups of its topology.
#[derive(Clone, Debug, PartialEq)]
pub struct Primitive {
    pub topology: Topology,
    /// Vertex indices, one per corner: a multiple of 1, 2 or 3 of them. A
    /// file's strips, fans and loops arrive as lists, and a primitive without
    /// indices as `0..count`. Lists a file shares between primitives stay
    /// shared (one allocation).
    pub indices: Arc<[u32]>,
    /// The vertex attributes, every one with the same vertex count:
    /// `POSITION`, `NORMAL`, `TANGENT`, `TEXCOORD_n`, `COLOR_n` and
    /// application ones (`_NAME`), in the file's order.
    pub attributes: Vec<Attribute>,
    pub material: Option<usize>,
}

/// How a primitive's corners group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topology {
    Points,
    /// Pairs of corners.
    Lines,
    /// Triples of corners, counter-clockwise when seen from the front.
    Triangles,
}

impl Topology {
    /// Corners per element.
    pub fn corners(self) -> usize {
        match self {
            Topology::Points => 1,
            Topology::Lines => 2,
            Topology::Triangles => 3,
        }
    }
}

/// One vertex attribute: `kind.width()` numbers per vertex.
///
/// Values are `f32` whatever the file stores: normalized integers as glTF
/// defines them (`u8` 255 is 1.0), other integers as their values. Values a
/// file shares between primitives stay shared (one allocation).
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub kind: Kind,
    pub values: Arc<[f32]>,
}

impl Attribute {
    /// The number of vertices.
    pub fn count(&self) -> usize {
        self.values.len() / self.kind.width()
    }
}

impl Primitive {
    /// The attribute named `name`.
    pub fn attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }

    /// The number of vertices (0 without attributes).
    pub fn vertex_count(&self) -> usize {
        self.attributes.first().map_or(0, Attribute::count)
    }
}

/// An accessor's element: a scalar, a vector or a column-major matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Scalar,
    Vec2,
    Vec3,
    Vec4,
    Mat2,
    Mat3,
    Mat4,
}

impl Kind {
    /// Numbers per element.
    pub fn width(self) -> usize {
        match self {
            Kind::Scalar => 1,
            Kind::Vec2 => 2,
            Kind::Vec3 => 3,
            Kind::Vec4 | Kind::Mat2 => 4,
            Kind::Mat3 => 9,
            Kind::Mat4 => 16,
        }
    }

    /// Numbers per matrix column (the element's width for others).
    pub(crate) fn rows(self) -> usize {
        match self {
            Kind::Mat2 => 2,
            Kind::Mat3 => 3,
            Kind::Mat4 => 4,
            other => other.width(),
        }
    }

    /// glTF's name (`"VEC3"`).
    pub fn name(self) -> &'static str {
        match self {
            Kind::Scalar => "SCALAR",
            Kind::Vec2 => "VEC2",
            Kind::Vec3 => "VEC3",
            Kind::Vec4 => "VEC4",
            Kind::Mat2 => "MAT2",
            Kind::Mat3 => "MAT3",
            Kind::Mat4 => "MAT4",
        }
    }

    pub(crate) fn named(name: &str) -> Option<Kind> {
        [
            Kind::Scalar,
            Kind::Vec2,
            Kind::Vec3,
            Kind::Vec4,
            Kind::Mat2,
            Kind::Mat3,
            Kind::Mat4,
        ]
        .into_iter()
        .find(|kind| kind.name() == name)
    }
}
