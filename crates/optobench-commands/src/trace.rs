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

pub(crate) fn text<'a>(
    parameters: &'a Parameters,
    key: &str,
) -> Result<Option<&'a str>, CommandError> {
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
/// The default for `small_range_mm`: slides that adjust this little or less either way
/// from the middle of their range are warned about.
pub(crate) const SMALL_RANGE_MM: f64 = 1.0;

/// `small_range_mm` from the parameters (default [`SMALL_RANGE_MM`]).
pub(crate) fn small_range(parameters: &Parameters) -> Result<f64, CommandError> {
    match parameters.get("small_range_mm") {
        None => Ok(SMALL_RANGE_MM),
        Some(v) => v.as_f64().filter(|x| *x >= 0.0).ok_or_else(|| {
            CommandError::new(
                ErrorCode::InvalidParameters,
                "`small_range_mm` is a travel in mm, not negative",
            )
        }),
    }
}

/// Warnings for the scene's slides (free, not coupled) that can adjust only `small` mm or
/// less either way from the middle of their range: little room to correct the part, so
/// it must be placed with care.
pub(crate) fn range_warnings(scene: &optobench_scene::Scene, small: f64) -> Vec<String> {
    scene
        .dofs
        .iter()
        .filter(|d| !d.motion.is_angle() && d.follows.is_none() && !d.locked)
        .filter_map(|d| {
            let (low, high) = d.range?;
            let half = 0.5 * (high - low);
            (half <= small + 1e-12).then(|| {
                format!(
                    "`{}` adjusts only ±{:.3} mm{}: little room to correct this part; place it with care",
                    d.name,
                    half,
                    d.source.as_ref().map_or(String::new(), |s| format!(" ({s})"))
                )
            })
        })
        .collect()
}

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
                    "kind": d.motion.kind(),
                    "unit": if angle { "deg" } else { "mm" },
                    "value": unit(d.value),
                    "locked": d.locked,
                });
                if let Some((low, high)) = d.range {
                    v["range"] = json!([unit(low), unit(high)]);
                }
                if let Some(source) = &d.source {
                    v["source"] = json!(source);
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
    let source = std::fs::read_to_string(path).map_err(|e| failed(format!("{path}: {e}")))?;
    load_text(path, source, parameters)
}

/// A design's text read as if it were the file at `path` (for checking an edit before it
/// is written).
#[allow(clippy::type_complexity)]
pub(crate) fn load_text(
    path: &str,
    source: String,
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
            "small_range_mm",
        ],
    )?;
    let small = small_range(parameters)?;
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
            "scattered_w": ledger.scattered,
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
        "warnings": range_warnings(&scene, small),
        "embedded": crate::embed::compare(&source, data.as_deref())?,
        "layers": traced.layers.iter().map(|l| json!({
            "sample": l.sample,
            "part": l.part,
            "absorbed_w": l.absorbed,
            "scattered_w": l.scattered,
        })).collect::<Vec<_>>(),
        "footprints": traced.footprints.iter().map(|f| json!({
            "id": f.id,
            "hits": f.hits,
            "power_w": f.power,
            "centroid_mm": [f.centroid.x, f.centroid.y, f.centroid.z],
            "radius_mm": f.radius,
            "clear_aperture_used": f.clear_aperture_used,
        })).collect::<Vec<_>>(),
    });
    // On a table layout: where alignment irises go on the bench.
    if let Some(irises) = crate::irises::alignment(&source, &scene, &built, &traced) {
        result["alignment_irises"] = irises;
    }
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
    let (scene, _, source, _) = load(path, parameters)?;
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
                        ElementKind::Objective { .. } => "objective",
                        ElementKind::IdealLens { .. } => "lens",
                        ElementKind::Sample { .. } => "sample",
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
            // Where a layout item sits: its frame's origin and +z (optical axis, a mirror's
            // normal), for the interface's positions and move handles.
            let frame = match owner {
                Owner::Element(i) => Some(scene.elements[i].frame),
                Owner::Proxy(i) => Some(scene.proxies[i].frame),
                Owner::Detector(i) => Some(scene.detectors[i].frame),
                Owner::Solid(_) => None,
            };
            if let (Some(frame), Some(object)) = (frame, item.as_object_mut()) {
                object.insert("origin".into(), json!(point(frame.origin())));
                object.insert("axis".into(), json!(point(frame.axes()[2])));
            }
            // A lens's first-order properties, for its editor: at the first source's
            // wavelength (d line without one).
            if let (Owner::Element(i), Some(object)) = (owner, item.as_object_mut()) {
                if let Some(lens) = lens_json(&scene, &scene.elements[i].kind) {
                    object.insert("lens".into(), lens);
                }
            }
            // A CAD body's own axes in the scene frame: a sidecar names faces in them
            // (a mirror's `reflect`).
            if let Owner::Solid(i) = owner {
                if let (Some(face), Some(object)) =
                    (scene.solids[i].faces.first(), item.as_object_mut())
                {
                    let r = face.placement.rotation;
                    object.insert(
                        "body_axes".into(),
                        json!([
                            point(r.apply(optobench_math::Vec3::X)),
                            point(r.apply(optobench_math::Vec3::Y)),
                            point(r.apply(optobench_math::Vec3::Z)),
                        ]),
                    );
                }
            }
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
        "design": design_json(&source),
    }))
}

