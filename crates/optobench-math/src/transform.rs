//! Rotations, rigid transforms and frames.
//!
//! Convention: a [`Transform`] maps coordinates in a local frame to its parent frame,
//! `p_parent = R · p_local + t`. Composition reads like function composition:
//! `(a * b).apply_point(p) == a.apply_point(b.apply_point(p))`.

use core::ops::Mul;

use crate::Vec3;

/// A proper rotation, stored as a 3×3 orthonormal matrix with determinant +1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotation {
    /// Rows of the matrix: `rows[i][j]` is row `i`, column `j`.
    rows: [[f64; 3]; 3],
}

impl Rotation {
    /// The rotation that changes nothing.
    pub const IDENTITY: Self = Self {
        rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    };

    /// Rotation by `angle` radians about `axis`, right-handed (counter-clockwise when
    /// looking down the axis towards the origin). `None` if the axis has zero length.
    ///
    /// Rodrigues' rotation formula: R = I cos θ + (1 − cos θ) a aᵀ + [a]ₓ sin θ.
    pub fn from_axis_angle(axis: Vec3, angle: f64) -> Option<Self> {
        let a = axis.normalized()?;
        let (s, c) = angle.sin_cos();
        let k = 1.0 - c;
        Some(Self {
            rows: [
                [
                    c + k * a.x * a.x,
                    k * a.x * a.y - s * a.z,
                    k * a.x * a.z + s * a.y,
                ],
                [
                    k * a.y * a.x + s * a.z,
                    c + k * a.y * a.y,
                    k * a.y * a.z - s * a.x,
                ],
                [
                    k * a.z * a.x - s * a.y,
                    k * a.z * a.y + s * a.x,
                    c + k * a.z * a.z,
                ],
            ],
        })
    }

    /// The rotation whose columns are the given axes, i.e. the one that maps the local
    /// x, y and z axes onto them. The caller guarantees an orthonormal right-handed set;
    /// [`Frame::new`] builds one safely.
    fn from_columns(x: Vec3, y: Vec3, z: Vec3) -> Self {
        Self {
            rows: [[x.x, y.x, z.x], [x.y, y.y, z.y], [x.z, y.z, z.z]],
        }
    }

    /// Column `j` of the matrix: the image of local axis `j`.
    fn column(&self, j: usize) -> Vec3 {
        Vec3::new(self.rows[0][j], self.rows[1][j], self.rows[2][j])
    }

    /// Rotates a vector.
    pub fn apply(&self, v: Vec3) -> Vec3 {
        let r = &self.rows;
        Vec3::new(
            r[0][0] * v.x + r[0][1] * v.y + r[0][2] * v.z,
            r[1][0] * v.x + r[1][1] * v.y + r[1][2] * v.z,
            r[2][0] * v.x + r[2][1] * v.y + r[2][2] * v.z,
        )
    }

    /// The inverse rotation (the transpose).
    pub fn inverse(&self) -> Self {
        let r = &self.rows;
        Self {
            rows: [
                [r[0][0], r[1][0], r[2][0]],
                [r[0][1], r[1][1], r[2][1]],
                [r[0][2], r[1][2], r[2][2]],
            ],
        }
    }
}

impl Default for Rotation {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mul for Rotation {
    type Output = Self;
    /// `(a * b).apply(v) == a.apply(b.apply(v))`.
    fn mul(self, rhs: Self) -> Self {
        let mut rows = [[0.0; 3]; 3];
        for (i, row) in rows.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                *value = (0..3).map(|k| self.rows[i][k] * rhs.rows[k][j]).sum();
            }
        }
        Self { rows }
    }
}

/// A rigid transform (rotation, then translation) from a local frame to its parent.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Transform {
    /// Orientation of the local axes in the parent frame.
    pub rotation: Rotation,
    /// Position of the local origin in the parent frame, in millimetres.
    pub translation: Vec3,
}

impl Transform {
    /// The transform that changes nothing.
    pub const IDENTITY: Self = Self {
        rotation: Rotation::IDENTITY,
        translation: Vec3::ZERO,
    };

    /// A transform from its rotation and translation.
    pub const fn new(rotation: Rotation, translation: Vec3) -> Self {
        Self {
            rotation,
            translation,
        }
    }

    /// A pure translation.
    pub const fn from_translation(translation: Vec3) -> Self {
        Self::new(Rotation::IDENTITY, translation)
    }

    /// Maps a point from the local frame to the parent frame.
    pub fn apply_point(&self, p: Vec3) -> Vec3 {
        self.rotation.apply(p) + self.translation
    }

    /// Maps a direction from the local frame to the parent frame (no translation).
    pub fn apply_vector(&self, v: Vec3) -> Vec3 {
        self.rotation.apply(v)
    }

    /// The transform from the parent frame back to the local frame.
    pub fn inverse(&self) -> Self {
        let rotation = self.rotation.inverse();
        Self::new(rotation, -rotation.apply(self.translation))
    }
}

impl Mul for Transform {
    type Output = Self;
    /// `(a * b).apply_point(p) == a.apply_point(b.apply_point(p))`.
    fn mul(self, rhs: Self) -> Self {
        Self::new(
            self.rotation * rhs.rotation,
            self.apply_point(rhs.translation),
        )
    }
}

/// A frame: an origin and a right-handed orthonormal basis, given in a parent frame.
/// Each optical element's local +z is its optical axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    transform: Transform,
}

