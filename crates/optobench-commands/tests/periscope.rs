//! M6 exit criterion, on the golden periscope (crates/optobench-io/tests/step,
//! periscope*.json → .step, made by SolidCraft 0.30.1): `optimize` on the misaligned
//! assembly writes a move report; the report's moves, made in SolidCraft and exported
//! again (periscope-realigned.step: the misaligned script plus the report's
//! `occurrence.move` commands), restore alignment in one iteration.

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../optobench-io/tests/step")
}

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "optobench-periscope-{label}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("scratch");
    path
}

fn run(command: &str, mut parameters: Value) -> Value {
    parameters["data_directory"] = json!(scratch("empty").to_string_lossy());
    Registry::standard()
        .run(command, &parameters)
        .unwrap_or_else(|e| panic!("{command}: {e:?}"))
}

fn number(v: &Value) -> f64 {
    v.as_f64().expect("a number")
}

fn suggested(result: &Value, name: &str) -> f64 {
    let dof = result["dofs"]
        .as_array()
        .and_then(|l| l.iter().find(|d| d["name"] == name))
        .expect("the DOF");
    number(&dof["suggested"])
}

/// Optimises the misaligned periscope, writing its report into a scratch directory.
fn report(label: &str) -> (Value, PathBuf) {
    let stem = scratch(label).join("periscope-misaligned");
    let result = run(
        "optimize",
        json!({
            "path": fixtures().join("periscope-misaligned.optobench").to_string_lossy(),
            "report": stem.to_string_lossy(),
        }),
    );
    let json_path = PathBuf::from(result["report"]["json"].as_str().expect("written"));
    (result, json_path)
}

#[test]
fn the_report_undoes_the_misalignment() {
    // periscope-misaligned.json tilts M1 by +0.4° about y and M2 by −0.3° about
    // (1, 0, 1)/√2, and raises L2 by 2 mm. Centring and pointing at the screen fix the
    // four mirror tilts uniquely; collimation fixes L2.
    let (result, json_path) = report("undo");
    assert_eq!(result["converged"], true);
    assert!((suggested(&result, "M1_y") + 0.4).abs() < 1e-4);
    assert!((suggested(&result, "M2_d") - 0.3).abs() < 1e-4);
    assert!(suggested(&result, "M1_d").abs() < 1e-4 && suggested(&result, "M2_y").abs() < 1e-4);
    // L2 back to within a few µm: collimation is flat near its best.
    assert!((suggested(&result, "L2_z") + 2.0).abs() < 0.01);
    let report: Value =
        serde_json::from_str(&std::fs::read_to_string(&json_path).expect("report")).expect("JSON");
    assert_eq!(report["kind"], "moves");
    let moves = report["moves"].as_array().expect("moves");
    assert_eq!(moves.len(), 3);
    assert!(
        moves
            .iter()
            .all(|m| m["solidcraft"]["command"] == "occurrence.move")
    );
    let markdown = std::fs::read_to_string(json_path.with_extension("").with_extension("moves.md"))
        .expect("markdown");
    assert!(markdown.contains("| M1_y | deg |") && markdown.contains("occurrence `mirror:2`"));
}

#[test]
fn the_moves_made_in_cad_restore_alignment_in_one_iteration() {
    let (_, json_path) = report("restore");
    let check = run(
        "check-moves",
        json!({
            "path": fixtures().join("periscope-realigned.optobench").to_string_lossy(),
            "report": json_path.to_string_lossy(),
        }),
    );
    assert_eq!(check["ok"], true, "{check}");
    assert_eq!(check["poses"].as_array().map(Vec::len), Some(4));
    // Aligned: the beam on the screen's centre, pointing along its normal, and as well
    // collimated as the nominal design.
    let nominal = run(
        "trace",
        json!({ "path": fixtures().join("periscope.optobench").to_string_lossy() }),
    );
    let merit = |kind: &str| {
        check["merit"]
            .as_array()
            .and_then(|l| l.iter().find(|m| m["kind"] == kind))
            .map(|m| m["now"].clone())
            .expect("term")
    };
    let centring = merit("centring");
    assert!(
        number(&centring[0]).hypot(number(&centring[1])) < 1e-6,
        "{centring}"
    );
    let pointing = merit("pointing");
    assert!(
        number(&pointing[0]).hypot(number(&pointing[1])) < 1e-5,
        "{pointing}"
    );
    let collimation = number(&merit("collimation")[0]);
    let best = number(&nominal["detectors"][0]["direct_rms_slope_mrad"]);
    assert!((collimation - best).abs() < 1e-6, "{collimation} vs {best}");
}

#[test]
fn parts_not_moved_as_reported_are_flagged() {
    // The misaligned export itself, nothing moved: the three parts the report moves are
    // off, L1 is where it should be.
    let (_, json_path) = report("flag");
    let check = run(
        "check-moves",
        json!({
            "path": fixtures().join("periscope-misaligned.optobench").to_string_lossy(),
            "report": json_path.to_string_lossy(),
        }),
    );
    assert_eq!(check["ok"], false);
    let flagged: Vec<&str> = check["poses"]
        .as_array()
        .expect("poses")
        .iter()
        .filter(|p| p["ok"] == false)
        .filter_map(|p| p["solid"].as_str())
        .collect();
    assert_eq!(flagged, vec!["L2", "M1", "M2"]);
}

#[test]
fn a_layout_report_says_which_values_to_set() {
    let dir = scratch("layout");
    let layout = dir.join("fold.optobench");
    std::fs::write(
        &layout,
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 1.0\nat = 0\n\n\
         [[element]]\nid = \"M1\"\nkind = \"mirror\"\nat = 50\nfold = 90\ndiameter = 25\nclear_aperture = 22\n\
         dof = [{ name = \"M1_tilt\", kind = \"tilt\", axis = [0, 0, 1], frame = \"scene\", range = [-2, 2], value = 0.2 }]\n\n\
         [[detector]]\nid = \"screen\"\nat = 150\nsize = [20, 20]\npixels = [20, 20]\n\n\
         [[merit]]\nkind = \"centring\"\ndetector = \"screen\"\n",
    )
    .expect("written");
    let result = run(
        "optimize",
        json!({ "path": layout.to_string_lossy(), "report": true }),
    );
    let markdown = std::fs::read_to_string(dir.join("fold.moves.md")).expect("written next to it");
    assert!(markdown.contains("Set each DOF's `value`"), "{markdown}");
    assert!(!markdown.contains("occurrence.move"));
    assert!(suggested(&result, "M1_tilt").abs() < 1e-6);
}
