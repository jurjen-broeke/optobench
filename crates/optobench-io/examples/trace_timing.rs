//! Where a sidecar trace spends its time: reading the STEP file, converting its faces,
//! building the scene and tracing (M7). For timing designs locally:
//! `cargo run --release -p optobench-io --example trace_timing -- <sidecar> [data directory] [traces]`
//! Traces on one thread and then on every core, and checks the two traces are the same.
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = std::path::PathBuf::from(args.next().expect("a sidecar"));
    let data = args.next().map(std::path::PathBuf::from);
    let repeat: usize = args.next().map_or(5, |n| n.parse().expect("a count"));
    let text = std::fs::read_to_string(&path).expect("sidecar");
    let lookup = |name: &str| optobench_io::store::material(data.as_deref(), name);
    let library = optobench_library::Library::default();
    let lookups = optobench_io::scene_file::Lookups {
        materials: &lookup,
        curves: &optobench_io::coatings::no_curves,
        library: &library,
    };
    let start = Instant::now();
    let mut read = optobench_io::sidecar::read(&text, &path, &lookups).expect("reads");
    let faces: usize = read.scene.solids.iter().map(|s| s.faces.len()).sum();
    println!(
        "read STEP and sidecar  {:>8.3} s ({} solids, {faces} faces)",
        start.elapsed().as_secs_f64(),
        read.scene.solids.len()
    );
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let mut reference = None;
    for threads in [1, cores] {
        read.scene.threads = threads;
        let first = Instant::now();
        let traced = read.scene.trace().expect("traces");
        println!(
            "first trace, {threads:>2} threads {:>8.3} s ({} rays)",
            first.elapsed().as_secs_f64(),
            traced.records.len()
        );
        let again = Instant::now();
        for _ in 0..repeat {
            read.scene.trace().expect("traces");
        }
        println!(
            "trace, {threads:>2} threads, mean of {repeat:<3} {:>8.3} s",
            again.elapsed().as_secs_f64() / repeat.max(1) as f64
        );
        match &reference {
            None => reference = Some(traced.records),
            Some(one) => assert!(*one == traced.records, "the threads changed the trace"),
        }
    }
}
