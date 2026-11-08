//! The `trace` command: trace a layout file and report detectors, clipping and the power
//! ledger, optionally writing the 2D layout view.

use std::path::{Path, PathBuf};

use optobench_io::scene_file;
use optobench_io::sidecar;
use optobench_io::store::{default_data_directory, material};
use optobench_io::svg::{self, View};
use optobench_scene::Owner;
use optobench_surfaces::brep::Fineness;
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

/// The `dofs` parameter: `{ "<name>": value }` in mm (slides) or degrees (tilts).
pub(crate) fn dof_values(
    parameters: &Parameters,
    scene: &optobench_scene::Scene,
) -> Result<std::collections::BTreeMap<String, f64>, CommandError> {
    let invalid = |m: String| CommandError::new(ErrorCode::InvalidParameters, m);
    let mut out = std::collections::BTreeMap::new();
    let Some(value) = parameters.get("dofs") else {
        return Ok(out);
    };
    let map = value
        .as_object()
        .ok_or_else(|| invalid("`dofs` is an object of name: value".into()))?;
    for (name, v) in map {
        let v = v
            .as_f64()
            .ok_or_else(|| invalid(format!("`dofs.{name}` must be a number")))?;
        let dof = scene
            .dofs
            .iter()
            .find(|d| &d.name == name)
            .ok_or_else(|| invalid(format!("no degree of freedom `{name}`")))?;
        if dof.locked {
            return Err(invalid(format!("`{name}` is locked")));
        }
        out.insert(
            name.clone(),
            if dof.motion.is_angle() {
                v.to_radians()
            } else {
                v
            },
        );
    }
    Ok(out)
}

/// The scene's DOFs and their values (mm or degrees).
pub(crate) fn dofs_json(scene: &optobench_scene::Scene) -> Value {
    json!(
        scene
            .dofs
            .iter()
            .map(|d| {
                let angle = d.motion.is_angle();
                let unit = |x: f64| if angle { x.to_degrees() } else { x };
                let mut v = json!({
                    "name": d.name,
                    "kind": if angle { "tilt" } else { "slide" },
                    "unit": if angle { "deg" } else { "mm" },
                    "value": unit(d.value),
                    "locked": d.locked,
                });
                if let Some((low, high)) = d.range {
                    v["range"] = json!([unit(low), unit(high)]);
                }
                if let Some((leader, ratio)) = &d.follows {
                    v["follows"] = json!(leader);
                    v["ratio"] = json!(ratio);
                }
                v
            })
            .collect::<Vec<_>>()
    )
}

/// A design file read with the user's data: its scene, the library parts it matched,
/// its text and the data directory used (the `data_directory` parameter or the
/// platform's).
pub(crate) fn load(
    path: &str,
    parameters: &Parameters,
) -> Result<
    (
        optobench_scene::Scene,
        Vec<optobench_io::sidecar::PartMatch>,
        String,
        Option<PathBuf>,
    ),
    CommandError,
> {
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
    Ok((scene, parts, source, data))
}

