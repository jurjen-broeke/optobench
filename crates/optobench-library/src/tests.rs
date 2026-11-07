use super::*;
use optobench_math::Frame;

fn frame(origin: Vec3, z: Vec3) -> Frame {
    let x = if z.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    Frame::new(origin, z, x).expect("valid frame")
}

/// A circle of radius `r` about the z axis at height `z`.
fn circle(r: f64, z: f64) -> Vec<Vec3> {
    (0..16)
        .map(|i| {
            let a = core::f64::consts::TAU * i as f64 / 16.0;
            Vec3::new(r * a.cos(), r * a.sin(), z)
        })
        .collect()
}

fn cylinder(r: f64, from: f64, to: f64) -> BrepFace {
    BrepFace::new(
        FaceSurface::Cylinder {
            frame: frame(Vec3::ZERO, Vec3::Z),
            radius: r,
        },
        false,
        vec![circle(r, from), circle(r, to)],
    )
}

fn flat(z: f64, r: f64, outward: f64) -> BrepFace {
    BrepFace::new(
        FaceSurface::Plane(frame(Vec3::new(0.0, 0.0, z), Vec3::Z)),
        outward < 0.0,
        vec![circle(r, z)],
    )
}

/// A spherical cap with its vertex at `vertex` and signed radius `radius` for light
/// along +z (centre at vertex + radius), cut at rim radius `r`.
fn cap(vertex: f64, radius: f64, r: f64) -> BrepFace {
    let centre = vertex + radius;
    let sag = radius.abs() - (radius * radius - r * r).sqrt();
    let rim = vertex + sag * radius.signum();
    BrepFace::new(
        FaceSurface::Sphere {
            frame: frame(Vec3::new(0.0, 0.0, centre), Vec3::Z),
            radius: radius.abs(),
        },
        false,
        vec![circle(r, rim)],
    )
}

fn part(surfaces: &[f64], thickness: &[f64], diameter: f64) -> Part {
    Part {
        vendor: "Example".into(),
        number: "PC-30".into(),
        part_type: PartType::Lens,
        source: "test".into(),
        outline: Outline::Round { diameter },
        clear_aperture: None,
        surfaces: surfaces
            .iter()
            .map(|&radius| PartSurface { radius, conic: 0.0 })
            .collect(),
        thickness: thickness.to_vec(),
        glass: thickness.iter().map(|_| Glass::Index(1.5)).collect(),
        coating: None,
        products: vec!["pc-lens".into()],
    }
}

fn read(faces: &[BrepFace]) -> Profile {
    profile(&faces.iter().collect::<Vec<_>>()).expect("an axis")
}

#[test]
fn plano_convex_lens_is_read_and_matched_either_way_round() {
    // Flat face at z = 0, convex face (R 30) with its vertex at z = 5: light along +z sees
    // a flat then R = −30.
    let lens = [
        cylinder(12.7, 0.0, 2.0),
        flat(0.0, 12.7, -1.0),
        cap(5.0, -30.0, 12.7),
    ];
    let p = read(&lens);
    assert_eq!(p.rim_radius, Some(12.7));
    assert_eq!(p.surfaces.len(), 2);
    assert!((p.surfaces[0].0).abs() < 1e-12 && p.surfaces[0].1.is_infinite());
    assert!((p.surfaces[1].0 - 5.0).abs() < 1e-12 && (p.surfaces[1].1 + 30.0).abs() < 1e-12);
    // The catalogue lists it curved side first (R 30, flat): it fits reversed, exactly.
    let catalogue = part(&[30.0, f64::INFINITY], &[5.0], 25.4);
    let best = closest(&catalogue, &p, None).expect("comparable");
    assert!(best.fits && best.reversed && best.differences.is_empty() && best.score < 1e-9);
    // Listed flat side first, it fits forward.
    let flipped = closest(&catalogue.reversed(), &p, None).expect("comparable");
    assert!(flipped.fits && !flipped.reversed);
}

#[test]
fn differences_above_the_tolerance_are_reported() {
    let lens = [
        cylinder(12.7, 0.0, 2.0),
        flat(0.0, 12.7, -1.0),
        cap(5.0, -30.0, 12.7),
    ];
    let p = read(&lens);
    // A spec sheet with R 30.2 and 5.04 mm: the radius is reported, the thickness is
    // within 0.05 mm and is not.
    let sheet = part(&[30.2, f64::INFINITY], &[5.04], 25.4);
    let c = closest(&sheet, &p, None).expect("comparable");
    assert!(c.fits);
    assert_eq!(c.differences.len(), 1, "{:?}", c.differences);
    assert_eq!(c.differences[0].quantity, "radius 1");
    assert!((c.differences[0].step - 30.0).abs() < 1e-12);
    // A very different part does not fit.
    let other = part(&[50.0, f64::INFINITY], &[3.0], 25.4);
    assert!(!closest(&other, &p, None).expect("comparable").fits);
}

