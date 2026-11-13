//! M11b3: a fluorescent protein imported from FPbase's API output (`import-fluorophore`)
//! into a design, and read back as a fluorophore.
//!
//! The JSON files here are shaped like the API's responses (docs/licence-review.md,
//! FPbase) with made-up values; they are not FPbase data. Reference values:
//! σ₁ = 1000 ln 10 ε / N_A cm² (N_A = 6.022 140 76 × 10²³ /mol, exact in the SI);
//! 1 GM = 10⁻⁵⁰ cm⁴ s (C. Xu et al., Proc. Natl. Acad. Sci. USA 93, 10763 (1996)).

use std::path::PathBuf;

use optobench_commands::Registry;
use optobench_io::scene_file::parse;
use optobench_io::store::material;
use serde_json::{Value, json};

const SPECTRA: &str = r#"[{"name": "TestFP", "slug": "testfp", "spectra": [
    {"state": "default_ex", "ec": 50000, "max": 480, "data": [[460, 0.5], [480, 1.0], [500, 0.25]]},
    {"state": "default_em", "qy": 0.7, "max": 510, "data": [[490, 0.2], [510, 1.0], [540, 0.3]]},
    {"state": "default_2p", "data": [[900, 0.6], [920, 1.0], [960, 0.4]]}
]}]"#;
const PROTEINS: &str = r#"[{"name": "TestFP", "slug": "testfp", "doi": "10.0000/test.1",
    "states": [{"slug": "testfp_default", "name": "default", "ext_coeff": 50000, "qy": 0.7, "lifetime": 3.1}]}]"#;

const DESIGN: &str = "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
    [[element]]\nid = \"S\"\nkind = \"sample\"\nat = 10\n\
    layers = [ { thickness = 1.0, material = 1.33 } ]\n";

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("optobench-fpbase-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn file(&self, name: &str, text: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, text).expect("written");
        path.to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(parameters: Value) -> Result<Value, String> {
    Registry::standard()
        .run("import-fluorophore", &parameters)
        .map_err(|e| e.message)
}

#[test]
fn a_protein_is_imported_into_a_design_with_its_attribution() {
    let scratch = Scratch::new("import");
    let spectra = scratch.file("spectra.json", SPECTRA);
    let protein = scratch.file("protein.json", PROTEINS);
    let design = scratch.file("design.optobench", DESIGN);
    let data = scratch.0.join("data").to_string_lossy().into_owned();
    let parameters = json!({
        "spectra": spectra, "protein": protein, "append_to": design, "data_directory": data,
    });

    // First the terms, nothing stored or written.
    let preview = run(parameters.clone()).expect("preview");
    assert_eq!(preview["stored"], false);
    assert!(
        preview["notice"][0]
            .as_str()
            .expect("notice")
            .contains("free of all copyright restrictions")
    );
    assert_eq!(preview["fluorophore"]["doi"], "10.0000/test.1");
    assert_eq!(std::fs::read_to_string(&design).expect("design"), DESIGN);
    assert!(!scratch.0.join("data").exists());

    // Accepted: the files are stored with their provenance and the entry appended.
    let mut accepted = parameters.clone();
    accepted["accept_licence"] = json!(true);
    let imported = run(accepted.clone()).expect("imported");
    assert_eq!(imported["stored"], true);
    let directory = PathBuf::from(
        imported["provenance"]["directory"]
            .as_str()
            .expect("directory"),
    );
    assert!(directory.join("testfp-spectra.json").exists());
    assert!(directory.join("testfp-protein.json").exists());
    assert!(directory.join("provenance.json").exists());

    let text = std::fs::read_to_string(&design).expect("design");
    let scene = parse(&text, &|name: &str| material(None, name)).expect("the design reads");
    let dye = &scene.fluorophores[0];
    assert_eq!(dye.id, "testfp");
    assert_eq!(dye.quantum_yield, 0.7);
    assert!((dye.lifetime.expect("lifetime") - 3.1e-9).abs() < 1e-21);
    assert!(
        dye.source.contains("Nat. Methods 16, 277") && dye.source.contains("doi:10.0000/test.1")
    );
    // σ₁ at the peak from ε = 50 000 L/(mol cm), in mm²; the spectrum in µm.
    let peak = 1000.0 * core::f64::consts::LN_10 * 50000.0 / 6.022_140_76e23 * 100.0;
    let one = dye.one_photon.as_ref().expect("σ₁");
    assert!((one.at(0.48).expect("in range") - peak).abs() < 1e-12 * peak);
    assert!((one.at(0.5).expect("in range") - 0.25 * peak).abs() < 1e-12 * peak);
    assert!(dye.emission.is_some());
    // No cross-section from FPbase: the relative two-photon spectrum stays commented out.
    assert!(dye.two_photon.is_none());
    assert!(text.contains("# two_photon_spectrum = ["));

    // A second import under the same id is refused.
    let again = run(accepted).expect_err("taken");
    assert!(
        again.contains("already has a fluorophore `testfp`"),
        "{again}"
    );
}

#[test]
fn a_relative_two_photon_spectrum_is_shaped_by_its_peak() {
    let entry = "[[fluorophore]]\nid = \"d\"\nsource = \"test\"\nquantum_yield = 0.5\n\
                 two_photon_spectrum = [[0.9, 0.6], [0.92, 1.0], [0.96, 0.4]]\n";
    let read = |extra: &str| {
        parse(&format!("{DESIGN}\n{entry}{extra}"), &|n: &str| {
            material(None, n)
        })
    };
    let scene = read("two_photon_cross_section_gm = 20\n").expect("read");
    let two = scene.fluorophores[0].two_photon.as_ref().expect("σ₂");
    assert!((two.at(0.92).expect("in range") - 20e-48).abs() < 1e-60);
    assert!((two.at(0.9).expect("in range") - 12e-48).abs() < 1e-60);
    // An action cross-section of 10 GM at η = 0.5 is the same 20 GM.
    let scene = read("two_photon_action_cross_section_gm = 10\n").expect("read");
    let two = scene.fluorophores[0].two_photon.as_ref().expect("σ₂");
    assert!((two.at(0.92).expect("in range") - 20e-48).abs() < 1e-60);
    let error = read("").expect_err("no peak").to_string();
    assert!(error.contains("give its peak"), "{error}");
}

#[test]
fn unusable_files_are_refused() {
    let scratch = Scratch::new("refused");
    let two = format!(
        "[{}, {}]",
        &SPECTRA[1..SPECTRA.len() - 1],
        SPECTRA[1..SPECTRA.len() - 1]
            .replace("TestFP", "OtherFP")
            .replace("testfp", "otherfp")
    );
    let spectra = scratch.file("two.json", &two);
    let error = run(json!({ "spectra": spectra })).expect_err("which one");
    assert!(error.contains("name one"), "{error}");
    let picked = run(json!({ "spectra": spectra, "name": "otherfp" })).expect("picked");
    assert_eq!(picked["fluorophore"]["protein"], "OtherFP");
    let error =
        run(json!({ "spectra": spectra, "name": "otherfp", "state": "on" })).expect_err("state");
    assert!(error.contains("its states: default"), "{error}");
    let broken = scratch.file("broken.json", "not json");
    assert!(
        run(json!({ "spectra": broken }))
            .expect_err("not JSON")
            .contains("not JSON")
    );
}
