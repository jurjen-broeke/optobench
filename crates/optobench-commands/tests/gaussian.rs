//! M9c: Gaussian beams through traced systems, against the textbook results for the
//! complex beam parameter q = z + i z_R, z_R = π w₀² / λ (A. E. Siegman, Lasers, 1986,
//! ch. 17; H. Kogelnik and T. Li, Appl. Opt. 5, 1550 (1966)): free space adds the
//! distance to q, a thin lens subtracts 1/f from 1/q, and w² = −λ / (π Im(1/q)).

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

const LAMBDA: f64 = 0.633e-3; // mm
const W0: f64 = 0.5; // mm

fn layout(label: &str, rest: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("optobench-gaussian-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch");
    let path = dir.join(format!("{label}.optobench"));
    std::fs::write(
        &path,
        format!(
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
             [[source]]\nid = \"laser\"\nkind = \"gaussian\"\nwavelength_um = 0.633\npower_w = 0.01\n\
             waist_diameter = {}\nat = 0\n\n{rest}",
            2.0 * W0
        ),
    )
    .expect("written");
    path
}

fn planes(path: &Path) -> Vec<Value> {
    Registry::standard()
        .run("gaussian", &json!({ "path": path.to_string_lossy() }))
        .unwrap_or_else(|e| panic!("{}", e.message))["planes"]
        .as_array()
        .expect("planes")
        .clone()
}

fn plane<'a>(planes: &'a [Value], id: &str) -> &'a Value {
    planes
        .iter()
        .find(|p| p["id"] == id)
        .unwrap_or_else(|| panic!("{id} in {planes:?}"))
}

fn both(v: &Value, key: &str) -> (f64, f64) {
    (
        v[key][0].as_f64().unwrap_or(f64::INFINITY),
        v[key][1].as_f64().unwrap_or(f64::INFINITY),
    )
}

fn close(a: f64, b: f64, relative: f64) -> bool {
    (a - b).abs() <= relative * b.abs().max(1e-12)
}

#[test]
fn a_beam_spreads_from_its_waist_as_w_of_z_says() {
    let z = 1000.0;
    let path = layout(
        "free",
        &format!("[[detector]]\nid = \"far\"\nat = {z}\nsize = [20, 20]\npixels = [10, 10]\n"),
    );
    let all = planes(&path);
    let far = plane(&all, "far");
    let zr = std::f64::consts::PI * W0 * W0 / LAMBDA;
    let w = W0 * (1.0 + (z / zr).powi(2)).sqrt();
    let r = z + zr * zr / z;
    for (radius, curvature, waist, to_waist) in [0, 1].map(|k| {
        let pick = |key: &str| {
            if k == 0 {
                both(far, key).0
            } else {
                both(far, key).1
            }
        };
        (
            pick("radius_mm"),
            pick("curvature_mm"),
            pick("waist_mm"),
            pick("to_waist_mm"),
        )
    }) {
        assert!(close(radius, w, 1e-6), "{radius} vs {w}");
        assert!(close(curvature, r, 1e-6), "{curvature} vs {r}");
        assert!(close(waist, W0, 1e-6) && close(to_waist, -z, 1e-6), "{far}");
    }
}

#[test]
fn an_ideal_lens_moves_and_shrinks_the_waist_as_the_q_law_says() {
    // Waist at the source, a 100 mm lens 50 mm away: q = 50 + i z_R at the lens,
    // 1/q' = 1/q − 1/f after it; the new waist lies −Re q' further on, of radius
    // √(λ Im q' / π).
    let (at, f) = (50.0, 100.0);
    let zr = std::f64::consts::PI * W0 * W0 / LAMBDA;
    let (qr, qi) = (at, zr);
    let m = qr * qr + qi * qi;
    let (ir, ii) = (qr / m - 1.0 / f, -qi / m); // 1/q'
    let m2 = ir * ir + ii * ii;
    let (q2r, q2i) = (ir / m2, -ii / m2);
    let distance = -q2r;
    let waist = (LAMBDA * q2i / std::f64::consts::PI).sqrt();
    let path = layout(
        "lens",
        &format!(
            "[[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = {at}\nfocal_length = {f}\n\n\
             [[detector]]\nid = \"waist\"\nat = {}\nsize = [5, 5]\npixels = [10, 10]\n",
            at + distance
        ),
    );
    let all = planes(&path);
    let w = plane(&all, "waist");
    let (rx, ry) = both(w, "radius_mm");
    assert!(
        close(rx, waist, 1e-6) && close(ry, waist, 1e-6),
        "{w} vs {waist}"
    );
    let (tx, _) = both(w, "to_waist_mm");
    assert!(tx.abs() < 1e-6, "at the waist: {w}");
    let (cx, _) = both(w, "curvature_mm");
    assert!(cx.abs() > 1e6, "a flat wavefront: {w}");
}

