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

/// One layer from 0 to `thickness` (mm) between ambient media of index 1.
fn slab(
    thickness: f64,
    index: f64,
    absorption: f64,
    scattering: f64,
    anisotropy: f64,
) -> optobench_analysis::photon::Tissue {
    optobench_analysis::photon::Tissue {
        layers: vec![optobench_analysis::photon::TissueLayer {
            top: 0.0,
            bottom: thickness,
            index,
            absorption,
            scattering,
            anisotropy,
        }],
        above: 1.0,
        below: 1.0,
    }
}

/// A unit pencil beam along the normal through `tissue`: (specular, transport).
fn pencil(
    tissue: &optobench_analysis::photon::Tissue,
    packets: usize,
) -> (f64, optobench_analysis::photon::Transport) {
    let (specular, launch) = optobench_analysis::photon::entering(
        tissue,
        optobench_math::Vec3::ZERO,
        optobench_math::Vec3::Z,
        1.0,
    );
    let grid = RzGrid {
        radius: 5.0,
        rings: 2,
        z: (0.0, tissue.layers[0].bottom.min(50.0)),
        slices: 2,
    };
    let settings = WalkSettings {
        packets,
        seed: 17,
        ..Default::default()
    };
    (specular, walk(tissue, &grid, &[launch], &settings, 0))
}

#[test]
fn a_matched_slab_against_van_de_hulst_as_mcml_quotes_it() {
    // L. Wang, S. L. Jacques and L. Zheng, Comput. Methods Programs Biomed. 47, 131 (1995),
    // Table 1 (also S. A. Prahl et al., SPIE IS 5, 102 (1989), Sect. 10): n = 1 (matched),
    // µa = 10 /cm, µs = 90 /cm, g = 0.75, d = 0.02 cm, normal incidence. From van de Hulst's
    // tables: total diffuse reflectance 0.09739, total transmittance (the unscattered
    // e^{−2} = 0.13534 included) 0.66096. MCML found 0.09734 ± 0.00035 and
    // 0.66096 ± 0.00020; Prahl et al. 0.0971 ± 0.0003 and 0.6616 ± 0.0005.
    let n = 400_000;
    let (specular, t) = pencil(&slab(0.2, 1.0, 1.0, 9.0, 0.75), n);
    assert_eq!(specular, 0.0);
    let sigma = |p: f64| (p * (1.0 - p) / n as f64).sqrt();
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
    assert!(t.imbalance().abs() < 1e-9);
}

#[test]
fn a_mismatched_semi_infinite_medium_against_giovanelli() {
    // Wang et al. (1995), Table 2; Prahl et al. (1989), Sect. 10: a semi-infinite medium of
    // relative index 1.5, µa = 10 /cm, µs = 90 /cm, g = 0 (albedo 0.9), normal incidence:
    // total reflection 0.2600 (R. G. Giovanelli, Optica Acta 2, 153 (1955)); MCML
    // 0.25907 ± 0.0017, Prahl et al. 0.2608 ± 0.0008. The total includes the specular
    // reflection at the face, ((n − 1)/(n + 1))² = 0.04: Prahl et al. call it "total
    // reflection", and MCML adds the specular reflectance to its unscattered reflectance
    // (Wang et al., Sect. 4.1). 30 mm stands for semi-infinite (300 mean free paths).
    let n = 200_000;
    let (specular, t) = pencil(&slab(30.0, 1.5, 1.0, 9.0, 0.0), n);
    assert!((specular - 0.04).abs() < 1e-12);
    let sigma = (0.22f64 * 0.78 / n as f64).sqrt();
    let total = specular + t.reflected;
    assert!((total - 0.2600).abs() < 4.0 * sigma, "R {total}");
    assert!(t.transmitted < 1e-9);
    assert!(t.imbalance().abs() < 1e-9);
}

#[test]
fn deep_fluence_decays_at_the_transport_eigenvalue() {
    // Deep in a semi-infinite medium of isotropic scattering and albedo ω, the fluence
    // decays as e^{−k µt z} with k the root of (ω / 2k) ln((1 + k)/(1 − k)) = 1 (the
    // asymptotic solution of the transport equation; K. M. Case and P. F. Zweifel, Linear
    // Transport Theory, 1967; S. Chandrasekhar, Radiative Transfer, 1950), not diffusion's
    // √(3 (1 − ω)). The laterally summed track length per slice, between 4 and 12 mean free
    // paths, against it (to 3 %).
    let omega: f64 = 0.9;
    // The root, by bisection.
    let f = |k: f64| omega / (2.0 * k) * ((1.0 + k) / (1.0 - k)).ln() - 1.0;
    let (mut a, mut b) = (1e-9, 1.0 - 1e-12);
    for _ in 0..200 {
        let m = 0.5 * (a + b);
        // f rises with k, from ω − 1 < 0 near 0 to +∞ near 1.
        if f(m) > 0.0 {
            b = m;
        } else {
            a = m;
        }
    }
    let k = 0.5 * (a + b);
    let diffusion = (3.0 * (1.0 - omega)).sqrt();
    assert!(
        (k - diffusion).abs() > 0.01,
        "the two differ: {k} {diffusion}"
    );
    let tissue = slab(40.0, 1.0, 1.0 - omega, omega, 0.0);
    let (_, launch) = optobench_analysis::photon::entering(
        &tissue,
        optobench_math::Vec3::ZERO,
        optobench_math::Vec3::Z,
        1.0,
    );
    let grid = RzGrid {
        radius: 60.0,
        rings: 1,
        z: (0.0, 16.0),
        slices: 64,
    };
    let settings = WalkSettings {
        packets: 150_000,
        seed: 23,
        ..Default::default()
    };
    let t = walk(&tissue, &grid, &[launch], &settings, 0);
    // Least squares on ln Φ against z over 4 to 12 mm.
    let points: Vec<(f64, f64)> = (0..grid.slices)
        .map(|j| (grid.z_at(j), t.track[grid.index(0, j)]))
        .filter(|(z, v)| (4.0..12.0).contains(z) && *v > 0.0)
        .map(|(z, v)| (z, v.ln()))
        .collect();
    let n = points.len() as f64;
    let (sx, sy) = points
        .iter()
        .fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (sx / n, sy / n);
    let slope = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f64>()
        / points.iter().map(|p| (p.0 - mx).powi(2)).sum::<f64>();
    assert!(
        (-slope / k - 1.0).abs() < 0.03,
        "slope {slope}, k {k}, diffusion {diffusion}"
    );
}
