//! Aperture stop, pupils, fields and ray aiming.
//!
//! The entrance and exit pupils are the paraxial images of the aperture stop in object
//! and image space (Hecht, Optics, 5th ed., ch. 5, stops). With the stop's paraxial
//! heights linear in a ray's input height y and reduced angle nu, h = a·y + b·nu:
//!
//! - every ray through the stop centre comes from the axial point z_E = b·n/a, the
//!   entrance pupil, and
//! - a collimated beam of radius r_stop/|a| exactly fills the stop: the entrance pupil
//!   radius.
//!
//! The exit pupil is where the paraxial chief ray crosses the axis in image space.

use optobench_materials::MaterialError;
use optobench_math::Vec3;
use optobench_surfaces::Aperture;
use optobench_trace::{Ray, SequentialSystem, Status, TraceResult};

/// The direction of the incoming collimated beam, as angles from the axis in the x–z
/// and y–z planes (radians). The beam travels along (tan x, tan y, 1).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Field {
    /// Angle in the x–z plane (radians).
    pub x: f64,
    /// Angle in the y–z plane (radians).
    pub y: f64,
}

impl Field {
    /// The on-axis field.
    pub const AXIS: Self = Self { x: 0.0, y: 0.0 };

    /// A field in the y–z (tangential) plane.
    pub const fn tangential(angle: f64) -> Self {
        Self { x: 0.0, y: angle }
    }

    /// Unit direction of the beam.
    pub fn direction(&self) -> Vec3 {
        Vec3::new(self.x.tan(), self.y.tan(), 1.0)
            .normalized()
            .unwrap_or(Vec3::Z)
    }
}

/// How rays are aimed at the pupil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Aiming {
    /// Rays pass through the paraxial entrance pupil at (px, py) · its radius. Fast;
    /// accurate when pupil aberrations are small.
    Paraxial,
    /// Rays are aimed so they cross the real stop at (px, py) · its radius, by Newton's
    /// method on the start point. The reference for comparisons.
    #[default]
    Real,
}

/// Why an analysis could not run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnalysisError {
    /// The stop index is past the last surface, or the system has no image surface.
    NoSuchStop,
    /// The stop surface has no circular clear aperture to size the pupil.
    StopWithoutAperture,
    /// The stop is not imaged into object space at finite size (a = 0).
    EntrancePupilUndefined,
    /// The exit pupil is at infinity (image-space telecentric); a reference sphere cannot
    /// be centred on it yet.
    ExitPupilAtInfinity,
    /// A medium could not be evaluated at the analysis wavelength.
    Material(MaterialError),
    /// The chief ray did not reach the image plane.
    ChiefRayFailed(Status),
    /// Real ray aiming did not converge.
    AimingFailed,
}

impl From<MaterialError> for AnalysisError {
    fn from(error: MaterialError) -> Self {
        AnalysisError::Material(error)
    }
}

/// Paraxial pupil data. Positions are z coordinates in the system frame (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pupils {
    /// Entrance pupil position.
    pub entrance_position: f64,
    /// Entrance pupil radius.
    pub entrance_radius: f64,
    /// Exit pupil position.
    pub exit_position: f64,
    /// Clear radius of the stop surface.
    pub stop_radius: f64,
}

/// A ray that reached the image plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageRay {
    /// Where it meets the image plane (system frame).
    pub point: Vec3,
    /// Its unit direction in image space.
    pub direction: Vec3,
    /// Optical path from the incoming plane wavefront through the entrance pupil centre
    /// to the image plane (mm).
    pub optical_path: f64,
}

/// A sequential system set up for image analysis: stop, wavelength, aiming, pupils.
#[derive(Debug)]
pub struct Analysis<'a> {
    system: &'a SequentialSystem,
    stop: usize,
    wavelength: f64,
    aiming: Aiming,
    pupils: Pupils,
    object_index: f64,
    image_index: f64,
}

/// Ray aiming limits: the stop hit must be within this distance of its target (mm).
const AIMING_TOLERANCE: f64 = 1e-10;
const AIMING_ITERATIONS: usize = 30;

impl<'a> Analysis<'a> {
    /// Sets up an analysis of `system` with the aperture stop at surface `stop`, at
    /// `wavelength` (µm).
    pub fn new(
        system: &'a SequentialSystem,
        stop: usize,
        wavelength: f64,
        aiming: Aiming,
    ) -> Result<Self, AnalysisError> {
        let last = system
            .surfaces
            .len()
            .checked_sub(1)
            .ok_or(AnalysisError::NoSuchStop)?;
        if stop >= last {
            return Err(AnalysisError::NoSuchStop);
        }
        let Aperture::Circular {
            semi_diameter: stop_radius,
        } = system.surfaces[stop].aperture
        else {
            return Err(AnalysisError::StopWithoutAperture);
        };

        let object_index = system
            .object_medium
            .material()
            .refractive_index(wavelength)?;
        let image_index = system.surfaces[last - 1]
            .medium_after
            .material()
            .refractive_index(wavelength)?;

        // Stop heights per unit input height (a) and per unit reduced angle (b).
        let a = system.paraxial_trace(1.0, 0.0, wavelength)?[stop].height;
        let b = system.paraxial_trace(0.0, 1.0, wavelength)?[stop].height;
        if a == 0.0 {
            return Err(AnalysisError::EntrancePupilUndefined);
        }
        let entrance_position = b * object_index / a;
        let entrance_radius = stop_radius / a.abs();

        // Paraxial chief ray with unit reduced angle, through the entrance pupil centre.
        let chief = system.paraxial_trace(-entrance_position / object_index, 1.0, wavelength)?;
        let at_image = chief[last];
        if at_image.reduced_angle == 0.0 {
            return Err(AnalysisError::ExitPupilAtInfinity);
        }
        let image_z = system.placement(last).translation.z;
        let signed_image_index = system.image_index(wavelength)?;
        let exit_position = image_z - at_image.height * signed_image_index / at_image.reduced_angle;

        Ok(Self {
            system,
            stop,
            wavelength,
            aiming,
            pupils: Pupils {
                entrance_position,
                entrance_radius,
                exit_position,
                stop_radius,
            },
            object_index,
            image_index,
        })
    }

