//! M5c3 exit criterion: a shared design traces identically on a machine without the
//! library. Designs that use a library part, an imported R/T curve and an imported
//! material are embedded (`embed`) and traced again with an empty data directory.

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("optobench-embed-{label}-{}", std::process::id()));
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

fn run(command: &str, parameters: Value) -> Value {
    Registry::standard()
        .run(command, &parameters)
        .unwrap_or_else(|e| panic!("{command}: {e:?}"))
}

fn trace(path: &Path, data: &Scratch) -> Value {
    run(
        "trace",
        json!({ "path": path.to_string_lossy(), "data_directory": data.path() }),
    )
}

/// The lens of the golden cage relay as a part, with a glass by name.
const PART: &str = "format = \"optobench/1\"\nkind = \"part\"\npart = \"PC-30\"\nvendor = \"Example\"\ntype = \"lens\"\n\
    source = \"hand-written test part\"\ndiameter = 25.4\nclear_aperture = 22.9\n\
    surfaces = [{ radius = 30 }, { radius = \"inf\" }]\nthickness = [5]\nglass = [\"test-glass\"]\nproducts = [\"pc-lens\"]\n";

/// A glass of constant index 1.52 in the refractiveindex.info layout (hand-written).
const GLASS: &str = "REFERENCES: |\n    Hand-written test glass.\nDATA:\n  - type: tabulated n\n    data: |\n        0.4 1.52\n        0.8 1.52\n";

/// A data directory with the part, the glass and a 70 % curve.
fn library(label: &str, curve_redistribute: bool) -> Scratch {
    let data = Scratch::new(label);
    let file = |name: &str, text: &str| {
        let path = data.0.join(name);
        std::fs::write(&path, text).expect("written");
        path.to_string_lossy().into_owned()
    };
    run(
        "import-material",
        json!({ "path": file("glass.yml", GLASS), "name": "test-glass", "accept_licence": true,
                "redistribute": true, "data_directory": data.path() }),
    );
    run(
        "import-part",
        json!({ "path": file("part.toml", PART), "data_directory": data.path() }),
    );
    run(
        "import-curve",
        json!({ "path": file("seventy.csv", "Wavelength (nm),% Transmission\n400,70\n700,70\n"),
                "name": "seventy", "redistribute": curve_redistribute, "data_directory": data.path() }),
    );
    data
}

/// A layout: a collimated beam through the library lens with the curve on its flat face,
/// onto a detector near its focus.
fn layout(dir: &Scratch) -> PathBuf {
    let path = dir.0.join("design.optobench");
    std::fs::write(
        &path,
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 6.0\nat = 0\n\n\
         [[element]]\nid = \"L1\"\nat = 20\nlibrary = \"example/PC-30\"\ncoatings = [\"ideal\", { curve = \"seventy\" }]\n\n\
         [[detector]]\nid = \"focus\"\nat = 77\nsize = [4, 4]\npixels = [20, 20]\n",
    )
    .expect("written");
    path
}

fn detector(result: &Value) -> Value {
    let mut d = result["detectors"][0].clone();
    d.as_object_mut().map(|o| o.remove("id"));
    d
}

