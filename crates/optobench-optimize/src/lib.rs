//! Damped least squares over degrees of freedom, merit functions and tolerancing.
//!
//! Layer L4. See `docs/design.md`, section Architecture, and the M6 milestone.
//!
//! - [`lm`]: the Levenberg–Marquardt method with bounds, on any residuals.
//! - [`Term`] and [`Merit`]: what alignment means, as residuals from a traced scene:
//!   focus, centring, pointing, collimation, diameter, power.
//! - [`optimise`]: the free DOFs of a scene (neither locked nor coupled) that minimise a
//!   merit.

#![forbid(unsafe_code)]

pub mod lm;
pub mod tolerance;

use std::collections::BTreeMap;

use optobench_scene::{Scene, SceneTrace};

/// One thing alignment should achieve, at a detector.
#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    /// The direct light's RMS spot radius as small as possible (residual in mm).
    Focus,
    /// The direct light's centroid on `target` (detector x, y; mm).
    Centring {
        /// Where, in the detector's own frame (mm).
        target: (f64, f64),
    },
    /// The direct beam's mean direction at `target` slopes (dx/dz, dy/dz in the
    /// detector's frame; residual in mrad).
    Pointing {
        /// The wanted slopes (radians).
        target: (f64, f64),
    },
    /// The direct beam collimated: its RMS slope spread as small as possible (mrad).
    Collimation,
    /// The direct beam's diameter at `target` (mm): 2√2 times its power-weighted RMS
    /// radius, which is the edge diameter of an evenly filled disc and the 1/e² diameter
    /// of a Gaussian beam (residual in mm).
    Diameter {
        /// The wanted diameter (mm).
        target: f64,
    },
    /// As much of the launched power as possible arriving directly (residual: the
    /// fraction that does not).
    Power,
    /// Every free DOF with a range near the middle of it, leaving room to adjust both
    /// ways: one residual per such DOF, (value − middle) / half the range (0 in the
    /// middle, ±1 at the ends). Measured on the DOFs, not at a detector. With a small
    /// weight it decides only where the optics leave a choice (two mirrors steering one
    /// beam, a lens and its detector refocusing alike).
    Headroom,
}

impl Term {
    /// Its name in files and reports.
    pub fn name(&self) -> &'static str {
        match self {
            Term::Focus => "focus",
            Term::Centring { .. } => "centring",
            Term::Pointing { .. } => "pointing",
            Term::Collimation => "collimation",
            Term::Diameter { .. } => "diameter",
            Term::Power => "power",
            Term::Headroom => "headroom",
        }
    }
}

/// A weighted term at a detector.
#[derive(Debug, Clone, PartialEq)]
pub struct Merit {
    /// What.
    pub term: Term,
    /// At which detector (id).
    pub detector: String,
    /// Its weight: residuals are multiplied by it.
    pub weight: f64,
}

/// The headroom residuals of `scene`'s free DOFs (neither locked nor coupled) that have a
/// range, at `values` (by name; a DOF not in it at its own value), in scene order.
pub fn headroom(scene: &Scene, values: &BTreeMap<String, f64>) -> Vec<f64> {
    scene
        .dofs
        .iter()
        .filter(|d| !d.locked && d.follows.is_none())
        .filter_map(|d| {
            let (low, high) = d.range?;
            let half = 0.5 * (high - low);
            (half > 0.0).then(|| {
                let v = values.get(&d.name).copied().unwrap_or(d.value);
                (v - 0.5 * (low + high)) / half
            })
        })
        .collect()
}

/// A beam's diameter over its power-weighted RMS radius, 2√2: for an evenly filled disc
/// of radius a the mean of r² is a²/2; for a Gaussian of 1/e² radius w it is w²/2.
pub const DIAMETER_PER_RMS_RADIUS: f64 = 2.0 * std::f64::consts::SQRT_2;