/// The d line (µm), for lens properties when no source gives a wavelength.
const D_LINE: f64 = 0.587_56;

/// A lens element's shape and first-order properties: focal length (from the rear
/// principal plane), back focal length (from the last vertex), the glasses' indices, all
/// at the first source's wavelength.
fn lens_json(scene: &optobench_scene::Scene, kind: &optobench_scene::ElementKind) -> Option<Value> {
    let optobench_scene::ElementKind::Lens {
        surfaces,
        materials,
        ..
    } = kind
    else {
        return None;
    };
    let wavelength = scene
        .sources
        .first()
        .map(|s| s.spectrum.first().map_or(s.wavelength, |l| l.0))
        .unwrap_or(D_LINE);
    let indices: Vec<Value> = materials
        .iter()
        .map(|m| {
            m.material()
                .refractive_index(wavelength)
                .map_or(Value::Null, |n| json!((n * 1e6).round() / 1e6))
        })
        .collect();
    let radii: Vec<f64> = surfaces.iter().map(|s| s.radius).collect();
    let mut out = json!({
        "wavelength_um": wavelength,
        "indices": indices,
        "shape": lens_shape(&radii),
    });
    if let Some(c) = optobench_scene::lens_cardinal_points(kind, &scene.surrounding, wavelength) {
        out["focal_length_mm"] = json!(round(c.effective_focal_length));
        out["back_focal_length_mm"] = json!(round(c.back_focal_length));
    }
    Some(out)
}

/// A lens's shape by its radii (light along +z, a radius positive when its centre lies
/// further along): two surfaces as biconvex, plano-convex, convex-plano (the flat side
/// first), plano-concave, concave-plano, biconcave or meniscus; three as a cemented
/// doublet; more as a cemented group.
pub(crate) fn lens_shape(radii: &[f64]) -> &'static str {
    let flat = |r: f64| !r.is_finite() || r.abs() > 1e6;
    match radii {
        [r1, r2] => {
            // Convex on the way in: r1 > 0; convex on the way out: r2 < 0.
            match (flat(*r1), flat(*r2)) {
                (true, true) => "window",
                (false, true) if *r1 > 0.0 => "plano-convex",
                (false, true) => "plano-concave",
                (true, false) if *r2 < 0.0 => "convex-plano",
                (true, false) => "concave-plano",
                (false, false) => match (*r1 > 0.0, *r2 < 0.0) {
                    (true, true) => "biconvex",
                    (false, false) => "biconcave",
                    _ => "meniscus",
                },
            }
        }
        [_, _, _] => "doublet",
        _ => "cemented group",
    }
}

