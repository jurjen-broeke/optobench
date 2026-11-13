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
//! - Two-photon excitation in a thick sample (C. Xu and W. W. Webb, J. Opt. Soc. Am. B 13,
//!   481 (1996)): ⟨F⟩ = ½ g φ η₂ C δ ⟨I₀⟩² ∫ S² dV (Eq. (5)). For a Gaussian beam
//!   ∫ I² dV = π n P² / λ, independent of the waist (their note 15, and from the
//!   Gaussian–Lorentzian beam: ∫ I² dA = P² / (π w²), w² = w₀² (1 + z² / z_R²),
//!   z_R = π w₀² n / λ). For a uniformly filled pupil (paraxial Airy focus)
//!   ∫ S² dV ≈ 8 n λ³ / (π³ NA⁴) with I₀ = π NA² P / λ² (relation (10), a numerical
//!   result to 3 %, their note 14; Eq. (9)), so ∫ I² dV ≈ 8 n P² / (π λ).
//!   g = g_p / (f τ) for pulses (Eq. (13)).
//! - Through any plane across the focus ∫ I dA is the power there (energy conservation),
//!   so one-photon excitation per depth is σ₁ C P / (h ν).
//! - The group-delay dispersion of a path is the sum of GVD × length over its media
//!   (Diels and Rudolph, Ultrashort Laser Pulse Phenomena, 2nd ed., 2006, ch. 1), each
//!   GVD from the materials' own dispersion formulas (checked against the formulas
//!   differentiated by hand in `optobench_materials::dispersion`).

use optobench_analysis::excitation::{
    ExcitationSettings, Timing, broadened_width, excitation, photon_energy, pulse_factor,
    pulse_path,
};
use optobench_analysis::{PsfSettings, psf};
use optobench_commands::Registry;
use optobench_io::scene_file::parse;
use optobench_io::store::material;
use optobench_materials::Spectrum;
use optobench_materials::dispersion::group_velocity_dispersion;
use optobench_scene::{PulseShape, Scene};
use serde_json::{Value, json};

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

/// A source at 0.8 µm into an ideal objective (f 9 mm, NA `na`, immersion n = 1.33)
/// focused 1 mm deep into a matched layer, the detector at the focus.
fn focus_layout(source: &str, na: f64, fluorophore: &str) -> String {
    format!(
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"laser\"\nat = -10\nwavelength_um = 0.8\npower_w = 1.0\n{source}\n\n\
         [[element]]\nid = \"OBJ\"\nkind = \"objective\"\nat = 0\nfocal_length = 9.0\nna = {na}\n\
         pupil_distance = 45.0\nworking_distance = 2.0\nimmersion = 1.33\n\n\
         [[element]]\nid = \"S\"\nkind = \"sample\"\nat = 46\nabove = 1.33\n\
         layers = [ {{ thickness = 5.0, material = 1.33 }} ]\n\n\
         [[detector]]\nid = \"focus\"\nat = 47\nsize = [1, 1]\npixels = [10, 10]\n\n{fluorophore}"
    )
}

const LAMBDA_MM: f64 = 0.8e-3;
const INDEX: f64 = 1.33;

/// The Gaussian beam's focus: 1/e² diameter 3 mm in a 9 mm objective (n sin θ ≈ 0.17 at
/// 1/e²), paraxial; 84 rays across reach far enough for the volume integral.
const GAUSSIAN: &str = "kind = \"gaussian\"\nwaist_diameter = 3.0";
/// A uniform beam slightly overfilling an NA 0.5 pupil (9 mm across).
const UNIFORM: &str = "kind = \"collimated\"\ndiameter = 9.4";

fn volume(
    source: &str,
    na: f64,
    samples: usize,
    planes: usize,
    across: usize,
) -> optobench_analysis::Psf {
    let scene = read(&focus_layout(source, na, "")).expect("read");
    psf(
        &scene,
        0,
        0,
        &PsfSettings {
            samples,
            volume: Some((planes, across)),
            ..Default::default()
        },
    )
    .expect("a focus")
}

