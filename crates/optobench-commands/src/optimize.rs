//! `optimize`: the DOF values that best meet a design's merit (M6b;
//! `optobench_optimize`).

use optobench_optimize::{Merit, Term, lm, optimise};
use serde_json::{Value, json};

use crate::trace::{dof_values, load};
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn failed(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::Failed, message)
}

/// The `merit` parameter: a list of `{ kind, detector, target?, weight? }` objects, as the
/// file's `[[merit]]` tables.
fn merit_parameter(value: &Value) -> Result<Vec<Merit>, CommandError> {
    let invalid = |m: String| CommandError::new(ErrorCode::InvalidParameters, m);
    let list = value
        .as_array()
        .ok_or_else(|| invalid("`merit` is a list of { kind, detector, … }".into()))?;
    // Through TOML text, so the parameter and the file read alike.
    let tables: Vec<toml::Value> = list
        .iter()
        .map(|v| {
            let text = serde_json::to_string(v).map_err(|e| invalid(e.to_string()))?;
            serde_json::from_str::<toml::Value>(&text).map_err(|e| invalid(e.to_string()))
        })
        .collect::<Result<_, _>>()?;
    optobench_io::merit::from_tables(&tables).map_err(|e| invalid(e.to_string()))
}

fn term_json(merit: &Merit, start: &[f64], end: &[f64]) -> Value {
    let unit = match merit.term {
        Term::Focus | Term::Centring { .. } => "mm",
        Term::Pointing { .. } | Term::Collimation => "mrad",
        Term::Diameter { .. } => "mm",
        Term::Power => "fraction",
        Term::Backscatter => "%",
        Term::Headroom => "half range",
    };
    json!({
        "kind": merit.term.name(),
        "detector": merit.detector,
        "weight": merit.weight,
        "unit": unit,
        "start": start,
        "end": end,
    })
}

/// The adjustment room of a DOF at `value` within [low, high] (mm or radians; `shown`
/// converts to the file's units): how much of its travel towards one end it uses (−1 to
/// 1 of half the range), the travel left each way, the range to give the DOF if the move
/// is made with its adjuster and the design exported again (the travel left, around the
/// new zero), its status (`ok`, `off-centre` beyond `warning`, `at limit`) and advice.
pub(crate) fn room(
    value: f64,
    low: f64,
    high: f64,
    warning: f64,
    shown: &dyn Fn(f64) -> f64,
) -> Value {
    let half = 0.5 * (high - low);
    let middle = 0.5 * (low + high);
    let used = if half > 0.0 {
        (value - middle) / half
    } else {
        0.0
    };
    let slack = 1e-9 * half.max(1e-12);
    let status = if value <= low + slack || value >= high - slack {
        "at limit"
    } else if used.abs() > warning {
        "off-centre"
    } else {
        "ok"
    };
    let advice = if status == "ok" {
        "room enough both ways".to_owned()
    } else {
        format!(
            "re-place the part's mounting by {:.4} along or about this DOF's axis in the design, and keep the adjuster in the middle of its range rather than using {:.0} % of its travel",
            shown(value - middle),
            100.0 * used.abs()
        )
    };
    json!({
        "range": [shown(low), shown(high)],
        "used": used,
        "left": [shown(value - low), shown(high - value)],
        "range_after_adjusting": [shown(low - value), shown(high - value)],
        "status": status,
        "advice": advice,
    })
}

