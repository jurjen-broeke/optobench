//! M10c: rays to fields and back, fibre coupling, and the M10 exit check.
//!
//! Reference values:
//!
//! - Two Gaussian modes of 1/e² radii w₁, w₂ in one plane, flat and on one axis, overlap
//!   by (2 w₁ w₂ / (w₁² + w₂²))² (e.g. A. W. Snyder and J. D. Love, Optical Waveguide
//!   Theory, 1983, §20; H. Kogelnik, in Integrated Optics, 1975).
//! - A Gaussian beam's waist behind a lens by the q law: q = z − i z_R, 1/q' = 1/q − 1/f,
//!   the waist −Re q' on, of radius √(λ z_R' / π), z_R' = −Im q' (Kogelnik and Li 1966).
//! - The ray trace itself, for rays → field → rays with nothing in the way.
//! - The vectorial PSF of M9c2 (commands `tests/psf.rs`) at NA 0.1, for a field focused
//!   by an ideal lens of the same aperture and focal length.
//! - A Gaussian (field E₀ (q₀/q) e^{i k r²/(2q)}) clipped at radius a and carried a
//!   distance D: on the axis U = (π E₀ / (λ D)) |q₀/q| |(e^{β a²} − 1) / β|,
//!   β = i k (1/(2q) + 1/(2D)) (Fresnel's integral over the disk, Goodman §4.2), and the
//!   power through the iris 1 − exp(−2a²/w²).
//!
//! Set `OPTOBENCH_GRAPH_OUT` to a directory to keep the SVG files.

use std::path::PathBuf;

use optobench_commands::Registry;
use optobench_field::{Field, rays::coupling};
use optobench_math::Complex;
use serde_json::{Value, json};

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("optobench-field-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("scratch");
    path
}

fn write(label: &str, body: &str) -> PathBuf {
    let path = scratch(label).join(format!("{label}.optobench"));
    std::fs::write(
        &path,
        format!("format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n{body}"),
    )
    .expect("written");
    path
}

fn run(command: &str, parameters: Value) -> Value {
    let mut parameters = parameters;
    parameters["data_directory"] = json!(scratch("empty").to_string_lossy());
    Registry::standard()
        .run(command, &parameters)
        .unwrap_or_else(|e| panic!("{}", e.message))
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("a number: {v}"))
}

