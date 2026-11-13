//! Tolerances in layouts and sidecars (M14): `[[tolerance]]` tables and the
//! `[tolerancing]` table.
//!
//! ```toml
//! [[tolerance]]
//! name = "L1_decentre_x"
//! kind = "slide"                    # or "tilt" (degrees), "dof", "radius", "thickness",
//!                                   # "index", "focal_length"
//! target = "L1"                     # an element, source or detector; a sidecar element of
//!                                   # CAD bodies moves its occurrence (as its DOFs), or
//!                                   # name one by `occurrence = "<path>"`
//! axis = "x"                        # slide and tilt: "x", "y", "z" or [x, y, z], in the
//! frame = "local"                   # target's own frame at its nominal pose or "scene";
//!                                   # default local, for CAD elements the assembly frame
//! pivot = [0, 0, 0]                 # tilt: default the target's origin (CAD elements: the
//!                                   # centre of their bodies; occurrences: required)
//! limit = 0.05                      # mm, degrees, a fraction (relative) or Δn
//! distribution = "uniform"          # or "normal" (σ = limit / 3, truncated), "end" (±limit)
//!
//! # kind = "dof":          dof = "L3_z" (an error added to that DOF; degrees for a tilt)
//! # kind = "radius":       surface = 1 (from 1), relative = false (true: limit a fraction)
//! # kind = "thickness":    gap = 1 (between surfaces 1 and 2)
//! # kind = "index":        gap = 1 (that gap's material, Δn)
//! # kind = "focal_length": an ideal lens; relative = false
//!
//! [tolerancing]
//! compensators = ["D_z"]            # DOFs re-optimised on the design's [[merit]] per trial
//! criteria = [ { kind = "focus", detector = "screen", max = 0.01 } ]
//!                                   # merit terms (target for centring and pointing, as in
//!                                   # [[merit]]) with the largest magnitude that passes:
//!                                   # mm (focus, centring), mrad (pointing, collimation),
//!                                   # a fraction (power)
//! ```

use optobench_math::{Frame, Vec3};
use optobench_optimize::tolerance::{Criterion, Tolerancing};
use optobench_scene::dof::{Motion, Target};
use optobench_scene::tolerance::{Distribution, Tolerance, ToleranceKind};
use optobench_scene::{ElementKind, Scene};
use toml::{Table, Value};

use crate::scene_file::{FileError, Item, check_keys, error, items};

/// A sidecar element made of CAD bodies: its id, its occurrence, the frame of its first
/// body and the centre of its bodies (assembly frame), as its DOFs use them.
#[derive(Debug, Clone)]
pub(crate) struct CadElement {
    pub(crate) id: String,
    pub(crate) occurrence: String,
    pub(crate) frame: Frame,
    pub(crate) centre: Vec3,
}

