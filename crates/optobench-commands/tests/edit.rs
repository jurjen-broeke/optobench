//! M8c: the `edit` command, as the interface's editors use it: edits are written only
//! when the design still reads; a layout can be built from nothing; the optimiser's
//! suggestions can be written back as DOF values.

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("optobench-edit-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("scratch");
    path
}

fn empty() -> String {
    scratch("empty").to_string_lossy().into_owned()
}

fn run(command: &str, parameters: Value) -> Result<Value, String> {
    Registry::standard()
        .run(command, &parameters)
        .map_err(|e| e.message)
}

fn edit(path: &Path, operations: Value) -> Result<Value, String> {
    run(
        "edit",
        json!({ "path": path.to_string_lossy(), "operations": operations, "data_directory": empty() }),
    )
}

fn trace(path: &Path) -> Value {
    run(
        "trace",
        json!({ "path": path.to_string_lossy(), "data_directory": empty() }),
    )
    .expect("traces")
}

fn detected(report: &Value) -> f64 {
    report["ledger"]["detected_w"].as_f64().expect("detected")
}

fn copy_example(name: &str, dir: &Path) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    let to = dir.join(name);
    std::fs::copy(from, &to).expect("copied");
    to
}

#[test]
fn an_edit_is_written_and_undone() {
    let dir = scratch("periscope");
    let path = copy_example("table-periscope.optobench", &dir);
    let original = std::fs::read_to_string(&path).expect("read");
    assert!(detected(&trace(&path)) > 0.0);

    // Move the screen off the beam: nothing reaches it.
    let out = edit(
        &path,
        json!([{ "op": "set", "table": "detector", "id": "screen", "key": "height", "value": 300.0 }]),
    )
    .expect("edits");
    assert_eq!(out["changed"], true);
    assert_eq!(out["previous"], original.as_str());
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains("height = 300.0"));
    // The comment at the top is still there.
    assert!(text.contains("# A periscope on a 25 mm breadboard"));
    assert_eq!(detected(&trace(&path)), 0.0);

    // Undo: the previous text back.
    edit(&path, json!([{ "op": "replace", "text": original }])).expect("undoes");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), original);
}

#[test]
fn an_edit_that_breaks_the_design_is_not_written() {
    let dir = scratch("broken");
    let path = copy_example("table-periscope.optobench", &dir);
    let original = std::fs::read_to_string(&path).expect("read");
    for operations in [
        json!([{ "op": "set", "table": "element", "id": "M1", "key": "kind", "value": "banana" }]),
        json!([{ "op": "set", "table": "element", "id": "M1", "key": "heigth", "value": 1.0 }]),
        json!([{ "op": "set", "table": "detector", "id": "screen", "key": "size", "value": "big" }]),
    ] {
        let e = edit(&path, operations.clone())
            .map(|_| format!("written: {operations}"))
            .expect_err("refused");
        assert!(
            e.starts_with("the edit was not written"),
            "{operations}: {e}"
        );
        assert_eq!(std::fs::read_to_string(&path).expect("read"), original);
    }
    // A dry run shows the text and writes nothing.
    let dry = run(
        "edit",
        json!({ "path": path.to_string_lossy(), "dry_run": true, "data_directory": empty(),
                "operations": [{ "op": "set", "table": "element", "id": "M2", "key": "height", "value": 151.0 }] }),
    )
    .expect("a dry run");
    assert!(
        dry["text"]
            .as_str()
            .expect("text")
            .contains("height = 151.0")
    );
    assert_eq!(std::fs::read_to_string(&path).expect("read"), original);
}

#[test]
fn a_layout_is_built_from_nothing_and_traced() {
    // A planar layout: a collimated beam, a 90° fold mirror 50 mm along it, and a screen
    // 50 mm after the fold. All of the light reaches the screen (an ideal mirror).
    let dir = scratch("new");
    let path = dir.join("fold.optobench");
    let _ = std::fs::remove_file(&path);
    let created = run(
        "edit",
        json!({ "path": path.to_string_lossy(), "create": { "layout": "planar" }, "data_directory": empty() }),
    )
    .expect("created");
    assert_eq!(created["previous"], Value::Null);
    assert!(
        run(
            "edit",
            json!({ "path": path.to_string_lossy(), "create": { "layout": "planar" } })
        )
        .expect_err("exists")
        .contains("exists already")
    );
    edit(
        &path,
        json!([
            { "op": "add", "table": "source", "entry": [["id", "beam"], ["kind", "collimated"], ["wavelength_um", 0.55], ["power_w", 0.001], ["diameter", 4.0], ["at", 0.0]] },
            { "op": "add", "table": "element", "entry": [["id", "M1"], ["kind", "mirror"], ["at", 50.0], ["fold", 90.0], ["diameter", 25.4], ["clear_aperture", 22.9]] },
            { "op": "add", "table": "detector", "entry": [["id", "screen"], ["at", 100.0], ["size", [10, 10]], ["pixels", [20, 20]]] },
            { "op": "add", "table": "merit", "entry": [["kind", "centring"], ["detector", "screen"]] },
        ]),
    )
    .expect("built");
    let report = trace(&path);
    let launched = report["ledger"]["launched_w"].as_f64().expect("launched");
    assert!(
        (detected(&report) - launched).abs() < 1e-12 * launched,
        "{report}"
    );
}

#[test]
fn the_optimisers_suggestions_are_written_back_as_dof_values() {
    // The misaligned golden periscope (M6): optimise, write each suggested value into its
    // DOF, and the sidecar now describes the realigned assembly: tracing it gives the
    // optimiser's end merit.
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../optobench-io/tests/step");
    let dir = scratch("realign");
    for name in [
        "periscope-misaligned.optobench",
        "periscope-misaligned.step",
    ] {
        std::fs::copy(fixtures.join(name), dir.join(name)).expect("copied");
    }
    let path = dir.join("periscope-misaligned.optobench");
    let optimised = run(
        "optimize",
        json!({ "path": path.to_string_lossy(), "data_directory": empty() }),
    )
    .expect("optimises");
    let operations: Vec<Value> = optimised["dofs"]
        .as_array()
        .expect("dofs")
        .iter()
        .map(|d| json!({ "op": "set", "dof": d["name"], "key": "value", "value": d["suggested"] }))
        .collect();
    assert_eq!(operations.len(), 5);
    edit(&path, Value::Array(operations)).expect("written back");
    let text = std::fs::read_to_string(&path).expect("read");
    assert_eq!(text.matches("value = ").count(), 5, "{text}");
    // Optimising again starts where the first run ended.
    let again = run(
        "optimize",
        json!({ "path": path.to_string_lossy(), "data_directory": empty() }),
    )
    .expect("optimises");
    let end = optimised["cost"]["end"].as_f64().expect("cost");
    let start = again["cost"]["start"].as_f64().expect("cost");
    assert!(
        (start - end).abs() <= 1e-6 * (1.0 + end),
        "{start} vs {end}"
    );
}
