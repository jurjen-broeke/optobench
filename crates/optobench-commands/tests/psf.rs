//! M9c2: the vectorial PSF behind an ideal objective, the correction collar, and the exit
//! check against published stratified-media results.
//!
//! Reference values:
//!
//! - The Richards–Wolf field at the focus of an aplanatic objective with a uniformly
//!   filled pupil, linearly polarized along x: only E_x is left there, and with the
//!   pupil power P spread uniformly over p = n sin θ < NA the angular-spectrum form gives
//!   n |E|² = P π n⁴ J² / (NA² λ₀²), J = ∫₀^α √cos θ sin θ (1 + cos θ) dθ
//!   = 2/3 + 2/5 − (2/3) cos^{3/2} α − (2/5) cos^{5/2} α (B. Richards and E. Wolf, Proc. R.
//!   Soc. A 253, 358 (1959), their I₀ at the focus). At low NA this is the Airy peak
//!   P π NA² / λ₀² (Born and Wolf, Principles of Optics, 7th ed., §8.5.2).
//! - The Airy pattern's full width at half maximum, 2 × 1.6163 / (2π) · λ₀ / NA
//!   (where (2 J₁(v) / v)² = ½), and the axial one, where sinc²(u/4) = ½ at
//!   u = 4 × 1.39156 with u = 2π NA² z / (n λ₀) (Born and Wolf, §8.8.1).
//! - Focusing into a mismatched medium: the depth-dependent axial re-scaling factor
//!   ζ = z_A / z_N of S. V. Loginov, D. B. Boltje, M. N. F. Hensgens, J. P. Hoogenboom and
//!   E. B. van der Wee, "Depth-dependent scaling of axial distances in light
//!   microscopy", Optica 11, 553 (2024), Eqs. (3), (13) and (15), with its geometric
//!   bounds ζ_paraxial = n₂ / n₁ (Eq. 1) and ζ_marginal = √((n₂² − NA²) / (n₁² − NA²))
//!   (Eq. 2, after Visser et al.). Their analytical expression follows their vectorial
//!   wave-optics calculation to about 2 % beyond 15 µm depth (their Figs. 5 and 6), which
//!   sets the tolerance here.

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("optobench-psf-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("scratch");
    path
}

/// A planar layout: a collimated beam of `diameter` at λ `wavelength` into an objective
/// (pupil plane at 50, front plane at 80), then `rest`.
fn layout(label: &str, wavelength: f64, diameter: f64, objective: &str, rest: &str) -> PathBuf {
    let dir = scratch(label);
    let path = dir.join(format!("{label}.optobench"));
    let text = format!(
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = {wavelength}\npower_w = 1.0\n\
         diameter = {diameter}\nrings = 4\nat = 0\n\n\
         [[element]]\nid = \"OBJ\"\nkind = \"objective\"\nat = 50\npupil_distance = 30.0\n{objective}\n\n{rest}"
    );
    std::fs::write(&path, text).expect("written");
    path
}

fn run(command: &str, path: &Path, extra: Value) -> Value {
    let mut parameters = json!({
        "path": path.to_string_lossy(),
        "data_directory": scratch("empty").to_string_lossy(),
    });
    if let (Some(map), Some(more)) = (parameters.as_object_mut(), extra.as_object()) {
        map.extend(more.clone());
    }
    Registry::standard()
        .run(command, &parameters)
        .unwrap_or_else(|e| panic!("{}", e.message))
}

fn number(v: &Value, key: &str) -> f64 {
    v[key]
        .as_f64()
        .unwrap_or_else(|| panic!("`{key}` in {}", v["peak_mm"]))
}

