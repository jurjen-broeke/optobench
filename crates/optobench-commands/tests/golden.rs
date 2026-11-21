//! M4 exit criteria, on files committed in `crates/optobench-io/tests/step/` (made by
//! SolidCraft 0.30.1 from the scripts beside them):
//!
//! - the golden cage relay is traced from STEP and its sidecar, and matches both theory
//!   and its expected report;
//! - a re-export after moving a lens keeps every match;
//! - `export-placements` turns a standalone layout into poses and a sidecar.

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../optobench-io/tests/step")
}

fn trace(path: &Path) -> Value {
    // A data directory with no parts library or imported materials, so the user's own
    // library cannot change the result.
    let empty = std::env::temp_dir().join(format!("optobench-golden-{}", std::process::id()));
    Registry::standard()
        .run(
            "trace",
            &json!({ "path": path.to_string_lossy(), "data_directory": empty.to_string_lossy() }),
        )
        .expect("traces")
}

fn number(v: &Value) -> f64 {
    v.as_f64().expect("a number")
}

/// Numbers equal within 1e-12 absolute plus 1e-9 relative; everything else equal.
fn same(a: &Value, b: &Value, at: &str) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: Vec<&String> = x.keys().collect();
            assert_eq!(keys, y.keys().collect::<Vec<_>>(), "keys at {at}");
            for k in keys {
                same(&x[k], &y[k], &format!("{at}.{k}"));
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            assert_eq!(x.len(), y.len(), "length at {at}");
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                same(p, q, &format!("{at}[{i}]"));
            }
        }
        (Value::Number(_), Value::Number(_)) => {
            let (p, q) = (number(a), number(b));
            assert!(
                (p - q).abs() <= 1e-12 + 1e-9 * p.abs().max(q.abs()),
                "{at}: {p} vs {q}"
            );
        }
        _ => assert_eq!(a, b, "at {at}"),
    }
}

#[test]
fn golden_cage_relay_traces_from_step() {
    let mut result = trace(&fixtures().join("cage-relay.optobench"));
    let image = &result["detectors"][0];
    // Theory (header of cage-relay.optobench): magnification −1, so the point 2 mm off
    // axis images 2 mm on the other side; four uncoated faces near normal incidence pass
    // 0.96⁴ (Fresnel; Hecht, Optics, 5th ed., ch. 4); nothing is clipped.
    assert!((number(&image["direct_centroid_mm"][1]) + 2.0).abs() < 1e-3);
    assert!(number(&image["direct_centroid_mm"][0]).abs() < 1e-9);
    assert!((number(&image["direct_power_w"]) / 0.001 - 0.96_f64.powi(4)).abs() < 1e-3);
    // The direct beam passes the plates' bores; only ghosts (stray reflections) reach
    // the mounts.
    let clipped = number(&result["ledger"]["clipped_w"]) / 0.001;
    assert!(clipped < 0.05, "{clipped}");
    assert!(number(&result["ledger"]["imbalance_w"]).abs() < 1e-15);
    // And it reproduces its expected report.
    result.as_object_mut().map(|o| o.remove("scene"));
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures().join("cage-relay.expected.json"))
            .expect("expected report"),
    )
    .expect("JSON");
    same(&result, &expected, "report");
}

