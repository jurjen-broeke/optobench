//! The `tolerance` command (M14): sensitivities and Monte Carlo over a design's
//! tolerances (`optobench_optimize::tolerance`). Trials run on threads here; the engine
//! stays single-threaded.

use optobench_optimize::Merit;
use optobench_optimize::lm;
use optobench_optimize::tolerance::{
    MonteCarlo, Outcome, Sensitivity, Tolerancing, Trial, sensitivity, summarise, trial,
};
use optobench_scene::Scene;
use optobench_scene::tolerance::{Distribution, ToleranceKind};
use serde_json::{Value, json};

use crate::trace::{dof_values, load};
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidParameters, message)
}

fn failed(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::Failed, message)
}

/// The parameters of `tolerance`, besides `path`, `dofs` and `data_directory`.
pub(crate) const TOLERANCE_KEYS: &[&str] = &[
    "mode",
    "trials",
    "seed",
    "threads",
    "max_iterations",
    "report",
];

/// A design read for tolerancing.
pub(crate) struct Design {
    pub(crate) path: String,
    pub(crate) scene: Scene,
    pub(crate) tolerancing: Tolerancing,
    pub(crate) merit: Vec<Merit>,
    pub(crate) options: lm::Options,
}

pub(crate) fn design(parameters: &Parameters) -> Result<Design, CommandError> {
    let path = parameters
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing parameter `path`"))?
        .to_owned();
    let (scene, _, source, _) = load(&path, parameters)?;
    let start = dof_values(parameters, &scene)?;
    let scene = scene
        .moved(&start)
        .map_err(|e| failed(format!("{path}: {e}")))?;
    let tolerancing = optobench_io::tolerance::parse(&source, &scene)
        .map_err(|e| failed(format!("{path}: {e}")))?;
    let merit = optobench_io::merit::parse(&source).map_err(|e| failed(format!("{path}: {e}")))?;
    if !tolerancing.compensators.is_empty() && merit.is_empty() {
        return Err(failed(format!(
            "{path}: compensators need the design's [[merit]] to say what they adjust for"
        )));
    }
    let mut options = lm::Options::default();
    if let Some(n) = parameters.get("max_iterations") {
        options.max_iterations = n
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(|| invalid("`max_iterations` must be a whole number ≥ 1"))?
            as usize;
    }
    Ok(Design {
        path,
        scene,
        tolerancing,
        merit,
        options,
    })
}

/// `trials` trials of a run seeded `seed`, on `threads` threads (each trial from its own
/// stream, so the result does not depend on the threads).
pub(crate) fn run_trials(design: &Design, seed: u64, trials: u64, threads: usize) -> Vec<Trial> {
    let threads = threads.clamp(1, trials.max(1) as usize);
    // With trials on threads, each trial traces on its own thread (not threads in threads).
    let mut single = None;
    if threads > 1 {
        let mut scene = design.scene.clone();
        scene.threads = 1;
        single = Some(scene);
    }
    let scene = single.as_ref().unwrap_or(&design.scene);
    let one = |index: u64| {
        trial(
            scene,
            &design.tolerancing,
            &design.merit,
            &design.options,
            seed,
            index,
        )
    };
    if threads == 1 {
        return (0..trials).map(one).collect();
    }
    let next = std::sync::atomic::AtomicU64::new(0);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if k >= trials {
                            break;
                        }
                        done.push(one(k));
                    }
                    done
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap_or_default())
            .collect()
    })
}

/// Whether a tolerance's error is an angle.
fn is_angle(scene: &Scene, index: usize) -> bool {
    match &scene.tolerances[index].kind {
        ToleranceKind::Placement(m) => m.is_angle(),
        ToleranceKind::Dof(name) => scene
            .dofs
            .iter()
            .find(|d| &d.name == name)
            .is_some_and(|d| d.motion.is_angle()),
        _ => false,
    }
}

