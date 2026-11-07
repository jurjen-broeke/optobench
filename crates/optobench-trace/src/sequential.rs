//! Sequential real-ray trace through an ordered list of surfaces.
//!
//! # Prescription conventions
//!
//! - Surfaces are coaxial on the system z axis. Surface 0's vertex is at z = 0; each
//!   surface's `thickness` is the signed distance along z to the next vertex.
//! - Light enters travelling towards +z in the object medium. After a mirror it travels
//!   towards −z, so thicknesses after an odd number of mirrors are negative (the usual
//!   sign convention).
//! - Media are materials evaluated at each ray's wavelength. Their indices are positive;
//!   the paraxial trace applies the sign flip at mirrors itself.
//!
//! # Power
//!
//! - An uncoated refracting surface transmits the Fresnel fraction for unpolarised light,
//!   averaged over s and p at each surface. That is exact for the first surface and an
//!   approximation after it, until rays carry Jones vectors (docs/design.md, Optical
//!   model). The reflected part leaves the sequential path and is booked as reflected.
//! - A surface with an ideal coating transmits everything; a mirror reflects everything.
//!   Real coatings arrive with thin-film stacks in M5.
//! - Each medium absorbs along the path by Beer–Lambert, exp(−α·s), with α from the
//!   material's extinction coefficient. Virtual (negative) segments absorb nothing.
//! - Per ray: carried + reflected + absorbed = launched.

use optobench_materials::{Material, MaterialError, Medium};
use optobench_math::{Transform, Vec3};
use optobench_surfaces::{Aperture, Intersection, Surface};

use crate::interaction::{Refraction, fresnel_power, reflect, refract};
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

/// The coating on a refracting surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Coating {
    /// Bare interface: Fresnel reflection losses.
    #[default]
    Uncoated,
    /// A perfect anti-reflection coating: no reflection loss.
    Ideal,
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
    /// The coating, for refracting surfaces.
    pub coating: Coating,
    /// Signed distance along z from this vertex to the next one (mm).
    pub thickness: f64,
    /// The medium after the surface.
    pub medium_after: Medium,
}

impl SequentialSurface {
    /// An uncoated refracting surface with no aperture limit. `medium_after` may be a
    /// constant index (`1.5`) or any material.
    pub fn refracting(
        surface: impl Surface + 'static,
        thickness: f64,
        medium_after: impl Into<Medium>,
    ) -> Self {
        Self {
            surface: Box::new(surface),
            aperture: Aperture::Unlimited,
            interaction: Interaction::Refract,
            coating: Coating::Uncoated,
            thickness,
            medium_after: medium_after.into(),
        }
    }

    /// A perfect mirror with no aperture limit; `medium` is the one the ray returns into.
    pub fn reflecting(
        surface: impl Surface + 'static,
        thickness: f64,
        medium: impl Into<Medium>,
    ) -> Self {
        Self {
            interaction: Interaction::Reflect,
            ..Self::refracting(surface, thickness, medium)
        }
    }

    /// The same surface with a clear aperture.
    pub fn with_aperture(mut self, aperture: Aperture) -> Self {
        self.aperture = aperture;
        self
    }

    /// The same surface with a coating.
    pub fn with_coating(mut self, coating: Coating) -> Self {
        self.coating = coating;
        self
    }
}

/// An ordered list of surfaces with the media between them.
#[derive(Debug)]
pub struct SequentialSystem {
    /// The medium before the first surface.
    pub object_medium: Medium,
    /// The surfaces in the order light meets them.
    pub surfaces: Vec<SequentialSurface>,
}

/// How a ray's trip through the system ended.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// A medium could not be evaluated at the ray's wavelength. `medium` 0 is the object
    /// medium; `medium` i > 0 is the one after surface i − 1.
    MaterialUndefined {
        /// Which medium.
        medium: usize,
        /// Why.
        error: MaterialError,
    },
}

/// The result of tracing one ray.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceResult {
    /// The ray after the last surface it reached: positioned at that surface, travelling
    /// in its new direction (or as it arrived, if it was stopped there). Its `power` is
    /// what it still carries.
    pub ray: Ray,
    /// How the trace ended.
    pub status: Status,
    /// The hit point on each surface reached, in system coordinates. A vignetted ray's
    /// last entry is where it met the blocked surface.
    pub points: Vec<Vec3>,
    /// Power reflected out of the sequential path at refracting surfaces (W).
    pub reflected_power: f64,
    /// Power absorbed in the media (W).
    pub absorbed_power: f64,
}

