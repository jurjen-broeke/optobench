//! The ideal objective (M9a): a microscope objective by its first-order properties and its
//! design, without a prescription.
//!
//! In its local frame the pupil plane is z = 0 and the front plane, its last surface
//! (taken as flat), is z = `pupil_distance`; +z points to the sample. The objective is
//! aplanatic: it obeys the Abbe sine condition, so a ray crossing the pupil at height ρ
//! (from the pupil centre, where every chief ray crosses) leaves towards its focus with
//! n sin θ = |ρ| / f (Born and Wolf, Principles of Optics, 7th ed., §4.5.1). Across flat
//! layers that invariant p = ρ / f is kept (Snell's law), which makes the design ray path
//! a closed form: from the design focus, the ray is traced backwards through the design
//! sample medium, the design cover glass and the design immersion to the front plane. A
//! ray at field angle φ (its direction at the pupil) focuses at f · tan φ off the axis in
//! the design focal plane, and its chief ray leaves parallel to the axis (telecentric
//! object space when the pupil is the back focal plane).
//!
//! Forward from the front plane the ray meets whatever is actually there, so an objective
//! used as designed focuses perfectly and one used otherwise shows its focal shift,
//! spherical aberration and loss of aperture in the trace itself (the stratified-media
//! picture of S. F. Gibson and F. Lanni, J. Opt. Soc. Am. A 8, 1601 (1991)). The ray's
//! optical path is set so that design rays reach the design focus in phase.

use optobench_materials::{MaterialError, Medium};
use optobench_math::Vec3;

/// A layer of the design between the objective's front plane and its focus.
#[derive(Debug, Clone)]
pub struct DesignLayer {
    /// Thickness along the axis (mm).
    pub thickness: f64,
    /// Its medium.
    pub medium: Medium,
}

/// An ideal objective.
#[derive(Debug, Clone)]
pub struct IdealObjective {
    /// Focal length f (mm): the pupil radius is f · NA.
    pub focal_length: f64,
    /// Numerical aperture n sin θ, in the design medium at the focus.
    pub na: f64,
    /// Distance from the pupil plane to the front plane (mm).
    pub pupil_distance: f64,
    /// The design layers from the front plane towards the focus: the immersion (its
    /// thickness the working distance), then the cover glass if there is one.
    pub design_layers: Vec<DesignLayer>,
    /// The design medium at the focus (the sample, just past the cover glass).
    pub design_sample: Medium,
}

impl PartialEq for IdealObjective {
    /// The same numbers and layer thicknesses, and the same media (shared handles).
    fn eq(&self, other: &Self) -> bool {
        self.focal_length.to_bits() == other.focal_length.to_bits()
            && self.na.to_bits() == other.na.to_bits()
            && self.pupil_distance.to_bits() == other.pupil_distance.to_bits()
            && self.design_sample.same(&other.design_sample)
            && self.design_layers.len() == other.design_layers.len()
            && self
                .design_layers
                .iter()
                .zip(&other.design_layers)
                .all(|(a, b)| {
                    a.thickness.to_bits() == b.thickness.to_bits() && a.medium.same(&b.medium)
                })
    }
}

/// What an objective does to a ray arriving at its pupil.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Passage {
    /// The ray leaves from the front plane: local position, unit direction, and the
    /// optical path to add (mm), so design rays meet at the design focus in phase.
    Leaves {
        /// Where it leaves, on the front plane (local).
        position: Vec3,
        /// Its direction (local), in the medium in front of the objective.
        direction: Vec3,
        /// Optical path to add (mm).
        optical_path: f64,
    },
    /// Stopped at the pupil: beyond f · NA, or travelling the wrong way.
    Stopped,
    /// The aperture asks for a steeper ray than the medium in front can carry
    /// (n sin θ beyond its index).
    Evanescent,
}

impl IdealObjective {
    /// The pupil radius f · NA (mm).
    pub fn pupil_radius(&self) -> f64 {
        self.focal_length * self.na
    }

    /// Where the design focus is on the axis (local z).
    pub fn design_focus(&self) -> f64 {
        self.pupil_distance + self.design_layers.iter().map(|l| l.thickness).sum::<f64>()
    }

