//! The live link against a stand-in for SolidCraft (`optobench_bridge::fake`): found by
//! its control folder, its contract checked, the design read, a changed contract and a
//! wrong token refused, change events received and the design exported as STEP.

use std::path::{Path, PathBuf};
use std::time::Duration;

use optobench_bridge::BridgeError;
use optobench_bridge::connection::Connection;
use optobench_bridge::discovery::{Endpoint, find_in};
use optobench_bridge::fake::FakeApp;
use optobench_bridge::{contract, read};

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../optobench-io/tests/step")
        .join(name)
}

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("optobench-bridge-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

const SHORT: Duration = Duration::from_secs(2);

#[test]
fn the_design_is_found_checked_and_read() {
    let app = FakeApp::start(&golden("cage-relay.step"));
    let control = scratch("found");
    app.write_control_folder(&control);
    let endpoint = find_in(&control, SHORT).expect("found");
    assert_eq!(endpoint.port, app.port);
    let mut link = Connection::open(&endpoint, SHORT).expect("connected");
    contract::check(&mut link).expect("the contract holds");
    let snapshot = read::snapshot(&mut link).expect("read");
    assert_eq!(snapshot.revision.document_id, "design-1");
    let names: Vec<&str> = snapshot
        .occurrences
        .iter()
        .map(|o| o.name.as_str())
        .collect();
    assert_eq!(names, ["cage:1", "lens:1"]);
    assert_eq!(snapshot.occurrences[1].path, [1, 2]);
    assert_eq!(snapshot.occurrences[1].world[2][3], 50.0);
    assert_eq!(snapshot.joints[0].kind, "slider");
    assert_eq!(snapshot.joints[0].values[0].limits, Some((40.0, 60.0)));
    assert_eq!(snapshot.parameters[0].value, Some(150.0));
    // Read only: nothing but reads were asked for.
    assert!(
        app.served().iter().all(|s| {
            s == "engine.describe"
                || s.starts_with("engine.execute document.")
                || s.ends_with(".list")
        }),
        "{:?}",
        app.served()
    );
    let _ = std::fs::remove_dir_all(&control);
}

#[test]
fn a_changed_contract_and_a_wrong_token_are_refused() {
    let app = FakeApp::start(&golden("cage-relay.step"));
    app.remove_from_contract("component.list", "transform");
    let endpoint = Endpoint::new(app.port, app.token());
    let mut link = Connection::open(&endpoint, SHORT).expect("connected");
    match contract::check(&mut link) {
        Err(BridgeError::Contract(changes)) => {
            assert_eq!(
                changes,
                ["`component.list` output PlacementInfo has no `transform`"]
            );
        }
        other => panic!("{other:?}"),
    }
    let wrong = Endpoint::new(app.port, "not-the-token");
    let mut link = Connection::open(&wrong, SHORT).expect("connected");
    let error = read::revision(&mut link).expect_err("refused");
    assert_eq!(error, BridgeError::Unauthorized);
    assert!(!error.to_string().contains(app.token()));
}

#[test]
fn a_change_is_announced_and_the_design_exported() {
    let app = FakeApp::start(&golden("cage-relay.step"));
    let endpoint = Endpoint::new(app.port, app.token());
    let mut link = Connection::open(&endpoint, SHORT).expect("connected");
    let start = read::subscribe(&mut link).expect("subscribed");
    assert_eq!(start.revision, 1);
    // Nothing changes: nothing comes.
    assert_eq!(
        read::wait_for_change(
            &mut link,
            Duration::from_millis(100),
            Duration::from_millis(50)
        )
        .expect("waited"),
        None
    );
    // Three quick changes come as one, the last.
    for _ in 0..3 {
        app.change(&golden("cage-relay-moved.step"));
    }
    let changed = read::wait_for_change(&mut link, SHORT, Duration::from_millis(100))
        .expect("waited")
        .expect("a change");
    assert_eq!(changed.revision, 4);
    let folder = scratch("export");
    let written = read::export_step(&mut link, &folder, "relay").expect("exported");
    assert_eq!(written, folder.join("relay.step"));
    assert_eq!(
        std::fs::read(&written).expect("read"),
        std::fs::read(golden("cage-relay-moved.step")).expect("read")
    );
    // Requests after the subscription still get their replies between events.
    assert_eq!(read::revision(&mut link).expect("read").revision, 4);
    let _ = std::fs::remove_dir_all(&folder);
}
