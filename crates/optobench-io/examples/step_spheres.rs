//! Lists the spherical faces of a STEP file: radius, centre and the vertex nearest the
//! centre's axis line (for lenses: where each surface's vertex sits). `cargo run -p
//! optobench-io --example step_spheres -- <file>`
fn main() {
    for path in std::env::args().skip(1) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(model) = optobench_io::step::read(&text) else {
            continue;
        };
        for (p, product) in model.products.iter().enumerate() {
            for (b, body) in product.bodies.iter().enumerate() {
                for face in &body.faces {
                    if let optobench_io::step::Surface::Sphere { frame, radius } = &face.surface {
                        let c = frame.origin();
                        println!(
                            "product {p} body {b} `{}`: R = {radius:.4} mm, centre ({:.4}, {:.4}, {:.4}), same_sense {}",
                            body.name, c.x, c.y, c.z, face.same_sense
                        );
                    }
                }
            }
        }
    }
}
