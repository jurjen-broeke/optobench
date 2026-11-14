//! Alignment irises on a table layout (M16): where to put them on the bench, from the
//! traced beam. Reference values from the layout's geometry: a beam at 75 mm folded by two
//! 45° mirrors runs along y = 200 mm from M2 at hole (6, 8) to the screen at hole (20, 8);
//! with 1.5 pitches kept free at each end, the holes on it run from column 8 to 18.

use optobench_commands::Registry;
use serde_json::{Value, json};

fn trace(label: &str, text: &str) -> Value {
    let path = std::env::temp_dir().join(format!(
        "optobench-irises-{label}-{}.optobench",
        std::process::id()
    ));
    std::fs::write(&path, text).expect("written");
    let result = Registry::standard()
        .run("trace", &json!({ "path": path.to_string_lossy() }))
        .unwrap_or_else(|e| panic!("{}", e.message));
    let _ = std::fs::remove_file(&path);
    result
}

fn layout(m2: &str, screen: &str, screen_axis: &str) -> String {
    format!(
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"table\"\n\n[table]\npitch = 25.0\n\n\
         [[source]]\nid = \"laser\"\nkind = \"collimated\"\nwavelength_um = 0.633\npower_w = 0.001\ndiameter = 2.0\n\
         hole = [0, 0]\nheight = 75.0\naxis = [1, 0, 0]\n\n\
         [[element]]\nid = \"M1\"\nkind = \"mirror\"\nhole = [6, 0]\nheight = 75.0\naxis = [-1, 1, 0]\ndiameter = 25.4\nclear_aperture = 22.9\n\n\
         [[element]]\nid = \"M2\"\nkind = \"mirror\"\n{m2}\nheight = 75.0\ndiameter = 25.4\nclear_aperture = 22.9\n\n\
         [[detector]]\nid = \"screen\"\n{screen}\nheight = 75.0\naxis = {screen_axis}\nsize = [20, 20]\npixels = [20, 20]\n"
    )
}

#[test]
fn two_irises_go_on_the_last_run_as_far_apart_as_its_mounts_allow() {
    let r = trace(
        "folded",
        &layout(
            "hole = [6, 8]\naxis = [1, -1, 0]",
            "hole = [20, 8]",
            "[1, 0, 0]",
        ),
    );
    assert!((r["ledger"]["detected_w"].as_f64().expect("w") - 0.001).abs() < 1e-12);
    let a = &r["alignment_irises"];
    assert_eq!(a["segment"]["from"], "M2");
    assert_eq!(a["segment"]["to"], "screen");
    assert!((a["segment"]["length_mm"].as_f64().expect("mm") - 350.0).abs() < 1e-9);
    assert_eq!(a["steering"], json!(["M1", "M2"]));
    let irises = a["irises"].as_array().expect("irises");
    assert_eq!(irises.len(), 2);
    assert_eq!(irises[0]["hole"], json!([8, 8]));
    assert_eq!(irises[1]["hole"], json!([18, 8]));
    for i in irises {
        assert_eq!(i["height"].as_f64(), Some(75.0));
        assert_eq!(i["on_grid"], true);
    }
    let advice = a["advice"].as_str().expect("advice");
    assert!(
        advice.contains("250 mm apart")
            && advice.contains("near one with M1")
            && advice.contains("far one with M2"),
        "{advice}"
    );
}

/// M2 at hole (6, 8) sending the beam along `direction` (in the table's plane) to the
/// screen at `screen`: its normal bisects the reversed incoming (+y) and the outgoing.
fn turned_to(label: &str, direction: [f64; 2], screen: &str) -> Value {
    let length = direction[0].hypot(direction[1]);
    let normal = [direction[0] / length, direction[1] / length - 1.0];
    trace(
        label,
        &layout(
            &format!("hole = [6, 8]\naxis = [{}, {}, 0]", normal[0], normal[1]),
            screen,
            &format!("[{}, {}, 0]", direction[0], direction[1]),
        ),
    )
}

#[test]
fn a_diagonal_run_through_holes_gets_its_irises_on_them() {
    // Along (1, 1): holes (7, 9) … (15, 17) lie on the beam; those within 1.5 pitches
    // (37.5 mm) of the ends, (7, 9) and (15, 17) at 35.4 mm, are left out.
    let r = turned_to("diagonal", [1.0, 1.0], "hole = [16, 18]");
    assert!((r["ledger"]["detected_w"].as_f64().expect("w") - 0.001).abs() < 1e-12);
    let irises = r["alignment_irises"]["irises"].as_array().expect("irises");
    assert_eq!(irises.len(), 2, "{r}");
    assert_eq!(irises[0]["hole"], json!([8, 10]));
    assert_eq!(irises[1]["hole"], json!([14, 16]));
    assert!(irises.iter().all(|i| i["on_grid"] == true));
}

#[test]
fn a_run_between_holes_gets_irises_off_the_grid() {
    // Along (1, 0.3) to hole (16, 11): the beam passes 0.1 holes or more from every hole
    // between, so the irises go a third and two thirds along, by their nearest holes:
    // (9.33, 9) → (9, 9) and (12.67, 10) → (13, 10).
    let r = turned_to("between", [1.0, 0.3], "hole = [16, 11]");
    assert!((r["ledger"]["detected_w"].as_f64().expect("w") - 0.001).abs() < 1e-12);
    let irises = r["alignment_irises"]["irises"].as_array().expect("irises");
    assert_eq!(irises.len(), 2, "{r}");
    assert!(irises.iter().all(|i| i["on_grid"] == false));
    assert_eq!(irises[0]["hole"], json!([9, 9]));
    assert_eq!(irises[1]["hole"], json!([13, 10]));
}
