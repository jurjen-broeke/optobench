//! M11b: fluorophores and pulsed lasers read from a layout, and the pulse's dispersion
//! along the chief ray (`optobench_analysis::excitation`).
//!
//! Reference values:
//!
//! - σ₁ = 1000 ln 10 ε / N_A cm² for a molar extinction coefficient ε in L mol⁻¹ cm⁻¹
//!   (Beer–Lambert in base 10; N_A = 6.022 140 76 × 10²³ /mol, exact in the SI).
//! - 1 GM (Göppert-Mayer) = 10⁻⁵⁰ cm⁴ s per photon (C. Xu, W. Zipfel, J. B. Shear,
//!   R. M. Williams and W. W. Webb, Proc. Natl. Acad. Sci. USA 93, 10763 (1996), Fig. 1,
//!   where most spectra are action cross-sections η σ₂: the absorption σ₂ is η σ₂ / η).
//! - The group-delay dispersion of a path is the sum of GVD × length over its media
//!   (Diels and Rudolph, Ultrashort Laser Pulse Phenomena, 2nd ed., 2006, ch. 1), each
//!   GVD from the materials' own dispersion formulas (checked against the formulas
//!   differentiated by hand in `optobench_materials::dispersion`).

use optobench_analysis::excitation::{broadened_width, pulse_path};
use optobench_io::scene_file::parse;
use optobench_io::store::material;
use optobench_materials::Spectrum;
use optobench_materials::dispersion::group_velocity_dispersion;
use optobench_scene::{PulseShape, Scene};

const AVOGADRO: f64 = 6.022_140_76e23;

/// A pulsed laser at 920 nm through a 10 mm fused-silica window into an objective
/// (stated GDD 3000 fs²) dipped in water, its focus 1 mm into a water layer, with
/// `fluorophore` added.
fn layout(fluorophore: &str) -> String {
    format!(
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"laser\"\nkind = \"collimated\"\nat = 0\ndiameter = 2.0\n\
         wavelength_um = 0.92\npower_w = 0.1\n\
         pulse = {{ repetition_rate_mhz = 80, width_fs = 120, shape = \"sech2\", prechirp_fs2 = -5000 }}\n\n\
         [[element]]\nid = \"window\"\nkind = \"lens\"\nat = 20\n\
         surfaces = [ {{ radius = \"inf\" }}, {{ radius = \"inf\" }} ]\nthickness = [10.0]\n\
         material = [\"fused-silica\"]\ndiameter = 25.4\nclear_aperture = 22.0\ncoating = \"ideal\"\n\n\
         [[element]]\nid = \"OBJ\"\nkind = \"objective\"\nat = 100\nfocal_length = 9.0\nna = 0.8\n\
         pupil_distance = 45.0\nworking_distance = 2.0\nimmersion = \"water\"\n\
         group_delay_dispersion_fs2 = 3000\n\n\
         [[element]]\nid = \"S\"\nkind = \"sample\"\nat = 146\nabove = \"water\"\n\
         layers = [ {{ thickness = 5.0, material = \"water\" }} ]\n\n\
         [[detector]]\nid = \"focus\"\nat = 147\nsize = [1, 1]\npixels = [10, 10]\n\n\
         {fluorophore}\n"
    )
}

fn read(text: &str) -> Result<Scene, String> {
    parse(text, &|name: &str| material(None, name)).map_err(|e| e.to_string())
}

const DYE: &str = "[[fluorophore]]\nid = \"dye\"\nsource = \"test values\"\nquantum_yield = 0.9\n\
    lifetime_ns = 4.0\nextinction_coefficient = 56000\n\
    excitation = [[0.45, 0.2], [0.49, 2.0], [0.52, 0.1]]\n\
    two_photon_cross_section_gm = [[0.90, 30.0], [0.94, 20.0]]\n\
    emission = [[0.50, 0.3], [0.52, 1.0], [0.60, 0.1]]\n\
    concentrations = [ { sample = \"S\", layer = 1, micromolar = 10.0 } ]\n";

#[test]
fn a_fluorophore_and_a_pulsed_laser_are_read() {
    let scene = read(&layout(DYE)).expect("read");
    let pulse = scene.sources[0].pulse.expect("pulsed");
    assert_eq!(pulse.shape, PulseShape::SechSquared);
    assert!((pulse.repetition_rate - 80e6).abs() < 1e-6);
    assert!((pulse.width - 120e-15).abs() < 1e-27);
    assert!((pulse.prechirp + 5000e-30).abs() < 1e-40);

    let dye = &scene.fluorophores[0];
    assert_eq!(dye.id, "dye");
    assert_eq!(dye.quantum_yield, 0.9);
    assert!((dye.lifetime.expect("given") - 4e-9).abs() < 1e-21);
    // σ₁ at the excitation peak (relative 2.0 at 490 nm) from ε = 56 000 L/(mol cm),
    // in mm² (×100 from cm²): about 2.14 × 10⁻¹⁴ mm².
    let peak = 1000.0 * core::f64::consts::LN_10 * 56000.0 / AVOGADRO * 100.0;
    let one = dye.one_photon.as_ref().expect("given");
    assert!((one.at(0.49).expect("in range") - peak).abs() < 1e-12 * peak);
    assert!((one.at(0.45).expect("in range") - 0.1 * peak).abs() < 1e-12 * peak);
    assert!((peak - 2.1414e-14).abs() < 1e-17);
    // σ₂ from GM: 30 GM = 3 × 10⁻⁴⁷ mm⁴ s.
    let two = dye.two_photon.as_ref().expect("given");
    assert!((two.at(0.90).expect("in range") - 3e-47).abs() < 1e-58);
    assert_eq!(dye.concentrations.len(), 1);
    assert_eq!(dye.concentrations[0].sample, "S");
    assert_eq!(dye.concentrations[0].layer, 1);
    assert!((dye.concentrations[0].molar - 1e-5).abs() < 1e-17);
    assert!(matches!(dye.emission, Some(Spectrum::Table(_))));

    // An action cross-section η σ₂ = 27 GM with η = 0.9 is σ₂ = 30 GM.
    let action = DYE.replace(
        "two_photon_cross_section_gm = [[0.90, 30.0], [0.94, 20.0]]",
        "two_photon_action_cross_section_gm = [[0.90, 27.0], [0.94, 18.0]]",
    );
    let scene = read(&layout(&action)).expect("read");
    let two = scene.fluorophores[0].two_photon.as_ref().expect("given");
    assert!((two.at(0.90).expect("in range") - 3e-47).abs() < 1e-58);
    assert!((two.at(0.94).expect("in range") - 2e-47).abs() < 1e-58);
}

