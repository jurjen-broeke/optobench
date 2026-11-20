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
//! coating = "uncoated"              # or "ideal", a curve or layers: see `coatings`
//!
//! [[element]]
//! id = "F1"
//! kind = "filter"                   # a plate: its curve is the whole filter's response,
//! body = { product = "mvf52a", body = "MVF52A-1-OC-2-solid1" }   # on one flat face,
//! material = "n-bk7"                # the other ideal
//! coating = { curve = "mvf52a" }
//!
//! [[element]]
//! id = "L2"
//! body = { product = "ac254-060-a" }  # a mounted part: its glass bodies are found by shape
//! library = "thorlabs/AC254-060-A"  # prescription and glasses from the parts library
//!
//! [[element]]
//! id = "M2"
//! kind = "mirror"
//! body = { product = "pf10-03-p01p" }
//! reflect = [0, 0, 1]
//! # Degrees of freedom as in layout files, moving the element's whole CAD occurrence
//! # (part and mount); axes in the assembly frame unless frame = "local" (the body's
//! # own), tilts about the centre of the element's bodies unless given a `pivot`.
//! dof = [ { name = "M2_tip", kind = "tilt", axis = [1, 0, 0], range = [-4, 4] } ]
//!
//! [[element]]
//! id = "M1"
//! kind = "mirror"
//! body = { product = "pf10-03-p01p" }
//! reflect = [0, 0, 1]               # normal of the reflecting face, in the body's own frame
//!
//! [[source]]
//! id = "fiber"
//! kind = "point"                    # or "collimated", "gaussian", "fiber", "led", "isotropic": the keys
//!                                   # and `spectrum` as in layout files (scene_file)
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
//! # quantum_efficiency = { value = 0.3, source = "…" }   # optional, as in layout files
//! # returning = true                 # optional, as in layout files: light coming back
//!
//! [[merit]]                         # what `optimize` aligns for, as in layout files
//! kind = "centring"
//! detector = "image"
//!
//! # [resolved] ...                  # written by `embed`, always last: see `resolved`
//! ```
//!
//! # Objectives and samples
//!
//! An ideal objective (`kind = "objective"`, keys as in layout files, see
//! `scene_file`) is placed like a source or detector, by `position` and `axis` or by
//! `frame = { body, offset, axis }` on a body: the frame's origin is its pupil plane and
//! its axis points to the sample. `body` names the objective's own bodies (barrel and
//! glass), which the ideal model then replaces in the trace. A `sample` is placed the
//! same way and names no body. Their degrees of freedom move the element itself; axes
//! in the assembly frame unless `frame = "local"`.
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
//!
//! # Parts library
//!
//! An element with `library` takes its glasses from the part (see `library`). Of the
//! bodies its `body` matches, those whose exact faces have the part's radii and centre
//! thicknesses become its glasses, each the glass it matches; the others (the mount, a
//! retaining ring) absorb. A single body is taken as it is.
//!
//! A STEP product named after a part (its part number or one of its `products`, case
//! ignored) that no element names is tagged automatically the same way, with its
//! occurrence path as the element id. `[library]` with `automatic = false` switches that
//! off. Library mirrors are not tagged automatically: which face reflects needs `reflect`.
//!
//! Either way the radii, centre thicknesses and diameter read from STEP are compared
//! with the part, and differences above 0.05 mm are reported with the trace
//! (`optobench_library::CHECK_TOLERANCE`). The exact STEP faces are what is traced.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use optobench_library::{Comparison, Library, Part, Profile, closest, profile};
use optobench_materials::Medium;
use optobench_math::{Frame, Transform, Vec3, radians_from_degrees};
use optobench_scene::layout::frame_along;
use optobench_scene::{Detector, Scene, Solid, SolidFace, SolidRole};
use optobench_surfaces::brep::FaceSurface;
use optobench_trace::Coating;
use toml::{Table, Value};

use crate::scene_file::{
    FileError, Item, Lookups, MaterialLookup, OBJECTIVE_KEYS, SAMPLE_KEYS, check_header,
    check_keys, dofs, error, items, medium, objective_kind, resolve_coatings, sample_kind, with,
    with_element_coatings,
};
use crate::sha256::sha256_hex;
use crate::step::faces::{EDGE_TOLERANCE, PlacedFace, faces};
use crate::step::{self, StepModel, Surface};
use optobench_library::CoatingSpec;
use optobench_scene::dof::Target;

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

