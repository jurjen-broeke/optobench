//! M3a reference tests for the non-sequential trace. The sequential trace (checked
//! against theory in M1 and M2) is the reference for refraction and Fresnel losses; the
//! rest is checked against geometry. Every test checks the power ledger.

use std::sync::Arc;

use optobench_math::{Transform, Vec3};
use optobench_surfaces::{Plane, Sphere};
use optobench_trace::nonsequential::{End, Face, NonSequentialSystem, Region, Role, Shape};
use optobench_trace::{Coating, Ray, SequentialSurface, SequentialSystem};

const WAVELENGTH: f64 = 0.55;
const AIR: usize = 0;

fn ray(position: Vec3, direction: Vec3) -> Ray {
    Ray::new(position, direction, WAVELENGTH, 1.0).expect("non-zero direction")
}

fn at(z: f64) -> Transform {
    Transform::from_translation(Vec3::new(0.0, 0.0, z))
}

fn plane(z: f64, region: Region, role: Role, negative: usize, positive: usize) -> Face {
    Face {
        shape: Shape::Profile(Arc::new(Plane)),
        placement: at(z),
        region,
        role,
        negative_medium: negative,
        positive_medium: positive,
        owner: 0,
    }
}

fn detector(z: f64) -> Face {
    plane(
        z,
        Region::Rectangle {
            half_width: 100.0,
            half_height: 100.0,
        },
        Role::Detector { detector: 0 },
        AIR,
        AIR,
    )
}

fn balanced(ledger: &optobench_trace::nonsequential::Ledger) -> bool {
    ledger.imbalance().abs() < 1e-12 * ledger.launched.max(1.0)
}

const REFRACT: Role = Role::Refract {
    coating: Coating::Uncoated,
    clear_radius: None,
};

#[test]
fn singlet_matches_the_sequential_trace() {
    // The same thick singlet traced both ways must land rays at the same points with the
    // same power (Fresnel losses at both faces).
    let (r1, r2, t, n) = (50.0, -100.0, 10.0, 1.5);
    let image = 80.0;
    let sequential = SequentialSystem {
        object_medium: 1.0.into(),
        surfaces: vec![
            SequentialSurface::refracting(Sphere::from_radius(r1), t, n),
            SequentialSurface::refracting(Sphere::from_radius(r2), image, 1.0),
            SequentialSurface::refracting(Plane, 0.0, 1.0),
        ],
    };
    let mut system = NonSequentialSystem::new(1.0.into());
    let glass = system.add_medium(n.into());
    let lens_face = |radius: f64, z: f64, negative, positive| Face {
        shape: Shape::Profile(Arc::new(Sphere::from_radius(radius))),
        placement: at(z),
        region: Region::Disk { radius: 20.0 },
        role: REFRACT,
        negative_medium: negative,
        positive_medium: positive,
        owner: 1,
    };
    system.faces = vec![
        lens_face(r1, 0.0, AIR, glass),
        lens_face(r2, t, glass, AIR),
        detector(t + image),
    ];
    let rays: Vec<Ray> = [0.0, 2.0, 5.0, 9.0, 14.0]
        .iter()
        .map(|&h| ray(Vec3::new(h, 0.5 * h, -10.0), Vec3::new(0.01, 0.0, 1.0)))
        .collect();
    let (records, ledger) = system.trace_all(rays.clone());
    assert!(balanced(&ledger));
    for (record, launched) in records.iter().zip(rays) {
        let expected = sequential.trace(launched);
        assert!(matches!(record.end, End::Detected { detector: 0, .. }));
        assert!((record.ray.position - expected.ray.position).length() < 1e-11);
        assert!((record.ray.power - expected.ray.power).abs() < 1e-14);
        assert!((record.ray.optical_path - expected.ray.optical_path).abs() < 1e-10);
        assert_eq!(record.points.len(), 4, "start, two faces, detector");
    }
}