/// A tolerance's value in the file's units (degrees for angles).
pub(crate) fn shown(scene: &Scene, index: usize, value: f64) -> f64 {
    if is_angle(scene, index) {
        value.to_degrees()
    } else {
        value
    }
}

/// The unit of a tolerance's limit, as shown.
pub(crate) fn unit(scene: &Scene, index: usize) -> &'static str {
    let t = &scene.tolerances[index];
    match &t.kind {
        ToleranceKind::Placement(_) | ToleranceKind::Dof(_) => {
            if is_angle(scene, index) {
                "deg"
            } else {
                "mm"
            }
        }
        ToleranceKind::Radius { relative: true, .. }
        | ToleranceKind::FocalLength { relative: true } => "fraction",
        ToleranceKind::Index { .. } => "index",
        _ => "mm",
    }
}

/// The unit of a criterion's magnitude.
pub(crate) fn criterion_unit(merit: &Merit) -> &'static str {
    use optobench_optimize::Term;
    match merit.term {
        Term::Focus | Term::Centring { .. } => "mm",
        Term::Pointing { .. } | Term::Collimation => "mrad",
        Term::Power => "fraction",
        Term::Headroom => "half range",
    }
}

fn outcome_json(o: &Outcome) -> Value {
    json!({
        "values": o.values,
        "passed": o.passed,
        "limited": o.limited,
        "compensators": o.compensators.iter().map(|(n, v)| json!({ "name": n, "value": v })).collect::<Vec<_>>(),
        "failure": o.failure,
    })
}

pub(crate) fn tolerances_json(scene: &Scene) -> Vec<Value> {
    scene
        .tolerances
        .iter()
        .enumerate()
        .map(|(k, t)| {
            json!({
                "name": t.name,
                "limit": shown(scene, k, t.limit),
                "unit": unit(scene, k),
                "distribution": match t.distribution {
                    Distribution::Uniform => "uniform",
                    Distribution::Normal => "normal",
                    Distribution::End => "end",
                },
            })
        })
        .collect()
}

pub(crate) fn criteria_json(tolerancing: &Tolerancing) -> Vec<Value> {
    tolerancing
        .criteria
        .iter()
        .map(|c| {
            json!({
                "kind": c.merit.term.name(),
                "detector": c.merit.detector,
                "max": c.limit,
                "unit": criterion_unit(&c.merit),
            })
        })
        .collect()
}

pub(crate) fn sensitivity_json(design: &Design, s: &Sensitivity) -> Value {
    json!({
        "nominal": outcome_json(&s.nominal),
        "rows": s.rows.iter().map(|r| json!({
            "tolerance": r.tolerance,
            "minus": outcome_json(&r.minus),
            "plus": outcome_json(&r.plus),
            "change": r.change,
        })).collect::<Vec<_>>(),
        "rss": s.rss,
        "ranking": s.ranking.iter().map(|order| order.iter().map(|&k| design.scene.tolerances[k].name.clone()).collect::<Vec<_>>()).collect::<Vec<_>>(),
    })
}

