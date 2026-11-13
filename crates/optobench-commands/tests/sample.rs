//! M11a: turbid sample layers and the direct beam in them.
//!
//! Reference values:
//!
//! - Beer–Lambert with extinction µt = µa + µs: the direct beam keeps exp(−µt t) of its
//!   power through a layer of thickness t, and of what it loses the share µa / µt is
//!   absorbed and µs / µt scattered (e.g. A. J. Welch and M. J. C. van Gemert,
//!   Optical-Thermal Response of Laser-Irradiated Tissue, 2nd ed., 2011, ch. 3).
//! - Water's absorption at 920 nm from Table I of G. M. Hale and M. R. Querry, Appl. Opt.
//!   12, 555 (1973): k = 4.86 × 10⁻⁷ at 0.900 µm and 1.06 × 10⁻⁶ at 0.925 µm, linearly
//!   9.452 × 10⁻⁷ at 0.920 µm, so µa = 4π k / λ.
//!
//! Every layer is index-matched to its neighbours, so no Fresnel loss enters.

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

fn scratch(label: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("optobench-sample-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("scratch");
    path
}

/// A 2 mm collimated beam (1 W at `wavelength` µm) into a sample at 10 mm with `layers`
/// between `around` media, and a screen 1 mm past it in the medium below.
fn layout(label: &str, wavelength: f64, around: &str, layers: &str, total: f64) -> PathBuf {
    let path = scratch(label).join(format!("{label}.optobench"));
    std::fs::write(
        &path,
        format!(
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
             [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = {wavelength}\npower_w = 1.0\ndiameter = 2.0\nrings = 4\nat = 0\n\n\
             [[element]]\nid = \"S\"\nkind = \"sample\"\nat = 10\nabove = {around}\nbelow = {around}\n\
             layers = [ {layers} ]\n\n\
             [[detector]]\nid = \"screen\"\nat = {}\nsize = [5, 5]\npixels = [10, 10]\n",
            10.0 + total + 1.0
        ),
    )
    .expect("written");
    path
}

fn trace(path: &Path) -> Result<Value, String> {
    Registry::standard()
        .run(
            "trace",
            &json!({ "path": path.to_string_lossy(),
                     "data_directory": scratch("empty").to_string_lossy() }),
        )
        .map_err(|e| e.message)
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("a number: {v}"))
}

fn part<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["layers"]
        .as_array()
        .and_then(|l| l.iter().find(|p| p["part"] == name))
        .unwrap_or_else(|| panic!("no `{name}` in {}", report["layers"]))
}

#[test]
fn a_turbid_stack_takes_its_share_from_the_direct_beam_layer_by_layer() {
    // Layer 1: µa 0.05, µs 2.0 /mm over 0.5 mm; layer 2: µa 0.1 /mm, clear, over 1 mm.
    let path = layout(
        "stack",
        0.8,
        "1.0",
        "{ thickness = 0.5, material = 1.0, absorption = 0.05, scattering = 2.0, anisotropy = 0.9, source = \"test values\" }, \
         { thickness = 1.0, material = 1.0, absorption = 0.1, source = \"test values\" }",
        1.5,
    );
    let report = trace(&path).expect("traces");
    let (mu_a1, mu_s1, t1, mu_a2, t2): (f64, f64, f64, f64, f64) = (0.05, 2.0, 0.5, 0.1, 1.0);
    let after_first = (-(mu_a1 + mu_s1) * t1).exp();
    let through = after_first * (-mu_a2 * t2).exp();
    let detected = number(&report["detectors"][0]["direct_power_w"]);
    assert!(
        (detected - through).abs() < 1e-12,
        "{detected} vs {through}"
    );
    let lost = 1.0 - after_first;
    let first = part(&report, "layer 1");
    assert!((number(&first["absorbed_w"]) - lost * mu_a1 / (mu_a1 + mu_s1)).abs() < 1e-12);
    assert!((number(&first["scattered_w"]) - lost * mu_s1 / (mu_a1 + mu_s1)).abs() < 1e-12);
    let second = part(&report, "layer 2");
    assert!(
        (number(&second["absorbed_w"]) - after_first * (1.0 - (-mu_a2 * t2).exp())).abs() < 1e-12
    );
    assert_eq!(number(&second["scattered_w"]), 0.0);
    // The ledger books the scattered power and still closes.
    let ledger = &report["ledger"];
    assert!((number(&ledger["scattered_w"]) - number(&first["scattered_w"])).abs() < 1e-12);
    assert!(number(&ledger["imbalance_w"]).abs() < 1e-12, "{ledger}");
}

