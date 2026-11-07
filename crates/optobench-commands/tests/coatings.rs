//! M5c2 coatings in files: measured R/T curves (imported with `import-curve`) and
//! thin-film stacks, on layout elements and library parts, traced against theory.

use std::path::PathBuf;

use optobench_commands::Registry;
use serde_json::{Value, json};

/// A scratch data directory, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("optobench-coatings-{label}-{}", std::process::id()));
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

fn number(v: &Value) -> f64 {
    v.as_f64().expect("a number")
}

/// A collimated beam along the axis through `element`, onto a detector behind it.
fn trace(data: &Scratch, element: &str) -> Value {
    let text = format!(
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 4.0\nat = 0\n\n\
         [[element]]\nid = \"E\"\nat = 20\n{element}\n\
         [[detector]]\nid = \"screen\"\nat = 60\nsize = [10, 10]\npixels = [10, 10]\n"
    );
    let path = data.0.join("layout.optobench");
    std::fs::write(&path, text).expect("written");
    run(
        "trace",
        json!({ "path": path.to_string_lossy(), "data_directory": data.path() }),
    )
}

/// A plane-parallel window of n = 1.5, 2 mm thick, with the coatings given.
fn window(coating: &str) -> String {
    format!(
        "kind = \"lens\"\nsurfaces = [{{ radius = \"inf\" }}, {{ radius = \"inf\" }}]\nthickness = [2]\nmaterial = [1.5]\ndiameter = 25\nclear_aperture = 22\n{coating}\n"
    )
}

/// Imports a flat 50 % filter curve (transmittance in percent, R = 1 − T).
fn import_half(data: &Scratch) -> Value {
    let file = data.0.join("half.csv");
    std::fs::write(&file, "Wavelength (nm),Transmission (%)\n400,50\n700,50\n").expect("written");
    run(
        "import-curve",
        json!({
            "path": file.to_string_lossy(),
            "name": "half",
            "source": "hand-written test table",
            "data_directory": data.path(),
        }),
    )
}

#[test]
fn a_filter_curve_counts_once_for_the_whole_filter() {
    let data = Scratch::new("filter");
    let record = import_half(&data);
    assert_eq!(record["curve"]["percent"], true);
    assert_eq!(record["curve"]["reflectance"], "1 - transmittance");
    assert_eq!(
        run("curves", json!({ "data_directory": data.path() }))["curves"][0]["name"],
        "half"
    );
    // As a filter part: the curve on its first face, the other ideal, so the beam keeps
    // exactly half its power (R + T = 1 at every face, nothing absorbed).
    let part = data.0.join("filter.toml");
    std::fs::write(
        &part,
        "format = \"optobench/1\"\nkind = \"part\"\npart = \"F-50\"\nvendor = \"Example\"\ntype = \"filter\"\n\
         source = \"hand-written test part\"\ndiameter = 25\nclear_aperture = 22\nthickness = [2]\nglass = [1.5]\n\
         coating = { curve = \"half\" }\n",
    )
    .expect("written");
    run(
        "import-part",
        json!({ "path": part.to_string_lossy(), "data_directory": data.path() }),
    );
    let result = trace(&data, "library = \"example/F-50\"\n");
    let screen = &result["detectors"][0];
    assert!((number(&screen["direct_power_w"]) - 0.5).abs() < 1e-12);
    // The other face is ideal, so no ghost reaches the screen either.
    assert!((number(&screen["power_w"]) - 0.5).abs() < 1e-12);
    assert!(number(&result["ledger"]["imbalance_w"]).abs() < 1e-12);
    // The same curve on both faces of a window, as `coating` on a lens does, passes a
    // quarter directly.
    let both = trace(&data, &window("coating = { curve = \"half\" }"));
    assert!((number(&both["detectors"][0]["direct_power_w"]) - 0.25).abs() < 1e-12);
    // Surface by surface: curve then ideal is the filter again.
    let listed = trace(
        &data,
        &window("coatings = [{ curve = \"half\" }, \"ideal\"]"),
    );
    assert!((number(&listed["detectors"][0]["direct_power_w"]) - 0.5).abs() < 1e-12);
}

