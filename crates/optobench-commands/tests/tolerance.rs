//! M14a: tolerances read from layouts, applied, compensated and measured one at a time
//! (`optobench_optimize::tolerance`).
//!
//! Reference values (geometric optics):
//! - An ideal lens moved sideways by d brings a collimated beam to focus d sideways.
//! - A mirror turned by θ turns the reflected beam by 2θ.
//! - A thick lens in air (index n, radii R₁, R₂, thickness t):
//!   1/f = (n − 1) (1/R₁ − 1/R₂ + (n − 1) t / (n R₁ R₂)), and its back focal distance
//!   f (1 − (n − 1) t / (n R₁)) (e.g. E. Hecht, Optics, 4th ed., 2002, §6.2).
//! - A despace of a lens is taken out exactly by moving the detector the same way.

use optobench_io::scene_file::parse;
use optobench_io::store::material;
use optobench_optimize::lm::Options;
use optobench_optimize::tolerance::{Sensitivity, sensitivity};
use optobench_scene::Scene;

fn read(
    text: &str,
) -> (
    Scene,
    optobench_optimize::tolerance::Tolerancing,
    Vec<optobench_optimize::Merit>,
) {
    let scene = parse(text, &|n: &str| material(None, n)).expect("the layout reads");
    let tolerancing = optobench_io::tolerance::parse(text, &scene).expect("tolerancing");
    let merit = optobench_io::merit::parse(text).expect("merit");
    (scene, tolerancing, merit)
}

fn run(text: &str) -> Sensitivity {
    let (scene, tolerancing, merit) = read(text);
    sensitivity(&scene, &tolerancing, &merit, &Options::default()).expect("sensitivities")
}

const HEADER: &str = "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
    [[source]]\nid = \"beam\"\nkind = \"collimated\"\ndiameter = 2.0\nrings = 6\nat = 0\n\
    wavelength_um = 0.633\npower_w = 1.0\n\n";

#[test]
fn a_decentred_ideal_lens_moves_the_focus_by_its_decentre() {
    let text = format!(
        "{HEADER}[[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 50\nfocal_length = 100\n\n\
         [[detector]]\nid = \"D\"\nat = 150\nsize = [10, 10]\npixels = [10, 10]\n\n\
         [[tolerance]]\nname = \"L_x\"\nkind = \"slide\"\ntarget = \"L\"\naxis = \"x\"\nlimit = 0.1\n\n\
         [[tolerance]]\nname = \"L_y\"\nkind = \"slide\"\ntarget = \"L\"\naxis = \"y\"\nlimit = 0.03\n\n\
         [tolerancing]\ncriteria = [ {{ kind = \"centring\", detector = \"D\", max = 0.05 }} ]\n"
    );
    let s = run(&text);
    assert!(s.nominal.values[0].expect("measured") < 1e-12);
    assert!(
        (s.rows[0].change[0] - 0.1).abs() < 1e-9,
        "{:?}",
        s.rows[0].change
    );
    assert!((s.rows[1].change[0] - 0.03).abs() < 1e-9);
    // RSS of independent errors, and the ranking.
    assert!((s.rss[0] - (0.1f64.powi(2) + 0.03f64.powi(2)).sqrt()).abs() < 1e-9);
    assert_eq!(s.ranking[0], vec![0, 1]);
    // At ±0.1 mm the centring fails its 0.05 mm limit; at 0.03 mm it passes.
    assert!(!s.rows[0].plus.passed && s.rows[1].plus.passed);
}

