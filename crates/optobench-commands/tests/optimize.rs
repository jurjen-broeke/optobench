//! M6b: the `optimize` command on cases with exact answers.

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

fn scratch(label: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("optobench-optimize-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("scratch");
    path
}

fn run(command: &str, parameters: Value) -> Result<Value, String> {
    Registry::standard()
        .run(command, &parameters)
        .map_err(|e| format!("{e:?}"))
}

fn optimize(path: &Path, extra: Value) -> Result<Value, String> {
    let mut parameters = json!({
        "path": path.to_string_lossy(),
        "data_directory": scratch("empty").to_string_lossy(),
    });
    if let (Some(p), Some(e)) = (parameters.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    run("optimize", parameters)
}

fn number(v: &Value) -> f64 {
    v.as_f64().expect("a number")
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("written");
    path
}

const HEADER: &str = "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
    [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 4.0\nat = 0\n\n";

#[test]
fn refocuses_onto_the_focus_of_an_aberration_free_lens() {
    // A flat face, then a hyperboloid of conic constant K = −n² (eccentricity n) from
    // glass into air: collimated light inside the glass meets at one point, R/(n − 1)
    // past the vertex (Descartes; Hecht, Optics, 5th ed., ch. 5). With R = −30 mm and
    // n = 1.5 that is 60 mm. The screen starts 5 mm too far.
    let dir = scratch("focus");
    let vertex = 20.0 + 6.0;
    let layout = write(
        &dir,
        "focus.optobench",
        &format!(
            "{HEADER}[[element]]\nid = \"L1\"\nkind = \"lens\"\nat = 20\n\
             surfaces = [{{ radius = \"inf\" }}, {{ radius = -30, conic = -2.25 }}]\nthickness = [6]\nmaterial = [1.5]\n\
             diameter = 25.4\nclear_aperture = 22.9\ncoating = \"ideal\"\n\n\
             [[detector]]\nid = \"screen\"\nat = {}\nsize = [4, 4]\npixels = [20, 20]\n\
             dof = [{{ name = \"screen_z\", kind = \"slide\", axis = \"z\", range = [-10, 10], value = 5 }}]\n\n\
             [[merit]]\nkind = \"focus\"\ndetector = \"screen\"\n",
            vertex + 60.0
        ),
    );
    let result = optimize(&layout, json!({})).expect("optimises");
    assert_eq!(result["converged"], true);
    let dof = &result["dofs"][0];
    assert_eq!(dof["name"], "screen_z");
    assert_eq!(dof["start"], 5.0);
    assert!(number(&dof["suggested"]).abs() < 1e-6, "{dof}");
    let focus = &result["merit"][0];
    assert!(number(&focus["start"][0]) > 0.1);
    assert!(number(&focus["end"][0]) < 1e-6, "{focus}");
}

/// Two fold mirrors (a periscope in the plane): up at 50 mm, back along +x at 150 mm, a
/// screen 100 mm after the second. Both tilt about the scene's z axis.
fn periscope(dir: &Path, m1_range: &str) -> PathBuf {
    write(
        dir,
        "periscope.optobench",
        &format!(
            "{HEADER}[[element]]\nid = \"M1\"\nkind = \"mirror\"\nat = 50\nfold = 90\ndiameter = 25\nclear_aperture = 22\n\
             dof = [{{ name = \"M1_tilt\", kind = \"tilt\", axis = [0, 0, 1], frame = \"scene\", range = {m1_range}, value = 0.3 }}]\n\n\
             [[element]]\nid = \"M2\"\nkind = \"mirror\"\nat = 150\nfold = -90\ndiameter = 25\nclear_aperture = 22\n\
             dof = [{{ name = \"M2_tilt\", kind = \"tilt\", axis = [0, 0, 1], frame = \"scene\", range = [-2, 2] }}]\n\n\
             [[detector]]\nid = \"screen\"\nat = 250\nsize = [40, 40]\npixels = [20, 20]\n\n\
             [[merit]]\nkind = \"centring\"\ndetector = \"screen\"\n\n\
             [[merit]]\nkind = \"pointing\"\ndetector = \"screen\"\n"
        ),
    )
}

#[test]
fn steers_a_periscope_back_onto_its_target() {
    // M1 starts 0.3° off. Position and direction at the screen fix both tilts: the only
    // solution is the nominal one, both at 0.
    let dir = scratch("periscope");
    let layout = periscope(&dir, "[-2, 2]");
    let traced = run(
        "trace",
        json!({ "path": layout.to_string_lossy(), "data_directory": scratch("empty").to_string_lossy() }),
    )
    .expect("traces");
    let offset = &traced["detectors"][0]["direct_centroid_mm"];
    assert!(
        number(&offset[0]).hypot(number(&offset[1])) > 1.0,
        "misaligned at the start"
    );
    let result = optimize(&layout, json!({})).expect("optimises");
    assert_eq!(result["converged"], true);
    for dof in result["dofs"].as_array().expect("dofs") {
        assert!(number(&dof["suggested"]).abs() < 1e-6, "{dof}");
    }
    assert!((number(&result["dofs"][0]["change"]) + 0.3).abs() < 1e-6);
    for term in result["merit"].as_array().expect("terms") {
        for v in term["end"].as_array().expect("residuals") {
            assert!(number(v).abs() < 1e-6, "{term}");
        }
    }
}

#[test]
fn stops_at_a_range_limit_and_says_so() {
    // M1 may not go below 0.2°: the best is at that limit, with M2 doing what it can.
    let dir = scratch("limit");
    let layout = periscope(&dir, "[0.2, 2]");
    let result = optimize(&layout, json!({})).expect("optimises");
    let m1 = &result["dofs"][0];
    assert!((number(&m1["suggested"]) - 0.2).abs() < 1e-9, "{m1}");
    assert_eq!(m1["at_range_limit"], true);
    assert_eq!(result["dofs"][1]["at_range_limit"], false);
    assert!(number(&result["cost"]["end"]) < number(&result["cost"]["start"]));
}

#[test]
fn merit_from_the_command_and_errors() {
    let dir = scratch("errors");
    let layout = periscope(&dir, "[-2, 2]");
    // Only centring, given as a parameter: some tilt pair puts the beam on the centre.
    let result = optimize(
        &layout,
        json!({ "merit": [{ "kind": "centring", "detector": "screen" }] }),
    )
    .expect("optimises");
    assert_eq!(result["merit"].as_array().map(Vec::len), Some(1));
    let c = &result["merit"][0]["end"];
    assert!(number(&c[0]).hypot(number(&c[1])) < 1e-6);
    let error = optimize(&layout, json!({ "merit": [] })).expect_err("no merit");
    assert!(error.contains("no merit terms"), "{error}");
    let error = optimize(
        &layout,
        json!({ "merit": [{ "kind": "focus", "detector": "nowhere" }] }),
    )
    .expect_err("no such detector");
    assert!(error.contains("nowhere"), "{error}");
}