pub(crate) fn monte_carlo_json(design: &Design, m: &MonteCarlo, seed: u64) -> Value {
    json!({
        "seed": seed,
        "trials": m.trials.len(),
        "yield": m.yield_fraction,
        "yield_error": m.yield_error,
        "statistics": m.statistics.iter().map(|s| json!({
            "measured": s.measured,
            "mean": s.mean,
            "deviation": s.deviation,
            "p50": s.percentiles[0], "p90": s.percentiles[1], "p95": s.percentiles[2], "p99": s.percentiles[3],
            "max": s.max,
            "passing": s.passing,
        })).collect::<Vec<_>>(),
        "shares": m.shares,
        "ranking": m.ranking.iter().map(|order| order.iter().map(|&k| design.scene.tolerances[k].name.clone()).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "failures": m.failures.iter().map(|(k, why)| json!({ "trial": k, "why": why })).collect::<Vec<_>>(),
        // Per criterion each trial's value (null where it could not be measured), and
        // whether each trial passed, in trial order.
        "values": (0..design.tolerancing.criteria.len())
            .map(|c| m.trials.iter().map(|t| t.outcome.values[c]).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        "passed": m.trials.iter().map(|t| t.outcome.passed).collect::<Vec<_>>(),
        "compensators": m.compensators.iter().map(|c| compensator_json(design, c)).collect::<Vec<_>>(),
    })
}

/// How the trials used a compensator's travel, in the file's units, with advice.
fn compensator_json(design: &Design, c: &optobench_optimize::tolerance::CompensatorUse) -> Value {
    let angle = design
        .scene
        .dofs
        .iter()
        .find(|d| d.name == c.name)
        .is_some_and(|d| d.motion.is_angle());
    let shown = |v: f64| if angle { v.to_degrees() } else { v };
    let advice = match c.range {
        Some((low, high)) => {
            let half = 0.5 * (high - low);
            let need = 0.5 * (c.spread.1 - c.spread.0);
            // Values stop at the range's ends, so running out shows in the share of
            // trials at a limit, not in the spread.
            if c.limited > 0.0 {
                format!(
                    "it ran out of travel in {:.1} % of the trials (its range ±{:.4}): tighten the tolerances or use a longer adjuster",
                    100.0 * c.limited,
                    shown(half)
                )
            } else if need > half {
                format!(
                    "98 % of the trials need ±{:.4} of travel, more than its range's ±{:.4}: tighten the tolerances or use a longer adjuster",
                    shown(need),
                    shown(half)
                )
            } else {
                format!(
                    "re-place the part's nominal by {:.4} to centre the travel the trials need (±{:.4} of its ±{:.4} for 98 % of them)",
                    shown(c.mean - 0.5 * (low + high)),
                    shown(need),
                    shown(half)
                )
            }
        }
        None => "no range given: the travel it needs is not checked".to_owned(),
    };
    json!({
        "name": c.name,
        "unit": if angle { "deg" } else { "mm" },
        "range": c.range.map(|(a, b)| [shown(a), shown(b)]),
        "mean": shown(c.mean),
        "deviation": shown(c.deviation),
        "p1": shown(c.spread.0),
        "p99": shown(c.spread.1),
        "limited": c.limited,
        "advice": advice,
    })
}

/// A Markdown summary of a `tolerance` result.
pub(crate) fn markdown(design: &Design, result: &Value) -> String {
    let mut out = format!("# Tolerancing of `{}`\n\n", design.path);
    out.push_str(
        "## Tolerances\n\n| Tolerance | Limit | Unit | Distribution |\n| --- | --- | --- | --- |\n",
    );
    for t in result["tolerances"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "| {} | ±{} | {} | {} |\n",
            t["name"].as_str().unwrap_or("?"),
            t["limit"],
            t["unit"].as_str().unwrap_or(""),
            t["distribution"].as_str().unwrap_or("")
        ));
    }
    out.push_str("\n## Criteria\n\n| Criterion | Detector | Passes up to | Unit |\n| --- | --- | --- | --- |\n");
    let criteria: Vec<&Value> = result["criteria"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    for c in &criteria {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            c["kind"].as_str().unwrap_or("?"),
            c["detector"].as_str().unwrap_or("?"),
            c["max"],
            c["unit"].as_str().unwrap_or("")
        ));
    }
    let compensators: Vec<&str> = result["compensators"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    out.push_str(&format!(
        "\nCompensators: {}.\n",
        if compensators.is_empty() {
            "none".to_owned()
        } else {
            compensators.join(", ")
        }
    ));
    let label = |c: usize| {
        criteria.get(c).map_or("?".to_owned(), |c| {
            format!(
                "{} at `{}` ({})",
                c["kind"].as_str().unwrap_or("?"),
                c["detector"].as_str().unwrap_or("?"),
                c["unit"].as_str().unwrap_or("")
            )
        })
    };
    if let Some(s) = result.get("sensitivity") {
        out.push_str("\n## Sensitivity (each tolerance alone at ±limit)\n");
        for c in 0..criteria.len() {
            out.push_str(&format!(
                "\n### {}\n\nNominal {}; root sum of squares of the changes {}.\n\n| Rank | Tolerance | Largest change |\n| --- | --- | --- |\n",
                label(c),
                s["nominal"]["values"][c],
                s["rss"][c]
            ));
            for (rank, name) in s["ranking"][c].as_array().into_iter().flatten().enumerate() {
                let row = s["rows"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|r| r["tolerance"] == *name));
                out.push_str(&format!(
                    "| {} | {} | {} |\n",
                    rank + 1,
                    name.as_str().unwrap_or("?"),
                    row.map_or(Value::Null, |r| r["change"][c].clone())
                ));
            }
        }
    }
    if let Some(m) = result.get("monte_carlo") {
        out.push_str(&format!(
            "\n## Monte Carlo\n\n{} trials, seed {}: yield {:.4} ± {:.4} (one standard error).\n",
            m["trials"],
            m["seed"],
            m["yield"].as_f64().unwrap_or(f64::NAN),
            m["yield_error"].as_f64().unwrap_or(f64::NAN)
        ));
        out.push_str("\n| Criterion | Mean | Std. dev. | 50 % | 90 % | 95 % | 99 % | Largest | Passing |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
        for (c, st) in m["statistics"].as_array().into_iter().flatten().enumerate() {
            let f = |k: &str| {
                st[k]
                    .as_f64()
                    .map_or("–".to_owned(), |v| format!("{v:.4e}"))
            };
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {:.4} |\n",
                label(c),
                f("mean"),
                f("deviation"),
                f("p50"),
                f("p90"),
                f("p95"),
                f("p99"),
                f("max"),
                st["passing"].as_f64().unwrap_or(f64::NAN)
            ));
        }
        for c in 0..criteria.len() {
            out.push_str(&format!(
                "\n### Shares of the variance of {}\n\n| Rank | Tolerance | Share |\n| --- | --- | --- |\n",
                label(c)
            ));
            for (rank, name) in m["ranking"][c].as_array().into_iter().flatten().enumerate() {
                let k = design
                    .scene
                    .tolerances
                    .iter()
                    .position(|t| Some(t.name.as_str()) == name.as_str())
                    .unwrap_or(0);
                out.push_str(&format!(
                    "| {} | {} | {:.3} |\n",
                    rank + 1,
                    name.as_str().unwrap_or("?"),
                    m["shares"][c][k].as_f64().unwrap_or(f64::NAN)
                ));
            }
        }
        let compensators: Vec<&Value> =
            m["compensators"].as_array().into_iter().flatten().collect();
        if !compensators.is_empty() {
            out.push_str("\n### Compensators' travel\n\n| Compensator | Range | Mean | 1 % to 99 % | At a limit | Advice |\n| --- | --- | --- | --- | --- | --- |\n");
            for c in compensators {
                let f = |v: &Value| v.as_f64().map_or("–".to_owned(), |x| format!("{x:.4}"));
                let range = if c["range"].is_array() {
                    format!("{} to {}", f(&c["range"][0]), f(&c["range"][1]))
                } else {
                    "none".to_owned()
                };
                out.push_str(&format!(
                    "| {} ({}) | {} | {} | {} to {} | {:.1} % | {} |\n",
                    c["name"].as_str().unwrap_or("?"),
                    c["unit"].as_str().unwrap_or(""),
                    range,
                    f(&c["mean"]),
                    f(&c["p1"]),
                    f(&c["p99"]),
                    100.0 * c["limited"].as_f64().unwrap_or(0.0),
                    c["advice"].as_str().unwrap_or("")
                ));
            }
        }
        let failures = m["failures"].as_array().map_or(0, Vec::len);
        if failures > 0 {
            out.push_str(&format!(
                "\n{failures} trials could not be evaluated (they count as failing).\n"
            ));
        }
    }
    out
}