#[test]
fn a_quarter_wave_layer_matches_the_single_layer_formula() {
    let data = Scratch::new("quarter");
    // A quarter-wave layer of index n₁ on a substrate of index n_s, in air, at its design
    // wavelength and normal incidence: R = ((n_s − n₁²) / (n_s + n₁²))² (Macleod,
    // Thin-Film Optical Filters, 4th ed., ch. 3; Hecht, Optics, 5th ed., ch. 9).
    let (n1, ns) = (1.38_f64, 1.5_f64);
    let r = ((ns - n1 * n1) / (ns + n1 * n1)).powi(2);
    let result = trace(
        &data,
        &window("coating = { layers = [{ material = 1.38, quarter_wave_um = 0.55 }] }"),
    );
    let direct = number(&result["detectors"][0]["direct_power_w"]);
    // Two coated faces in a row (the stack faces outward on both sides).
    assert!(
        (direct - (1.0 - r).powi(2)).abs() < 1e-12,
        "{direct} vs {}",
        (1.0 - r).powi(2)
    );
    // Uncoated, each face reflects ((n − 1)/(n + 1))² = 0.04 (Fresnel; Hecht, ch. 4):
    // the coating does better.
    let bare = trace(&data, &window(""));
    let bare_direct = number(&bare["detectors"][0]["direct_power_w"]);
    assert!((bare_direct - 0.96_f64.powi(2)).abs() < 1e-12);
    assert!(direct > bare_direct);
}

#[test]
fn a_two_layer_stack_is_met_from_the_air_side_on_both_faces() {
    let data = Scratch::new("vcoat");
    // Two quarter-wave layers, n₁ outside and n₂ next to the substrate: admittance
    // Y = (n₁/n₂)² n_s and R = ((1 − Y)/(1 + Y))² in air (Macleod, Thin-Film Optical
    // Filters, 4th ed., ch. 3). With n₁ = 1.38, n₂ = 1.7 on n_s = 1.5, R = 3.4e-5; the
    // layers the other way round would reflect 15 %.
    let (n1, n2, ns) = (1.38_f64, 1.7_f64, 1.5_f64);
    let y = (n1 / n2).powi(2) * ns;
    let r = ((1.0 - y) / (1.0 + y)).powi(2);
    let swapped = {
        let y = (n2 / n1).powi(2) * ns;
        ((1.0 - y) / (1.0 + y)).powi(2)
    };
    assert!(r < 1e-4 && swapped > 0.1);
    let result = trace(
        &data,
        &window(
            "coating = { layers = [{ material = 1.38, quarter_wave_um = 0.55 }, { material = 1.7, quarter_wave_um = 0.55 }] }",
        ),
    );
    let direct = number(&result["detectors"][0]["direct_power_w"]);
    assert!(
        (direct - (1.0 - r).powi(2)).abs() < 1e-12,
        "{direct} vs {}",
        (1.0 - r).powi(2)
    );
}

#[test]
fn coating_errors_name_what_is_missing() {
    let data = Scratch::new("errors");
    let path = data.0.join("layout.optobench");
    std::fs::write(
        &path,
        format!(
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n[[element]]\nid = \"E\"\nat = 20\n{}",
            window("coating = { curve = \"absent\" }")
        ),
    )
    .expect("written");
    let error = Registry::standard()
        .run(
            "trace",
            &json!({ "path": path.to_string_lossy(), "data_directory": data.path() }),
        )
        .expect_err("no such curve");
    assert!(
        format!("{error:?}").contains("no curve `absent`"),
        "{error:?}"
    );
    let file = data.0.join("bad.csv");
    std::fs::write(&file, "nothing here\n").expect("written");
    let error = Registry::standard()
        .run(
            "import-curve",
            &json!({ "path": file.to_string_lossy(), "name": "bad", "data_directory": data.path() }),
        )
        .expect_err("no numbers");
    assert!(
        format!("{error:?}").contains("fewer than two rows"),
        "{error:?}"
    );
}
