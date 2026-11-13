//! M11c: the photon budget of an emitter in a sample (the `emission` command).
//!
//! Reference values:
//!
//! - An isotropic point emitter radiates the fraction (1 − cos θ) / 2 of its power into a
//!   cone of half-angle θ (the solid angle 2π (1 − cos θ) over 4π). An objective of
//!   numerical aperture NA collects, from an emitter in a medium of index n, the cone
//!   n sin θ = NA; with every medium matched there is no Fresnel loss on the way.
//! - Photons per second from power: N = P λ / (h c), h = 6.626 070 15 × 10⁻³⁴ J s and
//!   c = 299 792 458 m/s (exact in the SI).
//! - A filter's transmission folded with a line spectrum by hand:
//!   Σ wᵢ T(λᵢ) / Σ wᵢ.

use std::path::PathBuf;

use optobench_commands::Registry;
use serde_json::{Value, json};

/// A scratch directory, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("optobench-emission-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const N: f64 = 1.33;
const NA: f64 = 1.0;
const RINGS: usize = 100;
const HALF_ANGLE_DEG: f64 = 60.0;

/// A water-dipping objective (NA 1.0, f 9 mm, matched index 1.33 throughout) over a
/// matched sample, an emitter at the design focus facing the objective, `extra`
/// elements in the emission path and a camera 20 mm behind the pupil.
fn layout(data: &Scratch, emitter: &str, extra: &str, camera: &str) -> String {
    layout_with(
        data,
        emitter,
        &format!("{{ thickness = 5.0, material = {N} }}"),
        extra,
        camera,
    )
}

/// As [`layout`], with the sample's `layers` (from 45.5 mm; the emitter is at 47).
fn layout_with(data: &Scratch, emitter: &str, layers: &str, extra: &str, camera: &str) -> String {
    let path = data.0.join("emission.optobench");
    std::fs::write(
        &path,
        format!(
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
             [[source]]\nid = \"E\"\nkind = \"isotropic\"\nat = 47\nreversed = true\n\
             half_angle_deg = {HALF_ANGLE_DEG}\nrings = {RINGS}\npower_w = 1e-9\n{emitter}\n\n\
             [[element]]\nid = \"OBJ\"\nkind = \"objective\"\nat = 0\nfocal_length = 9.0\nna = {NA}\n\
             pupil_distance = 45.0\nworking_distance = 2.0\nimmersion = {N}\n\n\
             [[element]]\nid = \"S\"\nkind = \"sample\"\nat = 45.5\nabove = {N}\nbelow = {N}\n\
             layers = [ {layers} ]\n\n\
             {extra}\n\
             [[detector]]\nid = \"camera\"\nat = -20\nsize = [30, 30]\npixels = [10, 10]\n{camera}\n"
        ),
    )
    .expect("written");
    path.to_string_lossy().into_owned()
}

fn run(command: &str, parameters: Value) -> Result<Value, String> {
    Registry::standard()
        .run(command, &parameters)
        .map_err(|e| e.message)
}

fn emission(data: &Scratch, path: &str) -> Value {
    run(
        "emission",
        json!({ "path": path, "data_directory": data.path() }),
    )
    .expect("a budget")
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("a number: {v}"))
}

fn watts(v: &Value) -> f64 {
    number(&v["w"])
}

fn photons_per_watt(wavelength_um: f64) -> f64 {
    wavelength_um * 1e-6 / (6.626_070_15e-34 * 299_792_458.0)
}

/// The ends add up to what was traced, and with the part outside the cone to what was
/// emitted.
fn check_closes(budget: &Value) {
    let emitted = watts(&budget["emitted"]);
    let ends: f64 = budget["ends"]
        .as_object()
        .expect("ends")
        .values()
        .map(watts)
        .sum();
    assert!(
        (ends - watts(&budget["traced"])).abs() < 1e-9 * emitted,
        "{ends} vs {}",
        budget["traced"]
    );
    assert!(number(&budget["imbalance_w"]).abs() < 1e-9 * emitted);
    let cone = (1.0 - HALF_ANGLE_DEG.to_radians().cos()) / 2.0;
    assert!((watts(&budget["traced"]) / emitted - cone).abs() < 1e-12);
    assert!((watts(&budget["outside_cone"]) / emitted - (1.0 - cone)).abs() < 1e-12);
}