/// `tolerance`: parameters `path` (a layout or sidecar with `[[tolerance]]` and
/// `[tolerancing]`), and optionally `mode` (`sensitivity`, default, or `monte-carlo`),
/// `trials` (default 200), `seed` (default 1), `threads` (default the machine's),
/// `max_iterations` (for the compensators, default 100), `report` (true: write
/// `<design>.tolerance.json` and `.tolerance.md` next to the design; or a path stem),
/// `dofs` and `data_directory`.
pub(crate) fn command(parameters: &Parameters) -> Result<Value, CommandError> {
    let mut keys = vec!["path", "dofs", "data_directory"];
    keys.extend_from_slice(TOLERANCE_KEYS);
    check_keys(parameters, &keys)?;
    let design = design(parameters)?;
    let path = design.path.clone();
    let started = std::time::Instant::now();
    let mut result = json!({
        "tolerances": tolerances_json(&design.scene),
        "criteria": criteria_json(&design.tolerancing),
        "compensators": design.tolerancing.compensators,
        "warnings": crate::trace::range_warnings(&design.scene, crate::trace::small_range(parameters)?),
    });
    match parameters
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("sensitivity")
    {
        "sensitivity" => {
            let s = sensitivity(
                &design.scene,
                &design.tolerancing,
                &design.merit,
                &design.options,
            )
            .map_err(|e| failed(format!("{path}: {e}")))?;
            result["sensitivity"] = sensitivity_json(&design, &s);
        }
        "monte-carlo" => {
            if design.scene.tolerances.is_empty() || design.tolerancing.criteria.is_empty() {
                return Err(failed(format!(
                    "{path}: Monte Carlo needs tolerances ([[tolerance]]) and criteria ([tolerancing] criteria)"
                )));
            }
            let count = |key: &str, default: u64, most: u64| -> Result<u64, CommandError> {
                match parameters.get(key) {
                    None => Ok(default),
                    Some(v) => v
                        .as_u64()
                        .filter(|n| (1..=most).contains(n))
                        .ok_or_else(|| {
                            invalid(format!("`{key}` must be a whole number from 1 to {most}"))
                        }),
                }
            };
            let trials = count("trials", 200, 1_000_000)?;
            let seed = match parameters.get("seed") {
                None => 1,
                Some(v) => v
                    .as_u64()
                    .ok_or_else(|| invalid("`seed` must be a whole number ≥ 0"))?,
            };
            let machine = std::thread::available_parallelism().map_or(1, |n| n.get()) as u64;
            let threads = count("threads", machine, 256)? as usize;
            let done = run_trials(&design, seed, trials, threads);
            let m = summarise(&design.scene, &design.tolerancing, done);
            result["monte_carlo"] = monte_carlo_json(&design, &m, seed);
            result["threads"] = json!(threads);
        }
        other => {
            return Err(invalid(format!(
                "`mode` `{other}` is not known; use \"sensitivity\" or \"monte-carlo\""
            )));
        }
    }
    result["seconds"] = json!(started.elapsed().as_secs_f64());
    if let Some(parameter) = parameters.get("report") {
        if let Some((json_path, md_path)) =
            crate::moves::report_paths(std::path::Path::new(&path), parameter, "tolerance")?
        {
            let mut saved = result.clone();
            // Only what does not change from run to run.
            if let Some(o) = saved.as_object_mut() {
                o.remove("seconds");
                o.remove("threads");
            }
            let text = serde_json::to_string_pretty(&saved).map_err(|e| failed(e.to_string()))?;
            std::fs::write(&json_path, text)
                .map_err(|e| failed(format!("{}: {e}", json_path.display())))?;
            std::fs::write(&md_path, markdown(&design, &saved))
                .map_err(|e| failed(format!("{}: {e}", md_path.display())))?;
            result["report"] = json!({ "json": json_path.to_string_lossy(), "markdown": md_path.to_string_lossy() });
        }
    }
    Ok(result)
}