#[test]
fn the_squared_focus_of_a_gaussian_beam_is_xu_and_webbs() {
    let psf = volume(GAUSSIAN, 0.8, 84, 16, 24);
    let v = psf.volume.as_ref().expect("volume");
    let p = psf.power;
    // Note 15: π n P² / λ, independent of the waist.
    let expected = core::f64::consts::PI * INDEX * p * p / LAMBDA_MM;
    assert!(
        (v.squared / expected - 1.0).abs() < 0.01,
        "{} {expected}",
        v.squared
    );
    // The tails beyond the planes are a fair part, found in closed form.
    assert!(v.tails > 0.05 * v.squared && v.tails < 0.4 * v.squared);
    // Every plane carries the power (no loss in the matched layer).
    for plane in &v.planes {
        assert!((plane.1 / p - 1.0).abs() < 0.01, "{plane:?}");
    }
    // I² is narrower than I.
    let (wx, wz) = (psf.fwhm.0.expect("x"), psf.fwhm.2.expect("z"));
    let (sx, sz) = (
        psf.fwhm_squared.0.expect("x"),
        psf.fwhm_squared.2.expect("z"),
    );
    assert!(sx < wx && sz < wz);
}

#[test]
fn the_squared_focus_of_a_filled_pupil_is_xu_and_webbs() {
    let focus = volume(UNIFORM, 0.5, 64, 24, 28);
    let v = focus.volume.as_ref().expect("volume");
    let p = focus.power;
    // Relations (9) and (10): 8 n P² / (π λ), to their stated 3 %; NA 0.5 in n = 1.33
    // (sin θ = 0.38) is within their paraxial range.
    let expected = 8.0 * INDEX * p * p / (core::f64::consts::PI * LAMBDA_MM);
    assert!(
        (v.squared / expected - 1.0).abs() < 0.03,
        "{} {expected}",
        v.squared
    );
    let middle = v.planes[v.planes.len() / 2];
    assert!((middle.1 / p - 1.0).abs() < 0.01, "{middle:?}");
    // Too few rays for the depth are refused, not integrated wrongly.
    let scene = read(&focus_layout(GAUSSIAN, 0.8, "")).expect("read");
    let error = psf(
        &scene,
        0,
        0,
        &PsfSettings {
            samples: 40,
            volume: Some((8, 8)),
            ..Default::default()
        },
    )
    .expect_err("too few rays");
    assert!(error.contains("raise `samples`"), "{error}");
}

/// A dye with σ₁ and σ₂ at 0.8 µm, in the focus layer.
const FOCUS_DYE: &str = "[[fluorophore]]\nid = \"dye\"\nsource = \"test values\"\n\
    quantum_yield = 0.8\nlifetime_ns = 4.0\none_photon_cross_section_cm2 = 1e-17\n\
    two_photon_cross_section_gm = 40.0\n\
    concentrations = [ { sample = \"S\", layer = 1, micromolar = 10.0 } ]\n";

#[test]
fn excitation_at_a_pulsed_gaussian_focus_follows_the_closed_forms() {
    let pulse = "pulse = { repetition_rate_mhz = 80, width_fs = 100, shape = \"sech2\" }";
    let scene = read(&focus_layout(
        &format!("{GAUSSIAN}\n{pulse}"),
        0.8,
        FOCUS_DYE,
    ))
    .expect("read");
    let settings = ExcitationSettings {
        samples: 84,
        volume: (16, 24),
        ..Default::default()
    };
    let e = excitation(&scene, 0, 0, 0, &settings).expect("excitation");
    assert_eq!(e.place, Some(("S".into(), "layer 1".into())));
    assert!((e.molar.expect("there") - 1e-5).abs() < 1e-18);
    assert!((e.transmission - 1.0).abs() < 1e-12);
    // Constant indices: no dispersion, the pulse arrives as it left.
    let Timing::Pulsed { width, .. } = e.timing else {
        panic!("pulsed")
    };
    assert!((width - 100e-15).abs() < 1e-27);
    let energy = photon_energy(0.8);
    let molecules = 1e-5 * 6.022_140_76e23 * 1e-6; // per mm³
    let g = pulse_factor(optobench_scene::PulseShape::SechSquared) / (80e6 * 100e-15);
    // One photon: σ₁ I / (h ν) at the peak; per depth σ₁ C P / (h ν).
    let one = e.one_photon.as_ref().expect("σ₁");
    let sigma1 = 1e-17 * 100.0; // mm²
    assert!((one.peak_rate - sigma1 * e.peak_irradiance / energy).abs() < 1e-12 * one.peak_rate);
    assert!(
        (one.saturation.expect("lifetime") - one.peak_rate * 4e-9).abs() < 1e-12 * one.peak_rate
    );
    let per_depth = one.per_depth.expect("concentration");
    assert!((per_depth - sigma1 * molecules * e.pupil_power / energy).abs() < 1e-9 * per_depth);
    // Two photons: the total against Xu and Webb's Gaussian closed form with g.
    let two = e.two_photon.as_ref().expect("σ₂");
    let sigma2 = 40.0 * 1e-48;
    let p = e.pupil_power;
    let closed = 0.5 * sigma2 * molecules * g * core::f64::consts::PI * INDEX * p * p
        / (LAMBDA_MM * energy * energy);
    let total = two.total.expect("concentration");
    assert!((total / closed - 1.0).abs() < 0.01, "{total} {closed}");
    assert!((two.per_pulse.expect("pulsed") - two.peak_rate / 80e6).abs() < 1e-12 * two.peak_rate);
    assert!((e.emitted.expect("emitted") - 0.8 * total).abs() < 1e-9 * total);
}

