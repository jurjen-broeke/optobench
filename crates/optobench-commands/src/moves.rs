//! The move report and the re-export diff (M6c; docs/design.md, Phase A steps 4 and 5).
//!
//! `optimize` with `report` writes `<design>.moves.json` and `<design>.moves.md`: per DOF
//! the current and suggested value, and per moved item the move to make in CAD (from the
//! pose in the STEP file, where its DOFs are 0): a translation and a rotation in the
//! assembly frame, the 3 × 4 matrix and the SolidCraft command that makes it. After the
//! moves are made and the assembly exported again, `check-moves` compares every body's
//! pose with what the report expects.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use optobench_math::{Transform, Vec3};
use optobench_optimize::{Optimised, residuals};
use optobench_scene::Scene;
use optobench_scene::dof::Target;
use serde_json::{Value, json};

use crate::trace::load;
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn failed(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::Failed, message)
}

/// A transform as a row-major 3 × 4 matrix (rotation and translation).
pub(crate) fn matrix(t: &Transform) -> [f64; 12] {
    let [x, y, z] = [Vec3::X, Vec3::Y, Vec3::Z].map(|e| t.apply_vector(e));
    let o = t.translation;
    [x.x, y.x, z.x, o.x, x.y, y.y, z.y, o.y, x.z, y.z, z.z, o.z]
}

/// The rotation of a transform as a unit axis and an angle (degrees).
fn axis_angle(t: &Transform) -> (Vec3, f64) {
    let m = matrix(t);
    let (r11, r12, r13, r21, r22, r23, r31, r32, r33) =
        (m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]);
    let cos = ((r11 + r22 + r33 - 1.0) / 2.0).clamp(-1.0, 1.0);
    let angle = cos.acos();
    let axis = Vec3::new(r32 - r23, r13 - r31, r21 - r12);
    match axis.normalized() {
        Some(a) if angle > 1e-15 => (a, angle.to_degrees()),
        _ => (Vec3::Z, 0.0),
    }
}

fn v(p: Vec3) -> Value {
    json!([p.x, p.y, p.z])
}

fn target_json(target: &Target) -> (&'static str, String) {
    match target {
        Target::Element(id) => ("element", id.clone()),
        Target::Source(id) => ("source", id.clone()),
        Target::Detector(id) => ("detector", id.clone()),
        Target::Occurrence(path) => ("occurrence", path.clone()),
    }
}

/// The pose of every CAD solid (its first face's placement).
fn poses(scene: &Scene) -> BTreeMap<String, Transform> {
    scene
        .solids
        .iter()
        .filter_map(|s| s.faces.first().map(|f| (s.id.clone(), f.placement)))
        .collect()
}

fn zeros(scene: &Scene) -> BTreeMap<String, f64> {
    // Those that follow another take their 0 from it.
    scene
        .dofs
        .iter()
        .filter(|d| d.follows.is_none())
        .map(|d| (d.name.clone(), 0.0))
        .collect()
}

