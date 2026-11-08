//! How a sidecar's faces tessellate for the shaded view (M8b): triangles and time per
//! surface type, and the faces with the most triangles. For checking designs locally:
//! `cargo run --release -p optobench-io --example mesh_survey -- <sidecar> [data directory]`
use std::collections::BTreeMap;
use std::time::Instant;

use optobench_surfaces::brep::{FaceSurface, Fineness};
use optobench_trace::nonsequential::Shape;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = std::path::PathBuf::from(args.next().expect("a sidecar"));
    let data = args.next().map(std::path::PathBuf::from);
    let text = std::fs::read_to_string(&path).expect("sidecar");
    let lookup = |name: &str| optobench_io::store::material(data.as_deref(), name);
    let library = optobench_library::Library::default();
    let lookups = optobench_io::scene_file::Lookups {
        materials: &lookup,
        curves: &optobench_io::coatings::no_curves,
        library: &library,
    };
    let read = optobench_io::sidecar::read(&text, &path, &lookups).expect("reads");
    let built = read.scene.build().expect("builds");
    let mut by_kind: BTreeMap<&str, (usize, usize, usize, f64)> = BTreeMap::new();
    let mut largest = Vec::new();
    for (index, face) in built.system.faces.iter().enumerate() {
        let Shape::Brep(brep) = &face.shape else {
            continue;
        };
        let kind = match brep.surface {
            FaceSurface::Plane(_) => "plane",
            FaceSurface::Cylinder { .. } => "cylinder",
            FaceSurface::Cone { .. } => "cone",
            FaceSurface::Sphere { .. } => "sphere",
            FaceSurface::Torus { .. } => "torus",
            FaceSurface::BSpline(_) => "b-spline",
            FaceSurface::Revolution { .. } => "revolution",
            FaceSurface::Extrusion { .. } => "extrusion",
        };
        let start = Instant::now();
        let mesh = brep.tessellate(Fineness::default());
        let seconds = start.elapsed().as_secs_f64();
        let boundary: usize = brep.loops.iter().map(Vec::len).sum();
        let entry = by_kind.entry(kind).or_default();
        entry.0 += 1;
        entry.1 += mesh.triangles.len();
        entry.2 += boundary;
        entry.3 += seconds;
        largest.push((mesh.triangles.len(), boundary, kind, index));
    }
    println!(
        "{:<11} {:>6} {:>10} {:>10} {:>8}",
        "surface", "faces", "triangles", "boundary", "s"
    );
    for (kind, (faces, triangles, boundary, seconds)) in &by_kind {
        println!("{kind:<11} {faces:>6} {triangles:>10} {boundary:>10} {seconds:>8.3}");
    }
    largest.sort_by(|a, b| b.cmp(a));
    for (triangles, boundary, kind, index) in largest.iter().take(12) {
        println!(
            "face {index:>5} {kind:<11} {triangles:>8} triangles, {boundary:>6} boundary points"
        );
    }
}