#[test]
fn a_tilted_fold_mirror_turns_the_beam_by_twice_its_tilt() {
    let text = format!(
        "{HEADER}[[element]]\nid = \"M\"\nkind = \"mirror\"\nat = 50\nfold = 90\ndiameter = 25\nclear_aperture = 24\n\n\
         [[detector]]\nid = \"D\"\nat = 150\nsize = [20, 20]\npixels = [10, 10]\n\n\
         [[tolerance]]\nname = \"M_tilt\"\nkind = \"tilt\"\ntarget = \"M\"\nframe = \"scene\"\naxis = \"z\"\nlimit = 0.1\n\n\
         [tolerancing]\ncriteria = [ {{ kind = \"pointing\", detector = \"D\", max = 1.0 }} ]\n"
    );
    let s = run(&text);
    // 2 × 0.1° in mrad, as the tangent of the slope.
    let expected = (2.0 * 0.1f64.to_radians()).tan() * 1e3;
    assert!(
        (s.rows[0].change[0] / expected - 1.0).abs() < 1e-9,
        "{} {expected}",
        s.rows[0].change[0]
    );
}

/// A biconvex singlet R₁ = 50, R₂ = −50, t = 5, n = 1.5 lit by a 1 mm beam, a detector at
/// its paraxial back focus free to slide along the axis (the compensator, on best focus).
fn singlet(tolerances: &str) -> String {
    let (n, r1, r2, t): (f64, f64, f64, f64) = (1.5, 50.0, -50.0, 5.0);
    let (f, bfl) = thick(n, r1, r2, t);
    let _ = f;
    format!(
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
         [[source]]\nid = \"beam\"\nkind = \"collimated\"\ndiameter = 1.0\nrings = 4\nat = 0\n\
         wavelength_um = 0.633\npower_w = 1.0\n\n\
         [[element]]\nid = \"L\"\nkind = \"lens\"\nat = 20\n\
         surfaces = [ {{ radius = {r1} }}, {{ radius = {r2} }} ]\nthickness = [{t}]\nmaterial = [{n}]\n\
         diameter = 25\nclear_aperture = 24\ncoating = \"ideal\"\n\n\
         [[detector]]\nid = \"D\"\nat = {}\nsize = [5, 5]\npixels = [10, 10]\n\
         dof = [ {{ name = \"D_z\", kind = \"slide\", axis = \"z\", range = [-10, 10] }} ]\n\n\
         [[merit]]\nkind = \"focus\"\ndetector = \"D\"\n\n\
         {tolerances}\n\
         [tolerancing]\ncompensators = [\"D_z\"]\ncriteria = [ {{ kind = \"focus\", detector = \"D\", max = 0.01 }} ]\n",
        20.0 + t + bfl
    )
}

/// (EFL, BFL) of a thick lens in air.
fn thick(n: f64, r1: f64, r2: f64, t: f64) -> (f64, f64) {
    let power = (n - 1.0) * (1.0 / r1 - 1.0 / r2 + (n - 1.0) * t / (n * r1 * r2));
    let f = 1.0 / power;
    (f, f * (1.0 - (n - 1.0) * t / (n * r1)))
}

#[test]
fn part_errors_move_the_focus_as_the_thick_lens_formula_says() {
    let s = run(&singlet(
        "[[tolerance]]\nname = \"R1\"\nkind = \"radius\"\ntarget = \"L\"\nsurface = 1\nlimit = 0.5\n\n\
         [[tolerance]]\nname = \"t\"\nkind = \"thickness\"\ntarget = \"L\"\ngap = 1\nlimit = 0.2\n\n\
         [[tolerance]]\nname = \"n\"\nkind = \"index\"\ntarget = \"L\"\ngap = 1\nlimit = 0.01\n\n\
         [[tolerance]]\nname = \"R2_rel\"\nkind = \"radius\"\ntarget = \"L\"\nsurface = 2\nrelative = true\nlimit = 0.01\n",
    ));
    let (_, bfl) = thick(1.5, 50.0, -50.0, 5.0);
    let nominal = s.nominal.compensators[0].1;
    let moved = |row: usize| s.rows[row].plus.compensators[0].1 - nominal;
    // The detector follows the back focus (measured from the first vertex: t + BFL), to
    // the small difference of best and paraxial focus for a 1 mm beam (µm).
    let checks = [
        (0, 5.0 + thick(1.5, 50.5, -50.0, 5.0).1),
        (1, 5.2 + thick(1.5, 50.0, -50.0, 5.2).1),
        (2, 5.0 + thick(1.51, 50.0, -50.0, 5.0).1),
        (3, 5.0 + thick(1.5, 50.0, -50.5, 5.0).1),
    ];
    for (row, expected) in checks {
        let found = moved(row);
        let wanted = expected - (5.0 + bfl);
        assert!(
            (found - wanted).abs() < 2e-3,
            "{}: {found} {wanted}",
            s.rows[row].tolerance
        );
    }
    // After compensation the focus is as good as nominal (each error is a pure refocus
    // here, to the spherical aberration's small change).
    for row in &s.rows {
        assert!(row.plus.values[0].expect("measured") < 1e-3, "{row:?}");
    }
}