/// Refractive index and absorption coefficient of each medium at one wavelength:
/// entry 0 is the object medium, entry i the medium after surface i − 1.
type Media = Vec<(f64, f64)>;

impl SequentialSystem {
    /// Where surface `index`'s local frame sits in the system frame.
    pub fn placement(&self, index: usize) -> Transform {
        let z: f64 = self.surfaces[..index].iter().map(|s| s.thickness).sum();
        Transform::from_translation(Vec3::new(0.0, 0.0, z))
    }

    /// Every medium in order: the object medium, then the one after each surface.
    pub(crate) fn media(&self) -> impl Iterator<Item = &Medium> {
        core::iter::once(&self.object_medium).chain(self.surfaces.iter().map(|s| &s.medium_after))
    }

    /// Index and absorption coefficient of every medium at `wavelength`, or the first
    /// medium that cannot be evaluated.
    fn evaluate_media(&self, wavelength: f64) -> Result<Media, (usize, MaterialError)> {
        self.media()
            .enumerate()
            .map(|(i, medium)| {
                let material: &dyn Material = medium.material();
                let n = material.refractive_index(wavelength).map_err(|e| (i, e))?;
                let alpha = material
                    .absorption_coefficient(wavelength)
                    .map_err(|e| (i, e))?;
                Ok((n, alpha))
            })
            .collect()
    }

    /// Traces one ray through every surface in order.
    pub fn trace(&self, ray: Ray) -> TraceResult {
        self.trace_with(ray, true)
    }

    /// Traces one ray as if every clear aperture were unlimited. Ray aiming uses this
    /// while it searches, so a trial ray outside an aperture still reports where it went.
    pub fn trace_ignoring_apertures(&self, ray: Ray) -> TraceResult {
        self.trace_with(ray, false)
    }

