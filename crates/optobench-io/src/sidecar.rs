//! The `.optobench` sidecar: optical roles, sources and detectors for a CAD design read
//! from STEP (docs/design.md, Sidecar and library formats).
//!
//! ```toml
//! format = "optobench/1"
//! kind = "sidecar"
//! step = "assembly.step"            # optional: next to the sidecar; default same stem, .step or .stp
//! surrounding = "standard-air"      # optional, default 1.0
//!
//! [[element]]
//! id = "L1"
//! kind = "lens"
//! body = { product = "ac254-030-a", occurrence = "ac254-030-a:1" }  # see Matching
//! materials = { "20522-001-1-solid1" = "n-baf10", "20522-001-1-solid2" = "n-sf6ht" }
//! # or material = "fused-silica" for every body of the match
//! coating = "uncoated"              # or "ideal"
//!
//! [[element]]
//! id = "M1"
//! kind = "mirror"
//! body = { product = "pf10-03-p01p" }
//! reflect = [0, 0, 1]               # normal of the reflecting face, in the body's own frame
//!
//! [[source]]
//! id = "fiber"
//! kind = "point"                    # or "collimated" with diameter
//! wavelength_um = 0.633
//! power_w = 0.001
//! half_angle_deg = 5.0
//! position = [320, 0, 51.58]        # in the STEP root frame (mm)
//! axis = [-1, 0, 0]
//! # or frame = { body = { … }, offset = [0, 0, 0], axis = [0, 0, 1] } in that body's frame
//!
//! [[detector]]
//! id = "image"
//! position = [24.13, -130, 51.58]
//! axis = [0, -1, 0]
//! size = [10, 10]
//! pixels = [100, 100]
//! ```
//!
//! # Matching
//!
//! A `body` names a placed body of the STEP assembly, in this order of preference
//! (docs/design.md, Coupling with SolidCraft):
//!
//! 1. `product`: the STEP product name, optionally with `occurrence`, the assembly path of
//!    occurrence names joined by `/` (`er10:3`). A product placed more than once needs the
//!    occurrence.
//! 2. `occurrence` alone: the assembly path.
//! 3. `fingerprint`: the body's geometry fingerprint (from the `bodies` command), used when
//!    neither name is found, for example after a part was renamed.
//!
//! `body = "<name>"` inside the reference picks one body of a product with several. A
//! reference that matches nothing, or more than one placement, is an error, never a
//! silent skip. Every body no element names becomes an absorber, so clipping by mounts
//! shows up.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use optobench_materials::Medium;
use optobench_math::{Frame, Transform, Vec3, radians_from_degrees};
use optobench_scene::layout::frame_along;
use optobench_scene::{Detector, Scene, Solid, SolidFace, SolidRole, Source, SourceKind};
use optobench_surfaces::brep::FaceSurface;
use optobench_trace::Coating;
use toml::{Table, Value};

use crate::scene_file::{
    FileError, Item, MaterialLookup, check_header, check_keys, error, items, medium, with,
};
use crate::sha256::sha256_hex;
use crate::step::faces::{EDGE_TOLERANCE, PlacedFace, faces};
use crate::step::{self, StepModel, Surface};

/// One placed body of the assembly, ready to become a solid.
#[derive(Debug, Clone)]
pub struct PlacedBody {
    /// Occurrence names from the root down, joined by `/`.
    pub path: String,
    /// The product's name.
    pub product: String,
    /// The body's name in the file.
    pub name: String,
    /// Index of the body in its product.
    pub index: usize,
    /// Geometry fingerprint (see [`fingerprint`]).
    pub fingerprint: String,
    /// Body frame to root frame.
    pub placement: Transform,
    /// Its exact faces, in its own frame.
    pub faces: Vec<SolidFace>,
}

