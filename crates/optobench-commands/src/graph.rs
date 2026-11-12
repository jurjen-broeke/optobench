//! The `graph` command: any result as a graph (`optobench_analysis::graph`), compared
//! with saved results or a reference, written as SVG, CSV and JSON.

use std::path::Path;

use optobench_analysis::graph::{self as graphs, Graph};
use optobench_io::graph as files;
use serde_json::{Value, json};

use crate::psf::{PSF_KEYS, detector_of, moved_scene, request};
use crate::trace::text;
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidParameters, message)
}

fn failed(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::Failed, message)
}

/// What can be drawn, for help texts and errors.
const KINDS: &[&str] = &[
    "psf-axial",
    "psf-lateral",
    "psf-section",
    "focus",
    "spot",
    "irradiance",
];

/// A graph saved as JSON (by this command's `json`).
fn read_saved(path: &str) -> Result<Graph, CommandError> {
    let text = std::fs::read_to_string(path).map_err(|e| failed(format!("{path}: {e}")))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| failed(format!("{path}: {e}")))?;
    // A saved command result holds the graph under `graph`.
    let value = if value["graph"].is_object() {
        &value["graph"]
    } else {
        &value
    };
    files::from_json(value).map_err(|e| failed(format!("{path}: {e}")))
}

/// The graph of a design's result.
fn of_design(parameters: &Parameters, kind: &str) -> Result<Graph, CommandError> {
    let (scene, path) = moved_scene(parameters)?;
    let fail = |e: String| failed(format!("{path}: {e}"));
    match kind {
        "psf-axial" | "psf-lateral" | "psf-section" => {
            let (source, detector, mut settings) = request(parameters, &scene, &path)?;
            if kind == "psf-section" {
                settings.section = Some(match parameters.get("section") {
                    None => (121, 161),
                    Some(v) => v
                        .as_array()
                        .filter(|a| a.len() == 2)
                        .and_then(|a| Some((a[0].as_u64()? as usize, a[1].as_u64()? as usize)))
                        .filter(|(c, r)| (2..=1000).contains(c) && (2..=1000).contains(r))
                        .ok_or_else(|| invalid("`section` is [columns, rows], each 2 to 1000"))?,
                });
            }
            let psf = optobench_analysis::psf(&scene, source, detector, &settings).map_err(fail)?;
            Ok(match kind {
                "psf-axial" => graphs::psf_axial(&psf),
                "psf-lateral" => graphs::psf_lateral(&psf),
                _ => graphs::psf_section(&psf).ok_or_else(|| fail("no section".into()))?,
            })
        }
        "focus" | "spot" | "irradiance" => {
            let detector = detector_of(parameters, &scene, &path)?;
            let trace = scene.trace().map_err(|e| fail(e.to_string()))?;
            let id = &scene.detectors[detector].id;
            match kind {
                "focus" => optobench_analysis::focus(&scene, &trace, detector)
                    .map(|f| graphs::focus_crossings(&f))
                    .ok_or_else(|| fail(format!("fewer than two rays reach `{id}`"))),
                "spot" => graphs::spot(&scene, &trace, detector)
                    .ok_or_else(|| fail(format!("no detector `{id}`"))),
                _ => graphs::irradiance(&scene, &trace, detector)
                    .ok_or_else(|| fail(format!("no detector `{id}`"))),
            }
        }
        other => Err(invalid(format!(
            "`of` = `{other}` is not known; use one of {}",
            KINDS.join(", ")
        ))),
    }
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map_or_else(|| path.to_owned(), |s| s.to_string_lossy().into_owned())
}