/// The report for an optimisation of `scene` (as read from `design`), in JSON and
/// Markdown.
pub(crate) fn report(
    scene: &Scene,
    result: &Optimised,
    dofs: &[Value],
    merit: &[Value],
    design: &Path,
) -> Result<(Value, String), CommandError> {
    // The DOFs that lead or stand alone: those that follow take their values from them.
    let current: BTreeMap<String, f64> = scene
        .dofs
        .iter()
        .filter(|d| d.follows.is_none())
        .map(|d| (d.name.clone(), d.value))
        .collect();
    let mut leading = current.clone();
    leading.extend(result.end());
    // Every DOF's value, those that follow included (for each item's chain of moves).
    let suggested = scene
        .dof_values(&leading)
        .map_err(|e| failed(e.to_string()))?;
    let zero = zeros(scene);
    let mut moves = Vec::new();
    for target in scene.dof_targets() {
        let t = scene.movement(&target, &zero, &suggested);
        let (kind, name) = target_json(&target);
        let (axis, angle) = axis_angle(&t);
        let mut entry = json!({
            "kind": kind,
            "name": name,
            "dofs": scene.dofs.iter().filter(|d| d.target == target).map(|d| d.name.clone()).collect::<Vec<_>>(),
            "translate_mm": v(t.translation),
            "rotate_axis": v(axis),
            "rotate_deg": angle,
            "matrix": matrix(&t),
        });
        if kind == "occurrence" {
            entry["solidcraft"] = json!({
                "command": "occurrence.move",
                "params": { "occurrence": name, "space": "world", "matrix": matrix(&t) },
            });
        }
        moves.push(entry);
    }
    let step_scene = scene.moved(&zero).map_err(|e| failed(e.to_string()))?;
    let end_scene = scene.moved(&leading).map_err(|e| failed(e.to_string()))?;
    let before = poses(&step_scene);
    let after = poses(&end_scene);
    let pose_list: Vec<Value> = before
        .iter()
        .map(|(id, b)| json!({ "solid": id, "before": matrix(b), "expected": matrix(&after[id]) }))
        .collect();
    let report = json!({
        "format": optobench_io::format_tag(),
        "kind": "moves",
        "design": design.to_string_lossy(),
        "dofs": dofs,
        "merit": merit,
        "moves": moves,
        "poses": pose_list,
        "next": if scene.solids.is_empty() {
            "Set each DOF's `value` in the layout to the suggested one."
        } else {
            "Make each move in CAD (the matrix is in the assembly frame, applied to the pose in the STEP file), export the assembly again, set every DOF's `value` back to 0 and run `check-moves` with this report. A move made with a part's adjuster uses its travel: give that DOF its `range_after_adjusting` (in `dofs`); a part re-placed in the design keeps its range."
        },
    });
    // Markdown, for people.
    let f = |x: f64| format!("{x:.4}");
    let mut md = format!(
        "# Move report for `{}`\n\nSuggested by `optobench optimize`. Values are mm and degrees; moves are in the assembly (scene) frame, from the pose in the file where every DOF is 0.\n\n## Degrees of freedom\n\n| DOF | Unit | Current | Suggested | Change | At range limit |\n| --- | --- | ---: | ---: | ---: | --- |\n",
        design.display()
    );
    for d in dofs {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            d["name"].as_str().unwrap_or("?"),
            d["unit"].as_str().unwrap_or("?"),
            f(d["start"].as_f64().unwrap_or(f64::NAN)),
            f(d["suggested"].as_f64().unwrap_or(f64::NAN)),
            f(d["change"].as_f64().unwrap_or(f64::NAN)),
            if d["at_range_limit"] == true {
                "yes"
            } else {
                "no"
            }
        ));
    }
    if dofs.iter().any(|d| d["room"].is_object()) {
        md.push_str("\n## Adjustment room\n\nHow much of each adjuster's travel towards one end the suggested value uses (0 % in the middle of its range, 100 % at an end), the travel left each way, and the range to give the DOF if the move is made with the adjuster and the design exported again.\n\n| DOF | Range | Suggested | Used | Left (−, +) | Range after adjusting | Status |\n| --- | --- | ---: | ---: | --- | --- | --- |\n");
        let pair = |v: &Value| {
            format!(
                "{} to {}",
                f(v[0].as_f64().unwrap_or(f64::NAN)),
                f(v[1].as_f64().unwrap_or(f64::NAN))
            )
        };
        for d in dofs.iter().filter(|d| d["room"].is_object()) {
            let r = &d["room"];
            md.push_str(&format!(
                "| {} | {} | {} | {:.0} % | {} | {} | {} |\n",
                d["name"].as_str().unwrap_or("?"),
                pair(&r["range"]),
                f(d["suggested"].as_f64().unwrap_or(f64::NAN)),
                100.0 * r["used"].as_f64().unwrap_or(f64::NAN).abs(),
                pair(&r["left"]),
                pair(&r["range_after_adjusting"]),
                r["status"].as_str().unwrap_or("?")
            ));
        }
        for d in dofs
            .iter()
            .filter(|d| d["room"].is_object() && d["room"]["status"] != "ok")
        {
            md.push_str(&format!(
                "\n- `{}`: {}.",
                d["name"].as_str().unwrap_or("?"),
                d["room"]["advice"].as_str().unwrap_or("")
            ));
        }
        md.push('\n');
    }
    md.push_str("\n## Moves\n\n| Item | Translate (mm) | Rotate about | By (deg) |\n| --- | --- | --- | ---: |\n");
    for m in &moves {
        let three = |key: &str| {
            m[key]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|x| f(x.as_f64().unwrap_or(f64::NAN)))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default()
        };
        md.push_str(&format!(
            "| {} `{}` | ({}) | ({}) | {} |\n",
            m["kind"].as_str().unwrap_or("?"),
            m["name"].as_str().unwrap_or("?"),
            three("translate_mm"),
            three("rotate_axis"),
            f(m["rotate_deg"].as_f64().unwrap_or(f64::NAN))
        ));
    }
    md.push_str("\n## Merit\n\n| Term | Detector | Unit | Before | After |\n| --- | --- | --- | --- | --- |\n");
    for m in merit {
        let list = |key: &str| {
            m[key]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|x| format!("{:.6}", x.as_f64().unwrap_or(f64::NAN)))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default()
        };
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            m["kind"].as_str().unwrap_or("?"),
            m["detector"].as_str().unwrap_or("?"),
            m["unit"].as_str().unwrap_or("?"),
            list("start"),
            list("end")
        ));
    }
    md.push_str(&format!(
        "\nNext: {}\n",
        report["next"].as_str().unwrap_or("")
    ));
    if !scene.solids.is_empty() {
        md.push_str("\nFor SolidCraft, each move as one command (`occurrence.move`, world space):\n\n```json\n");
        for m in &moves {
            if let Some(c) = m.get("solidcraft") {
                md.push_str(&format!("{c}\n"));
            }
        }
        md.push_str("```\n");
    }
    Ok((report, md))
}

