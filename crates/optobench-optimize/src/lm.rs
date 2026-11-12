//! Damped least squares: the Levenberg–Marquardt method with bounds.
//!
//! Minimises the sum of squares of residuals r(x) over variables with optional bounds
//! (K. Levenberg, Q. Appl. Math. 2, 164 (1944); D. W. Marquardt, J. SIAM 11, 431
//! (1963)). Each step solves the damped linear problem
//! [J; √λ D] δ = [−r; 0] in the least-squares sense, with D the column norms of the
//! Jacobian J (Marquardt's scaling), so steps do not depend on the units of the
//! variables. J comes from central differences. A step is clamped to the bounds; it is
//! accepted when it lowers the cost (λ shrinks) and rejected otherwise (λ grows). A
//! trial point where the residuals cannot be evaluated counts as rejected. It stops when
//! an accepted step gains almost nothing, or is smaller than every variable's
//! finite-difference step (finer than the Jacobian resolves).

use optobench_math::least_squares;

/// One variable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Variable {
    /// Where to start.
    pub start: f64,
    /// Lower and upper bound, if any.
    pub bounds: Option<(f64, f64)>,
    /// Finite-difference step for the Jacobian.
    pub step: f64,
}

impl Variable {
    fn clamp(&self, x: f64) -> f64 {
        match self.bounds {
            Some((low, high)) => x.clamp(low, high),
            None => x,
        }
    }
}

/// When to stop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// At most this many accepted or rejected iterations.
    pub max_iterations: usize,
    /// Stop when an accepted step lowers the cost by less than this fraction.
    pub cost_tolerance: f64,
    /// Stop when the cost is below this (an exact fit).
    pub cost_floor: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            cost_tolerance: 1e-12,
            cost_floor: 1e-24,
        }
    }
}

/// The result.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The variables at the end.
    pub x: Vec<f64>,
    /// The sum of squared residuals at the start.
    pub start_cost: f64,
    /// The sum of squared residuals at the end.
    pub cost: f64,
    /// Iterations taken.
    pub iterations: usize,
    /// Times the residuals were evaluated.
    pub evaluations: usize,
    /// Whether it stopped because it converged (else: out of iterations).
    pub converged: bool,
    /// Which variables end at a bound.
    pub at_bound: Vec<bool>,
    /// The cost at the start and after each accepted step.
    pub history: Vec<f64>,
}

fn cost(r: &[f64]) -> f64 {
    r.iter().map(|v| v * v).sum()
}

