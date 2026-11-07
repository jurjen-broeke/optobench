//! A geometric ray carrying power.

use optobench_math::Vec3;

/// A ray: where it is, where it goes, and what it carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// Current position (mm).
    pub position: Vec3,
    /// Unit direction of travel.
    pub direction: Vec3,
    /// Vacuum wavelength (µm).
    pub wavelength: f64,
    /// Power carried (W).
    pub power: f64,
    /// Optical path length accumulated so far (mm): Σ n · distance.
    pub optical_path: f64,
}

impl Ray {
    /// A ray with zero accumulated optical path. The direction is normalised; `None` if it
    /// has zero length.
    pub fn new(position: Vec3, direction: Vec3, wavelength: f64, power: f64) -> Option<Self> {
        Some(Self {
            position,
            direction: direction.normalized()?,
            wavelength,
            power,
            optical_path: 0.0,
        })
    }

    /// The point at signed distance `distance` along the ray.
    pub fn at(&self, distance: f64) -> Vec3 {
        self.position + self.direction * distance
    }
}
