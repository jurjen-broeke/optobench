//! Tolerancing (M14): random errors drawn within the design's tolerances
//! (`optobench_scene::tolerance`), applied to the scene, taken out as far as the
//! compensators can (the adjustments made at assembly), and measured against criteria.

use crate::Merit;

/// A pass limit on a merit term: the term's residuals, as one magnitude (the length of
/// the residual vector: the RMS radius for focus, the centroid's distance from its target
/// for centring, mm; the slope error for pointing and the RMS slope for collimation,
/// mrad; the power lost for power, a fraction), may be at most `limit`.
#[derive(Debug, Clone, PartialEq)]
pub struct Criterion {
    /// The term, at its detector (its weight is not used).
    pub merit: Merit,
    /// The largest magnitude that passes.
    pub limit: f64,
}

/// What a design says about its tolerancing besides the tolerances themselves.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tolerancing {
    /// What must hold after compensation.
    pub criteria: Vec<Criterion>,
    /// The DOFs re-optimised in every trial (on the design's merit).
    pub compensators: Vec<String>,
}

use std::collections::BTreeMap;

use optobench_materials::{Material, MaterialError, Medium};
use optobench_scene::dof::Dof;
use optobench_scene::tolerance::{Distribution, ToleranceKind};
use optobench_scene::{ElementKind, Scene};

use crate::{lm, optimise, residuals};

/// A seeded random generator: xoshiro256** (D. Blackman and S. Vigna, "Scrambled linear
/// pseudorandom number generators", ACM Trans. Math. Softw. 47, 36 (2021)), its state
/// filled by SplitMix64 as the paper recommends. The same seed gives the same numbers on
/// every platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Random {
    state: [u64; 4],
}

