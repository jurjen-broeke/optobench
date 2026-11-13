//! Scene tests: layouts, building and tracing. Reference values from geometry and the
//! lens equations already verified in optobench-trace.

use super::*;
use crate::layout::{PlanarLayout, TableLayout, frame_along};

const WAVELENGTH: f64 = 0.55;

fn beam(frame: Frame, diameter: f64) -> Source {
    Source {
        id: "laser".into(),
        frame,
        kind: SourceKind::Collimated { diameter },
        wavelength: WAVELENGTH,
        power: 1.0,
        rings: 6,
        spectrum: vec![],
        pulse: None,
    }
}

fn detector(id: &str, frame: Frame) -> Detector {
    Detector {
        id: id.into(),
        frame,
        size: (20.0, 20.0),
        pixels: (40, 40),
        quantum_efficiency: None,
    }
}

fn singlet(id: &str, frame: Frame, r1: f64, r2: f64, thickness: f64) -> Element {
    Element {
        id: id.into(),
        frame,
        kind: ElementKind::Lens {
            surfaces: vec![
                LensSurface {
                    radius: r1,
                    conic: 0.0,
                },
                LensSurface {
                    radius: r2,
                    conic: 0.0,
                },
            ],
            thicknesses: vec![thickness],
            materials: vec![1.5.into()],
            diameter: 25.4,
            clear_aperture: 22.9,
            coatings: vec![Coating::Ideal; 2],
        },
    }
}

fn empty() -> Scene {
    Scene {
        surrounding: 1.0.into(),
        elements: vec![],
        sources: vec![],
        proxies: vec![],
        detectors: vec![],
        solids: vec![],
        split_cutoff: None,
        dofs: Vec::new(),
        fluorophores: Vec::new(),
        thermal: Vec::new(),
    }
}

fn balanced(trace: &SceneTrace) -> bool {
    trace.ledger.imbalance().abs() < 1e-12
}

#[test]
fn collimated_beam_focuses_at_the_back_focus_of_a_singlet() {
    // Thick lens in air, R₁ = 50, R₂ = −50, t = 4, n = 1.5 (Hecht, Optics, 5th ed., ch. 6):
    // 1/f = (n − 1)(2/50 − (n − 1)t/(n · 2500)), BFL = f − f(n − 1)t/(n R₁).
    let (r1, r2, t, n) = (50.0, -50.0, 4.0, 1.5);
    let f = 1.0 / ((n - 1.0) * (1.0 / r1 - 1.0 / r2 + (n - 1.0) * t / (n * r1 * r2)));
    let bfl = f - f * (n - 1.0) * t / (n * r1);
    let layout = PlanarLayout::default();
    let at = |d: f64| layout.frame(d).expect("valid");
    let scene = Scene {
        elements: vec![singlet("L1", at(20.0), r1, r2, t)],
        sources: vec![beam(at(0.0), 2.0)],
        detectors: vec![detector("focus", at(20.0 + t + bfl))],
        ..empty()
    };
    let trace = scene.trace().expect("valid scene");
    let result = &trace.detectors[0];
    assert_eq!(result.rays, trace.records.len());
    assert!(
        (result.power - 1.0).abs() < 1e-12,
        "ideal coating, nothing clipped"
    );
    // A 2 mm beam at f ≈ 50 mm: spherical aberration leaves a spot of a few µm.
    assert!(result.rms_radius < 5e-3, "rms {}", result.rms_radius);
    assert!(result.centroid.0.abs() < 1e-12 && result.centroid.1.abs() < 1e-12);
    // The irradiance map holds all the power.
    let pixel_area_m2 = (20.0 / 40.0) * (20.0 / 40.0) * 1e-6;
    let total: f64 = result.irradiance.iter().map(|e| e * pixel_area_m2).sum();
    assert!((total - 1.0).abs() < 1e-12);
    assert!(balanced(&trace));
}

#[test]
fn planar_fold_sends_the_beam_along_the_new_axis() {
    let layout = PlanarLayout {
        folds: vec![(100.0, 90.0)],
        ..PlanarLayout::default()
    };
    let scene = Scene {
        elements: vec![Element {
            id: "M1".into(),
            frame: layout.fold_frame(100.0).expect("valid"),
            kind: ElementKind::Mirror {
                radius: f64::INFINITY,
                diameter: 25.4,
                clear_aperture: 22.9,
            },
        }],
        sources: vec![beam(layout.frame(0.0).expect("valid"), 4.0)],
        detectors: vec![detector("screen", layout.frame(200.0).expect("valid"))],
        ..empty()
    };
    let trace = scene.trace().expect("valid scene");
    assert!((trace.detectors[0].power - 1.0).abs() < 1e-12);
    // The detector sits at (100, 100, 0), facing +y; the beam arrives centred.
    let centre = scene.detectors[0].frame.origin();
    assert!((centre - Vec3::new(100.0, 100.0, 0.0)).length() < 1e-12);
    let (cx, cy) = trace.detectors[0].centroid;
    assert!(cx.abs() < 1e-12 && cy.abs() < 1e-12);
    assert!(balanced(&trace));
}

