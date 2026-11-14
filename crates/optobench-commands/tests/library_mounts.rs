//! Mounts in the parts library (M14's open question, 2026-11-14): a mount's adjusters kept
//! as a part file of `type = "mount"` and named by part number, `mount = "<key>"`. The
//! same mounts written in the design (`[[mount]]`) and taken from the library give the
//! same DOFs and the same trace; `embed` keeps them so the design traces without the
//! library.
//!
//! The specifications are the ones the user gave (2026-11-13), as in `tolerance.rs`: an
//! XY translation mount ±1 mm in X and Y; a translation stage of 12.7 mm; a rotation mount
//! turning freely (coarse) with ±7° fine; a kinematic mirror mount ±4° tip and tilt.

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "optobench-library-mounts-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, text).expect("written");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(command: &str, parameters: Value) -> Result<Value, String> {
    Registry::standard()
        .run(command, &parameters)
        .map_err(|e| e.message)
}

fn ok(command: &str, parameters: Value) -> Value {
    run(command, parameters).unwrap_or_else(|e| panic!("{command}: {e}"))
}

const HEADER: &str =
    "format = \"optobench/1\"\nkind = \"part\"\nvendor = \"Example\"\ntype = \"mount\"\n";

/// The four mounts as part files: (file name, text).
fn mount_files() -> Vec<(&'static str, String)> {
    vec![
        (
            "xy.toml",
            format!(
                "{HEADER}part = \"XY1\"\nsource = \"XY translation mount: ±1 mm in X and Y\"\n\
                 adjusters = [ {{ name = \"x\", kind = \"slide\", axis = \"x\", range = [-1, 1] }},\n\
                               {{ name = \"y\", kind = \"slide\", axis = \"y\", range = [-1, 1] }} ]\n"
            ),
        ),
        (
            "stage.toml",
            format!(
                "{HEADER}part = \"ST12\"\nsource = \"translation stage: 12.7 mm of travel\"\n\
                 adjusters = [ {{ name = \"z\", kind = \"slide\", axis = \"z\", travel = 12.7 }} ]\n"
            ),
        ),
        (
            "rotation.toml",
            format!(
                "{HEADER}part = \"RM1\"\nsource = \"rotation mount: 360° coarse, ±7° fine\"\n\
                 adjusters = [ {{ name = \"coarse\", kind = \"tilt\", axis = \"z\" }},\n\
                               {{ name = \"fine\", kind = \"tilt\", axis = \"z\", range = [-7, 7] }} ]\n"
            ),
        ),
        (
            "kinematic.toml",
            format!(
                "{HEADER}part = \"KM1\"\nsource = \"kinematic mirror mount: ±4° tip and tilt\"\n\
                 adjusters = [ {{ name = \"tip\", kind = \"tilt\", axis = \"x\", range = [-4, 4] }},\n\
                               {{ name = \"tilt\", kind = \"tilt\", axis = \"y\", range = [-4, 4] }} ]\n"
            ),
        ),
    ]
}

/// A data directory with the four mounts imported.
fn library(label: &str) -> Scratch {
    let data = Scratch::new(label);
    for (name, text) in mount_files() {
        let file = data.write(name, &text);
        ok(
            "import-part",
            json!({ "path": file.to_string_lossy(), "data_directory": data.path() }),
        );
    }
    data
}

/// The same mounts written in the design, under the library's keys.
fn own_mounts() -> String {
    let mut text = String::new();
    for (key, (_, file)) in ["example/XY1", "example/ST12", "example/RM1", "example/KM1"]
        .iter()
        .zip(mount_files())
    {
        let source = file
            .lines()
            .find(|l| l.starts_with("source"))
            .expect("source");
        let adjusters = &file[file.find("adjusters").expect("adjusters")..];
        text.push_str(&format!(
            "[[mount]]\nid = \"{key}\"\n{source}\n{adjusters}\n"
        ));
    }
    text
}

/// The mounted layout of `tolerance.rs`, its mounts named by library key.
fn design(mounts: &str, lens_extra: &str) -> String {
    format!(
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n{mounts}\
         [[source]]\nid = \"beam\"\nkind = \"collimated\"\ndiameter = 2.0\nrings = 6\nat = 0\n\
         wavelength_um = 0.633\npower_w = 1.0\nmount = \"example/RM1\"\n\n\
         [[element]]\nid = \"M\"\nkind = \"mirror\"\nat = 30\nfold = 90\ndiameter = 25\nclear_aperture = 24\nmount = \"example/KM1\"\n\n\
         [[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 50\nfocal_length = 100\nmount = \"Example/XY1\"\n{lens_extra}\n\n\
         [[detector]]\nid = \"D\"\nat = 150\nsize = [10, 10]\npixels = [10, 10]\nmount = [\"example/ST12\", \"example/XY1\"]\n\n\
         [[merit]]\nkind = \"centring\"\ndetector = \"D\"\n\n[[merit]]\nkind = \"headroom\"\n"
    )
}

