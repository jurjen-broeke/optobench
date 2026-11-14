//! M15b: the tissue Monte Carlo on the GPU against the CPU and the published benchmarks.
//! Skipped (with a message) where no GPU adapter is found, as on most CI runners.
//!
//! References: L. Wang, S. L. Jacques and L. Zheng, Comput. Methods Programs Biomed. 47,
//! 131 (1995), Tables 1 and 2 (van de Hulst's matched slab: diffuse reflectance 0.09739,
//! total transmittance 0.66096; Giovanelli's semi-infinite medium of index 1.5: total
//! reflection 0.2600, the specular 0.04 included).

use optobench_analysis::diffusion::RzGrid;
use optobench_analysis::photon::{Tissue, TissueLayer, Transport, WalkSettings, entering, walk};
use optobench_gpu::Gpu;
use optobench_math::Vec3;

fn slab(thickness: f64, index: f64, anisotropy: f64) -> Tissue {
    Tissue {
        layers: vec![TissueLayer {
            top: 0.0,
            bottom: thickness,
            index,
            absorption: 1.0,
            scattering: 9.0,
            anisotropy,
        }],
        above: 1.0,
        below: 1.0,
    }
}

fn gpu() -> Option<Gpu> {
    let gpu = Gpu::blocking();
    match &gpu {
        Some(g) => eprintln!("GPU: {}", g.name),
        None => eprintln!("no GPU adapter: the GPU checks are skipped"),
    }
    gpu
}

#[test]
fn the_gpu_meets_the_published_slab_benchmarks() {
    let Some(gpu) = gpu() else { return };
    let n = 1_000_000;
    let settings = WalkSettings {
        packets: n,
        seed: 3,
        ..Default::default()
    };
    let sigma = |p: f64| (p * (1.0 - p) / n as f64).sqrt();
    // van de Hulst's matched slab.
    let tissue = slab(0.2, 1.0, 0.75);
    let grid = RzGrid {
        radius: 5.0,
        rings: 2,
        z: (0.0, 0.2),
        slices: 2,
    };
    let (_, launch) = entering(&tissue, Vec3::ZERO, Vec3::Z, 1.0);
    let t = gpu
        .walk_blocking(&tissue, &grid, &[launch], &settings, 0)
        .expect("runs");
    assert!(
        (t.reflected - 0.09739).abs() < 4.0 * sigma(0.09739),
        "R_d {}",
        t.reflected
    );
    assert!(
        (t.transmitted - 0.66096).abs() < 4.0 * sigma(0.66096),
        "T_t {}",
        t.transmitted
    );
    eprintln!(
        "van de Hulst: R_d {:.5}, T_t {:.5}",
        t.reflected, t.transmitted
    );
    // Giovanelli's mismatched semi-infinite medium.
    let tissue = slab(30.0, 1.5, 0.0);
    let grid = RzGrid {
        radius: 5.0,
        rings: 2,
        z: (0.0, 30.0),
        slices: 2,
    };
    let (specular, launch) = entering(&tissue, Vec3::ZERO, Vec3::Z, 1.0);
    let t = gpu
        .walk_blocking(&tissue, &grid, &[launch], &settings, 0)
        .expect("runs");
    let total = specular + t.reflected;
    assert!((total - 0.2600).abs() < 4.0 * sigma(0.26), "R {total}");
    eprintln!("Giovanelli: R {total:.5}");
    // The balance closes to the fixed point's rounding.
    assert!(t.imbalance().abs() < 1e-5, "{}", t.imbalance());
}

/// Per slice, the absorbed power summed over the rings.
fn profile(t: &Transport) -> Vec<f64> {
    let g = &t.grid;
    (0..g.slices)
        .map(|j| (0..g.rings).map(|i| t.absorbed[g.index(i, j)]).sum())
        .collect()
}

#[test]
fn the_gpu_equals_the_cpu_within_statistical_error() {
    let Some(gpu) = gpu() else { return };
    // A forward-scattering, index-mismatched slab (n 1.4, g 0.9): its absorbed depth
    // profile, slice by slice, from 8 independent batches on each side; χ² of the two
    // means against their standard errors.
    let tissue = Tissue {
        layers: vec![
            TissueLayer {
                top: 0.0,
                bottom: 0.5,
                index: 1.4,
                absorption: 0.05,
                scattering: 10.0,
                anisotropy: 0.9,
            },
            TissueLayer {
                top: 0.5,
                bottom: 3.0,
                index: 1.4,
                absorption: 0.2,
                scattering: 20.0,
                anisotropy: 0.8,
            },
        ],
        above: 1.0,
        below: 1.33,
    };
    let grid = RzGrid {
        radius: 4.0,
        rings: 4,
        z: (0.0, 3.0),
        slices: 12,
    };
    let (_, launch) = entering(&tissue, Vec3::ZERO, Vec3::Z, 1.0);
    let batches = 8u64;
    let cpu_settings = WalkSettings {
        packets: 20_000,
        seed: 41,
        ..Default::default()
    };
    let gpu_settings = WalkSettings {
        packets: 200_000,
        seed: 41,
        ..Default::default()
    };
    let run = |on_gpu: bool| -> Vec<Vec<f64>> {
        (0..batches)
            .map(|b| {
                let t = if on_gpu {
                    gpu.walk_blocking(&tissue, &grid, &[launch], &gpu_settings, b)
                        .expect("runs")
                } else {
                    walk(&tissue, &grid, &[launch], &cpu_settings, b)
                };
                let mut row = profile(&t);
                row.push(t.reflected);
                row.push(t.transmitted);
                row
            })
            .collect()
    };
    let started = std::time::Instant::now();
    let cpu = run(false);
    let cpu_time = started.elapsed().as_secs_f64() / cpu_settings.packets as f64;
    let started = std::time::Instant::now();
    let gpu_rows = run(true);
    let gpu_time = started.elapsed().as_secs_f64() / gpu_settings.packets as f64;
    eprintln!(
        "per packet: CPU {:.3} µs, GPU {:.4} µs ({:.0}×)",
        cpu_time * 1e6,
        gpu_time * 1e6,
        cpu_time / gpu_time
    );
    let stats = |rows: &[Vec<f64>], k: usize| {
        let values: Vec<f64> = rows.iter().map(|r| r[k]).collect();
        let m = values.iter().sum::<f64>() / values.len() as f64;
        let v = values.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (values.len() - 1) as f64;
        (m, v / values.len() as f64)
    };
    let quantities = cpu[0].len();
    let mut chi2 = 0.0;
    for k in 0..quantities {
        let (mc, vc) = stats(&cpu, k);
        let (mg, vg) = stats(&gpu_rows, k);
        chi2 += (mc - mg).powi(2) / (vc + vg).max(1e-30);
    }
    // χ² with 14 degrees of freedom: below 36.1 with probability 0.999 (the variances,
    // from 8 batches, are themselves uncertain: a generous bound).
    eprintln!("χ² {chi2:.2} over {quantities} quantities");
    assert!(chi2 < 36.1, "χ² {chi2} over {quantities}");
}
