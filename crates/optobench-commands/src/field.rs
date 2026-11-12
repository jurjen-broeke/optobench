//! The `field` command: a source's light carried as a field from a plane to a detector,
//! through ideal lenses and mechanics, optionally into a fibre and on as rays
//! (`optobench_field::scene`).

use optobench_field::scene::{Outcome, Request, Start, run};
use optobench_math::Complex;
use serde_json::{Value, json};

use crate::psf::moved_scene;
use crate::trace::text;
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidParameters, message)
}

/// The parameters of a field, besides `path`, `dofs` and `data_directory`.
pub(crate) const FIELD_KEYS: &[&str] = &[
    "source",
    "start",
    "target",
    "grid",
    "padding",
    "samples",
    "polarization",
    "fibre",
    "rays",
    "threshold",
];

/// The request the parameters in [`FIELD_KEYS`] describe.
pub(crate) fn request(
    parameters: &Parameters,
    scene: &optobench_scene::Scene,
    path: &str,
) -> Result<Request, CommandError> {
    let detector = |id: &str| {
        scene
            .detectors
            .iter()
            .position(|d| d.id == id)
            .ok_or_else(|| invalid(format!("no detector `{id}`")))
    };
    let source = match text(parameters, "source")? {
        Some(id) => scene
            .sources
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| invalid(format!("no source `{id}`")))?,
        None => scene
            .sources
            .iter()
            .position(|s| {
                matches!(
                    s.kind,
                    optobench_scene::SourceKind::Collimated { .. }
                        | optobench_scene::SourceKind::Gaussian { .. }
                )
            })
            .ok_or_else(|| invalid(format!("{path}: no collimated or Gaussian source")))?,
    };
    let target = detector(
        text(parameters, "target")?
            .ok_or_else(|| invalid("missing `target` (the detector the field is carried to)"))?,
    )?;
    let start = match text(parameters, "start")? {
        None => Start::Source,
        Some(id) => Start::Detector(detector(id)?),
    };
    let mut request = Request::new(source, start, target);
    if let Some(v) = parameters.get("grid") {
        request.grid = v
            .as_u64()
            .filter(|n| *n >= 16 && *n <= 4096 && n % 2 == 0)
            .ok_or_else(|| invalid("`grid` is an even number of samples, 16 to 4096"))?
            as usize;
    }
    if let Some(v) = parameters.get("padding") {
        request.padding = v
            .as_f64()
            .filter(|p| *p >= 1.0)
            .ok_or_else(|| invalid("`padding` is at least 1 (grid width over beam width)"))?;
    }
    if let Some(v) = parameters.get("samples") {
        request.samples =
            v.as_u64()
                .filter(|n| *n >= 8)
                .ok_or_else(|| invalid("`samples` is a whole number ≥ 8"))? as usize;
    }
    if let Some(v) = parameters.get("threshold") {
        request.threshold = v
            .as_f64()
            .filter(|t| (0.0..1.0).contains(t))
            .ok_or_else(|| invalid("`threshold` is a fraction of the peak, 0 to 1"))?;
    }
    request.polarization = match text(parameters, "polarization")? {
        None | Some("x") => (Complex::ONE, Complex::ZERO),
        Some("y") => (Complex::ZERO, Complex::ONE),
        Some("circular") => (Complex::ONE, Complex::I),
        Some(other) => {
            return Err(invalid(format!(
                "polarization `{other}` is not known; use \"x\", \"y\" or \"circular\""
            )));
        }
    };
    if let Some(v) = parameters.get("fibre") {
        let diameter = v["mode_field_diameter"]
            .as_f64()
            .filter(|d| *d > 0.0)
            .ok_or_else(|| invalid("`fibre` is { \"mode_field_diameter\": mm }"))?;
        request.fibre = Some(diameter / 2.0);
    }
    if let Some(v) = parameters.get("rays") {
        request.rays = v
            .as_bool()
            .ok_or_else(|| invalid("`rays` is true or false"))?;
    }
    Ok(request)
}

/// The design's scene and the field's outcome.
pub(crate) fn outcome(
    parameters: &Parameters,
) -> Result<(optobench_scene::Scene, Request, Outcome), CommandError> {
    let (scene, path) = moved_scene(parameters)?;
    let request = request(parameters, &scene, &path)?;
    let outcome = run(&scene, &request)
        .map_err(|e| CommandError::new(ErrorCode::Failed, format!("{path}: {e}")))?;
    Ok((scene, request, outcome))
}

/// `field`: parameters `path` and `target` (a detector), and optionally `source`,
/// `start` (a detector where the traced rays become the field; default the source's own
/// plane and exact field), `grid` (samples per side, default 256), `padding` (grid width
/// over beam width, default 2), `samples` (rays across the beam for `start`, default
/// 128), `polarization` (`x`, `y`, `circular`), `fibre` (`{ "mode_field_diameter": mm }`:
/// coupling at the target), `rays` (true: on as rays from the target), `threshold`,
/// `dofs` and `data_directory`.
pub(crate) fn field(parameters: &Parameters) -> Result<Value, CommandError> {
    let mut keys = vec!["path", "dofs", "data_directory"];
    keys.extend_from_slice(FIELD_KEYS);
    check_keys(parameters, &keys)?;
    let (scene, request, outcome) = outcome(parameters)?;
    let target = &scene.detectors[request.target];
    let traced = scene
        .trace()
        .map_err(|e| CommandError::new(ErrorCode::Failed, e.to_string()))?;
    let f = &outcome.field;
    let radius = f.second_moment_radius();
    Ok(json!({
        "target": target.id,
        "start_power_w": outcome.start_power,
        "power_w": f.power(),
        "radius_mm": [radius.0, radius.1],
        "tilt": [f.tilt().0, f.tilt().1],
        "grid": { "samples": f.size(), "spacing_mm": f.spacing(), "curvature_per_mm": f.curvature() },
        "steps": outcome.steps.iter().map(|s| {
            let mut v = json!({ "at_mm": s.at, "what": s.what, "power_w": s.power });
            if let Some(r) = s.report {
                v["method"] = json!(match r.method {
                    optobench_field::Method::AngularSpectrum => "angular spectrum",
                    optobench_field::Method::Fresnel => "Fresnel",
                });
                v["fresnel_number"] = json!(r.fresnel_number);
                v["edge_fraction"] = json!(r.edge_fraction);
            }
            v
        }).collect::<Vec<_>>(),
        "coupling": outcome.coupling,
        "launched": { "rays": outcome.launched.0, "rough": outcome.launched.1 },
        "delivered": outcome.delivered.iter().map(|d| json!({
            "id": d.id,
            "power_w": d.power,
            "rays": d.rays,
            "centroid_mm": [d.centroid.0, d.centroid.1],
            "rms_radius_mm": d.rms_radius,
        })).collect::<Vec<_>>(),
        "traced": traced.detectors.iter().map(|d| json!({
            "id": d.id,
            "power_w": d.direct_power,
            "centroid_mm": [d.direct_centroid.0, d.direct_centroid.1],
            "rms_radius_mm": d.direct_rms_radius,
        })).collect::<Vec<_>>(),
    }))
}