#[test]
fn a_compensator_takes_out_a_despace_exactly() {
    let text = format!(
        "{HEADER}[[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 50\nfocal_length = 100\n\n\
         [[detector]]\nid = \"D\"\nat = 150\nsize = [10, 10]\npixels = [10, 10]\n\
         dof = [ {{ name = \"D_z\", kind = \"slide\", axis = \"z\", range = [-5, 5] }} ]\n\n\
         [[merit]]\nkind = \"focus\"\ndetector = \"D\"\n\n\
         [[tolerance]]\nname = \"L_z\"\nkind = \"slide\"\ntarget = \"L\"\naxis = \"z\"\nlimit = 0.3\n\n\
         [[tolerance]]\nname = \"f\"\nkind = \"focal_length\"\ntarget = \"L\"\nrelative = true\nlimit = 0.01\n\n\
         [tolerancing]\ncompensators = [\"D_z\"]\ncriteria = [ {{ kind = \"focus\", detector = \"D\", max = 0.001 }} ]\n"
    );
    let s = run(&text);
    // The lens 0.3 mm further on: the detector follows by 0.3 mm, the spot back to a point
    // (to the optimiser's reach: the RMS radius has a V-shaped minimum, which least
    // squares approaches to a fraction of a µm).
    let row = &s.rows[0];
    assert!((row.plus.compensators[0].1 - 0.3).abs() < 1e-3, "{row:?}");
    assert!((row.minus.compensators[0].1 + 0.3).abs() < 1e-3);
    assert!(row.plus.values[0].expect("measured") < 1e-5 && row.plus.passed);
    // A 1 % longer focal length: 1 mm further.
    assert!(
        (s.rows[1].plus.compensators[0].1 - 1.0).abs() < 1e-3,
        "{:?}",
        s.rows[1]
    );
    // Without the compensator the despace fails the criterion.
    let (scene, mut tolerancing, merit) = read(&text);
    tolerancing.compensators.clear();
    let held = sensitivity(&scene, &tolerancing, &merit, &Options::default()).expect("runs");
    assert!(!held.rows[0].plus.passed);
}

#[test]
fn tolerances_are_checked() {
    let base = format!(
        "{HEADER}[[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 50\nfocal_length = 100\n\n\
         [[detector]]\nid = \"D\"\nat = 150\nsize = [10, 10]\npixels = [10, 10]\n\n"
    );
    let refused = |extra: &str, words: &str| {
        let text = format!("{base}{extra}");
        let message = parse(&text, &|n: &str| material(None, n))
            .map(|scene| optobench_io::tolerance::parse(&text, &scene).map(|_| ()))
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string()))
            .expect_err("refused");
        assert!(message.contains(words), "{message}");
    };
    refused(
        "[[tolerance]]\nname = \"a\"\nkind = \"slide\"\ntarget = \"X\"\naxis = \"x\"\nlimit = 0.1\n",
        "no element, source or detector `X`",
    );
    refused(
        "[[tolerance]]\nname = \"a\"\nkind = \"wobble\"\ntarget = \"L\"\nlimit = 0.1\n",
        "kind `wobble` is not known",
    );
    refused(
        "[[tolerance]]\nname = \"a\"\nkind = \"radius\"\ntarget = \"L\"\nsurface = 1\nlimit = 0.1\n",
        "applies to a lens element",
    );
    refused(
        "[[tolerance]]\nname = \"a\"\nkind = \"slide\"\ntarget = \"L\"\naxis = \"x\"\nlimit = -1\n",
        "`limit` must be positive",
    );
    refused(
        "[[tolerance]]\nname = \"a\"\nkind = \"slide\"\ntarget = \"L\"\naxis = \"x\"\nlimit = 0.1\ngap = 1\n",
        "has no `gap`",
    );
    refused(
        "[[tolerance]]\nname = \"a\"\nkind = \"slide\"\ntarget = \"L\"\naxis = \"x\"\nlimit = 0.1\n\n\
         [[tolerance]]\nname = \"a\"\nkind = \"slide\"\ntarget = \"L\"\naxis = \"y\"\nlimit = 0.1\n",
        "a second tolerance `a`",
    );
    refused(
        "[tolerancing]\ncompensators = [\"nope\"]\n",
        "no degree of freedom `nope`",
    );
    refused(
        "[tolerancing]\ncriteria = [ { kind = \"focus\", detector = \"D\" } ]\n",
        "missing `max`",
    );
    refused(
        "[tolerancing]\ncriteria = [ { kind = \"focus\", detector = \"Q\", max = 1 } ]\n",
        "no detector `Q`",
    );
}

