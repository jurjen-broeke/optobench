//! M9a: the ideal objective and index mismatch. Reference values from Snell's law across
//! flat layers with the sine-condition invariant p = n sin θ = h / f (Born and Wolf,
//! Principles of Optics, 7th ed., §4.5.1; the mismatch picture of Gibson and Lanni,
//! J. Opt. Soc. Am. A 8, 1601 (1991)): a layer of thickness t crossed with
//! tan θ = p / √(n² − p²) moves a ray transversely by t tan θ, and a transverse
//! difference Δr made up in the final medium moves its axial crossing by Δr / tan θ.
//! Indices are constants, so the values are exact.

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "optobench-objective-{label}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("scratch");
    path
}

/// A planar layout: a collimated beam filling `fill` of the pupil of an objective
/// (f = 10 mm, pupil plane at 50, front plane at 80), then `rest`.
fn layout(label: &str, na: f64, fill: f64, objective: &str, rest: &str) -> PathBuf {
    let dir = scratch(label);
    let path = dir.join(format!("{label}.optobench"));
    let text = format!(
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\n\
         diameter = {diameter}\nrings = {rings}\nat = 0\n\n\
         [[element]]\nid = \"OBJ\"\nkind = \"objective\"\nat = 50\nfocal_length = 10.0\nna = {na}\n\
         pupil_distance = 30.0\n{objective}\n\n{rest}",
        diameter = 2.0 * 10.0 * na * fill,
        // Finer rings for the oil case, to sample close to the water's cut-off.
        rings = if label == "oil" { 32 } else { 8 },
    );
    std::fs::write(&path, text).expect("written");
    path
}

fn focus(path: &Path) -> Value {
    Registry::standard()
        .run(
            "focus",
            &json!({ "path": path.to_string_lossy(), "data_directory": scratch("empty").to_string_lossy() }),
        )
        .unwrap_or_else(|e| panic!("{}", e.message))
}

fn tan(p: f64, n: f64) -> f64 {
    p / (n * n - p * p).sqrt()
}

/// Every crossing against the expected axial position for its invariant p = h / f.
fn check_crossings(report: &Value, expected: impl Fn(f64) -> f64, tolerance: f64) {
    let crossings = report["crossings"].as_array().expect("crossings");
    assert!(crossings.len() > 100, "{} rays", crossings.len());
    for c in crossings {
        let h = c["height_mm"].as_f64().expect("height");
        if h < 1e-9 {
            continue;
        }
        let z = c["z_mm"].as_f64().expect("z");
        let want = expected(h / 10.0);
        assert!(
            (z - want).abs() < tolerance,
            "h = {h}: z = {z}, expected {want}"
        );
        assert!(c["miss_mm"].as_f64().expect("miss") < 1e-9, "meridional");
    }
}

#[test]
fn a_matched_water_immersion_objective_focuses_perfectly_through_its_cover_glass() {
    // NA 1.0 water immersion (n = 1.333), 2 mm working distance, a 0.17 mm cover glass
    // (n = 1.5255): the screen 0.05 mm past the design focus, which is just under the
    // cover glass (a screen on the glass–water face itself would meet the rays there).
    let path = layout(
        "matched",
        1.0,
        0.995,
        "working_distance = 2.0\nimmersion = 1.333\ncover_glass = { thickness = 0.17, material = 1.5255 }",
        "[[element]]\nid = \"S\"\nkind = \"sample\"\nat = 82.0\nabove = 1.333\n\
         layers = [ { thickness = 0.17, material = 1.5255 }, { thickness = 5.0, material = 1.333 } ]\n\n\
         [[detector]]\nid = \"focus\"\nat = 82.22\nsize = [1, 1]\npixels = [10, 10]\n",
    );
    let report = focus(&path);
    check_crossings(&report, |_| -0.05, 1e-9);
    assert!(
        (report["best_mm"].as_f64().expect("best") + 0.05).abs() < 1e-9,
        "{report}"
    );
    assert!(
        report["best_rms_mm"].as_f64().expect("rms") < 1e-9,
        "{report}"
    );
}