#[test]
fn table_periscope_raises_the_beam() {
    // A beam at 50 mm height heading +x; mirror 1 at hole (4, 0) sends it up, mirror 2
    // 100 mm higher sends it on along +x. Mirror normals bisect the reversed incoming and
    // outgoing directions.
    let table = TableLayout { pitch: 25.0 };
    let mirror = |id: &str, height: f64, normal: Vec3| Element {
        id: id.into(),
        frame: table.frame((4.0, 0.0), height, normal).expect("valid"),
        kind: ElementKind::Mirror {
            radius: f64::INFINITY,
            diameter: 25.4,
            clear_aperture: 22.9,
        },
    };
    let scene = Scene {
        elements: vec![
            mirror("M1", 50.0, Vec3::new(-1.0, 0.0, 1.0)),
            mirror("M2", 150.0, Vec3::new(1.0, 0.0, -1.0)),
        ],
        sources: vec![beam(
            table.frame((0.0, 0.0), 50.0, Vec3::X).expect("valid"),
            4.0,
        )],
        detectors: vec![detector(
            "screen",
            table.frame((8.0, 0.0), 150.0, Vec3::X).expect("valid"),
        )],
        ..empty()
    };
    let trace = scene.trace().expect("valid scene");
    let result = &trace.detectors[0];
    assert!((result.power - 1.0).abs() < 1e-12);
    assert!(result.centroid.0.abs() < 1e-12 && result.centroid.1.abs() < 1e-12);
    // Every ray made two reflections: start, M1, M2, detector.
    assert!(trace.records.iter().all(|r| r.points.len() == 4));
    assert!(balanced(&trace));
}

#[test]
fn tube_clips_the_wide_part_of_a_cone() {
    // A point source at the entrance of a tube of inner diameter 10 mm and length 100 mm.
    // Rays steeper than 5/100 (tan θ) meet the wall; a cone of half-angle 0.1 rad has
    // rays on both sides of that.
    let layout = PlanarLayout::default();
    let at = |d: f64| layout.frame(d).expect("valid");
    let scene = Scene {
        sources: vec![Source {
            id: "point".into(),
            frame: at(0.0),
            kind: SourceKind::Point { half_angle: 0.1 },
            wavelength: WAVELENGTH,
            power: 1.0,
            rings: 8,
            spectrum: vec![],
            pulse: None,
        }],
        proxies: vec![Proxy {
            id: "tube".into(),
            frame: at(0.0),
            kind: ProxyKind::Tube {
                inner_diameter: 10.0,
                length: 100.0,
            },
        }],
        detectors: vec![detector("screen", at(110.0))],
        ..empty()
    };
    let trace = scene.trace().expect("valid scene");
    assert_eq!(trace.clipping.len(), 1);
    let clipped = &trace.clipping[0];
    assert_eq!(
        (clipped.id.as_str(), clipped.owner),
        ("tube", Owner::Proxy(0))
    );
    // Each ray's verdict follows from its slope alone.
    for record in &trace.records {
        let d = record.launched.direction;
        let slope = d.y.hypot(d.z) / d.x;
        match record.end {
            End::Clipped { .. } => assert!(slope > 0.05 - 1e-12),
            End::Detected { .. } => assert!(slope <= 0.05 + 1e-12),
            other => panic!("unexpected end {other:?}"),
        }
    }
    assert!(clipped.power > 0.0 && trace.detectors[0].power > 0.0);
    assert!(balanced(&trace));
}

#[test]
fn invalid_scenes_are_rejected_with_the_item_named() {
    let frame = frame_along(Vec3::ZERO, Vec3::X).expect("valid");
    let message = |scene: Scene| match scene.build() {
        Err(SceneError::Invalid(message)) => message,
        Ok(_) => String::from("accepted"),
    };
    // A sphere of radius 10 cannot span a 25.4 mm lens.
    let small = Scene {
        elements: vec![singlet("L1", frame, 10.0, -50.0, 5.0)],
        ..empty()
    };
    assert!(message(small).contains("smaller than the lens diameter"));
    // Too thin: the rims cross.
    let thin = Scene {
        elements: vec![singlet("L2", frame, 30.0, -30.0, 1.0)],
        ..empty()
    };
    assert!(message(thin).contains("negative edge thickness"));
    let twice = Scene {
        elements: vec![singlet("same", frame, 50.0, -50.0, 5.0)],
        detectors: vec![detector("same", frame)],
        ..empty()
    };
    assert!(message(twice).contains("used more than once"));
}

