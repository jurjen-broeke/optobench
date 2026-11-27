//! Light coming back into a laser (M17e): a returning detector at the laser, facing along
//! its beam, weighs what the faces of an uncoated window reflect back; the `backscatter`
//! merit term tilts the window until that light passes the laser by. The term on a
//! returning detector in the beam is what comes back over what goes through, and on a
//! screen the share of the launched power reaching it.
//!
//! Reference values from theory: an uncoated face between air and glass of index n
//! reflects R = ((n − 1)/(n + 1))² at normal incidence (Fresnel; E. Hecht, Optics, 5th
//! ed., §4.6), 0.04 for n = 1.5. A window returns R from its first face and, from its
//! second, T²R (1 + R² + R⁴ + …) = T²R/(1 − R²) with T = 1 − R: 0.076923 of the beam in
//! all. Every trace also keeps the ledger's power conservation (docs/design.md, Tests and
//! validation).

use std::path::PathBuf;

use optobench_commands::Registry;
use serde_json::{Value, json};

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "optobench-backscatter-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch");
    path
}

/// A 2 mm laser beam along x at hole (0, 0), an uncoated flat window (n 1.5, 5 mm) four
/// holes on, tilted `tilt` degrees about the board's Z by its DOF, a screen ten holes
/// on, and a returning detector 3 mm square at the laser. Reflections are followed down
/// to 1e-12 of the source's power per ray: with the default 1e-6 the third return of each
/// of the beam's rays (about 5e-10 W of 1 mW shared by its rays) would be booked below the
/// cutoff, and the series stop at R + T²R.
fn layout(tilt: f64) -> String {
    format!(
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"table\"\n\n[table]\npitch = 25.0\n\n\
         [[source]]\nid = \"laser\"\nkind = \"collimated\"\nwavelength_um = 0.633\npower_w = 0.001\n\
         diameter = 2.0\nhole = [0, 0]\nheight = 75.0\naxis = [1, 0, 0]\n\n\
         [[element]]\nid = \"window\"\nkind = \"lens\"\nhole = [4, 0]\nheight = 75.0\naxis = [1, 0, 0]\n\
         surfaces = [{{ radius = \"inf\" }}, {{ radius = \"inf\" }}]\nthickness = [5.0]\nmaterial = [1.5]\n\
         diameter = 25.4\nclear_aperture = 22.9\n\
         dof = [ {{ name = \"window_tilt\", kind = \"tilt\", axis = [0, 0, 1], frame = \"scene\", range = [-5, 5], value = {tilt} }} ]\n\n\
         [[detector]]\nid = \"screen\"\nhole = [10, 0]\nheight = 75.0\naxis = [1, 0, 0]\nsize = [40, 40]\npixels = [40, 40]\n\n\
         [[detector]]\nid = \"back\"\nhole = [0, 0]\nheight = 75.0\naxis = [1, 0, 0]\nsize = [3, 3]\npixels = [30, 30]\n\
         returning = true\n\n\
         [[merit]]\nkind = \"backscatter\"\ndetector = \"back\"\n\n\
         [trace]\nsplit_cutoff = 1e-12\n"
    )
}

fn run(command: &str, path: &std::path::Path) -> Value {
    Registry::standard()
        .run(command, &json!({ "path": path.to_string_lossy() }))
        .unwrap_or_else(|e| panic!("{command}: {}", e.message))
}

fn detector<'a>(traced: &'a Value, id: &str) -> &'a Value {
    traced["detectors"]
        .as_array()
        .and_then(|l| l.iter().find(|d| d["id"] == id))
        .unwrap_or_else(|| panic!("no detector {id}"))
}

