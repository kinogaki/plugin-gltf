//! glTF's space (metres, +Y up) and the project's: `C = k R`, `k` the
//! project's units a metre and `R` the turn from +Y up to the project's up
//! axis.
//!
//! A value is moved into the project's space whole: positions by `C`,
//! directions by `R`, lengths by `k`. A placement relative to a parent
//! moves so that what it places lands where `C` puts it: a group's or a
//! mesh's as `C M C⁻¹`, so it carries no scale of units; a camera's or a
//! light's as `C M / k`, since their values keep the frame they look down
//! (−Z, +Y up) in either space and their lengths are in the project's
//! units already.
use crate::gltf::Transform;
use kinogaki_studio_api::geometry::{Units, UpAxis, Xform};

/// What a placement places: what keeps its frame across spaces or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placed {
    /// A group, a mesh or instances: its value moves with `C`.
    Value,
    /// A camera or a light: its value keeps its frame.
    Frame,
}

/// glTF's space in the project's.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Space {
    /// The project's units a metre.
    k: f64,
    /// Whether the project is Z up: `R` turns (x, y, z) to (x, −z, y).
    z_up: bool,
}

/// Rows of a 3 × 4 affine matrix, as [`Xform`] holds them.
type Rows = [[f64; 4]; 3];

impl Space {
    /// The project's space of `units`.
    pub(crate) fn of(units: Units) -> Space {
        Space {
            k: 1.0 / units.meters_per_unit(),
            z_up: units.up() == UpAxis::Z,
        }
    }

    /// Direction `v` turned to the project's up: `R v`.
    pub(crate) fn turn(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        match self.z_up {
            true => [x, -z, y],
            false => [x, y, z],
        }
    }

    /// Direction `v` turned back to glTF's +Y up: `R⁻¹ v`.
    pub(crate) fn unturn(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        match self.z_up {
            true => [x, z, -y],
            false => [x, y, z],
        }
    }

    /// `metres` in the project's units.
    pub(crate) fn length(&self, metres: f64) -> f64 {
        metres * self.k
    }

    /// `length`, in the project's units, in metres.
    pub(crate) fn metres(&self, length: f64) -> f64 {
        length / self.k
    }

    /// Position `p` in the project's space: `C p`.
    pub(crate) fn point(&self, p: [f64; 3]) -> [f64; 3] {
        self.turn(p).map(|c| c * self.k)
    }

    /// Position `p` of the project's space in glTF's: `C⁻¹ p`.
    pub(crate) fn file_point(&self, p: [f64; 3]) -> [f64; 3] {
        self.unturn(p).map(|c| c / self.k)
    }

    /// A node's placement relative to its parent in the project's space.
    pub(crate) fn placement(&self, transform: &Transform, placed: Placed) -> Xform {
        let m = rows(transform);
        // Column `col` of the linear part: R L R⁻¹ e_col for a value, R L
        // e_col for a frame.
        let cols: [[f64; 3]; 3] = std::array::from_fn(|col| {
            let source = match placed {
                Placed::Value => self.unturn(unit(col)),
                Placed::Frame => unit(col),
            };
            self.turn(linear(&m, source))
        });
        let origin = self.point([m[0][3], m[1][3], m[2][3]]);
        Xform::from_rows(joined(cols, origin))
    }

    /// A placement of the project's space as a glTF node's: the inverse of
    /// [`placement`](Self::placement).
    pub(crate) fn file_placement(&self, xform: &Xform, placed: Placed) -> Rows {
        let x = xform.rows();
        let cols: [[f64; 3]; 3] = std::array::from_fn(|col| {
            let source = match placed {
                Placed::Value => self.turn(unit(col)),
                Placed::Frame => unit(col),
            };
            self.unturn(linear(x, source))
        });
        joined(cols, self.file_point([x[0][3], x[1][3], x[2][3]]))
    }
}

/// Rows of the affine matrix of linear part `cols` (by columns) and
/// translation `origin`.
fn joined(cols: [[f64; 3]; 3], origin: [f64; 3]) -> Rows {
    std::array::from_fn(|row| [cols[0][row], cols[1][row], cols[2][row], origin[row]])
}

/// Unit vector along axis `axis`.
fn unit(axis: usize) -> [f64; 3] {
    let mut v = [0.0; 3];
    v[axis] = 1.0;
    v
}

/// The linear part of `m` applied to `v`.
fn linear(m: &Rows, v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|row| (0..3).map(|col| m[row][col] * v[col]).sum())
}

/// A glTF node's transform as rows.
pub(crate) fn rows(transform: &Transform) -> Rows {
    let m = transform.matrix();
    std::array::from_fn(|row| std::array::from_fn(|col| m[col * 4 + row]))
}