#[test]
fn an_embedded_layout_traces_identically_without_the_library() {
    let data = library("layout", true);
    let design = layout(&data);
    let before = trace(&design, &data);
    // The curve passes 70 % and the ideal face everything (n = 1.52 between).
    assert!(
        (before["detectors"][0]["direct_power_w"]
            .as_f64()
            .expect("number")
            - 0.7)
            .abs()
            < 1e-12
    );

    let embedded = run(
        "embed",
        json!({ "path": design.to_string_lossy(), "data_directory": data.path() }),
    );
    assert_eq!(embedded["embedded"]["parts"], json!(["example/PC-30"]));
    assert_eq!(embedded["embedded"]["curves"], json!(["seventy"]));
    assert_eq!(embedded["embedded"]["materials"], json!(["test-glass"]));
    assert_eq!(embedded["not_embedded"], json!([]));
    assert_eq!(embedded["self_contained"], true);

    // On a machine without the library: the same result, to the last digit.
    let empty = Scratch::new("layout-empty");
    let after = trace(&design, &empty);
    assert_eq!(detector(&after), detector(&before));
    assert_eq!(after["ledger"], before["ledger"]);
    assert!(
        after["embedded"]
            .as_array()
            .expect("list")
            .iter()
            .all(|e| e["library"] == "absent")
    );
    // With the library, everything is the same as embedded.
    let with = trace(&design, &data);
    assert!(
        with["embedded"]
            .as_array()
            .expect("list")
            .iter()
            .all(|e| e["library"] == "same")
    );

    // Embedding again without the library keeps what was embedded.
    let text = std::fs::read_to_string(&design).expect("written");
    let again = run(
        "embed",
        json!({ "path": design.to_string_lossy(), "data_directory": empty.path() }),
    );
    assert_eq!(again["carried_over"].as_array().map(Vec::len), Some(3));
    assert_eq!(std::fs::read_to_string(&design).expect("written"), text);

    // A changed library part is reported; the design still traces as saved.
    let changed = data.0.join("part.toml");
    std::fs::write(&changed, PART.replace("radius = 30", "radius = 31")).expect("written");
    run(
        "import-part",
        json!({ "path": changed.to_string_lossy(), "replace": true, "data_directory": data.path() }),
    );
    let now = trace(&design, &data);
    let part = now["embedded"]
        .as_array()
        .and_then(|l| l.iter().find(|e| e["kind"] == "part"))
        .expect("the part");
    assert_eq!(part["library"], "differs");
    assert_eq!(detector(&now), detector(&before));
}

#[test]
fn restricted_values_are_embedded_only_with_consent() {
    // Curves are imported with redistribute = false by default.
    let data = library("restricted", false);
    let design = layout(&data);
    let embedded = run(
        "embed",
        json!({ "path": design.to_string_lossy(), "data_directory": data.path() }),
    );
    assert_eq!(embedded["not_embedded"][0]["name"], "seventy");
    assert_eq!(embedded["embedded"]["curves"], json!([]));
    assert_eq!(embedded["self_contained"], false);
    assert!(embedded["needs"].as_str().expect("why").contains("seventy"));
    let consent = run(
        "embed",
        json!({ "path": design.to_string_lossy(), "data_directory": data.path(), "include_restricted": true }),
    );
    assert_eq!(consent["embedded"]["curves"], json!(["seventy"]));
    assert_eq!(consent["self_contained"], true);
    let text = std::fs::read_to_string(&design).expect("written");
    assert!(text.contains("redistribute = false"), "the entry says so");
}

#[test]
fn an_embedded_sidecar_tags_its_lenses_without_the_library() {
    // The golden cage relay with no elements: its lenses are tagged from the library
    // part, then from the embedded one.
    let data = library("sidecar", true);
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../optobench-io/tests/step");
    let original = std::fs::read_to_string(fixtures.join("cage-relay.optobench")).expect("sidecar");
    let tail = &original[original.find("[[source]]").expect("sources")..];
    let design = data.0.join("relay.optobench");
    std::fs::write(
        &design,
        format!(
            "format = \"optobench/1\"\nkind = \"sidecar\"\nstep = '{}'\n\n{tail}",
            fixtures.join("cage-relay.step").to_string_lossy()
        ),
    )
    .expect("written");
    let before = trace(&design, &data);
    assert_eq!(before["parts"].as_array().map(Vec::len), Some(2));
    let embedded = run(
        "embed",
        json!({ "path": design.to_string_lossy(), "data_directory": data.path() }),
    );
    assert_eq!(embedded["embedded"]["parts"], json!(["example/PC-30"]));
    assert_eq!(embedded["embedded"]["materials"], json!(["test-glass"]));
    assert_eq!(embedded["self_contained"], true);
    let empty = Scratch::new("sidecar-empty");
    let after = trace(&design, &empty);
    assert_eq!(after["parts"].as_array().map(Vec::len), Some(2));
    assert_eq!(detector(&after), detector(&before));
}
