//! M11d2: heating of a sample by the light it absorbs (the `heating` command).
//!
//! Reference values:
//!
//! - A layer of thickness L absorbing µa (no scattering) between two faces held at the
//!   starting temperature, lit evenly over its whole radius R and insulated at its side:
//!   averaged over the radius the rise obeys k ΔT″ = −µa I₀ e^{−µa z} with I₀ = P / (π R²)
//!   (the side passes no heat, so the average sees only the slice's power), so
//!   ΔT(z) = I₀ / (k µa) (1 − e^{−µa z}) + A z, A = I₀ / (k µa) (e^{−µa L} − 1) / L.
//! - Its time course: the slowest mode of the slab decays with L² / (π² α), α = k / ρc.
//! - Water near 37 °C: k 0.623 W m⁻¹ K⁻¹ (J. W. Valvano, Table 12.9, in A. J. Welch and
//!   M. J. C. van Gemert, Optical-Thermal Response of Laser-Irradiated Tissue, 2nd ed.,
//!   2011), ρ 993 kg m⁻³ and c 4178 J kg⁻¹ K⁻¹ (K. R. Diller, ibid., Table 10.2).

use std::path::PathBuf;

use optobench_commands::Registry;
use serde_json::{Value, json};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("optobench-heating-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn design(&self, text: &str) -> String {
        let path = self.0.join("heating.optobench");
        std::fs::write(&path, text).expect("written");
        path.to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const K: f64 = 0.623e-3; // W mm⁻¹ K⁻¹
const RHO_C: f64 = 993.0 * 4178.0 * 1e-9; // J mm⁻³ K⁻¹
const MU_A: f64 = 0.5;
const L: f64 = 1.0;
const R: f64 = 5.0;
const POWER: f64 = 0.1;

const WATER: &str = "thermal = { conductivity_w_per_m_k = 0.623, density_kg_per_m3 = 993, \
    heat_capacity_j_per_kg_k = 4178, source = \"Welch and van Gemert (2011), Tables 12.9 and 10.2\" }";

/// A beam of `diameter` mm on a 1 mm layer absorbing 0.5 /mm, in a matched medium (no
/// Fresnel loss), its faces held and its side as `side`.
fn design(diameter: f64, side: &str) -> String {
    format!(
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\nsurrounding = 1.33\n\n\
         [[source]]\nid = \"beam\"\nkind = \"collimated\"\ndiameter = {diameter}\nrings = 40\nat = -5\n\
         wavelength_um = 0.8\npower_w = {POWER}\n\n\
         [[element]]\nid = \"S\"\nkind = \"sample\"\nat = 0\ndiameter = {}\n\
         layers = [ {{ thickness = {L}, material = 1.33, absorption = {MU_A}, source = \"test value\", {WATER} }} ]\n\
         thermal_boundaries = {{ top = \"fixed\", bottom = \"fixed\", side = {side} }}\n",
        2.0 * R
    )
}

fn run(command: &str, parameters: Value) -> Result<Value, String> {
    Registry::standard()
        .run(command, &parameters)
        .map_err(|e| e.message)
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("a number: {v}"))
}

#[test]
fn an_evenly_lit_absorbing_layer_heats_as_the_slab_solution() {
    let scratch = Scratch::new("slab");
    let path = scratch.design(&design(2.0 * R - 0.2, "\"insulated\""));
    let report = run(
        "heating",
        json!({ "path": path, "rings": 10, "slices": 100 }),
    )
    .expect("heats");
    // No Fresnel loss: the whole beam enters; Beer–Lambert takes 1 − e^{−µa L} of it.
    let surface = number(&report["surface_power_w"]);
    assert!((surface / POWER - 1.0).abs() < 1e-9, "{surface}");
    let absorbed = number(&report["absorbed_w"]);
    let expected = POWER * (1.0 - (-MU_A * L).exp());
    assert!(
        (absorbed / expected - 1.0).abs() < 1e-3,
        "{absorbed} {expected}"
    );
    // Out through the held faces, none through the insulated side.
    let b = &report["balance"];
    let out = number(&b["conducted_top_w"]) + number(&b["conducted_bottom_w"]);
    assert!((out / absorbed - 1.0).abs() < 1e-9);
    assert_eq!(number(&b["conducted_side_w"]), 0.0);
}

