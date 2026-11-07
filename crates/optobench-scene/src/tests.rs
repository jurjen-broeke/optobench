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
    }
}

fn detector(id: &str, frame: Frame) -> Detector {
    Detector {
        id: id.into(),
        frame,
        size: (20.0, 20.0),
        pixels: (40, 40),
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
            coating: Coating::Ideal,
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
