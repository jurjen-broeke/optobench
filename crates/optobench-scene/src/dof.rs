//! Degrees of freedom (docs/design.md, Coupling with SolidCraft, "The DOF model").
//!
//! A DOF moves one item of the scene from its nominal pose: a *slide* along an axis (mm)
//! or a *tilt* about an axis through a pivot (radians). Axes and pivots are fixed in the
//! scene frame at the nominal pose. A DOF may *follow* another (a coupled DOF: its value
//! is the other's times a ratio) or be *locked* (kept at its value, never optimised).
//! Poses are always the nominal pose plus the DOF offsets, never free transforms; an item
//! with several DOFs takes them in the order they are listed.

use std::collections::BTreeMap;

use optobench_math::{Frame, Rotation, Transform, Vec3};

use crate::{Scene, SceneError};

/// How a DOF moves its item, in the scene frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Motion {
    /// Along a unit axis; the value is in mm.
    Slide {
        /// The direction (unit).
        axis: Vec3,
    },
    /// About a unit axis through a pivot; the value is in radians (right-handed).
    Tilt {
        /// The rotation axis (unit).
        axis: Vec3,
        /// A point on the axis.
        pivot: Vec3,
    },
}

impl Motion {
    /// The move for `value` (mm or radians).
    pub fn transform(&self, value: f64) -> Transform {
        match *self {
            Motion::Slide { axis } => Transform::from_translation(axis * value),
            Motion::Tilt { axis, pivot } => {
                let rotation = Rotation::from_axis_angle(axis, value).unwrap_or(Rotation::IDENTITY);
                Transform::from_translation(pivot)
                    * Transform::new(rotation, Vec3::ZERO)
                    * Transform::from_translation(-pivot)
            }
        }
    }

    /// Whether the value is an angle.
    pub fn is_angle(&self) -> bool {
        matches!(self, Motion::Tilt { .. })
    }
}

/// What a DOF moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A sequential element, by id.
    Element(String),
    /// A source, by id.
    Source(String),
    /// A detector, by id.
    Detector(String),
    /// Every solid of a CAD occurrence (its assembly path): a part with its mount, as it
    /// is moved in CAD.
    Occurrence(String),
}

/// One degree of freedom.
#[derive(Debug, Clone, PartialEq)]
pub struct Dof {
    /// Its name, unique in the scene.
    pub name: String,
    /// What it moves.
    pub target: Target,
    /// How.
    pub motion: Motion,
    /// The mechanical range of the value (mm or radians), if limited.
    pub range: Option<(f64, f64)>,
    /// Kept at `value`; never changed by the optimiser.
    pub locked: bool,
    /// Coupled: follows this DOF, times this ratio.
    pub follows: Option<(String, f64)>,
    /// The current value (mm or radians), relative to the nominal pose.
    pub value: f64,
}

fn invalid(message: String) -> SceneError {
    SceneError::Invalid(message)
}

impl Scene {
    /// Every DOF's value: the scene's own, with `overrides` (by name) in place; coupled
    /// DOFs take theirs from the DOF they follow. An unknown name, an override of a
    /// coupled DOF, a coupling to an unknown or coupled DOF, or a value outside its range
    /// is an error.
    pub fn dof_values(
        &self,
        overrides: &BTreeMap<String, f64>,
    ) -> Result<BTreeMap<String, f64>, SceneError> {
        for name in overrides.keys() {
            let dof = self
                .dofs
                .iter()
                .find(|d| &d.name == name)
                .ok_or_else(|| invalid(format!("no degree of freedom `{name}`")))?;
            if let Some((leader, _)) = &dof.follows {
                return Err(invalid(format!(
                    "`{name}` follows `{leader}`; set `{leader}` instead"
                )));
            }
        }
        let mut values = BTreeMap::new();
        for dof in self.dofs.iter().filter(|d| d.follows.is_none()) {
            let value = overrides.get(&dof.name).copied().unwrap_or(dof.value);
            values.insert(dof.name.clone(), value);
        }
        for dof in &self.dofs {
            if let Some((leader, ratio)) = &dof.follows {
                let lead = values.get(leader).copied().ok_or_else(|| {
                    invalid(format!(
                        "`{}` follows `{leader}`, which is not a degree of freedom that leads",
                        dof.name
                    ))
                })?;
                values.insert(dof.name.clone(), lead * ratio);
            }
        }
        for dof in &self.dofs {
            let value = values[&dof.name];
            if let Some((low, high)) = dof.range {
                let slack = 1e-9 * (high - low).abs().max(1.0);
                if !(low - slack..=high + slack).contains(&value) {
                    let (unit, scale) = if dof.motion.is_angle() {
                        ("°", 180.0 / core::f64::consts::PI)
                    } else {
                        (" mm", 1.0)
                    };
                    return Err(invalid(format!(
                        "`{}` = {}{unit} is outside its range {} to {}{unit}",
                        dof.name,
                        value * scale,
                        low * scale,
                        high * scale
                    )));
                }
            }
        }
        Ok(values)
    }

    /// The scene with every item at its nominal pose plus its DOF offsets (see
    /// [`Scene::dof_values`]).
    pub fn moved(&self, overrides: &BTreeMap<String, f64>) -> Result<Scene, SceneError> {
        let values = self.dof_values(overrides)?;
        let mut scene = self.clone();
        let frame_moved =
            |frame: &Frame, t: &Transform| Frame::from_transform(*t * frame.transform());
        for dof in &self.dofs {
            let t = dof.motion.transform(values[&dof.name]);
            let missing = || {
                invalid(format!(
                    "`{}` moves {:?}, which is not in the scene",
                    dof.name, dof.target
                ))
            };
            match &dof.target {
                Target::Element(id) => {
                    let e = scene
                        .elements
                        .iter_mut()
                        .find(|e| &e.id == id)
                        .ok_or_else(missing)?;
                    e.frame = frame_moved(&e.frame, &t);
                }
                Target::Source(id) => {
                    let s = scene
                        .sources
                        .iter_mut()
                        .find(|s| &s.id == id)
                        .ok_or_else(missing)?;
                    s.frame = frame_moved(&s.frame, &t);
                }
                Target::Detector(id) => {
                    let d = scene
                        .detectors
                        .iter_mut()
                        .find(|d| &d.id == id)
                        .ok_or_else(missing)?;
                    d.frame = frame_moved(&d.frame, &t);
                }
                Target::Occurrence(path) => {
                    let mut found = false;
                    for solid in scene.solids.iter_mut().filter(|s| &s.occurrence == path) {
                        found = true;
                        for face in &mut solid.faces {
                            face.placement = t * face.placement;
                        }
                    }
                    if !found {
                        return Err(missing());
                    }
                }
            }
        }
        // The DOFs now say where the items are.
        for dof in &mut scene.dofs {
            dof.value = values[&dof.name];
        }
        Ok(scene)
    }
}