/// A body's geometry fingerprint: independent of names and placement, so it survives a
/// rename or a move. Built from the count of faces of each surface type, the rounded
/// radii of its cylinders, cones, spheres and tori, and its size (the sorted extents of
/// the box around its edge vertices in its own frame, to 0.01 mm), hashed to 16
/// hexadecimal digits.
pub fn fingerprint(body: &step::Body) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut radii: Vec<String> = Vec::new();
    for face in &body.faces {
        let (kind, radius) = match &face.surface {
            Surface::Plane(_) => ("plane", None),
            Surface::Cylinder { radius, .. } => ("cylinder", Some(*radius)),
            Surface::Cone { radius, .. } => ("cone", Some(*radius)),
            Surface::Sphere { radius, .. } => ("sphere", Some(*radius)),
            Surface::Torus { major, .. } => ("torus", Some(*major)),
            Surface::BSpline(_) => ("bspline", None),
            Surface::Revolution { .. } => ("revolution", None),
            Surface::Extrusion { .. } => ("extrusion", None),
            Surface::Unsupported(_) => ("unsupported", None),
        };
        *counts.entry(kind).or_default() += 1;
        if let Some(r) = radius {
            radii.push(format!("{kind}:{:.3}", r));
        }
    }
    radii.sort();
    let (low, high) = body
        .faces
        .iter()
        .flat_map(|f| &f.bounds)
        .flat_map(|l| {
            l.edges
                .iter()
                .flat_map(|e| [e.start, e.end])
                .chain(l.vertex)
        })
        .fold(
            (
                Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
                -Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            ),
            |(lo, hi), p| {
                (
                    Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)),
                    Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z)),
                )
            },
        );
    let mut extents = [high.x - low.x, high.y - low.y, high.z - low.z];
    extents.sort_by(f64::total_cmp);
    let size: Vec<String> = extents
        .iter()
        .map(|e| {
            if e.is_finite() {
                format!("{e:.2}")
            } else {
                "-".into()
            }
        })
        .collect();
    let text = format!("{counts:?}|{}|{}", radii.join(","), size.join("x"));
    sha256_hex(text.as_bytes())[..16].to_owned()
}

/// Every placed body of a STEP model with its exact faces.
pub fn placed_bodies(model: &StepModel) -> (Vec<PlacedBody>, usize) {
    let converted = faces(model, EDGE_TOLERANCE);
    let mut bodies: BTreeMap<(String, usize, usize), PlacedBody> = BTreeMap::new();
    let mut order: Vec<(String, usize, usize)> = Vec::new();
    for instance in model.instances() {
        let path = instance.path.join("/");
        let product = &model.products[instance.product];
        let body = &product.bodies[instance.body];
        let key = (path.clone(), instance.product, instance.body);
        order.push(key.clone());
        bodies.insert(
            key,
            PlacedBody {
                path,
                product: product.name.clone(),
                name: body.name.clone(),
                index: instance.body,
                fingerprint: fingerprint(body),
                placement: instance.transform,
                faces: Vec::new(),
            },
        );
    }
    for PlacedFace {
        path,
        product,
        body,
        geometry,
        placement,
        ..
    } in converted.faces
    {
        if let Some(b) = bodies.get_mut(&(path.join("/"), product, body)) {
            b.faces.push(SolidFace {
                geometry: geometry.clone(),
                placement,
            });
        }
    }
    let out = order.iter().filter_map(|k| bodies.remove(k)).collect();
    (out, converted.skipped)
}

