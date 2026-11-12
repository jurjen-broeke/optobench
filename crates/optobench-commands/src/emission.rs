//! The `emission` command: the photon budget of an emitter in a sample
//! (`optobench_analysis::emission`).

use optobench_analysis::emission::{Amount, budget};
use serde_json::{Value, json};

use crate::psf::moved_scene;
use crate::trace::text;
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn amount(a: Amount) -> Value {
    json!({ "w": a.watts, "photons_per_s": a.photons })
}

/// `emission`: parameters `path`, and optionally `emitter` (a source id; default the
/// first isotropic source), `dofs` and `data_directory`. Traces the emitter on its own
/// and returns its photon budget: emitted, outside the traced cone, collected by the
/// objective, lost in each part of the sample and at each item, arriving at and counted
/// by each detector, where the rest ended, and the imbalance.
pub(crate) fn emission(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(parameters, &["path", "emitter", "dofs", "data_directory"])?;
    let (scene, path) = moved_scene(parameters)?;
    let invalid = |m: String| CommandError::new(ErrorCode::InvalidParameters, m);
    let emitter = match text(parameters, "emitter")? {
        Some(id) => scene
            .sources
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| invalid(format!("no source `{id}`")))?,
        None => scene
            .sources
            .iter()
            .position(|s| matches!(s.kind, optobench_scene::SourceKind::Isotropic { .. }))
            .ok_or_else(|| {
                invalid(format!(
                    "{path}: the design has no emitter (a source of kind \"isotropic\")"
                ))
            })?,
    };
    let b = budget(&scene, emitter)
        .map_err(|e| CommandError::new(ErrorCode::Failed, format!("{path}: {e}")))?;
    let e = &b.ends;
    Ok(json!({
        "emitter": b.emitter,
        "wavelengths_um": b.wavelengths,
        "emitted": amount(b.emitted),
        "outside_cone": amount(b.outside_cone),
        "traced": amount(b.traced),
        "collected": amount(b.collected),
        "sample": b.sample.iter().map(|p| json!({
            "sample": p.sample, "part": p.part,
            "absorbed": amount(p.absorbed), "scattered": amount(p.scattered),
        })).collect::<Vec<_>>(),
        "items": b.items.iter().map(|i| json!({
            "id": i.id, "at_faces": amount(i.at_faces), "clipped": amount(i.clipped),
        })).collect::<Vec<_>>(),
        "detectors": b.detectors.iter().map(|d| json!({
            "id": d.id,
            "arriving": amount(d.arriving),
            "counted": d.counted.map(amount),
            "quantum_efficiency_source": d.source,
        })).collect::<Vec<_>>(),
        "ends": {
            "detected": amount(e.detected),
            "clipped": amount(e.clipped),
            "absorbed_in_samples": amount(e.absorbed_in_samples),
            "absorbed_in_other_media": amount(e.absorbed_in_other_media),
            "absorbed_in_coatings": amount(e.absorbed_in_coatings),
            "scattered": amount(e.scattered),
            "reflected": amount(e.reflected),
            "escaped": amount(e.escaped),
            "stopped": amount(e.stopped),
        },
        "imbalance_w": b.imbalance,
    }))
}
