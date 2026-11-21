//! The contract test (M13d): every engine command M12 and M13 send, with the parameters
//! the bridge sends, run by an installed SolidCraft on a design built for it, and its
//! replies read by the bridge's own readers and contract check. Run with the path of
//! `solidcraft-cli` in `OPTOBENCH_SOLIDCRAFT_CLI`:
//!
//! ```text
//! OPTOBENCH_SOLIDCRAFT_CLI=/Applications/SolidCraft.app/Contents/MacOS/solidcraft-cli \
//!     cargo test -p optobench-bridge --test contract -- --ignored
//! ```
//!
//! Nightly in CI against the latest SolidCraft release
//! (`.github/workflows/solidcraft-contract.yml`). `solidcraft-cli run` runs engine
//! commands without the app; the control channel's own methods (`engine.batch`,
//! `control.subscribe`) need the running app and are covered by the stand-in's tests.
//!
//! The design: a base (grounded), a lens placed at z `10 mm + L_z` (a placement given as
//! an expression, as a parameter binding is made), and a carriage on a slider joint
//! `stage` (±20 mm) from the base.

use std::path::{Path, PathBuf};
use std::process::Command;

use optobench_bridge::{contract, read};
use serde_json::{Value, json};

fn cli() -> PathBuf {
    PathBuf::from(std::env::var_os("OPTOBENCH_SOLIDCRAFT_CLI").expect(
        "set OPTOBENCH_SOLIDCRAFT_CLI to the path of solidcraft-cli to run the contract test",
    ))
}