/// The CAD occurrence an element's bodies belong to, with the frame of its first body and
/// the centre of the box around its bodies (assembly frame); `None` without bodies. Its
/// bodies must all be in one occurrence.
fn element_occurrence(
    item: &Item,
    bodies: &[PlacedBody],
    found: &[usize],
) -> Result<Option<(String, Frame, Vec3)>, FileError> {
    let occurrences: Vec<String> = found.iter().map(|&i| bodies[i].occurrence()).collect();
    let Some(first) = occurrences.first() else {
        return Ok(None);
    };
    if occurrences.iter().any(|o| o != first) {
        return Err(error(format!(
            "{}: an element that moves (a DOF, a tolerance) must be one CAD occurrence; its bodies are in {}",
            item.context,
            occurrences.join(", ")
        )));
    }
    let points: Vec<Vec3> = found
        .iter()
        .flat_map(|&i| {
            let body = &bodies[i];
            body.faces.iter().flat_map(move |f| {
                f.geometry
                    .loops
                    .iter()
                    .flatten()
                    .map(move |p| body.placement.apply_point(*p))
            })
        })
        .collect();
    let centre = if points.is_empty() {
        bodies[found[0]].placement.translation
    } else {
        let low = points.iter().fold(points[0], |a, p| {
            Vec3::new(a.x.min(p.x), a.y.min(p.y), a.z.min(p.z))
        });
        let high = points.iter().fold(points[0], |a, p| {
            Vec3::new(a.x.max(p.x), a.y.max(p.y), a.z.max(p.z))
        });
        (low + high) * 0.5
    };
    Ok(Some((
        first.clone(),
        Frame::from_transform(bodies[found[0]].placement),
        centre,
    )))
}

/// An element's DOFs: they move its CAD occurrence (every body of it, the mount too).
/// Axes are in the assembly frame unless a DOF says `frame = "local"` (the body's own
/// frame); tilts turn about the centre of the element's bodies unless given a `pivot`.
fn element_dofs(
    item: &Item,
    bodies: &[PlacedBody],
    found: &[usize],
) -> Result<Vec<optobench_scene::dof::Dof>, FileError> {
    if !item.table.contains_key("dof") {
        return Ok(Vec::new());
    }
    let Some((occurrence, local, centre)) = element_occurrence(item, bodies, found)? else {
        return Ok(Vec::new());
    };
    dofs(item, &Target::Occurrence(occurrence), &local, true, centre)
}

impl PlacedBody {
    /// The CAD occurrence it belongs to: its assembly path, or its product's name for a
    /// body at the root.
    pub fn occurrence(&self) -> String {
        if self.path.is_empty() {
            self.product.clone()
        } else {
            self.path.clone()
        }
    }
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
    let library = Library::default();
    let lookups = Lookups {
        materials: lookup,
        curves: &crate::coatings::no_curves,
        library: &library,
    };
    read(text, sidecar, &lookups).map(|read| read.scene)
}

/// A glass body of a library part, as matched.
#[derive(Debug, Clone, PartialEq)]
pub struct GlassBody {
    /// The solid's id.
    pub solid: String,
    /// Which glass of the part (0-based).
    pub glass: usize,
    /// How its STEP geometry compares with the part; `None` when no prescription could
    /// be read from its faces.
    pub comparison: Option<Comparison>,
    /// The body's faces (indices) that are the part's surfaces `glass` and `glass + 1`;
    /// empty when not known.
    pub faces: [Vec<usize>; 2],
}

/// The faces of a profile's two surfaces in the part's order.
fn part_order_faces(profile: &Profile, comparison: &Comparison) -> [Vec<usize>; 2] {
    match &profile.surface_faces[..] {
        [a, b] if comparison.reversed => [b.clone(), a.clone()],
        [a, b] => [a.clone(), b.clone()],
        _ => [Vec::new(), Vec::new()],
    }
}

/// An element whose prescription comes from the parts library.
#[derive(Debug, Clone, PartialEq)]
pub struct PartMatch {
    /// The element id.
    pub element: String,
    /// The part's key.
    pub part: String,
    /// Tagged automatically (its STEP product is named after the part), not by an
    /// `[[element]]`.
    pub automatic: bool,
    /// Its glass bodies.
    pub bodies: Vec<GlassBody>,
    /// Glasses of the part (0-based) no body was found for.
    pub missing: Vec<usize>,
}