#[test]
fn doublet_bodies_find_their_own_glass() {
    // Cemented doublet along +z: R 40 (vertex 0), R −25 (vertex 8), R −200 (vertex 10.5).
    let crown = [
        cylinder(12.7, 1.0, 6.0),
        cap(0.0, 40.0, 12.7),
        cap(8.0, -25.0, 12.7),
    ];
    let flint = [
        cylinder(12.7, 8.0, 10.0),
        cap(8.0, -25.0, 12.7),
        cap(10.5, -200.0, 12.7),
    ];
    let doublet = part(&[40.0, -25.0, -200.0], &[8.0, 2.5], 25.4);
    let a = closest(&doublet, &read(&crown), None).expect("comparable");
    let b = closest(&doublet, &read(&flint), None).expect("comparable");
    assert!(a.fits && b.fits);
    assert_eq!((a.glass, a.reversed), (0, false));
    assert_eq!((b.glass, b.reversed), (1, false));
}

#[test]
fn a_mount_ring_does_not_fit() {
    // An annulus 2 mm thick with an inner bore: flat faces at both ends, rim 15.
    let ring = [
        cylinder(15.0, 0.0, 2.0),
        cylinder(11.5, 0.0, 2.0),
        flat(0.0, 15.0, -1.0),
        flat(2.0, 15.0, 1.0),
    ];
    let filter = Part {
        part_type: PartType::Filter,
        ..part(&[f64::INFINITY, f64::INFINITY], &[2.0], 25.0)
    };
    let c = closest(&filter, &read(&ring), None).expect("comparable");
    assert!(!c.fits, "a Ø30 ring is not a Ø25 filter");
}

#[test]
fn rectangular_plate_axis_comes_from_its_thinnest_pair_of_faces() {
    // A 25 × 36 × 1.1 mm box: its axis is z.
    let face = |o: Vec3, n: Vec3| BrepFace::new(FaceSurface::Plane(frame(o, n)), false, vec![]);
    let plate = [
        face(Vec3::new(0.0, 0.0, 0.0), -Vec3::Z),
        face(Vec3::new(0.0, 0.0, 1.1), Vec3::Z),
        face(Vec3::new(0.0, 0.0, 0.0), -Vec3::X),
        face(Vec3::new(25.0, 0.0, 0.0), Vec3::X),
        face(Vec3::new(0.0, 0.0, 0.0), -Vec3::Y),
        face(Vec3::new(0.0, 36.0, 0.0), Vec3::Y),
    ];
    let p = read(&plate);
    assert!(p.axis.dot(Vec3::Z).abs() > 1.0 - 1e-12);
    assert_eq!(p.rim_radius, None);
    let dichroic = Part {
        part_type: PartType::Filter,
        outline: Outline::Rectangular {
            width: 25.0,
            height: 36.0,
        },
        ..part(&[f64::INFINITY, f64::INFINITY], &[1.1], 0.0)
    };
    let c = closest(&dichroic, &p, None).expect("comparable");
    assert!(c.fits && c.differences.is_empty());
}

#[test]
fn parts_are_checked_and_keys_are_unique() {
    assert!(part(&[30.0, f64::INFINITY], &[5.0], 25.4).check().is_ok());
    assert!(part(&[30.0, f64::INFINITY], &[], 25.4).check().is_err());
    assert!(part(&[30.0, 0.0], &[5.0], 25.4).check().is_err());
    let mut wide = part(&[30.0, f64::INFINITY], &[5.0], 25.4);
    wide.clear_aperture = Some(30.0);
    assert!(wide.check().is_err());
    let a = part(&[30.0, f64::INFINITY], &[5.0], 25.4);
    assert_eq!(a.key(), "example/PC-30");
    let library = Library::new(vec![a.clone()]).expect("one part");
    assert!(library.get("EXAMPLE/pc-30").is_some());
    assert!(library.for_product("PC-LENS").is_some());
    assert!(library.for_product("pc-30").is_some());
    assert!(library.for_product("other").is_none());
    assert!(Library::new(vec![a.clone(), a.clone()]).is_err());
    let mut alias = a.clone();
    alias.number = "PC-31".into();
    assert!(
        Library::new(vec![a, alias]).is_err(),
        "both name the product pc-lens"
    );
}

#[test]
fn a_flipped_profile_reads_the_same_lens_from_the_other_side() {
    let lens = [
        cylinder(12.7, 0.0, 2.0),
        flat(0.0, 12.7, -1.0),
        cap(5.0, -30.0, 12.7),
    ];
    let p = read(&lens);
    let f = p.flipped();
    assert_eq!(f.axis, -p.axis);
    // Along −z: the curved vertex first (at −5, R +30), then the flat face (at 0).
    assert!((f.surfaces[0].0 + 5.0).abs() < 1e-12 && (f.surfaces[0].1 - 30.0).abs() < 1e-12);
    assert!(f.surfaces[1].0.abs() < 1e-12 && f.surfaces[1].1.is_infinite());
    assert_eq!(f.flipped(), p);
}
