//! M4b tests: exact faces from the STEP fixtures, hit by rays with known answers, and the
//! residual of every hit below 1e-9 mm. The corpus test (local only) fires rays at the
//! golden assembly.

use optobench_io::step::faces::{EDGE_TOLERANCE, Faces, faces};
use optobench_io::step::read;
use optobench_math::Vec3;
use optobench_surfaces::brep::FaceHit;

const EPS: f64 = 1e-9;

fn load(name: &str) -> Faces {
    let path = format!("{}/tests/step/{name}.step", env!("CARGO_MANIFEST_DIR"));
    let model = read(&std::fs::read_to_string(path).expect("fixture")).expect("reads");
    let faces = faces(&model, EDGE_TOLERANCE);
    assert_eq!(faces.skipped, 0);
    faces
}

/// The nearest hit over all faces, in the root frame, by brute force.
fn nearest(faces: &Faces, origin: Vec3, direction: Vec3) -> Option<FaceHit> {
    let direction = direction.normalized()?;
    faces
        .faces
        .iter()
        .filter_map(|f| {
            let to_local = f.placement.inverse();
            let hit = f.geometry.intersect(
                to_local.apply_point(origin),
                to_local.apply_vector(direction),
                1e-9,
            )?;
            Some(FaceHit {
                point: f.placement.apply_point(hit.point),
                normal: f.placement.apply_vector(hit.normal),
                ..hit
            })
        })
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

#[test]
fn block_faces_are_hit_on_the_right_side_and_trimmed() {
    // Box from the origin to (10, 20, 30).
    let faces = load("block");
    let hit = nearest(&faces, Vec3::new(5.0, 10.0, 50.0), -Vec3::Z).expect("top face");
    assert!((hit.point - Vec3::new(5.0, 10.0, 30.0)).length() < EPS);
    assert!((hit.normal - Vec3::Z).length() < EPS, "outward normal");
    assert!(hit.residual < EPS);
    let side = nearest(&faces, Vec3::new(-5.0, 3.0, 7.0), Vec3::X).expect("x = 0 face");
    assert!((side.point - Vec3::new(0.0, 3.0, 7.0)).length() < EPS);
    assert!((side.normal + Vec3::X).length() < EPS);
    // From inside, the ray leaves through the far face.
    let inside = nearest(&faces, Vec3::new(5.0, 10.0, 15.0), Vec3::Y).expect("y = 20 face");
    assert!((inside.point.y - 20.0).abs() < EPS);
    // Just beside the box: nothing.
    assert!(nearest(&faces, Vec3::new(10.01, 10.0, 50.0), -Vec3::Z).is_none());
    assert!(nearest(&faces, Vec3::new(5.0, -0.01, 50.0), -Vec3::Z).is_none());
}

#[test]
fn lens_surface_is_hit_at_its_exact_sag() {
    // Sphere R 30 centred at z = −25 over a Ø25.4 rim: z(r) = −25 + √(900 − r²).
    let faces = load("lens");
    for r in [0.0, 3.0, 8.0, 12.0, 12.69] {
        let hit = nearest(&faces, Vec3::new(r, 0.0, 50.0), -Vec3::Z).expect("sphere face");
        let z = -25.0 + (900.0 - r * r).sqrt();
        assert!((hit.point.z - z).abs() < EPS, "r = {r}");
        assert!(hit.residual < EPS);
        assert!(hit.normal.z > 0.0, "outward normal");
    }
    // Outside the rim the ray passes the lens.
    assert!(nearest(&faces, Vec3::new(12.75, 0.0, 50.0), -Vec3::Z).is_none());
    // From below it meets the flat face at z = 0.
    let flat = nearest(&faces, Vec3::new(4.0, 4.0, -10.0), Vec3::Z).expect("flat face");
    assert!(flat.point.z.abs() < EPS && (flat.normal + Vec3::Z).length() < EPS);
}

#[test]
fn curved_primitives_are_hit_on_their_exact_surfaces() {
    // Cylinder Ø12 × 25 about z.
    let hit = nearest(&load("cylinder"), Vec3::new(-20.0, 1.0, 10.0), Vec3::X).expect("side");
    assert!((hit.point.x.hypot(hit.point.y) - 6.0).abs() < EPS && hit.normal.x < 0.0);
    // Sphere radius 8.
    let hit = nearest(&load("sphere"), Vec3::new(2.0, -30.0, 3.0), Vec3::Y).expect("sphere");
    assert!((hit.point.length() - 8.0).abs() < EPS && hit.normal.y < 0.0);
    let hit = nearest(&load("sphere"), Vec3::new(2.0, 1.0, -30.0), Vec3::Z).expect("lower half");
    assert!((hit.point.length() - 8.0).abs() < EPS && hit.point.z < 0.0);
    // Torus 20 / 4: a ray along x at height 1 meets the tube at x = −(20 + √15).
    let hit = nearest(&load("torus"), Vec3::new(-40.0, 0.0, 1.0), Vec3::X).expect("torus");
    assert!((hit.point.x + 20.0 + 15f64.sqrt()).abs() < EPS && hit.residual < EPS);
    // Cone Ø20 to Ø8 over 15: at height 7.5 its radius is 7.
    let hit = nearest(&load("cone"), Vec3::new(30.0, 0.0, 7.5), -Vec3::X).expect("cone");
    assert!((hit.point.x - 7.0).abs() < EPS && hit.residual < EPS);
}

#[test]
fn assembly_faces_are_placed() {
    // rod:1's top (z = 100 in its own frame) lands at z = 104 above (15, 15).
    let faces = load("assembly");
    let hit = nearest(&faces, Vec3::new(15.0, 15.0, 200.0), -Vec3::Z).expect("rod:1 top");
    assert!((hit.point.z - 104.0).abs() < EPS);
    // rod:2 lies along −y at height 30 from x = −15: hit from above.
    let hit = nearest(&faces, Vec3::new(-15.0, -50.0, 100.0), -Vec3::Z).expect("rod:2 side");
    assert!((hit.point.z - 33.0).abs() < EPS);
}

#[test]
fn corpus_golden_assembly_hits_are_exact() {
    // M4b exit criterion: every hit on the golden assembly lies within 1e-9 mm of its exact
    // surface, and the BVH finds the same nearest hit as a search over every face.
    use optobench_surfaces::brep::{Aabb, FaceSurface};
    use optobench_trace::bvh::Bvh;
    use std::collections::BTreeMap;

    let path = format!(
        "{}/../../corpus/solidcraft/thorlabs-cage.step",
        env!("CARGO_MANIFEST_DIR")
    );
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("corpus absent: skipped");
        return;
    };
    let model = read(&text).expect("reads");
    let start = std::time::Instant::now();
    let faces = faces(&model, EDGE_TOLERANCE);
    let built = start.elapsed();
    assert_eq!(faces.skipped, 0);
    let boxes: Vec<Aabb> = faces
        .faces
        .iter()
        .map(|f| {
            f.geometry
                .bounds()
                .corners()
                .iter()
                .fold(Aabb::EMPTY, |b, c| b.with(f.placement.apply_point(*c)))
        })
        .collect();
    let bvh = Bvh::build(&boxes);
    let scene = boxes.iter().fold(Aabb::EMPTY, |a, b| a.union(*b));

    // Deterministic rays: from points on a sphere around the assembly towards a random
    // point in the box of a random face, so every kind of surface gets hit.
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let centre = scene.centre();
    let reach = scene.size();
    let local_hit = |i: usize, origin: Vec3, direction: Vec3| {
        let f = &faces.faces[i];
        let to_local = f.placement.inverse();
        f.geometry.intersect(
            to_local.apply_point(origin),
            to_local.apply_vector(direction),
            1e-9,
        )
    };
    let kind = |s: &FaceSurface| {
        let name = format!("{s:?}");
        name.split(['(', ' ', '{']).next().unwrap_or("").to_owned()
    };
    let rays = 20_000;
    let mut hits = 0;
    let mut worst: BTreeMap<String, f64> = BTreeMap::new();
    let trace_start = std::time::Instant::now();
    for ray in 0..rays {
        let (a, b) = (
            random() * core::f64::consts::TAU,
            (random() * 2.0 - 1.0).acos(),
        );
        let origin = centre + Vec3::new(b.sin() * a.cos(), b.sin() * a.sin(), b.cos()) * reach;
        let aim = boxes[((random() * boxes.len() as f64) as usize).min(boxes.len() - 1)];
        let target = Vec3::new(
            aim.min.x + random() * (aim.max.x - aim.min.x),
            aim.min.y + random() * (aim.max.y - aim.min.y),
            aim.min.z + random() * (aim.max.z - aim.min.z),
        );
        let direction = (target - origin).normalized().expect("non-zero");
        let mut best_hit = None;
        let found = bvh.nearest(origin, direction, |i| {
            let hit = local_hit(i, origin, direction)?;
            if best_hit
                .as_ref()
                .is_none_or(|(_, h): &(usize, FaceHit)| hit.distance < h.distance)
            {
                best_hit = Some((i, hit));
            }
            Some(hit.distance)
        });
        if let Some((i, hit)) = best_hit {
            hits += 1;
            let entry = worst
                .entry(kind(&faces.faces[i].geometry.surface))
                .or_default();
            *entry = entry.max(hit.residual);
            assert!(
                hit.residual < 1e-9,
                "ray {ray}: residual {} on {:?}",
                hit.residual,
                kind(&faces.faces[i].geometry.surface)
            );
        }
        if ray < 200 {
            let brute = (0..faces.faces.len())
                .filter_map(|i| local_hit(i, origin, direction).map(|h| h.distance))
                .fold(None, |best: Option<f64>, d| {
                    Some(best.map_or(d, |b| b.min(d)))
                });
            assert_eq!(found.is_some(), brute.is_some(), "ray {ray}");
            if let (Some((_, a)), Some(b)) = (found, brute) {
                assert!((a - b).abs() < 1e-9, "ray {ray}: BVH {a} vs all faces {b}");
            }
        }
    }
    eprintln!(
        "{} faces built in {:?}; {rays} rays, {hits} hits in {:?}; worst residual by surface: {worst:?}",
        faces.faces.len(),
        built,
        trace_start.elapsed()
    );
    assert!(hits > rays / 2, "most rays aimed at faces hit something");
    let kinds: std::collections::BTreeSet<String> = faces
        .faces
        .iter()
        .map(|f| kind(&f.geometry.surface))
        .collect();
    for k in &kinds {
        assert!(worst.contains_key(k), "no hit on any {k} face");
    }
}