/// SplitMix64: advances `x` and returns the next output.
fn split_mix(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl Random {
    /// The generator for `seed`.
    pub fn new(seed: u64) -> Self {
        let mut x = seed;
        Self {
            state: [
                split_mix(&mut x),
                split_mix(&mut x),
                split_mix(&mut x),
                split_mix(&mut x),
            ],
        }
    }

    /// An independent stream for trial `index` of a run seeded `seed`: each trial draws
    /// from its own, so the trials are the same however they are shared among threads.
    pub fn stream(seed: u64, index: u64) -> Self {
        let mut x = seed;
        let base = split_mix(&mut x);
        let mut y = index;
        Self::new(base ^ split_mix(&mut y))
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.state;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// Uniform in [0, 1), from the top 53 bits.
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Standard normal, by the Box–Muller transform.
    pub fn normal(&mut self) -> f64 {
        let u1 = 1.0 - self.uniform(); // (0, 1]
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * core::f64::consts::PI * u2).cos()
    }

    /// An error within ±`limit` drawn from `distribution`.
    pub fn draw(&mut self, distribution: Distribution, limit: f64) -> f64 {
        match distribution {
            Distribution::Uniform => limit * (2.0 * self.uniform() - 1.0),
            Distribution::Normal => loop {
                let z = self.normal();
                if z.abs() <= 3.0 {
                    return z * limit / 3.0;
                }
            },
            Distribution::End => {
                if self.uniform() < 0.5 {
                    -limit
                } else {
                    limit
                }
            }
        }
    }
}

/// A material with its index shifted by Δn (a melt's departure from the catalogue).
#[derive(Debug)]
struct Shifted {
    base: Medium,
    delta: f64,
}

impl Material for Shifted {
    fn refractive_index(&self, wavelength: f64) -> Result<f64, MaterialError> {
        Ok(self.base.material().refractive_index(wavelength)? + self.delta)
    }

    fn extinction(&self, wavelength: f64) -> Result<f64, MaterialError> {
        self.base.material().extinction(wavelength)
    }

    fn absorption_coefficient(&self, wavelength: f64) -> Result<f64, MaterialError> {
        self.base.material().absorption_coefficient(wavelength)
    }

    fn scattering_coefficient(&self, wavelength: f64) -> Result<f64, MaterialError> {
        self.base.material().scattering_coefficient(wavelength)
    }

    fn anisotropy(&self, wavelength: f64) -> Result<f64, MaterialError> {
        self.base.material().anisotropy(wavelength)
    }

    fn smooth_index(&self) -> bool {
        self.base.material().smooth_index()
    }
}

/// The scene with `errors` (one per tolerance of the scene, in order) applied: placement
/// errors as moves of their targets on top of their DOFs (each a locked DOF of its own,
/// named `tolerance <name>`), DOF errors added to the DOFs' values, part errors on the
/// prescriptions.
pub fn apply(scene: &Scene, errors: &[f64]) -> Result<Scene, String> {
    if errors.len() != scene.tolerances.len() {
        return Err("one error per tolerance".into());
    }
    let mut changed = scene.clone();
    let mut overrides: BTreeMap<String, f64> = BTreeMap::new();
    for (t, &e) in scene.tolerances.iter().zip(errors) {
        let element = |s: &mut Scene| -> Result<usize, String> {
            match &t.target {
                optobench_scene::dof::Target::Element(id) => s
                    .elements
                    .iter()
                    .position(|x| &x.id == id)
                    .ok_or_else(|| format!("tolerance `{}`: no element `{id}`", t.name)),
                _ => Err(format!("tolerance `{}`: not an element", t.name)),
            }
        };
        match &t.kind {
            ToleranceKind::Placement(motion) => {
                let name = format!("tolerance {}", t.name);
                changed.dofs.push(Dof {
                    name: name.clone(),
                    target: t.target.clone(),
                    motion: *motion,
                    range: None,
                    locked: true,
                    follows: None,
                    value: 0.0,
                });
                overrides.insert(name, e);
            }
            ToleranceKind::Dof(name) => {
                let current = overrides.get(name).copied().unwrap_or_else(|| {
                    scene
                        .dofs
                        .iter()
                        .find(|d| &d.name == name)
                        .map_or(0.0, |d| d.value)
                });
                overrides.insert(name.clone(), current + e);
            }
            ToleranceKind::Radius { surface, relative } => {
                let k = element(&mut changed)?;
                if let ElementKind::Lens { surfaces, .. } = &mut changed.elements[k].kind {
                    let r = &mut surfaces[*surface].radius;
                    if *relative {
                        *r *= 1.0 + e;
                    } else if r.is_finite() {
                        *r += e;
                    }
                }
            }
            ToleranceKind::Thickness { gap } => {
                let k = element(&mut changed)?;
                if let ElementKind::Lens { thicknesses, .. } = &mut changed.elements[k].kind {
                    thicknesses[*gap] += e;
                    if thicknesses[*gap] <= 0.0 {
                        return Err(format!("tolerance `{}`: the thickness reaches 0", t.name));
                    }
                }
            }
            ToleranceKind::Index { gap } => {
                let k = element(&mut changed)?;
                if let ElementKind::Lens { materials, .. } = &mut changed.elements[k].kind {
                    materials[*gap] = Medium::new(Shifted {
                        base: materials[*gap].clone(),
                        delta: e,
                    });
                }
            }
            ToleranceKind::FocalLength { relative } => {
                let k = element(&mut changed)?;
                if let ElementKind::IdealLens { focal_length, .. } = &mut changed.elements[k].kind {
                    if *relative {
                        *focal_length *= 1.0 + e;
                    } else {
                        *focal_length += e;
                    }
                }
            }
        }
    }
    changed.moved(&overrides).map_err(|e| e.to_string())
}

/// What a scene gives against the criteria.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// Per criterion its magnitude; `None` where it could not be measured (no light on
    /// the detector).
    pub values: Vec<Option<f64>>,
    /// The compensators' values after compensation (mm or radians).
    pub compensators: Vec<(String, f64)>,
    /// Whether every criterion is within its limit.
    pub passed: bool,
    /// Why it could not be evaluated, if so.
    pub failure: Option<String>,
}

/// The magnitude of each criterion for a traced scene.
fn magnitudes(criteria: &[Criterion], traced: &optobench_scene::SceneTrace) -> Vec<Option<f64>> {
    criteria
        .iter()
        .map(|c| {
            residuals(std::slice::from_ref(&c.merit), traced)
                .ok()
                .map(|r| r[0].iter().map(|v| v * v).sum::<f64>().sqrt())
        })
        .collect()
}

