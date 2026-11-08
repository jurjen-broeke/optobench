//! Tests of the exact B-rep geometry. Parametric surfaces are checked against the
//! analytic surfaces they represent exactly, so each test has an independent reference.

use super::*;

const EPS: f64 = 1e-9;
const S2: f64 = core::f64::consts::FRAC_1_SQRT_2;

fn frame() -> Frame {
    Frame::PARENT
}

/// The rational quadratic full circle in the x–y plane (9 control points on a square,
/// weights 1 and √2/2, knots 0, ¼, ½, ¾, 1 doubled; The NURBS Book, ch. 7).
fn circle(radius: f64, centre: Vec3) -> BSplineCurve {
    let square = [
        (1.0, 0.0),
        (1.0, 1.0),
        (0.0, 1.0),
        (-1.0, 1.0),
        (-1.0, 0.0),
        (-1.0, -1.0),
        (0.0, -1.0),
        (1.0, -1.0),
        (1.0, 0.0),
    ];
    BSplineCurve {
        degree: 2,
        points: square
            .iter()
            .map(|&(x, y)| centre + Vec3::new(x * radius, y * radius, 0.0))
            .collect(),
        weights: Some(vec![1.0, S2, 1.0, S2, 1.0, S2, 1.0, S2, 1.0]),
        knots: vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ],
    }
}

#[test]
fn basis_functions_sum_to_one_and_derivatives_match_differences() {
    let knots = [0.0, 0.0, 0.0, 0.3, 0.5, 0.5, 0.8, 1.0, 1.0, 1.0];
    let (degree, count) = (2, 7);
    for t in [0.0, 0.1, 0.31, 0.5, 0.77, 0.999] {
        let k = span(&knots, degree, count, t);
        let (n, dn) = basis(&knots, degree, k, t);
        assert!(
            (n.iter().take(degree + 1).sum::<f64>() - 1.0).abs() < 1e-14,
            "t = {t}"
        );
        let h = 1e-7;
        let (n_plus, _) = basis(&knots, degree, k, t + h);
        let (n_minus, _) = basis(&knots, degree, k, (t - h).max(0.0));
        for r in 0..=degree {
            let numeric = (n_plus[r] - n_minus[r]) / (t + h - (t - h).max(0.0));
            assert!((numeric - dn[r]).abs() < 1e-5, "t = {t}, r = {r}");
        }
    }
}

#[test]
fn rational_circle_lies_on_its_circle() {
    let c = circle(12.7, Vec3::new(1.0, 2.0, 3.0));
    for k in 0..=40 {
        let (p, d) = c.evaluate(k as f64 / 40.0);
        let r = (p - Vec3::new(1.0, 2.0, 3.0)).length();
        assert!((r - 12.7).abs() < 1e-12);
        // The tangent is perpendicular to the radius.
        assert!(d.dot(p - Vec3::new(1.0, 2.0, 3.0)).abs() < 1e-9 * d.length());
    }
}

#[test]
fn planar_face_with_a_hole_is_trimmed() {
    // A 20 × 20 square with a 4 × 4 square hole, in z = 0, normal +z: the outer loop runs
    // counter-clockwise and the hole clockwise, so the face is on their left.
    let outer = vec![
        Vec3::new(-10.0, -10.0, 0.0),
        Vec3::new(10.0, -10.0, 0.0),
        Vec3::new(10.0, 10.0, 0.0),
        Vec3::new(-10.0, 10.0, 0.0),
    ];
    let hole = vec![
        Vec3::new(-2.0, -2.0, 0.0),
        Vec3::new(-2.0, 2.0, 0.0),
        Vec3::new(2.0, 2.0, 0.0),
        Vec3::new(2.0, -2.0, 0.0),
    ];
    let face = BrepFace::new(FaceSurface::Plane(frame()), false, vec![outer, hole]);
    let down = -Vec3::Z;
    let hit = |x: f64, y: f64| face.intersect(Vec3::new(x, y, 5.0), down, 0.0);
    let inside = hit(5.0, 5.0).expect("on the face");
    assert!((inside.distance - 5.0).abs() < EPS && inside.residual < EPS);
    assert!((inside.normal - Vec3::Z).length() < EPS);
    assert!(hit(0.0, 0.0).is_none(), "in the hole");
    assert!(hit(11.0, 0.0).is_none(), "outside");
    assert!(hit(9.99, -9.99).is_some(), "near a corner, inside");
    assert!(hit(10.01, 10.01).is_none(), "near a corner, outside");
}

