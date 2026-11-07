//! M2b reference tests: pupils, spot diagrams, ray fans, OPD and Zernike fits against
//! closed-form geometry. Each reference value is derived in the test from cited theory.

use optobench_analysis::{Aiming, Analysis, Field};
use optobench_surfaces::{Aperture, Conic, Plane, Sphere};
use optobench_trace::{SequentialSurface, SequentialSystem};

const WAVELENGTH: f64 = 0.55; // µm

/// A concave parabolic mirror, R = −200 mm (focus 100 mm in front of it), used as the
/// stop with an aperture of radius `radius`, and an image plane `defocus` mm beyond the
/// focus along the returning light.
fn parabola(radius: f64, defocus: f64) -> SequentialSystem {
    SequentialSystem {
        object_medium: 1.0.into(),
        surfaces: vec![
            SequentialSurface::reflecting(
                Conic {
                    curvature: -1.0 / 200.0,
                    conic: -1.0,
                },
                -100.0 - defocus,
                1.0,
            )
            .with_aperture(Aperture::Circular {
                semi_diameter: radius,
            }),
            SequentialSurface::refracting(Plane, 0.0, 1.0),
        ],
    }
}

#[test]
fn entrance_pupil_of_a_stop_behind_a_thin_lens() {
    // A stop 20 mm behind a thin lens of f = 100 mm is seen from the front as its
    // virtual image: 1/s′ = 1/f − 1/d gives s′ = d·f/(f − d) = 25 mm behind the lens,
    // magnified s′/d = 1.25 (thin-lens imaging of the stop; Hecht, Optics, 5th ed., ch. 5).
    let (f, d, stop_radius) = (100.0, 20.0, 4.0);
    let r = 2.0 * f * (1.5 - 1.0); // equiconvex, n = 1.5: 1/f = 0.5 · 2/r
    let system = SequentialSystem {
        object_medium: 1.0.into(),
        surfaces: vec![
            SequentialSurface::refracting(Sphere::from_radius(r), 0.0, 1.5),
            SequentialSurface::refracting(Sphere::from_radius(-r), d, 1.0),
            SequentialSurface::refracting(Plane, f - d, 1.0).with_aperture(Aperture::Circular {
                semi_diameter: stop_radius,
            }),
            SequentialSurface::refracting(Plane, 0.0, 1.0),
        ],
    };
    let analysis = Analysis::new(&system, 2, WAVELENGTH, Aiming::Paraxial).expect("valid");
    let pupils = analysis.pupils();
    let image = d * f / (f - d);
    assert!((pupils.entrance_position - image).abs() < 1e-9);
    assert!((pupils.entrance_radius - stop_radius * image / d).abs() < 1e-9);
}

#[test]
fn perfect_focus_has_no_spot_and_no_wavefront_error() {
    // A paraboloid images an on-axis point at infinity perfectly (focal property of the
    // parabola; Hecht, Optics, 5th ed., ch. 5).
    let system = parabola(10.0, 0.0);
    let analysis = Analysis::new(&system, 0, WAVELENGTH, Aiming::Real).expect("valid");
    let pupils = analysis.pupils();
    assert!(pupils.entrance_position.abs() < 1e-12);
    assert!((pupils.entrance_radius - 10.0).abs() < 1e-12);
    assert!(
        pupils.exit_position.abs() < 1e-9,
        "exit pupil at the mirror"
    );

    let spot = analysis.spot_diagram(Field::AXIS, 6).expect("traces");
    assert_eq!(spot.lost(), 0);
    assert!(spot.rms_radius < 1e-12);
    let wavefront = analysis.wavefront(Field::AXIS, 15).expect("traces");
    assert!(
        wavefront.peak_to_valley < 1e-8,
        "{}",
        wavefront.peak_to_valley
    );
}