/// Runs a script of `commands` (on `design` when given), saving to `save` when given;
/// each command's result.
fn run(
    dir: &Path,
    name: &str,
    commands: &[Value],
    design: Option<&Path>,
    save: Option<&Path>,
) -> Vec<Value> {
    let script = dir.join(format!("{name}.json"));
    std::fs::write(&script, json!({ "commands": commands }).to_string()).expect("written");
    let out = dir.join(format!("{name}.out.json"));
    let mut command = Command::new(cli());
    command
        .arg("run")
        .arg(&script)
        .args(["--results", "--quiet", "--json-out"])
        .arg(&out);
    if let Some(design) = design {
        command.arg("--in").arg(design);
    }
    if let Some(save) = save {
        command.arg("--save").arg(save);
    }
    let output = command.output().expect("solidcraft-cli runs");
    assert!(
        output.status.success(),
        "{name}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let results: Value =
        serde_json::from_str(&std::fs::read_to_string(&out).expect("results")).expect("JSON");
    let results = results.as_array().cloned().expect("one result per command");
    assert_eq!(
        results.len(),
        commands.len(),
        "{name}: a result per command"
    );
    results
}

fn step(command: &str, params: Value) -> Value {
    json!({ "command": command, "params": params })
}

/// The reads M12 makes, with the parameters `read::snapshot` sends.
fn reads() -> Vec<Value> {
    vec![
        step("document.revision", json!({})),
        step("component.list", json!({})),
        step("joint.list", json!({ "quiet": true })),
        step("parameters.list", json!({})),
        step(
            "document.inspect",
            json!({ "measure": false, "timeline": "summary", "bodies": true, "sketch_counts": false }),
        ),
    ]
}

/// What the reads at `at` say: (occurrences, joints, parameters, geometry fingerprint).
fn read_at(
    results: &[Value],
    at: usize,
) -> (
    Vec<read::Occurrence>,
    Vec<read::Joint>,
    Vec<read::Parameter>,
    Value,
) {
    assert!(results[at]["revision"].is_u64(), "{}", results[at]);
    (
        read::occurrences(&results[at + 1]).expect("occurrences"),
        read::joints(&results[at + 2]).expect("joints"),
        read::parameters(&results[at + 3]).expect("parameters"),
        read::geometry(&results[at + 1], &results[at + 3], &results[at + 4]),
    )
}

fn translation(occurrences: &[read::Occurrence], name: &str) -> [f64; 3] {
    let o = occurrences
        .iter()
        .find(|o| o.name == name)
        .unwrap_or_else(|| panic!("no {name}"));
    [o.world[0][3], o.world[1][3], o.world[2][3]]
}

#[test]
#[ignore = "needs an installed SolidCraft: set OPTOBENCH_SOLIDCRAFT_CLI and run with --ignored"]
fn solidcraft_keeps_the_contract_the_bridge_relies_on() {
    let dir = std::env::temp_dir().join(format!("optobench-contract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");

    // 1. The design, and the contracts of every command optobench sends.
    let mut build = vec![
        step("component.create", json!({ "name": "base" })),
        step(
            "solid.box",
            json!({ "length": "40", "width": "40", "height": "5", "name": "BaseBox" }),
        ),
        step("component.activate", json!({ "component": 0 })),
        step("component.create", json!({ "name": "lens" })),
        step(
            "solid.box",
            json!({ "length": "5", "width": "5", "height": "5", "name": "LensBox" }),
        ),
        step("component.activate", json!({ "component": 0 })),
        step("component.create", json!({ "name": "carriage" })),
        step(
            "solid.box",
            json!({ "length": "5", "width": "5", "height": "5", "name": "CarriageBox" }),
        ),
        step("component.activate", json!({ "component": 0 })),
        step(
            "parameters.add",
            json!({ "name": "L_z", "expression": "0 mm" }),
        ),
        step(
            "occurrence.move",
            json!({ "occurrence": "lens:1", "space": "world", "translate": [0, 0, "10 mm + L_z"] }),
        ),
        step(
            "joint.create",
            json!({ "type": "slider", "name": "stage", "flip": true, "limits": [[-20, 20]],
            "a": { "occurrence": "base:1", "plane": "XY" }, "b": { "occurrence": "carriage:1", "plane": "XY" } }),
        ),
    ];
    let described: Vec<&str> = contract::commands()
        .into_iter()
        .chain(contract::commands_for_writing())
        .collect();
    let first_description = build.len();
    for command in &described {
        build.push(step("engine.describe", json!({ "command": command })));
    }
    let first_read = build.len();
    build.extend(reads());
    let design = dir.join("contract.sc");
    let built = run(&dir, "build", &build, None, Some(&design));
    let mut differences = Vec::new();
    for (k, command) in described.iter().enumerate() {
        differences.extend(contract::differences(
            command,
            &built[first_description + k],
        ));
    }
    assert!(
        differences.is_empty(),
        "the contract changed: {differences:?}"
    );
    let (occurrences, joints, parameters, _) = read_at(&built, first_read);
    assert_eq!(translation(&occurrences, "lens:1"), [0.0, 0.0, 10.0]);
    let stage = joints
        .iter()
        .find(|j| j.name == "stage")
        .expect("the joint");
    assert_eq!(stage.kind, "slider");
    assert_eq!(stage.values[0].limits, Some((-20.0, 20.0)));
    assert_eq!(
        parameters
            .iter()
            .find(|p| p.name == "L_z")
            .and_then(|p| p.value),
        Some(0.0)
    );
    let lens = occurrences
        .iter()
        .find(|o| o.name == "lens:1")
        .expect("lens")
        .path
        .clone();

    // 2. The writes, as `write::write` sends them, read back, and taken back.
    let mut writes = vec![step("edit.checkpoint", json!({}))];
    let a = writes.len();
    writes.extend(reads());
    writes.push(step(
        "occurrence.move",
        json!({
        "occurrence": lens.last(), "path": lens, "space": "world",
        "matrix": [[1, 0, 0, 1], [0, 1, 0, 0], [0, 0, 1, 0]], "capture": true }),
    ));
    let b = writes.len();
    writes.extend(reads());
    writes.push(step(
        "parameters.change",
        json!({ "changes": { "L_z": "3 mm" } }),
    ));
    writes.push(step(
        "joint.drive",
        json!({ "joint": stage.id, "index": 0, "value": 4.0, "quiet": true }),
    ));
    let c = writes.len();
    writes.extend(reads());
    writes.push(step("edit.restore_checkpoint", json!({ "checkpoint": 1 })));
    let d = writes.len();
    writes.extend(reads());
    let done = run(&dir, "writes", &writes, Some(&design), None);

    assert_eq!(
        done[0]["checkpoint"], 1,
        "the first checkpoint of a run: {}",
        done[0]
    );
    let (_, _, _, geometry_a) = read_at(&done, a);
    let (moved, _, _, geometry_b) = read_at(&done, b);
    // A move changes placements only: the fast path's premise.
    assert_eq!(
        geometry_a, geometry_b,
        "a move changed the geometry fingerprint"
    );
    assert_eq!(translation(&moved, "lens:1"), [1.0, 0.0, 10.0]);
    let (after, joints, parameters, _) = read_at(&done, c);
    // The parameter moves the lens through its placement's expression, after the move.
    assert_eq!(translation(&after, "lens:1"), [1.0, 0.0, 13.0]);
    assert_eq!(translation(&after, "carriage:1")[2], 4.0);
    let stage_after = joints
        .iter()
        .find(|j| j.name == "stage")
        .expect("the joint");
    assert_eq!(stage_after.values[0].value, 4.0);
    assert_eq!(
        parameters
            .iter()
            .find(|p| p.name == "L_z")
            .and_then(|p| p.value),
        Some(3.0)
    );
    let (restored, joints, parameters, geometry_d) = read_at(&done, d);
    assert_eq!(translation(&restored, "lens:1"), [0.0, 0.0, 10.0]);
    assert_eq!(translation(&restored, "carriage:1")[2], 0.0);
    assert_eq!(
        joints
            .iter()
            .find(|j| j.name == "stage")
            .expect("joint")
            .values[0]
            .value,
        0.0
    );
    assert_eq!(
        parameters
            .iter()
            .find(|p| p.name == "L_z")
            .and_then(|p| p.value),
        Some(0.0)
    );
    assert_eq!(geometry_d, geometry_a);
    let _ = std::fs::remove_dir_all(&dir);
}
