//! The `fluence` command: the light in a sample, direct and diffuse
//! (`optobench_analysis::sample`).

use optobench_analysis::sample::{LightSettings, SampleLight, sample_light};
use serde_json::{Value, json};

use crate::psf::moved_scene;
use crate::trace::text;
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidParameters, message)
}

/// The parameters of the light in a sample, besides `path`, `dofs` and `data_directory`.
pub(crate) const LIGHT_KEYS: &[&str] = &[
    "sample",
    "radius",
    "rings",
    "slices",
    "primary",
    "transport",
    "packets",
    "seed",
    "gpu",
    "threads",
];

/// The light in the sample the parameters name (`sample`; default the first).
pub(crate) fn light(parameters: &Parameters) -> Result<SampleLight, CommandError> {
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
    let mut settings = LightSettings::default();
    if let Some(v) = parameters.get("radius") {
        settings.radius = Some(
            v.as_f64()
                .filter(|r| *r > 0.0)
                .ok_or_else(|| invalid("`radius` is the grid's radius in mm, > 0"))?,
        );
    }
    for (key, slot) in [
        ("rings", &mut settings.rings),
        ("slices", &mut settings.slices),
    ] {
        if let Some(v) = parameters.get(key) {
            *slot = v
                .as_u64()
                .filter(|n| (4..=2000).contains(n))
                .ok_or_else(|| invalid(format!("`{key}` is a whole number from 4 to 2000")))?
                as usize;
        }
    }
    settings.primary = match text(parameters, "primary")? {
        None | Some("delta-eddington") => optobench_analysis::sample::Primary::DeltaEddington,
        Some("ballistic") => optobench_analysis::sample::Primary::Ballistic,
        Some(other) => {
            return Err(invalid(format!(
                "`primary` `{other}` is not known; use \"delta-eddington\" (default) or \"ballistic\""
            )));
        }
    };
    let choice = crate::transport::choice(parameters)?;
    if choice.monte_carlo {
        let built = scene.build().map_err(|e| failed(e.to_string()))?;
        let trace = scene.trace_built(&built);
        return crate::transport::monte_carlo_light(
            &scene, &built, &trace, sample, &settings, &choice, true,
        )
        .map_err(failed);
    }
    let trace = scene.trace().map_err(|e| failed(e.to_string()))?;
    sample_light(&scene, &trace, sample, &settings).map_err(failed)
}

/// `fluence`: parameters `path`, and optionally `sample` (an element id; default the
/// first sample), `radius` (the grid's radius, mm; default half the sample's diameter),
/// `rings` (default 100), `slices` (default 200), `primary` (`delta-eddington`, default,
/// or `ballistic`: see `optobench_analysis::sample`), `dofs` and `data_directory`. Returns the
/// power scattered and absorbed on the grid, the diffuse light's balance and the
/// absorption per layer and at its peak.
pub(crate) fn fluence(parameters: &Parameters) -> Result<Value, CommandError> {
    let mut keys = vec!["path", "dofs", "data_directory"];
    keys.extend_from_slice(LIGHT_KEYS);
    check_keys(parameters, &keys)?;
    let light = light(parameters)?;
    let g = &light.grid;
    let peak = (0..g.cells())
        .max_by(|&a, &b| light.absorbed[a].total_cmp(&light.absorbed[b]))
        .unwrap_or(0);
    let diffuse = light.diffuse_light.as_ref().map(|d| {
        json!({
            "absorbed_w": d.absorbed,
            "top_w": d.top,
            "bottom_w": d.bottom,
            "side_w": d.side,
            "iterations": d.iterations,
        })
    });
    Ok(json!({
        "sample": light.id,
        "wavelength_um": light.wavelength,
        "grid": {
            "radius_mm": g.radius, "rings": g.rings,
            "depth_mm": [g.z.0, g.z.1], "slices": g.slices,
        },
        "scattered_w": light.scattered,
        "direct_absorbed_w": light.direct_absorbed,
        "diffuse": diffuse,
        "monte_carlo": light.monte_carlo.as_ref().map(|m| json!({
            "packets": m.packets,
            "launched_w": m.launched,
            "reflected_w": m.reflected,
            "transmitted_w": m.transmitted,
            "absorbed_outside_w": m.absorbed_outside,
            "roulette_w": m.roulette,
            "gpu": m.gpu,
        })),
        "layers": light.parts.iter().map(|p| json!({
            "part": p.part, "direct_absorbed_w": p.direct, "diffuse_absorbed_w": p.diffuse,
        })).collect::<Vec<_>>(),
        "peak_absorbed_w_per_mm3": light.absorbed[peak],
        "peak_at_mm": { "r": g.r(peak % g.rings), "depth": g.z_at(peak / g.rings) },
    }))
}
