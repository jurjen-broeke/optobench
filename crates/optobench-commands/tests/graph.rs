//! M10a: results as graphs, compared with saved results and references.
//!
//! The reference curve is the Airy pattern I(v) = I₀ (2 J₁(v) / v)², v = 2π NA r / λ
//! (Born and Wolf, Principles of Optics, 7th ed., §8.5.2), with J₁ from Bessel's
//! integral J₁(v) = (1/π) ∫₀^π cos(τ − v sin τ) dτ (Watson, A Treatise on the Theory of
//! Bessel Functions, §2.2). Set `OPTOBENCH_GRAPH_OUT` to a directory to keep the SVG files
//! for looking at.

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("optobench-graph-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("scratch");
    path
}

/// Where SVG files go: `OPTOBENCH_GRAPH_OUT` if set, else a scratch directory.
fn out(name: &str) -> String {
    let dir = std::env::var_os("OPTOBENCH_GRAPH_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| scratch("out"));
    std::fs::create_dir_all(&dir).expect("output directory");
    dir.join(name).to_string_lossy().into_owned()
}

/// A dry NA 0.1 objective (f = 10 mm) focusing a 2.4 mm beam, and a 50 × 50 µm screen
/// with 40 × 40 pixels at its focus; `extra` goes in the objective.
fn layout(label: &str, extra: &str, rest: &str) -> PathBuf {
    layout_at(label, extra, rest, 85.0)
}

/// As [`layout`], with the screen at `screen`.
fn layout_at(label: &str, extra: &str, rest: &str, screen: f64) -> PathBuf {
    let dir = scratch(label);
    let path = dir.join(format!("{label}.optobench"));
    std::fs::write(
        &path,
        format!(
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
             [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\n\
             diameter = 2.4\nrings = 6\nat = 0\n\n\
             [[element]]\nid = \"OBJ\"\nkind = \"objective\"\nat = 50\nfocal_length = 10.0\nna = 0.1\n\
             pupil_distance = 30.0\nworking_distance = 5.0\n{extra}\n\n{rest}\
             [[detector]]\nid = \"focus\"\nat = {screen}\nsize = [0.05, 0.05]\npixels = [40, 40]\n"
        ),
    )
    .expect("written");
    path
}

fn run(parameters: Value) -> Value {
    let mut parameters = parameters;
    parameters["data_directory"] = json!(scratch("empty").to_string_lossy());
    Registry::standard()
        .run("graph", &parameters)
        .unwrap_or_else(|e| panic!("{}", e.message))
}

fn series(graph: &Value) -> &Vec<Value> {
    graph["graph"]["series"].as_array().expect("series")
}

fn bessel_j1(v: f64) -> f64 {
    let n = 400;
    let h = core::f64::consts::PI / n as f64;
    let f = |t: f64| (t - v * t.sin()).cos();
    // Simpson's rule.
    let mut sum = f(0.0) + f(core::f64::consts::PI);
    for k in 1..n {
        sum += f(k as f64 * h) * if k % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0 / core::f64::consts::PI
}

#[test]
fn a_psf_profile_is_drawn_against_the_airy_pattern_and_reads_back() {
    let path = layout("airy", "", "");
    let svg = out("psf-lateral-airy.svg");
    let saved = scratch("saved").join("psf-lateral.json");
    let first = run(json!({
        "path": path.to_string_lossy(), "of": "psf-lateral", "samples": 96,
        "svg": svg, "json": saved.to_string_lossy(),
    }));
    // The profile is the PSF's own.
    let psf = Registry::standard()
        .run(
            "psf",
            &json!({ "path": path.to_string_lossy(), "samples": 96, "data_directory": scratch("empty").to_string_lossy() }),
        )
        .expect("psf");
    assert_eq!(series(&first)[0]["points"], psf["lateral_x"]);
    // With the Airy pattern of the same peak as a reference.
    let peak = psf["peak_w_per_mm2"].as_f64().expect("peak");
    let (na, lambda) = (0.1, 0.55e-3);
    let airy: Vec<Value> = (-80..=80)
        .map(|k| {
            let r = k as f64 * 0.0001;
            let v = 2.0 * core::f64::consts::PI * na * r / lambda;
            let level = if v.abs() < 1e-9 {
                1.0
            } else {
                (2.0 * bessel_j1(v) / v).powi(2)
            };
            json!([r, peak * level])
        })
        .collect();
    let compared = run(json!({
        "from": saved.to_string_lossy(),
        "reference": { "label": "Airy pattern", "points": airy },
        "svg": out("psf-lateral-against-airy.svg"),
    }));
    let list = series(&compared);
    assert_eq!(list.len(), 3);
    assert_eq!(list[2]["reference"], true);
    // Along x the PSF and the Airy pattern agree to 0.5 % of the peak.
    let profile = list[0]["points"].as_array().expect("points");
    for p in profile {
        let (r, value) = (p[0].as_f64().expect("r"), p[1].as_f64().expect("v"));
        let v = 2.0 * core::f64::consts::PI * na * r / lambda;
        let level = if v.abs() < 1e-9 {
            1.0
        } else {
            (2.0 * bessel_j1(v) / v).powi(2)
        };
        assert!(
            (value - peak * level).abs() < 5e-3 * peak,
            "r = {r}: {value} vs {}",
            peak * level
        );
    }
    // Saved, read back and drawn again: the same graph and the same SVG.
    let again =
        run(json!({ "from": saved.to_string_lossy(), "svg": out("psf-lateral-again.svg") }));
    assert_eq!(again["graph"], first["graph"]);
    assert_eq!(
        std::fs::read_to_string(&svg).expect("svg"),
        std::fs::read_to_string(out("psf-lateral-again.svg")).expect("svg")
    );
    // A graph minus itself is zero everywhere.
    let zero =
        run(json!({ "from": saved.to_string_lossy(), "difference": saved.to_string_lossy() }));
    for s in series(&zero) {
        for p in s["points"].as_array().expect("points") {
            assert_eq!(p[1].as_f64(), Some(0.0));
        }
    }
}

#[test]
fn two_designs_are_compared_on_shared_axes_and_by_difference() {
    // An objective corrected for 0.17 mm of glass, imaging through 0.5 mm, before and
    // after setting its collar to the glass that is there: the PSF along the axis, both
    // on one graph and their difference. Set, it focuses at its new design focus, 0.05 mm
    // before the screen (a screen on the glass's face would meet the rays there); not
    // set, 0.33 mm × (1 − 1/n) = 0.11 mm short of that.
    let glass = "cover_glass = { thickness = 0.17, material = 1.5255 }\n\
                 dof = [ { name = \"OBJ_collar\", kind = \"collar\", range = [-0.5, 0.5] } ]";
    let sample = "[[element]]\nid = \"S\"\nkind = \"sample\"\nat = 85.0\n\
                  layers = [ { thickness = 0.5, material = 1.5255 }, { thickness = 5.0, material = 1.0 } ]\n\n";
    let path = layout_at("collar", glass, sample, 85.55);
    let saved = scratch("collar-saved");
    let axial = json!([-0.3, 0.05]);
    let before = saved.join("before.json");
    run(json!({
        "path": path.to_string_lossy(), "of": "psf-axial", "samples": 64, "axial": axial, "points": 401,
        "json": before.to_string_lossy(), "svg": out("collar-before.svg"),
    }));
    let after = run(json!({
        "path": path.to_string_lossy(), "of": "psf-axial", "samples": 64, "axial": axial, "points": 401,
        "dofs": { "OBJ_collar": 0.33 }, "name": "collar set",
        "compare": [ { "path": before.to_string_lossy(), "name": "collar at 0.17 mm" } ],
        "svg": out("collar-compared.svg"), "csv": out("collar-compared.csv"),
    }));
    let labels: Vec<&str> = series(&after)
        .iter()
        .filter_map(|s| s["label"].as_str())
        .collect();
    assert_eq!(labels, ["collar set", "collar at 0.17 mm"]);
    assert_eq!(after["written"].as_array().map(Vec::len), Some(2));
    let csv = std::fs::read_to_string(out("collar-compared.csv")).expect("csv");
    assert!(csv.starts_with("series,z (mm),n |E|² (W/mm²)\n"));
    assert_eq!(csv.lines().count(), 1 + 2 * 401);
    let difference = run(json!({
        "path": path.to_string_lossy(), "of": "psf-axial", "samples": 64, "axial": axial, "points": 401,
        "dofs": { "OBJ_collar": 0.33 }, "difference": before.to_string_lossy(),
        "svg": out("collar-difference.svg"),
    }));
    assert_eq!(difference["graph"]["y"]["label"], "Δ n |E|²");
}

#[test]
fn maps_and_spot_diagrams_are_drawn_to_scale() {
    let path = layout("maps", "", "");
    let section = run(json!({
        "path": path.to_string_lossy(), "of": "psf-section", "samples": 64, "section": [41, 61],
        "svg": out("psf-section.svg"),
    }));
    let map = &section["graph"]["map"];
    assert_eq!(
        (map["columns"].as_u64(), map["rows"].as_u64()),
        (Some(41), Some(61))
    );
    assert_eq!(map["colours"], "sequential");
    let irradiance = run(json!({
        "path": path.to_string_lossy(), "of": "irradiance", "svg": out("irradiance.svg"),
    }));
    let values = irradiance["graph"]["map"]["values"]
        .as_array()
        .expect("values");
    assert_eq!(values.len(), 1600);
    assert_eq!(irradiance["graph"]["equal_aspect"], true);
    let spot = run(json!({
        "path": path.to_string_lossy(), "of": "spot", "svg": out("spot.svg"),
    }));
    assert_eq!(spot["graph"]["equal_aspect"], true);
    assert_eq!(series(&spot)[0]["mark"], "dots");
    let svg = std::fs::read_to_string(out("spot.svg")).expect("svg");
    assert!(svg.contains("<circle"));
}

#[test]
fn the_focus_of_a_mismatched_objective_is_drawn_by_ray_height() {
    let sample = "[[element]]\nid = \"W\"\nkind = \"sample\"\nat = 84.0\n\
                  layers = [ { thickness = 5.0, material = 1.333 } ]\n\n";
    let path = layout("focus", "", sample);
    let focus = run(json!({
        "path": path.to_string_lossy(), "of": "focus", "svg": out("focus.svg"),
    }));
    assert_eq!(focus["graph"]["x"]["label"], "launch height");
    assert!(
        series(&focus)[0]["points"]
            .as_array()
            .expect("points")
            .len()
            > 50
    );
}

#[test]
fn unknown_kinds_and_mixed_sources_are_refused() {
    let path = layout("refuse", "", "");
    for parameters in [
        json!({ "path": path.to_string_lossy(), "of": "histogram" }),
        json!({ "path": path.to_string_lossy(), "of": "spot", "from": "x.json" }),
        json!({ "path": path.to_string_lossy() }),
    ] {
        let error = Registry::standard()
            .run("graph", &parameters)
            .expect_err("refused");
        assert_eq!(
            error.code.as_str(),
            "invalid_parameters",
            "{}",
            error.message
        );
    }
    assert!(Path::new(&path).exists());
}
