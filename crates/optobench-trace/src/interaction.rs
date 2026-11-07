//! Reflection and refraction of a direction at a surface.
//!
//! Vector form of the law of reflection and of Snell's law (e.g. Hecht, Optics, 5th ed.,
//! ch. 4): with unit incident direction d, unit normal n oriented against d
//! (cos θᵢ = −n·d > 0) and index ratio η = n₁/n₂,
//!
//! - reflected: r = d + 2 cos θᵢ n
//! - refracted: t = η d + (η cos θᵢ − cos θₜ) n, with cos θₜ = √(1 − η² (1 − cos² θᵢ))
//!
//! When the square root's argument is negative there is no refracted ray: total internal
//! reflection.

use optobench_math::Vec3;

/// The outcome of refraction at an interface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Refraction {
    /// The ray passes; the new unit direction.
    Refracted(Vec3),
    /// Beyond the critical angle: no transmitted ray. The reflected unit direction.
    TotalInternalReflection(Vec3),
}

/// The normal flipped, if needed, to point against `direction`, and cos θᵢ ≥ 0.
fn oriented(direction: Vec3, normal: Vec3) -> (Vec3, f64) {
    let cosine = -normal.dot(direction);
    if cosine < 0.0 {
        (-normal, -cosine)
    } else {
        (normal, cosine)
    }
}

/// Mirror reflection of a unit `direction` at a surface with unit `normal` (either
/// orientation).
pub fn reflect(direction: Vec3, normal: Vec3) -> Vec3 {
    let (normal, cosine) = oriented(direction, normal);
    direction + normal * (2.0 * cosine)
}

/// Refraction of a unit `direction` from index `index_before` into `index_after` at a
/// surface with unit `normal` (either orientation).
pub fn refract(direction: Vec3, normal: Vec3, index_before: f64, index_after: f64) -> Refraction {
    let (normal, cosine) = oriented(direction, normal);
    let ratio = index_before / index_after;
    let radicand = 1.0 - ratio * ratio * (1.0 - cosine * cosine);
    if radicand < 0.0 {
        return Refraction::TotalInternalReflection(direction + normal * (2.0 * cosine));
    }
    Refraction::Refracted(direction * ratio + normal * (ratio * cosine - radicand.sqrt()))
}

/// Fresnel amplitude coefficients for s (perpendicular) and p (parallel) polarisation,
/// in Hecht's sign convention (Hecht, Optics, 5th ed., ch. 4):
///
/// - r_s = (n₁ cos θᵢ − n₂ cos θₜ) / (n₁ cos θᵢ + n₂ cos θₜ)
/// - t_s = 2 n₁ cos θᵢ / (n₁ cos θᵢ + n₂ cos θₜ)
/// - r_p = (n₂ cos θᵢ − n₁ cos θₜ) / (n₁ cos θₜ + n₂ cos θᵢ)
/// - t_p = 2 n₁ cos θᵢ / (n₁ cos θₜ + n₂ cos θᵢ)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FresnelAmplitudes {
    /// Reflected amplitude, s.
    pub reflection_s: f64,
    /// Reflected amplitude, p.
    pub reflection_p: f64,
    /// Transmitted amplitude, s.
    pub transmission_s: f64,
    /// Transmitted amplitude, p.
    pub transmission_p: f64,
}

/// Fresnel power coefficients: the fraction of incident power reflected and transmitted
/// for each polarisation. For lossless media R + T = 1 for each.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FresnelPower {
    /// Reflectance, s.
    pub reflectance_s: f64,
    /// Reflectance, p.
    pub reflectance_p: f64,
    /// Transmittance, s.
    pub transmittance_s: f64,
    /// Transmittance, p.
    pub transmittance_p: f64,
}

impl FresnelPower {
    /// Reflectance of unpolarised light: the mean of s and p.
    pub fn unpolarised_reflectance(&self) -> f64 {
        0.5 * (self.reflectance_s + self.reflectance_p)
    }

    /// Transmittance of unpolarised light: the mean of s and p.
    pub fn unpolarised_transmittance(&self) -> f64 {
        0.5 * (self.transmittance_s + self.transmittance_p)
    }
}

/// cos θₜ from Snell's law, or `None` beyond the critical angle.
fn transmitted_cosine(cos_incidence: f64, index_before: f64, index_after: f64) -> Option<f64> {
    let ratio = index_before / index_after;
    let radicand = 1.0 - ratio * ratio * (1.0 - cos_incidence * cos_incidence);
    (radicand >= 0.0).then(|| radicand.sqrt())
}

