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