/// A sidecar as read: the scene, and the elements that came from the parts library.
#[derive(Debug, Clone)]
pub struct Read {
    /// The scene.
    pub scene: Scene,
    /// Library elements with their geometry check.
    pub parts: Vec<PartMatch>,
}

/// The glass bodies of `part` among `bodies` (indices into `bodies`), with the glass each
/// one is. One body is taken as it is, at its closest glass; of several (a mounted part,
/// a doublet), those that fit the part, all facing the same way, one per glass.
fn glass_bodies(part: &Part, bodies: &[&PlacedBody]) -> Vec<(usize, GlassBody)> {
    // Each body's axis takes its sign from its own rim; turn them all one way (in the
    // assembly frame), so "facing the same way" means the same for every body.
    let mut direction: Option<Vec3> = None;
    let profiles: Vec<Option<Profile>> = bodies
        .iter()
        .map(|b| {
            let p = profile(&b.faces.iter().map(|f| &*f.geometry).collect::<Vec<_>>())?;
            let axis = b.placement.apply_vector(p.axis);
            let reference = *direction.get_or_insert(axis);
            Some(if axis.dot(reference) < 0.0 {
                p.flipped()
            } else {
                p
            })
        })
        .collect();
    if bodies.len() == 1 {
        let comparison = profiles[0].as_ref().and_then(|p| closest(part, p, None));
        let faces = match (&profiles[0], &comparison) {
            (Some(p), Some(c)) => part_order_faces(p, c),
            _ => [Vec::new(), Vec::new()],
        };
        return vec![(
            0,
            GlassBody {
                solid: String::new(),
                glass: comparison.as_ref().map_or(0, |c| c.glass),
                comparison,
                faces,
            },
        )];
    }
    // Per direction: the fitting bodies, at most one per glass; the direction with more
    // of them wins, then the smaller total difference.
    let score = |chosen: &[(usize, Comparison)]| chosen.iter().map(|(_, c)| c.score).sum::<f64>();
    let mut best: Vec<(usize, Comparison)> = Vec::new();
    for reversed in [false, true] {
        let mut chosen: Vec<(usize, Comparison)> = Vec::new();
        for (i, p) in profiles.iter().enumerate() {
            let Some(c) = p
                .as_ref()
                .and_then(|p| closest(part, p, Some(reversed)))
                .filter(|c| c.fits)
            else {
                continue;
            };
            match chosen.iter_mut().find(|(_, d)| d.glass == c.glass) {
                Some(slot) if slot.1.score > c.score => *slot = (i, c),
                Some(_) => {}
                None => chosen.push((i, c)),
            }
        }
        if chosen.len() > best.len()
            || (chosen.len() == best.len() && score(&chosen) < score(&best))
        {
            best = chosen;
        }
    }
    let mut chosen = best;
    chosen.sort_by_key(|(_, c)| c.glass);
    chosen
        .into_iter()
        .map(|(i, c)| {
            let faces = profiles[i]
                .as_ref()
                .map_or([Vec::new(), Vec::new()], |p| part_order_faces(p, &c));
            (
                i,
                GlassBody {
                    solid: String::new(),
                    glass: c.glass,
                    comparison: Some(c),
                    faces,
                },
            )
        })
        .collect()
}