/// `focus`: where the light that reaches a detector focuses (M9a): per ray its launch
/// height and axial crossing, the paraxial, marginal and best focus (least RMS spot) and
/// that RMS radius, all along the detector's axis from its plane (mm). Parameters `path`;
/// optionally `detector` (an id; default the first), `dofs` and `data_directory`.
pub(crate) fn focus(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(parameters, &["path", "detector", "dofs", "data_directory"])?;
    let path = text(parameters, "path")?.ok_or_else(|| {
        CommandError::new(ErrorCode::InvalidParameters, "missing parameter `path`")
    })?;
    let (scene, _, _, _) = load(path, parameters)?;
    let overrides = dof_values(parameters, &scene)?;
    let scene = scene
        .moved(&overrides)
        .map_err(|e| failed(format!("{path}: {e}")))?;
    let index = match text(parameters, "detector")? {
        None => 0,
        Some(id) => scene
            .detectors
            .iter()
            .position(|d| d.id == id)
            .ok_or_else(|| {
                CommandError::new(ErrorCode::InvalidParameters, format!("no detector `{id}`"))
            })?,
    };
    let detector = scene.detectors.get(index).ok_or_else(|| {
        CommandError::new(
            ErrorCode::InvalidParameters,
            format!("{path}: the design has no detector"),
        )
    })?;
    let traced = scene.trace().map_err(|e| failed(format!("{path}: {e}")))?;
    let found = optobench_analysis::focus(&scene, &traced, index).ok_or_else(|| {
        failed(format!(
            "{path}: fewer than two rays reached `{}`",
            detector.id
        ))
    })?;
    Ok(json!({
        "detector": detector.id,
        "paraxial_mm": found.paraxial,
        "marginal_mm": found.marginal,
        "best_mm": found.best,
        "best_rms_mm": found.best_rms,
        "steepest_deg": found.steepest.to_degrees(),
        "crossings": found.crossings.iter().map(|c| json!({
            "height_mm": c.height, "z_mm": c.z, "miss_mm": c.miss, "power_w": c.power,
        })).collect::<Vec<_>>(),
    }))
}

/// `gaussian`: a Gaussian source's beam at every element and detector its axis reaches
/// (M9c): 1/e² radius, wavefront curvature, waist and distance to it per transverse
/// direction, the index there, and an objective's pupil fill w / (f · NA). Parameters
/// `path`; optionally `source` (an id; default the first Gaussian source), `dofs` and
/// `data_directory`.
pub(crate) fn gaussian(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(parameters, &["path", "source", "dofs", "data_directory"])?;
    let path = text(parameters, "path")?.ok_or_else(|| {
        CommandError::new(ErrorCode::InvalidParameters, "missing parameter `path`")
    })?;
    let (scene, _, _, _) = load(path, parameters)?;
    let overrides = dof_values(parameters, &scene)?;
    let scene = scene
        .moved(&overrides)
        .map_err(|e| failed(format!("{path}: {e}")))?;
    let index = match text(parameters, "source")? {
        Some(id) => scene
            .sources
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| {
                CommandError::new(ErrorCode::InvalidParameters, format!("no source `{id}`"))
            })?,
        None => scene
            .sources
            .iter()
            .position(|s| matches!(s.kind, optobench_scene::SourceKind::Gaussian { .. }))
            .ok_or_else(|| {
                CommandError::new(
                    ErrorCode::InvalidParameters,
                    format!("{path}: the design has no Gaussian source"),
                )
            })?,
    };
    let beams =
        optobench_analysis::gaussian(&scene, index).map_err(|e| failed(format!("{path}: {e}")))?;
    let pair = |p: (f64, f64)| {
        let finite = |x: f64| if x.is_finite() { json!(x) } else { Value::Null };
        json!([finite(p.0), finite(p.1)])
    };
    Ok(json!({
        "source": scene.sources[index].id,
        "planes": beams.iter().map(|b| json!({
            "id": b.id,
            "kind": b.kind,
            "radius_mm": pair(b.radius),
            "curvature_mm": pair(b.curvature),
            "waist_mm": pair(b.waist),
            "to_waist_mm": pair(b.to_waist),
            "index": b.index,
            "fill": b.fill,
        })).collect::<Vec<_>>(),
    }))
}

/// A design file's entries as JSON for editors: its kind, layout, and each
/// `[[element]]`, `[[source]]`, `[[detector]]`, `[[proxy]]` and `[[merit]]` with the keys
/// as written (without the `[resolved]` section).
fn design_json(source: &str) -> Value {
    let body = optobench_io::resolved::split(source)
        .map(|(body, _)| body)
        .unwrap_or(source);
    let Ok(table) = body.parse::<toml::Table>() else {
        return Value::Null;
    };
    let list = |key: &str| {
        table
            .get(key)
            .and_then(|v| serde_json::to_value(v).ok())
            .unwrap_or_else(|| json!([]))
    };
    json!({
        "kind": table.get("kind").and_then(toml::Value::as_str),
        "layout": table.get("layout").and_then(toml::Value::as_str),
        "step": table.get("step").and_then(toml::Value::as_str),
        "element": list("element"),
        "source": list("source"),
        "detector": list("detector"),
        "proxy": list("proxy"),
        "merit": list("merit"),
        "group": list("group"),
        "table": table.get("table").and_then(|v| serde_json::to_value(v).ok()),
        "planar": table.get("planar").and_then(|v| serde_json::to_value(v).ok()),
    })
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
