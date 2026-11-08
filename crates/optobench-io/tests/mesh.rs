//! M8b tests: the display tessellation of the STEP fixtures' exact faces encloses each
//! solid's volume. With every triangle facing out of the solid, the divergence theorem
//! gives the volume as Σ a·(b × c) / 6 over the triangles; a missing or flipped triangle,
//! or a face cut wrongly at its seam, shows as a volume error. Reference volumes are
//! from geometry (the fixtures' dimensions in tests/step/README.md).

use std::f64::consts::PI;

use optobench_io::step::faces::{EDGE_TOLERANCE, faces};
use optobench_io::step::read;
use optobench_surfaces::brep::Fineness;

/// The volume the tessellated faces of a fixture enclose, and the number of triangles.
fn mesh_volume(name: &str, chord: f64) -> (f64, usize) {
    let path = format!("{}/tests/step/{name}.step", env!("CARGO_MANIFEST_DIR"));
    let model = read(&std::fs::read_to_string(path).expect("fixture")).expect("reads");
    let faces = faces(&model, EDGE_TOLERANCE);
    assert_eq!(faces.skipped, 0);
    // Steps as fine as the chord tolerance asks (the display default stops at 4°).
    let fineness = Fineness {
        chord,
        min_angle: 0.5_f64.to_radians(),
        ..Fineness::default()
    };
    let mut volume = 0.0;
    let mut count = 0;
    for face in &faces.faces {
        let mesh = face.geometry.tessellate(fineness);
        assert!(!mesh.is_empty(), "{name}: a face without triangles");
        for n in &mesh.normals {
            assert!((n.length() - 1.0).abs() < 1e-9, "{name}: normal not unit");
        }
        for t in &mesh.triangles {
            let [a, b, c] = t.map(|i| face.placement.apply_point(mesh.positions[i as usize]));
            volume += a.dot(b.cross(c)) / 6.0;
        }
        count += mesh.triangles.len();
    }
    (volume, count)
}

fn check(name: &str, exact: f64, tolerance: f64) {
    let (volume, triangles) = mesh_volume(name, 0.005);
    let error = (volume - exact) / exact;
    assert!(
        error.abs() < tolerance,
        "{name}: {volume} mm³ from {triangles} triangles, exact {exact} ({:+.3} %)",
        100.0 * error
    );
}

#[test]
fn a_block_is_exact() {
    // Box 10 × 20 × 30: flat faces tessellate without error.
    check("block", 6000.0, 1e-9);
}

#[test]
fn curved_solids_enclose_their_volume() {
    // Inscribed facets fall short of curved surfaces by about the chord tolerance
    // (0.005 mm) over the surface; at these sizes well under 0.5 %.
    check("cylinder", PI * 6.0 * 6.0 * 25.0, 5e-3);
    check("sphere", 4.0 / 3.0 * PI * 8.0_f64.powi(3), 5e-3);
    check("torus", 2.0 * PI * PI * 20.0 * 4.0 * 4.0, 5e-3);
    // Frustum Ø20 to Ø8 over 15: π h (R² + R r + r²) / 3.
    check("cone", PI * 15.0 * (100.0 + 40.0 + 16.0) / 3.0, 5e-3);
    // Plano-convex lens: Ø25.4 cylinder below a sphere R 30 centred 25 below the flat
    // side, so 5 mm thick at the centre; the edge height is where the sphere meets
    // r = 12.7, and above it a spherical cap πh²(3R − h)/3.
    let (radius, r_sphere): (f64, f64) = (12.7, 30.0);
    let edge = -25.0 + (r_sphere * r_sphere - radius * radius).sqrt();
    let cap = 5.0 - edge;
    let lens = PI * radius * radius * edge + PI * cap * cap * (3.0 * r_sphere - cap) / 3.0;
    check("lens", lens, 5e-3);
}

#[test]
fn coarser_tessellation_is_smaller_but_close() {
    // The default display tolerance (0.05 mm) gives fewer triangles, still within 3 %.
    let (fine, fine_count) = mesh_volume("sphere", 0.005);
    let (coarse, coarse_count) = mesh_volume("sphere", Fineness::default().chord);
    assert!(coarse_count < fine_count);
    let exact = 4.0 / 3.0 * PI * 8.0_f64.powi(3);
    assert!(coarse < fine + 1e-9 && fine < exact);
    assert!((coarse - exact).abs() < 0.03 * exact, "{coarse}");
}