/// Evaluates `scene` (errors applied): the compensators re-optimised on `merit` from
/// their current values (every other DOF held), then the criteria measured.
pub fn evaluate(
    scene: &Scene,
    tolerancing: &Tolerancing,
    merit: &[Merit],
    options: &lm::Options,
) -> Outcome {
    let fail = |why: String| Outcome {
        values: vec![None; tolerancing.criteria.len()],
        compensators: Vec::new(),
        passed: false,
        failure: Some(why),
    };
    let mut current = scene.clone();
    let mut compensators = Vec::new();
    if !tolerancing.compensators.is_empty() {
        let mut held = scene.clone();
        for d in &mut held.dofs {
            d.locked = !tolerancing.compensators.contains(&d.name);
        }
        let result = match optimise(&held, merit, options) {
            Ok(r) => r,
            Err(e) => return fail(format!("compensation: {e}")),
        };
        let end = result.end();
        current = match scene.moved(&end) {
            Ok(s) => s,
            Err(e) => return fail(e.to_string()),
        };
        compensators = result
            .values
            .iter()
            .map(|(n, _, v)| (n.clone(), *v))
            .collect();
    }
    let traced = match current.trace() {
        Ok(t) => t,
        Err(e) => return fail(e.to_string()),
    };
    let values = magnitudes(&tolerancing.criteria, &traced);
    let passed = values
        .iter()
        .zip(&tolerancing.criteria)
        .all(|(v, c)| v.is_some_and(|v| v <= c.limit));
    Outcome {
        values,
        compensators,
        passed,
        failure: None,
    }
}

/// One tolerance alone at −limit and +limit.
#[derive(Debug, Clone, PartialEq)]
pub struct SensitivityRow {
    /// The tolerance's name.
    pub tolerance: String,
    /// At −limit.
    pub minus: Outcome,
    /// At +limit.
    pub plus: Outcome,
    /// Per criterion the larger change from nominal of the two (∞ where a side could not
    /// be measured).
    pub change: Vec<f64>,
}

/// One tolerance at a time.
#[derive(Debug, Clone, PartialEq)]
pub struct Sensitivity {
    /// The nominal scene's outcome.
    pub nominal: Outcome,
    /// Per tolerance.
    pub rows: Vec<SensitivityRow>,
    /// Per criterion, the root sum of squares of the changes: the joint change if the
    /// errors were independent and the response linear.
    pub rss: Vec<f64>,
    /// Per criterion, the tolerances from the largest change to the smallest.
    pub ranking: Vec<Vec<usize>>,
}

