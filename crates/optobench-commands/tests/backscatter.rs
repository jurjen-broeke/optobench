//! Light coming back into a laser (M17e): a returning detector at the laser, facing along
//! its beam, weighs what the faces of an uncoated window reflect back; the `backscatter`
//! merit term tilts the window until that light passes the laser by.
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

#[test]
fn backscatter_needs_a_returning_detector() {
    let dir = scratch("refused");
    let path = dir.join("window.optobench");
    std::fs::write(
        &path,
        layout(0.2).replace(
            "kind = \"backscatter\"\ndetector = \"back\"",
            "kind = \"backscatter\"\ndetector = \"screen\"",
        ),
    )
    .expect("written");
    let error = Registry::standard()
        .run("optimize", &json!({ "path": path.to_string_lossy() }))
        .expect_err("refused");
    assert!(
        error.message.contains("returning detector"),
        "{}",
        error.message
    );
}