    fn trace_with(&self, ray: Ray, apertures: bool) -> TraceResult {
        let mut result = TraceResult {
            ray,
            status: Status::Complete,
            points: Vec::with_capacity(self.surfaces.len()),
            reflected_power: 0.0,
            absorbed_power: 0.0,
        };
        let media = match self.evaluate_media(ray.wavelength) {
            Ok(media) => media,
            Err((medium, error)) => {
                result.status = Status::MaterialUndefined { medium, error };
                return result;
            }
        };
        let mut placement = Transform::IDENTITY;

        for (i, element) in self.surfaces.iter().enumerate() {
            let (index_before, alpha_before) = media[i];
            let (index_after, _) = media[i + 1];
            let to_local = placement.inverse();
            let origin = to_local.apply_point(result.ray.position);
            let direction = to_local.apply_vector(result.ray.direction);

            let aperture = if apertures {
                element.aperture
            } else {
                Aperture::Unlimited
            };
            let (hit, inside) =
                match aperture.intersect(element.surface.as_ref(), origin, direction) {
                    Intersection::Hit(hit) => (hit, true),
                    Intersection::Vignetted(hit) => (hit, false),
                    Intersection::Miss => {
                        result.status = Status::Missed { surface: i };
                        return result;
                    }
                };

            // Propagate to the surface: optical path and bulk absorption.
            let ray = &mut result.ray;
            ray.optical_path += index_before * hit.distance;
            let kept = (-alpha_before * hit.distance.max(0.0)).exp();
            result.absorbed_power += ray.power * (1.0 - kept);
            ray.power *= kept;
            ray.position = placement.apply_point(hit.point);
            result.points.push(ray.position);
            if !inside {
                result.status = Status::Vignetted { surface: i };
                return result;
            }

            let normal = placement.apply_vector(hit.normal);
            match element.interaction {
                Interaction::Reflect => ray.direction = reflect(ray.direction, normal),
                Interaction::Refract => {
                    match refract(ray.direction, normal, index_before, index_after) {
                        Refraction::Refracted(direction) => {
                            if element.coating == Coating::Uncoated {
                                let cos_incidence = normal.dot(ray.direction).abs();
                                let transmitted =
                                    fresnel_power(cos_incidence, index_before, index_after)
                                        .unpolarised_transmittance();
                                result.reflected_power += ray.power * (1.0 - transmitted);
                                ray.power *= transmitted;
                            }
                            ray.direction = direction;
                        }
                        Refraction::TotalInternalReflection(_) => {
                            result.status = Status::TotalInternalReflection { surface: i };
                            return result;
                        }
                    }
                }
            }
            placement = Transform::from_translation(
                placement.translation + Vec3::new(0.0, 0.0, element.thickness),
            );
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use optobench_materials::Tabulated;
    use optobench_surfaces::Plane;

    const LAUNCHED: f64 = 2.0;

    fn launched(position: Vec3, direction: Vec3) -> Ray {
        Ray::new(position, direction, 0.55, LAUNCHED).expect("non-zero direction")
    }

    fn ledger_balances(result: &TraceResult) -> bool {
        let total = result.ray.power + result.reflected_power + result.absorbed_power;
        (total - LAUNCHED).abs() < 1e-12 * LAUNCHED
    }

    #[test]
    fn uncoated_window_transmits_the_fresnel_fraction_twice() {
        // Normal incidence on a plane-parallel window, n = 1.5: T = 0.96 per face
        // (Fresnel, normal incidence), so 0.96² overall, ignoring multiple reflections,
        // which leave the sequential path.
        let window = SequentialSystem {
            object_medium: 1.0.into(),
            surfaces: vec![
                SequentialSurface::refracting(Plane, 5.0, 1.5),
                SequentialSurface::refracting(Plane, 0.0, 1.0),
            ],
        };
        let result = window.trace(launched(Vec3::new(0.0, 0.0, -1.0), Vec3::Z));
        assert_eq!(result.status, Status::Complete);
        assert!((result.ray.power - LAUNCHED * 0.96 * 0.96).abs() < 1e-12);
        assert!(ledger_balances(&result));
    }

    #[test]
    fn ideal_coating_and_matched_index_lose_nothing() {
        let coated = SequentialSystem {
            object_medium: 1.0.into(),
            surfaces: vec![
                SequentialSurface::refracting(Plane, 5.0, 1.5).with_coating(Coating::Ideal),
                SequentialSurface::refracting(Plane, 0.0, 1.5),
            ],
        };
        let result = coated.trace(launched(Vec3::new(0.0, 0.0, -1.0), Vec3::Z));
        assert!((result.ray.power - LAUNCHED).abs() < 1e-15);
        assert!(ledger_balances(&result));
    }

    #[test]
    fn absorbing_medium_follows_beer_lambert() {
        // k = 1e-5 at 0.55 µm: α = 4πk/λ per mm. A 10 mm path keeps exp(−10α); a ray at
        // 30° inside travels 10/cos 30° mm (Born & Wolf, Principles of Optics, 7th ed.,
        // ch. 14).
        let absorbing =
            Tabulated::new(vec![(0.5, 1.0, 1e-5), (0.6, 1.0, 1e-5)]).expect("valid table");
        let alpha = 4.0 * core::f64::consts::PI * 1e-5 / 0.55e-3;
        let system = SequentialSystem {
            object_medium: absorbing.into(),
            surfaces: vec![SequentialSurface::refracting(Plane, 0.0, 1.0)],
        };
        let angle = 30.0_f64.to_radians();
        let direction = Vec3::new(angle.sin(), 0.0, angle.cos());
        let result = system.trace(launched(Vec3::new(0.0, 0.0, -10.0), direction));
        let expected = LAUNCHED * (-alpha * 10.0 / angle.cos()).exp();
        assert!((result.ray.power - expected).abs() < 1e-12);
        assert!(ledger_balances(&result));
    }

    #[test]
    fn medium_outside_its_range_stops_the_ray() {
        let narrow = Tabulated::new(vec![(0.6, 1.5, 0.0), (0.7, 1.5, 0.0)]).expect("valid table");
        let system = SequentialSystem {
            object_medium: 1.0.into(),
            surfaces: vec![SequentialSurface::refracting(Plane, 1.0, narrow)],
        };
        let result = system.trace(launched(Vec3::ZERO, Vec3::Z));
        assert!(matches!(
            result.status,
            Status::MaterialUndefined { medium: 1, .. }
        ));
    }
}