/// Turns the glass bodies of a library element into lens solids and records the match.
#[allow(clippy::too_many_arguments)]
fn add_part(
    scene: &mut Scene,
    tagged: &mut [bool],
    bodies: &[PlacedBody],
    candidates: &[usize],
    part: &Part,
    id: &str,
    automatic: bool,
    lookups: &Lookups,
    context: &str,
) -> Result<Option<PartMatch>, FileError> {
    let coatings = resolve_coatings(&part.surface_coatings(), lookups, context)?;
    let placed: Vec<&PlacedBody> = candidates.iter().map(|&i| &bodies[i]).collect();
    let found = glass_bodies(part, &placed);
    if found.is_empty() {
        if automatic {
            return Ok(None);
        }
        return Err(error(format!(
            "{context}: no body matches part `{}` (see the `bodies` command)",
            part.key()
        )));
    }
    let mut glass_bodies = Vec::new();
    let several = found.len() > 1;
    for (k, mut glass_body) in found {
        let index = candidates[k];
        let body = &bodies[index];
        tagged[index] = true;
        let solid = if several {
            format!("{id}/{}", body.name)
        } else {
            id.to_owned()
        };
        let material = crate::library::glass(&part.glass[glass_body.glass], lookups.materials)
            .map_err(|e| error(format!("{context} (part `{}`): {e}", part.key())))?;
        // Each surface's coating on the faces that are that surface; when they are not
        // known, the glass's first surface coating on every optical face.
        let g = glass_body.glass;
        let known = glass_body.faces.iter().any(|f| !f.is_empty());
        let face_coatings: Vec<(usize, Coating)> = glass_body
            .faces
            .iter()
            .zip([&coatings[g], &coatings[g + 1]])
            .flat_map(|(faces, c)| faces.iter().map(move |&f| (f, c.clone())))
            .collect();
        scene.solids.push(Solid {
            id: solid.clone(),
            faces: body.faces.clone(),
            occurrence: body.occurrence(),
            role: SolidRole::Lens {
                material,
                coating: if known {
                    Coating::Uncoated
                } else {
                    coatings[g].clone()
                },
                face_coatings,
            },
        });
        glass_body.solid = solid;
        glass_bodies.push(glass_body);
    }
    let missing = (0..part.glass.len())
        .filter(|g| glass_bodies.iter().all(|b| b.glass != *g))
        .collect();
    Ok(Some(PartMatch {
        element: id.to_owned(),
        part: part.key(),
        automatic,
        bodies: glass_bodies,
        missing,
    }))
}