/// A lens and its detector each placed off sideways, uniformly within ±`l1` and ±`l2`
/// (mm); the criterion is the centring at the detector, at most `max`.
fn two_decentres(l1: f64, l2: f64, max: f64) -> String {
    format!(
        "{HEADER}[[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 50\nfocal_length = 100\n\n\
         [[detector]]\nid = \"D\"\nat = 150\nsize = [10, 10]\npixels = [10, 10]\n\n\
         [[tolerance]]\nname = \"L_x\"\nkind = \"slide\"\ntarget = \"L\"\naxis = \"x\"\nlimit = {l1}\n\n\
         [[tolerance]]\nname = \"D_x\"\nkind = \"slide\"\ntarget = \"D\"\naxis = \"x\"\nlimit = {l2}\n\n\
         [tolerancing]\ncriteria = [ {{ kind = \"centring\", detector = \"D\", max = {max} }} ]\n"
    )
}

fn monte_carlo(text: &str, label: &str, extra: serde_json::Value) -> serde_json::Value {
    let path = std::env::temp_dir().join(format!(
        "optobench-tolerance-{label}-{}.optobench",
        std::process::id()
    ));
    std::fs::write(&path, text).expect("written");
    let mut parameters =
        serde_json::json!({ "path": path.to_string_lossy(), "mode": "monte-carlo" });
    for (k, v) in extra.as_object().expect("object") {
        parameters[k] = v.clone();
    }
    let result = optobench_commands::Registry::standard()
        .run("tolerance", &parameters)
        .unwrap_or_else(|e| panic!("{}", e.message));
    let _ = std::fs::remove_file(&path);
    result
}

#[test]
fn monte_carlo_of_two_decentres_against_the_triangular_distribution() {
    // The centroid sits at the lens's decentre less the detector's: the difference of two
    // uniforms on ±L, triangular on ±2L. E|X| = 2L/3, and |X| ≤ L with probability 3/4.
    let l = 0.05;
    let n = 2000;
    let r = monte_carlo(
        &two_decentres(l, l, l),
        "triangle",
        serde_json::json!({ "trials": n, "seed": 11 }),
    );
    let m = &r["monte_carlo"];
    let mean = m["statistics"][0]["mean"].as_f64().expect("mean");
    // The mean's standard error: √((2L²/3 − (2L/3)²) / n).
    let error = ((2.0 * l * l / 3.0 - (2.0 * l / 3.0).powi(2)) / n as f64).sqrt();
    assert!((mean - 2.0 * l / 3.0).abs() < 4.0 * error, "{mean}");
    let passing = m["yield"].as_f64().expect("yield");
    let bound = 4.0 * (0.75f64 * 0.25 / n as f64).sqrt();
    assert!((passing - 0.75).abs() < bound, "{passing}");
    assert!(
        (m["yield_error"].as_f64().expect("error") - (passing * (1.0 - passing) / n as f64).sqrt())
            .abs()
            < 1e-12
    );
    // Their shares of the residual's variance: the centroid's x offset is U₁ − U₂ (y is
    // 0), linear, so each has exactly half.
    let shares: Vec<f64> = m["shares"][0]
        .as_array()
        .expect("shares")
        .iter()
        .map(|v| v.as_f64().expect("share"))
        .collect();
    for share in &shares {
        assert!((share - 0.5).abs() < 0.03, "{shares:?}");
    }
}