    /// The pupils.
    pub fn pupils(&self) -> Pupils {
        self.pupils
    }

    /// The analysis wavelength (µm).
    pub fn wavelength(&self) -> f64 {
        self.wavelength
    }

    /// Index of the image space (positive).
    pub fn image_index(&self) -> f64 {
        self.image_index
    }

    /// The image plane's z coordinate in the system frame.
    pub fn image_z(&self) -> f64 {
        self.system
            .placement(self.system.surfaces.len() - 1)
            .translation
            .z
    }

    fn ray_from(&self, start: Vec3, direction: Vec3) -> Ray {
        Ray {
            position: start,
            direction,
            wavelength: self.wavelength,
            power: 1.0,
            optical_path: 0.0,
        }
    }

    /// Where a trial ray from `start` crosses the stop surface, in the stop's frame.
    fn stop_hit(&self, start: Vec3, direction: Vec3) -> Option<(f64, f64)> {
        let result = self
            .system
            .trace_ignoring_apertures(self.ray_from(start, direction));
        let point = *result.points.get(self.stop)?;
        let local = self
            .system
            .placement(self.stop)
            .inverse()
            .apply_point(point);
        Some((local.x, local.y))
    }

    /// The start point, on the entrance pupil plane, of the ray for field `field` at
    /// pupil coordinates (px, py).
    pub fn aim(&self, field: Field, px: f64, py: f64) -> Result<Vec3, AnalysisError> {
        let direction = field.direction();
        let z = self.pupils.entrance_position;
        let r = self.pupils.entrance_radius;
        let paraxial = Vec3::new(px * r, py * r, z);
        if self.aiming == Aiming::Paraxial {
            return Ok(paraxial);
        }
        // Newton on the start point (sx, sy) so the stop hit equals the target.
        let target = (px * self.pupils.stop_radius, py * self.pupils.stop_radius);
        let step = 1e-7 * r.max(1.0);
        let (mut sx, mut sy) = (paraxial.x, paraxial.y);
        for _ in 0..AIMING_ITERATIONS {
            let at = |x: f64, y: f64| self.stop_hit(Vec3::new(x, y, z), direction);
            let (hx, hy) = at(sx, sy).ok_or(AnalysisError::AimingFailed)?;
            let (ex, ey) = (hx - target.0, hy - target.1);
            if ex.hypot(ey) <= AIMING_TOLERANCE {
                return Ok(Vec3::new(sx, sy, z));
            }
            let (ax, ay) = at(sx + step, sy).ok_or(AnalysisError::AimingFailed)?;
            let (bx, by) = at(sx, sy + step).ok_or(AnalysisError::AimingFailed)?;
            // Jacobian columns: d(hit)/d(sx), d(hit)/d(sy).
            let (j11, j21) = ((ax - hx) / step, (ay - hy) / step);
            let (j12, j22) = ((bx - hx) / step, (by - hy) / step);
            let determinant = j11 * j22 - j12 * j21;
            if determinant == 0.0 || !determinant.is_finite() {
                return Err(AnalysisError::AimingFailed);
            }
            sx -= (j22 * ex - j12 * ey) / determinant;
            sy -= (-j21 * ex + j11 * ey) / determinant;
        }
        Err(AnalysisError::AimingFailed)
    }

    /// Traces the ray for `field` at pupil coordinates (px, py), with apertures.
    pub fn trace(&self, field: Field, px: f64, py: f64) -> Result<TraceResult, AnalysisError> {
        let start = self.aim(field, px, py)?;
        Ok(self.system.trace(self.ray_from(start, field.direction())))
    }

    /// Traces the ray for `field` at (px, py) to the image plane. `Ok(None)` if it is
    /// vignetted, misses a surface or is totally internally reflected on the way.
    pub fn image_ray(
        &self,
        field: Field,
        px: f64,
        py: f64,
    ) -> Result<Option<ImageRay>, AnalysisError> {
        let start = self.aim(field, px, py)?;
        let direction = field.direction();
        let result = self.system.trace(self.ray_from(start, direction));
        if result.status != Status::Complete {
            return Ok(None);
        }
        // Optical path from the plane wavefront through the entrance pupil centre: the
        // start point lies (start − centre)·d beyond that plane along the ray.
        let centre = Vec3::new(0.0, 0.0, self.pupils.entrance_position);
        let offset = self.object_index * (start - centre).dot(direction);
        Ok(Some(ImageRay {
            point: result.ray.position,
            direction: result.ray.direction,
            optical_path: result.ray.optical_path + offset,
        }))
    }

    /// The chief ray (pupil centre) at the image plane.
    pub fn chief_ray(&self, field: Field) -> Result<ImageRay, AnalysisError> {
        let start = self.aim(field, 0.0, 0.0)?;
        let result = self.system.trace(self.ray_from(start, field.direction()));
        if result.status != Status::Complete {
            return Err(AnalysisError::ChiefRayFailed(result.status));
        }
        self.image_ray(field, 0.0, 0.0)?
            .ok_or(AnalysisError::ChiefRayFailed(result.status))
    }
}