    /// The ray leaving for a ray arriving at the pupil plane at local `point` with
    /// local `direction`; `exit_index` is the index of the medium in front of the front
    /// plane, at the ray's wavelength.
    pub fn pass(
        &self,
        point: Vec3,
        direction: Vec3,
        wavelength: f64,
        exit_index: f64,
    ) -> Result<Passage, MaterialError> {
        if direction.z <= 0.0 {
            return Ok(Passage::Stopped);
        }
        // Pupil coordinate and the invariant p = n sin θ (a vector towards the axis).
        let rho = (point.x, point.y);
        if rho.0.hypot(rho.1) > self.pupil_radius() * (1.0 + 1e-12) {
            return Ok(Passage::Stopped);
        }
        let f = self.focal_length;
        let p = (-rho.0 / f, -rho.1 / f);
        let p_size = p.0.hypot(p.1);
        // The field point in the design focal plane.
        let field = (f * direction.x / direction.z, f * direction.y / direction.z);

        // Back from the design focus through the design layers (last first): in a layer
        // of index n the ray's transverse slope is p / √(n² − p²).
        let mut x = field.0;
        let mut y = field.1;
        let mut optical_path = 0.0;
        for layer in self.design_layers.iter().rev() {
            let n = layer.medium.material().refractive_index(wavelength)?;
            if p_size >= n {
                // Beyond what the design itself can carry: not a ray of this design.
                return Ok(Passage::Stopped);
            }
            let cos_n = (n * n - p_size * p_size).sqrt(); // n cos θ
            x -= layer.thickness * p.0 / cos_n;
            y -= layer.thickness * p.1 / cos_n;
            // Optical path n · t / cos θ = t n² / (n cos θ).
            optical_path += layer.thickness * n * n / cos_n;
        }
        if p_size >= exit_index {
            return Ok(Passage::Evanescent);
        }
        let cos_exit = (exit_index * exit_index - p_size * p_size).sqrt() / exit_index;
        let direction = Vec3::new(p.0 / exit_index, p.1 / exit_index, cos_exit);
        // The design path from the front plane to the focus is replaced by a constant:
        // the axial one, Σ n t.
        let axial: f64 = self
            .design_layers
            .iter()
            .map(|l| {
                l.medium
                    .material()
                    .refractive_index(wavelength)
                    .map(|n| n * l.thickness)
            })
            .sum::<Result<f64, MaterialError>>()?;
        Ok(Passage::Leaves {
            position: Vec3::new(x, y, self.pupil_distance),
            direction,
            optical_path: axial - optical_path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use optobench_materials::Medium;

    fn objective(layers: &[(f64, f64)], sample: f64) -> IdealObjective {
        IdealObjective {
            focal_length: 9.0,
            na: 0.8,
            pupil_distance: 40.0,
            design_layers: layers
                .iter()
                .map(|&(t, n)| DesignLayer {
                    thickness: t,
                    medium: Medium::from(n),
                })
                .collect(),
            design_sample: Medium::from(sample),
        }
    }

    /// Where a leaving ray, carried on through the design layers, crosses the design
    /// focal plane.
    fn at_focus(o: &IdealObjective, passage: Passage) -> (f64, f64, f64) {
        let Passage::Leaves {
            position,
            direction,
            optical_path,
        } = passage
        else {
            panic!("{passage:?}");
        };
        let (mut x, mut y, mut path) = (position.x, position.y, optical_path);
        let p = (direction.x * 1.33, direction.y * 1.33); // exit index used below
        let p_size = p.0.hypot(p.1);
        for layer in &o.design_layers {
            let n = layer
                .medium
                .material()
                .refractive_index(0.55)
                .unwrap_or(1.0);
            let cos_n = (n * n - p_size * p_size).sqrt();
            x += layer.thickness * p.0 / cos_n;
            y += layer.thickness * p.1 / cos_n;
            path += layer.thickness * n * n / cos_n;
        }
        (x, y, path)
    }

    #[test]
    fn design_rays_meet_at_the_design_focus_in_phase() {
        // Water immersion (n = 1.33, 2 mm) through a 0.17 mm cover glass (n = 1.525):
        // every pupil ray of a field reaches the same point with the same optical path.
        let o = objective(&[(2.0, 1.33), (0.17, 1.525)], 1.33);
        let field = Vec3::new(0.01, -0.02, 1.0).normalized().expect("unit");
        let mut reached = Vec::new();
        for rho in [0.0, 1.0, 3.0, 5.0, 7.0] {
            for angle in [0.0_f64, 1.0, 2.5] {
                let point = Vec3::new(rho * angle.cos(), rho * angle.sin(), 0.0);
                let passage = o.pass(point, field, 0.55, 1.33).expect("indices");
                reached.push(at_focus(&o, passage));
            }
        }
        let (fx, fy) = (9.0 * 0.01, 9.0 * -0.02);
        for (x, y, path) in &reached {
            assert!((x - fx).abs() < 1e-12 && (y - fy).abs() < 1e-12, "{x} {y}");
            assert!((path - reached[0].2).abs() < 1e-12, "{path}");
        }
    }

    #[test]
    fn the_pupil_stops_rays_beyond_f_na_and_the_sine_condition_holds() {
        let o = objective(&[(2.0, 1.33)], 1.33);
        let axial = Vec3::Z;
        assert_eq!(
            o.pass(Vec3::new(7.3, 0.0, 0.0), axial, 0.55, 1.33)
                .expect("ok"),
            Passage::Stopped
        );
        // At the rim, n sin θ = NA in the immersion.
        let Passage::Leaves { direction, .. } = o
            .pass(Vec3::new(0.0, 7.2, 0.0), axial, 0.55, 1.33)
            .expect("ok")
        else {
            panic!("passes");
        };
        let sin = direction.x.hypot(direction.y);
        assert!((1.33 * sin - 0.8).abs() < 1e-12, "{}", 1.33 * sin);
        assert!(direction.y < 0.0, "towards the axis");
        // In air in front of it, the same aperture cannot be carried.
        let o = IdealObjective { na: 1.2, ..o };
        assert_eq!(
            o.pass(Vec3::new(0.0, 10.0, 0.0), axial, 0.55, 1.0)
                .expect("ok"),
            Passage::Evanescent
        );
    }
}
