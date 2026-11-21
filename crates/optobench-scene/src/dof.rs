//! Degrees of freedom (docs/design.md, Coupling with SolidCraft, "The DOF model").
//!
//! A DOF moves one item of the scene from its nominal pose: a *slide* along an axis (mm)
//! or a *tilt* about an axis through a pivot (radians). An objective's correction
//! *collar* is a DOF too: it changes the cover-glass thickness the objective is corrected
//! for (mm), not a pose. Axes and pivots are fixed in the
//! scene frame at the nominal pose. A DOF may *follow* another (a coupled DOF: its value
//! is the other's times a ratio) or be *locked* (kept at its value, never optimised).
//! Poses are always the nominal pose plus the DOF offsets, never free transforms; an item
//! with several DOFs takes them in the order they are listed. A scene's poses always
//! match its DOFs' values: readers place items with [`Scene::placed`], and
//! [`Scene::moved`] moves them by the change in value.

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
    /// An objective's correction collar: the value is the change in the design
    /// cover-glass thickness it corrects for (mm). It moves nothing.
    Collar,
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
            Motion::Collar => Transform::IDENTITY,
        }
    }

    /// Whether the value is an angle.
    pub fn is_angle(&self) -> bool {
        matches!(self, Motion::Tilt { .. })
    }

    /// Its kind as written in files: `slide`, `tilt` or `collar`.
    pub fn kind(&self) -> &'static str {
        match self {
            Motion::Slide { .. } => "slide",
            Motion::Tilt { .. } => "tilt",
            Motion::Collar => "collar",
        }
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

/// What a DOF is in the CAD design, for writing its value there (Phase B, M13): a
/// parameter or a joint value that moves the part as the DOF does, or, without one, the
/// part itself moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    /// A user parameter in mm (a slide) or degrees (a tilt), changed by the DOF's change.
    Parameter(String),
    /// A joint's value, by the joint's name and, for a joint of several values, its
    /// index from 0.
    Joint {
        /// The joint.
        name: String,
        /// Which of its values; the first when `None`.
        index: Option<usize>,
    },
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
    /// A rough placement (M18, `range = "free"`): limited only by where its part would
    /// overlap another, which `clearance` finds; `range` is then what was found, if
    /// anything was.
    pub free: bool,
    /// Kept at `value`; never changed by the optimiser.
    pub locked: bool,
    /// Coupled: follows this DOF, times this ratio.
    pub follows: Option<(String, f64)>,
    /// The current value (mm or radians), relative to the nominal pose.
    pub value: f64,
    /// Where its range comes from (a mount's specification), if said.
    pub source: Option<String>,
    /// What it is in the CAD design, if bound (M13).
    pub binding: Option<Binding>,
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

    /// The scene with its DOFs at new values: `overrides` (by name) in place of the
    /// current ones, coupled DOFs following (see [`Scene::dof_values`]). Items move by the
    /// difference, so the poses keep matching the DOF values.
    pub fn moved(&self, overrides: &BTreeMap<String, f64>) -> Result<Scene, SceneError> {
        let old: BTreeMap<String, f64> = self
            .dofs
            .iter()
            .map(|d| (d.name.clone(), d.value))
            .collect();
        let new = self.dof_values(overrides)?;
        self.move_between(&old, &new)
    }

    /// The scene read from a file: poses nominal, DOF values given. Moves every item from
    /// its nominal pose to its DOF values, so the poses match them (readers call this
    /// once).
    pub fn placed(&self) -> Result<Scene, SceneError> {
        let zero: BTreeMap<String, f64> = self.dofs.iter().map(|d| (d.name.clone(), 0.0)).collect();
        let values = self.dof_values(&BTreeMap::new())?;
        self.move_between(&zero, &values)
    }

    /// How `target` moves (in the scene frame) when its DOFs go from the values `from` to
    /// `to`: the chain of its DOFs at the new values after undoing the chain at the old
    /// ones. Applied to the item's pose on the left (`t * pose`). A DOF missing from a map
    /// counts as 0.
    pub fn movement(
        &self,
        target: &Target,
        from: &BTreeMap<String, f64>,
        to: &BTreeMap<String, f64>,
    ) -> Transform {
        let chain = |values: &BTreeMap<String, f64>| {
            self.dofs
                .iter()
                .filter(|d| &d.target == target)
                .fold(Transform::IDENTITY, |t, d| {
                    d.motion
                        .transform(values.get(&d.name).copied().unwrap_or(0.0))
                        * t
                })
        };
        chain(to) * chain(from).inverse()
    }

    /// The items some DOF moves, each once, in the order their first DOF is listed.
    pub fn dof_targets(&self) -> Vec<Target> {
        let mut targets: Vec<Target> = Vec::new();
        for dof in &self.dofs {
            if !targets.contains(&dof.target) {
                targets.push(dof.target.clone());
            }
        }
        targets
    }

    /// Moves each item from where `old` puts it to where `new` does: by the chain of its
    /// DOFs at the new values after undoing the chain at the old ones.
    fn move_between(
        &self,
        old: &BTreeMap<String, f64>,
        new: &BTreeMap<String, f64>,
    ) -> Result<Scene, SceneError> {
        let mut scene = self.clone();
        let frame_moved =
            |frame: &Frame, t: &Transform| Frame::from_transform(*t * frame.transform());
        for target in &self.dof_targets() {
            let t = self.movement(target, old, new);
            let missing = || {
                invalid(format!(
                    "a degree of freedom moves {target:?}, which is not in the scene"
                ))
            };
            match target {
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
        // Collars: each changes its objective's design cover glass by the change in value.
        for dof in self
            .dofs
            .iter()
            .filter(|d| matches!(d.motion, Motion::Collar))
        {
            let change = new[&dof.name] - old.get(&dof.name).copied().unwrap_or(0.0);
            let Target::Element(id) = &dof.target else {
                return Err(invalid(format!(
                    "`{}`: a collar belongs to an objective",
                    dof.name
                )));
            };
            let element = scene
                .elements
                .iter_mut()
                .find(|e| &e.id == id)
                .ok_or_else(|| invalid(format!("`{}`: no element `{id}`", dof.name)))?;
            let crate::ElementKind::Objective { objective, .. } = &mut element.kind else {
                return Err(invalid(format!(
                    "`{}`: a collar belongs to an objective; `{id}` is not one",
                    dof.name
                )));
            };
            // The design layers are the immersion, then the cover glass.
            let Some(cover) = objective.design_layers.get_mut(1) else {
                return Err(invalid(format!(
                    "`{}`: objective `{id}` has no design cover glass for its collar to correct",
                    dof.name
                )));
            };
            cover.thickness += change;
            if cover.thickness <= 0.0 {
                return Err(invalid(format!(
                    "`{}`: the collar would correct for a cover glass of {:.4} mm",
                    dof.name, cover.thickness
                )));
            }
        }
        for dof in &mut scene.dofs {
            dof.value = new[&dof.name];
        }
        Ok(scene)
    }
}