/// The residuals of `merit` for a traced scene, term by term (unweighted). A detector the
/// direct light misses is an error: there is nothing to align. Headroom terms are
/// measured on the DOFs ([`headroom`]) and come out empty here.
pub fn residuals(merit: &[Merit], traced: &SceneTrace) -> Result<Vec<Vec<f64>>, String> {
    merit
        .iter()
        .map(|m| {
            if m.term == Term::Headroom {
                return Ok(Vec::new());
            }
            let d = traced
                .detectors
                .iter()
                .find(|d| d.id == m.detector)
                .ok_or_else(|| format!("no detector `{}`", m.detector))?;
            if d.direct_power <= 0.0 && m.term != Term::Power {
                return Err(format!("no direct light reaches detector `{}`", m.detector));
            }
            Ok(match &m.term {
                Term::Focus => vec![d.direct_rms_radius],
                Term::Centring { target } => vec![
                    d.direct_centroid.0 - target.0,
                    d.direct_centroid.1 - target.1,
                ],
                Term::Pointing { target } => vec![
                    1e3 * (d.direct_mean_slope.0 - target.0),
                    1e3 * (d.direct_mean_slope.1 - target.1),
                ],
                Term::Collimation => vec![1e3 * d.direct_rms_slope],
                Term::Diameter { target } => {
                    vec![DIAMETER_PER_RMS_RADIUS * d.direct_rms_radius - target]
                }
                Term::Headroom => Vec::new(),
                Term::Power => {
                    let launched = traced.ledger.launched;
                    vec![if launched > 0.0 {
                        1.0 - d.direct_power / launched
                    } else {
                        1.0
                    }]
                }
            })
        })
        .collect()
}

/// The result of an optimisation.
#[derive(Debug, Clone, PartialEq)]
pub struct Optimised {
    /// The free DOFs, in scene order, with their start and end values (mm or radians).
    pub values: Vec<(String, f64, f64)>,
    /// Each merit term's residuals (unweighted) at the start and at the end.
    pub terms: Vec<(Vec<f64>, Vec<f64>)>,
    /// The optimiser's account.
    pub outcome: lm::Outcome,
}

impl Optimised {
    /// The end values of the free DOFs, by name.
    pub fn end(&self) -> BTreeMap<String, f64> {
        self.values
            .iter()
            .map(|(n, _, v)| (n.clone(), *v))
            .collect()
    }
}

/// Finite-difference steps: 1 µm for slides, 1 µrad for tilts.
const SLIDE_STEP: f64 = 1e-3;
const TILT_STEP: f64 = 1e-6;

/// The values of the scene's free DOFs (neither locked nor coupled) that minimise the
/// weighted merit, starting from the scene's own values, within the DOFs' ranges.
pub fn optimise(
    scene: &Scene,
    merit: &[Merit],
    options: &lm::Options,
) -> Result<Optimised, String> {
    if merit.iter().all(|m| m.term == Term::Headroom) {
        return Err("no merit terms: say what alignment should achieve".into());
    }
    let free: Vec<&optobench_scene::dof::Dof> = scene
        .dofs
        .iter()
        .filter(|d| !d.locked && d.follows.is_none())
        .collect();
    if scene.dofs.is_empty() {
        return Err("nothing to move: give the items it may move a degree of freedom (a slide or tilt; in the interface: select the item, then Add under Degrees of freedom)".into());
    }
    if free.is_empty() {
        return Err("no free degrees of freedom (every DOF is locked or coupled)".into());
    }
    let variables: Vec<lm::Variable> = free
        .iter()
        .map(|d| lm::Variable {
            start: d.value,
            bounds: d.range,
            step: if d.motion.is_angle() {
                TILT_STEP
            } else {
                SLIDE_STEP
            },
        })
        .collect();
    let evaluate = |x: &[f64]| -> Result<Vec<Vec<f64>>, String> {
        let values: BTreeMap<String, f64> = free
            .iter()
            .zip(x)
            .map(|(d, &v)| (d.name.clone(), v))
            .collect();
        let moved = scene.moved(&values).map_err(|e| e.to_string())?;
        let traced = moved.trace().map_err(|e| e.to_string())?;
        let mut terms = residuals(merit, &traced)?;
        for (term, m) in terms.iter_mut().zip(merit) {
            if m.term == Term::Headroom {
                *term = headroom(scene, &values);
            }
        }
        Ok(terms)
    };
    let weighted = |x: &[f64]| -> Result<Vec<f64>, String> {
        Ok(evaluate(x)?
            .iter()
            .zip(merit)
            .flat_map(|(r, m)| r.iter().map(move |v| v * m.weight))
            .collect())
    };
    let start: Vec<f64> = variables.iter().map(|v| v.start).collect();
    let before = evaluate(&start)?;
    let outcome = lm::minimise(weighted, &variables, options)?;
    let after = evaluate(&outcome.x)?;
    Ok(Optimised {
        values: free
            .iter()
            .zip(&outcome.x)
            .map(|(d, &v)| (d.name.clone(), d.value, v))
            .collect(),
        terms: before.into_iter().zip(after).collect(),
        outcome,
    })
}
