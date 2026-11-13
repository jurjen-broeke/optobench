//! The `heating` command: the temperature rise of a sample from the light it absorbs
//! (`optobench_analysis::heating`).

use optobench_analysis::heating::{Heating, HeatingMode, HeatingSettings, ScanField, heating};
use serde_json::{Value, json};

use crate::psf::moved_scene;
use crate::trace::text;
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidParameters, message)
}

/// The parameters of `heating`, besides `path`, `dofs` and `data_directory`.
pub(crate) const HEATING_KEYS: &[&str] = &[
    "sample",
    "radius",
    "rings",
    "slices",
    "growth",
    "field_mm",
    "duty",
    "focal_depth_mm",
    "duration_s",
    "step_s",
    "off_at_s",
    "probes",
    "max_rise_k",
];

fn number(parameters: &Parameters, key: &str) -> Result<Option<f64>, CommandError> {
    match parameters.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite())
            .map(Some)
            .ok_or_else(|| invalid(format!("`{key}` must be a number"))),
    }
}

/// The heating the parameters ask for.
pub(crate) fn compute(parameters: &Parameters) -> Result<Heating, CommandError> {
    let (scene, path) = moved_scene(parameters)?;
    let failed = |e: String| CommandError::new(ErrorCode::Failed, format!("{path}: {e}"));
    let sample = match text(parameters, "sample")? {
        Some(id) => scene
            .elements
            .iter()
            .position(|e| e.id == id)
            .ok_or_else(|| invalid(format!("no element `{id}`")))?,
        None => scene
            .elements
            .iter()
            .position(|e| matches!(e.kind, optobench_scene::ElementKind::Sample { .. }))
            .ok_or_else(|| invalid(format!("{path}: the design has no sample")))?,
    };
    let mut settings = HeatingSettings::default();
    if let Some(r) = number(parameters, "radius")? {
        if r <= 0.0 {
            return Err(invalid("`radius` is the light grid's radius in mm, > 0"));
        }
        settings.light.radius = Some(r);
    }
    for (key, slot) in [
        ("rings", &mut settings.light.rings),
        ("slices", &mut settings.light.slices),
    ] {
        if let Some(v) = parameters.get(key) {
            *slot = v
                .as_u64()
                .filter(|n| (4..=2000).contains(n))
                .ok_or_else(|| invalid(format!("`{key}` is a whole number from 4 to 2000")))?
                as usize;
        }
    }
    if let Some(g) = number(parameters, "growth")? {
        if !(1.0..=2.0).contains(&g) {
            return Err(invalid("`growth` lies from 1 to 2"));
        }
        settings.growth = g;
    }
    settings.field = match (number(parameters, "field_mm")?, number(parameters, "duty")?) {
        (None, None) => None,
        (Some(side), duty) => Some(ScanField {
            side,
            duty: duty.unwrap_or(1.0),
        }),
        (None, Some(_)) => return Err(invalid("`duty` goes with `field_mm`")),
    };
    settings.focal_depth = number(parameters, "focal_depth_mm")?;
    settings.mode = match (
        number(parameters, "duration_s")?,
        number(parameters, "step_s")?,
    ) {
        (None, None) => {
            if parameters.contains_key("off_at_s") {
                return Err(invalid("`off_at_s` goes with `duration_s`"));
            }
            HeatingMode::Steady
        }
        (Some(duration), step) => HeatingMode::Time {
            duration,
            step: step.unwrap_or(duration / 200.0),
            off_at: number(parameters, "off_at_s")?,
        },
        (None, Some(_)) => return Err(invalid("`step_s` goes with `duration_s`")),
    };
    if let Some(list) = parameters.get("probes") {
        settings.probes = list
            .as_array()
            .ok_or_else(|| invalid("`probes` is a list of [radius_mm, depth_mm]"))?
            .iter()
            .map(|p| {
                p.as_array()
                    .filter(|a| a.len() == 2)
                    .and_then(|a| Some((a[0].as_f64()?, a[1].as_f64()?)))
                    .ok_or_else(|| invalid("`probes` is a list of [radius_mm, depth_mm]"))
            })
            .collect::<Result<_, _>>()?;
    }
    heating(&scene, sample, &settings).map_err(failed)
}

/// `heating`: parameters `path`, and optionally `sample` (an element id; default the first
/// sample, whose layers need `thermal` properties), `radius`, `rings`, `slices` (the light
/// grid, as for `fluence`), `growth` (the thermal grid's, default 1.1), `field_mm` (a
/// scanned square field's side) with `duty` (default 1), `focal_depth_mm` (default where
/// the direct light peaks on the axis), `duration_s` with `step_s` (default a 200th) and
/// `off_at_s` for a time course instead of the steady state, `probes` ([radius_mm,
/// depth_mm] followed in time; default the focus and the surface above it), `max_rise_k`
/// (warn above it), `dofs` and `data_directory`.
pub(crate) fn command(parameters: &Parameters) -> Result<Value, CommandError> {
    let mut keys = vec!["path", "dofs", "data_directory"];
    keys.extend_from_slice(HEATING_KEYS);
    check_keys(parameters, &keys)?;
    let h = compute(parameters)?;
    let mut warnings = Vec::new();
    if let Some(limit) = number(parameters, "max_rise_k")? {
        if h.peak.0 > limit {
            warnings.push(format!(
                "the rise reaches {:.3} K, above the {limit} K set (at r {:.3} mm, depth {:.3} mm)",
                h.peak.0, h.peak.1, h.peak.2
            ));
        }
    }
    let course = h.course.as_ref().map(|c| {
        json!({
            "times_s": c.times,
            "probes": h.probes.iter().zip(&c.probes).map(|(&(r, z), history)| json!({
                "radius_mm": r, "depth_mm": z, "rise_k": history,
            })).collect::<Vec<_>>(),
        })
    });
    let balance = match h.stored {
        None => json!({
            "absorbed_w": h.absorbed,
            "conducted_top_w": h.conducted[0],
            "conducted_bottom_w": h.conducted[1],
            "conducted_side_w": h.conducted[2],
            "perfused_w": h.perfused,
        }),
        Some(stored) => {
            let absorbed = h.course.as_ref().map_or(0.0, |c| c.absorbed);
            json!({
                "absorbed_j": absorbed,
                "stored_j": stored,
                "conducted_top_j": h.conducted[0],
                "conducted_bottom_j": h.conducted[1],
                "conducted_side_j": h.conducted[2],
                "perfused_j": h.perfused,
            })
        }
    };
    Ok(json!({
        "sample": h.sample,
        "surface_power_w": h.surface_power,
        "focal_depth_mm": h.focal_depth,
        "focal_power_w": h.focal_power,
        "absorbed_w": h.absorbed,
        "absorbed_by_layer": h.absorbed_by_layer.iter().map(|(part, w)| json!({ "part": part, "w": w })).collect::<Vec<_>>(),
        "peak_rise_k": h.peak.0,
        "peak_at": { "radius_mm": h.peak.1, "depth_mm": h.peak.2 },
        "rise_at_focus_k": h.at_focus,
        "field_average_rise_k": h.field_average,
        "rise_per_100_mw_at_focus_k": if h.surface_power > 0.0 { Some(h.at_focus * 0.1 / h.surface_power) } else { None },
        "balance": balance,
        "course": course,
        "grid": { "rings": h.grid.rings(), "slices": h.grid.slices() },
        "warnings": warnings,
        "notes": h.notes,
    }))
}