/// A sidecar, with parts from `library`: elements that name a part, and STEP products
/// named after a part (tagged automatically unless `[library] automatic = false`).
pub fn read(text: &str, sidecar: &Path, lookups: &Lookups) -> Result<Read, FileError> {
    check_header(text)?;
    let mut root: Table = text
        .parse()
        .map_err(|e: toml::de::Error| error(format!("not valid TOML: {e}")))?;
    // Embedded values (`[resolved]`) first, then the user's own data.
    let embedded = crate::resolved::read(&root)?;
    let merged_library = embedded.library(lookups.library)?;
    crate::mounts::expand(&mut root, &merged_library)?;
    crate::groups::expand(&mut root)?;
    let merged_materials = |name: &str| {
        embedded
            .material(name)
            .map_or_else(|| (lookups.materials)(name), Ok)
    };
    let merged_curves = |name: &str| {
        embedded
            .curve(name)
            .map_or_else(|| (lookups.curves)(name), Ok)
    };
    let lookups = &Lookups {
        materials: &merged_materials,
        curves: &merged_curves,
        library: &merged_library,
    };
    let (lookup, library) = (lookups.materials, lookups.library);
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
            "resolved",
            "merit",
            "library",
            "fluorophore",
            "tolerance",
            "tolerancing",
        ],
        "top level",
    )?;
    let automatic = match root.get("library") {
        None => true,
        Some(value) => {
            let table = value
                .as_table()
                .ok_or_else(|| error("[library] must be a table"))?;
            check_keys(table, &["automatic"], "[library]")?;
            match table.get("automatic") {
                None => true,
                Some(Value::Boolean(b)) => *b,
                Some(_) => return Err(error("[library]: `automatic` is true or false")),
            }
        }
    };
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

    let (split_cutoff, threads) = crate::scene_file::trace_options(&root)?;
    let mut scene = Scene {
        surrounding,
        elements: Vec::new(),
        sources: Vec::new(),
        proxies: Vec::new(),
        detectors: Vec::new(),
        solids: Vec::new(),
        split_cutoff,
        threads,
        dofs: Vec::new(),
        fluorophores: Vec::new(),
        thermal: Vec::new(),
        tolerances: Vec::new(),
    };
    let mut tagged = vec![false; bodies.len()];
    // Bodies an element names, tagged or not (a library part's mount).
    let mut claimed = vec![false; bodies.len()];
    let mut cad: Vec<crate::tolerance::CadElement> = Vec::new();
    let mut parts: Vec<PartMatch> = Vec::new();

    for item in items(&root, "element")? {
        let id = item.text("id")?.to_owned();
        // An ideal objective or a sample is placed like a source or detector; an
        // objective may name its CAD body (barrel and glass), which the ideal model then
        // replaces in the trace.
        let kind = item.table.get("kind").and_then(Value::as_str);
        if matches!(kind, Some("objective" | "sample")) {
            let objective = kind == Some("objective");
            let mut keys = with(
                &["position", "axis", "frame"],
                if objective {
                    OBJECTIVE_KEYS
                } else {
                    SAMPLE_KEYS
                },
            );
            if objective {
                keys.push("body");
            }
            check_keys(item.table, &keys, &item.context)?;
            if objective && item.table.contains_key("body") {
                for i in matches(&bodies, &reference(&item, "body")?)? {
                    if tagged[i] {
                        return Err(error(format!(
                            "{}: body `{}` of `{}` is already named by another element",
                            item.context, bodies[i].name, bodies[i].path
                        )));
                    }
                    tagged[i] = true;
                    claimed[i] = true;
                }
            }
            let frame = placement_frame(&item, &bodies)?;
            let element_kind = if objective {
                objective_kind(&item, lookup)?
            } else {
                if let Some(thermal) = crate::scene_file::sample_thermal(&item, &id)? {
                    scene.thermal.push(thermal);
                }
                sample_kind(&item, lookup)?
            };
            scene.dofs.extend(dofs(
                &item,
                &optobench_scene::dof::Target::Element(id.clone()),
                &frame,
                true,
                frame.origin(),
            )?);
            scene.elements.push(optobench_scene::Element {
                id,
                frame,
                kind: element_kind,
            });
            continue;
        }
        let found = matches(&bodies, &reference(&item, "body")?)?;
        if let Some(value) = item.table.get("library") {
            check_keys(
                item.table,
                &[
                    "id", "kind", "body", "library", "coating", "coatings", "dof",
                ],
                &item.context,
            )?;
            let key = value.as_str().ok_or_else(|| {
                error(format!(
                    "{}: `library` is \"<vendor>/<part number>\"",
                    item.context
                ))
            })?;
            let part = library.get(key).ok_or_else(|| {
                error(format!(
                    "{}: part `{key}` is not in the parts library (see the `parts` command)",
                    item.context
                ))
            })?;
            if !part.part_type.is_optic() {
                return Err(error(format!(
                    "{}: part `{key}` is a mount; give it with `mount = \"{key}\"`",
                    item.context
                )));
            }
            if !part.part_type.transmits()
                || item
                    .table
                    .get("kind")
                    .is_some_and(|k| k.as_str() != Some("lens"))
            {
                return Err(error(format!(
                    "{}: library parts in sidecars are lenses and filters (`{}` is a {}); tag mirrors with kind = \"mirror\" and `reflect`",
                    item.context,
                    part.key(),
                    part.part_type.as_str()
                )));
            }
            if let Some(&i) = found.iter().find(|&&i| tagged[i]) {
                return Err(error(format!(
                    "{}: body `{}` of `{}` is already named by another element",
                    item.context, bodies[i].name, bodies[i].path
                )));
            }
            let part = &with_element_coatings(&item, part)?;
            if let Some(matched) = add_part(
                &mut scene,
                &mut tagged,
                &bodies,
                &found,
                part,
                &id,
                false,
                lookups,
                &item.context,
            )? {
                parts.push(matched);
            }
            // The part's other bodies (its mount) stay untagged and absorb, so the
            // element claims all of them from automatic tagging.
            for &i in &found {
                claimed[i] = true;
            }
            scene.dofs.extend(element_dofs(&item, &bodies, &found)?);
            if let Some((path, frame, centre)) =
                element_occurrence(&item, &bodies, &found).unwrap_or(None)
            {
                cad.push(crate::tolerance::CadElement {
                    id: item.text("id")?.to_owned(),
                    occurrence: path,
                    frame,
                    centre,
                });
            }
            continue;
        }
        let kind = item.text("kind")?;
        for &i in &found {
            claimed[i] = true;
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
            "lens" | "filter" => {
                check_keys(
                    item.table,
                    &[
                        "id",
                        "kind",
                        "body",
                        "material",
                        "materials",
                        "coating",
                        "dof",
                    ],
                    &item.context,
                )?;
                let spec = match item.table.get("coating") {
                    None => CoatingSpec::Uncoated,
                    Some(c) => crate::coatings::parse(c, &item.context)?,
                };
                // A filter's curve is the whole filter's response: on one of its two flat
                // faces, the other ideal.
                let specs = match (&spec, kind) {
                    (CoatingSpec::Curve(_), "filter") => vec![spec.clone(), CoatingSpec::Ideal],
                    _ => vec![spec.clone(); 2],
                };
                let resolved = resolve_coatings(&specs, lookups, &item.context)?;
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
                    let face_coatings = if kind == "filter" {
                        let faces: Vec<_> = bodies[i].faces.iter().map(|f| &*f.geometry).collect();
                        let plate = profile(&faces)
                            .filter(|p| p.surfaces.len() == 2 && p.surfaces.iter().all(|s| s.1.is_infinite()))
                            .ok_or_else(|| {
                                error(format!(
                                    "{}: body `{}` is not a plate with two flat faces across its axis",
                                    item.context, bodies[i].name
                                ))
                            })?;
                        plate
                            .surface_faces
                            .iter()
                            .zip(&resolved)
                            .flat_map(|(faces, c)| faces.iter().map(move |&f| (f, c.clone())))
                            .collect()
                    } else {
                        Vec::new()
                    };
                    scene.solids.push(Solid {
                        id: solid_id(i),
                        faces: bodies[i].faces.clone(),
                        occurrence: bodies[i].occurrence(),
                        role: SolidRole::Lens {
                            material: medium(value, lookup, &item.context)?,
                            coating: if kind == "filter" {
                                Coating::Uncoated
                            } else {
                                resolved[0].clone()
                            },
                            face_coatings,
                        },
                    });
                }
            }
            "mirror" => {
                check_keys(
                    item.table,
                    &["id", "kind", "body", "reflect", "dof"],
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
                        occurrence: bodies[i].occurrence(),
                        role: SolidRole::Mirror { reflecting },
                    });
                }
            }
            other => {
                return Err(error(format!(
                    "{}: element kind `{other}` is not known; use \"lens\", \"filter\" or \"mirror\"",
                    item.context
                )));
            }
        }
        scene.dofs.extend(element_dofs(&item, &bodies, &found)?);
        if let Some((path, frame, centre)) =
            element_occurrence(&item, &bodies, &found).unwrap_or(None)
        {
            cad.push(crate::tolerance::CadElement {
                id: item.text("id")?.to_owned(),
                occurrence: path,
                frame,
                centre,
            });
        }
    }

    // Products named after a library part: their glass bodies become lenses.
    if automatic && !library.parts().is_empty() {
        let mut instances: Vec<((String, String), Vec<usize>)> = Vec::new();
        for (i, body) in bodies.iter().enumerate() {
            if tagged[i] || claimed[i] {
                continue;
            }
            let key = (body.path.clone(), body.product.clone());
            match instances.iter_mut().find(|(k, _)| *k == key) {
                Some((_, members)) => members.push(i),
                None => instances.push((key, vec![i])),
            }
        }
        for ((path, product), members) in instances {
            let Some(part) = library
                .for_product(&product)
                .filter(|p| p.part_type.transmits())
            else {
                continue;
            };
            let id = if path.is_empty() {
                product.clone()
            } else {
                path.clone()
            };
            if let Some(matched) = add_part(
                &mut scene,
                &mut tagged,
                &bodies,
                &members,
                part,
                &id,
                true,
                lookups,
                &format!("product `{product}`"),
            )? {
                parts.push(matched);
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
            occurrence: body.occurrence(),
            role: SolidRole::Absorber,
        });
    }

    let position = ["position", "axis", "frame"];
    for item in items(&root, "source")? {
        let frame = placement_frame(&item, &bodies)?;
        scene
            .sources
            .push(crate::scene_file::source(&item, &position, frame)?);
        let target = Target::Source(item.text("id")?.to_owned());
        scene
            .dofs
            .extend(dofs(&item, &target, &frame, false, frame.origin())?);
    }
    for item in items(&root, "detector")? {
        check_keys(
            item.table,
            &with(
                &position,
                &[
                    "id",
                    "size",
                    "pixels",
                    "quantum_efficiency",
                    "dof",
                    "returning",
                ],
            ),
            &item.context,
        )?;
        let size = item.pair("size")?;
        if !(size.0 > 0.0 && size.1 > 0.0) {
            return Err(error(format!("{}: `size` must be positive", item.context)));
        }
        let frame = placement_frame(&item, &bodies)?;
        scene.detectors.push(Detector {
            id: item.text("id")?.to_owned(),
            frame,
            size,
            pixels: item.count_pair("pixels")?,
            quantum_efficiency: crate::scene_file::quantum_efficiency(&item)?,
            returning: crate::scene_file::returning(&item)?,
        });
        let target = Target::Detector(item.text("id")?.to_owned());
        scene
            .dofs
            .extend(dofs(&item, &target, &frame, false, frame.origin())?);
    }
    scene.fluorophores = crate::scene_file::fluorophores(&root, &scene)?;
    crate::scene_file::check_dofs(&scene)?;
    scene.tolerances = crate::tolerance::tolerances(&root, &scene, &cad)?;
    let scene = scene.placed().map_err(|e| error(e.to_string()))?;
    Ok(Read { scene, parts })
}

