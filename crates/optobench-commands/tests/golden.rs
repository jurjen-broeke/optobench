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
    Registry::standard()
        .run("trace", &json!({ "path": path.to_string_lossy() }))
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
    assert!((number(&image["centroid_mm"][1]) + 2.0).abs() < 1e-3);
    assert!(number(&image["centroid_mm"][0]).abs() < 1e-9);
    assert!((number(&image["power_w"]) / 0.001 - 0.96_f64.powi(4)).abs() < 1e-3);
    assert_eq!(result["clipping"].as_array().map(Vec::len), Some(0));
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
    let moved_text = text.replacen(
        "kind = \"sidecar\"\n",
        "kind = \"sidecar\"\nstep = \"cage-relay-moved.step\"\n",
        1,
    );
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
    assert!((shift("L2") - optobench_math::Vec3::new(0.0, 0.0, 1.0)).length() < 1e-9);
    assert!(shift("L1").length() < 1e-12);
    assert!(
        (shift("cage-plate:2/Plate") - optobench_math::Vec3::new(0.0, 0.0, 1.0)).length() < 1e-9
    );
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