#[test]
fn hemisphere_cap_includes_its_pole_and_excludes_the_other_half() {
    // The upper half of a sphere of radius 5, bounded by its equator run counter-clockwise
    // seen from above (the outward normal at the pole points up).
    let equator: Vec<Vec3> = (0..360)
        .map(|k| {
            let a = TAU * k as f64 / 360.0;
            Vec3::new(5.0 * a.cos(), 5.0 * a.sin(), 0.0)
        })
        .collect();
    let face = BrepFace::new(
        FaceSurface::Sphere {
            frame: frame(),
            radius: 5.0,
        },
        false,
        vec![equator],
    );
    let pole = face
        .intersect(Vec3::new(0.0, 0.0, 20.0), -Vec3::Z, 0.0)
        .expect("hits the pole");
    assert!((pole.point - Vec3::new(0.0, 0.0, 5.0)).length() < EPS);
    // A ray from below meets the lower half first, which is not part of the face, then
    // the upper half from inside.
    let through = face
        .intersect(Vec3::new(1.0, 0.0, -20.0), Vec3::Z, 0.0)
        .expect("hits the cap");
    assert!(through.point.z > 0.0 && through.residual < EPS);
    // A ray along y at z = −1 meets only the lower half.
    assert!(
        face.intersect(Vec3::new(0.0, -20.0, -1.0), Vec3::Y, 0.0)
            .is_none()
    );
}

#[test]
fn rational_bspline_cylinder_matches_the_analytic_cylinder() {
    // A quarter cylinder of radius 10 about z, height 20: rational quadratic in u (the
    // arc from +x to +y), linear in v (z from 0 to 20).
    let arc = [(10.0, 0.0, 1.0), (10.0, 10.0, S2), (0.0, 10.0, 1.0)];
    let surface = BSplineSurface {
        u_degree: 2,
        v_degree: 1,
        points: arc
            .iter()
            .map(|&(x, y, _)| vec![Vec3::new(x, y, 0.0), Vec3::new(x, y, 20.0)])
            .collect(),
        weights: Some(arc.iter().map(|&(_, _, w)| vec![w, w]).collect()),
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
    };
    let face = BrepFace::new(FaceSurface::BSpline(surface), false, vec![]);
    let analytic = BrepFace::new(
        FaceSurface::Cylinder {
            frame: frame(),
            radius: 10.0,
        },
        false,
        vec![],
    );
    for (origin, direction) in [
        (Vec3::new(0.0, 0.0, 5.0), Vec3::new(1.0, 1.0, 0.0)),
        (Vec3::new(2.0, 1.0, 1.0), Vec3::new(1.0, 0.3, 0.4)),
        (Vec3::new(20.0, 20.0, 15.0), Vec3::new(-1.0, -0.7, -0.1)),
    ] {
        let direction = direction.normalized().expect("non-zero");
        let a = face
            .intersect(origin, direction, 0.0)
            .expect("hits the B-spline");
        let b = analytic
            .intersect(origin, direction, 0.0)
            .expect("hits the cylinder");
        assert!(a.residual < EPS, "residual {}", a.residual);
        // The exact cylinder agrees to 1e-9 mm.
        assert!((a.point.x.hypot(a.point.y) - 10.0).abs() < EPS);
        assert!((a.point - b.point).length() < EPS);
        assert!((a.normal.dot(b.normal).abs() - 1.0).abs() < 1e-9);
    }
}