#[test]
fn monte_carlo_is_the_same_for_any_threads_and_its_ranking_holds_across_seeds() {
    let text = two_decentres(0.05, 0.01, 0.04);
    let one = monte_carlo(
        &text,
        "one",
        serde_json::json!({ "trials": 300, "seed": 5, "threads": 1 }),
    );
    let four = monte_carlo(
        &text,
        "four",
        serde_json::json!({ "trials": 300, "seed": 5, "threads": 4 }),
    );
    assert_eq!(one["monte_carlo"], four["monte_carlo"]);
    let other = monte_carlo(
        &text,
        "other",
        serde_json::json!({ "trials": 300, "seed": 6, "threads": 4 }),
    );
    assert_ne!(
        one["monte_carlo"]["statistics"],
        other["monte_carlo"]["statistics"]
    );
    // The larger decentre ranks first whatever the seed.
    for seed in 1..=4 {
        let r = monte_carlo(
            &text,
            "rank",
            serde_json::json!({ "trials": 300, "seed": seed }),
        );
        assert_eq!(r["monte_carlo"]["ranking"][0][0], "L_x", "seed {seed}");
    }
}

/// The golden periscope (crates/optobench-io/tests/step/periscope.*, M6) with tolerances
/// added: setting errors of its five adjusters, decentres of both lenses and a tilt of
/// the first, against centring, pointing and collimation on the screen.
pub fn golden_periscope() -> String {
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../optobench-io/tests/step");
    let text = std::fs::read_to_string(fixtures.join("periscope.optobench")).expect("fixture");
    let step = fixtures.join("periscope.step");
    text.replacen(
        "kind = \"sidecar\"\n",
        &format!(
            "kind = \"sidecar\"\nstep = \"{}\"\n",
            step.to_string_lossy()
        ),
        1,
    ) + "\n\
        [[tolerance]]\nname = \"M1_y set\"\nkind = \"dof\"\ndof = \"M1_y\"\nlimit = 0.05\n\n\
        [[tolerance]]\nname = \"M1_d set\"\nkind = \"dof\"\ndof = \"M1_d\"\nlimit = 0.05\n\n\
        [[tolerance]]\nname = \"M2_y set\"\nkind = \"dof\"\ndof = \"M2_y\"\nlimit = 0.02\n\n\
        [[tolerance]]\nname = \"M2_d set\"\nkind = \"dof\"\ndof = \"M2_d\"\nlimit = 0.02\n\n\
        [[tolerance]]\nname = \"L2_z set\"\nkind = \"dof\"\ndof = \"L2_z\"\nlimit = 0.2\n\n\
        [[tolerance]]\nname = \"L1 decentre\"\nkind = \"slide\"\ntarget = \"L1\"\naxis = \"x\"\nlimit = 0.1\n\n\
        [[tolerance]]\nname = \"L2 decentre\"\nkind = \"slide\"\ntarget = \"L2\"\naxis = \"y\"\nlimit = 0.05\n\n\
        [[tolerance]]\nname = \"L1 tilt\"\nkind = \"tilt\"\ntarget = \"L1\"\naxis = \"y\"\nlimit = 0.5\n\n\
        [tolerancing]\ncriteria = [ { kind = \"centring\", detector = \"screen\", max = 0.5 },\n\
                    { kind = \"pointing\", detector = \"screen\", max = 0.5 },\n\
                    { kind = \"collimation\", detector = \"screen\", max = 0.2 } ]\n"
}