#[test]
fn total_internal_reflection_is_followed() {
    // A ray inside glass (n = 1.5) meets the flat top face at 45°, beyond the critical
    // angle of 41.8° (sin θc = 1/1.5; Hecht, Optics, 5th ed., ch. 4), reflects without
    // loss and reaches a detector below.
    let mut system = NonSequentialSystem::new(1.0.into());
    let glass = system.add_medium(1.5.into());
    system.start_medium = glass;
    let disk = Region::Disk { radius: 100.0 };
    system.faces = vec![
        // The glass lies below the face (local −z side).
        plane(0.0, disk, REFRACT, glass, AIR),
        plane(-20.0, disk, Role::Detector { detector: 7 }, glass, glass),
    ];
    let start = Vec3::new(0.0, 0.0, -10.0);
    let direction = Vec3::new(1.0, 0.0, 1.0);
    let (records, ledger) = system.trace_all([ray(start, direction)]);
    let record = &records[0];
    assert!(matches!(record.end, End::Detected { detector: 7, .. }));
    // Up 10, across 10 to the face, then down 20 and across 20 to the detector.
    assert!((record.ray.position - Vec3::new(30.0, 0.0, -20.0)).length() < 1e-12);
    assert!(
        (record.ray.power - 1.0).abs() < 1e-15,
        "no loss in total internal reflection"
    );
    assert!(balanced(&ledger));
}

#[test]
fn a_soft_rim_passes_a_share_falling_linearly_across_it() {
    // An iris plate at z = 10 with a hole of 1 mm radius and a soft rim out to 3 mm,
    // a detector at z = 20. Rays parallel to the axis at r = 0.5 (in the hole), 2 (the
    // rim's middle), 2.5 and 3.5 (beyond it) keep 1, (3 − r)/(3 − 1) = 1/2, 1/4 and
    // nothing (geometry of the definition); what they lose is clipping.
    let mut system = NonSequentialSystem::new(1.0.into());
    system.faces = vec![
        plane(
            10.0,
            Region::SquareWithHole {
                half_size: 50.0,
                hole_radius: 1.0,
            },
            Role::SoftEdge {
                inner: 1.0,
                outer: 3.0,
            },
            AIR,
            AIR,
        ),
        detector(20.0),
    ];
    let rays: Vec<Ray> = [0.5, 2.0, 2.5, 3.5]
        .iter()
        .map(|&x| ray(Vec3::new(x, 0.0, 0.0), Vec3::Z))
        .collect();
    let (records, ledger) = system.trace_all(rays);
    let kept: Vec<f64> = records
        .iter()
        .map(|r| match r.end {
            End::Detected { .. } => r.ray.power,
            _ => 0.0,
        })
        .collect();
    for (k, expected) in kept.iter().zip([1.0, 0.5, 0.25, 0.0]) {
        assert!((k - expected).abs() < 1e-15, "{kept:?}");
    }
    assert!(matches!(records[3].end, End::Clipped { face: 0 }));
    assert_eq!(records[1].edge_losses, [(0, 0.5)]);
    assert!((ledger.clipped - (0.5 + 0.75 + 1.0)).abs() < 1e-15);
    assert!(balanced(&ledger));
}

#[test]
fn tube_wall_clips_steep_rays_and_passes_axial_ones() {
    // A tube of inner radius 5 mm, 100 mm long. A ray parallel to the axis passes; a ray
    // at slope 0.1 from the axis meets the wall at z = 50 mm (r = 5 = 0.1·z).
    let mut system = NonSequentialSystem::new(1.0.into());
    system.faces = vec![
        Face {
            shape: Shape::Cylinder {
                radius: 5.0,
                length: 100.0,
            },
            placement: Transform::IDENTITY,
            region: Region::Disk { radius: 0.0 },
            role: Role::Absorber,
            negative_medium: AIR,
            positive_medium: AIR,
            owner: 3,
        },
        detector(120.0),
    ];
    let (records, ledger) = system.trace_all([
        ray(Vec3::new(2.0, 0.0, 0.0), Vec3::Z),
        ray(Vec3::ZERO, Vec3::new(0.1, 0.0, 1.0)),
    ]);
    assert!(matches!(records[0].end, End::Detected { .. }));
    assert_eq!(records[1].end, End::Clipped { face: 0 });
    assert!((records[1].ray.position - Vec3::new(5.0, 0.0, 50.0)).length() < 1e-12);
    assert!((ledger.clipped - 1.0).abs() < 1e-15 && (ledger.detected - 1.0).abs() < 1e-15);
    assert!(balanced(&ledger));
}