#[test]
fn defocused_spot_and_wavefront_match_exact_geometry() {
    // With the image plane Δ beyond a perfect focus F, a ray from the mirror at height h
    // passes F and lands at |x| = h·Δ / (F_z − s(h)) from the axis, with s(h) the sag.
    // Its OPD against the reference sphere (centre P′ on the image plane, through the
    // exit pupil centre E at the vertex) is |F − E| − |F − X|, X being where the ray
    // meets that sphere: all rays have equal path to F (Fermat's principle; Born & Wolf,
    // Principles of Optics, 7th ed., ch. 3).
    let (radius, delta) = (10.0_f64, 0.5_f64);
    let system = parabola(radius, delta);
    let analysis = Analysis::new(&system, 0, WAVELENGTH, Aiming::Real).expect("valid");

    let focus_z = -100.0;
    let sag = |h: f64| -h * h / 400.0;
    let landing = |h: f64| h * delta / (sag(h) - focus_z);
    let spot = analysis.spot_diagram(Field::AXIS, 8).expect("traces");
    assert!(spot.centroid.0.abs() < 1e-12 && spot.centroid.1.abs() < 1e-12);
    assert!((spot.geometric_radius - landing(radius)).abs() < 1e-12);

    let fan = analysis.ray_fan(Field::AXIS, 5).expect("traces");
    for point in &fan.tangential {
        let h = point.pupil * radius;
        let y = point.y.expect("arrives");
        // Past the focus the ray has crossed the axis: it lands on the opposite side.
        assert!((y + landing(h)).abs() < 1e-12, "p = {}", point.pupil);
    }

    // Exact OPD of the marginal ray.
    let centre = (0.0, focus_z - delta); // P′ in the x–z plane
    let sphere_radius = (focus_z - delta).abs(); // |E − P′|, E at the origin
    let mirror_point = (radius, sag(radius));
    let towards_focus = {
        let (dx, dz) = (0.0 - mirror_point.0, focus_z - mirror_point.1);
        let length = dx.hypot(dz);
        (dx / length, dz / length)
    };
    // Points on the ray: M + t·u. Solve |M + t·u − P′|² = ρ² on the pupil side (z > P′).
    let w = (mirror_point.0 - centre.0, mirror_point.1 - centre.1);
    let half_b = w.0 * towards_focus.0 + w.1 * towards_focus.1;
    let c = w.0 * w.0 + w.1 * w.1 - sphere_radius * sphere_radius;
    let t = [
        -half_b - (half_b * half_b - c).sqrt(),
        -half_b + (half_b * half_b - c).sqrt(),
    ]
    .into_iter()
    .find(|t| mirror_point.1 + t * towards_focus.1 > centre.1)
    .expect("meets the sphere");
    let x = (
        mirror_point.0 + t * towards_focus.0,
        mirror_point.1 + t * towards_focus.1,
    );
    let to_focus = |p: (f64, f64)| p.0.hypot(p.1 - focus_z);
    let expected_waves = (to_focus((0.0, 0.0)) - to_focus(x)) / (WAVELENGTH * 1e-3);
    let opd = analysis
        .optical_path_difference(Field::AXIS, 1.0, 0.0)
        .expect("traces")
        .expect("arrives");
    assert!(
        (opd - expected_waves).abs() < 1e-6,
        "{opd} vs {expected_waves}"
    );

    // A defocused wavefront is (almost) pure Zernike defocus: Z₄ = √3 (2ρ² − 1), so a
    // wavefront W(ρ) = W₂₀ρ² has Z₄ coefficient W₂₀ / (2√3) (Thibos et al. 2002).
    let wavefront = analysis.wavefront(Field::AXIS, 21).expect("traces");
    let fit = wavefront.zernike(4).expect("enough samples");
    let defocus = fit[4];
    assert!((defocus - opd / (2.0 * 3f64.sqrt())).abs() < 1e-3 * opd.abs());
    for (j, c) in fit.iter().enumerate() {
        if j != 0 && j != 4 {
            assert!(c.abs() < 1e-3 * defocus.abs(), "Z{j} = {c}");
        }
    }
}