fn free_space(source: Source, detector_z: f64, size: f64) -> SceneTrace {
    // A source on the z axis and a large detector across it at z, nothing between.
    let frame = |z: f64| frame_along(Vec3::new(0.0, 0.0, z), Vec3::Z).expect("valid");
    let scene = Scene {
        sources: vec![Source {
            frame: frame(0.0),
            ..source
        }],
        detectors: vec![Detector {
            id: "screen".into(),
            frame: frame(detector_z),
            size: (size, size),
            pixels: (10, 10),
            quantum_efficiency: None,
        }],
        ..empty()
    };
    scene.trace().expect("traces")
}

fn source(kind: SourceKind, rings: usize) -> Source {
    Source {
        id: "source".into(),
        frame: Frame::PARENT,
        kind,
        wavelength: 1.064,
        power: 1.0,
        rings,
        spectrum: vec![],
        pulse: None,
    }
}

#[test]
fn gaussian_beam_follows_the_embedded_gaussian_law() {
    // w(z) = w₀ √(1 + (z/z_R)²), z_R = π w₀² / (M² λ) (Siegman, Lasers, ch. 17), for the
    // second-moment width 2σₓ of the ray bundle. Per axis σₓ = rms radius / √2.
    let (w0, m2, lambda) = (0.5_f64, 1.3_f64, 1.064e-3_f64);
    let rayleigh = core::f64::consts::PI * w0 * w0 / (m2 * lambda);
    for z in [0.0, rayleigh, 3.0 * rayleigh] {
        let trace = free_space(
            source(
                SourceKind::Gaussian {
                    waist_diameter: 2.0 * w0,
                    m_squared: m2,
                    angle_rings: 4,
                },
                4,
            ),
            z.max(1e-6),
            200.0,
        );
        let result = &trace.detectors[0];
        assert!((result.power - 1.0).abs() < 1e-12);
        let width = 2.0 * result.rms_radius / 2f64.sqrt();
        let expected = w0 * (1.0 + (z / rayleigh).powi(2)).sqrt();
        assert!(
            (width - expected).abs() < 1e-9 * expected,
            "z = {z}: {width} vs {expected}"
        );
    }
}

#[test]
fn fiber_stays_within_its_numerical_aperture() {
    // Directions uniform in direction cosines within NA: the power within a half-angle α
    // is (sin α / NA)², and none leaves beyond asin(NA).
    let na = 0.22;
    let trace = free_space(
        source(
            SourceKind::Fiber {
                core_diameter: 0.05,
                numerical_aperture: na,
                angle_rings: 10,
            },
            2,
        ),
        1.0,
        10.0,
    );
    let angular = &trace.detectors[0].angular;
    let within = |degrees: f64| -> f64 {
        let bins = (degrees / ANGULAR_BIN_DEGREES).round() as usize;
        angular.iter().take(bins).sum()
    };
    for degrees in [5.0_f64, 10.0] {
        // Rings carry whole annuli: the cumulative power can be off by one annulus,
        // 2 s Δ with s = sin α / NA and Δ = 1 / (rings + 0.5).
        let s = degrees.to_radians().sin() / na;
        let expected = s * s;
        let tolerance = 2.0 * s / 10.5;
        assert!(
            (within(degrees) - expected).abs() <= tolerance,
            "{degrees}°: {} vs {expected}",
            within(degrees)
        );
    }
    let edge = na.asin().to_degrees();
    assert!((within(15.0) - 1.0).abs() < 1e-12 && edge < 15.0);
}

#[test]
fn led_is_lambertian() {
    // A Lambertian emitter sends sin²α of its power within a half-angle α.
    let trace = free_space(
        source(
            SourceKind::Led {
                size: 1.0,
                angle_rings: 24,
            },
            1,
        ),
        1e-3,
        100.0,
    );
    let angular = &trace.detectors[0].angular;
    let within = |degrees: f64| -> f64 {
        let bins = (degrees / ANGULAR_BIN_DEGREES).round() as usize;
        angular.iter().take(bins).sum()
    };
    for degrees in [30.0_f64, 45.0, 60.0] {
        // As for the fiber: off by at most one annulus, 2 sin α / (rings + 0.5).
        let s = degrees.to_radians().sin();
        let expected = s * s;
        let tolerance = 2.0 * s / 24.5;
        assert!(
            (within(degrees) - expected).abs() <= tolerance,
            "{degrees}°: {} vs {expected}",
            within(degrees)
        );
    }
    assert!((trace.ledger.launched - 1.0).abs() < 1e-12);
}