/// `optimize`: parameters `path` (a layout or sidecar with DOFs); optionally `merit`
/// (in place of the file's `[[merit]]`), `dofs` (start values, mm or degrees),
/// `max_iterations` (default 100), `report` (true: write the move report next to the
/// design as `<design>.moves.json` and `.moves.md`; or a path stem), `headroom_warning`
/// (the share of its travel towards one end beyond which an adjuster is off-centre,
/// default 0.5), `small_range_mm` (slides adjusting this little or less either way are
/// warned about before anything else, default 1) and `data_directory`. Returns each free DOF's start and suggested value,
/// its adjustment room where it has a range, warnings for adjusters off-centre or at a
/// limit, and every merit term before and after.
pub(crate) fn optimize(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(
        parameters,
        &[
            "path",
            "merit",
            "dofs",
            "max_iterations",
            "report",
            "headroom_warning",
            "small_range_mm",
            "data_directory",
        ],
    )?;
    let small = crate::trace::small_range(parameters)?;
    let warning = match parameters.get("headroom_warning") {
        None => 0.5,
        Some(v) => v
            .as_f64()
            .filter(|w| (0.0..=1.0).contains(w))
            .ok_or_else(|| {
                CommandError::new(
                    ErrorCode::InvalidParameters,
                    "`headroom_warning` is a fraction of half a range, from 0 to 1",
                )
            })?,
    };
    let path = parameters
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            CommandError::new(ErrorCode::InvalidParameters, "missing parameter `path`")
        })?;
    let (scene, _, source, _) = load(path, parameters)?;
    let merit = match parameters.get("merit") {
        Some(value) => merit_parameter(value)?,
        None => optobench_io::merit::parse(&source).map_err(|e| failed(format!("{path}: {e}")))?,
    };
    let start = dof_values(parameters, &scene)?;
    let scene = scene
        .moved(&start)
        .map_err(|e| failed(format!("{path}: {e}")))?;
    let mut options = lm::Options::default();
    if let Some(n) = parameters.get("max_iterations") {
        options.max_iterations = n.as_u64().filter(|n| *n > 0).ok_or_else(|| {
            CommandError::new(
                ErrorCode::InvalidParameters,
                "`max_iterations` must be a whole number ≥ 1",
            )
        })? as usize;
    }
    let result = optimise(&scene, &merit, &options).map_err(|e| failed(format!("{path}: {e}")))?;
    let dofs: Vec<Value> = result
        .values
        .iter()
        .zip(&result.outcome.at_bound)
        .map(|((name, start, end), at_bound)| {
            let dof = scene
                .dofs
                .iter()
                .find(|d| &d.name == name)
                .expect("a free DOF");
            let angle = dof.motion.is_angle();
            let unit = |x: f64| if angle { x.to_degrees() } else { x };
            let mut entry = json!({
                "name": name,
                "unit": if angle { "deg" } else { "mm" },
                "start": unit(*start),
                "suggested": unit(*end),
                "change": unit(end - start),
                "at_range_limit": at_bound,
            });
            if let Some((low, high)) = dof.range {
                entry["room"] = room(*end, low, high, warning, &unit);
            }
            entry
        })
        .collect();
    let terms: Vec<Value> = merit
        .iter()
        .zip(&result.terms)
        .map(|(m, (a, b))| term_json(m, a, b))
        .collect();
    let mut written = Value::Null;
    if let Some(parameter) = parameters.get("report") {
        if let Some((json_path, md_path)) =
            crate::moves::report_paths(std::path::Path::new(path), parameter, "moves")?
        {
            let (report, markdown) =
                crate::moves::report(&scene, &result, &dofs, &terms, std::path::Path::new(path))?;
            let text = serde_json::to_string_pretty(&report).map_err(|e| failed(e.to_string()))?;
            std::fs::write(&json_path, text)
                .map_err(|e| failed(format!("{}: {e}", json_path.display())))?;
            std::fs::write(&md_path, markdown)
                .map_err(|e| failed(format!("{}: {e}", md_path.display())))?;
            written = json!({ "json": json_path.to_string_lossy(), "markdown": md_path.to_string_lossy() });
        }
    }
    let mut warnings = crate::trace::range_warnings(&scene, small);
    warnings.extend(
        dofs.iter()
            .filter(|d| d["room"].is_object() && d["room"]["status"] != "ok")
            .map(|d| {
                format!(
                    "`{}` uses {:.0} % of its travel towards one end: {}",
                    d["name"].as_str().unwrap_or("?"),
                    100.0 * d["room"]["used"].as_f64().unwrap_or(0.0).abs(),
                    d["room"]["advice"].as_str().unwrap_or("")
                )
            }),
    );
    Ok(json!({
        "dofs": dofs,
        "warnings": warnings,
        "merit": terms,
        "report": written,
        "cost": { "start": result.outcome.start_cost, "end": result.outcome.cost },
        "converged": result.outcome.converged,
        "iterations": result.outcome.iterations,
        "history": result.outcome.history,
        "traces": result.outcome.evaluations,
    }))
}