#[test]
fn off_axis_parabola_shows_third_order_coma() {
    // A paraboloid with the stop at the mirror has no spherical aberration but coma off
    // axis. For third-order coma the tangential coma is three times the sagittal coma
    // (Hecht, Optics, 5th ed., ch. 6; Welford, Aberrations of Optical Systems, 1986).
    // Averaging the p = ±1 tangential rays removes the odd (defocus, astigmatism) part.
    let system = parabola(5.0, 0.0);
    let analysis = Analysis::new(&system, 0, WAVELENGTH, Aiming::Real).expect("valid");
    let field = Field::tangential(0.2_f64.to_radians());
    let fan = analysis.ray_fan(field, 3).expect("traces");
    let tangential =
        0.5 * (fan.tangential[0].y.expect("arrives") + fan.tangential[2].y.expect("arrives"));
    let sagittal = fan.sagittal[2].y.expect("arrives");
    assert!(sagittal.abs() > 1e-7, "coma is present");
    assert!(
        (tangential / sagittal - 3.0).abs() < 0.01,
        "ratio {}",
        tangential / sagittal
    );

    // In the Zernike fit, vertical coma (ANSI j = 7, n = 3, m = −1) dominates the
    // non-trivial terms; horizontal coma (j = 8) vanishes by symmetry.
    let fit = analysis
        .wavefront(field, 21)
        .expect("traces")
        .zernike(4)
        .expect("fits");
    assert!(fit[7].abs() > 10.0 * fit[8].abs());
    for j in [3, 5, 6, 9, 10, 11, 12, 13, 14] {
        assert!(
            fit[7].abs() > fit[j].abs(),
            "Z7 = {}, Z{j} = {}",
            fit[7],
            fit[j]
        );
    }
}

#[test]
fn real_aiming_lands_on_the_real_stop_edge() {
    // Stop behind a fast lens: paraxial aiming misses the stop edge because of pupil
    // aberration; real aiming must land within its tolerance.
    let stop_radius = 6.0;
    let system = SequentialSystem {
        object_medium: 1.0.into(),
        surfaces: vec![
            SequentialSurface::refracting(Sphere::from_radius(40.0), 8.0, 1.6),
            SequentialSurface::refracting(Sphere::from_radius(-200.0), 10.0, 1.0),
            SequentialSurface::refracting(Plane, 40.0, 1.0).with_aperture(Aperture::Circular {
                semi_diameter: stop_radius,
            }),
            SequentialSurface::refracting(Plane, 0.0, 1.0),
        ],
    };
    let stop_edge = |aiming: Aiming| {
        let analysis = Analysis::new(&system, 2, WAVELENGTH, aiming).expect("valid");
        let result = analysis
            .trace(Field::tangential(5.0_f64.to_radians()), 0.0, 1.0)
            .expect("aims");
        let point = result.points[2];
        point.x.hypot(point.y)
    };
    assert!((stop_edge(Aiming::Real) - stop_radius).abs() < 1e-9);
    assert!((stop_edge(Aiming::Paraxial) - stop_radius).abs() > 1e-4);
}

#[test]
fn clipped_rays_are_counted_as_lost() {
    // An aperture smaller than the beam at the image plane's predecessor clips the edge.
    let mut system = parabola(10.0, 0.0);
    system.surfaces.insert(
        1,
        SequentialSurface::refracting(Plane, 0.0, 1.0)
            .with_aperture(Aperture::Circular { semi_diameter: 2.0 }),
    );
    // The new plane sits 50 mm in front of the focus: thicknesses −50 and −50.
    system.surfaces[0].thickness = -50.0;
    system.surfaces[1].thickness = -50.0;
    let analysis = Analysis::new(&system, 0, WAVELENGTH, Aiming::Paraxial).expect("valid");
    let spot = analysis.spot_diagram(Field::AXIS, 5).expect("traces");
    // The beam radius halfway to focus is about 5 mm, so rays beyond ~0.4 of the pupil
    // are clipped by the 2 mm aperture.
    assert!(spot.lost() > 0 && spot.lost() < spot.launched);
}