/// Where the report goes: `report: true` next to the design, or a path stem.
pub(crate) fn report_paths(
    design: &Path,
    parameter: &Value,
    suffix: &str,
) -> Result<Option<(PathBuf, PathBuf)>, CommandError> {
    let stem = match parameter {
        Value::Bool(false) | Value::Null => return Ok(None),
        Value::Bool(true) => design.with_extension(""),
        Value::String(s) => PathBuf::from(s),
        _ => {
            return Err(CommandError::new(
                ErrorCode::InvalidParameters,
                "`report` is true (next to the design) or a path stem",
            ));
        }
    };
    let name = stem
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Some((
        stem.with_file_name(format!("{name}.{suffix}.json")),
        stem.with_file_name(format!("{name}.{suffix}.md")),
    )))
}

fn transform_from(m: &[Value]) -> Option<Transform> {
    let n: Vec<f64> = m.iter().map(Value::as_f64).collect::<Option<_>>()?;
    if n.len() != 12 {
        return None;
    }
    let origin = Vec3::new(n[3], n[7], n[11]);
    let frame = optobench_math::Frame::new(
        origin,
        Vec3::new(n[2], n[6], n[10]),
        Vec3::new(n[0], n[4], n[8]),
    )?;
    Some(frame.transform())
}

/// `check-moves`: parameters `path` (the sidecar, now of the re-exported STEP file) and
/// `report` (the `.moves.json` file); optionally `tolerance_mm` (default 0.01),
/// `tolerance_deg` (0.01) and `data_directory`. Compares every body's pose in the new
/// export with the report's expectation, and gives the merit as it is now.
pub(crate) fn check_moves(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(
        parameters,
        &[
            "path",
            "report",
            "tolerance_mm",
            "tolerance_deg",
            "data_directory",
        ],
    )?;
    let text = |key: &str| {
        parameters.get(key).and_then(Value::as_str).ok_or_else(|| {
            CommandError::new(
                ErrorCode::InvalidParameters,
                format!("missing parameter `{key}`"),
            )
        })
    };
    let path = text("path")?;
    let report_path = text("report")?;
    let number = |key: &str, default: f64| -> Result<f64, CommandError> {
        match parameters.get(key) {
            None => Ok(default),
            Some(v) => v.as_f64().filter(|x| *x > 0.0).ok_or_else(|| {
                CommandError::new(
                    ErrorCode::InvalidParameters,
                    format!("`{key}` must be positive"),
                )
            }),
        }
    };
    let (tolerance_mm, tolerance_deg) = (
        number("tolerance_mm", 0.01)?,
        number("tolerance_deg", 0.01)?,
    );
    let report: Value = serde_json::from_str(
        &std::fs::read_to_string(report_path).map_err(|e| failed(format!("{report_path}: {e}")))?,
    )
    .map_err(|e| failed(format!("{report_path}: {e}")))?;
    if report["kind"] != "moves" {
        return Err(failed(format!("{report_path}: not a move report")));
    }
    let (scene, _, source, _) = load(path, parameters)?;
    let nonzero: Vec<String> = scene
        .dofs
        .iter()
        .filter(|d| d.value != 0.0)
        .map(|d| d.name.clone())
        .collect();
    // The poses in the new STEP file are the ones with every DOF at 0.
    let step_scene = scene
        .moved(&zeros(&scene))
        .map_err(|e| failed(e.to_string()))?;
    let now = poses(&step_scene);
    let mut checks = Vec::new();
    let mut all_ok = true;
    for entry in report["poses"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let id = entry["solid"].as_str().unwrap_or_default();
        let expected = entry["expected"]
            .as_array()
            .and_then(|m| transform_from(m))
            .ok_or_else(|| {
                failed(format!(
                    "{report_path}: pose of `{id}` is not a 3 × 4 matrix"
                ))
            })?;
        let Some(actual) = now.get(id) else {
            all_ok = false;
            checks.push(json!({ "solid": id, "ok": false, "problem": "not in the new export" }));
            continue;
        };
        let difference = *actual * expected.inverse();
        let moved_by = (actual.translation - expected.translation).length();
        let (_, turned_by) = axis_angle(&difference);
        let ok = moved_by <= tolerance_mm && turned_by <= tolerance_deg;
        all_ok &= ok;
        checks.push(json!({
            "solid": id,
            "ok": ok,
            "off_by_mm": moved_by,
            "turned_by_deg": turned_by,
        }));
    }
    let merit = optobench_io::merit::parse(&source).map_err(|e| failed(format!("{path}: {e}")))?;
    let traced = step_scene
        .trace()
        .map_err(|e| failed(format!("{path}: {e}")))?;
    let now_merit = residuals(&merit, &traced).map_err(|e| failed(format!("{path}: {e}")))?;
    let mut result = json!({
        "ok": all_ok,
        "poses": checks,
        "merit": merit.iter().zip(&now_merit).map(|(m, r)| json!({
            "kind": m.term.name(),
            "detector": m.detector,
            "now": r,
        })).collect::<Vec<_>>(),
        "suggested_merit": report["merit"],
    });
    if !nonzero.is_empty() {
        result["note"] = json!(format!(
            "these DOFs are not 0 in the sidecar: {}; the new export is the nominal now, so set them back to 0",
            nonzero.join(", ")
        ));
    }
    Ok(result)
}