/// The `kind` of an `.optobench` file, from its second line.
pub fn kind_of(text: &str) -> Option<String> {
    let table: Table = text.parse().ok()?;
    table.get("kind")?.as_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use optobench_library::{Glass, Outline, PartSurface, PartType};
    use optobench_scene::SolidFace;
    use optobench_surfaces::brep::BrepFace;

    use super::*;

    fn frame(origin: Vec3, z: Vec3) -> Frame {
        Frame::new(origin, z, Vec3::X).expect("valid frame")
    }

    fn circle(r: f64, z: f64) -> Vec<Vec3> {
        (0..12)
            .map(|i| {
                let a = core::f64::consts::TAU * i as f64 / 12.0;
                Vec3::new(r * a.cos(), r * a.sin(), z)
            })
            .collect()
    }

    /// A body of a cemented doublet along +z, its rim cylinder's axis along `rim_axis`.
    fn body(name: &str, caps: [(f64, f64); 2], rim_axis: Vec3) -> PlacedBody {
        let mut faces = vec![BrepFace::new(
            FaceSurface::Cylinder {
                frame: frame(Vec3::ZERO, rim_axis),
                radius: 12.7,
            },
            false,
            vec![],
        )];
        for (vertex, radius) in caps {
            let sag = radius.abs() - (radius * radius - 12.7 * 12.7).sqrt();
            faces.push(BrepFace::new(
                FaceSurface::Sphere {
                    frame: frame(Vec3::new(0.0, 0.0, vertex + radius), Vec3::Z),
                    radius: radius.abs(),
                },
                false,
                vec![circle(12.7, vertex + sag * radius.signum())],
            ));
        }
        PlacedBody {
            path: "doublet:1".into(),
            product: "ac-test".into(),
            name: name.into(),
            index: 0,
            fingerprint: String::new(),
            placement: Transform::IDENTITY,
            faces: faces
                .into_iter()
                .map(|f| SolidFace {
                    geometry: Arc::new(f),
                    placement: Transform::IDENTITY,
                })
                .collect(),
        }
    }

    #[test]
    fn doublet_glasses_are_found_whatever_way_each_rim_points() {
        // R 40, −25, −200 with 8 and 2.5 mm, as a catalogue would list it; the two bodies'
        // rim cylinders point opposite ways, as in vendor STEP files.
        let part = Part {
            vendor: "Example".into(),
            number: "AC-TEST".into(),
            part_type: PartType::Lens,
            source: "test".into(),
            outline: Some(Outline::Round { diameter: 25.4 }),
            clear_aperture: None,
            surfaces: [40.0, -25.0, -200.0]
                .map(|radius| PartSurface { radius, conic: 0.0 })
                .to_vec(),
            thickness: vec![8.0, 2.5],
            glass: vec![Glass::Index(1.6), Glass::Index(1.7)],
            coating: None,
            coatings: vec![],
            products: vec![],
            adjusters: Vec::new(),
        };
        let crown = body("crown", [(0.0, 40.0), (8.0, -25.0)], Vec3::Z);
        let flint = body("flint", [(8.0, -25.0), (10.5, -200.0)], -Vec3::Z);
        let mount = PlacedBody {
            name: "mount".into(),
            faces: vec![],
            ..crown.clone()
        };
        let found = glass_bodies(&part, &[&mount, &flint, &crown]);
        let glasses: Vec<(usize, usize)> = found.iter().map(|(i, b)| (*i, b.glass)).collect();
        assert_eq!(glasses, vec![(2, 0), (1, 1)]);
        assert!(found.iter().all(|(_, b)| {
            b.comparison
                .as_ref()
                .is_some_and(|c| c.differences.is_empty())
        }));
    }
}
