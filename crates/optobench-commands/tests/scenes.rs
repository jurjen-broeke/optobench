//! M3 exit criteria, through the `trace` command on the scene files in `examples/`:
//! a planar relay and a table periscope traced from scene files alone, and clipping by a
//! tube proxy detected. Reference values are worked out in each file's header comment.

use optobench_commands::Registry;
use serde_json::{Value, json};

fn trace(file: &str) -> Value {
    let path = format!("{}/../../examples/{file}", env!("CARGO_MANIFEST_DIR"));
    Registry::standard()
        .run("trace", &json!({ "path": path }))
        .expect("the scene traces")
}

fn number(value: &Value) -> f64 {
    value.as_f64().expect("a number")
}

fn balanced(result: &Value) {
    let ledger = &result["ledger"];
    assert!(number(&ledger["imbalance_w"]).abs() < 1e-12 * number(&ledger["launched_w"]));
}

#[test]
fn planar_relay_images_with_magnification_minus_one() {
    let result = trace("planar-relay.optobench");
    let image = &result["detectors"][0];
    // Object 2 mm above the axis; a 4f relay of identical lenses images it 2 mm below.
    assert!((number(&image["centroid_mm"][1]) + 2.0).abs() < 1e-3);
    assert!(number(&image["centroid_mm"][0]).abs() < 1e-9);
    // f/57 cone: the residual spherical aberration leaves a spot of a few µm.
    assert!(number(&image["rms_radius_mm"]) < 0.01);
    // Four uncoated faces near normal incidence: 0.96⁴ of the power, nothing clipped
    // (Fresnel at normal incidence, Hecht, Optics, 5th ed., ch. 4).
    let fraction = number(&image["power_w"]) / number(&result["ledger"]["launched_w"]);
    assert!((fraction - 0.96_f64.powi(4)).abs() < 1e-3, "{fraction}");
    assert_eq!(result["clipping"].as_array().map(Vec::len), Some(0));
    balanced(&result);
}

#[test]
fn table_periscope_raises_the_beam_without_clipping() {
    let result = trace("table-periscope.optobench");
    let screen = &result["detectors"][0];
    assert!((number(&screen["power_w"]) - 0.001).abs() < 1e-15);
    assert!(number(&screen["centroid_mm"][0]).abs() < 1e-9);
    assert!(number(&screen["centroid_mm"][1]).abs() < 1e-9);
    assert_eq!(
        result["clipping"].as_array().map(Vec::len),
        Some(0),
        "the riser tube is wide enough"
    );
    balanced(&result);
}

#[test]
fn narrow_tube_clipping_is_reported() {
    let result = trace("planar-relay-clipped.optobench");
    let clipping = &result["clipping"][0];
    assert_eq!(clipping["id"], "narrow-tube");
    assert_eq!(clipping["kind"], "proxy");
    let fraction = number(&clipping["fraction"]);
    assert!(fraction > 0.1 && fraction < 0.9, "{fraction}");
    // What gets through still images at the same place.
    assert!((number(&result["detectors"][0]["centroid_mm"][1]) + 2.0).abs() < 1e-2);
    balanced(&result);
}

#[test]
fn layout_view_is_written() {
    let path = format!(
        "{}/../../examples/table-periscope.optobench",
        env!("CARGO_MANIFEST_DIR")
    );
    let svg = std::env::temp_dir().join(format!("optobench-layout-{}.svg", std::process::id()));
    let result = Registry::standard()
        .run(
            "trace",
            &json!({ "path": path, "svg": svg.to_string_lossy(), "view": "side" }),
        )
        .expect("the scene traces");
    assert_eq!(result["svg"], json!(svg.to_string_lossy()));
    let text = std::fs::read_to_string(&svg).expect("written");
    assert!(text.starts_with("<svg") && text.contains("side view") && text.contains(">screen<"));
    let _ = std::fs::remove_file(&svg);
}