/// The bodies a `body = { … }` reference matches (indices into `bodies`).
fn matches(bodies: &[PlacedBody], reference: &Item) -> Result<Vec<usize>, FileError> {
    check_keys(
        reference.table,
        &["product", "occurrence", "body", "fingerprint"],
        &reference.context,
    )?;
    let product = reference.table.get("product").and_then(Value::as_str);
    let occurrence = reference.table.get("occurrence").and_then(Value::as_str);
    let body = reference.table.get("body").and_then(Value::as_str);
    let fingerprint = reference.table.get("fingerprint").and_then(Value::as_str);
    if product.is_none() && occurrence.is_none() && fingerprint.is_none() {
        return Err(error(format!(
            "{}: a body is named by `product`, `occurrence` or `fingerprint`",
            reference.context
        )));
    }
    let select = |test: &dyn Fn(&PlacedBody) -> bool| -> Vec<usize> {
        (0..bodies.len()).filter(|&i| test(&bodies[i])).collect()
    };
    let in_path = |b: &PlacedBody| {
        occurrence.is_none_or(|o| b.path == o || b.path.ends_with(&format!("/{o}")))
    };
    let named = |b: &PlacedBody| body.is_none_or(|n| b.name == n);
    let mut found = Vec::new();
    if let Some(product) = product {
        found = select(&|b: &PlacedBody| b.product == product && in_path(b) && named(b));
    }
    if found.is_empty() && product.is_none() && occurrence.is_some() {
        found = select(&|b: &PlacedBody| in_path(b) && named(b));
    }
    if found.is_empty() {
        if let Some(fingerprint) = fingerprint {
            found = select(&|b: &PlacedBody| b.fingerprint == fingerprint);
        }
    }
    if found.is_empty() {
        return Err(error(format!(
            "{}: matches no body of the STEP assembly (see the `bodies` command for names and fingerprints)",
            reference.context
        )));
    }
    let placements: std::collections::BTreeSet<&str> =
        found.iter().map(|&i| bodies[i].path.as_str()).collect();
    if placements.len() > 1 {
        return Err(error(format!(
            "{}: matches {} placements ({}); add `occurrence`",
            reference.context,
            placements.len(),
            placements.into_iter().collect::<Vec<_>>().join(", ")
        )));
    }
    Ok(found)
}

fn reference<'a>(item: &Item<'a>, key: &str) -> Result<Item<'a>, FileError> {
    let table = item.get(key)?.as_table().ok_or_else(|| {
        error(format!(
            "{}: `{key}` must be a table such as {{ product = \"…\" }}",
            item.context
        ))
    })?;
    Ok(Item {
        table,
        context: format!("{} `{key}`", item.context),
    })
}

/// The frame of a source or detector: `position` and `axis` in the root frame, or
/// `frame = { body, offset, axis }` relative to a body.
fn placement_frame(item: &Item, bodies: &[PlacedBody]) -> Result<Frame, FileError> {
    let invalid = |what: &str| error(format!("{}: {what}", item.context));
    if item.table.contains_key("frame") {
        let frame = reference(item, "frame")?;
        check_keys(frame.table, &["body", "offset", "axis"], &frame.context)?;
        let body_ref = reference(&frame, "body")?;
        let found = matches(bodies, &body_ref)?;
        let placement = bodies[found[0]].placement;
        let offset = if frame.table.contains_key("offset") {
            frame.vector("offset")?
        } else {
            Vec3::ZERO
        };
        let axis = frame.vector("axis")?;
        return frame_along(placement.apply_point(offset), placement.apply_vector(axis))
            .ok_or_else(|| invalid("axis has zero length"));
    }
    frame_along(item.vector("position")?, item.vector("axis")?)
        .ok_or_else(|| invalid("axis has zero length"))
}

