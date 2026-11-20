//! M12 exit criteria, against a stand-in for SolidCraft (`optobench_bridge::fake`; CI has
//! no SolidCraft): the golden cage relay's sidecar traced against the design "open in
//! SolidCraft", again within 2 s of a change, the same as tracing the STEP file the
//! design changed to; a SolidCraft whose contract changed is refused before anything is
//! read; a layout is refused (it has no CAD design).

use std::path::{Path, PathBuf};
use std::time::Duration;

use optobench_bridge::fake::FakeApp;
use optobench_bridge::live::Live;
use optobench_commands::Registry;
use serde_json::{Map, Value, json};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../optobench-io/tests/step")
}

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "optobench-live-test-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn parameters(control: &Path, empty: &Path) -> Map<String, Value> {
    json!({
        "path": fixtures().join("cage-relay.optobench").to_string_lossy(),
        "control_directory": control.to_string_lossy(),
        "data_directory": empty.to_string_lossy(),
    })
    .as_object()
    .expect("an object")
    .clone()
}

/// The detectors of a trace, which is what a change moves.
fn detectors(trace: &Value) -> Value {
    trace["detectors"].clone()
}

fn plain_trace(step: &str, empty: &Path) -> Value {
    Registry::standard()
        .run(
            "trace",
            &json!({
                "path": fixtures().join("cage-relay.optobench").to_string_lossy(),
                "step": fixtures().join(step).to_string_lossy(),
                "data_directory": empty.to_string_lossy(),
            }),
        )
        .expect("traces")
}

#[test]
fn the_design_is_traced_again_within_two_seconds_of_a_change() {
    let app = FakeApp::start(&fixtures().join("cage-relay.step"));
    let control = scratch("control");
    let empty = scratch("data");
    app.write_control_folder(&control);
    let (mut live, first) = Live::start(&parameters(&control, &empty)).expect("started");
    // The design as it is: as the golden relay traced from its own STEP file.
    assert_eq!(first["revision"], 1);
    assert_eq!(
        detectors(&first["trace"]),
        detectors(&plain_trace("cage-relay.step", &empty))
    );
    let names: Vec<&str> = first["design"]["occurrences"]
        .as_array()
        .expect("occurrences")
        .iter()
        .filter_map(|o| o["name"].as_str())
        .collect();
    assert_eq!(names, ["cage:1", "lens:1"]);
    // Nothing changes: nothing is traced.
    assert!(
        live.next(Duration::from_millis(100))
            .expect("waited")
            .is_none()
    );
    // A lens moved in SolidCraft: traced again, as the moved relay's STEP file traces.
    let started = std::time::Instant::now();
    app.change(&fixtures().join("cage-relay-moved.step"));
    let again = live
        .next(Duration::from_secs(5))
        .expect("waited")
        .expect("traced again");
    let elapsed = started.elapsed().as_secs_f64();
    assert_eq!(again["revision"], 2);
    let moved = plain_trace("cage-relay-moved.step", &empty);
    assert_eq!(detectors(&again["trace"]), detectors(&moved));
    assert_ne!(
        detectors(&again["trace"]),
        detectors(&first["trace"]),
        "the move shows"
    );
    assert!(elapsed < 2.0, "{elapsed} s from the change to the trace");
    assert!(again["seconds"].as_f64().expect("seconds") < 2.0);
    // The user's STEP files are untouched; the session's exports go when it ends.
    drop(live);
    let _ = std::fs::remove_dir_all(&control);
    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn a_changed_contract_or_a_layout_is_refused() {
    let app = FakeApp::start(&fixtures().join("cage-relay.step"));
    app.remove_from_contract("file.export_step", "bytes");
    let control = scratch("contract");
    let empty = scratch("contract-data");
    app.write_control_folder(&control);
    let error = Live::start(&parameters(&control, &empty)).expect_err("refused");
    assert!(
        error
            .message
            .contains("`file.export_step` output has no `bytes`")
            && error.message.contains("nothing was read"),
        "{}",
        error.message
    );
    // Only the contract was asked for.
    assert!(
        app.served().iter().all(|s| s == "engine.describe"),
        "{:?}",
        app.served()
    );
    let mut layout = parameters(&control, &empty);
    let periscope = scratch("layout").join("bench.optobench");
    std::fs::write(
        &periscope,
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n",
    )
    .expect("written");
    layout.insert("path".into(), json!(periscope.to_string_lossy()));
    let error = Live::start(&layout).expect_err("refused");
    assert!(
        error.message.contains("traces a sidecar"),
        "{}",
        error.message
    );
    let _ = std::fs::remove_dir_all(&control);
    let _ = std::fs::remove_dir_all(&empty);
}
