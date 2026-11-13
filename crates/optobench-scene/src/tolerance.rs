//! Tolerances (M14): the errors a built bench may have, each a random value within a
//! limit. They are part of a design, like its DOFs; `optobench_optimize::tolerance`
//! draws and applies them.

use crate::dof::{Motion, Target};

/// How a tolerance's error is distributed within ±limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribution {
    /// Evenly over [−limit, limit].
    Uniform,
    /// Normal with σ = limit / 3, truncated at ±limit.
    Normal,
    /// −limit or +limit, equally likely (the worst case of each).
    End,
}

/// What a tolerance changes.
#[derive(Debug, Clone, PartialEq)]
pub enum ToleranceKind {
    /// The target placed off by a slide (mm) or a tilt (radians), in the scene frame at the
    /// nominal pose, on top of its DOFs; never adjusted.
    Placement(Motion),
    /// An error added to a DOF's value (an adjustment that cannot be set exactly).
    Dof(String),
    /// A lens surface's radius of curvature (surface from 0): by the error in mm, or by
    /// the error as a fraction of the radius.
    Radius {
        /// The surface, from 0.
        surface: usize,
        /// The error is a fraction of the radius.
        relative: bool,
    },
    /// A lens's centre thickness between two surfaces (gap from 0), mm.
    Thickness {
        /// The gap, from 0.
        gap: usize,
    },
    /// A lens material's refractive index (gap from 0), Δn.
    Index {
        /// The gap, from 0.
        gap: usize,
    },
    /// An ideal lens's focal length: by the error in mm, or as a fraction.
    FocalLength {
        /// The error is a fraction of the focal length.
        relative: bool,
    },
}

/// One tolerance.
#[derive(Debug, Clone, PartialEq)]
pub struct Tolerance {
    /// Its name, unique in the scene.
    pub name: String,
    /// What it applies to.
    pub target: Target,
    /// What it changes.
    pub kind: ToleranceKind,
    /// The limit (mm, radians, a fraction or Δn), positive.
    pub limit: f64,
    /// How the error is distributed.
    pub distribution: Distribution,
}
