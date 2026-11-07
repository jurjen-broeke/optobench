//! Rays, interactions (Snell, Fresnel, absorption, coatings, scatter) and sequential and non-sequential tracing.
//!
//! Layer L2. See `docs/design.md`, sections Architecture and Optical model.
//! M1 provides the sequential core: rays, vector Snell's law with total internal
//! reflection, a sequential real-ray trace and the paraxial y-nu trace with cardinal
//! points. M2 adds media from `optobench-materials`, Fresnel s/p and power on rays.
//! M3 adds the non-sequential trace ([`nonsequential`]).

#![forbid(unsafe_code)]

pub mod bvh;
pub mod coating;
mod interaction;
pub mod nonsequential;
mod paraxial;
mod ray;
mod sequential;

pub use interaction::{
    FresnelAmplitudes, FresnelPower, Refraction, fresnel_amplitudes, fresnel_power, reflect,
    refract,
};
pub use paraxial::{CardinalPoints, ParaxialRay};
pub use ray::Ray;
pub use sequential::{
    Coating, Interaction, SequentialSurface, SequentialSystem, Status, TraceResult,
};