#[test]
fn a_turbid_layer_attenuates_the_focus_ballistically() {
    let turbid = focus_layout(GAUSSIAN, 0.8, "").replace(
        "layers = [ { thickness = 5.0, material = 1.33 } ]",
        "layers = [ { thickness = 5.0, material = 1.33, absorption = 0.1, scattering = 2.0, \
         anisotropy = 0.9, source = \"test values\" } ]",
    );
    let clear = read(&focus_layout(GAUSSIAN, 0.8, "")).expect("read");
    let turbid = read(&turbid).expect("read");
    let settings = PsfSettings {
        samples: 48,
        attenuated: true,
        ..Default::default()
    };
    let a = psf(&clear, 0, 0, &settings).expect("clear");
    let b = psf(&turbid, 0, 0, &settings).expect("turbid");
    // 1 mm of µa + µs = 2.1 /mm, each ray along its own slant: between exp(−2.1) on the
    // axis and exp(−2.1 / cos θ_max) at the edge.
    let cos_max = (1.0 - (b.na / INDEX).powi(2)).sqrt();
    assert!(b.transmission < (-2.1f64).exp() && b.transmission > (-2.1 / cos_max).exp());
    // The peak falls by about the transmission (the weights barely vary at this NA).
    let ratio = b.peak_intensity / a.peak_intensity;
    assert!(
        (ratio / b.transmission - 1.0).abs() < 0.01,
        "{ratio} {}",
        b.transmission
    );
    // Not attenuated, the focus is the clear one; the transmission is reported anyway.
    let plain = psf(
        &turbid,
        0,
        0,
        &PsfSettings {
            samples: 48,
            ..Default::default()
        },
    )
    .expect("plain");
    assert!((plain.peak_intensity / a.peak_intensity - 1.0).abs() < 1e-9);
    assert!((plain.transmission - b.transmission).abs() < 1e-12);
}

#[test]
fn the_excitation_command_and_its_graphs() {
    let directory =
        std::env::temp_dir().join(format!("optobench-excitation-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("scratch");
    let path = directory.join("focus.optobench");
    let pulse = "pulse = { repetition_rate_mhz = 80, width_fs = 120, shape = \"sech2\", prechirp_fs2 = 500 }";
    std::fs::write(
        &path,
        focus_layout(&format!("{UNIFORM}\n{pulse}"), 0.5, FOCUS_DYE),
    )
    .expect("written");
    let path = path.to_string_lossy().into_owned();
    let run = |command: &str, parameters: Value| {
        Registry::standard()
            .run(command, &parameters)
            .map_err(|e| e.message)
    };
    let small = json!({ "path": path, "samples": 64, "planes": 16, "across": 20 });
    let report = run("excitation", small.clone()).expect("excitation");
    assert_eq!(report["fluorophore"], "dye");
    assert_eq!(report["part"], "layer 1");
    assert!(report["two_photon"]["total_per_s"].as_f64().expect("total") > 0.0);
    // The prechirp alone widens the pulse (no glass on the way).
    let width = report["timing"]["width_at_focus_fs"]
        .as_f64()
        .expect("width");
    assert!((width - broadened_width(120e-15, 500e-30) * 1e15).abs() < 1e-9);
    let emitted = report["emitted_photons_per_s"].as_f64().expect("emitted");
    assert!(
        (emitted - 0.8 * report["two_photon"]["total_per_s"].as_f64().expect("total")).abs()
            < 1e-6 * emitted
    );

    for kind in ["excitation-depth", "excitation-map", "pulse-width"] {
        let mut parameters = small.clone();
        parameters["of"] = json!(kind);
        let graph = run("graph", parameters).unwrap_or_else(|e| panic!("{kind}: {e}"));
        assert!(graph["graph"].is_object() || graph.is_object(), "{kind}");
    }
    let refused = run("excitation", json!({ "path": path, "fluorophore": "none" }))
        .expect_err("no such fluorophore");
    assert!(refused.contains("no fluorophore `none`"), "{refused}");
    let _ = std::fs::remove_dir_all(&directory);
}
