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
    let one = |index: u64| {
        trial(
            &design.scene,
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
    }
}

fn outcome_json(o: &Outcome) -> Value {
    json!({
        "values": o.values,
        "passed": o.passed,
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
    })
}

/// `tolerance`: parameters `path` (a layout or sidecar with `[[tolerance]]` and
/// `[tolerancing]`), and optionally `mode` (`sensitivity`, default, or `monte-carlo`),
/// `trials` (default 200), `seed` (default 1), `threads` (default the machine's),
/// `max_iterations` (for the compensators, default 100), `dofs` and `data_directory`.
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
    Ok(result)
}
