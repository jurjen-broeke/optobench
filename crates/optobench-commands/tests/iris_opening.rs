//! An iris's opening as a degree of freedom (`kind = "opening"`), its soft rim (`edge`),
//! and the optimiser closing an iris until what comes back is cut and the beam is not.
//!
//! Reference values from geometry: an iris of diameter d passes (d/D)² of an evenly
//! filled collimated beam of diameter D. The beams here are sampled by N rings of rays
//! (`rings`), ring k at radius (k/N) D/2 standing for the annulus from (k − ½)/N to
//! (k + ½)/N of D/2 (`optobench_scene::hexapolar_weighted`), so a sharp hole passes whole
//! annuli: the share is (d/D)² to within the area of the annulus its edge falls in. Every trace keeps the ledger's
//! power conservation (docs/design.md, Tests and validation).

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "optobench-iris-opening-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch");
    path
}

fn run(command: &str, path: &Path, extra: Value) -> Result<Value, String> {
    let mut parameters = extra;
    parameters["path"] = json!(path.to_string_lossy());
    Registry::standard()
        .run(command, &parameters)
        .map_err(|e| e.message)
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("a number, not {v}"))
}

fn detector<'a>(result: &'a Value, id: &str) -> &'a Value {
    result["detectors"]
        .as_array()
        .and_then(|d| d.iter().find(|d| d["id"] == id))
        .unwrap_or_else(|| panic!("no detector `{id}`"))
}

fn balanced(result: &Value) {
    let ledger = &result["ledger"];
    assert!(
        number(&ledger["imbalance_w"]).abs() < 1e-12 * number(&ledger["launched_w"]),
        "{ledger}"
    );
}

/// A 1 W collimated beam 4 mm across (`rings` rings of rays), an iris at 20 mm with a
/// 2 mm hole and a soft rim `edge` wide, its opening free from −1.9 to +8 mm, and a
/// screen at 100 mm.
fn iris(dir: &Path, rings: usize, edge: f64) -> PathBuf {
    let path = dir.join(format!("iris-{rings}-{edge}.optobench"));
    std::fs::write(
        &path,
        format!(
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
             [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\n\
             diameter = 4.0\nat = 0\nrings = {rings}\n\n\
             [[proxy]]\nid = \"iris\"\nkind = \"plate\"\nat = 20\nhole_diameter = 2\nsize = 30\nedge = {edge}\n\
             dof = [ {{ name = \"iris_opening\", kind = \"opening\", range = [-1.9, 8] }} ]\n\n\
             [[detector]]\nid = \"screen\"\nat = 100\nsize = [20, 20]\npixels = [8, 8]\n"
        ),
    )
    .expect("written");
    path
}

/// The power on the screen with the iris opened by `opening` (mm), and the power the
/// trace books as clipped by the iris.
fn passed(path: &Path, opening: f64) -> (f64, f64) {
    let result = run(
        "trace",
        path,
        json!({ "dofs": { "iris_opening": opening } }),
    )
    .expect("traces");
    balanced(&result);
    let clipped = result["clipping"]
        .as_array()
        .and_then(|c| c.iter().find(|c| c["id"] == "iris"))
        .map_or(0.0, |c| number(&c["power_w"]));
    (number(&detector(&result, "screen")["power_w"]), clipped)
}

#[test]
fn an_iris_opening_sets_the_share_of_the_beam_it_passes() {
    // 40 rings: the edge of a 2 mm or 3 mm hole falls in an annulus 0.05 mm wide (in
    // radius), so the share is within 0.02 of (d/D)².
    let dir = scratch("share");
    let path = iris(&dir, 40, 0.0);
    for (opening, d) in [(0.0, 2.0), (1.0, 3.0)] {
        let (power, clipped) = passed(&path, opening);
        let theory = (d / 4.0_f64).powi(2);
        assert!((power - theory).abs() < 0.02, "{d} mm of 4: {power}");
        assert!((power + clipped - 1.0).abs() < 1e-12, "{power} + {clipped}");
    }
    // 5 mm passes all of the 4 mm beam.
    assert!((passed(&path, 3.0).0 - 1.0).abs() < 1e-12);
    // Beyond its plate, or an opening on a tube, is refused.
    let error = run("trace", &path, json!({ "dofs": { "iris_opening": 30.0 } }))
        .expect_err("outside its range");
    assert!(error.contains("range"), "{error}");
    let tube = dir.join("tube.optobench");
    std::fs::write(
        &tube,
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[proxy]]\nid = \"tube\"\nkind = \"tube\"\ninner_diameter = 10\nfrom = 10\nto = 20\n\
         dof = [ { name = \"tube_opening\", kind = \"opening\" } ]\n",
    )
    .expect("written");
    let error = run("trace", &tube, json!({})).expect_err("a tube has no opening");
    assert!(error.contains("iris"), "{error}");
}

