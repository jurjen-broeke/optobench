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
        "saved": saved.to_string_lossy(),
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
        run(json!({ "saved": saved.to_string_lossy(), "svg": out("psf-lateral-again.svg") }));
    assert_eq!(again["graph"], first["graph"]);
    assert_eq!(
        std::fs::read_to_string(&svg).expect("svg"),
        std::fs::read_to_string(out("psf-lateral-again.svg")).expect("svg")
    );
    // A graph minus itself is zero everywhere.
    let zero =
        run(json!({ "saved": saved.to_string_lossy(), "difference": saved.to_string_lossy() }));
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
        json!({ "path": path.to_string_lossy(), "of": "spot", "saved": "x.json" }),
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

fn write(label: &str, text: &str) -> PathBuf {
    let path = scratch(label).join(format!("{label}.optobench"));
    std::fs::write(&path, text).expect("written");
    path
}

fn points(series: &Value) -> Vec<(f64, f64)> {
    series["points"]
        .as_array()
        .expect("points")
        .iter()
        .map(|p| {
            (
                p[0].as_f64().unwrap_or(f64::NAN),
                p[1].as_f64().unwrap_or(f64::NAN),
            )
        })
        .collect()
}

#[test]
fn a_gaussian_beam_is_drawn_along_its_path_through_a_lens() {
    // λ = 0.633 µm, w₀ = 0.5 mm at the source, a 100 mm ideal lens 50 mm on, a screen at
    // 300 mm: before the lens w(z) = w₀ √(1 + (z / z_R)²), z_R = π w₀² / λ (Siegman,
    // Lasers, §17.1); after it, the curve ends on the radius the `gaussian` command gives
    // at the screen. Both planes are marked at their distance along the path.
    let path = write(
        "beam",
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"laser\"\nkind = \"gaussian\"\nwavelength_um = 0.633\npower_w = 0.01\nwaist_diameter = 1.0\nat = 0\n\n\
         [[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 50\nfocal_length = 100\n\n\
         [[detector]]\nid = \"screen\"\nat = 300\nsize = [5, 5]\npixels = [10, 10]\n",
    );
    let graph = run(
        json!({ "path": path.to_string_lossy(), "of": "gaussian", "svg": out("gaussian.svg") }),
    );
    let list = series(&graph);
    assert_eq!(list.len(), 1, "a round beam: one curve");
    let curve = points(&list[0]);
    let (w0, lambda) = (0.5, 0.633e-3);
    let rayleigh = core::f64::consts::PI * w0 * w0 / lambda;
    for &(z, w) in curve.iter().filter(|p| p.0 <= 50.0) {
        let expected = w0 * (1.0 + (z / rayleigh).powi(2)).sqrt();
        assert!((w - expected).abs() < 1e-9, "z = {z}: {w} vs {expected}");
    }
    let planes = Registry::standard()
        .run("gaussian", &json!({ "path": path.to_string_lossy() }))
        .expect("gaussian");
    let at_screen = planes["planes"][1]["radius_mm"][0]
        .as_f64()
        .expect("radius");
    let last = curve
        .iter()
        .rev()
        .find(|p| p.0.is_finite())
        .expect("a point");
    assert!(
        (last.0 - 300.0).abs() < 1e-9 && (last.1 - at_screen).abs() < 1e-9,
        "{last:?} vs {at_screen}"
    );
    let markers = graph["graph"]["markers"].as_array().expect("markers");
    let at: Vec<(f64, &str)> = markers
        .iter()
        .map(|m| {
            (
                m["at"].as_f64().expect("at"),
                m["label"].as_str().expect("label"),
            )
        })
        .collect();
    assert_eq!(at, [(50.0, "L"), (300.0, "screen")]);
}

