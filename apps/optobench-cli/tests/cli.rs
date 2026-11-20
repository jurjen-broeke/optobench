//! End-to-end checks of the `optobench` binary: JSON in, JSON out.

use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn run(arguments: &[&str], input: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_optobench"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the optobench binary starts");
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(text.as_bytes()).expect("write to stdin");
    }
    child
        .wait_with_output()
        .expect("the optobench binary exits")
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("standard output is JSON")
}

#[test]
fn version_prints_json() {
    let output = run(&["version"], None);
    assert!(output.status.success());
    assert_eq!(json(&output)["file_format"], "optobench/1");
}

#[test]
fn parameters_can_come_from_standard_input() {
    let output = run(&["version", "-"], Some("{}"));
    assert!(output.status.success());
    assert_eq!(json(&output)["application"], "optobench");
}

#[test]
fn errors_are_json_with_a_failing_status() {
    let output = run(&["version", "{\"verbose\": true}"], None);
    assert!(!output.status.success());
    assert_eq!(json(&output)["error"]["code"], "invalid_parameters");

    let output = run(&["no-such-command"], None);
    assert!(!output.status.success());
    assert_eq!(json(&output)["error"]["code"], "unknown_command");
}

#[test]
fn help_lists_the_registered_commands() {
    let output = run(&["help"], None);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(text.contains("version") && text.contains("commands"));
}

#[test]
fn live_prints_a_line_per_trace() {
    // Against a stand-in for SolidCraft (no SolidCraft in CI): the golden cage relay's
    // sidecar traced as the design is, then again after a lens moved; `changes: 1` ends it.
    use std::io::BufRead;
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/optobench-io/tests/step");
    let app = optobench_bridge::fake::FakeApp::start(&fixtures.join("cage-relay.step"));
    let scratch = std::env::temp_dir().join(format!("optobench-cli-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    app.write_control_folder(&scratch.join("control"));
    std::fs::create_dir_all(scratch.join("data")).expect("data");
    let parameters = serde_json::json!({
        "path": fixtures.join("cage-relay.optobench").to_string_lossy(),
        "control_directory": scratch.join("control").to_string_lossy(),
        "data_directory": scratch.join("data").to_string_lossy(),
        "changes": 1,
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_optobench"))
        .args(["live", &parameters.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("starts");
    let mut lines = std::io::BufReader::new(child.stdout.take().expect("stdout")).lines();
    let first: Value =
        serde_json::from_str(&lines.next().expect("a line").expect("read")).expect("JSON");
    assert_eq!(first["revision"], 1);
    assert_eq!(first["detectors"][0]["id"], "image");
    app.change(&fixtures.join("cage-relay-moved.step"));
    let second: Value =
        serde_json::from_str(&lines.next().expect("a line").expect("read")).expect("JSON");
    assert_eq!(second["revision"], 2);
    assert!(second["seconds"].as_f64().expect("seconds") < 2.0);
    assert_ne!(second["detectors"], first["detectors"], "the move shows");
    assert!(child.wait().expect("exits").success());
    let _ = std::fs::remove_dir_all(&scratch);
}