#[test]
fn an_objective_collects_the_cone_of_its_numerical_aperture() {
    let data = Scratch::new("collect");
    let path = layout(
        &data,
        "wavelength_um = 0.52",
        "",
        "quantum_efficiency = { value = 0.4, source = \"test value\" }",
    );
    let budget = emission(&data, &path);
    check_closes(&budget);
    let emitted = watts(&budget["emitted"]);
    // Photons per second of 1 nW at 0.52 µm.
    assert!(
        (number(&budget["emitted"]["photons_per_s"]) / (1e-9 * photons_per_watt(0.52)) - 1.0).abs()
            < 1e-12
    );
    // The NA cone, n sin θ = NA, against (1 − cos θ) / 2. The rays sample equal solid
    // angles in rings; the cut falls inside one ring, so the result is good to that
    // ring's share: ρ_c · cap / (rings + ½) of the emitted power, ρ_c² = (1 − cos θ) / cap.
    let cos = (1.0 - (NA / N).powi(2)).sqrt();
    let expected = (1.0 - cos) / 2.0;
    let cap = 1.0 - HALF_ANGLE_DEG.to_radians().cos();
    let ring = ((1.0 - cos) / cap).sqrt() * cap / (RINGS as f64 + 0.5);
    let collected = watts(&budget["collected"]) / emitted;
    assert!(
        (collected - expected).abs() <= ring,
        "{collected} vs {expected} (one ring {ring})"
    );
    // The objective clipped the rest of the cone; nothing else was lost on the way, so
    // all it collected reaches the camera, which counts 40 % of the photons.
    let objective = budget["items"]
        .as_array()
        .and_then(|i| i.iter().find(|i| i["id"] == "OBJ"))
        .expect("the objective clips");
    assert!(
        (watts(&objective["clipped"]) + watts(&budget["collected"]) - watts(&budget["traced"]))
            .abs()
            < 1e-9 * emitted
    );
    let camera = &budget["detectors"][0];
    assert!((watts(&camera["arriving"]) / watts(&budget["collected"]) - 1.0).abs() < 1e-12);
    assert!(
        (number(&camera["counted"]["photons_per_s"])
            / number(&camera["arriving"]["photons_per_s"])
            - 0.4)
            .abs()
            < 1e-12
    );
    assert_eq!(camera["quantum_efficiency_source"], "test value");
    assert!(watts(&budget["ends"]["reflected"]).abs() < 1e-12 * emitted);
}

#[test]
fn a_filter_curve_is_folded_with_the_emission_spectrum() {
    let data = Scratch::new("filter");
    let file = data.0.join("band.csv");
    std::fs::write(
        &file,
        "Wavelength (nm),Transmission (%)\n500,10\n520,80\n540,90\n560,20\n",
    )
    .expect("written");
    run(
        "import-curve",
        json!({ "path": file.to_string_lossy(), "name": "band",
                "source": "hand-written test table", "data_directory": data.path() }),
    )
    .expect("imported");
    // Lines (µm, weight) and the curve's T at each, 0.53 µm halfway between rows.
    let lines = [
        (0.50, 1.0, 0.10),
        (0.52, 2.0, 0.80),
        (0.53, 1.0, 0.85),
        (0.54, 3.0, 0.90),
        (0.56, 1.0, 0.20),
    ];
    let spectrum = lines
        .iter()
        .map(|(w, p, _)| format!("[{w}, {p}]"))
        .collect::<Vec<_>>()
        .join(", ");
    // A flat filter plate in the collimated emission path, index 1 (no refraction, no
    // Fresnel): the curve on one face, the other ideal.
    let filter = "[[element]]\nid = \"F\"\nkind = \"lens\"\nat = -10\n\
                  surfaces = [ { radius = \"inf\" }, { radius = \"inf\" } ]\nthickness = [2.0]\n\
                  material = [1.0]\ndiameter = 25.4\nclear_aperture = 25.0\n\
                  coatings = [ { curve = \"band\" }, \"ideal\" ]\n";
    let qe =
        "quantum_efficiency = { value = [[0.49, 0.30], [0.57, 0.50]], source = \"test table\" }";
    let path = layout(&data, &format!("spectrum = [{spectrum}]"), filter, qe);
    let budget = emission(&data, &path);
    check_closes(&budget);
    // Sums over ~150 000 rays: compared to 10⁻¹⁰.
    // Every line is collected alike (all indices constant), so the camera receives the
    // collected power times the folded transmission.
    let collected = watts(&budget["collected"]);
    let total: f64 = lines.iter().map(|l| l.1).sum();
    let folded: f64 = lines.iter().map(|(_, p, t)| p * t).sum::<f64>() / total;
    assert!((folded - 5.45 / 8.0).abs() < 1e-15);
    let camera = &budget["detectors"][0];
    assert!(
        (watts(&camera["arriving"]) / (collected * folded) - 1.0).abs() < 1e-10,
        "{} vs {}",
        watts(&camera["arriving"]),
        collected * folded
    );
    let filter_loss = budget["items"]
        .as_array()
        .and_then(|i| i.iter().find(|i| i["id"] == "F"))
        .expect("the filter rejects");
    assert!((watts(&filter_loss["at_faces"]) / (collected * (1.0 - folded)) - 1.0).abs() < 1e-10);
    // Counted photons: Σ collected share × T × QE(λ) × λ / (h c), QE linear in λ.
    let counted: f64 = lines
        .iter()
        .map(|(w, p, t)| {
            let qe = 0.30 + (0.50 - 0.30) * (w - 0.49) / (0.57 - 0.49);
            collected * p / total * t * qe * photons_per_watt(*w)
        })
        .sum();
    assert!(
        (number(&camera["counted"]["photons_per_s"]) / counted - 1.0).abs() < 1e-10,
        "{} vs {counted}",
        camera["counted"]
    );
}