#[test]
fn plate_with_hole_and_lens_edge_clip() {
    // A square plate (half size 20 mm) with a 4 mm hole radius, then a lens face whose
    // clear aperture (6 mm) is smaller than its diameter (10 mm radius).
    let mut system = NonSequentialSystem::new(1.0.into());
    let glass = system.add_medium(1.5.into());
    system.faces = vec![
        plane(
            0.0,
            Region::SquareWithHole {
                half_size: 20.0,
                hole_radius: 4.0,
            },
            Role::Absorber,
            AIR,
            AIR,
        ),
        Face {
            shape: Shape::Profile(Arc::new(Plane)),
            placement: at(10.0),
            region: Region::Disk { radius: 10.0 },
            role: Role::Refract {
                coating: Coating::Ideal,
                clear_radius: Some(6.0),
            },
            negative_medium: AIR,
            positive_medium: glass,
            owner: 5,
        },
    ];
    let end = |x: f64, z: f64, dx: f64| {
        system
            .trace(ray(Vec3::new(x, 0.0, z), Vec3::new(dx, 0.0, 1.0)))
            .end
    };
    assert_eq!(
        end(3.0, -5.0, 0.0),
        End::Escaped,
        "through hole and clear aperture"
    );
    assert_eq!(
        end(5.0, -5.0, 0.0),
        End::Clipped { face: 0 },
        "on the plate"
    );
    assert_eq!(
        end(30.0, -5.0, 0.0),
        End::Escaped,
        "beside the plate and the lens"
    );
    // Starts after the plate, 8 mm off axis: inside the lens but outside its clear aperture.
    assert_eq!(
        end(8.0, 5.0, 0.0),
        End::Clipped { face: 1 },
        "at the lens edge"
    );
}

#[test]
fn facing_mirrors_reach_the_interaction_limit() {
    let mut system = NonSequentialSystem::new(1.0.into());
    system.interaction_limit = 50;
    let disk = Region::Disk { radius: 10.0 };
    let mirror = Role::Mirror { clear_radius: None };
    system.faces = vec![
        plane(0.0, disk, mirror.clone(), AIR, AIR),
        plane(10.0, disk, mirror, AIR, AIR),
    ];
    let (records, ledger) = system.trace_all([ray(Vec3::new(0.0, 0.0, 5.0), Vec3::Z)]);
    assert_eq!(records[0].end, End::Stopped);
    assert_eq!(records[0].points.len(), 51);
    assert!((ledger.stopped - 1.0).abs() < 1e-15);
    assert!(balanced(&ledger));
}

#[test]
fn touching_solids_refract_once_at_their_shared_face() {
    // Two slabs as solids from exact faces: A (n = 1.5) from z = 0 to 5, B (n = 1.7) from
    // z = 5 to 10, touching at z = 5. Each face has the body's glass behind it and air in
    // front. A ray along z must cross z = 5 once, glass to glass, so the power follows the
    // normal-incidence Fresnel transmissions air→1.5, 1.5→1.7, 1.7→air (Hecht, Optics,
    // 5th ed., ch. 4), with no air gap at the shared face.
    use optobench_math::Frame;
    use optobench_surfaces::brep::{BrepFace, FaceSurface};

    let square = |z: f64, up: bool| -> Face {
        let corners = [(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0)];
        let mut ring: Vec<Vec3> = corners.iter().map(|&(x, y)| Vec3::new(x, y, z)).collect();
        if !up {
            ring.reverse();
        }
        let frame = Frame::new(Vec3::new(0.0, 0.0, z), Vec3::Z, Vec3::X).expect("valid");
        Face {
            shape: Shape::Brep(Arc::new(BrepFace::new(
                FaceSurface::Plane(frame),
                !up,
                vec![ring],
            ))),
            placement: Transform::IDENTITY,
            region: Region::Disk { radius: 0.0 },
            role: REFRACT,
            negative_medium: 0, // set below
            positive_medium: AIR,
            owner: 0,
        }
    };
    let mut system = NonSequentialSystem::new(1.0.into());
    let a = system.add_medium(1.5.into());
    let b = system.add_medium(1.7.into());
    let with = |mut f: Face, inside: usize| {
        f.negative_medium = inside;
        f
    };
    system.faces = vec![
        with(square(0.0, false), a),
        with(square(5.0, true), a),
        with(square(5.0, false), b),
        with(square(10.0, true), b),
        detector(20.0),
    ];
    system.build_index();
    let (records, ledger) = system.trace_all([ray(Vec3::new(1.0, 2.0, -5.0), Vec3::Z)]);
    let record = &records[0];
    assert!(matches!(record.end, End::Detected { .. }));
    assert_eq!(
        record.points.len(),
        5,
        "start, z = 0, z = 5 once, z = 10, detector"
    );
    let t = |n1: f64, n2: f64| 1.0 - ((n1 - n2) / (n1 + n2)).powi(2);
    let expected = t(1.0, 1.5) * t(1.5, 1.7) * t(1.7, 1.0);
    assert!(
        (record.ray.power - expected).abs() < 1e-14,
        "{} vs {expected}",
        record.ray.power
    );
    // Optical path: 5 mm of each glass plus the air before and after.
    let path = 5.0 + 1.5 * 5.0 + 1.7 * 5.0 + 10.0;
    assert!((record.ray.optical_path - path).abs() < 1e-9);
    assert!(balanced(&ledger));
}