#[test]
fn water_absorbs_at_920_nm_as_hale_and_querry_s_table_says() {
    let path = layout(
        "water",
        0.92,
        "\"water\"",
        "{ thickness = 10.0, material = \"water\" }",
        10.0,
    );
    let report = trace(&path).expect("traces");
    let k = 4.86e-7 + (0.920 - 0.900) / 0.025 * (1.06e-6 - 4.86e-7);
    let mu_a = 4.0 * core::f64::consts::PI * k / 0.92e-3;
    // The screen sits 1 mm into the water below, which absorbs too: 11 mm in all.
    let expected = (-mu_a * 11.0).exp();
    let detected = number(&report["detectors"][0]["direct_power_w"]);
    assert!(
        (detected - expected).abs() < 1e-12,
        "{detected} vs {expected}"
    );
    assert!(
        (number(&part(&report, "layer 1")["absorbed_w"]) - (1.0 - (-mu_a * 10.0).exp())).abs()
            < 1e-12
    );
}

#[test]
fn scattering_given_as_a_spectrum_is_interpolated_by_wavelength() {
    // µs from 1 /mm at 0.5 µm to 3 /mm at 1.0 µm: 2.2 /mm at 0.8 µm.
    let path = layout(
        "spectrum",
        0.8,
        "1.0",
        "{ thickness = 0.4, material = 1.0, scattering = [[0.5, 1.0], [1.0, 3.0]], source = \"test values\" }",
        0.4,
    );
    let report = trace(&path).expect("traces");
    let expected = (-2.2 * 0.4_f64).exp();
    assert!((number(&report["detectors"][0]["direct_power_w"]) - expected).abs() < 1e-12);
}

#[test]
fn turbid_layers_say_where_their_values_come_from_and_stay_in_range() {
    for (label, layer, words) in [
        (
            "no-source",
            "{ thickness = 1.0, material = 1.0, scattering = 1.0 }",
            "`source`",
        ),
        (
            "bad-g",
            "{ thickness = 1.0, material = 1.0, anisotropy = 1.5, source = \"x\" }",
            "−1 to 1",
        ),
        (
            "negative",
            "{ thickness = 1.0, material = 1.0, absorption = -0.1, source = \"x\" }",
            "not negative",
        ),
        (
            "order",
            "{ thickness = 1.0, material = 1.0, scattering = [[1.0, 2.0], [0.5, 1.0]], source = \"x\" }",
            "increasing wavelength",
        ),
        (
            "stray-source",
            "{ thickness = 1.0, material = 1.0, source = \"x\" }",
            "`source` goes with",
        ),
    ] {
        let error = trace(&layout(label, 0.8, "1.0", layer, 1.0)).expect_err("refused");
        assert!(error.contains(words), "{label}: {error}");
    }
}

fn run(command: &str, parameters: Value) -> Value {
    let mut parameters = parameters;
    parameters["data_directory"] = json!(scratch("empty").to_string_lossy());
    Registry::standard()
        .run(command, &parameters)
        .unwrap_or_else(|e| panic!("{}", e.message))
}

