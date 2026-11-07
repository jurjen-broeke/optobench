//! Paraxial (first-order) analysis: the y-nu trace and cardinal points.
//!
//! The y-nu method and the system matrix follow the standard first-order treatment
//! (e.g. Hecht, Optics, 5th ed., §6.2, matrix methods; Gerrard & Burch, Introduction to
//! Matrix Methods in Optics, 1975):
//!
//! - refraction at surface i: nu' = nu − y φᵢ, with power φᵢ = (n' − n) cᵢ
//! - transfer to the next surface: y' = y + tᵢ · nu' / n'
//!
//! where y is the ray height, nu the index times the paraxial angle, c the vertex
//! curvature and t the thickness. Mirrors use the signed-index convention: the index
//! after a reflection is −n, so the same two equations cover lenses and mirrors.

use crate::sequential::{Interaction, SequentialSystem};

/// A paraxial ray at one surface: its height and its reduced angle after the surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParaxialRay {
    /// Height at the surface (mm).
    pub height: f64,
    /// Signed index times paraxial angle after the surface (nu).
    pub reduced_angle: f64,
}

/// First-order properties of a system. Positions are signed z offsets in the system
/// frame (mm), measured from the first vertex (front quantities) or the last vertex
/// (rear quantities).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardinalPoints {
    /// Power Φ (1/mm).
    pub power: f64,
    /// Effective focal length 1/Φ (mm); the focal length in air.
    pub effective_focal_length: f64,
    /// Back focal length: the rear focal point's offset from the last vertex.
    pub back_focal_length: f64,
    /// Front focal length: the distance from the front focal point to the first vertex,
    /// positive when the focal point lies in front of it.
    pub front_focal_length: f64,
    /// Front principal plane's offset from the first vertex.
    pub front_principal_plane: f64,
    /// Rear principal plane's offset from the last vertex.
    pub rear_principal_plane: f64,
}

impl SequentialSystem {
    /// Signed index after each surface: the sign flips at every mirror.
    fn signed_indices(&self) -> Vec<f64> {
        let mut sign = 1.0;
        self.surfaces
            .iter()
            .map(|s| {
                if s.interaction == Interaction::Reflect {
                    sign = -sign;
                }
                sign * s.index_after
            })
            .collect()
    }

    /// Signed index of the image space (after the last surface).
    pub fn image_index(&self) -> f64 {
        self.signed_indices()
            .last()
            .copied()
            .unwrap_or(self.object_index)
    }

    /// Traces a paraxial ray entering surface 0 at `height` with reduced angle
    /// `reduced_angle` (object index times angle). Returns the ray at every surface.
    pub fn paraxial_trace(&self, height: f64, reduced_angle: f64) -> Vec<ParaxialRay> {
        let indices = self.signed_indices();
        let mut y = height;
        let mut nu = reduced_angle;
        let mut index_before = self.object_index;
        let mut out = Vec::with_capacity(self.surfaces.len());
        for (surface, &index_after) in self.surfaces.iter().zip(&indices) {
            let power = (index_after - index_before) * surface.surface.vertex_curvature();
            nu -= y * power;
            out.push(ParaxialRay {
                height: y,
                reduced_angle: nu,
            });
            y += surface.thickness * nu / index_after;
            index_before = index_after;
        }
        out
    }

    /// The system matrix [[A, B], [C, D]] from just before the first surface to just
    /// after the last: (y, nu) out = M · (y, nu) in. C = −Φ.
    fn system_matrix(&self) -> Option<[[f64; 2]; 2]> {
        let first = self.paraxial_trace(1.0, 0.0).last().copied()?;
        let second = self.paraxial_trace(0.0, 1.0).last().copied()?;
        Some([
            [first.height, second.height],
            [first.reduced_angle, second.reduced_angle],
        ])
    }

    /// Cardinal points, or `None` for an empty or afocal system (zero power).
    pub fn cardinal_points(&self) -> Option<CardinalPoints> {
        let [[a, _b], [c, d]] = self.system_matrix()?;
        let power = -c;
        if power == 0.0 {
            return None;
        }
        let n = self.object_index;
        let n_image = self.image_index();
        // Rear focus: a ray entering parallel at height 1 leaves at height A with reduced
        // angle C and crosses the axis A·n'/Φ beyond the last vertex.
        let back_focal_length = a * n_image / power;
        // Front focus: the input ray that leaves parallel (C·y + D·nu = 0) comes from the
        // point D·n/Φ in front of the first vertex.
        let front_focal_length = d * n / power;
        Some(CardinalPoints {
            power,
            effective_focal_length: 1.0 / power,
            back_focal_length,
            front_focal_length,
            front_principal_plane: n * (1.0 - d) / power,
            rear_principal_plane: n_image * (a - 1.0) / power,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequential::SequentialSurface;
    use optobench_surfaces::{Plane, Sphere};

    #[test]
    fn lagrange_invariant_is_conserved() {
        // The Lagrange invariant H = nū·y − nu·ȳ is the same at every surface for any two
        // paraxial rays (Hecht, Optics, 5th ed., ch. 6). A Cooke-triplet-like stack with
        // arbitrary values exercises it.
        let system = SequentialSystem {
            object_index: 1.0,
            surfaces: vec![
                SequentialSurface::refracting(Sphere::from_radius(21.5), 3.3, 1.62),
                SequentialSurface::refracting(Sphere::from_radius(-150.0), 5.0, 1.0),
                SequentialSurface::refracting(Sphere::from_radius(-20.5), 1.0, 1.61),
                SequentialSurface::refracting(Sphere::from_radius(19.6), 5.0, 1.0),
                SequentialSurface::refracting(Sphere::from_radius(150.0), 3.0, 1.62),
                SequentialSurface::refracting(Sphere::from_radius(-18.5), 40.0, 1.0),
                SequentialSurface::refracting(Plane, 0.0, 1.0),
            ],
        };
        let marginal = system.paraxial_trace(5.0, 0.0);
        let chief = system.paraxial_trace(-2.0, 0.1);
        let invariant: Vec<f64> = marginal
            .iter()
            .zip(&chief)
            .map(|(m, c)| c.reduced_angle * m.height - m.reduced_angle * c.height)
            .collect();
        for h in &invariant {
            assert!((h - invariant[0]).abs() < 1e-12, "{invariant:?}");
        }
    }

    #[test]
    fn afocal_and_empty_systems_have_no_cardinal_points() {
        let empty = SequentialSystem {
            object_index: 1.0,
            surfaces: vec![],
        };
        assert!(empty.cardinal_points().is_none());
        let window = SequentialSystem {
            object_index: 1.0,
            surfaces: vec![
                SequentialSurface::refracting(Plane, 5.0, 1.5),
                SequentialSurface::refracting(Plane, 0.0, 1.0),
            ],
        };
        assert!(window.cardinal_points().is_none());
    }
}