/// Minimises Σ rᵢ(x)² over `variables`.
pub fn minimise<E>(
    mut residuals: impl FnMut(&[f64]) -> Result<Vec<f64>, E>,
    variables: &[Variable],
    options: &Options,
) -> Result<Outcome, E> {
    let n = variables.len();
    let mut x: Vec<f64> = variables.iter().map(|v| v.clamp(v.start)).collect();
    let mut r = residuals(&x)?;
    let mut evaluations = 1;
    let start_cost = cost(&r);
    let mut current = start_cost;
    let mut history = vec![start_cost];
    let mut lambda: f64 = 1e-3;
    let mut iterations = 0;
    let mut converged = n == 0 || current <= options.cost_floor;
    'outer: while !converged && iterations < options.max_iterations {
        // Jacobian by central differences (one-sided at a bound).
        let m = r.len();
        let mut jacobian = vec![vec![0.0; n]; m];
        for (j, variable) in variables.iter().enumerate() {
            let (mut plus, mut minus) = (x.clone(), x.clone());
            plus[j] = variable.clamp(x[j] + variable.step);
            minus[j] = variable.clamp(x[j] - variable.step);
            let span = plus[j] - minus[j];
            if span == 0.0 {
                continue;
            }
            let (rp, rm) = (residuals(&plus)?, residuals(&minus)?);
            evaluations += 2;
            for i in 0..m {
                jacobian[i][j] = (rp[i] - rm[i]) / span;
            }
        }
        let norms: Vec<f64> = (0..n)
            .map(|j| {
                jacobian
                    .iter()
                    .map(|row| row[j] * row[j])
                    .sum::<f64>()
                    .sqrt()
            })
            .collect();
        let largest = norms.iter().copied().fold(0.0, f64::max);
        if largest == 0.0 {
            converged = true; // nothing moves the residuals
            break;
        }
        let scale: Vec<f64> = norms.iter().map(|&d| d.max(1e-9 * largest)).collect();
        loop {
            iterations += 1;
            let damping = lambda.sqrt();
            let rows: Vec<Vec<f64>> = jacobian
                .iter()
                .cloned()
                .chain((0..n).map(|j| {
                    let mut row = vec![0.0; n];
                    row[j] = damping * scale[j];
                    row
                }))
                .collect();
            let rhs: Vec<f64> = r
                .iter()
                .map(|v| -v)
                .chain(core::iter::repeat_n(0.0, n))
                .collect();
            let Ok(delta) = least_squares(&rows, &rhs) else {
                lambda *= 10.0;
                if lambda > 1e16 || iterations >= options.max_iterations {
                    break 'outer;
                }
                continue;
            };
            let trial: Vec<f64> = variables
                .iter()
                .zip(&x)
                .zip(&delta)
                .map(|((v, &xi), &d)| v.clamp(xi + d))
                .collect();
            evaluations += 1;
            let trial_r = residuals(&trial).ok();
            match trial_r {
                Some(tr) if cost(&tr) < current => {
                    let new_cost = cost(&tr);
                    let gain = (current - new_cost) / current.max(f64::MIN_POSITIVE);
                    // A step below every variable's finite-difference step cannot be
                    // resolved: the minimum is found to that precision.
                    let resolved = trial
                        .iter()
                        .zip(&x)
                        .zip(variables)
                        .all(|((t, xi), v)| (t - xi).abs() <= v.step);
                    x = trial;
                    r = tr;
                    current = new_cost;
                    history.push(new_cost);
                    lambda = (lambda / 10.0).max(1e-15);
                    if gain < options.cost_tolerance || current <= options.cost_floor || resolved {
                        converged = true;
                    }
                    break;
                }
                _ => {
                    lambda *= 10.0;
                    if lambda > 1e16 {
                        // No step lowers the cost: a (possibly constrained) minimum.
                        converged = true;
                        break 'outer;
                    }
                    if iterations >= options.max_iterations {
                        break 'outer;
                    }
                }
            }
        }
    }
    let at_bound = variables
        .iter()
        .zip(&x)
        .map(|(v, &xi)| v.bounds.is_some_and(|(low, high)| xi <= low || xi >= high))
        .collect();
    Ok(Outcome {
        x,
        start_cost,
        cost: current,
        iterations,
        evaluations,
        converged,
        at_bound,
        history,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn free(start: f64) -> Variable {
        Variable {
            start,
            bounds: None,
            step: 1e-6,
        }
    }

    #[test]
    fn rosenbrock_reaches_its_minimum() {
        // Rosenbrock's function as residuals (10(y − x²), 1 − x): minimum 0 at (1, 1),
        // the classic test of a curved valley (H. H. Rosenbrock, Comput. J. 3, 175 (1960)).
        let outcome = minimise(
            |v: &[f64]| Ok::<_, ()>(vec![10.0 * (v[1] - v[0] * v[0]), 1.0 - v[0]]),
            &[free(-1.2), free(1.0)],
            &Options::default(),
        )
        .expect("runs");
        assert!(outcome.converged);
        assert!(
            (outcome.x[0] - 1.0).abs() < 1e-8 && (outcome.x[1] - 1.0).abs() < 1e-8,
            "{:?}",
            outcome.x
        );
        assert!(outcome.cost < 1e-16);
    }

    #[test]
    fn linear_fit_in_one_step_and_bounds_hold() {
        // Residuals linear in x: the solution x = (2, −3) whatever the units.
        let outcome = minimise(
            |v: &[f64]| Ok::<_, ()>(vec![v[0] - 2.0, 1e3 * (v[1] + 3.0)]),
            &[free(0.0), free(0.0)],
            &Options::default(),
        )
        .expect("runs");
        assert!((outcome.x[0] - 2.0).abs() < 1e-9 && (outcome.x[1] + 3.0).abs() < 1e-9);
        // With x₀ bounded to [−1, 1], it stops at the bound nearest 2.
        let bounded = Variable {
            bounds: Some((-1.0, 1.0)),
            ..free(0.0)
        };
        let outcome = minimise(
            |v: &[f64]| Ok::<_, ()>(vec![v[0] - 2.0]),
            &[bounded],
            &Options::default(),
        )
        .expect("runs");
        assert!((outcome.x[0] - 1.0).abs() < 1e-12 && outcome.at_bound[0] && outcome.converged);
    }

    #[test]
    fn failing_trial_points_are_rejected() {
        // The residual cannot be evaluated beyond x = 1.5; the minimum (x = 1) is reached
        // anyway.
        let outcome = minimise(
            |v: &[f64]| {
                if v[0] > 1.5 {
                    Err("no light")
                } else {
                    Ok(vec![v[0] - 1.0, 0.1 * (v[0] - 1.0)])
                }
            },
            &[free(-4.0)],
            &Options::default(),
        )
        .expect("runs");
        assert!((outcome.x[0] - 1.0).abs() < 1e-9);
    }
}
