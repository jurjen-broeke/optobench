//! M9b exit check: a galvo scanning through a 4f scan/tube relay into an ideal objective,
//! against the hand calculation.
//!
//! The relay is two ideal thin lenses (a ray at height h has its slope changed by −h/f):
//! the galvo at the front focal plane of the scan lens (f₁), the tube lens (f₂) f₁ + f₂
//! further, the objective's pupil at the tube lens's back focal plane. Turning the galvo
//! by α turns the beam by 2α (reflection), so its chief ray leaves with slope
//! t₀ = tan 2α and, by the thin-lens rule and straight propagation (Hecht, Optics, 5th
//! ed., ch. 5–6, the 4f system):
//!
//! - at the scan lens: height f₁ t₀; leaving it, slope 0 (telecentric between the lenses);
//! - at the tube lens: height f₁ t₀, slope 0;
//! - at the pupil: height 0 (no pupil walk: the galvo is imaged onto the pupil), slope
//!   −(f₁/f₂) t₀;
//! - after the objective (f): at f · slope off the axis in its focal plane, its chief ray
//!   parallel to the axis (telecentric).

use std::path::PathBuf;

use optobench_commands::Registry;
use serde_json::{Value, json};

const F1: f64 = 50.0;
const F2: f64 = 200.0;
const F_OBJECTIVE: f64 = 9.0;

fn relay() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("optobench-scan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch");
    let path = dir.join("relay.optobench");
    let galvo = 50.0;
    let scan = galvo + F1;
    let tube = scan + F1 + F2;
    let pupil = tube + F2;
    let text = format!(
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 1.0\nrings = 4\nat = 0\n\n\
         [[element]]\nid = \"G\"\nkind = \"mirror\"\nat = {galvo}\nfold = 90\ndiameter = 10\nclear_aperture = 9\n\
         dof = [ {{ name = \"scan\", kind = \"tilt\", axis = [0, 0, 1], frame = \"scene\", range = [-10, 10] }} ]\n\n\
         [[element]]\nid = \"SL\"\nkind = \"ideal_lens\"\nat = {scan}\nfocal_length = {F1}\n\n\
         [[element]]\nid = \"TL\"\nkind = \"ideal_lens\"\nat = {tube}\nfocal_length = {F2}\ndiameter = 50\n\n\
         [[element]]\nid = \"OBJ\"\nkind = \"objective\"\nat = {pupil}\nfocal_length = {F_OBJECTIVE}\nna = 0.3\n\
         pupil_distance = 30.0\nworking_distance = 5.0\n\n\
         [[detector]]\nid = \"image\"\nat = {image}\nsize = [10, 10]\npixels = [100, 100]\n",
        image = pupil + 35.0,
    );
    std::fs::write(&path, text).expect("written");
    path
}

fn pair(v: &Value) -> (f64, f64) {
    (v[0].as_f64().expect("x"), v[1].as_f64().expect("y"))
}

fn hit<'a>(chief: &'a Value, element: &str) -> &'a Value {
    chief["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .find(|h| h["element"] == element)
        .unwrap_or_else(|| panic!("the chief ray meets {element}: {chief}"))
}

#[test]
fn a_galvo_through_a_4f_relay_scans_as_the_hand_calculation_says() {
    let path = relay();
    let swept = Registry::standard()
        .run(
            "sweep",
            &json!({ "path": path.to_string_lossy(), "dof": "scan", "from": -2.0, "to": 2.0, "steps": 9 }),
        )
        .unwrap_or_else(|e| panic!("{}", e.message));
    let steps = swept["steps"].as_array().expect("steps");
    assert_eq!(steps.len(), 9);
    const EPS: f64 = 1e-9;
    for step in steps {
        let alpha = step["value"].as_f64().expect("angle").to_radians();
        let chief = &step["chief"][0];
        // Into the scan lens (slopes as the ray arrives): the beam turned by 2α, in the
        // layout plane, nothing across it.
        let scan = hit(chief, "SL");
        let (s0, across) = pair(&scan["slope"]);
        let t0 = (2.0 * alpha).tan();
        assert!(
            (s0.abs() - t0.abs()).abs() < EPS && across.abs() < EPS,
            "α {alpha}: {scan}"
        );
        let (h_scan, _) = pair(&scan["position_mm"]);
        assert!((h_scan - F1 * s0).abs() < EPS, "{h_scan} vs {}", F1 * s0);
        // Between the lenses the chief ray runs parallel to the axis.
        let tube = hit(chief, "TL");
        let (h_tube, _) = pair(&tube["position_mm"]);
        let (s_tube, _) = pair(&tube["slope"]);
        assert!(
            (h_tube - h_scan).abs() < EPS && s_tube.abs() < EPS,
            "{tube}"
        );
        // At the pupil: no walk, the slope scaled by −f₁/f₂.
        let pupil = hit(chief, "OBJ");
        let walk = pair(&pupil["position_mm"]);
        assert!(walk.0.hypot(walk.1) < EPS, "pupil walk {walk:?}");
        let (s_pupil, _) = pair(&pupil["slope"]);
        assert!(
            (s_pupil + F1 / F2 * s0).abs() < EPS,
            "{s_pupil} vs {}",
            -F1 / F2 * s0
        );
        // At the image: f · slope off the axis, telecentric, and the whole beam there.
        let landed = &chief["landed"];
        let (x, y) = pair(&landed["position_mm"]);
        assert!(
            (x - F_OBJECTIVE * s_pupil).abs() < EPS && y.abs() < EPS,
            "{landed}"
        );
        let slope = pair(&landed["slope"]);
        assert!(slope.0.hypot(slope.1) < EPS, "telecentric: {slope:?}");
        let detector = &step["detectors"][0];
        let (cx, cy) = pair(&detector["direct_centroid_mm"]);
        assert!((cx - x).abs() < EPS && (cy - y).abs() < EPS, "{detector}");
        assert!(
            detector["direct_rms_radius_mm"].as_f64().expect("rms") < 1e-9,
            "a sharp focus"
        );
    }
    // The scan is linear in tan 2α and passes through the axis: ±2° gives ±f (f₁/f₂) tan 4°.
    let ends: Vec<f64> = [0, 8]
        .iter()
        .map(|&i| pair(&steps[i]["chief"][0]["landed"]["position_mm"]).0)
        .collect();
    let reach = F_OBJECTIVE * F1 / F2 * (4.0_f64.to_radians()).tan();
    assert!((ends[0].abs() - reach).abs() < EPS && (ends[1].abs() - reach).abs() < EPS);
    assert!(ends[0] * ends[1] < 0.0, "opposite sides");
}

#[test]
fn an_ideal_lens_focuses_a_collimated_beam_at_its_focal_length() {
    // A perfect thin lens: every ray of a collimated beam crosses the axis f behind it.
    let dir = std::env::temp_dir().join(format!("optobench-ideal-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch");
    let path = dir.join("ideal.optobench");
    std::fs::write(
        &path,
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 20.0\nrings = 6\nat = 0\n\n\
         [[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 10\nfocal_length = 40\n\n\
         [[detector]]\nid = \"screen\"\nat = 45\nsize = [10, 10]\npixels = [10, 10]\n",
    )
    .expect("written");
    let focus = Registry::standard()
        .run("focus", &json!({ "path": path.to_string_lossy() }))
        .unwrap_or_else(|e| panic!("{}", e.message));
    for c in focus["crossings"].as_array().expect("crossings") {
        if c["height_mm"].as_f64().expect("h") > 0.0 {
            assert!((c["z_mm"].as_f64().expect("z") - 5.0).abs() < 1e-9, "{c}");
        }
    }
}