#[test]
fn spectrum_shares_the_power_between_its_lines() {
    let mut beam = source(SourceKind::Collimated { diameter: 1.0 }, 2);
    beam.spectrum = vec![(0.488, 1.0), (0.561, 3.0)];
    let rays = Scene::launch(&beam);
    let power_at = |w: f64| -> f64 {
        rays.iter()
            .filter(|r| (r.wavelength - w).abs() < 1e-12)
            .map(|r| r.power)
            .sum()
    };
    assert!((power_at(0.488) - 0.25).abs() < 1e-12 && (power_at(0.561) - 0.75).abs() < 1e-12);
    assert_eq!(rays.len(), 2 * 19);
}

#[test]
fn disk_samples_cover_the_disk_and_stay_inside_it() {
    let samples = disk_samples(5);
    let total: f64 = samples.iter().map(|s| s.2).sum();
    assert!((total - 1.0).abs() < 1e-12);
    assert!(samples.iter().all(|s| s.0.hypot(s.1) < 1.0));
}

#[test]
fn poses_follow_dof_values_however_they_are_reached() {
    use crate::dof::{Dof, Motion, Target};
    use std::collections::BTreeMap;
    let frame = frame_along(Vec3::new(0.0, 0.0, 10.0), Vec3::Z).expect("valid");
    let dof = |name: &str, motion: Motion, value: f64| Dof {
        name: name.into(),
        target: Target::Element("L".into()),
        motion,
        range: None,
        locked: false,
        follows: None,
        value,
    };
    let tilt = Motion::Tilt {
        axis: Vec3::X,
        pivot: Vec3::new(0.0, 0.0, 10.0),
    };
    // As read from a file: nominal pose, values given (slide 1 mm, tilt 0.01 rad).
    let read = Scene {
        elements: vec![singlet("L", frame, 30.0, f64::INFINITY, 4.0)],
        dofs: vec![
            dof("z", Motion::Slide { axis: Vec3::Z }, 1.0),
            dof("tip", tilt, 0.01),
        ],
        ..empty()
    };
    let placed = read.placed().expect("placed");
    let origin = |s: &Scene| s.elements[0].frame.origin();
    let axis = |s: &Scene| s.elements[0].frame.axes()[2];
    // The slide, then the tilt about the fixed pivot: the origin goes to 11 mm along z,
    // then turns 0.01 rad about x through (0, 0, 10).
    let expected = Vec3::new(0.0, -(0.01_f64).sin(), 10.0 + (0.01_f64).cos());
    assert!(
        (origin(&placed) - expected).length() < 1e-12,
        "{:?}",
        origin(&placed)
    );
    // Moving twice ends where moving once does; moving back returns to the nominal pose.
    let values = |z: f64, tip: f64| BTreeMap::from([("z".to_owned(), z), ("tip".to_owned(), tip)]);
    let twice = placed
        .moved(&values(3.0, -0.02))
        .and_then(|s| s.moved(&values(-2.0, 0.005)))
        .expect("moves");
    let once = placed.moved(&values(-2.0, 0.005)).expect("moves");
    assert!((origin(&twice) - origin(&once)).length() < 1e-12);
    assert!((axis(&twice) - axis(&once)).length() < 1e-12);
    let back = once.moved(&values(0.0, 0.0)).expect("moves");
    assert!((origin(&back) - frame.origin()).length() < 1e-12);
    assert!((axis(&back) - frame.axes()[2]).length() < 1e-12);
    assert_eq!(back.dofs[0].value, 0.0);
}

#[test]
fn an_isotropic_emitter_samples_equal_solid_angles() {
    // Into a cone of θ_max the rays carry (1 − cos θ_max) / 2 of the power, and their
    // power-weighted mean cos θ is that of a uniform cap, (1 + cos θ_max) / 2; over the
    // whole sphere the mean direction is zero.
    for degrees in [60.0_f64, 180.0] {
        let theta = degrees.to_radians();
        let emitter = Source {
            kind: SourceKind::Isotropic { half_angle: theta },
            rings: 60,
            ..beam(Frame::PARENT, 1.0)
        };
        let rays = Scene::launch(&emitter);
        let power: f64 = rays.iter().map(|r| r.power).sum();
        assert!((power - (1.0 - theta.cos()) / 2.0).abs() < 1e-12, "{power}");
        let mean = rays.iter().map(|r| r.power * r.direction.z).sum::<f64>() / power;
        assert!(
            (mean - (1.0 + theta.cos()) / 2.0).abs() < 1e-3,
            "{degrees}°: {mean}"
        );
        assert!(
            rays.iter()
                .all(|r| (r.direction.length() - 1.0).abs() < 1e-12)
        );
        assert!(rays.iter().all(|r| r.direction.z >= theta.cos() - 1e-12));
    }
}