#[test]
fn a_dry_objective_focusing_into_water_shifts_its_focus_deeper() {
    // A dry NA 0.5 objective (designed for air, no cover glass, 3 mm working distance)
    // with water 0.5 mm above its design focus: a ray of invariant p reaches the axis at
    // depth d tan θ₁ / tan θ₂ below the surface instead of d (θ₁ in air, θ₂ in water),
    // and the paraxial focus at d n₂ / n₁. The screen is at the design focus.
    let (d, n_water) = (0.5, 1.333);
    let surface = 80.0 + 3.0 - d;
    let path = layout(
        "dry",
        0.5,
        0.998,
        "working_distance = 3.0",
        &format!(
            "[[element]]\nid = \"W\"\nkind = \"sample\"\nat = {surface}\nlayers = [ {{ thickness = 10.0, material = {n_water} }} ]\n\n\
             [[detector]]\nid = \"focus\"\nat = 83.0\nsize = [1, 1]\npixels = [10, 10]\n"
        ),
    );
    let report = focus(&path);
    check_crossings(&report, |p| d * tan(p, 1.0) / tan(p, n_water) - d, 1e-9);
    let paraxial = report["paraxial_mm"].as_f64().expect("paraxial");
    let expected = d * (n_water / 1.0 - 1.0);
    assert!(
        (paraxial - expected).abs() < 1e-4 * d,
        "{paraxial} vs {expected}"
    );
    // The marginal rays focus deeper still: positive spherical aberration.
    assert!(report["marginal_mm"].as_f64().expect("marginal") > paraxial);
}

#[test]
fn an_oil_objective_into_water_passes_no_ray_beyond_the_waters_index() {
    // NA 1.4 oil immersion (n = 1.518), through its cover glass into water: rays with
    // n sin θ > 1.333 are totally reflected at the glass–water face, so in the water
    // the steepest ray has 1.333 sin θ ≤ 1.333, and none of them reaches the screen.
    let path = layout(
        "oil",
        1.4,
        0.995,
        "working_distance = 0.15\nimmersion = 1.518\ncover_glass = { thickness = 0.17, material = 1.5255 }",
        "[[element]]\nid = \"S\"\nkind = \"sample\"\nat = 80.15\nabove = 1.518\n\
         layers = [ { thickness = 0.17, material = 1.5255 }, { thickness = 5.0, material = 1.333 } ]\n\n\
         [[detector]]\nid = \"focus\"\nat = 80.37\nsize = [2, 2]\npixels = [10, 10]\n",
    );
    let report = focus(&path);
    let heights: Vec<f64> = report["crossings"]
        .as_array()
        .expect("crossings")
        .iter()
        .filter_map(|c| c["height_mm"].as_f64())
        .collect();
    let largest = heights.iter().copied().fold(0.0, f64::max);
    assert!(largest / 10.0 <= 1.333 + 1e-12, "p = {}", largest / 10.0);
    // The beam filled the pupil out to p = 1.393 in 32 rings (0.044 apart); the rays
    // that arrive reach within one ring of the cut-off, so the cut is the water's.
    assert!(largest / 10.0 > 1.30, "p = {}", largest / 10.0);
    let steepest = report["steepest_deg"].as_f64().expect("angle").to_radians();
    assert!(1.333 * steepest.sin() <= 1.333 + 1e-9);
}

#[test]
fn a_matched_water_dipping_objective_focuses_perfectly_without_a_cover_glass() {
    // NA 0.8 water dipping, 2.5 mm working distance: the front lens in water.
    let path = layout(
        "dipping",
        0.8,
        0.995,
        "working_distance = 2.5\nimmersion = 1.333",
        "[[element]]\nid = \"W\"\nkind = \"sample\"\nat = 80.5\nabove = 1.333\n\
         layers = [ { thickness = 10.0, material = 1.333 } ]\n\n\
         [[detector]]\nid = \"focus\"\nat = 82.5\nsize = [1, 1]\npixels = [10, 10]\n",
    );
    let report = focus(&path);
    check_crossings(&report, |_| 0.0, 1e-9);
    assert!(report["best_rms_mm"].as_f64().expect("rms") < 1e-9);
}

#[test]
fn a_well_plate_bottom_in_place_of_the_cover_glass_moves_each_ray_as_the_slab_formula_says() {
    // A water objective designed for 0.17 mm of glass (n = 1.5255), imaging through a
    // 0.17 mm polymer well-plate bottom (n = 1.59): each ray's axial crossing moves by
    // t (tan θ_design − tan θ_actual) / tan θ_water.
    let (t, glass, plate, water) = (0.17, 1.5255, 1.59, 1.333);
    let path = layout(
        "plate",
        1.0,
        0.995,
        &format!(
            "working_distance = 2.0\nimmersion = {water}\ncover_glass = {{ thickness = {t}, material = {glass} }}"
        ),
        &format!(
            "[[element]]\nid = \"S\"\nkind = \"sample\"\nat = 82.0\nabove = {water}\n\
             layers = [ {{ thickness = {t}, material = {plate} }}, {{ thickness = 5.0, material = {water} }} ]\n\n\
             [[detector]]\nid = \"focus\"\nat = 82.22\nsize = [1, 1]\npixels = [10, 10]\n"
        ),
    );
    let report = focus(&path);
    // The screen is 0.05 mm past the design focus.
    check_crossings(
        &report,
        |p| t * (tan(p, glass) - tan(p, plate)) / tan(p, water) - 0.05,
        1e-9,
    );
}