/// The `[[tolerance]]` tables, checked against the scene (at its nominal poses); `cad`
/// lists a sidecar's elements made of CAD bodies, which a `target` may name like its
/// occurrence (axes in the assembly frame by default, tilts about the bodies' centre).
pub(crate) fn tolerances(
    root: &Table,
    scene: &Scene,
    cad: &[CadElement],
) -> Result<Vec<Tolerance>, FileError> {
    let mut out: Vec<Tolerance> = Vec::new();
    for item in items(root, "tolerance")? {
        check_keys(
            item.table,
            &[
                "name",
                "kind",
                "target",
                "occurrence",
                "axis",
                "frame",
                "pivot",
                "dof",
                "surface",
                "gap",
                "relative",
                "limit",
                "distribution",
            ],
            &item.context,
        )?;
        let context = &item.context;
        let name = item.text("name")?.to_owned();
        if out.iter().any(|t| t.name == name) {
            return Err(error(format!("{context}: a second tolerance `{name}`")));
        }
        let kind = item.text("kind")?;
        let allowed: &[&str] = match kind {
            "slide" => &["target", "occurrence", "axis", "frame"],
            "tilt" => &["target", "occurrence", "axis", "frame", "pivot"],
            "dof" => &["dof"],
            "radius" => &["target", "surface", "relative"],
            "thickness" | "index" => &["target", "gap"],
            "focal_length" => &["target", "relative"],
            other => {
                return Err(error(format!(
                    "{context}: kind `{other}` is not known; use \"slide\", \"tilt\", \"dof\", \"radius\", \"thickness\", \"index\" or \"focal_length\""
                )));
            }
        };
        for key in [
            "target",
            "occurrence",
            "axis",
            "frame",
            "pivot",
            "dof",
            "surface",
            "gap",
            "relative",
        ] {
            if item.table.contains_key(key) && !allowed.contains(&key) {
                return Err(error(format!(
                    "{context}: a `{kind}` tolerance has no `{key}`"
                )));
            }
        }
        let limit = item.positive("limit")?;
        let distribution = match item.table.get("distribution").map(|v| v.as_str()) {
            None | Some(Some("uniform")) => Distribution::Uniform,
            Some(Some("normal")) => Distribution::Normal,
            Some(Some("end")) => Distribution::End,
            _ => {
                return Err(error(format!(
                    "{context}: `distribution` is \"uniform\" (default), \"normal\" or \"end\""
                )));
            }
        };
        let relative = match item.table.get("relative") {
            None => false,
            Some(Value::Boolean(b)) => *b,
            Some(_) => return Err(error(format!("{context}: `relative` is true or false"))),
        };
        let index = |key: &str| -> Result<usize, FileError> {
            let n = item.number(key)?;
            if n >= 1.0 && n.fract() == 0.0 {
                Ok(n as usize - 1)
            } else {
                Err(error(format!(
                    "{context}: `{key}` is a whole number from 1"
                )))
            }
        };
        // The target, its frame at the nominal pose (None: the scene frame only), whether
        // axes default to the scene frame, and its default pivot.
        let mut scene_default = false;
        let mut default_pivot: Option<Vec3> = None;
        let (target, frame): (Target, Option<Frame>) = if kind == "dof" {
            let dof = item.text("dof")?;
            if !scene.dofs.iter().any(|d| d.name == dof) {
                return Err(error(format!("{context}: no degree of freedom `{dof}`")));
            }
            (Target::Element(String::new()), None)
        } else if let Some(Value::String(path)) = item.table.get("occurrence") {
            if item.table.contains_key("target") {
                return Err(error(format!(
                    "{context}: give `target` or `occurrence`, not both"
                )));
            }
            if !matches!(kind, "slide" | "tilt") {
                return Err(error(format!(
                    "{context}: a CAD occurrence takes slide and tilt tolerances only"
                )));
            }
            (Target::Occurrence(path.clone()), None)
        } else {
            let id = item.text("target")?;
            if let Some(e) = scene.elements.iter().find(|e| e.id == id) {
                (Target::Element(id.to_owned()), Some(e.frame))
            } else if let Some(s) = scene.sources.iter().find(|s| s.id == id) {
                (Target::Source(id.to_owned()), Some(s.frame))
            } else if let Some(d) = scene.detectors.iter().find(|d| d.id == id) {
                (Target::Detector(id.to_owned()), Some(d.frame))
            } else if let Some(c) = cad.iter().find(|c| c.id == id) {
                if !matches!(kind, "slide" | "tilt") {
                    return Err(error(format!(
                        "{context}: `{id}` is made of CAD bodies: it takes slide and tilt tolerances only"
                    )));
                }
                scene_default = true;
                default_pivot = Some(c.centre);
                (Target::Occurrence(c.occurrence.clone()), Some(c.frame))
            } else {
                return Err(error(format!(
                    "{context}: no element, source or detector `{id}` (a CAD part is named by `occurrence`)"
                )));
            }
        };
        let element = |id: &Target| match id {
            Target::Element(id) => scene.elements.iter().find(|e| &e.id == id),
            _ => None,
        };
        let kind = match kind {
            "slide" | "tilt" => {
                let in_scene = match item.table.get("frame").map(|f| f.as_str()) {
                    None => frame.is_none() || scene_default,
                    Some(Some("scene")) => true,
                    Some(Some("local")) if frame.is_some() => false,
                    _ => {
                        return Err(error(format!(
                            "{context}: `frame` is \"local\" (the target's own; not for occurrences) or \"scene\""
                        )));
                    }
                };
                let axis = match item.table.get("axis") {
                    Some(Value::String(a)) => match a.as_str() {
                        "x" => Vec3::X,
                        "y" => Vec3::Y,
                        "z" => Vec3::Z,
                        _ => {
                            return Err(error(format!(
                                "{context}: `axis` is \"x\", \"y\", \"z\" or [x, y, z]"
                            )));
                        }
                    },
                    Some(_) => item.vector("axis")?,
                    None => return Err(error(format!("{context}: missing `axis`"))),
                };
                let axis = match (in_scene, frame) {
                    (false, Some(f)) => f.to_parent_vector(axis),
                    _ => axis,
                }
                .normalized()
                .ok_or_else(|| error(format!("{context}: `axis` has zero length")))?;
                if kind == "slide" {
                    ToleranceKind::Placement(Motion::Slide { axis })
                } else {
                    let pivot = match (item.table.contains_key("pivot"), frame) {
                        (true, Some(f)) if !in_scene => f.to_parent_point(item.vector("pivot")?),
                        (true, _) => item.vector("pivot")?,
                        (false, Some(f)) => default_pivot.unwrap_or_else(|| f.origin()),
                        (false, None) => {
                            return Err(error(format!(
                                "{context}: a tilt of a CAD occurrence needs its `pivot` (scene frame)"
                            )));
                        }
                    };
                    ToleranceKind::Placement(Motion::Tilt { axis, pivot })
                }
            }
            "dof" => ToleranceKind::Dof(item.text("dof")?.to_owned()),
            "radius" | "thickness" | "index" => {
                let Some(ElementKind::Lens {
                    surfaces,
                    thicknesses,
                    ..
                }) = element(&target).map(|e| &e.kind)
                else {
                    return Err(error(format!(
                        "{context}: a `{kind}` tolerance applies to a lens element"
                    )));
                };
                match kind {
                    "radius" => {
                        let surface = index("surface")?;
                        if surface >= surfaces.len() {
                            return Err(error(format!(
                                "{context}: the lens has {} surfaces",
                                surfaces.len()
                            )));
                        }
                        if !surfaces[surface].radius.is_finite() {
                            return Err(error(format!(
                                "{context}: surface {} is flat: it has no radius to change",
                                surface + 1
                            )));
                        }
                        ToleranceKind::Radius { surface, relative }
                    }
                    _ => {
                        let gap = index("gap")?;
                        if gap >= thicknesses.len() {
                            return Err(error(format!(
                                "{context}: the lens has {} gaps",
                                thicknesses.len()
                            )));
                        }
                        if kind == "thickness" {
                            ToleranceKind::Thickness { gap }
                        } else {
                            ToleranceKind::Index { gap }
                        }
                    }
                }
            }
            _ => {
                if !matches!(
                    element(&target).map(|e| &e.kind),
                    Some(ElementKind::IdealLens { .. })
                ) {
                    return Err(error(format!(
                        "{context}: a `focal_length` tolerance applies to an ideal lens"
                    )));
                }
                ToleranceKind::FocalLength { relative }
            }
        };
        let limit = match &kind {
            ToleranceKind::Placement(Motion::Tilt { .. }) => limit.to_radians(),
            ToleranceKind::Dof(name) => {
                let angle = scene
                    .dofs
                    .iter()
                    .find(|d| &d.name == name)
                    .is_some_and(|d| d.motion.is_angle());
                if angle { limit.to_radians() } else { limit }
            }
            _ => limit,
        };
        out.push(Tolerance {
            name,
            target,
            kind,
            limit,
            distribution,
        });
    }
    Ok(out)
}

