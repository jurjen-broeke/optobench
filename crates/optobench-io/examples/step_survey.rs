//! Reads STEP files and prints what the reader found: products, bodies, faces by surface
//! type, unsupported entities and timing. `cargo run -p optobench-io --release --example
//! step_survey -- <files…>`
use std::collections::BTreeMap;

fn main() {
    for path in std::env::args().skip(1) {
        let start = std::time::Instant::now();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                println!("{path}: {e}");
                continue;
            }
        };
        match optobench_io::step::read(&text) {
            Err(e) => println!("{path}: ERROR {e}"),
            Ok(model) => {
                let mut surfaces: BTreeMap<String, usize> = BTreeMap::new();
                let mut faces = 0;
                let mut bodies = 0;
                for p in &model.products {
                    for b in &p.bodies {
                        bodies += 1;
                        for f in &b.faces {
                            faces += 1;
                            let kind = format!("{:?}", f.surface);
                            let kind = kind.split(['(', ' ', '{']).next().unwrap_or("").to_owned();
                            *surfaces.entry(kind).or_default() += 1;
                        }
                    }
                }
                let name = path.rsplit('/').next().unwrap_or(&path);
                println!(
                    "{name}: {} products, {} roots, {bodies} bodies, {faces} faces, {} instances, {:?}, unsupported {:?}, {} ms",
                    model.products.len(),
                    model.roots.len(),
                    model.instances().len(),
                    surfaces,
                    model.unsupported,
                    start.elapsed().as_millis()
                );
            }
        }
    }
}