/// `m` as a glTF node's transform: translation, rotation and scale when its
/// linear part is a rotation and a scale along its axes (to a part in
/// 10⁹), else its matrix.
pub(crate) fn transform(m: &Rows) -> Transform {
    let column = |col: usize| [m[0][col], m[1][col], m[2][col]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cols = [column(0), column(1), column(2)];
    let lengths = cols.map(|c| dot(c, c).sqrt());
    let matrix = || {
        let mut out = [0.0; 16];
        for (col, chunk) in out.chunks_mut(4).enumerate() {
            for (row, value) in chunk.iter_mut().enumerate() {
                *value = match (row, col) {
                    (3, 3) => 1.0,
                    (3, _) => 0.0,
                    _ => m[row][col],
                };
            }
        }
        Transform::Matrix(out)
    };
    if lengths.iter().any(|&l| l < 1e-12) {
        return matrix();
    }
    let mut axes = std::array::from_fn::<_, 3, _>(|i| cols[i].map(|c| c / lengths[i]));
    let mut scale = lengths;
    let [a, b, c] = axes;
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    if dot(cross, c) < 0.0 {
        // A mirror: the scale along X takes the sign.
        scale[0] = -scale[0];
        axes[0] = a.map(|v| -v);
    }
    let square = (dot(axes[0], axes[1]).abs())
        .max(dot(axes[1], axes[2]).abs())
        .max(dot(axes[0], axes[2]).abs());
    if square > 1e-9 {
        return matrix();
    }
    // The rotation's quaternion (x, y, z, w), from its columns.
    let r = |row: usize, col: usize| axes[col][row];
    let trace = r(0, 0) + r(1, 1) + r(2, 2);
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (r(2, 1) - r(1, 2)) / s,
            (r(0, 2) - r(2, 0)) / s,
            (r(1, 0) - r(0, 1)) / s,
            0.25 * s,
        ]
    } else if r(0, 0) > r(1, 1) && r(0, 0) > r(2, 2) {
        let s = (1.0 + r(0, 0) - r(1, 1) - r(2, 2)).sqrt() * 2.0;
        [
            0.25 * s,
            (r(0, 1) + r(1, 0)) / s,
            (r(0, 2) + r(2, 0)) / s,
            (r(2, 1) - r(1, 2)) / s,
        ]
    } else if r(1, 1) > r(2, 2) {
        let s = (1.0 + r(1, 1) - r(0, 0) - r(2, 2)).sqrt() * 2.0;
        [
            (r(0, 1) + r(1, 0)) / s,
            0.25 * s,
            (r(1, 2) + r(2, 1)) / s,
            (r(0, 2) - r(2, 0)) / s,
        ]
    } else {
        let s = (1.0 + r(2, 2) - r(0, 0) - r(1, 1)).sqrt() * 2.0;
        [
            (r(0, 2) + r(2, 0)) / s,
            (r(1, 2) + r(2, 1)) / s,
            0.25 * s,
            (r(1, 0) - r(0, 1)) / s,
        ]
    };
    // w ≥ 0, as most writers give it.
    let q = if q[3] < 0.0 { q.map(|v| -v) } else { q };
    Transform::Trs {
        translation: [m[0][3], m[1][3], m[2][3]],
        rotation: q,
        scale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &Rows, b: &Rows) -> bool {
        (0..3).all(|r| (0..4).all(|c| (a[r][c] - b[r][c]).abs() < 1e-12))
    }

    #[test]
    fn placements_move_into_the_projects_space_and_back() {
        let millimetres_z_up = Space::of(Units::new(0.001, UpAxis::Z).unwrap());
        let turned = Transform::Trs {
            translation: [1.0, 2.0, 3.0],
            rotation: [0.0, (0.5f64).sqrt(), 0.0, (0.5f64).sqrt()],
            scale: [2.0, 3.0, 4.0],
        };
        for placed in [Placed::Value, Placed::Frame] {
            let x = millimetres_z_up.placement(&turned, placed);
            assert_eq!(<[f64; 3]>::from(x.origin()), [1000.0, -3000.0, 2000.0]);
            let back = millimetres_z_up.file_placement(&x, placed);
            assert!(close(&back, &rows(&turned)), "{placed:?}: {back:?}");
            let Transform::Trs {
                translation,
                rotation,
                scale,
            } = transform(&back)
            else {
                panic!("decomposes");
            };
            assert!((translation[2] - 3.0).abs() < 1e-12);
            assert!((rotation[1] - (0.5f64).sqrt()).abs() < 1e-12);
            assert!((scale[2] - 4.0).abs() < 1e-12);
        }
        // A mesh at its node keeps its world place: C M C⁻¹ (C p) = C (M p).
        let x = millimetres_z_up.placement(&turned, Placed::Value);
        let p = [0.5, -1.0, 2.0];
        let moved = rows(&turned);
        let file = linear(&moved, p);
        let file = [
            file[0] + moved[0][3],
            file[1] + moved[1][3],
            file[2] + moved[2][3],
        ];
        let at: [f64; 3] = x.apply_point(millimetres_z_up.point(p).into()).into();
        let expected = millimetres_z_up.point(file);
        assert!((0..3).all(|i| (at[i] - expected[i]).abs() < 1e-9), "{at:?}");
    }

    #[test]
    fn a_shear_stays_a_matrix_and_a_mirror_a_negative_scale() {
        let shear = [
            [1.0, 0.5, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        assert!(matches!(transform(&shear), Transform::Matrix(_)));
        let mirror = [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let Transform::Trs { scale, .. } = transform(&mirror) else {
            panic!("decomposes");
        };
        assert_eq!(scale, [-1.0, 1.0, 1.0]);
        assert!(close(&rows(&transform(&mirror)), &mirror));
    }
}
