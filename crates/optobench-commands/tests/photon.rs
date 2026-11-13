//! M15a: the tissue Monte Carlo launched from a traced design
//! (`optobench_analysis::photon`).
//!
//! Reference values: an absorbing, non-scattering slab of index n in air, µa d = τ, lit
//! along its normal: with r = ((n − 1)/(n + 1))² and a = e^{−τ}, what enters is (1 − r),
//! and of that T = (1 − r) a / (1 − r² a²) leaves through the far face (the reflections
//! inside summed in power).

use optobench_analysis::diffusion::RzGrid;
use optobench_analysis::photon::{WalkSettings, sample_launches, sample_tissue, walk};
use optobench_io::scene_file::parse;
use optobench_io::store::material;

#[test]
fn packets_launched_from_the_trace_cross_an_absorbing_slab_as_the_formula_says() {
    let text = "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
        [[source]]\nid = \"beam\"\nkind = \"collimated\"\ndiameter = 2.0\nrings = 3\nat = 0\n\
        wavelength_um = 0.8\npower_w = 1.0\n\n\
        [[element]]\nid = \"S\"\nkind = \"sample\"\nat = 10\ndiameter = 20\n\
        layers = [ { thickness = 1.0, material = 1.5, absorption = 0.5, source = \"test value\" } ]\n";
    let scene = parse(text, &|n: &str| material(None, n)).expect("reads");
    let built = scene.build().expect("builds");
    let trace = scene.trace_built(&built);
    let (launches, wavelength) = sample_launches(&scene, &built, &trace, 0).expect("launches");
    assert_eq!(wavelength, 0.8);
    // Every ray of the beam enters, along the axis, with (1 − r) of its power.
    let r = (0.5f64 / 2.5).powi(2);
    let entered: f64 = launches.iter().map(|l| l.power).sum();
    assert!((entered - (1.0 - r)).abs() < 1e-12, "{entered}");
    assert!(
        launches
            .iter()
            .all(|l| (l.direction.z - 1.0).abs() < 1e-12 && l.position.z == 0.0)
    );
    let tissue = sample_tissue(&scene, 0, wavelength).expect("tissue");
    assert_eq!((tissue.above, tissue.below), (1.0, 1.0));
    let grid = RzGrid {
        radius: 10.0,
        rings: 4,
        z: (0.0, 1.0),
        slices: 4,
    };
    let settings = WalkSettings {
        packets: 20_000,
        seed: 1,
        ..Default::default()
    };
    let t = walk(&tissue, &grid, &launches, &settings, 0);
    let a = (-0.5f64).exp();
    let transmitted = (1.0 - r).powi(2) * a / (1.0 - r * r * a * a);
    let n = (launches.len() * settings.packets) as f64;
    let sigma = (transmitted * (1.0 - transmitted) / n).sqrt();
    assert!(
        (t.transmitted - transmitted).abs() < 4.0 * sigma,
        "{} {transmitted}",
        t.transmitted
    );
    assert!((t.launched - entered).abs() < 1e-9); // summed packet by packet
    assert!(t.imbalance().abs() < 1e-10);
}