/// Where SVG files go: `OPTOBENCH_GRAPH_OUT` if set, else a scratch directory.
fn out(name: &str) -> String {
    let dir = std::env::var_os("OPTOBENCH_GRAPH_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| scratch("out"));
    std::fs::create_dir_all(&dir).expect("directory");
    dir.join(name).to_string_lossy().into_owned()
}

#[test]
fn two_gaussian_modes_overlap_as_the_closed_form_says() {
    let x = (Complex::ONE, Complex::ZERO);
    for (w1, w2) in [(0.005, 0.005), (0.004, 0.006), (0.003, 0.009)] {
        let beam = Field::gaussian(256, 0.0002, 1.55, 1.0, w1, 0.0, 1.0, x).expect("field");
        let got = coupling(&beam, w2, (0.0, 0.0));
        let expected = (2.0 * w1 * w2 / (w1 * w1 + w2 * w2)).powi(2);
        assert!(
            (got - expected).abs() < 1e-7,
            "{w1} {w2}: {got} vs {expected}"
        );
    }
}

#[test]
fn exit_check_a_focused_gaussian_couples_into_a_fibre_as_the_q_law_says() {
    // w₀ = 1 mm at λ = 0.633 µm, a 50 mm ideal lens 100 mm on: the fibre sits at the
    // q-law waist. A fibre whose mode matches the waist takes everything; one of a
    // different size takes (2 w w_f / (w² + w_f²))².
    let (w0, lambda, f, at) = (1.0, 0.633e-3, 50.0, 100.0);
    let rayleigh = core::f64::consts::PI * w0 * w0 / lambda;
    let q = Complex::new(at, -rayleigh);
    let q2 = Complex::ONE / (Complex::ONE / q - Complex::real(1.0 / f));
    let (to_waist, rayleigh2) = (-q2.re, -q2.im);
    let waist = (lambda * rayleigh2 / core::f64::consts::PI).sqrt();
    let path = write(
        "fibre",
        &format!(
            "[[source]]\nid = \"laser\"\nkind = \"gaussian\"\nwavelength_um = 0.633\npower_w = 0.001\nwaist_diameter = {}\nat = 0\n\n\
             [[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = {at}\nfocal_length = {f}\ndiameter = 25.4\n\n\
             [[detector]]\nid = \"fibre\"\nat = {}\nsize = [0.2, 0.2]\npixels = [20, 20]\n",
            2.0 * w0,
            at + to_waist
        ),
    );
    for mode in [waist, 1.5 * waist] {
        let result = run(
            "field",
            json!({ "path": path.to_string_lossy(), "target": "fibre", "padding": 4,
                    "fibre": { "mode_field_diameter": 2.0 * mode } }),
        );
        let expected = (2.0 * waist * mode / (waist * waist + mode * mode)).powi(2);
        let got = number(&result["coupling"]);
        assert!(
            (got - expected).abs() < 1e-4,
            "mode {mode}: {got} vs {expected}"
        );
        assert!(
            (number(&result["power_w"]) - 0.001).abs() < 1e-9,
            "{result}"
        );
    }
}

#[test]
fn exit_check_rays_to_a_field_and_back_reproduce_the_ray_trace() {
    // A 4 mm beam, a 100 mm ideal lens at 50: the rays become a field at 60, it is carried
    // to 80 (Fresnel number a² / (λ z) in the hundreds: geometric), becomes rays again,
    // and they reach the screen at 100 as the plain trace's rays do. There the beam is 1 mm
    // in radius, 50 mm before its focus (Fresnel number 36), so diffraction at its edge
    // hardly moves the second moments; nearer the focus it would, and rightly.
    let path = write(
        "round-trip",
        "[[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 4.0\nrings = 24\nat = 0\n\n\
         [[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 50\nfocal_length = 100\ndiameter = 25.4\n\n\
         [[detector]]\nid = \"A\"\nat = 60\nsize = [10, 10]\npixels = [10, 10]\n\n\
         [[detector]]\nid = \"B\"\nat = 80\nsize = [10, 10]\npixels = [10, 10]\n\n\
         [[detector]]\nid = \"screen\"\nat = 100\nsize = [4, 4]\npixels = [40, 40]\n",
    );
    let result = run(
        "field",
        json!({ "path": path.to_string_lossy(), "start": "A", "target": "B", "rays": true,
                "samples": 256 }),
    );
    let delivered = result["delivered"]
        .as_array()
        .and_then(|d| d.iter().find(|d| d["id"] == "screen"))
        .unwrap_or_else(|| panic!("{result}"));
    // The plain trace's screen is downstream of A and B, which absorb: trace it alone.
    let alone = write(
        "alone",
        "[[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 4.0\nrings = 48\nat = 0\n\n\
         [[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 50\nfocal_length = 100\ndiameter = 25.4\n\n\
         [[detector]]\nid = \"screen\"\nat = 100\nsize = [4, 4]\npixels = [40, 40]\n",
    );
    let traced = run("trace", json!({ "path": alone.to_string_lossy() }));
    let screen = &traced["detectors"][0];
    let (rms_field, rms_rays) = (
        number(&delivered["rms_radius_mm"]),
        number(&screen["direct_rms_radius_mm"]),
    );
    // A uniform disk of radius 1 mm: RMS radius 1/√2 mm. The trace's hexapolar rings
    // overshoot it a little; the field, interpolated between the outermost rays' centres,
    // falls half a launch cell short at the rim. Both to their sampling.
    assert!((rms_rays * 2f64.sqrt() - 1.0).abs() < 0.015, "{rms_rays}");
    assert!(
        (rms_field * 2f64.sqrt() - 1.0).abs() < 0.01,
        "{rms_field} vs {rms_rays}"
    );
    for k in 0..2 {
        assert!(
            number(&delivered["centroid_mm"][k]).abs() < 1e-6,
            "{delivered}"
        );
    }
    let power = number(&delivered["power_w"]);
    assert!((power - 1.0).abs() < 0.02, "{power}");
    // Every ray from the field reaches the screen: all of the field's power but its
    // faintest cells (below a millionth of the peak, which make no rays).
    let field_power = number(&result["power_w"]);
    assert!(
        (power / field_power - 1.0).abs() < 1e-4,
        "{power} of {field_power} W reaches the screen"
    );
}

#[test]
fn exit_check_a_field_focused_by_an_ideal_lens_has_the_psf_s_peak() {
    // A uniform beam overfilling a 10 mm ideal lens of 2 mm diameter (NA 0.1) as a field,
    // against the vectorial PSF of an ideal objective of the same focal length and NA
    // (M9c2), each peak per watt through its stop. The beam starts 1 mm before the lens,
    // so diffraction on the way does not change what the stop passes.
    let lambda = 0.55;
    let lens = write(
        "lens",
        "[[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 2.2\nrings = 6\nat = 49\n\n\
         [[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 50\nfocal_length = 10\ndiameter = 2.0\n\n\
         [[detector]]\nid = \"focus\"\nat = 60\nsize = [0.05, 0.05]\npixels = [40, 40]\n",
    );
    let parameters = json!({ "path": lens.to_string_lossy(), "target": "focus", "grid": 512 });
    let through = run("field", parameters.clone());
    let stopped = number(&through["power_w"]);
    let mut graph = parameters.clone();
    graph["of"] = json!("field");
    graph["svg"] = json!(out("field-focus.svg"));
    let field = run("graph", graph);
    let values: Vec<f64> = field["graph"]["map"]["values"]
        .as_array()
        .expect("values")
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    let peak_field = values.iter().copied().fold(0.0, f64::max) / stopped;
    let objective = write(
        "objective",
        "[[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 2.2\nrings = 6\nat = 0\n\n\
         [[element]]\nid = \"OBJ\"\nkind = \"objective\"\nat = 50\nfocal_length = 10.0\nna = 0.1\npupil_distance = 30.0\nworking_distance = 5.0\n\n\
         [[detector]]\nid = \"focus\"\nat = 85.0\nsize = [1, 1]\npixels = [10, 10]\n",
    );
    let psf = run(
        "psf",
        json!({ "path": objective.to_string_lossy(), "samples": 160 }),
    );
    let peak_psf = number(&psf["peak_w_per_mm2"]) / number(&psf["power_w"]);
    assert!(
        (peak_field / peak_psf - 1.0).abs() < 0.01,
        "field {peak_field} vs PSF {peak_psf} (Airy {})",
        core::f64::consts::PI * 0.01 / (lambda * 1e-3_f64).powi(2)
    );
}

#[test]
fn exit_check_a_gaussian_clipped_by_an_iris_in_a_lens_tube() {
    // w₀ = 1 mm (λ = 0.633 µm) through an iris of radius a = 1 mm at 1 mm, then a 25 mm
    // lens tube from 10 to 300 mm (its bore clips nothing), to a screen at 1000 mm. The
    // field passes 1 − e⁻² of the power, lights the axis as Fresnel's integral over the
    // clipped beam says, and its difference from the rays (which know no diffraction) is
    // drawn.
    let (w0, a, lambda, iris, screen): (f64, f64, f64, f64, f64) =
        (1.0, 1.0, 0.633e-3, 1.0, 1000.0);
    let path = write(
        "iris",
        &format!(
            "[[source]]\nid = \"laser\"\nkind = \"gaussian\"\nwavelength_um = 0.633\npower_w = 0.001\nwaist_diameter = {}\nrings = 24\nat = 0\n\n\
             [[proxy]]\nid = \"iris\"\nkind = \"plate\"\nhole_diameter = {}\nsize = 25.4\nat = {iris}\n\n\
             [[proxy]]\nid = \"tube\"\nkind = \"tube\"\ninner_diameter = 25.0\nfrom = 10\nto = 300\n\n\
             [[detector]]\nid = \"screen\"\nat = {screen}\nsize = [6, 6]\npixels = [60, 60]\n",
            2.0 * w0,
            2.0 * a
        ),
    );
    let result = run(
        "field",
        json!({ "path": path.to_string_lossy(), "target": "screen", "grid": 512 }),
    );
    let steps = result["steps"].as_array().expect("steps");
    let through_iris = steps
        .iter()
        .find(|s| s["what"].as_str().is_some_and(|w| w.starts_with("iris")))
        .unwrap_or_else(|| panic!("{result}"));
    let passed = number(&through_iris["power_w"]) / 0.001;
    let expected = 1.0 - (-2.0 * a * a / (w0 * w0)).exp();
    assert!((passed - expected).abs() < 3e-3, "{passed} vs {expected}");
    assert!(
        steps
            .iter()
            .filter(|s| s["what"].as_str().is_some_and(|w| w.starts_with("tube")))
            .count()
            == 9
    );

    // On the axis at the screen.
    let rayleigh = core::f64::consts::PI * w0 * w0 / lambda;
    let k = 2.0 * core::f64::consts::PI / lambda;
    let q0 = Complex::new(0.0, -rayleigh);
    let q = Complex::new(iris, -rayleigh);
    let d = screen - iris;
    let beta = Complex::new(0.0, k) * (Complex::ONE / (q * 2.0) + Complex::real(1.0 / (2.0 * d)));
    let e0 = (2.0 * 0.001 / (core::f64::consts::PI * w0 * w0)).sqrt();
    let disk = ((beta * (a * a)).exp() - Complex::ONE) / beta;
    let on_axis = (core::f64::consts::PI * e0 / (lambda * d) * (q0 / q).abs() * disk.abs()).powi(2);
    let graph = run(
        "graph",
        json!({ "path": path.to_string_lossy(), "of": "field", "target": "screen", "grid": 512,
                "svg": out("iris-field.svg") }),
    );
    let map = &graph["graph"]["map"];
    let (columns, rows) = (
        map["columns"].as_u64().expect("columns") as usize,
        map["rows"].as_u64().expect("rows") as usize,
    );
    let centre = number(&map["values"][(rows / 2) * columns + columns / 2]);
    assert!(
        (centre / on_axis - 1.0).abs() < 0.01,
        "{centre} vs {on_axis}"
    );

    let difference = run(
        "graph",
        json!({ "path": path.to_string_lossy(), "of": "field-difference", "target": "screen",
                "grid": 512, "svg": out("iris-difference.svg") }),
    );
    assert_eq!(difference["graph"]["map"]["colours"], "diverging");
    assert_eq!(difference["graph"]["title"], "Field minus rays on `screen`");
}

#[test]
fn a_field_will_not_pass_a_real_lens() {
    let path = write(
        "refused",
        "[[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 4.0\nrings = 4\nat = 0\n\n\
         [[element]]\nid = \"L1\"\nat = 50\nradius = [50, -50]\nthickness = 4\nmaterial = 1.5\ndiameter = 25.4\n\n\
         [[detector]]\nid = \"screen\"\nat = 100\nsize = [5, 5]\npixels = [10, 10]\n",
    );
    let error = Registry::standard()
        .run(
            "field",
            &json!({ "path": path.to_string_lossy(), "target": "screen",
                     "data_directory": scratch("empty").to_string_lossy() }),
        )
        .expect_err("refused");
    assert!(error.message.contains("`L1`"), "{}", error.message);
}

/// A sidecar beside a copy of the STEP fixture `step` (from `optobench-io`): a uniform
/// 20 mm beam from z = −10 along +z to a screen at z = 40, plus `extra`.
fn sidecar(label: &str, step: &str, extra: &str) -> PathBuf {
    let dir = scratch(label);
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../optobench-io/tests/step");
    std::fs::copy(fixtures.join(step), dir.join(step)).expect("copied");
    let path = dir.join(format!("{label}.optobench"));
    std::fs::write(
        &path,
        format!(
            "format = \"optobench/1\"\nkind = \"sidecar\"\nstep = \"{step}\"\n\n{extra}\
             [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 20.0\nrings = 8\n\
             position = [0, 0, -10]\naxis = [0, 0, 1]\n\n\
             [[detector]]\nid = \"screen\"\nposition = [0, 0, 40]\naxis = [0, 0, 1]\nsize = [30, 30]\npixels = [30, 30]\n"
        ),
    )
    .expect("written");
    path
}

/// The field's power around the first plane that cuts the body (before, after), and
/// after the last of its planes.
fn through_body(result: &Value) -> (f64, f64, f64) {
    let steps = result["steps"].as_array().expect("steps");
    let body = |s: &Value| s["what"].as_str().is_some_and(|w| w.ends_with(": body"));
    let power = |k: usize| number(&steps[k]["power_w"]);
    let cut = (1..steps.len())
        .find(|&k| body(&steps[k]) && power(k) < 0.99 * power(k - 1))
        .unwrap_or_else(|| panic!("no plane cuts the body: {result}"));
    let last = (0..steps.len())
        .rev()
        .find(|&k| body(&steps[k]))
        .expect("a body step");
    (power(cut - 1), power(cut), power(last))
}

#[test]
fn bodies_from_cad_cut_the_field_where_they_stand() {
    // Fixtures (crates/optobench-io/tests/step/README.md): a cylinder Ø12 along z from the
    // origin, and a block 10 × 20 × 30 from the origin. Untagged, they absorb. In a uniform
    // beam of radius 10 the cylinder blocks the middle (passing 1 − (6/10)²), the block
    // the quadrant x, y > 0 (passing 3/4).
    for (step, passed) in [("cylinder.step", 1.0 - 0.36), ("block.step", 0.75)] {
        let path = sidecar(step.trim_end_matches(".step"), step, "");
        let result = run(
            "field",
            json!({ "path": path.to_string_lossy(), "target": "screen", "grid": 512 }),
        );
        // The first cut passes the geometric fraction (to the edge cells' sampling, as for
        // an aperture: 0.3 %); the later planes take a little more, the light diffracted
        // into the body's shadow on the way.
        let (before, after, end) = through_body(&result);
        assert!(
            (after / before - passed).abs() < 3e-3,
            "{step}: {}",
            after / before
        );
        assert!(
            end <= after && end / before > passed - 0.01,
            "{step}: {}",
            end / before
        );
        let planes = result["steps"]
            .as_array()
            .expect("steps")
            .iter()
            .filter(|s| s["what"].as_str().is_some_and(|w| w.ends_with(": body")))
            .count();
        assert_eq!(planes, 9, "{step}: cut at nine planes through its depth");
    }
    // Tagged as a lens, the body is the trace's: the field will not pass it.
    let path = sidecar(
        "tagged",
        "cylinder.step",
        "[[element]]\nid = \"L\"\nkind = \"lens\"\nbody = { product = \"cylinder\" }\nmaterial = 1.5\n\n",
    );
    let error = Registry::standard()
        .run(
            "field",
            &json!({ "path": path.to_string_lossy(), "target": "screen",
                     "data_directory": scratch("empty").to_string_lossy() }),
        )
        .expect_err("refused");
    assert!(error.message.contains("body `L`"), "{}", error.message);
}

#[test]
fn a_field_meets_a_tilted_screen_and_goes_on_as_rays() {
    // A Gaussian beam (w₀ = 0.5 mm, λ = 0.633 µm) onto a screen 100 mm on, tilted 30° about
    // its y (a DOF set to 30°): the same power crosses it, the footprint is w(z)/cos 30°
    // across x and w(z) along y, and rays from it reach a screen at 200 mm as they do from
    // an untilted plane.
    let (w0, lambda) = (0.5, 0.633e-3);
    let layout = |tilt: f64| {
        write(
            &format!("tilted-{tilt}"),
            &format!(
                "[[source]]\nid = \"laser\"\nkind = \"gaussian\"\nwavelength_um = 0.633\npower_w = 0.001\nwaist_diameter = {}\nat = 0\n\n\
                 [[detector]]\nid = \"tilted\"\nat = 100\nsize = [6, 6]\npixels = [30, 30]\n\
                 dof = [ {{ name = \"tip\", kind = \"tilt\", axis = \"y\", value = {tilt} }} ]\n\n\
                 [[detector]]\nid = \"far\"\nat = 200\nsize = [6, 6]\npixels = [30, 30]\n",
                2.0 * w0
            ),
        )
    };
    let run_at = |tilt: f64| {
        run(
            "field",
            json!({ "path": layout(tilt).to_string_lossy(), "target": "tilted", "rays": true }),
        )
    };
    let (flat, tilted) = (run_at(0.0), run_at(30.0));
    let rayleigh = core::f64::consts::PI * w0 * w0 / lambda;
    let w = w0 * (1.0 + (100.0 / rayleigh).powi(2)).sqrt();
    assert!(
        (number(&tilted["power_w"]) / 0.001 - 1.0).abs() < 1e-6,
        "{tilted}"
    );
    let radius = (
        number(&tilted["radius_mm"][0]),
        number(&tilted["radius_mm"][1]),
    );
    let cos = 30f64.to_radians().cos();
    assert!((radius.0 * cos / w - 1.0).abs() < 1e-3, "{radius:?} vs {w}");
    assert!((radius.1 / w - 1.0).abs() < 1e-3, "{radius:?} vs {w}");
    assert!(
        (number(&tilted["tilt"][0]).abs() - 0.5).abs() < 1e-9,
        "{tilted}"
    );
    let far = |result: &Value| -> Value {
        result["delivered"]
            .as_array()
            .and_then(|d| d.iter().find(|d| d["id"] == "far"))
            .cloned()
            .unwrap_or_else(|| panic!("{result}"))
    };
    let (a, b) = (far(&flat), far(&tilted));
    assert!(
        (number(&b["power_w"]) / number(&a["power_w"]) - 1.0).abs() < 1e-3,
        "{a} {b}"
    );
    assert!(
        (number(&b["rms_radius_mm"]) / number(&a["rms_radius_mm"]) - 1.0).abs() < 1e-2,
        "{a} {b}"
    );
    for k in 0..2 {
        assert!(number(&b["centroid_mm"][k]).abs() < 1e-3, "{b}");
    }
}