/// Richards and Wolf's peak n |E|² for a uniform pupil (W/mm²).
fn richards_wolf_peak(power: f64, na: f64, n: f64, wavelength_um: f64) -> f64 {
    let c = (1.0 - (na / n).powi(2)).sqrt();
    let j = 2.0 / 3.0 + 2.0 / 5.0 - 2.0 / 3.0 * c.powf(1.5) - 2.0 / 5.0 * c.powf(2.5);
    let lambda = wavelength_um * 1e-3;
    power * core::f64::consts::PI * n.powi(4) * j * j / (na * na * lambda * lambda)
}

#[test]
fn a_low_na_objective_focuses_an_airy_pattern() {
    // Dry, NA 0.1, f = 10 mm: the pupil (radius 1 mm) overfilled; the screen at the focus.
    let path = layout(
        "airy",
        0.55,
        2.4,
        "focal_length = 10.0\nna = 0.1\nworking_distance = 5.0",
        "[[detector]]\nid = \"focus\"\nat = 85.0\nsize = [1, 1]\npixels = [10, 10]\n",
    );
    let report = run("psf", &path, json!({ "samples": 160 }));
    let (na, lambda) = (0.1, 0.55e-3);
    let power = number(&report, "power_w");
    let peak = number(&report, "peak_w_per_mm2");
    let expected = richards_wolf_peak(power, na, 1.0, 0.55);
    assert!((peak / expected - 1.0).abs() < 2e-3, "{peak} vs {expected}");
    // At low NA, the Airy peak.
    let airy = power * core::f64::consts::PI * na * na / (lambda * lambda);
    assert!((peak / airy - 1.0).abs() < 3e-3, "{peak} vs {airy}");
    // At the focus, perfect.
    let z = report["peak_mm"][2].as_f64().expect("z");
    assert!(z.abs() < 1e-6, "{z}");
    assert!(number(&report, "strehl") > 1.0 - 1e-9);
    let lateral = 2.0 * 1.6163 / (2.0 * core::f64::consts::PI) * lambda / na;
    for key in ["fwhm_x_mm", "fwhm_y_mm"] {
        let width = number(&report, key);
        assert!(
            (width / lateral - 1.0).abs() < 5e-3,
            "{key}: {width} vs {lateral}"
        );
    }
    let axial = 2.0 * 4.0 * 1.39156 * lambda / (2.0 * core::f64::consts::PI * na * na);
    let width = number(&report, "fwhm_axial_mm");
    assert!((width / axial - 1.0).abs() < 5e-3, "{width} vs {axial}");
    // The PSF holds the power through the pupil: a uniform 2.4 mm beam on a 2 mm pupil
    // passes (1 / 1.2)² of it, to the grid's sampling of the rim.
    let fraction = (1.0f64 / 1.2).powi(2);
    assert!(
        (power / fraction - 1.0).abs() < 3e-3,
        "{power} vs {fraction}"
    );
    // The trace conserves power (launched = detected + clipped).
    let trace = run("trace", &path, json!({}));
    let ledger = &trace["ledger"];
    let detected = ledger["detected_w"].as_f64().expect("detected");
    assert!(
        (ledger["launched_w"].as_f64().expect("launched")
            - detected
            - ledger["clipped_w"].as_f64().expect("clipped"))
        .abs()
            < 1e-9,
        "{ledger}"
    );
}

