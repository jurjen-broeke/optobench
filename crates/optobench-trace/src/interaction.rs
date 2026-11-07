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
        // perpendicular (Born & Wolf, Principles of Optics, 7th ed., §1.5).
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
}
