//! Vectors, frames, transforms, root finding and sampling for optobench.
//!
//! Layer L0: no I/O, no dependencies. Units inside the engine are millimetres,
//! radians, micrometres for wavelength and watts for power. Degrees appear only at the
//! boundary (input files, reports); convert with [`radians_from_degrees`] and
//! [`degrees_from_radians`].
//! See `docs/design.md`, section Architecture.

#![forbid(unsafe_code)]

mod complex;
mod linear;
pub mod random;
mod root;
mod transform;
mod vector;

pub use complex::Complex;
pub use linear::{LeastSquaresError, least_squares};
pub use root::{Root, RootError, find_root, find_root_bracketed};
pub use transform::{Frame, Rotation, Transform};
pub use vector::Vec3;

/// Converts an angle in degrees to radians. Use only at the boundary.
pub fn radians_from_degrees(degrees: f64) -> f64 {
    degrees.to_radians()
}

/// Converts an angle in radians to degrees. Use only at the boundary.
pub fn degrees_from_radians(radians: f64) -> f64 {
    radians.to_degrees()
}
