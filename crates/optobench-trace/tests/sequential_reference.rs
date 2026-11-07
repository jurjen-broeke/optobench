//! M1 exit criteria: thin and thick lens and parabola cases against theory.
//!
//! Every reference value comes from a closed-form result, cited where it is used. Every
//! traced ray also checks the trace invariants of docs/design.md, Tests and validation:
//! unit-length directions and conserved power (no losses are modelled in M1).

use optobench_math::Vec3;
use optobench_surfaces::{Aperture, Conic, Plane, Sphere};
use optobench_trace::{Ray, SequentialSurface, SequentialSystem, Status, TraceResult};

const WAVELENGTH: f64 = 0.5876; // µm; any value, indices are constant in M1.
const POWER: f64 = 1.0e-3; // W

fn ray(position: Vec3, direction: Vec3) -> Ray {
    Ray::new(position, direction, WAVELENGTH, POWER).expect("non-zero direction")
}

/// Checks the invariants on a completed trace and returns the final ray.
fn completed(result: TraceResult) -> Ray {
    assert_eq!(result.status, Status::Complete);
    assert!(
        (result.ray.direction.length() - 1.0).abs() < 1e-12,
        "direction stays unit length"
    );
    assert!(
        (result.ray.power - POWER).abs() < 1e-15,
        "power is conserved"
    );
    result.ray
}

/// Where a ray in the x–z plane crosses the optical axis (x = 0), as a z coordinate.
fn axis_crossing(ray: &Ray) -> f64 {
    ray.position.z - ray.position.x * ray.direction.z / ray.direction.x
}

/// A singlet in air: radii r1, r2 (mm), centre thickness t (mm), index n.
fn singlet(r1: f64, r2: f64, t: f64, n: f64) -> SequentialSystem {
    SequentialSystem {
        object_index: 1.0,
        surfaces: vec![
            SequentialSurface::refracting(Sphere::from_radius(r1), t, n),
            SequentialSurface::refracting(Sphere::from_radius(r2), 0.0, 1.0),
        ],
    }
}

#[test]
fn thin_lens_in_air_follows_the_lensmaker_equation() {
    // Lensmaker's equation for a thin lens in air: 1/f = (n − 1)(1/R₁ − 1/R₂)
    // (Hecht, Optics, 5th ed., §5.2.3).
    for (r1, r2, n) in [(50.0, -50.0, 1.5), (40.0, 120.0, 1.7), (-80.0, 60.0, 1.45)] {
        let expected = 1.0 / ((n - 1.0) * (1.0 / r1 - 1.0 / r2));
        let points = singlet(r1, r2, 0.0, n)
            .cardinal_points()
            .expect("non-zero power");
        assert!((points.effective_focal_length - expected).abs() < 1e-9 * expected.abs());
        // A thin lens has its principal planes at the lens: BFL = FFL = f.
        assert!((points.back_focal_length - expected).abs() < 1e-9 * expected.abs());
        assert!((points.front_focal_length - expected).abs() < 1e-9 * expected.abs());
    }
}

#[test]
fn thick_singlet_matches_the_thick_lens_formulas() {
    // Thick lens in air (Hecht, Optics, 5th ed., §6.1):
    //   1/f = (n − 1) [1/R₁ − 1/R₂ + (n − 1) t / (n R₁ R₂)]
    //   BFL = f − f (n − 1) t / (n R₁)
    //   FFL = f + f (n − 1) t / (n R₂)
    // Principal planes: H at −f (n − 1) t / (n R₂) from the first vertex, H′ at
    // −f (n − 1) t / (n R₁) from the last.
    let (r1, r2, t, n) = (50.0, -100.0, 10.0, 1.5);
    let f = 1.0 / ((n - 1.0) * (1.0 / r1 - 1.0 / r2 + (n - 1.0) * t / (n * r1 * r2)));
    let bfl = f - f * (n - 1.0) * t / (n * r1);
    let ffl = f + f * (n - 1.0) * t / (n * r2);
    let h_front = -f * (n - 1.0) * t / (n * r2);
    let h_rear = -f * (n - 1.0) * t / (n * r1);

    let system = singlet(r1, r2, t, n);
    let points = system.cardinal_points().expect("non-zero power");
    assert!((points.effective_focal_length - f).abs() < 1e-9);
    assert!((points.back_focal_length - bfl).abs() < 1e-9);
    assert!((points.front_focal_length - ffl).abs() < 1e-9);
    assert!((points.front_principal_plane - h_front).abs() < 1e-9);
    assert!((points.rear_principal_plane - h_rear).abs() < 1e-9);

    // Real rays close to the axis focus at the paraxial back focus. The gap is
    // longitudinal spherical aberration, which to third order grows as h² (Hecht, Optics,
    // 5th ed., §6.3.1), so gap / h² must settle to a constant as h shrinks.
    let last_vertex = system.placement(1).translation.z;
    let ratios: Vec<f64> = [0.1, 0.01, 0.001]
        .iter()
        .map(|&h| {
            let out = completed(system.trace(ray(Vec3::new(h, 0.0, -20.0), Vec3::Z)));
            let gap = (axis_crossing(&out) - (last_vertex + bfl)).abs();
            assert!(gap < 2.0 * h * h, "h = {h}: gap {gap}");
            gap / (h * h)
        })
        .collect();
    assert!(
        (ratios[1] - ratios[2]).abs() < 1e-3 * ratios[2],
        "{ratios:?}"
    );
}