/// `cad-moves` (M13): for DOF values asked for, what to change in the CAD design. Parameters
/// `path` (a sidecar), `dofs` (`{ "<name>": value }`, mm for slides, degrees for tilts;
/// others keep the file's), and `data_directory`, `step` and `moves` as `trace` takes them.
/// A DOF's value in the file is what optobench adds to the CAD design, which stands where
/// every DOF is 0 (the STEP file's pose). So for each CAD occurrence a DOF moves: its
/// move in the assembly frame from that pose to where the values put it (a 4 × 4
/// row-major matrix, with its translation and rotation in words; `changed` when it moves
/// at all), and for DOFs with a `binding` the parameter or joint change, the DOF's value
/// itself; the DOFs of one part are all bound or none. A part whose DOFs are not asked
/// for moves by the file's values, so the design comes to match the model. Sources and
/// detectors of a sidecar are not CAD parts: listed under `skipped` (`changed` when a
/// value asked for differs from the file's), their DOFs stay in the file. `current` gives
/// the file's values, `values` those asked for (mm or degrees).
pub(crate) fn cad_moves(parameters: &Parameters) -> Result<Value, CommandError> {
    use optobench_scene::dof::{Binding, Motion};
    check_keys(
        parameters,
        &["path", "dofs", "data_directory", "step", "moves"],
    )?;
    let invalid = |m: String| CommandError::new(ErrorCode::InvalidParameters, m);
    let path = parameters
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing parameter `path`".into()))?;
    let (scene, _, _, _) = load(path, parameters)?;
    let asked = parameters
        .get("dofs")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("`dofs` is { \"<name>\": value } (mm or degrees)".into()))?;
    let degrees = |name: &str| {
        scene
            .dofs
            .iter()
            .find(|d| d.name == name)
            .is_some_and(|d| matches!(d.motion, Motion::Tilt { .. }))
    };
    let mut overrides = BTreeMap::new();
    for (name, value) in asked {
        let value = value
            .as_f64()
            .ok_or_else(|| invalid(format!("the value of `{name}` is not a number")))?;
        let scale = if degrees(name) {
            std::f64::consts::PI / 180.0
        } else {
            1.0
        };
        overrides.insert(name.clone(), value * scale);
    }
    let current = scene
        .dof_values(&BTreeMap::new())
        .map_err(|e| failed(e.to_string()))?;
    let wanted = scene
        .dof_values(&overrides)
        .map_err(|e| invalid(e.to_string()))?;
    // A change in the DOF's own unit: mm, or degrees for a tilt.
    // A CAD part's change: from 0, where the design stands, in mm or degrees.
    let change = |name: &str| {
        let c = wanted[name];
        if degrees(name) { c.to_degrees() } else { c }
    };
    let zero = zeros(&scene);
    let mut items = Vec::new();
    let mut skipped = Vec::new();
    for target in scene.dof_targets() {
        let dofs: Vec<&optobench_scene::dof::Dof> =
            scene.dofs.iter().filter(|d| d.target == target).collect();
        let names: Vec<&str> = dofs.iter().map(|d| d.name.as_str()).collect();
        let changed = names.iter().any(|n| wanted[*n].abs() > 0.0);
        let Target::Occurrence(occurrence) = &target else {
            let (kind, name) = target_json(&target);
            let differs = names.iter().any(|n| (wanted[*n] - current[*n]).abs() > 0.0);
            skipped.push(json!({
                "kind": kind, "name": name, "dofs": names, "changed": differs,
                "why": "not a CAD part: its degrees of freedom stay in the file",
            }));
            continue;
        };
        let bound = dofs.iter().filter(|d| d.binding.is_some()).count();
        if bound > 0 && bound < dofs.len() {
            return Err(invalid(format!(
                "`{occurrence}`: bind all its degrees of freedom ({}) or none",
                names.join(", ")
            )));
        }
        let t = scene.movement(&target, &zero, &wanted);
        let m = matrix(&t);
        let (axis, angle) = axis_angle(&t);
        let bindings: Value = if bound == 0 {
            Value::Null
        } else {
            json!(
                dofs.iter()
                    .map(|d| {
                        let unit = if degrees(&d.name) { "deg" } else { "mm" };
                        let mut b =
                            json!({ "dof": d.name, "change": change(&d.name), "unit": unit });
                        match &d.binding {
                            Some(Binding::Parameter(p)) => b["parameter"] = json!(p),
                            Some(Binding::Joint { name, index }) => {
                                b["joint"] = json!(name);
                                b["index"] = json!(index.unwrap_or(0));
                            }
                            None => {}
                        }
                        b
                    })
                    .collect::<Vec<_>>()
            )
        };
        items.push(json!({
            "occurrence": occurrence,
            "dofs": names,
            "changed": changed,
            "matrix": [[m[0], m[1], m[2], m[3]], [m[4], m[5], m[6], m[7]],
                       [m[8], m[9], m[10], m[11]], [0.0, 0.0, 0.0, 1.0]],
            "translate_mm": v(t.translation),
            "rotate_axis": v(axis),
            "rotate_deg": angle,
            "bindings": bindings,
        }));
    }
    let in_units = |values: &BTreeMap<String, f64>| -> serde_json::Map<String, Value> {
        values
            .iter()
            .map(|(n, w)| {
                (
                    n.clone(),
                    json!(if degrees(n) { w.to_degrees() } else { *w }),
                )
            })
            .collect()
    };
    Ok(json!({
        "items": items,
        "skipped": skipped,
        "values": in_units(&wanted),
        "current": in_units(&current),
        // Each DOF's travel, in its unit, where it has one.
        "ranges": scene.dofs.iter().filter_map(|d| {
            let (low, high) = d.range?;
            let k = if degrees(&d.name) { 180.0 / std::f64::consts::PI } else { 1.0 };
            Some((d.name.clone(), json!([low * k, high * k])))
        }).collect::<serde_json::Map<String, Value>>(),
    }))
}