#[test]
fn the_pulse_widens_along_the_chief_ray() {
    let scene = read(&layout("")).expect("read");
    let path = pulse_path(&scene, 0).expect("pulsed");
    assert_eq!(path.end, "focus");
    let lengths: Vec<f64> = path.steps.iter().map(|s| s.length).collect();
    let expected_lengths = [20.0, 10.0, 70.0, 0.0, 1.0, 1.0];
    assert_eq!(lengths.len(), expected_lengths.len(), "{:?}", path.steps);
    for (found, expected) in lengths.iter().zip(expected_lengths) {
        assert!((found - expected).abs() < 1e-9, "{lengths:?}");
    }
    // The prechirp, 10 mm of fused silica, the objective's stated 3000 fs² and 2 mm of
    // water; the surroundings (index 1.0) add nothing.
    let silica = material(None, "fused-silica").expect("bundled");
    let water = material(None, "water").expect("bundled");
    let gvd = |m: &optobench_materials::Medium| {
        group_velocity_dispersion(m.material(), 0.92).expect("in range")
    };
    let total = -5000e-30 + 10.0 * gvd(&silica) + 3000e-30 + 2.0 * gvd(&water);
    let last = path.steps.last().expect("steps");
    assert!(
        (last.total - total).abs() < 1e-12 * total.abs(),
        "{} {total}",
        last.total
    );
    assert!((last.width - broadened_width(120e-15, total)).abs() < 1e-27);
    assert_eq!(path.steps[3].to, "OBJ");
    assert!((path.steps[3].dispersion - 3000e-30).abs() < 1e-40);
    // Fused silica's GVD at 920 nm is near 30 fs²/mm: about 300 fs² in the window.
    let window = path.steps[1].dispersion * 1e30;
    assert!(window > 250.0 && window < 350.0, "{window}");
    // The prechirp less than cancels the path's dispersion: the pulse leaves the
    // source widened and is narrower at the focus than it would be without the prechirp.
    assert!(path.start > 120e-15);
    assert!(last.width < broadened_width(120e-15, total + 5000e-30));
}

#[test]
fn fluorophores_and_pulses_are_checked() {
    let refused = |dye: &str, words: &str| {
        let message = read(&layout(dye)).expect_err("refused");
        assert!(message.contains(words), "{message}");
    };
    let with = |line: &str| {
        DYE.replace(
            "quantum_yield = 0.9\n",
            &format!("quantum_yield = 0.9\n{line}\n"),
        )
    };
    refused(
        &DYE.replace("source = \"test values\"\n", ""),
        "say where the values come from",
    );
    refused(
        &DYE.replace("quantum_yield = 0.9", "quantum_yield = 1.2"),
        "from 0 to 1",
    );
    refused(&with("one_photon_cross_section_cm2 = 2e-16"), "not both");
    refused(
        &with("two_photon_action_cross_section_gm = 27.0"),
        "not both",
    );
    refused(
        &DYE.replace("quantum_yield = 0.9", "quantum_yield = 0.0")
            .replace(
                "two_photon_cross_section_gm",
                "two_photon_action_cross_section_gm",
            ),
        "needs a quantum yield above 0",
    );
    refused(
        &DYE.replace("sample = \"S\"", "sample = \"X\""),
        "no element `X`",
    );
    refused(
        &DYE.replace("sample = \"S\"", "sample = \"OBJ\""),
        "is not a sample",
    );
    refused(&DYE.replace("layer = 1", "layer = 2"), "from 1 to 1");
    refused(
        &DYE.replace("extinction_coefficient = 56000\n", ""),
        "which is missing",
    );
    let bad_shape = layout("").replace("shape = \"sech2\"", "shape = \"square\"");
    let message = read(&bad_shape).expect_err("refused");
    assert!(
        message.contains("\"sech2\" (default) or \"gaussian\""),
        "{message}"
    );
    // A continuous source has no pulse to follow.
    let continuous = layout("").replace(
        "pulse = { repetition_rate_mhz = 80, width_fs = 120, shape = \"sech2\", prechirp_fs2 = -5000 }\n",
        "",
    );
    let scene = read(&continuous).expect("read");
    assert!(scene.sources[0].pulse.is_none());
    assert!(
        pulse_path(&scene, 0)
            .expect_err("continuous")
            .contains("not pulsed")
    );
}
