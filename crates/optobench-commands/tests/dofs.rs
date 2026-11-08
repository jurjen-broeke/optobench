//! M6a degrees of freedom: DOFs in layout and sidecar files move their items from the
//! nominal pose, as `trace` applies them.

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("optobench-dofs-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("scratch");
    path
}

fn trace(path: &Path, dofs: Value) -> Result<Value, String> {
    Registry::standard()
        .run(
            "trace",
            &json!({
                "path": path.to_string_lossy(),
                "data_directory": scratch("empty").to_string_lossy(),
                "dofs": dofs,
            }),
        )
        .map_err(|e| format!("{e:?}"))
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
    [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 1.0\nat = 0\n\n";

#[test]
fn tilting_a_fold_mirror_turns_the_beam_twice_as_far() {
    // A fold mirror 50 mm along the beam, a screen 100 mm after it. Tilting the mirror by
    // θ about the axis normal to the plane of incidence turns the reflected beam by 2θ
    // (law of reflection; Hecht, Optics, 5th ed., ch. 4), so the spot moves L tan 2θ.
    let dir = scratch("mirror");
    let layout = write(
        &dir,
        "fold.optobench",
        &format!(
            "{HEADER}[[element]]\nid = \"M1\"\nkind = \"mirror\"\nat = 50\nfold = 90\ndiameter = 25\nclear_aperture = 22\n\
             dof = [{{ name = \"M1_tilt\", kind = \"tilt\", axis = [0, 0, 1], frame = \"scene\", range = [-2, 2] }}]\n\n\
             [[detector]]\nid = \"screen\"\nat = 150\nsize = [20, 20]\npixels = [20, 20]\n"
        ),
    );
    let nominal = trace(&layout, json!({})).expect("traces");
    let c = &nominal["detectors"][0]["direct_centroid_mm"];
    assert!(number(&c[0]).abs() < 1e-9 && number(&c[1]).abs() < 1e-9);
    assert_eq!(nominal["dofs"][0]["unit"], "deg");
    for degrees in [0.5_f64, -1.0] {
        let tilted = trace(&layout, json!({ "M1_tilt": degrees })).expect("traces");
        let c = &tilted["detectors"][0]["direct_centroid_mm"];
        let shift = number(&c[0]).hypot(number(&c[1]));
        let expected = 100.0 * (2.0 * degrees).to_radians().tan().abs();
        assert!(
            (shift - expected).abs() < 1e-9,
            "{degrees}°: {shift} vs {expected}"
        );
        assert_eq!(tilted["dofs"][0]["value"], degrees);
    }
    // Outside the mechanical range.
    let error = trace(&layout, json!({ "M1_tilt": 3.0 })).expect_err("out of range");
    assert!(error.contains("outside its range"), "{error}");
    let error = trace(&layout, json!({ "M2": 1.0 })).expect_err("unknown");
    assert!(error.contains("no degree of freedom `M2`"), "{error}");
}

#[test]
fn a_detector_coupled_to_a_lens_keeps_the_focus() {
    // The plano-convex lens (R 30, 5 mm, n 1.5) curved side first: back focal length
    // f − t/n = 60 − 5/1.5 mm from its flat face (Hecht, ch. 6). The screen sits at the
    // focus and follows the lens: sliding both by the same amount changes nothing.
    let dir = scratch("coupled");
    let layout = write(
        &dir,
        "focus.optobench",
        &format!(
            "{HEADER}[[element]]\nid = \"L1\"\nkind = \"lens\"\nat = 20\nsurfaces = [{{ radius = 30 }}, {{ radius = \"inf\" }}]\n\
             thickness = [5]\nmaterial = [1.5]\ndiameter = 25.4\nclear_aperture = 22.9\ncoating = \"ideal\"\n\
             dof = [{{ name = \"L1_z\", kind = \"slide\", axis = \"z\", range = [-10, 10] }}]\n\n\
             [[detector]]\nid = \"focus\"\nat = {}\nsize = [2, 2]\npixels = [20, 20]\n\
             dof = [{{ name = \"focus_z\", kind = \"slide\", axis = \"z\", follows = \"L1_z\", ratio = 1 }}]\n",
            20.0 + 5.0 + 60.0 - 5.0 / 1.5
        ),
    );
    let nominal = trace(&layout, json!({})).expect("traces");
    let moved = trace(&layout, json!({ "L1_z": 4.0 })).expect("traces");
    let rms = |r: &Value| number(&r["detectors"][0]["direct_rms_radius_mm"]);
    assert!(
        (rms(&moved) - rms(&nominal)).abs() < 1e-12,
        "{} vs {}",
        rms(&moved),
        rms(&nominal)
    );
    assert_eq!(moved["dofs"][1]["value"], 4.0, "the follower moved too");
    // A follower cannot be set on its own.
    let error = trace(&layout, json!({ "focus_z": 1.0 })).expect_err("follows");
    assert!(error.contains("follows `L1_z`"), "{error}");
    // Without the coupling, sliding the lens defocuses the screen.
    let alone = write(
        &dir,
        "alone.optobench",
        &std::fs::read_to_string(&layout)
            .expect("written")
            .replace("follows = \"L1_z\", ratio = 1", "locked = true"),
    );
    let defocused = trace(&alone, json!({ "L1_z": 4.0 })).expect("traces");
    assert!(rms(&defocused) > rms(&nominal) + 0.01);
    let error = trace(&alone, json!({ "focus_z": 1.0 })).expect_err("locked");
    assert!(error.contains("locked"), "{error}");
}

#[test]
fn a_step_occurrence_slides_like_the_re_exported_assembly() {
    // cage-relay-moved.step is the golden relay with `pc-lens:2` (and its plate) moved
    // 1 mm along z in SolidCraft and exported again. A slide DOF of +1 mm on that
    // occurrence in the original must trace the same direct image.
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../optobench-io/tests/step");
    let original = std::fs::read_to_string(fixtures.join("cage-relay.optobench")).expect("sidecar");
    let with_dof = original.replace(
        "body = { product = \"pc-lens\", occurrence = \"pc-lens:2\" }\nmaterial = 1.5\n",
        "body = { product = \"pc-lens\", occurrence = \"pc-lens:2\" }\nmaterial = 1.5\n\
         dof = [{ name = \"L2_z\", kind = \"slide\", axis = \"z\", range = [-5, 5] }]\n",
    );
    assert_ne!(with_dof, original);
    let dir = scratch("step");
    let step = |name: &str| fixtures.join(name).to_string_lossy().into_owned();
    let dof_path = write(
        &dir,
        "relay-dof.optobench",
        &with_dof.replacen(
            "kind = \"sidecar\"\n",
            &format!("kind = \"sidecar\"\nstep = '{}'\n", step("cage-relay.step")),
            1,
        ),
    );
    let moved_path = write(
        &dir,
        "relay-moved.optobench",
        &original.replacen(
            "kind = \"sidecar\"\n",
            &format!(
                "kind = \"sidecar\"\nstep = '{}'\n",
                step("cage-relay-moved.step")
            ),
            1,
        ),
    );
    let slid = trace(&dof_path, json!({ "L2_z": 1.0 })).expect("traces");
    let moved = trace(&moved_path, json!({})).expect("traces");
    for key in ["direct_power_w", "direct_rms_radius_mm"] {
        let (a, b) = (
            number(&slid["detectors"][0][key]),
            number(&moved["detectors"][0][key]),
        );
        assert!((a - b).abs() <= 1e-12 + 1e-9 * b.abs(), "{key}: {a} vs {b}");
    }
    for i in 0..2 {
        let (a, b) = (
            number(&slid["detectors"][0]["direct_centroid_mm"][i]),
            number(&moved["detectors"][0]["direct_centroid_mm"][i]),
        );
        assert!((a - b).abs() < 1e-9, "centroid {i}: {a} vs {b}");
    }
    // And the DOF really moved it: unmoved, the image differs.
    let still = trace(&dof_path, json!({})).expect("traces");
    let rms = |r: &Value| number(&r["detectors"][0]["direct_rms_radius_mm"]);
    assert!((rms(&still) - rms(&moved)).abs() > 1e-6);
}