fn trace(path: &Path, data: &Scratch, extra: Value) -> Result<Value, String> {
    let mut p = extra;
    p["path"] = json!(path.to_string_lossy());
    p["data_directory"] = json!(data.path());
    run("trace", p)
}

/// The DOFs without their `source` (which names the library key for library mounts).
fn dofs(result: &Value) -> Vec<Value> {
    result["dofs"]
        .as_array()
        .expect("dofs")
        .iter()
        .map(|d| {
            let mut d = d.clone();
            d.as_object_mut().map(|o| o.remove("source"));
            d
        })
        .collect()
}

#[test]
fn library_mounts_give_the_same_dofs_as_mounts_in_the_design() {
    let data = library("same");
    let listed = ok("parts", json!({ "data_directory": data.path() }));
    let xy = listed["parts"]
        .as_array()
        .expect("parts")
        .iter()
        .find(|p| p["key"] == "example/XY1")
        .expect("the XY mount");
    assert_eq!(xy["type"], "mount");
    assert_eq!(xy["adjusters"][0]["range"], json!([-1.0, 1.0]));
    let stage = listed["parts"]
        .as_array()
        .expect("parts")
        .iter()
        .find(|p| p["key"] == "example/ST12")
        .expect("the stage");
    assert_eq!(stage["adjusters"][0]["range"], json!([-6.35, 6.35]));

    let from_library = data.write("library.optobench", &design("", ""));
    let in_design = data.write("design.optobench", &design(&own_mounts(), ""));
    let a = trace(&from_library, &data, json!({})).expect("traces");
    let b = trace(&in_design, &data, json!({})).expect("traces");
    assert_eq!(dofs(&a), dofs(&b));
    assert_eq!(a["detectors"], b["detectors"]);
    // The warnings alike, but for the source, which names the library key.
    assert_eq!(
        a["warnings"].as_array().map(Vec::len),
        b["warnings"].as_array().map(Vec::len)
    );
    // Every mount's adjusters became DOFs, with their ranges, before any solve.
    let names: Vec<&str> = a["dofs"]
        .as_array()
        .expect("dofs")
        .iter()
        .filter_map(|d| d["name"].as_str())
        .collect();
    for name in [
        "beam_coarse",
        "beam_fine",
        "M_tip",
        "M_tilt",
        "L_x",
        "L_y",
        "D_z",
        "D_x",
        "D_y",
    ] {
        assert!(names.contains(&name), "{name} in {names:?}");
    }
    let lx = a["dofs"]
        .as_array()
        .expect("dofs")
        .iter()
        .find(|d| d["name"] == "L_x")
        .expect("L_x");
    assert_eq!(lx["range"], json!([-1.0, 1.0]));
    let source = lx["source"].as_str().expect("source");
    assert!(
        source.contains("±1 mm in X and Y") && source.contains("example/XY1"),
        "{source}"
    );
    // The ±1 mm slides are warned about, as for mounts in the design.
    assert_eq!(a["warnings"].as_array().map(Vec::len), Some(4), "{a}");

    // An item's own entry still changes the mount's key by key.
    let changed = data.write(
        "changed.optobench",
        &design(
            "",
            "dof = [ { name = \"L_x\", value = 0.25, range = [-0.5, 0.5] } ]",
        ),
    );
    let c = trace(&changed, &data, json!({})).expect("traces");
    let lx = c["dofs"]
        .as_array()
        .expect("dofs")
        .iter()
        .find(|d| d["name"] == "L_x")
        .expect("L_x");
    assert_eq!(lx["value"].as_f64(), Some(0.25));
    assert_eq!(lx["range"], json!([-0.5, 0.5]));
}

#[test]
fn a_mount_in_the_design_is_used_before_the_library_one() {
    let data = library("precedence");
    // The design's own `example/XY1` allows ±2 mm.
    let own = "[[mount]]\nid = \"example/XY1\"\nsource = \"the design's own\"\n\
               adjusters = [ { name = \"x\", kind = \"slide\", axis = \"x\", range = [-2, 2] } ]\n\n";
    let path = data.write("own.optobench", &design(own, ""));
    let r = trace(&path, &data, json!({})).expect("traces");
    let lx = r["dofs"]
        .as_array()
        .expect("dofs")
        .iter()
        .find(|d| d["name"] == "L_x")
        .expect("L_x");
    assert_eq!(lx["range"], json!([-2.0, 2.0]));
    assert_eq!(lx["source"], "the design's own");
    // Only the library's other mounts are used from the library.
    assert!(
        !r["dofs"]
            .as_array()
            .expect("dofs")
            .iter()
            .any(|d| d["name"] == "L_y")
    );
}