#[test]
fn a_4f_expander_fills_the_objective_pupil_and_the_objective_focuses_the_beam() {
    // L1 (25 mm) one focal length after the waist, L2 (100 mm) 125 mm on, the objective's
    // pupil at L2's back focal plane: the waist imaged there magnified 4×, so the pupil
    // sees w = 4 w₀ with a flat wavefront and a fill of w / (f · NA). The objective then
    // focuses it to the waist λ f / (π w) at its design focus.
    let (f1, f2, f_objective, na) = (25.0, 100.0, 9.0, 0.5);
    let pupil_at = f1 + f1 + f2 + f2;
    let path = layout(
        "expander",
        &format!(
            "[[element]]\nid = \"L1\"\nkind = \"ideal_lens\"\nat = {f1}\nfocal_length = {f1}\n\n\
             [[element]]\nid = \"L2\"\nkind = \"ideal_lens\"\nat = {}\nfocal_length = {f2}\ndiameter = 50\n\n\
             [[element]]\nid = \"OBJ\"\nkind = \"objective\"\nat = {pupil_at}\nfocal_length = {f_objective}\nna = {na}\n\
             pupil_distance = 30.0\nworking_distance = 3.0\n\n\
             [[detector]]\nid = \"focus\"\nat = {}\nsize = [1, 1]\npixels = [10, 10]\n",
            f1 + f1 + f2,
            pupil_at + 33.0
        ),
    );
    let all = planes(&path);
    let pupil = plane(&all, "OBJ");
    let w = W0 * f2 / f1;
    let (rx, ry) = both(pupil, "radius_mm");
    assert!(close(rx, w, 1e-6) && close(ry, w, 1e-6), "{pupil}");
    let fill = pupil["fill"].as_f64().expect("fill");
    assert!(close(fill, w / (f_objective * na), 1e-6), "{fill}");
    let focus = plane(&all, "focus");
    let spot = LAMBDA * f_objective / (std::f64::consts::PI * w);
    let (fx, fy) = both(focus, "radius_mm");
    assert!(
        close(fx, spot, 1e-5) && close(fy, spot, 1e-5),
        "{focus} vs {spot}"
    );
}

#[test]
fn a_fold_mirror_on_the_way_changes_nothing() {
    // The free-space case with a flat mirror folding the beam by 90° halfway: the same
    // radius and curvature at the same path length, in both directions.
    let z = 1000.0;
    let path = layout(
        "fold",
        &format!(
            "[[element]]\nid = \"M\"\nkind = \"mirror\"\nat = 500\nfold = 90\ndiameter = 25.4\nclear_aperture = 22.9\n\n\
             [[detector]]\nid = \"far\"\nat = {z}\nsize = [20, 20]\npixels = [10, 10]\n"
        ),
    );
    let all = planes(&path);
    let far = plane(&all, "far");
    let zr = std::f64::consts::PI * W0 * W0 / LAMBDA;
    let w = W0 * (1.0 + (z / zr).powi(2)).sqrt();
    let (rx, ry) = both(far, "radius_mm");
    assert!(close(rx, w, 1e-6) && close(ry, w, 1e-6), "{far}");
    let (cx, cy) = both(far, "curvature_mm");
    let r = z + zr * zr / z;
    assert!(close(cx, r, 1e-6) && close(cy, r, 1e-6), "{far}");
}