#[test]
fn a_soft_rim_passes_what_a_sharp_edge_does_but_changes_smoothly() {
    // Few rings (5): a sharp hole passes whole annuli, so between two rings of rays the
    // power holds still as the hole opens, and the optimiser finds no slope. A rim 0.4 mm
    // wide (as wide as the annuli) weighs each ray by where it falls across the rim:
    // the power rises at every step, and over the opening it averages what the sharp
    // hole passes (first order in the rim's width).
    let dir = scratch("soft");
    let (sharp, soft) = (iris(&dir, 5, 0.0), iris(&dir, 5, 0.4));
    let openings: Vec<f64> = (0..=20).map(|k| 0.05 * k as f64).collect();
    let sharp_powers: Vec<f64> = openings.iter().map(|&o| passed(&sharp, o).0).collect();
    let soft_powers: Vec<f64> = openings.iter().map(|&o| passed(&soft, o).0).collect();
    assert!(
        sharp_powers.windows(2).any(|w| (w[1] - w[0]).abs() < 1e-15),
        "a sharp hole holds still between rings: {sharp_powers:?}"
    );
    assert!(
        soft_powers.windows(2).all(|w| w[1] > w[0]),
        "a soft rim rises at every step: {soft_powers:?}"
    );
    // Averaged over the 1 mm of opening, the two agree to the annuli's granularity.
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let (a, b) = (mean(&sharp_powers), mean(&soft_powers));
    assert!((a - b).abs() < 0.03, "{a} against {b}");
    // A rim's width must not be negative.
    let text = std::fs::read_to_string(&soft)
        .expect("read")
        .replace("edge = 0.4", "edge = -1");
    std::fs::write(&soft, text).expect("written");
    let error = run("trace", &soft, json!({})).expect_err("negative edge");
    assert!(error.contains("edge"), "{error}");
}

#[test]
fn the_optimiser_closes_an_iris_on_the_beam_to_cut_what_comes_back() {
    // A 4 mm beam (20 rings), a lens 100 mm on whose convex first face (R 20 mm,
    // uncoated) sends back a reflection diverging from a point 10 mm behind it (a convex
    // mirror's focus, R/2), 44 mm across when it is back at the iris. A returning
    // detector 30 mm square in the beam just after the laser weighs it; an iris between
    // them, 12 mm open, with a 0.5 mm rim. Merit: backscatter there, and power on the
    // screen (weight 10). Closing the iris cuts the returning light in proportion to
    // the area it leaves open, until the rim reaches the beam: the beam's outer ring of
    // rays lies on its edge, 2 mm out, so the hole stops at 4 + 0.5 mm, the rim's inside
    // on that ring, with the whole beam still passing (to the optimiser's tolerance).
    let dir = scratch("optimise");
    let path = dir.join("lens.optobench");
    std::fs::write(
        &path,
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [trace]\nsplit_cutoff = 1e-12\n\n\
         [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\n\
         diameter = 4.0\nat = 0\nrings = 20\n\n\
         [[detector]]\nid = \"back\"\nat = 10\nsize = [30, 30]\npixels = [8, 8]\nreturning = true\n\n\
         [[proxy]]\nid = \"iris\"\nkind = \"plate\"\nat = 20\nhole_diameter = 12\nsize = 30\nedge = 0.5\n\
         dof = [ { name = \"iris_opening\", kind = \"opening\", range = [-11, 8] } ]\n\n\
         [[element]]\nid = \"L\"\nkind = \"lens\"\nat = 120\n\
         surfaces = [ { radius = 20 }, { radius = \"inf\" } ]\nthickness = [4]\nmaterial = [1.5]\n\
         diameter = 12.7\nclear_aperture = 11.4\n\n\
         [[detector]]\nid = \"screen\"\nat = 200\nsize = [40, 40]\npixels = [8, 8]\n\n\
         [[merit]]\nkind = \"backscatter\"\ndetector = \"back\"\n\n\
         [[merit]]\nkind = \"power\"\ndetector = \"screen\"\nweight = 10\n",
    )
    .expect("written");
    let result = run("optimize", &path, json!({})).expect("optimises");
    let hole = 12.0 + number(&result["dofs"][0]["suggested"]);
    assert!((hole - 4.5).abs() < 5e-3, "{hole} mm");
    let term = |kind: &str| {
        result["merit"]
            .as_array()
            .and_then(|m| m.iter().find(|t| t["kind"] == kind))
            .unwrap_or_else(|| panic!("{kind}"))
            .clone()
    };
    let back = term("backscatter");
    let (start, end) = (number(&back["start"][0]), number(&back["end"][0]));
    assert!(end < 0.25 * start, "{start} % to {end} %");
    // The beam lost nothing to the iris: the power term ends where it started.
    let power = term("power");
    assert!(
        (number(&power["end"][0]) - number(&power["start"][0])).abs() < 1e-9,
        "{power}"
    );
}