#[test]
fn an_embedded_design_keeps_its_library_mounts() {
    let data = library("embed");
    let path = data.write("design.optobench", &design("", ""));
    let before = trace(&path, &data, json!({})).expect("traces");
    let embedded = ok(
        "embed",
        json!({ "path": path.to_string_lossy(), "data_directory": data.path() }),
    );
    let mut parts: Vec<&str> = embedded["embedded"]["parts"]
        .as_array()
        .expect("parts")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    parts.sort_unstable();
    assert_eq!(
        parts,
        ["example/KM1", "example/RM1", "example/ST12", "example/XY1"]
    );
    assert_eq!(embedded["self_contained"], true, "{embedded}");
    // Without the library: the same DOFs and trace.
    let empty = Scratch::new("embed-empty");
    let after = trace(&path, &empty, json!({})).expect("traces without the library");
    assert_eq!(dofs(&after), dofs(&before));
    assert_eq!(after["detectors"], before["detectors"]);
}

#[test]
fn library_mounts_are_checked() {
    let data = library("checked");
    // A name in neither the file nor the library.
    let path = data.write(
        "unknown.optobench",
        &design("", "").replace("example/RM1", "example/RM2"),
    );
    let e = trace(&path, &data, json!({})).expect_err("unknown mount");
    assert!(
        e.contains("no [[mount]] `example/RM2` in the file, and no mount `example/RM2` in the parts library"),
        "{e}"
    );
    // A lens named as a mount, and a mount named as a lens.
    let lens = data.write(
        "lens.toml",
        "format = \"optobench/1\"\nkind = \"part\"\npart = \"PC-30\"\nvendor = \"Example\"\ntype = \"lens\"\n\
         source = \"hand-written test part\"\ndiameter = 25.4\nsurfaces = [{ radius = 30 }, { radius = \"inf\" }]\n\
         thickness = [5]\nglass = [1.5]\n",
    );
    ok(
        "import-part",
        json!({ "path": lens.to_string_lossy(), "data_directory": data.path() }),
    );
    let path = data.write(
        "lens-as-mount.optobench",
        &design("", "").replace("example/RM1", "example/PC-30"),
    );
    let e = trace(&path, &data, json!({})).expect_err("a lens is no mount");
    assert!(e.contains("is a lens, not a mount"), "{e}");
    let path = data.write(
        "mount-as-lens.optobench",
        &design("", "").replace(
            "kind = \"ideal_lens\"\nat = 50\nfocal_length = 100\nmount = \"Example/XY1\"",
            "at = 50\nlibrary = \"example/XY1\"",
        ),
    );
    let e = trace(&path, &data, json!({})).expect_err("a mount is no lens");
    assert!(e.contains("is a mount; give it with `mount = "), "{e}");

    // Part files of mounts are read strictly.
    let bad = |label: &str, change: &dyn Fn(&str) -> String, words: &str| {
        let (_, text) = &mount_files()[0];
        let file = data.write(&format!("{label}.toml"), &change(text));
        let e = run(
            "import-part",
            json!({ "path": file.to_string_lossy(), "data_directory": data.path(), "replace": true }),
        )
        .expect_err(label);
        assert!(e.contains(words), "{label}: {e}");
    };
    bad(
        "optics",
        &|t| format!("{t}diameter = 25.4\n"),
        "did you mean",
    );
    bad(
        "no-adjusters",
        &|t| t[..t.find("adjusters").expect("adjusters")].to_owned() + "adjusters = []\n",
        "at least one adjuster",
    );
    bad(
        "twice",
        &|t| t.replace("name = \"y\"", "name = \"x\""),
        "two adjusters are named `x`",
    );
    bad(
        "both",
        &|t| t.replace("range = [-1, 1] },\n", "range = [-1, 1], travel = 2 },\n"),
        "give `range` or `travel`, not both",
    );
    bad(
        "kind",
        &|t| {
            t.replace(
                "kind = \"slide\", axis = \"x\"",
                "kind = \"rotate\", axis = \"x\"",
            )
        },
        "kind `rotate` is not known",
    );
    bad(
        "pivot",
        &|t| {
            t.replace(
                "axis = \"x\", range",
                "axis = \"x\", pivot = [0, 0, 0], range",
            )
        },
        "a slide has no pivot",
    );
    bad(
        "source",
        &|t| {
            t.replace(
                "source = \"XY translation mount: ±1 mm in X and Y\"",
                "source = \" \"",
            )
        },
        "say where the ranges come from",
    );
    bad(
        "frame",
        &|t| {
            t.replace(
                "axis = \"x\", range",
                "axis = \"x\", frame = \"scene\", range",
            )
        },
        "frame",
    );
}