/// The M14 exit check: on the golden periscope the ranking is the same for several seeds,
/// wherever it is decided by more than the sampling: the leading tolerance of every
/// criterion is the same, and two tolerances whose mean shares differ by more than 0.05
/// keep their order in every run. (Tolerances whose shares differ by less, within the
/// share's scatter from seed to seed, about 0.01 to 0.02 at 600 trials, may swap.)
#[test]
fn the_golden_periscope_ranking_is_stable_across_seeds() {
    let text = golden_periscope();
    let runs: Vec<serde_json::Value> = (1..=3)
        .map(|seed| {
            monte_carlo(
                &text,
                "golden",
                serde_json::json!({ "trials": 600, "seed": seed }),
            )["monte_carlo"]
                .clone()
        })
        .collect();
    let criteria = 3;
    for c in 0..criteria {
        let first: Vec<&serde_json::Value> = runs.iter().map(|r| &r["ranking"][c][0]).collect();
        assert!(
            first.iter().all(|f| *f == first[0]),
            "criterion {c}: {first:?}"
        );
        let shares: Vec<Vec<f64>> = runs
            .iter()
            .map(|r| {
                r["shares"][c]
                    .as_array()
                    .expect("shares")
                    .iter()
                    .map(|v| v.as_f64().expect("share"))
                    .collect()
            })
            .collect();
        let tolerances = shares[0].len();
        let mean: Vec<f64> = (0..tolerances)
            .map(|k| shares.iter().map(|s| s[k]).sum::<f64>() / shares.len() as f64)
            .collect();
        for a in 0..tolerances {
            for b in 0..tolerances {
                if mean[a] - mean[b] > 0.05 {
                    for (run, s) in shares.iter().enumerate() {
                        assert!(
                            s[a] > s[b],
                            "criterion {c}, run {run}: tolerance {a} ({}) below {b} ({})",
                            s[a],
                            s[b]
                        );
                    }
                }
            }
        }
    }
    // What leads: M1's setting about y for centring, L1's decentre for pointing, L2's
    // height for collimation (as one at a time).
    assert_eq!(runs[0]["ranking"][0][0], "M1_y set");
    assert_eq!(runs[0]["ranking"][1][0], "L1 decentre");
    assert_eq!(runs[0]["ranking"][2][0], "L2_z set");
}

