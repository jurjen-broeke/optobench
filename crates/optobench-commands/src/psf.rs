//! The `psf` command: the vectorial PSF of a coherent source focused by an objective,
//! through whatever layers are in front of it (`optobench_analysis::psf`).

use optobench_analysis::{Polarization, PsfSettings};
use serde_json::{Value, json};

use crate::trace::{dof_values, load, text};
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidParameters, message)
}

/// The parameters the PSF takes, besides `path`, `dofs` and `data_directory`.
pub(crate) const PSF_KEYS: &[&str] = &[
    "source",
    "detector",
    "polarization",
    "samples",
    "axial",
    "lateral",
    "points",
];

/// The design at `path` with its DOFs as the parameters set them.
pub(crate) fn moved_scene(
    parameters: &Parameters,
) -> Result<(optobench_scene::Scene, String), CommandError> {
    let path = text(parameters, "path")?.ok_or_else(|| invalid("missing parameter `path`"))?;
    let (scene, _, _, _) = load(path, parameters)?;
    let overrides = dof_values(parameters, &scene)?;
    let scene = scene
        .moved(&overrides)
        .map_err(|e| CommandError::new(ErrorCode::Failed, format!("{path}: {e}")))?;
    Ok((scene, path.to_owned()))
}

/// The detector the parameters name (`detector`), or the first.
pub(crate) fn detector_of(
    parameters: &Parameters,
    scene: &optobench_scene::Scene,
    path: &str,
) -> Result<usize, CommandError> {
    match text(parameters, "detector")? {
        Some(id) => scene
            .detectors
            .iter()
            .position(|d| d.id == id)
            .ok_or_else(|| invalid(format!("no detector `{id}`"))),
        None if scene.detectors.is_empty() => {
            Err(invalid(format!("{path}: the design has no detector")))
        }
        None => Ok(0),
    }
}

/// The source, detector and settings of a PSF, from the parameters in [`PSF_KEYS`].
pub(crate) fn request(
    parameters: &Parameters,
    scene: &optobench_scene::Scene,
    path: &str,
) -> Result<(usize, usize, PsfSettings), CommandError> {
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
            .ok_or_else(|| {
                invalid(format!(
                    "{path}: the design has no collimated or Gaussian source"
                ))
            })?,
    };
    let detector = detector_of(parameters, scene, path)?;
    let polarization = match text(parameters, "polarization")? {
        None | Some("x") => Polarization::X,
        Some("y") => Polarization::Y,
        Some("circular") => Polarization::Circular,
        Some("radial") => Polarization::Radial,
        Some("azimuthal") => Polarization::Azimuthal,
        Some(other) => {
            return Err(invalid(format!(
                "polarization `{other}` is not known; use \"x\", \"y\", \"circular\", \"radial\" or \"azimuthal\""
            )));
        }
    };
    let count = |key: &str, least: u64| -> Result<Option<usize>, CommandError> {
        match parameters.get(key) {
            None => Ok(None),
            Some(v) => v
                .as_u64()
                .filter(|n| *n >= least)
                .map(|n| Some(n as usize))
                .ok_or_else(|| invalid(format!("`{key}` must be a whole number ≥ {least}"))),
        }
    };
    let axial = match parameters.get("axial") {
        None => None,
        Some(v) => {
            let pair = v
                .as_array()
                .filter(|a| a.len() == 2)
                .and_then(|a| Some((a[0].as_f64()?, a[1].as_f64()?)))
                .filter(|(a, b)| a < b)
                .ok_or_else(|| invalid("`axial` is [from, to] in mm, from < to"))?;
            Some(pair)
        }
    };
    let lateral = match parameters.get("lateral") {
        None => None,
        Some(v) => Some(
            v.as_f64()
                .filter(|x| *x > 0.0)
                .ok_or_else(|| invalid("`lateral` is a half-width in mm, > 0"))?,
        ),
    };
    let settings = PsfSettings {
        samples: count("samples", 8)?.unwrap_or(128),
        polarization,
        axial,
        lateral,
        points: count("points", 3)?,
        section: None,
        attenuated: false,
        volume: None,
        cloud: None,
    };
    Ok((source, detector, settings))
}

/// `psf`: parameters `path`, and optionally `source` (id; default the first collimated or
/// Gaussian source), `detector` (id; default the first), `polarization` (`x`, `y`,
/// `circular`, `radial`, `azimuthal`; default `x`, in the objective's frame), `samples`
/// (rays across the beam, default 128), `axial` ([from, to] in mm from the detector
/// plane), `lateral` (half-width of the lateral profiles, mm), `points` (in the axial
/// scan), `dofs` and `data_directory`.
pub(crate) fn psf(parameters: &Parameters) -> Result<Value, CommandError> {
    let mut keys = vec!["path", "dofs", "data_directory"];
    keys.extend_from_slice(PSF_KEYS);
    check_keys(parameters, &keys)?;
    let (scene, path) = moved_scene(parameters)?;
    let (source, detector, settings) = request(parameters, &scene, &path)?;
    let result = optobench_analysis::psf(&scene, source, detector, &settings)
        .map_err(|e| CommandError::new(ErrorCode::Failed, format!("{path}: {e}")))?;
    let pairs =
        |list: &[(f64, f64)]| -> Vec<Value> { list.iter().map(|(a, b)| json!([a, b])).collect() };
    let (centre, direction) = result.line;
    Ok(json!({
        "source": scene.sources[source].id,
        "detector": scene.detectors[detector].id,
        "wavelength_um": result.wavelength,
        "index": result.index,
        "na": result.na,
        "power_w": result.power,
        "rays": result.rays,
        "peak_mm": [result.peak.x, result.peak.y, result.peak.z],
        "peak_w_per_mm2": result.peak_intensity,
        "strehl": result.strehl,
        "fwhm_x_mm": result.fwhm.0,
        "fwhm_y_mm": result.fwhm.1,
        "fwhm_axial_mm": result.fwhm.2,
        "line": {
            "point_mm": [centre.x, centre.y, centre.z],
            "direction": [direction.x, direction.y, direction.z],
        },
        "crossings_mm": [result.crossings.0, result.crossings.1],
        "axial": pairs(&result.axial),
        "lateral_x": pairs(&result.lateral.0),
        "lateral_y": pairs(&result.lateral.1),
    }))
}
