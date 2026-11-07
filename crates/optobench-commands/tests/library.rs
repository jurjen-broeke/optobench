//! M5c parts library, on the committed golden cage relay (`cage-relay.step`: the
//! plano-convex lens `pc-lens`, R 30 mm, 5 mm thick, Ø25.4, placed twice) and a
//! hand-written part for it in a scratch data directory.

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../optobench-io/tests/step")
}

/// A scratch data directory, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("optobench-parts-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The lens of the cage relay as a catalogue part, curved side first (radius may be
/// changed to make a wrong spec sheet).
fn part_file(radius: f64) -> String {
    format!(
        "format = \"optobench/1\"\nkind = \"part\"\npart = \"PC-30\"\nvendor = \"Example\"\ntype = \"lens\"\n\
         source = \"hand-written for the golden cage relay (crates/optobench-io/tests/step/README.md)\"\n\
         diameter = 25.4\nclear_aperture = 22.9\nsurfaces = [{{ radius = {radius:?} }}, {{ radius = \"inf\" }}]\n\
         thickness = [5.0]\nglass = [1.5]\nproducts = [\"pc-lens\"]\n"
    )
}

fn run(command: &str, parameters: Value) -> Value {
    Registry::standard()
        .run(command, &parameters)
        .unwrap_or_else(|e| panic!("{command}: {e:?}"))
}

fn import(data: &Scratch, radius: f64) {
    let file = data.0.join("pc-30.toml");
    std::fs::write(&file, part_file(radius)).expect("written");
    run(
        "import-part",
        json!({ "path": file.to_string_lossy(), "data_directory": data.path(), "replace": true }),
    );
}

/// The cage relay's sidecar with its elements replaced by `elements`, written to the
/// scratch directory and pointing at the committed STEP file.
fn sidecar(data: &Scratch, elements: &str) -> PathBuf {
    let original =
        std::fs::read_to_string(fixtures().join("cage-relay.optobench")).expect("sidecar");
    let start = original.find("[[element]]").expect("elements");
    let end = original.find("[[source]]").expect("sources");
    let step = fixtures().join("cage-relay.step");
    let text = format!(
        "format = \"optobench/1\"\nkind = \"sidecar\"\nstep = '{}'\n\n{elements}\n{}",
        step.to_string_lossy(),
        &original[end..]
    );
    assert!(start < end);
    let path = data.0.join("relay.optobench");
    std::fs::write(&path, text).expect("written");
    path
}

fn trace(data: &Scratch, path: &Path) -> Value {
    run(
        "trace",
        json!({ "path": path.to_string_lossy(), "data_directory": data.path() }),
    )
}

fn number(v: &Value) -> f64 {
    v.as_f64().expect("a number")
}

#[test]
fn library_lenses_are_tagged_automatically_and_trace_like_explicit_ones() {
    let data = Scratch::new("automatic");
    import(&data, 30.0);
    let listed = run("parts", json!({ "data_directory": data.path() }));
    assert_eq!(listed["parts"][0]["key"], "example/PC-30");

    // No elements at all: both pc-lens occurrences are the library part.
    let automatic = trace(&data, &sidecar(&data, ""));
    let parts = automatic["parts"].as_array().expect("parts");
    assert_eq!(parts.len(), 2);
    for (matched, id) in parts.iter().zip(["pc-lens:1", "pc-lens:2"]) {
        assert_eq!(matched["element"], id);
        assert_eq!(matched["automatic"], true);
        assert_eq!(matched["part"], "example/PC-30");
        // The STEP geometry is the part exactly: nothing to report.
        assert_eq!(matched["bodies"][0]["differences"], json!([]));
        assert_eq!(matched["missing_glasses"], json!([]));
    }
    // Compared in each body's own frame: both occurrences are the same product (flat
    // face at z = 0, curved vertex at z = 5), which is the listed part turned around.
    assert_eq!(parts[0]["bodies"][0]["reversed"], true);
    assert_eq!(parts[1]["bodies"][0]["reversed"], true);

    // The same as the committed sidecar with explicit lenses of n = 1.5.
    let empty = Scratch::new("explicit");
    let explicit = trace(&empty, &fixtures().join("cage-relay.optobench"));
    for key in [
        "direct_power_w",
        "direct_rms_radius_mm",
        "power_w",
        "rms_radius_mm",
    ] {
        let (a, b) = (
            number(&automatic["detectors"][0][key]),
            number(&explicit["detectors"][0][key]),
        );
        assert!((a - b).abs() <= 1e-12 + 1e-9 * b.abs(), "{key}: {a} vs {b}");
    }
    assert_eq!(explicit["parts"], json!([]));

    // `[library] automatic = false` switches it off: the lenses absorb.
    let off = sidecar(&data, "[library]\nautomatic = false\n");
    let absorbed = trace(&data, &off);
    assert_eq!(absorbed["parts"], json!([]));
    assert!(number(&absorbed["detectors"][0]["power_w"]) < 1e-12);
}