#[test]
fn an_emitter_in_a_turbid_layer_loses_light_on_its_way_out() {
    // Clear 45.5–46.5 mm, then turbid (µa 0.1, µs 2 /mm) to 50.5: the emitter at 47 is
    // d = 0.5 mm deep in it, so a ray at θ from the axis keeps exp(−µt d / cos θ). Over
    // the traced cone the layer takes ½ ∫ (1 − e^{−µt d / u}) du, u = cos θ from cos θ_max
    // to 1 (equal solid angles), shared µa : µs between absorbed and scattered.
    let data = Scratch::new("turbid");
    let layers = format!(
        "{{ thickness = 1.0, material = {N} }}, \
         {{ thickness = 4.0, material = {N}, absorption = 0.1, scattering = 2.0, anisotropy = 0.9, source = \"test values\" }}"
    );
    let path = layout_with(&data, "wavelength_um = 0.52", &layers, "", "");
    let budget = emission(&data, &path);
    check_closes(&budget);
    let emitted = watts(&budget["emitted"]);
    let part = budget["sample"]
        .as_array()
        .and_then(|p| p.iter().find(|p| p["part"] == "layer 2"))
        .expect("layer 2");
    let (absorbed, scattered) = (watts(&part["absorbed"]), watts(&part["scattered"]));
    assert!((absorbed / scattered - 0.05).abs() < 1e-12);
    let (mu_t, d) = (2.1_f64, 0.5_f64);
    let low = HALF_ANGLE_DEG.to_radians().cos();
    let steps = 20_000;
    let h = (1.0 - low) / steps as f64;
    let integral: f64 = (0..steps)
        .map(|k| {
            let u = low + (k as f64 + 0.5) * h;
            (1.0 - (-mu_t * d / u).exp()) * h
        })
        .sum::<f64>()
        / 2.0;
    let lost = (absorbed + scattered) / emitted;
    assert!((lost / integral - 1.0).abs() < 1e-3, "{lost} vs {integral}");
    // Booked as the sample's, not elsewhere.
    assert!((watts(&budget["ends"]["absorbed_in_samples"]) - absorbed).abs() < 1e-12 * emitted);
    assert!((watts(&budget["ends"]["scattered"]) - scattered).abs() < 1e-12 * emitted);
}

#[test]
fn emitters_and_quantum_efficiencies_are_checked() {
    let data = Scratch::new("refused");
    let qe = |text: &str| {
        let path = layout(&data, "wavelength_um = 0.52", "", text);
        run(
            "emission",
            json!({ "path": path, "data_directory": data.path() }),
        )
        .expect_err("refused")
    };
    assert!(qe("quantum_efficiency = { value = 0.4 }").contains("source"));
    assert!(qe("quantum_efficiency = { value = 1.4, source = \"x\" }").contains("0 to 1"));
    // Outside the table's range: no count rather than a guess.
    assert!(
        qe("quantum_efficiency = { value = [[0.6, 0.3], [0.7, 0.4]], source = \"x\" }")
            .contains("no quantum efficiency at 0.52")
    );
    let path = layout(&data, "wavelength_um = 0.52\nnot_a_key = 1", "", "");
    assert!(
        run(
            "emission",
            json!({ "path": path, "data_directory": data.path() })
        )
        .expect_err("refused")
        .contains("not_a_key")
    );
}