/// `trace`: parameters `path` (a `.optobench` layout, or a sidecar next to its STEP file),
/// and optionally `data_directory`
/// (for imported materials), `svg` (where to write the layout view), `view` (`top` or
/// `side`), `rays_drawn` (default 200) and `dofs` (`{ "<name>": value }`, mm or degrees,
/// in place of the file's values).
pub(crate) fn trace(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(
        parameters,
        &[
            "path",
            "data_directory",
            "svg",
            "view",
            "rays_drawn",
            "ray_paths",
            "dofs",
        ],
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
    let (scene, parts, source, data) = load(path, parameters)?;
    let overrides = dof_values(parameters, &scene)?;
    let scene = scene
        .moved(&overrides)
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
                // Where the direct beam points, and how far it is from collimated (mrad).
                "direct_mean_slope_mrad": [d.direct_mean_slope.0 * 1e3, d.direct_mean_slope.1 * 1e3],
                "direct_rms_slope_mrad": d.direct_rms_slope * 1e3,
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
        "dofs": dofs_json(&scene),
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
    if let Some(n) = parameters.get("ray_paths") {
        let n = n.as_u64().ok_or_else(|| {
            CommandError::new(
                ErrorCode::InvalidParameters,
                "`ray_paths` must be a whole number",
            )
        })? as usize;
        result["ray_paths"] = ray_paths(&traced, n);
    }
    if let Some(output) = text(parameters, "svg")? {
        let drawing = svg::layout(&scene, &built, &traced, view, rays_drawn);
        std::fs::write(Path::new(output), drawing).map_err(|e| failed(format!("{output}: {e}")))?;
        result["svg"] = json!(output);
    }
    Ok(result)
}

fn round(x: f64) -> f64 {
    (x * 1e4).round() / 1e4
}

fn point(p: optobench_math::Vec3) -> Value {
    json!([round(p.x), round(p.y), round(p.z)])
}

/// The first `n` rays' paths, main paths first: points (mm), how each ended, its power
/// and, for a ray that left the scene, its last direction.
fn ray_paths(traced: &optobench_scene::SceneTrace, n: usize) -> Value {
    use optobench_trace::nonsequential::End;
    let mut order: Vec<&optobench_trace::nonsequential::RayRecord> =
        traced.records.iter().collect();
    order.sort_by_key(|r| r.generation);
    json!(
        order
            .into_iter()
            .take(n)
            .map(|r| {
                let end = match r.end {
                    End::Detected { .. } => "detected",
                    End::Clipped { .. } => "clipped",
                    End::Escaped => "escaped",
                    End::Stopped => "stopped",
                    End::MaterialUndefined { .. } => "material_undefined",
                };
                json!({
                    "points": r.points.iter().map(|p| point(*p)).collect::<Vec<_>>(),
                    "end": end,
                    "power_w": r.launched.power,
                    "wavelength_um": r.ray.wavelength,
                    "generation": r.generation,
                    "direction": point(r.ray.direction),
                })
            })
            .collect::<Vec<_>>()
    )
}

/// `view`: the scene as a 3D wireframe for views with their own camera (the user
/// interface): per item its kind, id, role and polylines (mm, scene frame), the sources,
/// and the scene's bounds. Parameters `path`; optionally `data_directory`, `dofs`,
/// `max_points` (per boundary loop, default 32) and `mesh` (true, or the chord tolerance
/// in mm, default 0.05: add each item's triangles as flat arrays `positions` and
/// `normals`, three numbers per vertex, and `triangles`, three vertex indices each).
pub(crate) fn view(parameters: &Parameters) -> Result<Value, CommandError> {
    use optobench_scene::{ElementKind, SolidRole};
    check_keys(
        parameters,
        &["path", "data_directory", "dofs", "max_points", "mesh"],
    )?;
    let path = text(parameters, "path")?.ok_or_else(|| {
        CommandError::new(ErrorCode::InvalidParameters, "missing parameter `path`")
    })?;
    let max_points = match parameters.get("max_points") {
        None => 32,
        Some(v) => v.as_u64().filter(|n| *n >= 2).ok_or_else(|| {
            CommandError::new(
                ErrorCode::InvalidParameters,
                "`max_points` must be a whole number ≥ 2",
            )
        })? as usize,
    };
    let fineness = match parameters.get("mesh") {
        None | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(Fineness::default()),
        Some(v) => Some(Fineness {
            chord: v.as_f64().filter(|c| *c > 0.0).ok_or_else(|| {
                CommandError::new(
                    ErrorCode::InvalidParameters,
                    "`mesh` must be true, false or a chord tolerance in mm > 0",
                )
            })?,
            ..Fineness::default()
        }),
    };
    let (scene, _, _, _) = load(path, parameters)?;
    let overrides = dof_values(parameters, &scene)?;
    let scene = scene
        .moved(&overrides)
        .map_err(|e| failed(format!("{path}: {e}")))?;
    let built = scene.build().map_err(|e| failed(format!("{path}: {e}")))?;
    let mut low = optobench_math::Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut high = -low;
    let mut meshes = fineness.map(|f| optobench_io::mesh::meshes(&built, f).into_iter());
    let items: Vec<Value> = optobench_io::wireframe::wireframe(&built, max_points)
        .into_iter()
        .map(|(owner, lines)| {
            let mesh = meshes.as_mut().and_then(Iterator::next).map(|(_, m)| {
                let round = |x: f64, step: f64| (x / step).round() * step;
                json!({
                    "positions": m.positions.iter()
                        .flat_map(|p| [p.x, p.y, p.z].map(|x| round(x, 1e-4)))
                        .collect::<Vec<_>>(),
                    "normals": m.normals.iter()
                        .flat_map(|n| [n.x, n.y, n.z].map(|x| round(x, 1e-3)))
                        .collect::<Vec<_>>(),
                    "triangles": m.triangles.iter().flatten().collect::<Vec<_>>(),
                })
            });
            for p in lines.iter().flatten() {
                low = optobench_math::Vec3::new(low.x.min(p.x), low.y.min(p.y), low.z.min(p.z));
                high = optobench_math::Vec3::new(high.x.max(p.x), high.y.max(p.y), high.z.max(p.z));
            }
            let (kind, id, occurrence, role) = match owner {
                Owner::Element(i) => {
                    let e = &scene.elements[i];
                    let role = match e.kind {
                        ElementKind::Lens { .. } => "lens",
                        ElementKind::Mirror { .. } => "mirror",
                    };
                    ("element", e.id.clone(), None, role)
                }
                Owner::Proxy(i) => ("proxy", scene.proxies[i].id.clone(), None, "absorber"),
                Owner::Detector(i) => ("detector", scene.detectors[i].id.clone(), None, "detector"),
                Owner::Solid(i) => {
                    let s = &scene.solids[i];
                    let role = match s.role {
                        SolidRole::Absorber => "absorber",
                        SolidRole::Lens { .. } => "lens",
                        SolidRole::Mirror { .. } => "mirror",
                    };
                    ("solid", s.id.clone(), Some(s.occurrence.clone()), role)
                }
            };
            let mut item = json!({
                "kind": kind,
                "id": id,
                "occurrence": occurrence,
                "role": role,
                "lines": lines
                    .iter()
                    .map(|l| l.iter().map(|p| point(*p)).collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
            });
            if let (Some(mesh), Some(object)) = (mesh, item.as_object_mut()) {
                object.insert("mesh".into(), mesh);
            }
            item
        })
        .collect();
    Ok(json!({
        "items": items,
        "sources": scene.sources.iter().map(|s| json!({
            "id": s.id,
            "origin": point(s.frame.origin()),
            "axis": point(s.frame.axes()[2]),
        })).collect::<Vec<_>>(),
        "dofs": dofs_json(&scene),
        "bounds": if low.x.is_finite() { json!([point(low), point(high)]) } else { Value::Null },
    }))
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