#[test]
fn the_radial_average_follows_the_slab_solution() {
    // Directly through the analysis, to average the field over the radius.
    use optobench_analysis::heating::{HeatingSettings, heating};
    use optobench_analysis::sample::LightSettings;
    let scene =
        optobench_io::scene_file::parse(&design(2.0 * R - 0.2, "\"insulated\""), &|n: &str| {
            optobench_io::store::material(None, n)
        })
        .expect("read");
    let settings = HeatingSettings {
        light: LightSettings {
            radius: None,
            rings: 10,
            slices: 100,
            ..Default::default()
        },
        ..Default::default()
    };
    let h = heating(&scene, 0, &settings).expect("heats");
    let g = &h.grid;
    let i0 = h.surface_power / (core::f64::consts::PI * R * R);
    let scale = i0 / (K * MU_A);
    let a = scale * ((-MU_A * L).exp() - 1.0) / L;
    let exact = |z: f64| scale * (1.0 - (-MU_A * z).exp()) + a * z;
    let top = (0..1000)
        .map(|n| exact(n as f64 * L / 1000.0))
        .fold(0.0, f64::max);
    let mut checked = 0;
    for j in 0..g.slices() {
        let z = g.z_at(j);
        let (mut sum, mut area) = (0.0, 0.0);
        for i in 0..g.rings() {
            let v = g.volume(i, j);
            sum += v * h.rise[g.index(i, j)];
            area += v;
        }
        let mean = sum / area;
        let expected = exact(z);
        if expected > 0.1 * top {
            // To 1 % of the peak (the grid coarsens away from the focal depth).
            assert!(
                (mean - expected).abs() < 0.01 * top,
                "z {z}: {mean} {expected}"
            );
            checked += 1;
        }
    }
    assert!(checked > 20, "{checked} of {} slices", g.slices());
    // The largest rise (a local one, under the beam's centre) is not below the radial
    // average's.
    assert!(h.peak.0 >= top * 0.99, "{} {top}", h.peak.0);
}

#[test]
fn a_scanned_field_and_a_time_course() {
    let scratch = Scratch::new("field");
    let path = scratch.design(&design(1.0, "\"fixed\""));
    let parked = run(
        "heating",
        json!({ "path": path, "rings": 40, "slices": 50 }),
    )
    .expect("parked");
    let field = run(
        "heating",
        json!({ "path": path, "rings": 40, "slices": 50, "field_mm": 3.0, "duty": 0.5 }),
    )
    .expect("field");
    // The duty cycle halves the heat; spread over the field the peak is lower.
    let ratio = number(&field["absorbed_w"]) / number(&parked["absorbed_w"]);
    assert!((ratio - 0.5).abs() < 1e-9, "{ratio}");
    assert!(number(&field["peak_rise_k"]) < number(&parked["peak_rise_k"]));
    let average = number(&field["field_average_rise_k"]);
    assert!(average > 0.0 && average <= number(&field["peak_rise_k"]));
    assert!(parked["field_average_rise_k"].is_null());
    assert!(field["notes"].to_string().contains("time average"));

    // In time, switched on long enough to settle (the slab's slowest mode decays with
    // L² / (π² α) ≈ 0.68 s): the focus ends at its steady rise; the heat balance closes.
    let course = run(
        "heating",
        json!({ "path": path, "rings": 40, "slices": 50, "duration_s": 8.0, "step_s": 0.05 }),
    )
    .expect("in time");
    let focus = &course["course"]["probes"][0]["rise_k"];
    let last = number(focus.as_array().expect("history").last().expect("values"));
    let steady = number(&parked["rise_at_focus_k"]);
    assert!((last / steady - 1.0).abs() < 0.01, "{last} {steady}");
    let b = &course["balance"];
    let out = number(&b["stored_j"])
        + number(&b["conducted_top_j"])
        + number(&b["conducted_bottom_j"])
        + number(&b["conducted_side_j"])
        + number(&b["perfused_j"]);
    assert!((out / number(&b["absorbed_j"]) - 1.0).abs() < 1e-9);
    // Switched off half-way, it cools: lower at the end than at the switch.
    let off = run(
        "heating",
        json!({ "path": path, "rings": 40, "slices": 50, "duration_s": 4.0, "step_s": 0.05, "off_at_s": 2.0 }),
    )
    .expect("off");
    let history: Vec<f64> = off["course"]["probes"][0]["rise_k"]
        .as_array()
        .expect("history")
        .iter()
        .map(number)
        .collect();
    assert!(history[history.len() - 1] < 0.2 * history[40]);

    // The graphs, and a warning above a threshold.
    for (kind, extra) in [
        ("temperature-map", json!({})),
        (
            "temperature-time",
            json!({ "duration_s": 1.0, "step_s": 0.05 }),
        ),
    ] {
        let mut parameters = json!({ "path": path, "of": kind, "rings": 40, "slices": 50 });
        for (k, v) in extra.as_object().expect("object") {
            parameters[k] = v.clone();
        }
        run("graph", parameters).unwrap_or_else(|e| panic!("{kind}: {e}"));
    }
    let warned = run(
        "heating",
        json!({ "path": path, "rings": 40, "slices": 50, "max_rise_k": 1e-6 }),
    )
    .expect("heats");
    assert!(
        warned["warnings"][0]
            .as_str()
            .expect("warning")
            .contains("above")
    );
}