#[test]
fn revolved_circle_matches_the_analytic_torus() {
    // A circle of radius 4 centred 20 from the axis, in the x–z plane, turned about z.
    let profile = BSplineCurve {
        points: circle(4.0, Vec3::ZERO)
            .points
            .iter()
            .map(|p| Vec3::new(20.0 + p.x, 0.0, p.y))
            .collect(),
        ..circle(4.0, Vec3::ZERO)
    };
    let revolved = BrepFace::new(
        FaceSurface::Revolution {
            profile: Curve::BSpline(profile),
            axis_point: Vec3::ZERO,
            axis_direction: Vec3::Z,
        },
        false,
        vec![],
    );
    let torus = BrepFace::new(
        FaceSurface::Torus {
            frame: frame(),
            major: 20.0,
            minor: 4.0,
        },
        false,
        vec![],
    );
    for (origin, direction) in [
        (Vec3::new(-40.0, 1.0, 0.5), Vec3::X),
        (Vec3::new(18.0, 3.0, 30.0), Vec3::new(0.1, 0.0, -1.0)),
        (Vec3::new(0.0, -50.0, 2.0), Vec3::new(0.3, 1.0, 0.0)),
    ] {
        let direction = direction.normalized().expect("non-zero");
        let a = revolved
            .intersect(origin, direction, 0.0)
            .expect("hits the revolution");
        let b = torus
            .intersect(origin, direction, 0.0)
            .expect("hits the torus");
        assert!(a.residual < EPS && b.residual < EPS);
        assert!(
            (a.point - b.point).length() < EPS,
            "{:?} vs {:?}",
            a.point,
            b.point
        );
        assert!(torus.surface.analytic_residual(a.point) < EPS);
    }
}

#[test]
fn extruded_circle_matches_the_analytic_cylinder() {
    let extruded = BrepFace::new(
        FaceSurface::Extrusion {
            profile: Curve::Circle {
                frame: frame(),
                radius: 6.0,
            },
            direction: Vec3::new(0.0, 0.0, 25.0),
        },
        false,
        // Bottom and top rims; the outward normal is radial, so the face lies to the left of
        // the bottom rim run counter-clockwise seen from above and the top rim clockwise.
        [0.0, 25.0]
            .iter()
            .map(|&z| {
                let ring: Vec<Vec3> = (0..64)
                    .map(|k| {
                        let a = TAU * k as f64 / 64.0;
                        Vec3::new(6.0 * a.cos(), 6.0 * a.sin(), z)
                    })
                    .collect();
                if z == 0.0 {
                    ring
                } else {
                    ring.into_iter().rev().collect()
                }
            })
            .collect(),
    );
    let origin = Vec3::new(-20.0, 0.5, 10.0);
    let hit = extruded.surface_hits(origin, Vec3::X);
    assert_eq!(hit.len(), 2);
    let first = extruded
        .intersect(origin, Vec3::X, 0.0)
        .expect("hits the outside");
    assert!(first.normal.x < 0.0, "outward normal faces the ray");
    for h in &hit {
        assert!(h.residual < EPS);
        assert!((h.point.x.hypot(h.point.y) - 6.0).abs() < EPS);
    }
}

#[test]
fn sampled_circle_edge_stays_within_its_chord_tolerance() {
    let curve = Curve::Circle {
        frame: frame(),
        radius: 12.7,
    };
    let start = Vec3::new(12.7, 0.0, 0.0);
    let points = curve.sample(start, start, true, 1e-4);
    assert!(points.len() > 100);
    for pair in points.windows(2) {
        let mid = (pair[0] + pair[1]) * 0.5;
        assert!(12.7 - mid.length() <= 1e-4 + 1e-12);
    }
    // Backwards from +x to +y goes the long way round, through −y.
    let quarter = Vec3::new(0.0, 12.7, 0.0);
    let back = curve.sample(start, quarter, false, 1e-3);
    assert!(back.iter().any(|p| p.y < -12.0));
}

#[test]
fn far_hits_on_an_unbounded_surface_are_not_on_the_face() {
    // A band of a cylinder (radius 5) from z = 0 to z = 10; a ray through the cylinder at
    // z = 50 meets the surface but not the face.
    let ring = |z: f64, forward: bool| -> Vec<Vec3> {
        let points: Vec<Vec3> = (0..90)
            .map(|k| {
                let a = TAU * k as f64 / 90.0;
                Vec3::new(5.0 * a.cos(), 5.0 * a.sin(), z)
            })
            .collect();
        if forward {
            points
        } else {
            points.into_iter().rev().collect()
        }
    };
    let band = BrepFace::new(
        FaceSurface::Cylinder {
            frame: frame(),
            radius: 5.0,
        },
        false,
        vec![ring(0.0, true), ring(10.0, false)],
    );
    assert!(
        band.intersect(Vec3::new(-20.0, 0.0, 5.0), Vec3::X, 0.0)
            .is_some()
    );
    assert!(
        band.intersect(Vec3::new(-20.0, 0.0, 50.0), Vec3::X, 0.0)
            .is_none()
    );
    assert!(
        band.intersect(Vec3::new(-20.0, 0.0, -0.01), Vec3::X, 0.0)
            .is_none()
    );
}