/// The axis of a criterion's magnitudes, with the factor from shown values to the axis's
/// engine unit.
fn criterion_axis(merit: &Merit) -> (optobench_analysis::graph::Axis, f64) {
    use optobench_analysis::graph::{Axis, Unit};
    let label = format!("{} at `{}`", merit.term.name(), merit.detector);
    match criterion_unit(merit) {
        "mm" => (Axis::new(label, Unit::Length), 1.0),
        "mrad" => (Axis::new(label, Unit::Angle), 1e-3),
        _ => (Axis::new(label, Unit::Number), 1.0),
    }
}

/// Graphs of a `tolerance` result: `tolerance-histogram` (each criterion's distribution
/// with its pass limit; Monte Carlo), `tolerance-sensitivity` (the changes, or the
/// shares, in rank order) and `tolerance-yield` (the yield against the number of trials,
/// with its ±2σ band; Monte Carlo), for criterion `criterion` (from 0).
pub(crate) fn graph(
    design: &Design,
    result: &Value,
    kind: &str,
    criterion: usize,
) -> Result<optobench_analysis::graph::Graph, CommandError> {
    use optobench_analysis::graph::{Axis, Graph, Mark, Marker, Plot, Series, Unit};
    let c = design
        .tolerancing
        .criteria
        .get(criterion)
        .ok_or_else(|| invalid(format!("no criterion {criterion} (from 0)")))?;
    let (axis, factor) = criterion_axis(&c.merit);
    let numbers = |v: &Value| -> Vec<f64> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_f64)
            .collect()
    };
    match kind {
        "tolerance-histogram" | "tolerance-yield" => {
            let m = result.get("monte_carlo").ok_or_else(|| {
                invalid(format!(
                    "`{kind}` needs a Monte Carlo run (mode = \"monte-carlo\")"
                ))
            })?;
            if kind == "tolerance-yield" {
                let passed: Vec<bool> = m["passed"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| v.as_bool().unwrap_or(false))
                    .collect();
                let (mut running, mut low, mut high) = (Vec::new(), Vec::new(), Vec::new());
                let mut count = 0usize;
                for (k, &p) in passed.iter().enumerate() {
                    count += usize::from(p);
                    let n = (k + 1) as f64;
                    let y = count as f64 / n;
                    let band = 2.0 * (y * (1.0 - y) / n).sqrt();
                    running.push((n, y));
                    low.push((n, (y - band).max(0.0)));
                    high.push((n, (y + band).min(1.0)));
                }
                let line = |label: &str, points: Vec<(f64, f64)>, reference: bool| Series {
                    label: label.into(),
                    points,
                    mark: Mark::Line,
                    reference,
                };
                return Ok(Graph {
                    title: "Yield against the number of trials".into(),
                    x: Axis::new("trials", Unit::Number),
                    y: Axis::new("yield", Unit::Number),
                    plot: Plot::Series(vec![
                        line("yield", running, false),
                        line("−2σ", low, true),
                        line("+2σ", high, true),
                    ]),
                    equal_aspect: false,
                    markers: Vec::new(),
                    notes: vec![format!(
                        "yield {:.4} ± {:.4} after {} trials (seed {})",
                        m["yield"].as_f64().unwrap_or(f64::NAN),
                        m["yield_error"].as_f64().unwrap_or(f64::NAN),
                        passed.len(),
                        m["seed"]
                    )],
                });
            }
            let values: Vec<f64> = numbers(&m["values"][criterion])
                .iter()
                .map(|v| v * factor)
                .collect();
            if values.is_empty() {
                return Err(failed("no trial measured this criterion"));
            }
            let limit = c.limit * factor;
            let top = values.iter().cloned().fold(limit, f64::max);
            let bins = ((values.len() as f64).sqrt().ceil() as usize).clamp(5, 60);
            let width = top / bins as f64;
            let mut counts = vec![0usize; bins];
            for v in &values {
                counts[((v / width) as usize).min(bins - 1)] += 1;
            }
            let mut points = vec![(0.0, 0.0)];
            for (b, &n) in counts.iter().enumerate() {
                let share = n as f64 / values.len() as f64;
                points.push((b as f64 * width, share));
                points.push(((b + 1) as f64 * width, share));
            }
            points.push((top, 0.0));
            let passing =
                values.iter().filter(|v| **v <= limit).count() as f64 / values.len() as f64;
            Ok(Graph {
                title: format!("Distribution of {} over the trials", axis.label),
                x: axis.clone(),
                y: Axis::new("share of trials", Unit::Number),
                plot: Plot::Series(vec![Series {
                    label: "trials".into(),
                    points,
                    mark: Mark::Line,
                    reference: false,
                }]),
                equal_aspect: false,
                markers: vec![Marker {
                    at: limit,
                    label: "pass limit".into(),
                }],
                notes: vec![format!(
                    "{} measured trials; {:.4} within the limit",
                    values.len(),
                    passing
                )],
            })
        }
        _ => {
            let (values, label, y) = if let Some(m) = result.get("monte_carlo") {
                let shares = numbers(&m["shares"][criterion]);
                let order: Vec<usize> = m["ranking"][criterion]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|n| {
                        design
                            .scene
                            .tolerances
                            .iter()
                            .position(|t| Some(t.name.as_str()) == n.as_str())
                    })
                    .collect();
                (
                    order
                        .iter()
                        .map(|&k| (k, shares.get(k).copied().unwrap_or(0.0)))
                        .collect::<Vec<_>>(),
                    "share of the variance",
                    Axis::new("share", Unit::Number),
                )
            } else {
                let s = &result["sensitivity"];
                let order: Vec<usize> = s["ranking"][criterion]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|n| {
                        design
                            .scene
                            .tolerances
                            .iter()
                            .position(|t| Some(t.name.as_str()) == n.as_str())
                    })
                    .collect();
                let rows = s["rows"].as_array().cloned().unwrap_or_default();
                (
                    order
                        .iter()
                        .map(|&k| {
                            (
                                k,
                                rows.get(k)
                                    .and_then(|r| r["change"][criterion].as_f64())
                                    .unwrap_or(0.0)
                                    * factor,
                            )
                        })
                        .collect(),
                    "largest change at ±limit",
                    Axis {
                        label: axis.label.clone(),
                        ..axis.clone()
                    },
                )
            };
            let points: Vec<(f64, f64)> = values
                .iter()
                .enumerate()
                .flat_map(|(rank, &(_, v))| {
                    let x = rank as f64 + 1.0;
                    [
                        (x - 0.35, 0.0),
                        (x - 0.35, v),
                        (x + 0.35, v),
                        (x + 0.35, 0.0),
                    ]
                })
                .collect();
            Ok(Graph {
                title: format!("Tolerances ranked by {label}: {}", axis.label),
                x: Axis::new("rank", Unit::Number),
                y,
                plot: Plot::Series(vec![Series {
                    label: label.into(),
                    points,
                    mark: Mark::Line,
                    reference: false,
                }]),
                equal_aspect: false,
                markers: values
                    .iter()
                    .enumerate()
                    .map(|(rank, &(k, _))| Marker {
                        at: rank as f64 + 1.0,
                        label: design.scene.tolerances[k].name.clone(),
                    })
                    .collect(),
                notes: Vec::new(),
            })
        }
    }
}
