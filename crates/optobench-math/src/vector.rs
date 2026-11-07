//! Three-component vectors.

use core::ops::{Add, Mul, Neg, Sub};

/// A 3D vector of `f64` components, in millimetres when used as a position.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    /// x component.
    pub x: f64,
    /// y component.
    pub y: f64,
    /// z component.
    pub z: f64,
}

impl Vec3 {
    /// The zero vector.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    /// Unit vector along +x.
    pub const X: Self = Self::new(1.0, 0.0, 0.0);
    /// Unit vector along +y.
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);
    /// Unit vector along +z, the optical axis of an element's local frame.
    pub const Z: Self = Self::new(0.0, 0.0, 1.0);

    /// Creates a vector from its components.
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Dot product.
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Cross product.
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Squared Euclidean length.
    pub fn length_squared(self) -> f64 {
        self.dot(self)
    }

    /// Euclidean length.
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// The unit vector in the same direction, or `None` for a zero-length vector.
    pub fn normalized(self) -> Option<Self> {
        let len = self.length();
        (len > 0.0).then(|| self * (1.0 / len))
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Neg for Vec3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-12;

    #[test]
    fn cross_of_x_and_y_is_z() {
        let z = Vec3::new(1.0, 0.0, 0.0).cross(Vec3::new(0.0, 1.0, 0.0));
        assert!((z - Vec3::new(0.0, 0.0, 1.0)).length() < EPS);
    }

    #[test]
    fn normalized_has_unit_length() {
        let n = Vec3::new(3.0, 4.0, 12.0).normalized();
        assert!(n.is_some_and(|v| (v.length() - 1.0).abs() < EPS));
        assert!(Vec3::default().normalized().is_none());
    }
}