#[test]
fn splitting_sums_the_multiple_reflections_of_a_plate() {
    // An uncoated plate (n = 1.5) at normal incidence, with every internal reflection
    // followed. Adding the reflections incoherently gives T_total = T²/(1 − R²) and
    // R_total = R + R T²/(1 − R²), with R = 0.04 and T = 0.96 per face (Hecht, Optics,
    // 5th ed., ch. 9, multiple-beam interference, intensities summed).
    let mut system = NonSequentialSystem::new(1.0.into());
    let glass = system.add_medium(1.5.into());
    let disk = Region::Disk { radius: 50.0 };
    let detector_at = |z: f64, id: usize| {
        plane(
            z,
            Region::Rectangle {
                half_width: 100.0,
                half_height: 100.0,
            },
            Role::Detector { detector: id },
            AIR,
            AIR,
        )
    };
    system.faces = vec![
        plane(0.0, disk, REFRACT, AIR, glass),
        plane(5.0, disk, REFRACT, glass, AIR),
        detector_at(20.0, 0),  // transmitted
        detector_at(-20.0, 1), // reflected (the ray starts after it)
    ];
    system.split_cutoff = Some(1e-12);
    system.build_index();
    let (records, ledger) = system.trace_all([ray(Vec3::new(0.0, 0.0, -10.0), Vec3::Z)]);
    let detected = |id: usize| -> f64 {
        records
            .iter()
            .filter(|r| matches!(r.end, End::Detected { detector, .. } if detector == id))
            .map(|r| r.ray.power)
            .sum()
    };
    let (r, t) = (0.04, 0.96);
    let transmitted = t * t / (1.0 - r * r);
    let reflected = r + r * t * t / (1.0 - r * r);
    assert!(
        (detected(0) - transmitted).abs() < 1e-10,
        "{} vs {transmitted}",
        detected(0)
    );
    assert!(
        (detected(1) - reflected).abs() < 1e-10,
        "{} vs {reflected}",
        detected(1)
    );
    assert!(ledger.below_cutoff < 1e-10 && balanced(&ledger));
    assert!(
        records.iter().any(|r| r.generation >= 4),
        "deep reflections were followed"
    );
}

