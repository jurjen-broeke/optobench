//! `sweep`: trace a design over a degree of freedom's values (M9b), as a galvo or a scan
//! mirror is swept: per value, every detector's direct power, centroid and pointing, and
//! the chief ray's path element by element — where it crosses each element's plane and
//! at what slopes (pupil walk at an objective's pupil, scan angle at a lens) — and where
//! it lands on its detector and how steeply (telecentricity at the image).
//!
//! The chief ray is the ray each source launches from its centre along its axis.

use optobench_scene::Owner;
use optobench_trace::nonsequential::End;
use serde_json::{Value, json};

use crate::trace::{load, text};
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidParameters, message)
}

fn failed(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::Failed, message)
}

/// `sweep`: parameters `path` and `dof` (a name), with `values` (a list, mm or degrees)
/// or `from`, `to` and `steps` (≥ 2, evenly spaced); optionally `dofs` (other DOFs held
/// at these values) and `data_directory`. Returns per value the detectors and the chief
/// rays.
pub(crate) fn sweep(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(
        parameters,
        &[
            "path",
            "dof",
            "values",
            "from",
            "to",
            "steps",
            "dofs",
            "data_directory",
        ],
    )?;
    let path = text(parameters, "path")?.ok_or_else(|| invalid("missing parameter `path`"))?;
    let name = text(parameters, "dof")?.ok_or_else(|| invalid("missing parameter `dof`"))?;
    let values: Vec<f64> = match (parameters.get("values"), parameters.get("steps")) {
        (Some(list), None) => list
            .as_array()
            .ok_or_else(|| invalid("`values` must be a list of numbers"))?
            .iter()
            .map(|v| {
                v.as_f64()
                    .ok_or_else(|| invalid("`values` must be numbers"))
            })
            .collect::<Result<_, _>>()?,
        (None, Some(steps)) => {
            let steps = steps
                .as_u64()
                .filter(|n| *n >= 2 && *n <= 10_000)
                .ok_or_else(|| invalid("`steps` must be a whole number from 2 to 10000"))?
                as usize;
            let number = |key: &str| {
                parameters
                    .get(key)
                    .and_then(Value::as_f64)
                    .ok_or_else(|| invalid(format!("`{key}` must be a number")))
            };
            let (from, to) = (number("from")?, number("to")?);
            (0..steps)
                .map(|k| from + (to - from) * k as f64 / (steps - 1) as f64)
                .collect()
        }
        _ => return Err(invalid("give `values`, or `from`, `to` and `steps`")),
    };
    let (scene, _, _, _) = load(path, parameters)?;
    let held = crate::trace::dof_values(parameters, &scene)?;
    let dof = scene
        .dofs
        .iter()
        .find(|d| d.name == name)
        .ok_or_else(|| invalid(format!("no degree of freedom `{name}`")))?;
    if dof.locked {
        return Err(invalid(format!("`{name}` is locked")));
    }
    let angle = dof.motion.is_angle();
    let mut steps = Vec::with_capacity(values.len());
    for value in values {
        let mut overrides = held.clone();
        overrides.insert(
            name.to_owned(),
            if angle { value.to_radians() } else { value },
        );
        let moved = scene
            .moved(&overrides)
            .map_err(|e| failed(format!("{path}: {e}")))?;
        let built = moved.build().map_err(|e| failed(format!("{path}: {e}")))?;
        let traced = moved.trace().map_err(|e| failed(format!("{path}: {e}")))?;
        let detectors: Vec<Value> = traced
            .detectors
            .iter()
            .map(|d| {
                json!({
                    "id": d.id,
                    "direct_power_w": d.direct_power,
                    "direct_centroid_mm": [d.direct_centroid.0, d.direct_centroid.1],
                    "direct_rms_radius_mm": d.direct_rms_radius,
                    "direct_mean_slope_mrad": [d.direct_mean_slope.0 * 1e3, d.direct_mean_slope.1 * 1e3],
                })
            })
            .collect();
        let chief: Vec<Value> = moved
            .sources
            .iter()
            .filter_map(|source| {
                let origin = source.frame.origin();
                let axis = source.frame.axes()[2];
                let record = traced.records.iter().find(|r| {
                    r.generation == 0
                        && (r.launched.position - origin).length() < 1e-9
                        && (r.launched.direction - axis).length() < 1e-9
                })?;
                // Walk the path: `points[k + 1]` is where the ray met `faces[k]`.
                let mut hits = Vec::new();
                for (k, &face) in record.faces.iter().enumerate() {
                    let j = k + 1;
                    let (Some(&at), Some(&before)) =
                        (record.points.get(j), record.points.get(j - 1))
                    else {
                        break;
                    };
                    let role = &built.system.faces[face].role;
                    // An objective's second entry is where the ray leaves it.
                    if k > 0
                        && record.faces[k - 1] == face
                        && role.records_twice()
                    {
                        continue;
                    }
                    if let Some(Owner::Element(e)) = built.owners.get(built.system.faces[face].owner) {
                        let element = &moved.elements[*e];
                        let local = element.frame.to_local_point(at);
                        let incoming = element.frame.to_local_vector(at - before);
                        let slope = if incoming.z.abs() > 1e-15 {
                            json!([incoming.x / incoming.z.abs(), incoming.y / incoming.z.abs()])
                        } else {
                            Value::Null
                        };
                        hits.push(json!({
                            "element": element.id,
                            "position_mm": [local.x, local.y],
                            "slope": slope,
                        }));
                    }
                }
                let landed = match record.end {
                    End::Detected { detector, .. } => {
                        let d = &moved.detectors[detector];
                        let point = d.frame.to_local_point(record.ray.position);
                        let direction = d.frame.to_local_vector(record.ray.direction);
                        json!({
                            "detector": d.id,
                            "position_mm": [point.x, point.y],
                            "slope": [direction.x / direction.z.abs(), direction.y / direction.z.abs()],
                        })
                    }
                    _ => Value::Null,
                };
                Some(json!({
                    "source": source.id,
                    "hits": hits,
                    "landed": landed,
                }))
            })
            .collect();
        steps.push(json!({
            "value": value,
            "detectors": detectors,
            "chief": chief,
        }));
    }
    Ok(json!({
        "dof": name,
        "unit": if angle { "deg" } else { "mm" },
        "steps": steps,
    }))
}