#[test]
fn the_report_and_the_graphs() {
    let dir =
        std::env::temp_dir().join(format!("optobench-tolerance-report-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("decentres.optobench");
    std::fs::write(&path, two_decentres(0.05, 0.01, 0.04)).expect("written");
    let path = path.to_string_lossy().into_owned();
    let registry = optobench_commands::Registry::standard();
    let r = registry
        .run("tolerance", &serde_json::json!({ "path": path, "mode": "monte-carlo", "trials": 200, "report": true }))
        .unwrap_or_else(|e| panic!("{}", e.message));
    let markdown = std::fs::read_to_string(dir.join("decentres.tolerance.md")).expect("report");
    assert!(
        markdown.contains("## Monte Carlo") && markdown.contains("| L_x |"),
        "{markdown}"
    );
    let json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("decentres.tolerance.json")).expect("json"),
    )
    .expect("parses");
    assert_eq!(json["monte_carlo"]["yield"], r["monte_carlo"]["yield"]);
    assert!(json.get("seconds").is_none());
    for of in [
        "tolerance-histogram",
        "tolerance-sensitivity",
        "tolerance-yield",
    ] {
        let graph = registry
            .run(
                "graph",
                &serde_json::json!({ "path": path, "of": of, "trials": 100 }),
            )
            .unwrap_or_else(|e| panic!("{of}: {}", e.message));
        assert!(graph["graph"]["title"].is_string(), "{of}: {graph}");
    }
    let sensitivity = registry
        .run(
            "tolerance",
            &serde_json::json!({ "path": path, "report": dir.join("one").to_string_lossy() }),
        )
        .expect("sensitivity");
    assert!(sensitivity["sensitivity"]["rows"].is_array());
    assert!(
        std::fs::read_to_string(dir.join("one.tolerance.md"))
            .expect("report")
            .contains("## Sensitivity")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// M14d: adjustment range.

fn run_command(
    command: &str,
    text: &str,
    label: &str,
    parameters: serde_json::Value,
) -> serde_json::Value {
    let path = std::env::temp_dir().join(format!(
        "optobench-room-{label}-{}.optobench",
        std::process::id()
    ));
    std::fs::write(&path, text).expect("written");
    let mut p = parameters;
    p["path"] = serde_json::json!(path.to_string_lossy());
    let result = optobench_commands::Registry::standard()
        .run(command, &p)
        .unwrap_or_else(|e| panic!("{}", e.message));
    let _ = std::fs::remove_file(&path);
    result
}

/// A lens and its detector that can both slide sideways (an ideal lens images a
/// collimated beam onto its own axis, so only their difference matters), the lens
/// starting 2.5 mm off; `extra` merit added.
fn sliding_pair(extra: &str) -> String {
    format!(
        "{HEADER}[[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 50\nfocal_length = 100\n\
         dof = [ {{ name = \"L_x\", kind = \"slide\", axis = \"x\", range = [-1, 3], value = 2.5 }} ]\n\n\
         [[detector]]\nid = \"D\"\nat = 150\nsize = [10, 10]\npixels = [10, 10]\n\
         dof = [ {{ name = \"D_x\", kind = \"slide\", axis = \"x\", range = [-2, 2] }} ]\n\n\
         [[merit]]\nkind = \"centring\"\ndetector = \"D\"\n\n{extra}"
    )
}

#[test]
fn headroom_centres_the_adjusters_where_the_optics_leave_a_choice() {
    // Only L_x − D_x matters; with headroom the pair settles where both are as central as
    // they can be together: L_x = D_x minimising ((L − 1)/2)² + (D/2)², L = D = 0.5.
    let r = run_command(
        "optimize",
        &sliding_pair("[[merit]]\nkind = \"headroom\"\n"),
        "pair",
        serde_json::json!({}),
    );
    let value = |name: &str| {
        r["dofs"]
            .as_array()
            .expect("dofs")
            .iter()
            .find(|d| d["name"] == name)
            .expect("dof")["suggested"]
            .as_f64()
            .expect("value")
    };
    assert!((value("L_x") - 0.5).abs() < 1e-3, "{r}");
    assert!((value("D_x") - 0.5).abs() < 1e-3, "{r}");
    // The adjustment room: L_x at 0.5 of [−1, 3] uses 25 % of its travel towards the low
    // end, with 1.5 and 2.5 mm left.
    let l = r["dofs"]
        .as_array()
        .expect("dofs")
        .iter()
        .find(|d| d["name"] == "L_x")
        .expect("L_x");
    assert!((l["room"]["used"].as_f64().expect("used") + 0.25).abs() < 1e-3);
    assert_eq!(l["room"]["status"], "ok");
    assert!((l["room"]["left"][0].as_f64().expect("left") - 1.5).abs() < 1e-3);
    // Without headroom the optimiser stops wherever the pair first lines up.
    let plain = run_command(
        "optimize",
        &sliding_pair(""),
        "plain",
        serde_json::json!({}),
    );
    let difference = |r: &serde_json::Value| {
        let v: Vec<f64> = ["L_x", "D_x"]
            .iter()
            .map(|n| {
                r["dofs"]
                    .as_array()
                    .expect("dofs")
                    .iter()
                    .find(|d| d["name"] == *n)
                    .expect("dof")["suggested"]
                    .as_f64()
                    .expect("value")
            })
            .collect();
        v[0] - v[1]
    };
    assert!(difference(&plain).abs() < 1e-6);
}

#[test]
fn an_adjuster_far_off_centre_is_reported_with_its_range_after_adjusting() {
    // The lens 0.8 mm off; only it can move, within ±1 mm: 80 % of its travel.
    let text = format!(
        "{HEADER}[[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 50\nfocal_length = 100\n\
         dof = [ {{ name = \"L_x\", kind = \"slide\", axis = \"x\", range = [-1, 1] }} ]\n\n\
         [[detector]]\nid = \"D\"\nat = 150\nsize = [10, 10]\npixels = [10, 10]\n\n\
         [[merit]]\nkind = \"centring\"\ndetector = \"D\"\ntarget = [0.8, 0]\n"
    );
    let dir = std::env::temp_dir().join(format!("optobench-room-report-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let r = run_command(
        "optimize",
        &text,
        "off",
        serde_json::json!({ "report": dir.join("off").to_string_lossy() }),
    );
    let l = &r["dofs"][0];
    assert!((l["suggested"].as_f64().expect("value") - 0.8).abs() < 1e-6);
    let room = &l["room"];
    assert_eq!(room["status"], "off-centre");
    assert!((room["used"].as_f64().expect("used") - 0.8).abs() < 1e-6);
    let after: Vec<f64> = room["range_after_adjusting"]
        .as_array()
        .expect("range")
        .iter()
        .map(|v| v.as_f64().expect("v"))
        .collect();
    assert!(
        (after[0] + 1.8).abs() < 1e-6 && (after[1] - 0.2).abs() < 1e-6,
        "{after:?}"
    );
    assert!(
        r["warnings"][0]
            .as_str()
            .expect("warning")
            .contains("re-place the part's mounting by 0.8")
    );
    let markdown = std::fs::read_to_string(dir.join("off.moves.md")).expect("report");
    assert!(
        markdown.contains("## Adjustment room") && markdown.contains("off-centre"),
        "{markdown}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_compensator_that_runs_out_of_range_fails_the_trial() {
    // A ±0.3 mm despace and a detector compensator of ±0.1 mm: the focus can be found
    // only when |error| ≤ 0.1, two trials in three fail at the compensator's limit.
    let text = format!(
        "{HEADER}[[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 50\nfocal_length = 100\n\n\
         [[detector]]\nid = \"D\"\nat = 150\nsize = [10, 10]\npixels = [10, 10]\n\
         dof = [ {{ name = \"D_z\", kind = \"slide\", axis = \"z\", range = [-0.1, 0.1] }} ]\n\n\
         [[merit]]\nkind = \"focus\"\ndetector = \"D\"\n\n\
         [[tolerance]]\nname = \"L_z\"\nkind = \"slide\"\ntarget = \"L\"\naxis = \"z\"\nlimit = 0.3\n\n\
         [tolerancing]\ncompensators = [\"D_z\"]\ncriteria = [ {{ kind = \"focus\", detector = \"D\", max = 0.1 }} ]\n"
    );
    let n = 300;
    let r = run_command(
        "tolerance",
        &text,
        "limited",
        serde_json::json!({ "mode": "monte-carlo", "trials": n, "seed": 2 }),
    );
    let c = &r["monte_carlo"]["compensators"][0];
    let limited = c["limited"].as_f64().expect("share");
    let bound = 4.0 * (2.0f64 / 9.0 / n as f64).sqrt();
    assert!((limited - 2.0 / 3.0).abs() < bound, "{limited}");
    assert!((r["monte_carlo"]["yield"].as_f64().expect("yield") - (1.0 - limited)).abs() < 1e-12);
    assert!(
        c["advice"]
            .as_str()
            .expect("advice")
            .contains("ran out of travel in 66.3 %"),
        "{c}"
    );
    // With room enough, none runs out and the advice says how to centre it.
    let roomy = text.replace("range = [-0.1, 0.1]", "range = [-1, 1]");
    let r = run_command(
        "tolerance",
        &roomy,
        "roomy",
        serde_json::json!({ "mode": "monte-carlo", "trials": 100, "seed": 2 }),
    );
    let c = &r["monte_carlo"]["compensators"][0];
    assert_eq!(c["limited"].as_f64(), Some(0.0));
    assert!(
        c["advice"]
            .as_str()
            .expect("advice")
            .contains("re-place the part's nominal by"),
        "{c}"
    );
    assert!((c["mean"].as_f64().expect("mean")).abs() < 0.05);
}