#[test]
fn dichroic_curve_splits_a_beam_at_45_degrees() {
    // A plate whose front face carries a measured curve: at 0.633 µm it reflects 90 % and
    // transmits 10 %; the back face has an ideal anti-reflection coating. A beam at 45°
    // splits accordingly into a reflected and a transmitted detector.
    use optobench_math::Rotation;
    use optobench_trace::coating::CoatingCurve;
    let curve = CoatingCurve::new(vec![(0.6, 0.9, 0.1), (0.7, 0.9, 0.1)]).expect("valid");
    let dichroic = Role::Refract {
        coating: Coating::Curve(Arc::new(curve)),
        clear_radius: None,
    };
    let mut system = NonSequentialSystem::new(1.0.into());
    let glass = system.add_medium(1.5.into());
    let tilt = |z: f64| {
        Transform::new(
            Rotation::from_axis_angle(Vec3::X, core::f64::consts::FRAC_PI_4).expect("axis"),
            Vec3::new(0.0, 0.0, 0.0),
        ) * at(z)
    };
    let disk = Region::Disk { radius: 30.0 };
    let face = |z: f64, role: Role, negative: usize, positive: usize| Face {
        placement: tilt(z),
        ..plane(0.0, disk, role, negative, positive)
    };
    let screen = |position: Vec3, axis: Vec3, id: usize| Face {
        placement: Transform::new(
            optobench_math::Frame::new(position, axis, Vec3::X)
                .expect("frame")
                .transform()
                .rotation,
            position,
        ),
        ..plane(
            0.0,
            Region::Rectangle {
                half_width: 50.0,
                half_height: 50.0,
            },
            Role::Detector { detector: id },
            AIR,
            AIR,
        )
    };
    system.faces = vec![
        face(0.0, dichroic, glass, AIR),
        face(
            -3.0,
            Role::Refract {
                coating: Coating::Ideal,
                clear_radius: None,
            },
            AIR,
            glass,
        ),
        // The plate's normal is (0, −1, 1)/√2: light coming down −z reflects to −y and
        // passes on down −z.
        screen(Vec3::new(0.0, 0.0, -60.0), -Vec3::Z, 0),
        screen(Vec3::new(0.0, -60.0, 0.0), -Vec3::Y, 1),
    ];
    system.split_cutoff = Some(1e-9);
    system.build_index();
    let launched = Ray::new(Vec3::new(0.0, 0.0, 40.0), -Vec3::Z, 0.633, 1.0).expect("ray");
    // Travelling down onto the coated face from its outer side.
    let (records, ledger) = system.trace_all([launched]);
    let power = |id: usize| -> f64 {
        records
            .iter()
            .filter(|r| matches!(r.end, End::Detected { detector, .. } if detector == id))
            .map(|r| r.ray.power)
            .sum()
    };
    assert!((power(1) - 0.9).abs() < 1e-12, "reflected {}", power(1));
    // 10 % enters the glass and leaves through the ideal back face.
    assert!((power(0) - 0.1).abs() < 1e-12, "transmitted {}", power(0));
    assert!(balanced(&ledger));
}

#[test]
fn threads_give_the_same_trace_bit_for_bit() {
    // Rays are independent and booked in their order, so a trace on several threads must
    // equal the trace on one: records and ledger identical, not merely close. A split
    // plate, rays fanned across it and past its edge (some clipped by the detector's
    // edge, some escaping), enough of them for several threads.
    let mut system = NonSequentialSystem::new(1.0.into());
    let glass = system.add_medium(1.5.into());
    let disk = Region::Disk { radius: 20.0 };
    system.faces = vec![
        plane(0.0, disk, REFRACT, AIR, glass),
        plane(5.0, disk, REFRACT, glass, AIR),
        detector(40.0),
    ];
    system.split_cutoff = Some(1e-9);
    system.build_index();
    let rays: Vec<Ray> = (0..2000)
        .map(|k| {
            let t = k as f64 / 2000.0;
            let position = Vec3::new(60.0 * t - 30.0, 7.0 * (13.0 * t).sin(), -10.0);
            let direction = Vec3::new(0.3 * (5.0 * t).cos(), 0.2 * t, 1.0);
            Ray::new(position, direction, 0.55, 1e-3).expect("ray")
        })
        .collect();
    system.threads = 1;
    let one = system.trace_all(rays.clone());
    for threads in [2, 3, 8] {
        system.threads = threads;
        let many = system.trace_all(rays.clone());
        assert!(many.0 == one.0, "records differ on {threads} threads");
        assert!(many.1 == one.1, "ledgers differ on {threads} threads");
    }
    assert!(one.0.len() > rays.len(), "rays were split");
    assert!(balanced(&one.1));
}
