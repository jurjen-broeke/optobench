//! Spot diagrams and ray fans.

use crate::pupil::{Analysis, AnalysisError, Field};

/// Where rays from one field land on the image plane.
#[derive(Debug, Clone, PartialEq)]
pub struct SpotDiagram {
    /// Image-plane (x, y) of every ray that arrived, relative to the chief ray (mm).
    pub points: Vec<(f64, f64)>,
    /// Chief ray position on the image plane (mm, system frame).
    pub chief: (f64, f64),
    /// Centroid of `points`, relative to the chief ray (mm).
    pub centroid: (f64, f64),
    /// RMS distance of the points from their centroid (mm).
    pub rms_radius: f64,
    /// Largest distance of a point from the centroid (mm).
    pub geometric_radius: f64,
    /// Rays launched.
    pub launched: usize,
}

impl SpotDiagram {
    /// Rays that did not reach the image plane.
    pub fn lost(&self) -> usize {
        self.launched - self.points.len()
    }
}

/// Hexapolar pupil sampling: the centre plus `rings` rings, ring i holding 6i points at
/// radius i/rings. Close to uniform per unit area.
pub(crate) fn hexapolar(rings: usize) -> Vec<(f64, f64)> {
    let mut points = vec![(0.0, 0.0)];
    for ring in 1..=rings {
        let radius = ring as f64 / rings as f64;
        let count = 6 * ring;
        for k in 0..count {
            let angle = core::f64::consts::TAU * k as f64 / count as f64;
            points.push((radius * angle.cos(), radius * angle.sin()));
        }
    }
    points
}

/// One sample of a ray fan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FanPoint {
    /// Pupil coordinate along the fan (−1 to 1).
    pub pupil: f64,
    /// Transverse aberration in x relative to the chief ray (mm), or `None` if the ray
    /// was lost.
    pub x: Option<f64>,
    /// Transverse aberration in y relative to the chief ray (mm), or `None` if lost.
    pub y: Option<f64>,
}

/// Tangential (pupil y) and sagittal (pupil x) ray fans for one field.
#[derive(Debug, Clone, PartialEq)]
pub struct RayFan {
    /// Rays at (0, p).
    pub tangential: Vec<FanPoint>,
    /// Rays at (p, 0).
    pub sagittal: Vec<FanPoint>,
}

impl Analysis<'_> {
    /// Spot diagram for `field`, with hexapolar pupil sampling of `rings` rings.
    pub fn spot_diagram(&self, field: Field, rings: usize) -> Result<SpotDiagram, AnalysisError> {
        let chief = self.chief_ray(field)?.point;
        let samples = hexapolar(rings);
        let mut points = Vec::with_capacity(samples.len());
        for &(px, py) in &samples {
            if let Some(ray) = self.image_ray(field, px, py)? {
                points.push((ray.point.x - chief.x, ray.point.y - chief.y));
            }
        }
        let count = points.len().max(1) as f64;
        let centroid = (
            points.iter().map(|p| p.0).sum::<f64>() / count,
            points.iter().map(|p| p.1).sum::<f64>() / count,
        );
        let distance = |p: &(f64, f64)| (p.0 - centroid.0).hypot(p.1 - centroid.1);
        let rms_radius = (points.iter().map(|p| distance(p).powi(2)).sum::<f64>() / count).sqrt();
        let geometric_radius = points.iter().map(distance).fold(0.0, f64::max);
        Ok(SpotDiagram {
            points,
            chief: (chief.x, chief.y),
            centroid,
            rms_radius,
            geometric_radius,
            launched: samples.len(),
        })
    }

    /// Ray fans for `field` with `samples` points from pupil −1 to 1 (at least 2).
    pub fn ray_fan(&self, field: Field, samples: usize) -> Result<RayFan, AnalysisError> {
        let chief = self.chief_ray(field)?.point;
        let samples = samples.max(2);
        let fan = |along_y: bool| -> Result<Vec<FanPoint>, AnalysisError> {
            (0..samples)
                .map(|i| {
                    let p = -1.0 + 2.0 * i as f64 / (samples - 1) as f64;
                    let (px, py) = if along_y { (0.0, p) } else { (p, 0.0) };
                    let ray = self.image_ray(field, px, py)?;
                    Ok(FanPoint {
                        pupil: p,
                        x: ray.map(|r| r.point.x - chief.x),
                        y: ray.map(|r| r.point.y - chief.y),
                    })
                })
                .collect()
        };
        Ok(RayFan {
            tangential: fan(true)?,
            sagittal: fan(false)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hexapolar_sampling_counts_and_stays_in_the_unit_disk() {
        // 1 + 6 + 12 + 18 points for three rings.
        let points = hexapolar(3);
        assert_eq!(points.len(), 37);
        assert!(points.iter().all(|p| p.0.hypot(p.1) <= 1.0 + 1e-15));
    }
}