/// The STEP file a sidecar refers to.
fn step_path(root: &Table, sidecar: &Path) -> Result<PathBuf, FileError> {
    let directory = sidecar.parent().unwrap_or(Path::new("."));
    if let Some(step) = root.get("step") {
        let name = step
            .as_str()
            .ok_or_else(|| error("`step` must be a file name"))?;
        return Ok(directory.join(name));
    }
    let stem = sidecar
        .file_stem()
        .ok_or_else(|| error("the sidecar has no file name"))?;
    for extension in ["step", "stp", "STEP", "STP"] {
        let candidate = directory.join(stem).with_extension(extension);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(error(format!(
        "no STEP file named {}.step or .stp next to the sidecar; give it with `step = \"…\"`",
        stem.to_string_lossy()
    )))
}

/// Reads a sidecar and its STEP file into a scene. `sidecar` is the sidecar's path (the
/// STEP file is looked up next to it).
pub fn parse(text: &str, sidecar: &Path, lookup: &MaterialLookup) -> Result<Scene, FileError> {
    check_header(text)?;
    let root: Table = text
        .parse()
        .map_err(|e: toml::de::Error| error(format!("not valid TOML: {e}")))?;
    let top = Item {
        table: &root,
        context: "top level".into(),
    };
    let format = top.text("format")?;
    if format != crate::format_tag() {
        return Err(error(format!(
            "format `{format}` is not the one this release reads ({})",
            crate::format_tag()
        )));
    }
    if top.text("kind")? != "sidecar" {
        return Err(error("this reader takes kind = \"sidecar\""));
    }
    check_keys(
        &root,
        &[
            "format",
            "kind",
            "step",
            "surrounding",
            "element",
            "source",
            "detector",
            "trace",
        ],
        "top level",
    )?;
    let surrounding: Medium = match root.get("surrounding") {
        None => 1.0.into(),
        Some(v) => medium(v, lookup, "surrounding")?,
    };

    let path = step_path(&root, sidecar)?;
    let step_text =
        std::fs::read_to_string(&path).map_err(|e| error(format!("{}: {e}", path.display())))?;
    let model = step::read(&step_text).map_err(|e| error(format!("{}: {e}", path.display())))?;
    let (bodies, skipped) = placed_bodies(&model);
    if skipped > 0 {
        return Err(error(format!(
            "{}: {skipped} faces use geometry this reader does not support",
            path.display()
        )));
    }

    let mut scene = Scene {
        surrounding,
        elements: Vec::new(),
        sources: Vec::new(),
        proxies: Vec::new(),
        detectors: Vec::new(),
        solids: Vec::new(),
        split_cutoff: crate::scene_file::trace_options(&root)?,
    };
    let mut tagged = vec![false; bodies.len()];

    for item in items(&root, "element")? {
        let id = item.text("id")?.to_owned();
        let kind = item.text("kind")?;
        let found = matches(&bodies, &reference(&item, "body")?)?;
        for &i in &found {
            if tagged[i] {
                return Err(error(format!(
                    "{}: body `{}` of `{}` is already named by another element",
                    item.context, bodies[i].name, bodies[i].path
                )));
            }
            tagged[i] = true;
        }
        let solid_id = |i: usize| {
            if found.len() == 1 {
                id.clone()
            } else {
                format!("{id}/{}", bodies[i].name)
            }
        };
        match kind {
            "lens" => {
                check_keys(
                    item.table,
                    &["id", "kind", "body", "material", "materials", "coating"],
                    &item.context,
                )?;
                let coating = match item.table.get("coating").and_then(Value::as_str) {
                    None | Some("uncoated") => Coating::Uncoated,
                    Some("ideal") => Coating::Ideal,
                    Some(other) => {
                        return Err(error(format!(
                            "{}: coating `{other}` is not known; use \"uncoated\" or \"ideal\"",
                            item.context
                        )));
                    }
                };
                let per_body = item.table.get("materials").and_then(Value::as_table);
                for &i in &found {
                    let value = match (item.table.get("material"), per_body) {
                        (Some(v), None) => v,
                        (None, Some(table)) => table.get(&bodies[i].name).ok_or_else(|| {
                            error(format!(
                                "{}: `materials` has no entry for body `{}`",
                                item.context, bodies[i].name
                            ))
                        })?,
                        _ => {
                            return Err(error(format!(
                                "{}: give either `material` or `materials` (one per body)",
                                item.context
                            )));
                        }
                    };
                    scene.solids.push(Solid {
                        id: solid_id(i),
                        faces: bodies[i].faces.clone(),
                        role: SolidRole::Lens {
                            material: medium(value, lookup, &item.context)?,
                            coating: coating.clone(),
                        },
                    });
                }
            }
            "mirror" => {
                check_keys(
                    item.table,
                    &["id", "kind", "body", "reflect"],
                    &item.context,
                )?;
                let normal = item
                    .vector("reflect")?
                    .normalized()
                    .ok_or_else(|| error(format!("{}: `reflect` has zero length", item.context)))?;
                for &i in &found {
                    // Planar faces whose outward normal is within 1° of `reflect`.
                    let reflecting: Vec<usize> = bodies[i]
                        .faces
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| match &f.geometry.surface {
                            FaceSurface::Plane(frame) => {
                                let n = frame.axes()[2];
                                let n = if f.geometry.reversed { -n } else { n };
                                n.dot(normal) > radians_from_degrees(1.0).cos()
                            }
                            _ => false,
                        })
                        .map(|(k, _)| k)
                        .collect();
                    if reflecting.is_empty() {
                        return Err(error(format!(
                            "{}: body `{}` has no flat face facing {:?} (in its own frame)",
                            item.context,
                            bodies[i].name,
                            [normal.x, normal.y, normal.z]
                        )));
                    }
                    scene.solids.push(Solid {
                        id: solid_id(i),
                        faces: bodies[i].faces.clone(),
                        role: SolidRole::Mirror { reflecting },
                    });
                }
            }
            other => {
                return Err(error(format!(
                    "{}: element kind `{other}` is not known; use \"lens\" or \"mirror\"",
                    item.context
                )));
            }
        }
    }

    // Every other body absorbs. Ids: path (or product) and body name, numbered when a
    // product has two bodies of the same name.
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for (i, body) in bodies.iter().enumerate() {
        if tagged[i] {
            continue;
        }
        let place = if body.path.is_empty() {
            &body.product
        } else {
            &body.path
        };
        let mut id = format!("{place}/{}", body.name);
        let count = seen.entry(id.clone()).or_default();
        *count += 1;
        if *count > 1 {
            id = format!("{id}#{}", body.index);
        }
        scene.solids.push(Solid {
            id,
            faces: body.faces.clone(),
            role: SolidRole::Absorber,
        });
    }

    let position = ["position", "axis", "frame"];
    for item in items(&root, "source")? {
        let kind = match item.text("kind")? {
            "collimated" => {
                check_keys(
                    item.table,
                    &with(
                        &position,
                        &[
                            "id",
                            "kind",
                            "wavelength_um",
                            "power_w",
                            "diameter",
                            "rings",
                        ],
                    ),
                    &item.context,
                )?;
                SourceKind::Collimated {
                    diameter: item.positive("diameter")?,
                }
            }
            "point" => {
                check_keys(
                    item.table,
                    &with(
                        &position,
                        &[
                            "id",
                            "kind",
                            "wavelength_um",
                            "power_w",
                            "half_angle_deg",
                            "rings",
                        ],
                    ),
                    &item.context,
                )?;
                SourceKind::Point {
                    half_angle: radians_from_degrees(item.positive("half_angle_deg")?),
                }
            }
            other => {
                return Err(error(format!(
                    "{}: source kind `{other}` is not known; use \"collimated\" or \"point\"",
                    item.context
                )));
            }
        };
        let rings = item.optional_number("rings", 6.0)?;
        if rings < 0.0 || rings.fract() != 0.0 || rings > 200.0 {
            return Err(error(format!(
                "{}: `rings` must be a whole number from 0 to 200",
                item.context
            )));
        }
        scene.sources.push(Source {
            id: item.text("id")?.to_owned(),
            frame: placement_frame(&item, &bodies)?,
            kind,
            wavelength: item.positive("wavelength_um")?,
            power: item.positive("power_w")?,
            rings: rings as usize,
        });
    }
    for item in items(&root, "detector")? {
        check_keys(
            item.table,
            &with(&position, &["id", "size", "pixels"]),
            &item.context,
        )?;
        let size = item.pair("size")?;
        if !(size.0 > 0.0 && size.1 > 0.0) {
            return Err(error(format!("{}: `size` must be positive", item.context)));
        }
        scene.detectors.push(Detector {
            id: item.text("id")?.to_owned(),
            frame: placement_frame(&item, &bodies)?,
            size,
            pixels: item.count_pair("pixels")?,
        });
    }
    Ok(scene)
}

/// The `kind` of an `.optobench` file, from its second line.
pub fn kind_of(text: &str) -> Option<String> {
    let table: Table = text.parse().ok()?;
    table.get("kind")?.as_str().map(str::to_owned)
}