/// The inside test as it was before the segment tree and the plane grid (M7): the nearest
/// boundary segment by a scan in loop order, and its side.
fn contains_by_scan(face: &BrepFace, point: Vec3, normal: Vec3) -> bool {
    match face.nearest_by_scan(point) {
        None => false,
        Some((l, k, s, distance)) => face.lean(l, k, s, point, normal) > 0.0 || distance < 1e-20,
    }
}

#[test]
fn fast_inside_tests_agree_with_the_scan() {
    // A flat face (a finely sampled wavy outer loop, a square hole and a round hole, in a
    // tilted plane) and a curved one (a cylinder band): random points on each, and points
    // on and next to their edges.
    let mut seed = 99u64;
    let mut random = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let plane = Frame::new(
        Vec3::new(3.0, -2.0, 7.0),
        Vec3::new(0.2, -0.3, 1.0),
        Vec3::X,
    )
    .expect("frame");
    let on_plane = |x: f64, y: f64| plane.to_parent_point(Vec3::new(x, y, 0.0));
    let outer: Vec<Vec3> = (0..1500)
        .map(|i| {
            let a = core::f64::consts::TAU * i as f64 / 1500.0;
            let r = 20.0 + (9.0 * a).sin();
            on_plane(r * a.cos(), r * a.sin())
        })
        .collect();
    // Holes run clockwise (the face on their left).
    let square = vec![
        on_plane(-5.0, -5.0),
        on_plane(-5.0, 1.0),
        on_plane(1.0, 1.0),
        on_plane(1.0, -5.0),
    ];
    let round: Vec<Vec3> = (0..200)
        .map(|i| {
            let a = -core::f64::consts::TAU * i as f64 / 200.0;
            on_plane(8.0 + 3.0 * a.cos(), 6.0 + 3.0 * a.sin())
        })
        .collect();
    let flat = BrepFace::new(FaceSurface::Plane(plane), false, vec![outer, square, round]);
    assert!(flat.plane_grid.is_some());
    let normal = plane.axes()[2];
    let mut points: Vec<Vec3> = (0..6000)
        .map(|_| on_plane(50.0 * random() - 25.0, 50.0 * random() - 25.0))
        .collect();
    for l in &flat.loops {
        for (k, &p) in l.iter().enumerate().step_by(7) {
            let q = l[(k + 1) % l.len()];
            points.push(p);
            points.push(p + (q - p) * 0.5);
            points.push(p + (q - p) * 0.5 + plane.axes()[0] * 2e-6);
            points.push(p + (q - p) * 0.5 - plane.axes()[1] * 3e-7);
        }
    }
    for p in &points {
        assert_eq!(
            flat.contains(*p, normal),
            contains_by_scan(&flat, *p, normal),
            "flat at {p:?}"
        );
    }
    // A cylinder band (r 5, z 0 to 10) cut by a wavy top edge: the segment tree path.
    let cylinder = FaceSurface::Cylinder {
        frame: Frame::PARENT,
        radius: 5.0,
    };
    let bottom: Vec<Vec3> = (0..400)
        .map(|i| {
            let a = core::f64::consts::TAU * i as f64 / 400.0;
            Vec3::new(5.0 * a.cos(), 5.0 * a.sin(), 0.0)
        })
        .collect();
    let top: Vec<Vec3> = (0..400)
        .map(|i| {
            let a = -core::f64::consts::TAU * i as f64 / 400.0;
            Vec3::new(5.0 * a.cos(), 5.0 * a.sin(), 8.0 + (5.0 * a).sin())
        })
        .collect();
    let band = BrepFace::new(cylinder, false, vec![bottom, top]);
    assert!(band.plane_grid.is_none());
    for _ in 0..5000 {
        let (a, z) = (core::f64::consts::TAU * random(), 12.0 * random() - 1.0);
        let p = Vec3::new(5.0 * a.cos(), 5.0 * a.sin(), z);
        let n = Vec3::new(a.cos(), a.sin(), 0.0);
        assert_eq!(
            band.contains(p, n),
            contains_by_scan(&band, p, n),
            "band at {p:?}"
        );
    }
}
