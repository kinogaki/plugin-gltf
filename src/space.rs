//! glTF's space (metres, +Y up) and the project's, through the SDK's
//! [`Conversion`] ([`into_project`]): positions by `C`, directions by its
//! turn, lengths by its scale; a node's placement as a value's (`C M C⁻¹`,
//! a group, a mesh) or a frame's (a camera, a light: its value keeps the
//! frame it looks down), as [`Placed`] says. A writer converts back through
//! the inverse. And glTF's node transforms as placements' rows and back.
use crate::gltf::Transform;
use kinogaki_studio_api::format::Conversion;
use kinogaki_studio_api::geometry::{Units, Xform};

/// What a placement places: what keeps its frame across spaces or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placed {
    /// A group, a mesh or instances: its value moves with `C`.
    Value,
    /// A camera or a light: its value keeps its frame.
    Frame,
}

/// Rows of a 3 × 4 affine matrix, as [`Xform`] holds them.
type Rows = [[f64; 4]; 3];

/// glTF's space into the project's of `units`.
pub(crate) fn into_project(units: Units) -> Conversion {
    Conversion::new(Units::METERS, units)
}

/// Placement `m`, of what `placed` says, through `conversion`.
pub(crate) fn placed(conversion: &Conversion, m: &Xform, placed: Placed) -> Xform {
    match placed {
        Placed::Value => conversion.placement(m),
        Placed::Frame => conversion.frame(m),
    }
}

/// A node's transform as a placement.
pub(crate) fn xform(transform: &Transform) -> Xform {
    Xform::from_rows(rows(transform))
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
        use kinogaki_studio_api::geometry::UpAxis;
        let into = into_project(Units::new(0.001, UpAxis::Z).unwrap());
        let turned = Transform::Trs {
            translation: [1.0, 2.0, 3.0],
            rotation: [0.0, (0.5f64).sqrt(), 0.0, (0.5f64).sqrt()],
            scale: [2.0, 3.0, 4.0],
        };
        for how in [Placed::Value, Placed::Frame] {
            let x = placed(&into, &xform(&turned), how);
            assert_eq!(<[f64; 3]>::from(x.origin()), [1000.0, -3000.0, 2000.0]);
            let back = *placed(&into.inverse(), &x, how).rows();
            assert!(close(&back, &rows(&turned)), "{how:?}: {back:?}");
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