/// `clearance` (M18): how far each free DOF (`range = "free"`) of a design can move each
/// way before its parts overlap another part or, on a slide, reach the end of what guides
/// them (M20; `optobench_scene::clearance`). Parameters `path`, optionally `dof` (one
/// DOF, free or not), `tolerance` (mm, default 0.05), `spacing` (mm, default 1),
/// `guide_gap` (mm, default 0.5: the most play a guide may have), and `data_directory`,
/// `step` and `moves` as `trace` takes them. Per DOF its range (mm or degrees, from its
/// value now), what stops it each way (`null`: nothing within the scene's size, or half
/// a turn) and whether that is the end of a guide (`end_of_guide`) rather than an
/// overlap.
pub(crate) fn clearance(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(
        parameters,
        &[
            "path",
            "dof",
            "tolerance",
            "spacing",
            "guide_gap",
            "data_directory",
            "step",
            "moves",
        ],
    )?;
    let invalid = |m: String| CommandError::new(ErrorCode::InvalidParameters, m);
    let path = parameters
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing parameter `path`".into()))?;
    let (scene, _, _, _) = load(path, parameters)?;
    let mut settings = optobench_scene::clearance::Settings::default();
    for (key, slot) in [
        ("tolerance", &mut settings.tolerance),
        ("spacing", &mut settings.spacing),
        ("guide_gap", &mut settings.guide_gap),
    ] {
        if let Some(v) = parameters.get(key) {
            *slot = v
                .as_f64()
                .filter(|x| *x > 0.0)
                .ok_or_else(|| invalid(format!("`{key}` is a length in mm > 0")))?;
        }
    }
    let names: Vec<String> = match parameters.get("dof").and_then(Value::as_str) {
        Some(name) => vec![name.to_owned()],
        None => scene
            .dofs
            .iter()
            .filter(|d| d.free)
            .map(|d| d.name.clone())
            .collect(),
    };
    let dofs = names
        .iter()
        .map(|name| {
            let dof = scene
                .dofs
                .iter()
                .find(|d| &d.name == name)
                .ok_or_else(|| invalid(format!("no degree of freedom `{name}`")))?;
            let found = scene
                .clearance(name, &settings)
                .map_err(|e| failed(e.to_string()))?;
            let k = if dof.motion.is_angle() {
                180.0 / std::f64::consts::PI
            } else {
                1.0
            };
            Ok(json!({
                "name": name,
                "unit": if dof.motion.is_angle() { "deg" } else { "mm" },
                "value": dof.value * k,
                "range": [found.range.0 * k, found.range.1 * k],
                "stopped_by": [
                    found.stopped_by.0.as_ref().map(|s| &s.part),
                    found.stopped_by.1.as_ref().map(|s| &s.part),
                ],
                "end_of_guide": [
                    found.stopped_by.0.as_ref().is_some_and(|s| s.end),
                    found.stopped_by.1.as_ref().is_some_and(|s| s.end),
                ],
            }))
        })
        .collect::<Result<Vec<Value>, CommandError>>()?;
    Ok(json!({
        "dofs": dofs,
        "tolerance_mm": settings.tolerance,
        "spacing_mm": settings.spacing,
        "guide_gap_mm": settings.guide_gap,
    }))
}
