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
    "psf-wavefront",
    "focus",
    "spot",
    "irradiance",
    "gaussian",
    "sweep",
    "optimize",
    "field",
    "field-phase",
    "field-difference",
    "fluence-depth",
    "absorption",
];

/// The parameters of `of = "sweep"` and `of = "optimize"`, passed on to those commands.
const SWEEP_KEYS: &[&str] = &[
    "dof", "values", "from", "to", "steps", "quantity", "element",
];
const OPTIMIZE_KEYS: &[&str] = &["merit", "max_iterations"];

/// `parameters` restricted to `keys`.
fn subset(parameters: &Parameters, keys: &[&str]) -> Parameters {
    parameters
        .iter()
        .filter(|(k, _)| keys.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// A sweep's results as a graph: `quantity` (`centroid`, `power`, `rms`, `pointing` of
/// each detector's direct light, or `chief`: where each source's chief ray crosses
/// `element`) against the DOF's value.
fn sweep_graph(
    result: &Value,
    quantity: &str,
    detector: Option<&str>,
    element: Option<&str>,
) -> Result<Graph, CommandError> {
    use optobench_analysis::graph::{Axis, Mark, Plot, Series, Unit};
    let angle = result["unit"] == "deg";
    let name = result["dof"].as_str().unwrap_or("value");
    let steps = result["steps"].as_array().cloned().unwrap_or_default();
    let x_of = |step: &Value| {
        let v = step["value"].as_f64().unwrap_or(f64::NAN);
        if angle { v.to_radians() } else { v }
    };
    // Series by label, in order of appearance.
    let mut series: Vec<(String, Vec<(f64, f64)>)> = Vec::new();
    let mut add = |label: String, x: f64, y: f64| match series.iter_mut().find(|(l, _)| *l == label)
    {
        Some((_, points)) => points.push((x, y)),
        None => series.push((label, vec![(x, y)])),
    };
    let (y_axis, title) = match quantity {
        "centroid" | "power" | "rms" | "pointing" => {
            let ids: Vec<String> = steps
                .first()
                .and_then(|s| s["detectors"].as_array())
                .map(|d| {
                    d.iter()
                        .filter_map(|d| d["id"].as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let chosen: Vec<&String> = ids
                .iter()
                .filter(|id| detector.is_none_or(|d| d == id.as_str()))
                .collect();
            if chosen.is_empty() {
                return Err(invalid(format!(
                    "no detector {}in the sweep",
                    detector.map(|d| format!("`{d}` ")).unwrap_or_default()
                )));
            }
            let prefix = |id: &str, part: &str| {
                if chosen.len() > 1 {
                    if part.is_empty() {
                        id.to_owned()
                    } else {
                        format!("{id}: {part}")
                    }
                } else if part.is_empty() {
                    id.to_owned()
                } else {
                    part.to_owned()
                }
            };
            for step in &steps {
                let x = x_of(step);
                for d in step["detectors"].as_array().into_iter().flatten() {
                    let id = d["id"].as_str().unwrap_or("?");
                    if !chosen.iter().any(|c| c.as_str() == id) {
                        continue;
                    }
                    let pair = |key: &str, scale: f64| {
                        (
                            d[key][0].as_f64().unwrap_or(f64::NAN) * scale,
                            d[key][1].as_f64().unwrap_or(f64::NAN) * scale,
                        )
                    };
                    match quantity {
                        "centroid" => {
                            let (a, b) = pair("direct_centroid_mm", 1.0);
                            add(prefix(id, "x"), x, a);
                            add(prefix(id, "y"), x, b);
                        }
                        "pointing" => {
                            let (a, b) = pair("direct_mean_slope_mrad", 1e-3);
                            add(prefix(id, "x"), x, a);
                            add(prefix(id, "y"), x, b);
                        }
                        "power" => add(
                            prefix(id, ""),
                            x,
                            d["direct_power_w"].as_f64().unwrap_or(f64::NAN),
                        ),
                        _ => add(
                            prefix(id, ""),
                            x,
                            d["direct_rms_radius_mm"].as_f64().unwrap_or(f64::NAN),
                        ),
                    }
                }
            }
            match quantity {
                "centroid" => (
                    Axis::new("centroid", Unit::Length),
                    "Centroid of the direct light",
                ),
                "pointing" => (
                    Axis::new("mean slope", Unit::Angle),
                    "Pointing of the direct light",
                ),
                "power" => (
                    Axis::new("direct power", Unit::Power),
                    "Power of the direct light",
                ),
                _ => (
                    Axis::new("RMS radius", Unit::Length),
                    "RMS radius of the direct light",
                ),
            }
        }
        "chief" => {
            let element = element.ok_or_else(|| {
                invalid("`quantity` = `chief` needs `element` (an element or detector id)")
            })?;
            for step in &steps {
                let x = x_of(step);
                for chief in step["chief"].as_array().into_iter().flatten() {
                    let source = chief["source"].as_str().unwrap_or("?");
                    let at = chief["hits"]
                        .as_array()
                        .and_then(|h| h.iter().find(|h| h["element"] == element))
                        .map(|h| &h["position_mm"])
                        .or_else(|| {
                            (chief["landed"]["detector"] == element)
                                .then(|| &chief["landed"]["position_mm"])
                        });
                    if let Some(p) = at {
                        let (a, b) = (
                            p[0].as_f64().unwrap_or(f64::NAN),
                            p[1].as_f64().unwrap_or(f64::NAN),
                        );
                        add(format!("{source}: x"), x, a);
                        add(format!("{source}: y"), x, b);
                    }
                }
            }
            if series.is_empty() {
                return Err(failed(format!("no chief ray reaches `{element}`")));
            }
            (
                Axis::new(format!("chief ray at `{element}`"), Unit::Length),
                "Chief ray position",
            )
        }
        other => {
            return Err(invalid(format!(
                "`quantity` `{other}` is not known; use centroid, power, rms, pointing or chief"
            )));
        }
    };
    Ok(Graph {
        title: format!("{title} over `{name}`"),
        x: Axis::new(name, if angle { Unit::Angle } else { Unit::Length }),
        y: y_axis,
        plot: Plot::Series(
            series
                .into_iter()
                .map(|(label, points)| Series {
                    label,
                    points,
                    mark: Mark::Line,
                    reference: false,
                })
                .collect(),
        ),
        equal_aspect: false,
        markers: Vec::new(),
        notes: vec![format!("{} values", steps.len())],
    })
}

/// The optimiser's merit (sum of squared weighted residuals) by accepted step.
fn optimize_graph(result: &Value) -> Graph {
    use optobench_analysis::graph::{Axis, Mark, Plot, Scale, Series, Unit};
    let history: Vec<f64> = result["history"]
        .as_array()
        .map(|h| h.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default();
    let positive = history.iter().all(|c| *c > 0.0);
    Graph {
        title: "Optimiser progress".into(),
        x: Axis::new("accepted step", Unit::Number),
        y: Axis {
            label: "merit".into(),
            unit: Unit::Number,
            scale: if positive {
                Scale::Logarithmic
            } else {
                Scale::Linear
            },
        },
        plot: Plot::Series(vec![Series {
            label: "merit".into(),
            points: history
                .iter()
                .enumerate()
                .map(|(i, c)| (i as f64, *c))
                .collect(),
            mark: Mark::Line,
            reference: false,
        }]),
        equal_aspect: false,
        markers: Vec::new(),
        notes: vec![format!(
            "{} after {} iterations and {} traces",
            if result["converged"] == true {
                "converged"
            } else {
                "stopped"
            },
            result["iterations"],
            result["traces"]
        )],
    }
}

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
    let base = ["path", "dofs", "data_directory"];
    if kind == "sweep" {
        let mut keys = base.to_vec();
        keys.extend_from_slice(&SWEEP_KEYS[..5]);
        let result = crate::sweep::sweep(&subset(parameters, &keys))?;
        let quantity = text(parameters, "quantity")?.unwrap_or("centroid");
        return sweep_graph(
            &result,
            quantity,
            text(parameters, "detector")?,
            text(parameters, "element")?,
        );
    }
    if matches!(kind, "field" | "field-phase" | "field-difference") {
        let (scene, request, outcome) = crate::field::outcome(parameters)?;
        let target = &scene.detectors[request.target];
        return match kind {
            "field" => Ok(outcome
                .field
                .irradiance_graph(format!("Field at `{}`", target.id))),
            "field-phase" => Ok(outcome
                .field
                .phase_graph(format!("Field phase at `{}`", target.id))),
            _ => {
                use optobench_analysis::colour::ColourScale;
                use optobench_analysis::graph::{Axis, Map, Plot, Unit};
                let traced = scene.trace().map_err(|e| failed(e.to_string()))?;
                let rays: Vec<f64> = traced.detectors[request.target]
                    .irradiance
                    .iter()
                    .map(|v| v * 1e-6)
                    .collect();
                let field =
                    optobench_field::scene::on_pixels(&outcome.field, target.size, target.pixels);
                let map = |values: Vec<f64>, title: &str| Graph {
                    title: title.into(),
                    x: Axis::new("x", Unit::Length),
                    y: Axis::new("y", Unit::Length),
                    plot: Plot::Map(Map {
                        columns: target.pixels.0,
                        rows: target.pixels.1,
                        x: (-target.size.0 / 2.0, target.size.0 / 2.0),
                        y: (-target.size.1 / 2.0, target.size.1 / 2.0),
                        values,
                        value: Axis::new("irradiance", Unit::Irradiance),
                        colours: ColourScale::Sequential,
                    }),
                    equal_aspect: true,
                    markers: Vec::new(),
                    notes: Vec::new(),
                };
                let mut difference =
                    graphs::difference(&map(field, "field"), &map(rays, "rays")).map_err(failed)?;
                difference.title = format!("Field minus rays on `{}`", target.id);
                difference.notes = vec![format!(
                    "field {:.6} W, rays {:.6} W on the detector",
                    outcome.field.power(),
                    traced.detectors[request.target].power
                )];
                Ok(difference)
            }
        };
    }
    if matches!(kind, "fluence-depth" | "absorption") {
        let light = crate::fluence::light(parameters)?;
        return Ok(if kind == "absorption" {
            graphs::absorption_map(&light)
        } else {
            graphs::fluence_depth(&light)
        });
    }
    if kind == "optimize" {
        let mut keys = base.to_vec();
        keys.extend_from_slice(OPTIMIZE_KEYS);
        let result = crate::optimize::optimize(&subset(parameters, &keys))?;
        return Ok(optimize_graph(&result));
    }
    let (scene, path) = moved_scene(parameters)?;
    let fail = |e: String| failed(format!("{path}: {e}"));
    match kind {
        "psf-axial" | "psf-lateral" | "psf-section" | "psf-wavefront" => {
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
                "psf-wavefront" => graphs::psf_wavefront(&psf),
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
        "gaussian" => {
            let source = match text(parameters, "source")? {
                Some(id) => scene
                    .sources
                    .iter()
                    .position(|s| s.id == id)
                    .ok_or_else(|| invalid(format!("no source `{id}`")))?,
                None => scene
                    .sources
                    .iter()
                    .position(|s| matches!(s.kind, optobench_scene::SourceKind::Gaussian { .. }))
                    .ok_or_else(|| invalid(format!("{path}: the design has no Gaussian source")))?,
            };
            let src = &scene.sources[source];
            let optobench_scene::SourceKind::Gaussian { m_squared, .. } = src.kind else {
                return Err(invalid(format!(
                    "source `{}` is not a Gaussian beam",
                    src.id
                )));
            };
            let beams = optobench_analysis::gaussian(&scene, source).map_err(fail)?;
            Ok(graphs::gaussian_path(&beams, src.wavelength, m_squared))
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

/// `graph`: a result as a graph. Either `path` (a design) and `of` with that analysis's
/// parameters, or `saved`, a graph saved earlier as JSON. `of` is one of:
///
/// - `psf-axial`, `psf-lateral`, `psf-section`, `psf-wavefront`: the PSF (`source`,
///   `detector`, `polarization`, `samples`, `axial`, `lateral`, `points`; `section` as
///   [columns, rows]);
/// - `focus`, `spot`, `irradiance`: the trace at `detector`;
/// - `gaussian`: a Gaussian `source` along its path;
/// - `sweep`: the `sweep` command's `dof`, `values` or `from`, `to`, `steps`, with
///   `quantity` (`centroid`, `power`, `rms`, `pointing` at `detector`, or `chief` at
///   `element`);
/// - `optimize`: the optimiser's merit by step (`merit`, `max_iterations`).
///
/// Every kind takes `dofs` and `data_directory`. Optionally compared: `compare` (saved graphs drawn on the
/// same axes; strings, or `{ "path", "name" }`), `difference` (a saved graph subtracted
/// from this one), `reference` (`{ "label", "points": [[x, y], …] }` in mm, W, W/mm²,
/// radians), `name` (this graph's name in a comparison; default `this`). Written to
/// `svg`, `csv` and `json` when given; the graph is also returned.
pub(crate) fn graph(parameters: &Parameters) -> Result<Value, CommandError> {
    let mut keys = vec![
        "path",
        "of",
        "saved",
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
    keys.extend_from_slice(SWEEP_KEYS);
    keys.extend_from_slice(OPTIMIZE_KEYS);
    keys.extend_from_slice(crate::field::FIELD_KEYS);
    keys.extend_from_slice(crate::fluence::LIGHT_KEYS);
    check_keys(parameters, &keys)?;
    let mut graph = match (text(parameters, "saved")?, text(parameters, "of")?) {
        (Some(from), None) => {
            if parameters.contains_key("path") {
                return Err(invalid(
                    "give `saved` (a saved graph) or `path` and `of`, not both",
                ));
            }
            read_saved(from)?
        }
        (None, Some(kind)) => of_design(parameters, kind)?,
        (Some(_), Some(_)) => {
            return Err(invalid(
                "give `saved` (a saved graph) or `path` and `of`, not both",
            ));
        }
        (None, None) => {
            return Err(invalid(format!(
                "missing `of` ({}) with `path`, or `saved`",
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