fn conserved(traced: &Value) {
    let ledger = &traced["ledger"];
    let launched = ledger["launched_w"].as_f64().expect("launched");
    let accounted: f64 = [
        "detected_w",
        "clipped_w",
        "absorbed_w",
        "scattered_w",
        "reflected_w",
        "escaped_w",
        "stopped_w",
        "below_cutoff_w",
    ]
    .iter()
    .map(|k| ledger[*k].as_f64().unwrap_or_else(|| panic!("no {k}")))
    .sum();
    assert!(ledger["imbalance_w"].as_f64().expect("imbalance").abs() < 1e-9 * launched);
    assert!((accounted - launched).abs() < 1e-9 * launched, "{ledger}");
}

#[test]
fn a_window_square_to_the_beam_sends_its_reflections_back_into_the_laser() {
    let dir = scratch("square");
    let path = dir.join("window.optobench");
    std::fs::write(&path, layout(0.0)).expect("written");
    let traced = run("trace", &path);
    conserved(&traced);
    let back = detector(&traced, "back");
    assert_eq!(back["returning"], true);
    // Square to the beam, both faces' reflections come straight back over the beam.
    let (r, t) = (0.04, 0.96);
    let theory = r + t * t * r / (1.0 - r * r);
    let returned = back["power_w"].as_f64().expect("power") / 1e-3;
    assert!(
        (returned - theory).abs() < 1e-8,
        "{returned} against {theory}"
    );
    // It stops nothing: the screen still gets T² of the beam on the rays' main paths.
    let screen = detector(&traced, "screen")["direct_power_w"]
        .as_f64()
        .expect("direct")
        / 1e-3;
    assert!((screen - t * t).abs() < 1e-6, "{screen}");
    // Weighed as a Gaussian mode of 1/e² radius 1.5 mm takes it in: an even 2 mm disc
    // gives the mean of exp(−2r²/w²) over it, (1 − e^(−2a²/w²)) w²/(2a²) with a = 1 mm,
    // to within the beam's sampling by rings.
    let weighted = back["returned_w"].as_f64().expect("returned") / 1e-3;
    let (a, w) = (1.0_f64, 1.5_f64);
    let mean = (1.0 - (-2.0 * a * a / (w * w)).exp()) * w * w / (2.0 * a * a);
    assert!(
        (weighted / theory - mean).abs() < 0.005,
        "{} against {mean}",
        weighted / theory
    );
}

#[test]
fn the_optimiser_tilts_the_window_until_nothing_comes_back() {
    // From 0.2° (square to the beam the returning light is at its peak, where it has no
    // slope), backscatter tilts the window until the reflections pass the laser by: they
    // turn by twice the tilt, so at 100 mm a tilt of about 1° moves them 3.5 mm aside.
    let dir = scratch("tilt");
    let path = dir.join("window.optobench");
    std::fs::write(&path, layout(0.2)).expect("written");
    let result = run("optimize", &path);
    let term = result["merit"]
        .as_array()
        .and_then(|m| m.iter().find(|t| t["kind"] == "backscatter"))
        .expect("the term");
    assert_eq!(term["unit"], "%");
    let start = term["start"][0].as_f64().expect("start");
    let end = term["end"][0].as_f64().expect("end");
    assert!(start > 1.0, "{start} %");
    assert!(end < 1e-3, "{start} % to {end} %");
    let tilt = result["dofs"][0]["suggested"].as_f64().expect("suggested");
    assert!(tilt.abs() > 0.5 && tilt.abs() <= 5.0, "{tilt}°");
    // The window tilted so: nothing worth speaking of comes back, and the beam still
    // reaches the screen.
    std::fs::write(&path, layout(tilt)).expect("written");
    let traced = run("trace", &path);
    conserved(&traced);
    let back = detector(&traced, "back");
    assert!(
        back["returned_percent"].as_f64().expect("%") < 1e-3,
        "{back}"
    );
    let screen = detector(&traced, "screen")["direct_power_w"]
        .as_f64()
        .expect("direct");
    assert!(screen > 0.9e-3, "{screen}");
}