#[test]
fn a_high_na_focus_has_the_richards_wolf_peak_and_widens_along_the_polarization() {
    // Water dipping, NA 1.2 (n = 1.333), f = 5 mm, matched: the screen at the focus.
    let n = 1.333;
    let path = layout(
        "dipping",
        0.55,
        12.4,
        "focal_length = 5.0\nna = 1.2\nworking_distance = 2.5\nimmersion = 1.333",
        "[[element]]\nid = \"W\"\nkind = \"sample\"\nat = 80.5\nabove = 1.333\n\
         layers = [ { thickness = 10.0, material = 1.333 } ]\n\n\
         [[detector]]\nid = \"focus\"\nat = 82.5\nsize = [1, 1]\npixels = [10, 10]\n",
    );
    let x = run("psf", &path, json!({ "samples": 160, "polarization": "x" }));
    let power = number(&x, "power_w");
    let peak = number(&x, "peak_w_per_mm2");
    let expected = richards_wolf_peak(power, 1.2, n, 0.55);
    assert!((peak / expected - 1.0).abs() < 2e-3, "{peak} vs {expected}");
    assert!(number(&x, "strehl") > 1.0 - 1e-9);
    assert!(x["peak_mm"][2].as_f64().expect("z").abs() < 1e-6);
    // The focus is longer along the polarization (the E_z lobes either side of it),
    // and turning the polarization turns the spot.
    let (wx, wy) = (number(&x, "fwhm_x_mm"), number(&x, "fwhm_y_mm"));
    assert!(wx > 1.2 * wy, "{wx} {wy}");
    let y = run("psf", &path, json!({ "samples": 160, "polarization": "y" }));
    assert!((number(&y, "fwhm_y_mm") - wx).abs() < 1e-12);
    assert!((number(&y, "fwhm_x_mm") - wy).abs() < 1e-12);
    // Circular and radial light are round.
    for polarization in ["circular", "radial"] {
        let r = run(
            "psf",
            &path,
            json!({ "samples": 160, "polarization": polarization }),
        );
        let (a, b) = (number(&r, "fwhm_x_mm"), number(&r, "fwhm_y_mm"));
        assert!((a - b).abs() < 1e-3 * a, "{polarization}: {a} {b}");
    }
}

/// Loginov et al. 2024: ζ at actual depth `z_a` (mm), Eq. (3) from Eqs. (13) and (15).
fn loginov(na: f64, n1: f64, n2: f64, wavelength_um: f64, z_a: f64) -> f64 {
    let lambda = wavelength_um * 1e-3;
    let m2 = n2 * n2 - n1 * n1;
    let sign = if n2 >= n1 { 1.0 } else { -1.0 };
    let e = lambda / 4.0 / (z_a * n2) * sign;
    // m √(ε (2 − ε)): for n₁ > n₂, m and the root are both imaginary and their product is
    // real (§2.3); its sign is the one that keeps ζ below n₂ / n₁, as in their Fig. 5b.
    let root = sign * (m2 * e * (2.0 - e)).sqrt();
    let universal = n2 / n1 * ((1.0 - e) + root / n1) / (1.0 - (n2 / n1).powi(2) * e * (2.0 - e));
    let critical = loginov_critical(na, n1, n2);
    if n2 >= n1 {
        universal.min(critical)
    } else {
        universal.max(critical)
    }
}

/// Loginov et al. 2024, Eq. (15): ζ_crit, the real part of a ratio whose roots turn
/// imaginary beyond NA = n.
fn loginov_critical(na: f64, n1: f64, n2: f64) -> f64 {
    let root_of = |n: f64| -> (f64, f64) {
        let d = n * n - na * na;
        if d >= 0.0 {
            (d.sqrt(), 0.0)
        } else {
            (0.0, (-d).sqrt())
        }
    };
    let (a_re, a_im) = root_of(n1);
    let (b_re, b_im) = root_of(n2);
    let (num_re, num_im) = (n1 - a_re, -a_im);
    let (den_re, den_im) = (n2 - b_re, -b_im);
    (num_re * den_re + num_im * den_im) / (den_re * den_re + den_im * den_im)
}

#[test]
fn loginov_reference_values_are_reproduced() {
    // The values quoted in the paper's text: Figure 1c (NA 0.95, 1.0 → 1.33) has
    // ζ_paraxial = 1.33 and ζ_marginal = 2.98; for NA 1.4 from oil into water the shallow
    // plateau is ζ_crit = 0.64 (§4.2.2), with the indices they measured at 520 nm,
    // n₁ = 1.5197 and n₂ = 1.3421 (§3.4). Their analytic ζ at great depth tends to n₂ / n₁
    // (§2.4).
    let marginal: f64 = ((1.33f64.powi(2) - 0.95f64.powi(2)) / (1.0 - 0.95f64.powi(2))).sqrt();
    assert!((marginal - 2.98).abs() < 5e-3, "{marginal}");
    let critical = loginov_critical(1.4, 1.5197, 1.3421);
    assert!((critical - 0.64).abs() < 5e-3, "{critical}");
    assert!((loginov(0.85, 1.0, 1.34, 0.52, 10.0) / 1.34 - 1.0).abs() < 0.01);
}