#[test]
fn the_light_in_a_turbid_slab_is_accounted_for_direct_and_diffuse() {
    // A 0.5 mm beam at 800 nm into 3 mm of a brain-like medium (n = 1.36, µa 0.02, µs 10
    // /mm, g 0.9: µs′ = 1 /mm) between media of the same index. On the grid: the power
    // scattered out of the direct beam and absorbed from it equal the trace's own accounts
    // of the layer (both integrate Beer–Lambert exactly), and the diffuse light it feeds is
    // all absorbed or leaves through the grid's faces.
    let path = layout(
        "slab",
        0.8,
        "1.36",
        "{ thickness = 3.0, material = 1.36, absorption = 0.02, scattering = 10.0, anisotropy = 0.9, source = \"test values\" }",
        3.0,
    );
    let text = std::fs::read_to_string(&path).expect("layout");
    std::fs::write(
        &path,
        text.replace("diameter = 2.0\nrings = 4", "diameter = 0.5\nrings = 12"),
    )
    .expect("narrower beam");
    let report = trace(&path).expect("traces");
    let layer = part(&report, "layer 1");
    let light = run(
        "fluence",
        json!({ "path": path.to_string_lossy(), "radius": 6.0, "rings": 60, "slices": 60,
                "primary": "ballistic" }),
    );
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * b.abs().max(1e-12);
    assert!(
        close(number(&light["scattered_w"]), number(&layer["scattered_w"])),
        "{light}"
    );
    assert!(
        close(
            number(&light["direct_absorbed_w"]),
            number(&layer["absorbed_w"])
        ),
        "{light}"
    );
    let diffuse = &light["diffuse"];
    let out = number(&diffuse["absorbed_w"])
        + number(&diffuse["top_w"])
        + number(&diffuse["bottom_w"])
        + number(&diffuse["side_w"]);
    assert!(close(out, number(&light["scattered_w"])), "{diffuse}");
    // Most of the scattered light comes back out of a slab only 3 transport lengths thick
    // with so little absorption, through its faces.
    assert!(
        number(&diffuse["top_w"]) + number(&diffuse["bottom_w"]) > number(&diffuse["absorbed_w"])
    );
    // The layer's account by part sums the direct absorption.
    let parts = light["layers"].as_array().expect("layers");
    assert!(close(
        number(&parts[0]["direct_absorbed_w"]),
        number(&light["direct_absorbed_w"])
    ));
    // Delta-Eddington (the default): the primary decays at µa + µs (1 − g) = 1.02 /mm, so
    // over the 3 mm it gives up 1 − e^{−3.06} of what enters, µs (1 − g) / µt* of that to
    // the diffuse light and µa / µt* absorbed (Star, in Welch and van Gemert 2011, §6.3.7.1).
    let delta = run(
        "fluence",
        json!({ "path": path.to_string_lossy(), "radius": 6.0, "rings": 60, "slices": 60 }),
    );
    // What enters: the ballistic beam is spent in the layer (e^{−30} remains).
    let entering = number(&light["scattered_w"]) + number(&light["direct_absorbed_w"]);
    let mu_t = 0.02 + 10.0 * (1.0 - 0.9);
    let given = entering * (1.0 - (-mu_t * 3.0f64).exp());
    let scattered = number(&delta["scattered_w"]);
    let absorbed = number(&delta["direct_absorbed_w"]);
    assert!(
        (scattered / (given * 1.0 / mu_t) - 1.0).abs() < 1e-6,
        "{scattered} {given}"
    );
    assert!(
        (absorbed / (given * 0.02 / mu_t) - 1.0).abs() < 1e-6,
        "{absorbed} {given}"
    );
    // Seeded deeper, less of the diffuse light leaves through the top than with the
    // ballistic source.
    let top = |l: &Value| number(&l["diffuse"]["top_w"]) / number(&l["scattered_w"]);
    assert!(top(&delta) < top(&light), "{} {}", top(&delta), top(&light));
    // And as graphs.
    for of in ["fluence-depth", "absorption"] {
        let graph = run(
            "graph",
            json!({ "path": path.to_string_lossy(), "of": of, "radius": 6.0, "rings": 60, "slices": 60 }),
        );
        assert!(
            graph["graph"]["title"]
                .as_str()
                .is_some_and(|t| t.contains("`S`")),
            "{graph}"
        );
    }
}

#[test]
fn clear_layers_between_turbid_ones_are_refused_for_diffusion() {
    let path = layout(
        "gap",
        0.8,
        "1.36",
        "{ thickness = 1.0, material = 1.36, scattering = 5.0, source = \"x\" }, \
         { thickness = 1.0, material = 1.36 }, \
         { thickness = 1.0, material = 1.36, scattering = 5.0, source = \"x\" }",
        3.0,
    );
    let error = Registry::standard()
        .run(
            "fluence",
            &json!({ "path": path.to_string_lossy(), "data_directory": scratch("empty").to_string_lossy() }),
        )
        .expect_err("refused");
    assert!(error.message.contains("layer 2"), "{}", error.message);
}
