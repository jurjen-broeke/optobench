//! A 5× Galilean beam expander placed by the optimiser (M16): a plano-concave lens of
//! f −12 mm and a plano-convex lens of f 60 mm, both curved side first, of a glass of
//! n = 1.5168, the second lens free to slide along the path.
//!
//! Reference values (thick lenses, E. Hecht, Optics, 5th ed., §6.2):
//! - A plano lens curved side first has R₁ = (n − 1) f, back focal length f − t/n and
//!   front focal length f. The output is collimated (afocal) when L1's rear focal point
//!   meets L2's front one: L2's first vertex at L1's last vertex + (f₁ − t₁/n) + f₂,
//!   20 + 2 + (−12 − 2/1.5168) + 60 = 68.681 mm.
//! - Its angular magnification −f₂/f₁ = 5: a beam 1 mm across leaves 5 mm across.
//! - A thin pencil traces paraxially; a 1 mm beam meets the strongly curved R −6.2 mm
//!   surface's spherical aberration, and collimates best a little shorter (growing as the
//!   beam's radius squared).

use optobench_commands::Registry;
use serde_json::{Value, json};

fn optimise(label: &str, source: &str, merit: &str) -> Value {
    let text = format!(
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"laser\"\nwavelength_um = 0.633\npower_w = 0.001\nat = 0\n{source}\n\n\
         [[element]]\nid = \"L1\"\nkind = \"lens\"\nat = 20\nsurfaces = [ {{ radius = -6.2016 }}, {{ radius = \"inf\" }} ]\n\
         thickness = [2.0]\nmaterial = [1.5168]\ndiameter = 6.0\nclear_aperture = 5.0\n\n\
         [[element]]\nid = \"L2\"\nkind = \"lens\"\nat = 75\nsurfaces = [ {{ radius = 31.008 }}, {{ radius = \"inf\" }} ]\n\
         thickness = [4.0]\nmaterial = [1.5168]\ndiameter = 25.4\nclear_aperture = 22.9\n\
         dof = [ {{ name = \"L2_z\", kind = \"slide\", axis = \"z\", range = [-10, 10] }} ]\n\n\
         [[detector]]\nid = \"out\"\nat = 150\nsize = [20, 20]\npixels = [100, 100]\n\n{merit}"
    );
    let path = std::env::temp_dir().join(format!(
        "optobench-expander-{label}-{}.optobench",
        std::process::id()
    ));
    std::fs::write(&path, text).expect("written");
    let result = Registry::standard()
        .run("optimize", &json!({ "path": path.to_string_lossy() }))
        .map_err(|e| e.message);
    let _ = std::fs::remove_file(&path);
    result.unwrap_or_else(|e| panic!("{e}"))
}

fn place(result: &Value) -> f64 {
    75.0 + result["dofs"][0]["suggested"].as_f64().expect("L2_z")
}

fn end(result: &Value, kind: &str) -> f64 {
    let term = result["merit"]
        .as_array()
        .and_then(|m| m.iter().find(|t| t["kind"] == kind))
        .expect("the term");
    term["end"][0].as_f64().expect("a residual")
}

const COLLIMATION: &str = "[[merit]]\nkind = \"collimation\"\ndetector = \"out\"\n";

#[test]
fn a_thin_pencil_collimates_at_the_paraxial_spacing() {
    let n = 1.5168;
    let expected = 20.0 + 2.0 + (-12.0 - 2.0 / n) + 60.0;
    let r = optimise(
        "pencil",
        "kind = \"collimated\"\ndiameter = 0.02",
        COLLIMATION,
    );
    assert!(
        (place(&r) - expected).abs() < 1e-3,
        "{} vs {expected}",
        place(&r)
    );
    assert!(end(&r, "collimation") < 1e-5, "{r}");
}

#[test]
fn a_one_millimetre_laser_beam_leaves_five_millimetres_across_and_collimated() {
    // A Gaussian beam of 1 mm (1/e²) at its waist: collimation and a 5 mm target together.
    let merit = format!(
        "{COLLIMATION}\n[[merit]]\nkind = \"diameter\"\ndetector = \"out\"\ntarget = 5.0\n"
    );
    let r = optimise("laser", "kind = \"gaussian\"\nwaist_diameter = 1.0", &merit);
    assert_eq!(r["converged"], true, "{r}");
    // 5 mm across at the screen, to 1 %.
    assert!(end(&r, "diameter").abs() < 0.05, "{r}");
    // Collimated to what the beam's own divergence leaves: λ/(π w₀) = 0.40 mrad at the
    // waist, five times less after the expander, and a little aberration.
    assert!(end(&r, "collimation") < 0.15, "{r}");
    // Spherical aberration of the 1 mm beam: a little shorter than paraxial, not more
    // than 0.5 mm.
    let paraxial = 20.0 + 2.0 + (-12.0 - 2.0 / 1.5168) + 60.0;
    let shift = paraxial - place(&r);
    assert!(shift > 0.0 && shift < 0.5, "L2 at {}", place(&r));
}

#[test]
fn a_diameter_needs_its_target() {
    let path = std::env::temp_dir().join(format!(
        "optobench-expander-bad-{}.optobench",
        std::process::id()
    ));
    std::fs::write(
        &path,
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"laser\"\nkind = \"collimated\"\nwavelength_um = 0.633\npower_w = 0.001\ndiameter = 1\nat = 0\n\n\
         [[detector]]\nid = \"out\"\nat = 50\nsize = [10, 10]\npixels = [10, 10]\n\n\
         [[merit]]\nkind = \"diameter\"\ndetector = \"out\"\n",
    )
    .expect("written");
    let e = Registry::standard()
        .run("optimize", &json!({ "path": path.to_string_lossy() }))
        .expect_err("no target");
    let _ = std::fs::remove_file(&path);
    assert!(e.message.contains("target"), "{}", e.message);
}