#[test]
fn a_galvo_sweep_is_drawn_as_the_hand_calculation_says() {
    // The 4f galvo relay of commands `tests/scan.rs` (f₁ = 50, f₂ = 200 mm) into an
    // objective (f = 9 mm): swept ±2°, the chief ray does not walk at the objective's
    // pupil, and lands on the image at f · (f₁ / f₂) · tan 2α from the axis.
    let (f1, f2, f) = (50.0, 200.0, 9.0);
    let (galvo, scan) = (50.0, 50.0 + f1);
    let (tube, pupil) = (scan + f1 + f2, scan + f1 + f2 + f2);
    let path = write(
        "galvo",
        &format!(
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
             [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 1.0\nrings = 4\nat = 0\n\n\
             [[element]]\nid = \"G\"\nkind = \"mirror\"\nat = {galvo}\nfold = 90\ndiameter = 10\nclear_aperture = 9\n\
             dof = [ {{ name = \"scan\", kind = \"tilt\", axis = [0, 0, 1], frame = \"scene\", range = [-10, 10] }} ]\n\n\
             [[element]]\nid = \"SL\"\nkind = \"ideal_lens\"\nat = {scan}\nfocal_length = {f1}\n\n\
             [[element]]\nid = \"TL\"\nkind = \"ideal_lens\"\nat = {tube}\nfocal_length = {f2}\ndiameter = 50\n\n\
             [[element]]\nid = \"OBJ\"\nkind = \"objective\"\nat = {pupil}\nfocal_length = {f}\nna = 0.3\n\
             pupil_distance = 30.0\nworking_distance = 5.0\n\n\
             [[detector]]\nid = \"image\"\nat = {}\nsize = [10, 10]\npixels = [100, 100]\n",
            pupil + 35.0
        ),
    );
    let sweep = |element: &str, name: &str| {
        run(json!({
            "path": path.to_string_lossy(), "of": "sweep", "dof": "scan", "from": -2, "to": 2, "steps": 9,
            "quantity": "chief", "element": element, "svg": out(name),
        }))
    };
    let walk = sweep("OBJ", "sweep-pupil.svg");
    assert_eq!(walk["graph"]["x"]["unit"], "angle");
    for s in series(&walk) {
        for (_, p) in points(s) {
            assert!(p.abs() < 1e-9, "pupil walk {p}");
        }
    }
    let image = sweep("image", "sweep-image.svg");
    let moving: Vec<(f64, f64)> = series(&image)
        .iter()
        .map(points)
        .max_by(|a, b| {
            let span = |s: &Vec<(f64, f64)>| s.iter().map(|p| p.1.abs()).fold(0.0, f64::max);
            span(a).total_cmp(&span(b))
        })
        .expect("a series");
    assert_eq!(moving.len(), 9);
    for (alpha, position) in moving {
        let expected = f * (f1 / f2) * (2.0 * alpha).tan();
        assert!(
            (position.abs() - expected.abs()).abs() < 1e-9,
            "α = {alpha}: {position} vs {expected}"
        );
    }
}

#[test]
fn the_optimiser_s_progress_is_drawn_step_by_step() {
    // The refocusing case of commands `tests/optimize.rs`: a screen 5 mm past the
    // perfect focus of a hyperboloid (60 mm past its vertex), moved back by the optimiser.
    let path = write(
        "refocus",
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 10.0\nrings = 4\nat = 0\n\n\
         [[element]]\nid = \"L1\"\nkind = \"lens\"\nat = 20\n\
         surfaces = [{ radius = \"inf\" }, { radius = -30, conic = -2.25 }]\nthickness = [6]\nmaterial = [1.5]\n\
         diameter = 25.4\nclear_aperture = 22.9\ncoating = \"ideal\"\n\n\
         [[detector]]\nid = \"screen\"\nat = 86\nsize = [4, 4]\npixels = [20, 20]\n\
         dof = [{ name = \"screen_z\", kind = \"slide\", axis = \"z\", range = [-10, 10], value = 5 }]\n\n\
         [[merit]]\nkind = \"focus\"\ndetector = \"screen\"\n",
    );
    let graph = run(
        json!({ "path": path.to_string_lossy(), "of": "optimize", "svg": out("optimize.svg") }),
    );
    let history = points(&series(&graph)[0]);
    assert!(history.len() >= 2, "{history:?}");
    for pair in history.windows(2) {
        assert!(
            pair[1].1 < pair[0].1,
            "the merit falls at every accepted step: {history:?}"
        );
    }
    assert!(history[history.len() - 1].1 < 1e-6 * history[0].1);
}

#[test]
fn the_wavefront_error_at_a_mismatched_focus_agrees_with_its_strehl_ratio() {
    // A dry NA 0.4 objective focusing 0.2 mm into water: a little spherical aberration. For
    // small aberrations the Strehl ratio is about exp(−(2π σ)²), σ the RMS wavefront
    // error in waves referred to the best focus (Maréchal; Born and Wolf, §9.3). A
    // matched focus has no wavefront error.
    let matched = layout("flat", "", "");
    let flat =
        run(json!({ "path": matched.to_string_lossy(), "of": "psf-wavefront", "samples": 64 }));
    assert_eq!(flat["graph"]["map"]["colours"], "diverging");
    let values: Vec<f64> = flat["graph"]["map"]["values"]
        .as_array()
        .expect("values")
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    assert!(values.len() > 1000);
    assert!(values.iter().all(|v| v.abs() < 1e-6), "a perfect focus");

    let path = write(
        "water",
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 8.2\nrings = 4\nat = 0\n\n\
         [[element]]\nid = \"OBJ\"\nkind = \"objective\"\nat = 50\nfocal_length = 10.0\nna = 0.4\npupil_distance = 30.0\nworking_distance = 5.0\n\n\
         [[element]]\nid = \"W\"\nkind = \"sample\"\nat = 84.8\nlayers = [ { thickness = 5.0, material = 1.333 } ]\n\n\
         [[detector]]\nid = \"focus\"\nat = 85.0\nsize = [1, 1]\npixels = [10, 10]\n",
    );
    let report = run(json!({
        "path": path.to_string_lossy(), "of": "psf-wavefront", "samples": 128,
        "svg": out("psf-wavefront.svg"),
    }));
    let notes: Vec<&str> = report["graph"]["notes"]
        .as_array()
        .expect("notes")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let number_after = |text: &str, key: &str| -> f64 {
        let rest = &text[text.find(key).expect(key) + key.len()..];
        rest.split_whitespace()
            .next()
            .and_then(|w| w.trim_end_matches(',').parse().ok())
            .expect("number")
    };
    let sigma = number_after(notes[0], "RMS ");
    let strehl = number_after(notes[1], "Strehl ratio ");
    let marechal = (-(2.0 * core::f64::consts::PI * sigma).powi(2)).exp();
    assert!(
        sigma > 0.02 && sigma < 0.1,
        "a small aberration: σ = {sigma}"
    );
    assert!(
        (strehl - marechal).abs() < 0.005,
        "Strehl {strehl}, Maréchal {marechal} (σ = {sigma})"
    );
}