/// The PSF peak's depth below the interface for a nominal depth `z_n`, and ζ = z_A / z_N.
fn scaling(path: &Path, z_n: f64, extra: Value) -> (f64, f64) {
    let report = run("psf", path, extra);
    let z_a = z_n + report["peak_mm"][2].as_f64().expect("z");
    (z_a, z_a / z_n)
}

#[test]
fn exit_check_a_dry_objective_focusing_into_water_matches_the_stratified_media_scaling() {
    // Dry NA 0.85 (f = 10 mm, designed for air, 3 mm working distance) with water
    // (n₂ = 1.34) 40 µm above its design focus, λ = 0.52 µm: the case of Loginov et al.,
    // Fig. 6. The screen is at the design focus, so the peak's z there is z_A − z_N.
    let (na, n1, n2, lambda, z_n): (f64, f64, f64, f64, f64) = (0.85, 1.0, 1.34, 0.52, 0.040);
    let path = layout(
        "dry-water",
        lambda,
        17.4,
        "focal_length = 10.0\nna = 0.85\nworking_distance = 3.0",
        &format!(
            "[[element]]\nid = \"W\"\nkind = \"sample\"\nat = {}\nlayers = [ {{ thickness = 5.0, material = {n2} }} ]\n\n\
             [[detector]]\nid = \"focus\"\nat = 83.0\nsize = [1, 1]\npixels = [10, 10]\n",
            83.0 - z_n
        ),
    );
    let (z_a, zeta) = scaling(&path, z_n, json!({ "samples": 256 }));
    let marginal = ((n2 * n2 - na * na) / (n1 * n1 - na * na)).sqrt();
    assert!(n2 / n1 < zeta && zeta < marginal, "ζ = {zeta}");
    let expected = loginov(na, n1, n2, lambda, z_a);
    assert!(
        (zeta / expected - 1.0).abs() < 0.02,
        "ζ = {zeta}, Eq. (3) {expected}"
    );
}

#[test]
fn exit_check_an_oil_objective_focusing_into_water_matches_the_stratified_media_scaling() {
    // Oil NA 1.4 (n₁ = 1.52, f = 10 mm) with water (n₂ = 1.34) 40 µm above its design
    // focus, λ = 0.52 µm (Loginov et al., Fig. 7): light beyond n sin θ = 1.34 is
    // totally reflected, and the focus comes up, ζ < n₂ / n₁.
    let (na, n1, n2, lambda, z_n): (f64, f64, f64, f64, f64) = (1.4, 1.52, 1.34, 0.52, 0.040);
    let path = layout(
        "oil-water",
        lambda,
        28.4,
        &format!("focal_length = 10.0\nna = {na}\nworking_distance = 0.15\nimmersion = {n1}"),
        &format!(
            "[[element]]\nid = \"W\"\nkind = \"sample\"\nat = {}\nabove = {n1}\nlayers = [ {{ thickness = 5.0, material = {n2} }} ]\n\n\
             [[detector]]\nid = \"focus\"\nat = 80.15\nsize = [1, 1]\npixels = [10, 10]\n",
            80.15 - z_n
        ),
    );
    let (z_a, zeta) = scaling(
        &path,
        z_n,
        json!({ "samples": 256, "axial": [-0.015, 0.002], "points": 341 }),
    );
    assert!(zeta < n2 / n1, "ζ = {zeta}");
    let expected = loginov(na, n1, n2, lambda, z_a);
    assert!(
        (zeta / expected - 1.0).abs() < 0.02,
        "ζ = {zeta}, Eq. (3) {expected}"
    );
}

