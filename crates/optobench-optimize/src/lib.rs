//! Damped least squares over degrees of freedom, merit functions and tolerancing.
//!
//! Layer L4. See `docs/design.md`, section Architecture, and the M6 milestone.
//!
//! - [`lm`]: the Levenberg–Marquardt method with bounds, on any residuals.
//! - [`Term`] and [`Merit`]: what alignment means, as residuals from a traced scene:
//!   focus, centring, pointing, collimation, power.
//! - [`optimise`]: the free DOFs of a scene (neither locked nor coupled) that minimise a
//!   merit.

#![forbid(unsafe_code)]

pub mod lm;

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
    /// As much of the launched power as possible arriving directly (residual: the
    /// fraction that does not).
    Power,
}

impl Term {
    /// Its name in files and reports.
    pub fn name(&self) -> &'static str {
        match self {
            Term::Focus => "focus",
            Term::Centring { .. } => "centring",
            Term::Pointing { .. } => "pointing",
            Term::Collimation => "collimation",
            Term::Power => "power",
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

/// The residuals of `merit` for a traced scene, term by term (unweighted). A detector the
/// direct light misses is an error: there is nothing to align.
pub fn residuals(merit: &[Merit], traced: &SceneTrace) -> Result<Vec<Vec<f64>>, String> {
    merit
        .iter()
        .map(|m| {
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

/// The default weight of the smallest-moves preference (see [`optimise`]).
pub const MINIMAL_MOVES: f64 = 1e-3;

/// The values of the scene's free DOFs (neither locked nor coupled) that minimise the
/// weighted merit, starting from the scene's own values, within the DOFs' ranges.
///
/// When several settings meet the merit equally well (more DOFs than the merit pins
/// down, such as a lens shift and a mirror tilt that both steer the beam), the smallest
/// moves are preferred: a first solve adds, per DOF, the residual `minimal_moves` × its
/// change (mm for slides, degrees for tilts; Tikhonov regularisation), which picks the
/// smallest-move point of such a valley; a second solve without it polishes from there,
/// so DOFs the merit does fix are not biased. 0 switches the first solve off.
pub fn optimise(
    scene: &Scene,
    merit: &[Merit],
    options: &lm::Options,
    minimal_moves: f64,
) -> Result<Optimised, String> {
    if merit.is_empty() {
        return Err("no merit terms: say what alignment should achieve".into());
    }
    let free: Vec<&optobench_scene::dof::Dof> = scene
        .dofs
        .iter()
        .filter(|d| !d.locked && d.follows.is_none())
        .collect();
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
        residuals(merit, &traced)
    };
    let weighted = |x: &[f64], minimal_moves: f64| -> Result<Vec<f64>, String> {
        let mut r: Vec<f64> = evaluate(x)?
            .iter()
            .zip(merit)
            .flat_map(|(r, m)| r.iter().map(move |v| v * m.weight))
            .collect();
        if minimal_moves > 0.0 {
            r.extend(free.iter().zip(x).map(|(d, &v)| {
                let change = v - d.value;
                minimal_moves
                    * if d.motion.is_angle() {
                        change.to_degrees()
                    } else {
                        change
                    }
            }));
        }
        Ok(r)
    };
    let start: Vec<f64> = variables.iter().map(|v| v.start).collect();
    let before = evaluate(&start)?;
    // First with the smallest-moves preference (to choose among equally good settings),
    // then without it from there, so DOFs the merit does fix end where it puts them.
    let outcome = if minimal_moves > 0.0 {
        let first = lm::minimise(|x: &[f64]| weighted(x, minimal_moves), &variables, options)?;
        let polish: Vec<lm::Variable> = variables
            .iter()
            .zip(&first.x)
            .map(|(v, &x)| lm::Variable { start: x, ..*v })
            .collect();
        let mut second = lm::minimise(|x: &[f64]| weighted(x, 0.0), &polish, options)?;
        second.start_cost = weighted(&start, 0.0).map(|r| r.iter().map(|v| v * v).sum())?;
        second.iterations += first.iterations;
        second.evaluations += first.evaluations;
        second
    } else {
        lm::minimise(|x: &[f64]| weighted(x, 0.0), &variables, options)?
    };
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
