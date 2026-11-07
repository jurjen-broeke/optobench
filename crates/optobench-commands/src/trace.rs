//! The `trace` command: trace a layout file and report detectors, clipping and the power
//! ledger, optionally writing the 2D layout view.

use std::path::{Path, PathBuf};

use optobench_io::scene_file;
use optobench_io::sidecar;
use optobench_io::store::{default_data_directory, material};
use optobench_io::svg::{self, View};
use optobench_scene::Owner;
use serde_json::{Value, json};

use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn text<'a>(parameters: &'a Parameters, key: &str) -> Result<Option<&'a str>, CommandError> {
    match parameters.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(CommandError::new(
            ErrorCode::InvalidParameters,
            format!("`{key}` must be a string"),
        )),
    }
}

fn failed(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::Failed, message)
}

/// `trace`: parameters `path` (a `.optobench` layout, or a sidecar next to its STEP file),
/// and optionally `data_directory`
/// (for imported materials), `svg` (where to write the layout view), `view` (`top` or
/// `side`) and `rays_drawn` (default 200).
pub(crate) fn trace(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(
        parameters,
        &["path", "data_directory", "svg", "view", "rays_drawn"],
    )?;
    let path = text(parameters, "path")?.ok_or_else(|| {
        CommandError::new(ErrorCode::InvalidParameters, "missing parameter `path`")
    })?;
    let view = match text(parameters, "view")? {
        None | Some("top") => View::Top,
        Some("side") => View::Side,
        Some(other) => {
            return Err(CommandError::new(
                ErrorCode::InvalidParameters,
                format!("view `{other}` is not known; use \"top\" or \"side\""),
            ));
        }
    };
    let rays_drawn = match parameters.get("rays_drawn") {
        None => 200,
        Some(v) => v.as_u64().filter(|n| *n > 0).ok_or_else(|| {
            CommandError::new(
                ErrorCode::InvalidParameters,
                "`rays_drawn` must be a whole number ≥ 1",
            )
        })? as usize,
    };
    let data: Option<PathBuf> = text(parameters, "data_directory")?
        .map(PathBuf::from)
        .or_else(default_data_directory);

    let source = std::fs::read_to_string(path).map_err(|e| failed(format!("{path}: {e}")))?;
    let lookup = |name: &str| material(data.as_deref(), name);
    let library = match &data {
        Some(root) => optobench_io::library::load(root).map_err(failed)?,
        None => optobench_library::Library::default(),
    };
    let curves = |name: &str| match &data {
        Some(root) => optobench_io::curves::load(root, name),
        None => optobench_io::coatings::no_curves(name),
    };
    let lookups = scene_file::Lookups {
        materials: &lookup,
        curves: &curves,
        library: &library,
    };
    let (scene, parts) = match sidecar::kind_of(&source).as_deref() {
        Some("sidecar") => {
            sidecar::read(&source, Path::new(path), &lookups).map(|read| (read.scene, read.parts))
        }
        _ => scene_file::parse_with(&source, &lookups).map(|scene| (scene, Vec::new())),
    }
    .map_err(|e| failed(format!("{path}: {e}")))?;
    let built = scene.build().map_err(|e| failed(format!("{path}: {e}")))?;
    let traced = scene.trace().map_err(|e| failed(format!("{path}: {e}")))?;

    let ledger = traced.ledger;
    let launched = ledger.launched.max(f64::MIN_POSITIVE);
    let detectors: Vec<Value> = traced
        .detectors
        .iter()
        .map(|d| {
            json!({
                "id": d.id,
                "power_w": d.power,
                "direct_power_w": d.direct_power,
                "direct_centroid_mm": [d.direct_centroid.0, d.direct_centroid.1],
                "direct_rms_radius_mm": d.direct_rms_radius,
                "rays": d.rays,
                "centroid_mm": [d.centroid.0, d.centroid.1],
                "rms_radius_mm": d.rms_radius,
                "peak_irradiance_w_per_m2": d.irradiance.iter().copied().fold(0.0, f64::max),
                // Power by angle from the detector normal, in bins of angular_bin_deg.
                "angular_bin_deg": optobench_scene::ANGULAR_BIN_DEGREES,
                "angular_power_w": d.angular,
            })
        })
        .collect();
    let clipping: Vec<Value> = traced
        .clipping
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "kind": match c.owner {
                    Owner::Element(_) => "element",
                    Owner::Proxy(_) => "proxy",
                    Owner::Detector(_) => "detector",
                    Owner::Solid(_) => "body",
                },
                "power_w": c.power,
                "rays": c.rays,
                "fraction": c.power / launched,
            })
        })
        .collect();

    let mut result = json!({
        "scene": path,
        "rays": traced.records.len(),
        "ledger": {
            "launched_w": ledger.launched,
            "detected_w": ledger.detected,
            "clipped_w": ledger.clipped,
            "absorbed_w": ledger.absorbed,
            "reflected_w": ledger.reflected,
            "escaped_w": ledger.escaped,
            "stopped_w": ledger.stopped,
            "below_cutoff_w": ledger.below_cutoff,
            "imbalance_w": ledger.imbalance(),
        },
        "detectors": detectors,
        "clipping": clipping,
        "parts": crate::parts::matches_json(&parts),
        "embedded": crate::embed::compare(&source, data.as_deref())?,
        "footprints": traced.footprints.iter().map(|f| json!({
            "id": f.id,
            "hits": f.hits,
            "power_w": f.power,
            "centroid_mm": [f.centroid.x, f.centroid.y, f.centroid.z],
            "radius_mm": f.radius,
            "clear_aperture_used": f.clear_aperture_used,
        })).collect::<Vec<_>>(),
    });
    if let Some(output) = text(parameters, "svg")? {
        let drawing = svg::layout(&scene, &built, &traced, view, rays_drawn);
        std::fs::write(Path::new(output), drawing).map_err(|e| failed(format!("{output}: {e}")))?;
        result["svg"] = json!(output);
    }
    Ok(result)
}

