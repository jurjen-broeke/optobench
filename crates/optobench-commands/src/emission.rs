//! The `emission` command: the photon budget of an emitter in a sample
//! (`optobench_analysis::emission`).

use optobench_analysis::emission::{Amount, budget, budget_through};
use optobench_analysis::excitation::{ExcitationSettings, excitation};
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
///
/// With `excited_by` (a laser source's id) and `focus` (the detector at its focus), the
/// emitter is spread through the volume the laser excites two-photon in `fluorophore`
/// (id; default the first): placed at the cells of a grid around the focus (`cloud`:
/// [points across, points along, half-extent in widths of I²], default [3, 5, 1.0]),
/// each with its share of I², the photons emitted per second those of the excitation
/// (`samples`: rays across the laser's beam for its PSF, default 96). The detector at the
/// focus is left out of the emission's trace (it is a reference plane among the emitters).
pub(crate) fn emission(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(
        parameters,
        &[
            "path",
            "emitter",
            "excited_by",
            "focus",
            "fluorophore",
            "cloud",
            "samples",
            "dofs",
            "data_directory",
        ],
    )?;
    let (scene, path) = moved_scene(parameters)?;
    let invalid = |m: &str| CommandError::new(ErrorCode::InvalidParameters, m.to_owned());
    let emitter = match text(parameters, "emitter")? {
        Some(id) => scene
            .sources
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| invalid(&format!("no source `{id}`")))?,
        None => scene
            .sources
            .iter()
            .position(|s| matches!(s.kind, optobench_scene::SourceKind::Isotropic { .. }))
            .ok_or_else(|| {
                invalid(&format!(
                    "{path}: the design has no emitter (a source of kind \"isotropic\")"
                ))
            })?,
    };
    let fail = |e: String| CommandError::new(ErrorCode::Failed, format!("{path}: {e}"));
    let mut excited = Value::Null;
    let b = match text(parameters, "excited_by")? {
        None => {
            for key in ["focus", "fluorophore", "cloud", "samples"] {
                if parameters.contains_key(key) {
                    return Err(invalid(&format!("`{key}` belongs with `excited_by`")));
                }
            }
            budget(&scene, emitter).map_err(fail)?
        }
        Some(laser) => {
            let source = scene
                .sources
                .iter()
                .position(|s| s.id == laser)
                .ok_or_else(|| invalid(&format!("no source `{laser}`")))?;
            let focus = text(parameters, "focus")?.ok_or_else(|| {
                invalid("`excited_by` needs `focus`, the detector at the laser's focus")
            })?;
            let detector = scene
                .detectors
                .iter()
                .position(|d| d.id == focus)
                .ok_or_else(|| invalid(&format!("no detector `{focus}`")))?;
            let fluorophore = match text(parameters, "fluorophore")? {
                Some(id) => scene
                    .fluorophores
                    .iter()
                    .position(|f| f.id == id)
                    .ok_or_else(|| invalid(&format!("no fluorophore `{id}`")))?,
                None if scene.fluorophores.is_empty() => {
                    return Err(invalid(&format!("{path}: the design has no fluorophore")));
                }
                None => 0,
            };
            let cloud = match parameters.get("cloud") {
                None => (3, 5, 1.0),
                Some(v) => v
                    .as_array()
                    .filter(|a| a.len() == 3)
                    .and_then(|a| Some((a[0].as_u64()? as usize, a[1].as_u64()? as usize, a[2].as_f64()?)))
                    .filter(|(a, b, e)| (1..=15).contains(a) && (1..=31).contains(b) && *e > 0.0 && *e <= 5.0)
                    .ok_or_else(|| invalid("`cloud` is [points across 1–15, points along 1–31, half-extent in widths of I², up to 5]"))?,
            };
            let samples = match parameters.get("samples") {
                None => ExcitationSettings::default().samples,
                Some(v) => v
                    .as_u64()
                    .filter(|n| *n >= 8)
                    .ok_or_else(|| invalid("`samples` must be a whole number ≥ 8"))?
                    as usize,
            };
            let e = excitation(
                &scene,
                source,
                detector,
                fluorophore,
                &ExcitationSettings {
                    samples,
                    cloud: Some(cloud),
                    volume: (24, 24),
                    ..Default::default()
                },
            )
            .map_err(fail)?;
            let photons = e.emitted.ok_or_else(|| {
                fail(format!(
                    "no photons emitted from the focus: `{}` needs a two-photon cross-section at {} µm \
                     and a concentration where the focus is (one-photon excitation is not confined \
                     to the focus, so it is not spread here)",
                    e.fluorophore, e.wavelength
                ))
            })?;
            excited = json!({
                "laser": e.source,
                "focus": e.detector,
                "fluorophore": e.fluorophore,
                "emitted_photons_per_s": photons,
                "emitters": e.emitters.len(),
                "represented": e.represented,
            });
            // The focus's detector is a reference plane for the excitation, among the
            // emitters: it is left out of the emission's trace.
            let mut without = scene.clone();
            without.detectors.remove(detector);
            budget_through(&without, emitter, &e.emitters, photons).map_err(fail)?
        }
    };
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
        "excitation": excited,
    }))
}