#[test]
fn reexport_after_a_move_keeps_every_match() {
    // cage-relay-moved.step: the same design with L2 and its plate moved 1 mm along z and
    // exported again. The same sidecar must match the same bodies.
    use optobench_io::sidecar::parse;
    let lookup = |name: &str| Err(format!("no material `{name}`"));
    let text = std::fs::read_to_string(fixtures().join("cage-relay.optobench")).expect("sidecar");
    let original =
        parse(&text, &fixtures().join("cage-relay.optobench"), &lookup).expect("matches");
    // Point the same sidecar at the re-export, whatever the line endings of the checkout
    // (a Windows checkout may have CRLF).
    let mut inserted = false;
    let moved_text: String = text
        .lines()
        .flat_map(|line| {
            let mut out = vec![line.to_owned()];
            // After the header's `kind` line only (elements have `kind` lines too).
            if !inserted && line.trim_start().starts_with("kind") {
                out.push("step = \"cage-relay-moved.step\"".to_owned());
                inserted = true;
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(moved_text.contains("step = \"cage-relay-moved.step\""));
    // The same sidecar as a Windows checkout would have it (CRLF) reads the same.
    let crlf = moved_text.replace('\n', "\r\n");
    let from_crlf = parse(&crlf, &fixtures().join("cage-relay.optobench"), &lookup)
        .expect("CRLF line endings read the same");
    assert_eq!(from_crlf.solids.len(), original.solids.len());
    let moved = parse(
        &moved_text,
        &fixtures().join("cage-relay.optobench"),
        &lookup,
    )
    .expect("still matches");
    let ids = |scene: &optobench_scene::Scene| {
        scene
            .solids
            .iter()
            .map(|s| s.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&original), ids(&moved));
    let shift = |id: &str| {
        let find = |scene: &optobench_scene::Scene| {
            scene
                .solids
                .iter()
                .find(|s| s.id == id)
                .expect("solid")
                .faces[0]
                .placement
                .translation
        };
        find(&moved) - find(&original)
    };
    let one_mm = optobench_math::Vec3::new(0.0, 0.0, 1.0);
    let l2 = shift("L2");
    assert!((l2 - one_mm).length() < 1e-9, "L2 moved by {l2:?}");
    let l1 = shift("L1");
    assert!(l1.length() < 1e-12, "L1 moved by {l1:?}");
    let plate = shift("cage-plate:2/Plate");
    assert!((plate - one_mm).length() < 1e-9, "plate moved by {plate:?}");
}

#[test]
fn export_placements_writes_poses_and_a_matching_sidecar() {
    let layout =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/planar-relay.optobench");
    let out = std::env::temp_dir().join(format!("optobench-placements-{}", std::process::id()));
    std::fs::create_dir_all(&out).expect("scratch");
    let result = Registry::standard()
        .run(
            "export-placements",
            &json!({ "path": layout.to_string_lossy(), "out_directory": out.to_string_lossy() }),
        )
        .expect("exports");
    assert_eq!(result["elements"], 3);
    let placements: Value = serde_json::from_str(
        &std::fs::read_to_string(out.join("planar-relay.placements.json")).expect("written"),
    )
    .expect("JSON");
    // L1 sits at 99.3289 mm along +x on the axis; its optical axis is +x.
    let l1 = placements["elements"]
        .as_array()
        .and_then(|e| e.iter().find(|e| e["id"] == "L1"))
        .expect("L1");
    assert!((number(&l1["origin"][0]) - 99.3289).abs() < 1e-9);
    assert_eq!(l1["z_axis"], json!([1.0, 0.0, 0.0]));
    // The fold mirror's normal bisects −x and +y.
    let m1 = placements["elements"]
        .as_array()
        .and_then(|e| e.iter().find(|e| e["id"] == "M1"))
        .expect("M1");
    let s = std::f64::consts::FRAC_1_SQRT_2;
    assert!(
        (number(&m1["z_axis"][0]) + s).abs() < 1e-12
            && (number(&m1["z_axis"][1]) - s).abs() < 1e-12
    );
    // The sidecar skeleton is a sidecar for planar-relay.step naming each element's body.
    let sidecar =
        std::fs::read_to_string(out.join("planar-relay.sidecar.optobench")).expect("written");
    let table: toml::Table = sidecar.parse().expect("valid TOML");
    assert_eq!(table["kind"].as_str(), Some("sidecar"));
    assert_eq!(table["step"].as_str(), Some("planar-relay.step"));
    let elements = table["element"].as_array().expect("elements");
    assert_eq!(elements.len(), 3);
    assert_eq!(elements[0]["body"]["product"].as_str(), Some("L1"));
    assert_eq!(elements[0]["material"].as_float(), Some(1.5));
    assert_eq!(
        table["detector"][0]["position"].as_array().map(Vec::len),
        Some(3)
    );
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn moves_place_occurrences_as_a_new_export_would() {
    // The live link's fast path (M12e): the golden relay's STEP file with the moves its
    // moved re-export was made with (cage-relay-moved.json: `pc-lens:2` and
    // `cage-plate:2` 1 mm along world z) traces as the moved export does.
    let empty = std::env::temp_dir().join(format!("optobench-golden-moves-{}", std::process::id()));
    let along = json!([[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 1], [0, 0, 0, 1]]);
    let run = |extra: Value| {
        let mut parameters = json!({
            "path": fixtures().join("cage-relay.optobench").to_string_lossy(),
            "data_directory": empty.to_string_lossy(),
        });
        for (k, v) in extra.as_object().expect("an object") {
            parameters[k] = v.clone();
        }
        Registry::standard().run("trace", &parameters)
    };
    let moved = run(json!({ "moves": [
        { "occurrence": "pc-lens:2", "matrix": along },
        { "occurrence": "cage-plate:2", "matrix": along } ] }))
    .expect("traces");
    let exported =
        run(json!({ "step": fixtures().join("cage-relay-moved.step").to_string_lossy() }))
            .expect("traces");
    same(&moved["detectors"], &exported["detectors"], "detectors");
    same(&moved["ledger"], &exported["ledger"], "ledger");
    let unmoved = run(json!({})).expect("traces");
    assert_ne!(moved["detectors"], unmoved["detectors"], "the move shows");
    // An occurrence the assembly does not have, and a move that is not rigid, refused.
    let error = run(json!({ "moves": [{ "occurrence": "pc-lens:9", "matrix": along }] }))
        .expect_err("refused");
    assert!(
        error.message.contains("no occurrence `pc-lens:9`"),
        "{}",
        error.message
    );
    let stretched = json!([[2, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]);
    let error = run(json!({ "moves": [{ "occurrence": "pc-lens:2", "matrix": stretched }] }))
        .expect_err("refused");
    assert!(error.message.contains("not rigid"), "{}", error.message);
}

#[test]
fn cad_moves_say_what_to_change_in_the_design() {
    // M13a, on the golden relay: L2 slides 2 mm along z (no binding: its occurrence is
    // moved), L1 tilts 1.5° about x through its joint's second value, the screen's slide
    // stays in the file. A part with one DOF bound and one not, and a binding that is
    // neither a parameter nor a joint, are refused.
    let dir = std::env::temp_dir().join(format!("optobench-cad-moves-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let original = std::fs::read_to_string(fixtures().join("cage-relay.optobench")).expect("read");
    let step = fixtures().join("cage-relay.step");
    let with = |extra_l1: &str, extra_l2: &str| {
        original
            .replace(
                "body = { product = \"pc-lens\", occurrence = \"pc-lens:1\" }",
                &format!(
                    "body = {{ product = \"pc-lens\", occurrence = \"pc-lens:1\" }}\n{extra_l1}"
                ),
            )
            .replace(
                "body = { product = \"pc-lens\", occurrence = \"pc-lens:2\" }",
                &format!(
                    "body = {{ product = \"pc-lens\", occurrence = \"pc-lens:2\" }}\n{extra_l2}"
                ),
            )
            .replace(
                "kind = \"sidecar\"\n",
                &format!("kind = \"sidecar\"\nstep = '{}'\n", step.to_string_lossy()),
            )
            + "dof = [ { name = \"screen_z\", kind = \"slide\", axis = [0, 0, 1], range = [-5, 5] } ]\n"
    };
    let run = |text: &str, dofs: Value| {
        let path = dir.join("relay.optobench");
        std::fs::write(&path, text).expect("written");
        Registry::standard().run(
            "cad-moves",
            &json!({ "path": path.to_string_lossy(), "data_directory": dir.to_string_lossy(), "dofs": dofs }),
        )
    };
    let l1 = "dof = [ { name = \"L1_tip\", kind = \"tilt\", axis = [1, 0, 0], range = [-5, 5], binding = { joint = \"L1 mount\", index = 1 } } ]";
    let l2 = "dof = [ { name = \"L2_z\", kind = \"slide\", axis = [0, 0, 1], range = [-5, 5] } ]";
    let result = run(
        &with(l1, l2),
        json!({ "L1_tip": 1.5, "L2_z": 2.0, "screen_z": 1.0 }),
    )
    .expect("planned");
    let item = |occurrence: &str| {
        result["items"]
            .as_array()
            .and_then(|l| l.iter().find(|i| i["occurrence"] == occurrence))
            .cloned()
            .unwrap_or_else(|| panic!("no {occurrence}: {result}"))
    };
    let moved = item("pc-lens:2");
    assert_eq!(moved["bindings"], Value::Null);
    assert_eq!(moved["matrix"][2][3], json!(2.0));
    assert_eq!(moved["matrix"][3], json!([0.0, 0.0, 0.0, 1.0]));
    let tipped = item("pc-lens:1");
    assert!((tipped["rotate_deg"].as_f64().expect("deg") - 1.5).abs() < 1e-12);
    let binding = &tipped["bindings"][0];
    assert_eq!(binding["joint"], "L1 mount");
    assert_eq!(binding["index"], 1);
    assert_eq!(binding["unit"], "deg");
    assert!((binding["change"].as_f64().expect("change") - 1.5).abs() < 1e-12);
    assert_eq!(result["skipped"][0]["name"], "image");
    assert_eq!(result["skipped"][0]["changed"], true);
    // Mixed on one part, and a binding of neither kind: refused.
    let mixed = "dof = [ { name = \"L2_z\", kind = \"slide\", axis = [0, 0, 1], binding = { parameter = \"L2_z\" } },\n        { name = \"L2_x\", kind = \"slide\", axis = [1, 0, 0] } ]";
    let error = run(&with("", mixed), json!({ "L2_z": 1.0 })).expect_err("refused");
    assert!(
        error.message.contains("bind all its degrees of freedom"),
        "{}",
        error.message
    );
    let odd = "dof = [ { name = \"L2_z\", kind = \"slide\", axis = [0, 0, 1], binding = { motor = \"x\" } } ]";
    let error = run(&with("", odd), json!({ "L2_z": 1.0 })).expect_err("refused");
    assert!(error.message.contains("`binding`"), "{}", error.message);
    let _ = std::fs::remove_dir_all(&dir);
}
