//! Sequential real-ray trace through an ordered list of surfaces.
//!
//! # Prescription conventions
//!
//! - Surfaces are coaxial on the system z axis. Surface 0's vertex is at z = 0; each
//!   surface's `thickness` is the signed distance along z to the next vertex.
//! - Light enters travelling towards +z in a medium of index `object_index`. After a
//!   mirror it travels towards −z, so thicknesses after an odd number of mirrors are
//!   negative (the usual sign convention).
//! - Indices are given as positive numbers here; the paraxial trace applies the sign
//!   flip at mirrors itself.
//! - Indices are constant (no dispersion) until materials arrive in M2, and power is
//!   unchanged until Fresnel losses and absorption arrive in M2.

use optobench_math::{Transform, Vec3};
use optobench_surfaces::{Aperture, Intersection, Surface};

use crate::interaction::{Refraction, reflect, refract};
use crate::ray::Ray;

/// What a surface does to a ray.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Interaction {
    /// Refraction into the medium after the surface.
    #[default]
    Refract,
    /// Mirror reflection; the medium after the surface is the one the ray returns into.
    Reflect,
}

/// One surface of a sequential system.
#[derive(Debug)]
pub struct SequentialSurface {
    /// The surface shape in its local frame.
    pub surface: Box<dyn Surface>,
    /// The clear aperture.
    pub aperture: Aperture,
    /// What the surface does to a ray.
    pub interaction: Interaction,
    /// Signed distance along z from this vertex to the next one (mm).
    pub thickness: f64,
    /// Refractive index of the medium after the surface (positive).
    pub index_after: f64,
}

impl SequentialSurface {
    /// A refracting surface with no aperture limit.
    pub fn refracting(surface: impl Surface + 'static, thickness: f64, index_after: f64) -> Self {
        Self {
            surface: Box::new(surface),
            aperture: Aperture::Unlimited,
            interaction: Interaction::Refract,
            thickness,
            index_after,
        }
    }

    /// A mirror with no aperture limit; `index` is the medium the ray returns into.
    pub fn reflecting(surface: impl Surface + 'static, thickness: f64, index: f64) -> Self {
        Self {
            surface: Box::new(surface),
            aperture: Aperture::Unlimited,
            interaction: Interaction::Reflect,
            thickness,
            index_after: index,
        }
    }

    /// The same surface with a clear aperture.
    pub fn with_aperture(mut self, aperture: Aperture) -> Self {
        self.aperture = aperture;
        self
    }
}

/// An ordered list of surfaces with the media between them.
#[derive(Debug)]
pub struct SequentialSystem {
    /// Refractive index of the medium before the first surface.
    pub object_index: f64,
    /// The surfaces in the order light meets them.
    pub surfaces: Vec<SequentialSurface>,
}

/// How a ray's trip through the system ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The ray passed every surface.
    Complete,
    /// The ray did not meet surface `surface` at all.
    Missed {
        /// Index of the surface.
        surface: usize,
    },
    /// The ray met surface `surface` outside its clear aperture.
    Vignetted {
        /// Index of the surface.
        surface: usize,
    },
    /// The ray was totally internally reflected at refracting surface `surface`, which a
    /// sequential trace cannot follow.
    TotalInternalReflection {
        /// Index of the surface.
        surface: usize,
    },
}

/// The result of tracing one ray.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceResult {
    /// The ray after the last surface it reached: positioned at that surface, travelling
    /// in its new direction (or as it arrived, if it was stopped there).
    pub ray: Ray,
    /// How the trace ended.
    pub status: Status,
    /// The hit point on each surface reached, in system coordinates. A vignetted ray's
    /// last entry is where it met the blocked surface.
    pub points: Vec<Vec3>,
}

impl SequentialSystem {
    /// Where surface `index`'s local frame sits in the system frame.
    pub fn placement(&self, index: usize) -> Transform {
        let z: f64 = self.surfaces[..index].iter().map(|s| s.thickness).sum();
        Transform::from_translation(Vec3::new(0.0, 0.0, z))
    }

    /// Traces one ray through every surface in order.
    pub fn trace(&self, ray: Ray) -> TraceResult {
        let mut ray = ray;
        let mut points = Vec::with_capacity(self.surfaces.len());
        let mut index_before = self.object_index;
        let mut placement = Transform::IDENTITY;

        for (i, element) in self.surfaces.iter().enumerate() {
            let to_local = placement.inverse();
            let origin = to_local.apply_point(ray.position);
            let direction = to_local.apply_vector(ray.direction);

            let hit = match element
                .aperture
                .intersect(element.surface.as_ref(), origin, direction)
            {
                Intersection::Hit(hit) => hit,
                Intersection::Miss => {
                    return TraceResult {
                        ray,
                        status: Status::Missed { surface: i },
                        points,
                    };
                }
                Intersection::Vignetted(hit) => {
                    ray.optical_path += index_before * hit.distance;
                    ray.position = placement.apply_point(hit.point);
                    points.push(ray.position);
                    return TraceResult {
                        ray,
                        status: Status::Vignetted { surface: i },
                        points,
                    };
                }
            };

            ray.optical_path += index_before * hit.distance;
            ray.position = placement.apply_point(hit.point);
            points.push(ray.position);
            let normal = placement.apply_vector(hit.normal);

            ray.direction = match element.interaction {
                Interaction::Reflect => reflect(ray.direction, normal),
                Interaction::Refract => {
                    match refract(ray.direction, normal, index_before, element.index_after) {
                        Refraction::Refracted(direction) => direction,
                        Refraction::TotalInternalReflection(_) => {
                            return TraceResult {
                                ray,
                                status: Status::TotalInternalReflection { surface: i },
                                points,
                            };
                        }
                    }
                }
            };
            index_before = element.index_after;
            placement = Transform::from_translation(
                placement.translation + Vec3::new(0.0, 0.0, element.thickness),
            );
        }

        TraceResult {
            ray,
            status: Status::Complete,
            points,
        }
    }
}