/// The `backscatter` term's value at the start of an optimisation of `text`.
fn backscatter_at_start(label: &str, text: String) -> Result<f64, String> {
    let dir = scratch(label);
    let path = dir.join("window.optobench");
    std::fs::write(&path, text).expect("written");
    let result = Registry::standard()
        .run("optimize", &json!({ "path": path.to_string_lossy() }))
        .map_err(|e| e.message)?;
    let term = result["merit"]
        .as_array()
        .and_then(|m| m.iter().find(|t| t["kind"] == "backscatter"))
        .expect("the term");
    Ok(term["start"][0].as_f64().expect("start"))
}

#[test]
fn backscatter_on_a_screen_is_the_share_of_the_launched_power_reaching_it() {
    // A screen behind a part catches spill: the term is 100 × its power over the
    // launched power. Here the screen behind the window gets everything the window
    // passes, T²(1 + R² + R⁴ + …) = T²/(1 − R²) = 0.923077 of the beam.
    let (r, t) = (0.04, 0.96);
    let value = backscatter_at_start(
        "screen",
        layout(0.2).replace(
            "kind = \"backscatter\"\ndetector = \"back\"",
            "kind = \"backscatter\"\ndetector = \"screen\"",
        ),
    )
    .expect("measured on the screen");
    let theory = 100.0 * t * t / (1.0 - r * r);
    assert!(
        (value - theory).abs() < 1e-6,
        "{value} % against {theory} %"
    );
}

#[test]
fn backscatter_in_the_beam_is_what_comes_back_over_what_goes_through() {
    // A returning detector in the beam between the laser and the window (two holes on),
    // 200 mm square: the whole beam goes through it once, and the window's reflections
    // come back through it. Its term is 100 × returned / through: the window's
    // R + T²R/(1 − R²) times the mean weight exp(−2r²/w²) over the 2 mm beam, w = 100 mm,
    // (1 − e^(−2a²/w²)) w²/(2a²) with a = 1 mm.
    let (r, t) = (0.04, 0.96);
    let text = layout(0.0)
        .replace(
            "id = \"back\"\nhole = [0, 0]",
            "id = \"back\"\nhole = [2, 0]",
        )
        .replace("size = [3, 3]", "size = [200, 200]");
    let dir = scratch("in-beam");
    let path = dir.join("window.optobench");
    std::fs::write(&path, &text).expect("written");
    let traced = run("trace", &path);
    conserved(&traced);
    let back = detector(&traced, "back");
    let through = back["through_w"].as_f64().expect("through") / 1e-3;
    assert!((through - 1.0).abs() < 1e-12, "{through}");
    // Unweighted, the returning light inside it is the window's to 1e-8.
    let window = r + t * t * r / (1.0 - r * r);
    let returned = back["power_w"].as_f64().expect("power") / 1e-3;
    assert!((returned - window).abs() < 1e-8, "{returned}");
    // Weighted: the mean weight over the beam is 1 − 1e-4 or so; the beam's rings of
    // rays sample its mean r² (and so that departure from 1) to 1.4 %, so the value
    // agrees to a few 1e-6 of itself.
    let (a, w) = (1.0_f64, 100.0_f64);
    let mean = (1.0 - (-2.0 * a * a / (w * w)).exp()) * w * w / (2.0 * a * a);
    let theory = 100.0 * window * mean;
    let value = back["backscatter_percent"].as_f64().expect("%");
    assert!(
        (value - theory).abs() < 3e-6 * theory,
        "{value} % against {theory} %"
    );
    let term = backscatter_at_start("in-beam-term", text).expect("measured");
    assert!((term - value).abs() < 1e-12, "{term} against {value}");
}

#[test]
fn backscatter_needs_light_going_through_a_returning_detector() {
    // Three holes aside, the beam never crosses the returning detector: nothing to
    // compare what comes back with.
    let error = backscatter_at_start(
        "aside",
        layout(0.2).replace(
            "id = \"back\"\nhole = [0, 0]",
            "id = \"back\"\nhole = [2, 3]",
        ),
    )
    .expect_err("refused");
    assert!(error.contains("goes through"), "{error}");
}
