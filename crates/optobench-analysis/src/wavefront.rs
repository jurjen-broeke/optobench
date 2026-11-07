//! Optical path difference against a reference sphere.
//!
//! The reference sphere is centred on the chief ray's image point P′ and passes through
//! the point E where the chief ray crosses the exit pupil plane. Each ray's optical path
//! is followed from the incoming plane wavefront to where it meets that sphere; the OPD
//! is that path minus the chief ray's (Welford, Aberrations of Optical Systems, 1986,
//! ch. 6; Born & Wolf, Principles of Optics, 7th ed., ch. 5).
//!
//! OPD = OPL(ray) − OPL(chief), in waves. A ray with positive OPD has travelled further
//! than the chief ray to the reference sphere.

use optobench_math::Vec3;

use crate::pupil::{Analysis, AnalysisError, Field, ImageRay};
use crate::zernike::{ZernikeError, zernike_fit};

/// OPD samples over the pupil for one field.
#[derive(Debug, Clone, PartialEq)]
pub struct Wavefront {
    /// (px, py, OPD in waves) for every ray that arrived.
    pub samples: Vec<(f64, f64, f64)>,
    /// RMS of the OPD about its mean (waves).
    pub rms: f64,
    /// Largest minus smallest OPD (waves).
    pub peak_to_valley: f64,
}

impl Wavefront {
    /// Least-squares Zernike coefficients (waves), ANSI order, up to radial order
    /// `max_order`.
    pub fn zernike(&self, max_order: usize) -> Result<Vec<f64>, ZernikeError> {
        zernike_fit(&self.samples, max_order)
    }
}

/// The reference sphere: centre and radius, and the image index for the path back.
struct ReferenceSphere {
    centre: Vec3,
    /// Unit vector from the centre towards the exit pupil.
    towards_pupil: Vec3,
    radius: f64,
}

impl ReferenceSphere {
    /// Signed distance along the ray from its image point back to the sphere, on the
    /// exit-pupil side, or `None` if the ray line misses the sphere.
    fn distance(&self, ray: &ImageRay) -> Option<f64> {
        let w = ray.point - self.centre;
        let half_b = w.dot(ray.direction);
        let c = w.length_squared() - self.radius * self.radius;
        let discriminant = half_b * half_b - c;
        if discriminant < 0.0 {
            return None;
        }
        let root = discriminant.sqrt();
        [-half_b - root, -half_b + root]
            .into_iter()
            .find(|&t| (ray.point + ray.direction * t - self.centre).dot(self.towards_pupil) > 0.0)
    }
}

impl Analysis<'_> {
    fn reference_sphere(&self, chief: &ImageRay) -> Result<ReferenceSphere, AnalysisError> {
        let exit = self.pupils().exit_position;
        if chief.direction.z == 0.0 {
            return Err(AnalysisError::ExitPupilAtInfinity);
        }
        let t = (exit - chief.point.z) / chief.direction.z;
        let pupil_point = chief.point + chief.direction * t;
        let offset = pupil_point - chief.point;
        let radius = offset.length();
        let towards_pupil = offset
            .normalized()
            .ok_or(AnalysisError::ExitPupilAtInfinity)?;
        Ok(ReferenceSphere {
            centre: chief.point,
            towards_pupil,
            radius,
        })
    }

    fn path_to_sphere(&self, sphere: &ReferenceSphere, ray: &ImageRay) -> Option<f64> {
        let t = sphere.distance(ray)?;
        Some(ray.optical_path + self.image_index() * t)
    }

    /// OPD (waves) of the ray at pupil coordinates (px, py), or `Ok(None)` if it is lost.
    pub fn optical_path_difference(
        &self,
        field: Field,
        px: f64,
        py: f64,
    ) -> Result<Option<f64>, AnalysisError> {
        let chief = self.chief_ray(field)?;
        let sphere = self.reference_sphere(&chief)?;
        let reference = self
            .path_to_sphere(&sphere, &chief)
            .ok_or(AnalysisError::ExitPupilAtInfinity)?;
        let waves = self.wavelength() * 1e-3;
        Ok(self
            .image_ray(field, px, py)?
            .and_then(|ray| self.path_to_sphere(&sphere, &ray))
            .map(|path| (path - reference) / waves))
    }

    /// OPD over a square grid of `points` × `points` pupil samples inside the unit disk.
    pub fn wavefront(&self, field: Field, points: usize) -> Result<Wavefront, AnalysisError> {
        let chief = self.chief_ray(field)?;
        let sphere = self.reference_sphere(&chief)?;
        let reference = self
            .path_to_sphere(&sphere, &chief)
            .ok_or(AnalysisError::ExitPupilAtInfinity)?;
        let waves = self.wavelength() * 1e-3;
        let points = points.max(2);
        let mut samples = Vec::new();
        for i in 0..points {
            for j in 0..points {
                let px = -1.0 + 2.0 * i as f64 / (points - 1) as f64;
                let py = -1.0 + 2.0 * j as f64 / (points - 1) as f64;
                if px.hypot(py) > 1.0 {
                    continue;
                }
                let path = self
                    .image_ray(field, px, py)?
                    .and_then(|ray| self.path_to_sphere(&sphere, &ray));
                if let Some(path) = path {
                    samples.push((px, py, (path - reference) / waves));
                }
            }
        }
        let count = samples.len().max(1) as f64;
        let mean = samples.iter().map(|s| s.2).sum::<f64>() / count;
        let rms = (samples.iter().map(|s| (s.2 - mean).powi(2)).sum::<f64>() / count).sqrt();
        let (low, high) = samples
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), s| {
                (l.min(s.2), h.max(s.2))
            });
        Ok(Wavefront {
            samples,
            rms,
            peak_to_valley: if high >= low { high - low } else { 0.0 },
        })
    }
}
