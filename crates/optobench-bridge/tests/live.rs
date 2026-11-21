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
    assert_eq!(names[..2], ["pc-lens:1", "pc-lens:2"]);
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
        app.served()
            .iter()
            .all(|s| s == "engine.execute engine.describe"),
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

/// Numbers equal within 1e-12 absolute plus 1e-9 relative (a move applied in memory
/// against one written to a STEP file and read back); everything else equal.
fn close(a: &Value, b: &Value, at: &str) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            assert_eq!(
                x.keys().collect::<Vec<_>>(),
                y.keys().collect::<Vec<_>>(),
                "{at}"
            );
            for k in x.keys() {
                close(&x[k], &y[k], &format!("{at}.{k}"));
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            assert_eq!(x.len(), y.len(), "{at}");
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                close(p, q, &format!("{at}[{i}]"));
            }
        }
        (Value::Number(p), Value::Number(q)) => {
            let (p, q) = (p.as_f64().expect("a number"), q.as_f64().expect("a number"));
            assert!(
                (p - q).abs() <= 1e-12 + 1e-9 * p.abs().max(q.abs()),
                "{at}: {p} vs {q}"
            );
        }
        _ => assert_eq!(a, b, "{at}"),
    }
}

#[test]
fn a_move_is_traced_without_exporting_again() {
    // The fast path (M12e): L2 and its cage plate moved 1 mm along z in SolidCraft (as
    // the golden cage-relay-moved.json does) change placements only, so the first export
    // is traced with those two occurrences moved, with no export, as the moved export
    // traces. A change of geometry afterwards is exported again.
    let fixtures = fixtures();
    let app = FakeApp::start(&fixtures.join("cage-relay.step"));
    let control = scratch("fast");
    let empty = scratch("fast-data");
    app.write_control_folder(&control);
    let (mut live, first) = Live::start(&parameters(&control, &empty)).expect("started");
    assert_eq!(first["geometry"], "exported");
    let exports = |app: &FakeApp| {
        app.served()
            .iter()
            .filter(|s| *s == "engine.execute file.export_step")
            .count()
    };
    assert_eq!(exports(&app), 1);
    let moved_step = fixtures.join("cage-relay-moved.step");
    let started = std::time::Instant::now();
    app.move_occurrence("pc-lens:2", [0.0, 0.0, 1.0], &moved_step);
    app.move_occurrence("cage-plate:2", [0.0, 0.0, 1.0], &moved_step);
    let update = live
        .next(Duration::from_secs(5))
        .expect("waited")
        .expect("traced again");
    let elapsed = started.elapsed().as_secs_f64();
    assert_eq!(update["geometry"], "moved");
    assert_eq!(exports(&app), 1, "no export for a move");
    let moved: Vec<&str> = update["moves"]
        .as_array()
        .expect("moves")
        .iter()
        .filter_map(|m| m["occurrence"].as_str())
        .collect();
    assert_eq!(moved, ["pc-lens:2", "cage-plate:2"]);
    let exported = plain_trace("cage-relay-moved.step", &empty);
    close(
        &update["trace"]["detectors"],
        &exported["detectors"],
        "detectors",
    );
    close(&update["trace"]["ledger"], &exported["ledger"], "ledger");
    assert!(elapsed < 2.0, "{elapsed} s");
    // A feature added: exported again.
    app.change(&moved_step);
    let update = live
        .next(Duration::from_secs(5))
        .expect("waited")
        .expect("traced again");
    assert_eq!(update["geometry"], "exported");
    assert_eq!(exports(&app), 2);
    close(
        &update["trace"]["detectors"],
        &exported["detectors"],
        "detectors",
    );
    drop(live);
    let _ = std::fs::remove_dir_all(&control);
    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn dof_values_are_written_into_the_design_and_traced_again() {
    // M13c: the golden relay with L2 on a slide along z, at 0.5 mm in the file, and the
    // screen on a slide (not a CAD part). Writing L2_z = 2 and screen_z = 1 moves L2 2 mm
    // in "SolidCraft" (the file's 0.5 is optobench's offset on the design, which stands
    // at 0), sets L2_z back to 0 and screen_z to 1 in the sidecar; the next trace (a move:
    // no export) is the design traced with those values.
    let fixtures = fixtures();
    let app = FakeApp::start(&fixtures.join("cage-relay.step"));
    let control = scratch("write");
    let data = scratch("write-data");
    app.write_control_folder(&control);
    let original = std::fs::read_to_string(fixtures.join("cage-relay.optobench")).expect("read");
    let text = original
        .replace(
            "kind = \"sidecar\"\n",
            &format!(
                "kind = \"sidecar\"\nstep = \"{}\"\n",
                fixtures.join("cage-relay.step").to_string_lossy()
            ),
        )
        .replace(
            "body = { product = \"pc-lens\", occurrence = \"pc-lens:2\" }",
            "body = { product = \"pc-lens\", occurrence = \"pc-lens:2\" }\ndof = [ { name = \"L2_z\", kind = \"slide\", axis = [0, 0, 1], range = [-5, 5], value = 0.5 } ]",
        )
        + "dof = [ { name = \"screen_z\", kind = \"slide\", axis = [0, 0, 1], range = [-5, 5] } ]\n";
    let sidecar = data.join("relay.optobench");
    std::fs::write(&sidecar, &text).expect("written");
    let mut parameters = parameters(&control, &data);
    parameters.insert("path".into(), json!(sidecar.to_string_lossy()));
    let (mut live, _) = Live::start(&parameters).expect("started");
    let done = live
        .write(&json!({ "L2_z": 2.0, "screen_z": 1.0 }))
        .expect("written");
    assert_eq!(done["written"]["written"], 1, "{done}");
    // The design stands where L2_z is 0: it moves the whole 2 mm, the file's 0.5 being
    // only optobench's offset.
    assert!((app.world("pc-lens:2")[2][3] - 2.0).abs() < 1e-12);
    let after = std::fs::read_to_string(&sidecar).expect("read");
    let value_of = |name: &str| {
        let line = after
            .lines()
            .find(|l| l.contains(&format!("name = \"{name}\"")))
            .unwrap_or_else(|| panic!("no {name}: {after}"));
        let at = line.find("value = ").expect("a value") + "value = ".len();
        line[at..]
            .split([' ', '}'])
            .next()
            .and_then(|v| v.parse::<f64>().ok())
            .expect("a number")
    };
    assert_eq!(value_of("L2_z"), 0.0);
    assert_eq!(value_of("screen_z"), 1.0);
    // L2's slide used 2 mm of its travel: its range moves with it, so the mount's limits
    // stay where they are; the screen's slide is no CAD part and keeps its range.
    let line = |name: &str| {
        after
            .lines()
            .find(|l| l.contains(&format!("name = \"{name}\"")))
            .map(str::to_owned)
            .expect("declared")
    };
    assert!(line("L2_z").contains("range = [-7, 3]"), "{}", line("L2_z"));
    assert!(
        line("screen_z").contains("range = [-5, 5]"),
        "{}",
        line("screen_z")
    );
    let update = live
        .next(Duration::from_secs(5))
        .expect("waited")
        .expect("traced again");
    assert_eq!(update["geometry"], "moved");
    let original_sidecar = data.join("original.optobench");
    std::fs::write(&original_sidecar, &text).expect("written");
    let expected = Registry::standard()
        .run(
            "trace",
            &json!({ "path": original_sidecar.to_string_lossy(),
                     "data_directory": data.to_string_lossy(),
                     "dofs": { "L2_z": 2.0, "screen_z": 1.0 } }),
        )
        .expect("traces");
    // Everything but the power by angle: its bins jump when a ray lies on a bin's edge,
    // and the two ways reach the same placement by different sums (0.5 + 1.5 and 2).
    let without_bins = |trace: &Value| {
        let mut detectors = trace["detectors"].clone();
        for d in detectors.as_array_mut().into_iter().flatten() {
            d.as_object_mut().map(|m| m.remove("angular_power_w"));
        }
        detectors
    };
    close(
        &without_bins(&update["trace"]),
        &without_bins(&expected),
        "detectors",
    );
    close(&update["trace"]["ledger"], &expected["ledger"], "ledger");
    drop(live);
    let _ = std::fs::remove_dir_all(&control);
    let _ = std::fs::remove_dir_all(&data);
}