/// The `[tolerancing]` table: criteria and compensators, checked against the scene's DOFs.
pub fn tolerancing(root: &Table, scene: &Scene) -> Result<Tolerancing, FileError> {
    let Some(value) = root.get("tolerancing") else {
        return Ok(Tolerancing::default());
    };
    let table = value
        .as_table()
        .ok_or_else(|| error("[tolerancing] must be a table"))?;
    check_keys(table, &["compensators", "criteria"], "[tolerancing]")?;
    let mut out = Tolerancing::default();
    if let Some(list) = table.get("compensators") {
        for v in list
            .as_array()
            .ok_or_else(|| error("[tolerancing]: `compensators` is a list of DOF names"))?
        {
            let name = v
                .as_str()
                .ok_or_else(|| error("[tolerancing]: `compensators` is a list of DOF names"))?;
            let dof =
                scene.dofs.iter().find(|d| d.name == name).ok_or_else(|| {
                    error(format!("[tolerancing]: no degree of freedom `{name}`"))
                })?;
            if dof.follows.is_some() {
                return Err(error(format!(
                    "[tolerancing]: `{name}` follows another DOF; name the one it follows"
                )));
            }
            out.compensators.push(name.to_owned());
        }
    }
    if let Some(list) = table.get("criteria") {
        for (i, v) in list
            .as_array()
            .ok_or_else(|| error("[tolerancing]: `criteria` is a list of { kind, detector, max }"))?
            .iter()
            .enumerate()
        {
            let context = format!("[tolerancing] criterion {}", i + 1);
            let mut criterion = v
                .as_table()
                .ok_or_else(|| error(format!("{context}: write {{ kind, detector, max }}")))?
                .clone();
            let max = criterion
                .remove("max")
                .ok_or_else(|| error(format!("{context}: missing `max`")))?;
            let max = max
                .as_float()
                .or_else(|| max.as_integer().map(|i| i as f64))
                .filter(|m| *m > 0.0 && m.is_finite())
                .ok_or_else(|| error(format!("{context}: `max` must be positive")))?;
            let merit = crate::merit::term(&Item {
                table: &criterion,
                context: context.clone(),
            })?;
            if merit.term == optobench_optimize::Term::Headroom {
                return Err(error(format!(
                    "{context}: a criterion is measured at a detector; headroom belongs in [[merit]]"
                )));
            }
            if !scene.detectors.iter().any(|d| d.id == merit.detector) {
                return Err(error(format!(
                    "{context}: no detector `{}`",
                    merit.detector
                )));
            }
            out.criteria.push(Criterion { merit, limit: max });
        }
    }
    Ok(out)
}

/// The `[tolerancing]` table of a file's text, for a scene read from it.
pub fn parse(text: &str, scene: &Scene) -> Result<Tolerancing, FileError> {
    let root: Table = text
        .parse()
        .map_err(|e: toml::de::Error| error(format!("not valid TOML: {e}")))?;
    tolerancing(&root, scene)
}