impl Frame {
    /// The parent frame itself.
    pub const PARENT: Self = Self {
        transform: Transform::IDENTITY,
    };

    /// A frame at `origin` whose +z points along `z_axis`. The local +x axis is
    /// `x_hint` made perpendicular to `z_axis` (Gram–Schmidt), and +y completes a
    /// right-handed set. `None` if `z_axis` has zero length or `x_hint` is parallel to it.
    pub fn new(origin: Vec3, z_axis: Vec3, x_hint: Vec3) -> Option<Self> {
        let z = z_axis.normalized()?;
        let perpendicular = x_hint - z * x_hint.dot(z);
        // Reject a zero hint, or one parallel to z up to rounding.
        if perpendicular.length() <= 1e-12 * x_hint.length() {
            return None;
        }
        let x = perpendicular.normalized()?;
        let y = z.cross(x);
        Some(Self {
            transform: Transform::new(Rotation::from_columns(x, y, z), origin),
        })
    }

    /// The frame described by a local-to-parent transform.
    pub const fn from_transform(transform: Transform) -> Self {
        Self { transform }
    }

    /// The local-to-parent transform.
    pub const fn transform(&self) -> Transform {
        self.transform
    }

    /// The origin in the parent frame.
    pub fn origin(&self) -> Vec3 {
        self.transform.translation
    }

    /// The local x, y and z axes in the parent frame.
    pub fn axes(&self) -> [Vec3; 3] {
        let r = &self.transform.rotation;
        [r.column(0), r.column(1), r.column(2)]
    }

    /// Maps a point given in this frame to the parent frame.
    pub fn to_parent_point(&self, p: Vec3) -> Vec3 {
        self.transform.apply_point(p)
    }

    /// Maps a point given in the parent frame to this frame.
    pub fn to_local_point(&self, p: Vec3) -> Vec3 {
        self.transform.inverse().apply_point(p)
    }

    /// Maps a direction given in this frame to the parent frame.
    pub fn to_parent_vector(&self, v: Vec3) -> Vec3 {
        self.transform.apply_vector(v)
    }

    /// Maps a direction given in the parent frame to this frame.
    pub fn to_local_vector(&self, v: Vec3) -> Vec3 {
        self.transform.rotation.inverse().apply(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::FRAC_PI_2;

    const EPS: f64 = 1e-12;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < EPS
    }

    #[test]
    fn quarter_turn_about_z_maps_x_to_y() {
        let r = Rotation::from_axis_angle(Vec3::Z, FRAC_PI_2).expect("non-zero axis");
        assert!(close(r.apply(Vec3::X), Vec3::Y));
        assert!(close(r.apply(Vec3::Y), -Vec3::X));
        assert!(close(r.apply(Vec3::Z), Vec3::Z));
    }

    #[test]
    fn rotation_preserves_length_and_inverse_undoes_it() {
        let r = Rotation::from_axis_angle(Vec3::new(1.0, -2.0, 0.5), 0.7).expect("non-zero axis");
        let v = Vec3::new(0.3, 4.0, -1.2);
        assert!((r.apply(v).length() - v.length()).abs() < EPS);
        assert!(close(r.inverse().apply(r.apply(v)), v));
        assert!(Rotation::from_axis_angle(Vec3::ZERO, 1.0).is_none());
    }

    #[test]
    fn composition_applies_right_operand_first() {
        let a = Transform::new(
            Rotation::from_axis_angle(Vec3::X, 0.4).expect("non-zero axis"),
            Vec3::new(1.0, 2.0, 3.0),
        );
        let b = Transform::new(
            Rotation::from_axis_angle(Vec3::new(0.0, 1.0, 1.0), -1.1).expect("non-zero axis"),
            Vec3::new(-5.0, 0.5, 0.0),
        );
        let p = Vec3::new(0.7, -0.2, 9.0);
        assert!(close(
            (a * b).apply_point(p),
            a.apply_point(b.apply_point(p))
        ));
        assert!(close(
            (a * b).inverse().apply_point((a * b).apply_point(p)),
            p
        ));
        assert!(close(a.inverse().apply_point(a.apply_point(p)), p));
    }

    #[test]
    fn frame_is_right_handed_and_round_trips() {
        let frame = Frame::new(Vec3::new(10.0, 0.0, 5.0), Vec3::new(1.0, 1.0, 0.0), Vec3::Z)
            .expect("valid axes");
        let [x, y, z] = frame.axes();
        assert!((x.dot(y)).abs() < EPS && (y.dot(z)).abs() < EPS && (z.dot(x)).abs() < EPS);
        assert!(close(x.cross(y), z));
        assert!(close(z, Vec3::new(1.0, 1.0, 0.0) * (1.0 / 2f64.sqrt())));
        let p = Vec3::new(1.0, 2.0, 3.0);
        assert!(close(frame.to_local_point(frame.to_parent_point(p)), p));
        assert!(close(frame.to_parent_point(Vec3::ZERO), frame.origin()));
        assert!(close(frame.to_parent_vector(Vec3::Z), z));
        assert!(close(frame.to_local_vector(z), Vec3::Z));
    }

    #[test]
    fn frame_rejects_parallel_hint() {
        assert!(Frame::new(Vec3::ZERO, Vec3::Z, Vec3::new(0.0, 0.0, 2.0)).is_none());
        assert!(Frame::new(Vec3::ZERO, Vec3::ZERO, Vec3::X).is_none());
    }
}