#[test]
fn a_wrong_spec_sheet_radius_is_reported() {
    let data = Scratch::new("check");
    // 30.2 mm against the 30 mm of the STEP geometry: 0.2 mm, above the 0.05 mm limit
    // (docs/design.md, "Parts library").
    import(&data, 30.2);
    let elements = "[[element]]\nid = \"L1\"\nbody = { product = \"pc-lens\", occurrence = \"pc-lens:1\" }\nlibrary = \"example/PC-30\"\n";
    let result = trace(&data, &sidecar(&data, elements));
    let parts = result["parts"].as_array().expect("parts");
    // L1 named explicitly, pc-lens:2 tagged automatically.
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["element"], "L1");
    assert_eq!(parts[0]["automatic"], false);
    let difference = &parts[0]["bodies"][0]["differences"][0];
    assert_eq!(difference["quantity"], "radius 1");
    assert!((number(&difference["part_mm"]) - 30.2).abs() < 1e-12);
    assert!((number(&difference["step_mm"]) - 30.0).abs() < 1e-6);
}

#[test]
fn layouts_take_the_prescription_from_the_library() {
    let data = Scratch::new("layout");
    import(&data, 30.0);
    let layout = |element: &str| {
        let text = format!(
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
             [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 0.001\ndiameter = 4.0\nat = 0\n\n\
             [[element]]\nid = \"L1\"\nat = 20\n{element}\n\
             [[detector]]\nid = \"focus\"\nat = 81.666666666667\nsize = [2, 2]\npixels = [20, 20]\n"
        );
        let path = data.0.join("layout.optobench");
        std::fs::write(&path, text).expect("written");
        trace(&data, &path)
    };
    let inline = layout(
        "kind = \"lens\"\nsurfaces = [{ radius = 30 }, { radius = \"inf\" }]\nthickness = [5]\nmaterial = [1.5]\ndiameter = 25.4\nclear_aperture = 22.9\n",
    );
    let from_library = layout("library = \"example/PC-30\"\n");
    for key in ["power_w", "rms_radius_mm"] {
        assert_eq!(
            from_library["detectors"][0][key], inline["detectors"][0][key],
            "{key}"
        );
    }
    // Thick lens, curved side first (Hecht, Optics, 5th ed., ch. 6): back focal length
    // f − t/n = 60 − 5/1.5 = 56.667 mm behind the flat face at 25 mm: a small focus.
    assert!(number(&from_library["detectors"][0]["rms_radius_mm"]) < 0.05);
    // Turned around, the flat side meets the beam first and the focus moves.
    let reversed = layout("library = \"example/PC-30\"\nreversed = true\n");
    assert!(
        number(&reversed["detectors"][0]["rms_radius_mm"])
            > number(&from_library["detectors"][0]["rms_radius_mm"])
    );
}
