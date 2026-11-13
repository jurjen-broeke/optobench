//! The `excitation` command: a fluorophore excited at a laser's focus
//! (`optobench_analysis::excitation`), and the pulse's width along its path.

use optobench_analysis::excitation::{Excitation, ExcitationSettings, Timing, excitation};
use serde_json::{Value, json};

use crate::psf::{moved_scene, request};
use crate::trace::text;
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidParameters, message)
}

/// The parameters `excitation` takes besides `path`, `dofs` and `data_directory`.
pub(crate) const EXCITATION_KEYS: &[&str] = &[
    "source",
    "detector",
    "fluorophore",
    "polarization",
    "samples",
    "planes",
    "across",
];

/// The excitation the parameters ask for, with `section` for an x–z map.
pub(crate) fn compute(
    parameters: &Parameters,
    section: Option<(usize, usize)>,
) -> Result<Excitation, CommandError> {
    let (scene, path) = moved_scene(parameters)?;
    let (source, detector, psf) = request(parameters, &scene, &path)?;
    let fluorophore = match text(parameters, "fluorophore")? {
        Some(id) => scene
            .fluorophores
            .iter()
            .position(|f| f.id == id)
            .ok_or_else(|| invalid(format!("no fluorophore `{id}`")))?,
        None if scene.fluorophores.is_empty() => {
            return Err(invalid(format!(
                "{path}: the design has no fluorophore ([[fluorophore]])"
            )));
        }
        None => 0,
    };
    let count = |key: &str, default: usize| -> Result<usize, CommandError> {
        match parameters.get(key) {
            None => Ok(default),
            Some(v) => v
                .as_u64()
                .filter(|n| (4..=400).contains(n))
                .map(|n| n as usize)
                .ok_or_else(|| invalid(format!("`{key}` must be a whole number from 4 to 400"))),
        }
    };
    let settings = ExcitationSettings {
        samples: if parameters.contains_key("samples") {
            psf.samples
        } else {
            ExcitationSettings::default().samples
        },
        polarization: psf.polarization,
        volume: (count("planes", 32)?, count("across", 32)?),
        section,
        cloud: None,
    };
    excitation(&scene, source, detector, fluorophore, &settings)
        .map_err(|e| CommandError::new(ErrorCode::Failed, format!("{path}: {e}")))
}

/// `excitation`: parameters `path`, and optionally `source` (the laser; default the first
/// collimated or Gaussian source), `detector` (at the focus; default the first),
/// `fluorophore` (id; default the first), `polarization`, `samples` (rays across the beam,
/// default 96), `planes` and `across` (the volume integral's planes and points across
/// each, default 32), `dofs` and `data_directory`. Returns the irradiance at the focus,
/// one- and two-photon rates per molecule, the two-photon excitations in the focus and
/// the photons emitted, the saturation checks, the volume's sizes and the pulse along the
/// path.
pub(crate) fn command(parameters: &Parameters) -> Result<Value, CommandError> {
    let mut keys = vec!["path", "dofs", "data_directory"];
    keys.extend_from_slice(EXCITATION_KEYS);
    check_keys(parameters, &keys)?;
    let e = compute(parameters, None)?;
    let psf = &e.psf;
    let widths = |w: (Option<f64>, Option<f64>, Option<f64>)| json!({ "x_mm": w.0, "y_mm": w.1, "axial_mm": w.2 });
    let timing = match e.timing {
        Timing::Continuous => json!({ "kind": "continuous" }),
        Timing::Pulsed {
            repetition_rate,
            width,
            shape,
        } => json!({
            "kind": "pulsed",
            "repetition_rate_hz": repetition_rate,
            "width_at_focus_fs": width * 1e15,
            "shape": match shape {
                optobench_scene::PulseShape::Gaussian => "gaussian",
                optobench_scene::PulseShape::SechSquared => "sech2",
            },
            "squared_mean_ratio": e.timing.squared_mean_ratio(),
        }),
    };
    let pulse = e.pulse.as_ref().map(|p| {
        json!({
            "width_fs": p.width * 1e15,
            "prechirp_fs2": p.prechirp * 1e30,
            "start_fs": p.start * 1e15,
            "end": p.end,
            "steps": p.steps.iter().map(|s| json!({
                "from": s.from, "to": s.to, "length_mm": s.length,
                "dispersion_fs2": s.dispersion * 1e30, "total_fs2": s.total * 1e30,
                "width_fs": s.width * 1e15,
            })).collect::<Vec<_>>(),
        })
    });
    let volume = psf.volume.as_ref().map(|v| {
        json!({
            "squared_w2_per_mm": v.squared,
            "tails_w2_per_mm": v.tails,
            "reach_mm": v.reach,
        })
    });
    Ok(json!({
        "source": e.source,
        "detector": e.detector,
        "fluorophore": e.fluorophore,
        "wavelength_um": e.wavelength,
        "sample": e.place.as_ref().map(|p| p.0.clone()),
        "part": e.place.as_ref().map(|p| p.1.clone()),
        "concentration_um": e.molar.map(|m| m * 1e6),
        "timing": timing,
        "pupil_power_w": e.pupil_power,
        "transmission": e.transmission,
        "focal_power_w": e.pupil_power * e.transmission,
        "peak_w_per_mm2": e.peak_irradiance,
        "peak_mm": [psf.peak.x, psf.peak.y, psf.peak.z],
        "na": psf.na,
        "strehl": psf.strehl,
        "fwhm": widths(psf.fwhm),
        "fwhm_squared": widths(psf.fwhm_squared),
        "volume": volume,
        "one_photon": e.one_photon.as_ref().map(|o| json!({
            "cross_section_cm2": o.cross_section * 1e-2,
            "peak_rate_per_s": o.peak_rate,
            "saturation": o.saturation,
            "per_depth_per_s_per_mm": o.per_depth,
        })),
        "two_photon": e.two_photon.as_ref().map(|t| json!({
            "cross_section_gm": t.cross_section * 1e48,
            "peak_rate_per_s": t.peak_rate,
            "per_pulse": t.per_pulse,
            "total_per_s": t.total,
        })),
        "emitted_photons_per_s": e.emitted,
        "pulse": pulse,
        "notes": e.notes,
    }))
}