/// A pulsed Gaussian laser focused by the objective 1.5 mm into a matched sample with a
/// two-photon dye, an emitter of the dye's emission placed at the focus and facing the
/// objective, the focus's detector and the camera behind the pupil.
fn excited_layout(data: &Scratch, dye: &str) -> String {
    let path = data.0.join("excited.optobench");
    std::fs::write(
        &path,
        format!(
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
             [[source]]\nid = \"laser\"\nkind = \"gaussian\"\nwaist_diameter = 3.0\nat = -10\n\
             wavelength_um = 0.92\npower_w = 0.01\n\
             pulse = {{ repetition_rate_mhz = 80, width_fs = 100 }}\n\n\
             [[source]]\nid = \"E\"\nkind = \"isotropic\"\nat = 47\nreversed = true\n\
             half_angle_deg = {HALF_ANGLE_DEG}\nrings = 24\npower_w = 1e-9\nwavelength_um = 0.52\n\n\
             [[element]]\nid = \"OBJ\"\nkind = \"objective\"\nat = 0\nfocal_length = 9.0\nna = {NA}\n\
             pupil_distance = 45.0\nworking_distance = 2.0\nimmersion = {N}\n\n\
             [[element]]\nid = \"S\"\nkind = \"sample\"\nat = 45.5\nabove = {N}\nbelow = {N}\n\
             layers = [ {{ thickness = 5.0, material = {N} }} ]\n\n\
             [[detector]]\nid = \"focus\"\nat = 47\nsize = [1, 1]\npixels = [10, 10]\n\n\
             [[detector]]\nid = \"camera\"\nat = -20\nsize = [30, 30]\npixels = [10, 10]\n\n\
             {dye}"
        ),
    )
    .expect("written");
    path.to_string_lossy().into_owned()
}

const DYE: &str = "[[fluorophore]]\nid = \"dye\"\nsource = \"test values\"\nquantum_yield = 0.8\n\
    two_photon_cross_section_gm = 40.0\n\
    concentrations = [ { sample = \"S\", layer = 1, micromolar = 10.0 } ]\n";

#[test]
fn emitters_spread_through_the_excited_volume() {
    let data = Scratch::new("excited");
    let path = excited_layout(&data, DYE);
    let parameters = json!({
        "path": path, "excited_by": "laser", "focus": "focus", "samples": 84,
        "cloud": [3, 5, 1.0], "data_directory": data.path(),
    });
    let spread = run("emission", parameters).expect("a budget");
    let excitation = &spread["excitation"];
    let photons = number(&excitation["emitted_photons_per_s"]);
    assert_eq!(excitation["emitters"], 45);
    let represented = number(&excitation["represented"]);
    assert!(represented > 0.0 && represented < 1.0, "{represented}");
    // Everything emitted is accounted for, the photons those of the excitation.
    let emitted = &spread["emitted"];
    assert!((number(&emitted["photons_per_s"]) / photons - 1.0).abs() < 1e-9);
    check_closes(&spread);
    // The same as the excitation command's photons emitted at the same settings.
    let alone = run(
        "excitation",
        json!({ "path": path, "source": "laser", "detector": "focus", "samples": 84,
                "planes": 24, "across": 24, "data_directory": data.path() }),
    )
    .expect("excitation");
    let by_excitation = number(&alone["emitted_photons_per_s"]);
    assert!(
        (photons / by_excitation - 1.0).abs() < 1e-9,
        "{photons} {by_excitation}"
    );
    // A few µm of spread barely changes what the objective collects: the cone's share,
    // within the ring sampling's bound as for one emitter.
    let theta = (NA / N).asin();
    let cone = (1.0 - theta.cos()) / 2.0;
    let collected = watts(&spread["collected"]) / watts(emitted);
    assert!((collected / cone - 1.0).abs() < 0.06, "{collected} {cone}");
    // All of it reaches the camera; the focus's detector takes none (left out).
    let camera = spread["detectors"]
        .as_array()
        .expect("detectors")
        .iter()
        .find(|d| d["id"] == "camera")
        .expect("camera");
    assert!((watts(&camera["arriving"]) / watts(&spread["collected"]) - 1.0).abs() < 1e-9);
    assert!(
        spread["detectors"]
            .as_array()
            .expect("detectors")
            .iter()
            .all(|d| d["id"] != "focus")
    );

    // Without a two-photon cross-section nothing is emitted from the focus.
    let one_photon = DYE.replace(
        "two_photon_cross_section_gm = 40.0\n",
        "one_photon_cross_section_cm2 = 1e-16\n",
    );
    let path = excited_layout(&data, &one_photon);
    let error = run(
        "emission",
        json!({ "path": path, "excited_by": "laser", "focus": "focus", "samples": 84,
                "data_directory": data.path() }),
    )
    .expect_err("no two-photon");
    assert!(error.contains("two-photon cross-section"), "{error}");
    let error = run("emission", json!({ "path": path, "focus": "focus" })).expect_err("alone");
    assert!(error.contains("belongs with `excited_by`"), "{error}");
}