#[test]
fn thermal_properties_are_needed_and_checked() {
    let scratch = Scratch::new("refused");
    let bare = design(1.0, "\"fixed\"")
        .replace(&format!(", {WATER}"), "")
        .replace(
            "thermal_boundaries = { top = \"fixed\", bottom = \"fixed\", side = \"fixed\" }\n",
            "",
        );
    let path = scratch.design(&bare);
    let error = run("heating", json!({ "path": path })).expect_err("no thermal");
    assert!(error.contains("no thermal properties"), "{error}");
    let read = |text: &str| {
        optobench_io::scene_file::parse(text, &|n: &str| optobench_io::store::material(None, n))
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    let no_source = design(1.0, "\"fixed\"").replace(
        ", source = \"Welch and van Gemert (2011), Tables 12.9 and 10.2\"",
        "",
    );
    assert!(
        read(&no_source)
            .expect_err("no source")
            .contains("say where")
    );
    let bad_side = design(1.0, "\"warm\"");
    assert!(
        read(&bad_side)
            .expect_err("bad side")
            .contains("heat transfer coefficient")
    );
    let blood_only = design(1.0, "\"fixed\"").replace(
        "source = \"Welch",
        "blood_density_kg_per_m3 = 1060, source = \"Welch",
    );
    assert!(
        read(&blood_only)
            .expect_err("blood")
            .contains("go with `perfusion_per_s`")
    );
    // A transfer coefficient and perfusion read in mm units.
    let scene = optobench_io::scene_file::parse(
        &design(1.0, "10.0").replace(
            "source = \"Welch",
            "perfusion_per_s = 8.5e-3, blood_density_kg_per_m3 = 1060, blood_heat_capacity_j_per_kg_k = 3600, source = \"Welch",
        ),
        &|n: &str| optobench_io::store::material(None, n),
    )
    .expect("read");
    let t = &scene.thermal[0];
    let optobench_scene::HeatBoundary::Transfer(h) = t.side else {
        panic!("a transfer coefficient")
    };
    assert!((h - 1e-5).abs() < 1e-18); // 10 W m⁻² K⁻¹ in W mm⁻² K⁻¹
    let p = t.layers[0].as_ref().expect("thermal");
    assert!((p.conductivity - K).abs() < 1e-15);
    assert!((p.capacity - RHO_C).abs() < 1e-15);
    assert!((p.perfusion - 1060.0 * 3600.0 * 8.5e-3 * 1e-9).abs() < 1e-15);
}

#[test]
fn monte_carlo_light_heats_the_slab_as_the_formula_says() {
    // The same evenly lit absorbing layer, its light by Monte Carlo (on the CPU, for the
    // same result everywhere): Beer–Lambert's absorbed power within its sampling error,
    // the heat balance closed, and the rise at the focus as the diffusion-fed one (both
    // carry the same light here: no scattering), at the same depth.
    let scratch = Scratch::new("monte-carlo");
    let path = scratch.design(&design(2.0 * R - 0.2, "\"insulated\""));
    let mc = run(
        "heating",
        json!({ "path": path, "rings": 10, "slices": 100, "transport": "monte-carlo",
                "packets": 20, "gpu": false, "threads": 4, "focal_depth_mm": 0.5 }),
    )
    .expect("heats");
    let absorbed = number(&mc["absorbed_w"]);
    let expected = POWER * (1.0 - (-MU_A * L).exp());
    // 4921 rays × 20 packets, each absorbed or not: σ = P √(p (1 − p) / N).
    let p = expected / POWER;
    let sigma = POWER * (p * (1.0 - p) / (4921.0 * 20.0)).sqrt();
    assert!(
        (absorbed - expected).abs() < 4.0 * sigma,
        "{absorbed} {expected}"
    );
    let b = &mc["balance"];
    let out = number(&b["conducted_top_w"]) + number(&b["conducted_bottom_w"]);
    assert!((out / absorbed - 1.0).abs() < 1e-9);
    assert!(
        mc["notes"][0]
            .as_str()
            .expect("note")
            .contains("Monte Carlo"),
        "{}",
        mc["notes"]
    );
    let diffusion = run(
        "heating",
        json!({ "path": path, "rings": 10, "slices": 100, "focal_depth_mm": 0.5 }),
    )
    .expect("heats");
    let (a, d) = (
        number(&mc["rise_at_focus_k"]),
        number(&diffusion["rise_at_focus_k"]),
    );
    assert!((a / d - 1.0).abs() < 0.03, "{a} {d}");
    // The fluence command reports the run's account.
    let fluence = run(
        "fluence",
        json!({ "path": path, "rings": 10, "slices": 50, "transport": "monte-carlo",
                "packets": 5, "gpu": false }),
    )
    .expect("fluence");
    let m = &fluence["monte_carlo"];
    let balance = number(&m["launched_w"])
        - number(&m["reflected_w"])
        - number(&m["transmitted_w"])
        - number(&fluence["layers"][0]["direct_absorbed_w"])
        - number(&m["roulette_w"]);
    assert!(balance.abs() < 1e-9, "{balance}");
    let refused = run("heating", json!({ "path": path, "packets": 5 })).expect_err("diffusion");
    assert!(refused.contains("goes with `transport"), "{refused}");
}