/// `graph`: a result as a graph. Either `path` (a design) and `of` (`psf-axial`,
/// `psf-lateral`, `psf-section`, `focus`, `spot`, `irradiance`) with that analysis's
/// parameters (`detector`, `dofs`, `data_directory`, and for the PSF `source`,
/// `polarization`, `samples`, `axial`, `lateral`, `points`, `section`), or `from`, a
/// graph saved earlier as JSON. Optionally compared: `compare` (saved graphs drawn on the
/// same axes; strings, or `{ "path", "name" }`), `difference` (a saved graph subtracted
/// from this one), `reference` (`{ "label", "points": [[x, y], …] }` in mm, W, W/mm²,
/// radians), `name` (this graph's name in a comparison; default `this`). Written to
/// `svg`, `csv` and `json` when given; the graph is also returned.
pub(crate) fn graph(parameters: &Parameters) -> Result<Value, CommandError> {
    let mut keys = vec![
        "path",
        "of",
        "from",
        "dofs",
        "data_directory",
        "section",
        "compare",
        "difference",
        "reference",
        "name",
        "svg",
        "csv",
        "json",
    ];
    keys.extend_from_slice(PSF_KEYS);
    check_keys(parameters, &keys)?;
    let mut graph = match (text(parameters, "from")?, text(parameters, "of")?) {
        (Some(from), None) => {
            if parameters.contains_key("path") {
                return Err(invalid(
                    "give `from` (a saved graph) or `path` and `of`, not both",
                ));
            }
            read_saved(from)?
        }
        (None, Some(kind)) => of_design(parameters, kind)?,
        (Some(_), Some(_)) => {
            return Err(invalid(
                "give `from` (a saved graph) or `path` and `of`, not both",
            ));
        }
        (None, None) => {
            return Err(invalid(format!(
                "missing `of` ({}) with `path`, or `from`",
                KINDS.join(", ")
            )));
        }
    };

    if let Some(reference) = parameters.get("reference") {
        let label = reference["label"]
            .as_str()
            .ok_or_else(|| invalid("`reference.label` must be a string"))?;
        let points = reference["points"]
            .as_array()
            .ok_or_else(|| invalid("`reference.points` is a list of [x, y]"))?
            .iter()
            .map(|p| match p.as_array().map(Vec::as_slice) {
                Some([x, y]) => Ok((
                    x.as_f64()
                        .ok_or_else(|| invalid("reference x must be a number"))?,
                    y.as_f64()
                        .ok_or_else(|| invalid("reference y must be a number"))?,
                )),
                _ => Err(invalid("`reference.points` is a list of [x, y]")),
            })
            .collect::<Result<Vec<_>, CommandError>>()?;
        graph = graph.with_reference(label, points);
    }
    if let Some(list) = parameters.get("compare") {
        let list = list
            .as_array()
            .ok_or_else(|| invalid("`compare` is a list of saved graphs"))?;
        let name = text(parameters, "name")?.unwrap_or("this").to_owned();
        let mut named = vec![(name, graph)];
        for entry in list {
            let (path, name) = match entry {
                Value::String(p) => (p.as_str(), file_name(p)),
                Value::Object(o) => {
                    let p = o
                        .get("path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| invalid("each `compare` entry needs a `path`"))?;
                    let n = o
                        .get("name")
                        .and_then(Value::as_str)
                        .map_or_else(|| file_name(p), str::to_owned);
                    (p, n)
                }
                _ => return Err(invalid("each `compare` entry is a path or { path, name }")),
            };
            named.push((name, read_saved(path)?));
        }
        graph = graphs::combine(&named).map_err(failed)?;
    }
    if let Some(other) = text(parameters, "difference")? {
        let saved = read_saved(other)?;
        graph = graphs::difference(&graph, &saved).map_err(|e| failed(format!("{other}: {e}")))?;
    }

    let mut written = Vec::new();
    let mut write = |key: &str, content: String| -> Result<(), CommandError> {
        if let Some(target) = text(parameters, key)? {
            std::fs::write(target, content).map_err(|e| failed(format!("{target}: {e}")))?;
            written.push(target.to_owned());
        }
        Ok(())
    };
    write("svg", files::svg(&graph))?;
    write("csv", files::csv(&graph))?;
    let as_json = files::to_json(&graph);
    write(
        "json",
        serde_json::to_string_pretty(&as_json).unwrap_or_default(),
    )?;
    Ok(json!({ "graph": as_json, "written": written }))
}