/// Fresnel amplitude coefficients at an interface between real indices, for the angle of
/// incidence with cosine `cos_incidence` (0 to 1). `None` beyond the critical angle,
/// where the reflected amplitudes are complex with unit magnitude.
pub fn fresnel_amplitudes(
    cos_incidence: f64,
    index_before: f64,
    index_after: f64,
) -> Option<FresnelAmplitudes> {
    let ci = cos_incidence.abs();
    let ct = transmitted_cosine(ci, index_before, index_after)?;
    let (n1, n2) = (index_before, index_after);
    let s = n1 * ci + n2 * ct;
    let p = n1 * ct + n2 * ci;
    Some(FresnelAmplitudes {
        reflection_s: (n1 * ci - n2 * ct) / s,
        reflection_p: (n2 * ci - n1 * ct) / p,
        transmission_s: 2.0 * n1 * ci / s,
        transmission_p: 2.0 * n1 * ci / p,
    })
}

/// Fresnel power coefficients at an interface between real indices. Beyond the critical
/// angle everything is reflected. Transmittance includes the beam-area and index factor
/// T = (n₂ cos θₜ / n₁ cos θᵢ) |t|² (Hecht, Optics, 5th ed., ch. 4).
pub fn fresnel_power(cos_incidence: f64, index_before: f64, index_after: f64) -> FresnelPower {
    let ci = cos_incidence.abs();
    let total_reflection = FresnelPower {
        reflectance_s: 1.0,
        reflectance_p: 1.0,
        transmittance_s: 0.0,
        transmittance_p: 0.0,
    };
    let Some(amplitudes) = fresnel_amplitudes(ci, index_before, index_after) else {
        return total_reflection;
    };
    let Some(ct) = transmitted_cosine(ci, index_before, index_after) else {
        return total_reflection;
    };
    if ci == 0.0 {
        // Grazing incidence: nothing enters.
        return total_reflection;
    }
    let factor = (index_after * ct) / (index_before * ci);
    FresnelPower {
        reflectance_s: amplitudes.reflection_s * amplitudes.reflection_s,
        reflectance_p: amplitudes.reflection_p * amplitudes.reflection_p,
        transmittance_s: factor * amplitudes.transmission_s * amplitudes.transmission_s,
        transmittance_p: factor * amplitudes.transmission_p * amplitudes.transmission_p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-12;

    fn in_plane(angle: f64) -> Vec3 {
        // A unit direction at `angle` from +z in the x–z plane.
        Vec3::new(angle.sin(), 0.0, angle.cos())
    }

    /// Angle from the axis ±z of a direction in the x–z plane.
    fn angle_from_axis(v: Vec3) -> f64 {
        v.x.abs().atan2(v.z.abs())
    }

    fn refracted(direction: Vec3, n1: f64, n2: f64) -> Vec3 {
        match refract(direction, Vec3::Z, n1, n2) {
            Refraction::Refracted(t) => t,
            Refraction::TotalInternalReflection(_) => {
                panic!("unexpected total internal reflection")
            }
        }
    }

    #[test]
    fn normal_incidence_passes_straight_through() {
        let t = refracted(Vec3::Z, 1.0, 1.5);
        assert!((t - Vec3::Z).length() < EPS);
        // The normal may point either way.
        assert_eq!(
            refract(Vec3::Z, -Vec3::Z, 1.0, 1.5),
            Refraction::Refracted(t)
        );
    }

    #[test]
    fn snell_law_holds_and_directions_stay_unit() {
        // Snell's law, n₁ sin θ₁ = n₂ sin θ₂ (Hecht, Optics, 5th ed., ch. 4).
        for (n1, n2) in [(1.0, 1.5), (1.5, 1.0), (1.33, 1.52)] {
            for degrees in [5.0_f64, 20.0, 35.0, 40.0] {
                let incident = in_plane(degrees.to_radians());
                let t = refracted(incident, n1, n2);
                assert!((t.length() - 1.0).abs() < EPS);
                let lhs = n1 * degrees.to_radians().sin();
                let rhs = n2 * angle_from_axis(t).sin();
                assert!(
                    (lhs - rhs).abs() < EPS,
                    "n1 = {n1}, n2 = {n2}, θ = {degrees}°"
                );
                assert!(t.z > 0.0, "transmitted ray keeps going forward");
            }
        }
    }

    #[test]
    fn reflection_mirrors_the_angle() {
        let incident = in_plane(0.3);
        let r = reflect(incident, Vec3::Z);
        assert!((r - Vec3::new(incident.x, 0.0, -incident.z)).length() < EPS);
    }

    #[test]
    fn at_brewster_angle_reflected_and_refracted_rays_are_perpendicular() {
        // Brewster's angle tan θ_B = n₂/n₁; there the reflected and refracted rays are
        // perpendicular (Born & Wolf, Principles of Optics, 7th ed., ch. 1).
        for (n1, n2) in [(1.0, 1.5), (1.33, 1.0)] {
            let brewster = f64::atan2(n2, n1);
            let incident = in_plane(brewster);
            let t = refracted(incident, n1, n2);
            let r = reflect(incident, Vec3::Z);
            assert!(t.dot(r).abs() < EPS, "n1 = {n1}, n2 = {n2}");
        }
    }

    #[test]
    fn total_internal_reflection_beyond_the_critical_angle() {
        // Critical angle sin θ_c = n₂/n₁ for n₁ > n₂ (Hecht, Optics, 5th ed., ch. 4).
        let (n1, n2): (f64, f64) = (1.5, 1.0);
        let critical = (n2 / n1).asin();
        let below = refracted(in_plane(critical - 1e-6), n1, n2);
        assert!(
            angle_from_axis(below) > 89.5_f64.to_radians(),
            "nearly grazing"
        );
        let above = refract(in_plane(critical + 1e-6), Vec3::Z, n1, n2);
        let incident = in_plane(critical + 1e-6);
        assert_eq!(
            above,
            Refraction::TotalInternalReflection(reflect(incident, Vec3::Z))
        );
    }

    #[test]
    fn fresnel_at_normal_incidence() {
        // R = ((n₁ − n₂)/(n₁ + n₂))² for both polarisations (Hecht, Optics, 5th ed., ch. 4):
        // air to n = 1.5 gives R = 0.04.
        let power = fresnel_power(1.0, 1.0, 1.5);
        assert!((power.reflectance_s - 0.04).abs() < EPS);
        assert!((power.reflectance_p - 0.04).abs() < EPS);
        assert!((power.unpolarised_transmittance() - 0.96).abs() < EPS);
    }

    #[test]
    fn fresnel_p_reflectance_vanishes_at_brewster_angle() {
        // Brewster's angle tan θ_B = n₂/n₁ (Hecht, Optics, 5th ed., ch. 8).
        for (n1, n2) in [(1.0, 1.5), (1.5, 1.0), (1.33, 1.7)] {
            let cos_brewster = f64::atan2(n2, n1).cos();
            let power = fresnel_power(cos_brewster, n1, n2);
            assert!(power.reflectance_p < EPS, "n1 = {n1}, n2 = {n2}");
            assert!(power.reflectance_s > 0.0);
        }
    }

    #[test]
    fn fresnel_conserves_power_and_obeys_stokes_relation() {
        for (n1, n2) in [(1.0, 1.5), (1.5, 1.0), (1.0, 1.33)] {
            for degrees in [0.0_f64, 15.0, 30.0, 40.0, 60.0, 85.0] {
                let ci = degrees.to_radians().cos();
                let power = fresnel_power(ci, n1, n2);
                assert!((power.reflectance_s + power.transmittance_s - 1.0).abs() < 1e-12);
                assert!((power.reflectance_p + power.transmittance_p - 1.0).abs() < 1e-12);
                // Stokes relation r′ = −r: the reverse interface, entered at θₜ, reflects
                // with the opposite sign (Hecht, Optics, 5th ed., ch. 4).
                if let (Some(forward), Some(ct)) = (
                    fresnel_amplitudes(ci, n1, n2),
                    transmitted_cosine(ci, n1, n2),
                ) {
                    let back = fresnel_amplitudes(ct, n2, n1).expect("below critical");
                    assert!((forward.reflection_s + back.reflection_s).abs() < 1e-12);
                    assert!((forward.reflection_p + back.reflection_p).abs() < 1e-12);
                }
            }
        }
    }

    #[test]
    fn fresnel_reflects_everything_beyond_the_critical_angle_and_at_grazing() {
        let critical = (1.0_f64 / 1.5).asin();
        let power = fresnel_power((critical + 1e-3).cos(), 1.5, 1.0);
        assert_eq!(power.unpolarised_reflectance(), 1.0);
        assert!(fresnel_amplitudes((critical + 1e-3).cos(), 1.5, 1.0).is_none());
        assert_eq!(fresnel_power(0.0, 1.0, 1.5).unpolarised_reflectance(), 1.0);
    }
}