/// `bodies`: every placed body of a STEP file, for writing a sidecar: assembly path,
/// product, body name, number of faces and geometry fingerprint.
pub(crate) fn bodies(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(parameters, &["path"])?;
    let path = text(parameters, "path")?.ok_or_else(|| {
        CommandError::new(ErrorCode::InvalidParameters, "missing parameter `path`")
    })?;
    let source = std::fs::read_to_string(path).map_err(|e| failed(format!("{path}: {e}")))?;
    let model = optobench_io::step::read(&source).map_err(|e| failed(format!("{path}: {e}")))?;
    let (bodies, skipped) = sidecar::placed_bodies(&model);
    let list: Vec<Value> = bodies
        .iter()
        .map(|b| {
            json!({
                "occurrence": b.path,
                "product": b.product,
                "body": b.name,
                "faces": b.faces.len(),
                "fingerprint": b.fingerprint,
            })
        })
        .collect();
    Ok(
        json!({ "path": path, "bodies": list, "skipped_faces": skipped, "unsupported": model.unsupported }),
    )
}

/// `export-placements`: for a standalone layout (`path`), write `<name>.placements.json`
/// (every pose, to rebuild it in CAD) and `<name>.sidecar.optobench` (a matching sidecar
/// for the STEP export `<name>.step`), next to the layout or in `out_directory`.
pub(crate) fn export_placements(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(parameters, &["path", "out_directory", "data_directory"])?;
    let path = text(parameters, "path")?.ok_or_else(|| {
        CommandError::new(ErrorCode::InvalidParameters, "missing parameter `path`")
    })?;
    let source = std::fs::read_to_string(path).map_err(|e| failed(format!("{path}: {e}")))?;
    if sidecar::kind_of(&source).as_deref() != Some("layout") {
        return Err(CommandError::new(
            ErrorCode::InvalidParameters,
            format!("{path}: export-placements takes a layout (kind = \"layout\")"),
        ));
    }
    let data: Option<PathBuf> = text(parameters, "data_directory")?
        .map(PathBuf::from)
        .or_else(default_data_directory);
    let lookup = |name: &str| material(data.as_deref(), name);
    let library = match &data {
        Some(root) => optobench_io::library::load(root).map_err(failed)?,
        None => optobench_library::Library::default(),
    };
    let curves = |name: &str| match &data {
        Some(root) => optobench_io::curves::load(root, name),
        None => optobench_io::coatings::no_curves(name),
    };
    let lookups = scene_file::Lookups {
        materials: &lookup,
        curves: &curves,
        library: &library,
    };
    let scene =
        scene_file::parse_with(&source, &lookups).map_err(|e| failed(format!("{path}: {e}")))?;
    let layout: toml::Table = source.parse().map_err(|e| failed(format!("{path}: {e}")))?;
    let input = Path::new(path);
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "layout".into());
    let directory = match text(parameters, "out_directory")? {
        Some(d) => PathBuf::from(d),
        None => input.parent().map(Path::to_path_buf).unwrap_or_default(),
    };
    let placements_path = directory.join(format!("{stem}.placements.json"));
    let sidecar_path = directory.join(format!("{stem}.sidecar.optobench"));
    let document = optobench_io::placements::placements(&scene);
    let json = serde_json::to_string_pretty(&document).map_err(|e| failed(e.to_string()))?;
    std::fs::write(&placements_path, json + "\n")
        .map_err(|e| failed(format!("{}: {e}", placements_path.display())))?;
    let skeleton = optobench_io::placements::sidecar(&scene, &layout, &format!("{stem}.step"));
    std::fs::write(&sidecar_path, skeleton)
        .map_err(|e| failed(format!("{}: {e}", sidecar_path.display())))?;
    Ok(json!({
        "placements": placements_path.to_string_lossy(),
        "sidecar": sidecar_path.to_string_lossy(),
        "elements": scene.elements.len(),
    }))
}
