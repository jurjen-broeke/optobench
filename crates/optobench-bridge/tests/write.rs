//! M13b against a stand-in for SolidCraft (`optobench_bridge::fake`): DOF changes as
//! `cad-moves` gives them, made in the design in one batch, read back, and taken back
//! whole when anything is not where it was asked to be.

use std::path::{Path, PathBuf};
use std::time::Duration;

use optobench_bridge::BridgeError;
use optobench_bridge::connection::Connection;
use optobench_bridge::discovery::Endpoint;
use optobench_bridge::fake::FakeApp;
use optobench_bridge::write::{Tolerance, write};
use serde_json::{Value, json};

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../optobench-io/tests/step")
        .join(name)
}

fn start() -> (FakeApp, Connection) {
    let app = FakeApp::start(&golden("cage-relay.step"));
    let link = Connection::open(
        &Endpoint::new(app.port, app.token()),
        Duration::from_secs(2),
    )
    .expect("connected");
    (app, link)
}

/// One `cad-moves` item: `occurrence` moved `dz` mm along z, with `bindings` (or none).
fn along_z(occurrence: &str, dz: f64, bindings: Value) -> Value {
    json!({ "items": [{
        "occurrence": occurrence, "dofs": ["d"], "changed": true,
        "matrix": [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, dz], [0, 0, 0, 1]],
        "bindings": bindings,
    }], "skipped": [] })
}

#[test]
fn a_part_is_moved_where_it_was_asked_to_go() {
    let (app, mut link) = start();
    let done = write(
        &mut link,
        &along_z("cage-plate:2", 2.0, Value::Null),
        Tolerance::default(),
    )
    .expect("written");
    assert_eq!(done["written"], 1);
    assert_eq!(app.world("cage-plate:2")[2][3], 2.0);
    assert_eq!(app.checkpoints(), 0, "the checkpoint is dropped");
    assert!(
        app.served()
            .contains(&"engine.batch occurrence.move".to_owned())
    );
}

#[test]
fn a_parameter_and_a_joint_are_set_by_the_change_of_their_dofs() {
    let (app, mut link) = start();
    // L1's placement follows `L1_z`; L2 is held by the slider `L2 slide` (at 50 mm).
    app.bind_parameter("pc-lens:1", "L1_z", [0.0, 0.0, 1.0], 1.0);
    let plan = json!({ "items": [
        { "occurrence": "pc-lens:1", "dofs": ["L1_z"], "changed": true,
          "matrix": [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 1.5], [0, 0, 0, 1]],
          "bindings": [{ "dof": "L1_z", "parameter": "L1_z", "change": 1.5, "unit": "mm" }] },
        { "occurrence": "pc-lens:2", "dofs": ["L2_z"], "changed": true,
          "matrix": [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 3.0], [0, 0, 0, 1]],
          "bindings": [{ "dof": "L2_z", "joint": "L2 slide", "index": 0, "change": 3.0, "unit": "mm" }] },
        { "occurrence": "rod:1", "dofs": [], "changed": false,
          "matrix": [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]], "bindings": null } ],
        "skipped": [] });
    let done = write(&mut link, &plan, Tolerance::default()).expect("written");
    assert_eq!(done["written"], 2, "{done}");
    assert_eq!(app.parameter("L1_z"), Some(1.5));
    assert_eq!(app.world("pc-lens:1")[2][3], 1.5);
    assert_eq!(app.world("pc-lens:2")[2][3], 3.0);
    // All parameters in one command, first; unchanged parts not written.
    let served = app.served();
    let batch: Vec<&String> = served
        .iter()
        .filter(|s| s.starts_with("engine.batch "))
        .collect();
    assert_eq!(
        batch,
        ["engine.batch parameters.change", "engine.batch joint.drive"]
    );
}

#[test]
fn a_binding_that_does_not_move_its_part_as_asked_is_taken_back() {
    // `L1_z` moves L1 twice its value (an expression `2 * L1_z`): 1 mm asked, 2 mm made.
    let (app, mut link) = start();
    app.bind_parameter("pc-lens:1", "L1_z", [0.0, 0.0, 1.0], 2.0);
    let plan = along_z(
        "pc-lens:1",
        1.0,
        json!([{ "dof": "L1_z", "parameter": "L1_z", "change": 1.0, "unit": "mm" }]),
    );
    let error = write(&mut link, &plan, Tolerance::default()).expect_err("taken back");
    let BridgeError::Mismatch(differences) = &error else {
        panic!("{error}");
    };
    assert!(
        differences[0].contains("`pc-lens:1` is 1.000e0 mm"),
        "{differences:?}"
    );
    assert!(error.to_string().contains("the design is as it was"));
    assert_eq!(app.parameter("L1_z"), Some(0.0));
    assert_eq!(app.world("pc-lens:1")[2][3], 0.0);
    assert_eq!(app.checkpoints(), 0);
}

#[test]
fn a_joint_held_at_its_limit_is_taken_back() {
    // `L2 slide` is at 50 within [40, 60]: 20 mm more stops at 60.
    let (app, mut link) = start();
    let plan = along_z(
        "pc-lens:2",
        20.0,
        json!([{ "dof": "L2_z", "joint": "L2 slide", "index": 0, "change": 20.0, "unit": "mm" }]),
    );
    let error = write(&mut link, &plan, Tolerance::default()).expect_err("taken back");
    assert!(
        error
            .to_string()
            .contains("joint `L2 slide` is at Some(60.0), not 70"),
        "{error}"
    );
    assert_eq!(app.world("pc-lens:2")[2][3], 0.0);
}

#[test]
fn a_failing_command_or_a_changed_contract_changes_nothing() {
    let (app, mut link) = start();
    let plan = along_z(
        "pc-lens:2",
        1.0,
        json!([{ "dof": "L2_z", "joint": "no such joint", "index": 0, "change": 1.0, "unit": "mm" }]),
    );
    let error = write(&mut link, &plan, Tolerance::default()).expect_err("refused");
    assert!(
        error.to_string().contains("no joint `no such joint`"),
        "{error}"
    );
    // A part the design does not have.
    let error = write(
        &mut link,
        &along_z("lens:9", 1.0, Value::Null),
        Tolerance::default(),
    )
    .expect_err("refused");
    assert!(
        error.to_string().contains("no occurrence `lens:9`"),
        "{error}"
    );
    // A changed write contract: refused before anything is read or written.
    app.remove_from_contract("occurrence.move", "matrix");
    let error = write(
        &mut link,
        &along_z("cage-plate:2", 1.0, Value::Null),
        Tolerance::default(),
    )
    .expect_err("refused");
    assert!(matches!(error, BridgeError::Contract(_)), "{error}");
    assert!(!app.served().iter().any(|s| s.starts_with("engine.batch")));
    assert_eq!(app.world("cage-plate:2")[2][3], 0.0);
}