#[test]
fn parabolic_mirror_focuses_collimated_light_to_one_point() {
    // A paraboloid (k = −1) focuses rays parallel to its axis exactly at its focus, at
    // R/2 from the vertex (the focal property of the parabola; Hecht, Optics, 5th ed.,
    // ch. 5, aspherical mirrors). Concave towards the incoming light: R = −200 mm, so the
    // focus lies at z = −100 mm and light returns towards −z.
    let radius = -200.0;
    let focus = radius / 2.0;
    let mirror = |conic: f64| SequentialSystem {
        object_index: 1.0,
        surfaces: vec![
            SequentialSurface::reflecting(
                Conic {
                    curvature: 1.0 / radius,
                    conic,
                },
                focus,
                1.0,
            ),
            SequentialSurface::refracting(Plane, 0.0, 1.0),
        ],
    };

    let parabola = mirror(-1.0);
    let mut worst: f64 = 0.0;
    for x in [-40.0, -25.0, -10.0, 0.0, 5.0, 30.0] {
        for y in [-35.0, 0.0, 12.5, 40.0] {
            let result = parabola.trace(ray(Vec3::new(x, y, -50.0), Vec3::Z));
            let hit = completed(result).position;
            assert!((hit.z - focus).abs() < 1e-12);
            worst = worst.max(hit.x.hypot(hit.y));
        }
    }
    assert!(worst < 1e-12, "largest distance from the focus: {worst} mm");

    // The paraxial trace agrees: the back focus of the mirror alone lies R/2 from its
    // vertex, and its effective focal length is −R/2 = 100 mm (a converging mirror).
    let mirror_only = SequentialSystem {
        object_index: 1.0,
        surfaces: parabola.surfaces.into_iter().take(1).collect(),
    };
    let points = mirror_only.cardinal_points().expect("non-zero power");
    assert!((points.back_focal_length - focus).abs() < 1e-9);
    assert!((points.effective_focal_length + radius / 2.0).abs() < 1e-9);

    // Not a vacuous check: a sphere of the same radius does not focus perfectly.
    let sphere = mirror(0.0);
    let hit = completed(sphere.trace(ray(Vec3::new(40.0, 0.0, -50.0), Vec3::Z))).position;
    assert!(hit.x.abs() > 1e-3);
}

#[test]
fn optical_path_is_equal_for_all_rays_to_a_perfect_focus() {
    // Fermat's principle: every ray from a plane wavefront to a perfect focus has the same
    // optical path length (Born & Wolf, Principles of Optics, 7th ed., §3.3).
    let system = SequentialSystem {
        object_index: 1.0,
        surfaces: vec![
            SequentialSurface::reflecting(
                Conic {
                    curvature: -1.0 / 200.0,
                    conic: -1.0,
                },
                -100.0,
                1.0,
            ),
            SequentialSurface::refracting(Plane, 0.0, 1.0),
        ],
    };
    let paths: Vec<f64> = [0.0, 10.0, 25.0, 40.0]
        .iter()
        .map(|&h| completed(system.trace(ray(Vec3::new(h, 0.0, -50.0), Vec3::Z))).optical_path)
        .collect();
    for path in &paths {
        assert!((path - paths[0]).abs() < 1e-10, "{paths:?}");
    }
}

#[test]
fn trace_reports_vignetting_misses_and_total_internal_reflection() {
    let mut system = singlet(50.0, -50.0, 5.0, 1.5);
    system.surfaces[0].aperture = Aperture::Circular {
        semi_diameter: 10.0,
    };
    let status = |h: f64| system.trace(ray(Vec3::new(h, 0.0, -10.0), Vec3::Z)).status;
    assert_eq!(status(5.0), Status::Complete);
    assert_eq!(status(15.0), Status::Vignetted { surface: 0 });
    assert_eq!(status(60.0), Status::Missed { surface: 0 });

    // Inside a glass block, a ray steeper than the critical angle cannot leave through
    // the flat back face (sin θ_c = 1/1.5, θ_c ≈ 41.8°; Hecht, Optics, 5th ed., ch. 4).
    let block = SequentialSystem {
        object_index: 1.5,
        surfaces: vec![SequentialSurface::refracting(Plane, 0.0, 1.0)],
    };
    let steep = 45.0_f64.to_radians();
    let result = block.trace(ray(
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(steep.sin(), 0.0, steep.cos()),
    ));
    assert_eq!(
        result.status,
        Status::TotalInternalReflection { surface: 0 }
    );
}