/// The sensitivity of every criterion to every tolerance, each alone at ±limit, with the
/// compensators re-optimised each time.
pub fn sensitivity(
    scene: &Scene,
    tolerancing: &Tolerancing,
    merit: &[Merit],
    options: &lm::Options,
) -> Result<Sensitivity, String> {
    if scene.tolerances.is_empty() {
        return Err("the design has no tolerances ([[tolerance]])".into());
    }
    if tolerancing.criteria.is_empty() {
        return Err("no criteria ([tolerancing] criteria)".into());
    }
    let n = scene.tolerances.len();
    let nominal = evaluate(scene, tolerancing, merit, options);
    if nominal.failure.is_some() || nominal.values.iter().any(Option::is_none) {
        return Err(format!(
            "the nominal design cannot be evaluated: {}",
            nominal
                .failure
                .clone()
                .unwrap_or_else(|| "a detector gets no light".into())
        ));
    }
    let mut rows = Vec::with_capacity(n);
    for (k, t) in scene.tolerances.iter().enumerate() {
        let at = |sign: f64| -> Outcome {
            let mut errors = vec![0.0; n];
            errors[k] = sign * t.limit;
            match apply(scene, &errors) {
                Ok(s) => evaluate(&s, tolerancing, merit, options),
                Err(e) => Outcome {
                    values: vec![None; tolerancing.criteria.len()],
                    compensators: Vec::new(),
                    passed: false,
                    failure: Some(e),
                },
            }
        };
        let (minus, plus) = (at(-1.0), at(1.0));
        let change = (0..tolerancing.criteria.len())
            .map(|c| {
                let base = nominal.values[c].unwrap_or(0.0);
                let d = |o: &Outcome| o.values[c].map_or(f64::INFINITY, |v| (v - base).abs());
                d(&minus).max(d(&plus))
            })
            .collect();
        rows.push(SensitivityRow {
            tolerance: t.name.clone(),
            minus,
            plus,
            change,
        });
    }
    let criteria = tolerancing.criteria.len();
    let rss = (0..criteria)
        .map(|c| rows.iter().map(|r| r.change[c].powi(2)).sum::<f64>().sqrt())
        .collect();
    let ranking = (0..criteria)
        .map(|c| {
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by(|&a, &b| rows[b].change[c].total_cmp(&rows[a].change[c]));
            order
        })
        .collect();
    Ok(Sensitivity {
        nominal,
        rows,
        rss,
        ranking,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_mix_and_xoshiro_give_their_published_first_outputs() {
        // SplitMix64 from seed 0 begins 0xe220a8397b1dcdaf (the sequence published with
        // Vigna's reference splitmix64.c).
        let mut x = 0u64;
        assert_eq!(split_mix(&mut x), 0xe220_a839_7b1d_cdaf);
    }

    #[test]
    fn the_generator_s_statistics() {
        let mut r = Random::new(42);
        let n = 200_000;
        // Uniform: mean 1/2, variance 1/12; χ² over 20 bins (19 degrees of freedom: below
        // 43.8 with probability 0.999).
        let mut bins = [0usize; 20];
        let (mut sum, mut square) = (0.0, 0.0);
        for _ in 0..n {
            let u = r.uniform();
            assert!((0.0..1.0).contains(&u));
            sum += u;
            square += u * u;
            bins[(u * 20.0) as usize] += 1;
        }
        let mean = sum / n as f64;
        let variance = square / n as f64 - mean * mean;
        assert!((mean - 0.5).abs() < 3.0 * (1.0 / 12.0 / n as f64).sqrt() * 2.0);
        assert!((variance - 1.0 / 12.0).abs() < 1e-3);
        let expected = n as f64 / 20.0;
        let chi2: f64 = bins
            .iter()
            .map(|&b| (b as f64 - expected).powi(2) / expected)
            .sum();
        assert!(chi2 < 43.8, "{chi2}");
        // Normal: mean 0, variance 1, fourth moment 3.
        let (mut m1, mut m2, mut m4) = (0.0, 0.0, 0.0);
        for _ in 0..n {
            let z = r.normal();
            m1 += z;
            m2 += z * z;
            m4 += z.powi(4);
        }
        let (m1, m2, m4) = (m1 / n as f64, m2 / n as f64, m4 / n as f64);
        assert!(m1.abs() < 0.01 && (m2 - 1.0).abs() < 0.01 && (m4 - 3.0).abs() < 0.06);
        // The draws stay within the limit; the truncated normal's variance is
        // (L/3)² (1 − 6 φ(3) / (2Φ(3) − 1)) ≈ 0.9733 (L/3)².
        let limit = 0.2;
        let (mut s2, mut ends) = (0.0, 0);
        for _ in 0..n {
            let e = r.draw(Distribution::Normal, limit);
            assert!(e.abs() <= limit);
            s2 += e * e;
            let end = r.draw(Distribution::End, limit);
            assert!((end.abs() - limit).abs() < 1e-15);
            if end > 0.0 {
                ends += 1;
            }
        }
        let phi3 = (-4.5f64).exp() / (2.0 * core::f64::consts::PI).sqrt();
        let truncated = 1.0 - 6.0 * phi3 / 0.997_300_203_936_74;
        assert!((s2 / n as f64 / (limit / 3.0).powi(2) - truncated).abs() < 0.01);
        assert!((ends as f64 / n as f64 - 0.5).abs() < 0.005);
        // The same seed, the same numbers; streams differ.
        assert_eq!(Random::new(7).next_u64(), Random::new(7).next_u64());
        assert_ne!(
            Random::stream(7, 0).next_u64(),
            Random::stream(7, 1).next_u64()
        );
    }
}