#[test]
fn the_correction_collar_restores_a_perfect_focus_through_a_thicker_cover_glass() {
    // A water objective (NA 1.0, f = 10 mm) corrected for 0.17 mm of glass (n = 1.5255)
    // images through 0.20 mm: set to +0.03 mm, its collar corrects for the glass that is
    // there, and every ray meets at the new design focus, 0.05 mm before the screen.
    let path = layout(
        "collar",
        0.55,
        19.9,
        "focal_length = 10.0\nna = 1.0\nworking_distance = 2.0\nimmersion = 1.333\n\
         cover_glass = { thickness = 0.17, material = 1.5255 }\n\
         dof = [ { name = \"OBJ_collar\", kind = \"collar\", range = [-0.05, 0.05] } ]",
        "[[element]]\nid = \"S\"\nkind = \"sample\"\nat = 82.0\nabove = 1.333\n\
         layers = [ { thickness = 0.20, material = 1.5255 }, { thickness = 5.0, material = 1.333 } ]\n\n\
         [[detector]]\nid = \"focus\"\nat = 82.25\nsize = [1, 1]\npixels = [10, 10]\n",
    );
    let set = json!({ "dofs": { "OBJ_collar": 0.03 } });
    let focus = run("focus", &path, set.clone());
    for c in focus["crossings"].as_array().expect("crossings") {
        if c["height_mm"].as_f64().expect("height") > 1e-9 {
            let z = c["z_mm"].as_f64().expect("z");
            assert!((z + 0.05).abs() < 1e-9, "{z}");
        }
    }
    let unset = run("focus", &path, json!({}));
    assert!(
        number(&unset, "best_rms_mm") > 1e-4,
        "{}",
        unset["best_rms_mm"]
    );

    let mut corrected = set.clone();
    corrected["samples"] = json!(128);
    let psf = run("psf", &path, corrected);
    assert!(number(&psf, "strehl") > 1.0 - 1e-6, "{}", psf["strehl"]);
    assert!((psf["peak_mm"][2].as_f64().expect("z") + 0.05).abs() < 1e-6);
    // Without it, 0.03 mm of glass too much costs NA 1.0 a third of its peak.
    let off = run("psf", &path, json!({ "samples": 128 }));
    assert!(number(&off, "strehl") < 0.8, "{}", off["strehl"]);

    // The DOF is listed as a collar, in mm.
    let trace = run("trace", &path, json!({}));
    let dof = &trace["dofs"][0];
    assert_eq!(dof["kind"], "collar", "{trace}");
    assert_eq!(dof["unit"], "mm");
}

#[test]
fn a_collar_has_no_axis_and_needs_a_cover_glass() {
    let axis = layout(
        "collar-axis",
        0.55,
        2.0,
        "focal_length = 10.0\nna = 0.1\nworking_distance = 2.0\n\
         cover_glass = { thickness = 0.17, material = 1.5255 }\n\
         dof = [ { name = \"c\", kind = \"collar\", axis = \"z\" } ]",
        "",
    );
    let no_glass = layout(
        "collar-glass",
        0.55,
        2.0,
        "focal_length = 10.0\nna = 0.1\nworking_distance = 2.0\n\
         dof = [ { name = \"c\", kind = \"collar\", value = 0.01 } ]",
        "",
    );
    for (path, words) in [(axis, "no `axis`"), (no_glass, "no design cover glass")] {
        let error = Registry::standard()
            .run(
                "trace",
                &json!({ "path": path.to_string_lossy(), "data_directory": scratch("empty").to_string_lossy() }),
            )
            .expect_err("refused");
        assert!(error.message.contains(words), "{}", error.message);
    }
}
