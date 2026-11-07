//! Scalar root finding: Newton's method, and Newton safeguarded by bisection.
//!
//! Both take a closure returning `(f(x), f'(x))` and report how they converged, so
//! callers (asphere intersection, later the optimizer) can log or test it.
//!
//! The safeguarded method is the standard hybrid described in numerical-analysis texts
//! (for example Press et al., Numerical Recipes, 3rd ed., §9.4): take the Newton step
//! when it stays inside the current bracket and shrinks it fast enough, otherwise bisect.
//! Written here from that description.

/// A root that met the tolerance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Root {
    /// The root estimate.
    pub x: f64,
    /// f(x) at the returned estimate.
    pub residual: f64,
    /// Size of the last step; at most the tolerance when converged.
    pub last_step: f64,
    /// Number of function evaluations used, the final residual evaluation included.
    pub evaluations: u32,
}

/// Why root finding stopped without a root.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RootError {
    /// f has the same sign at both ends of the bracket.
    NoSignChange {
        /// f at the lower end.
        lower: f64,
        /// f at the upper end.
        upper: f64,
    },
    /// The derivative vanished where an unbracketed Newton step was needed.
    ZeroDerivative {
        /// Where it vanished.
        x: f64,
    },
    /// f or its derivative was not finite (NaN or infinite).
    NotFinite {
        /// Where it happened.
        x: f64,
    },
    /// The iteration limit was reached; `best` is the last estimate.
    NotConverged {
        /// The last estimate and its residual.
        best: Root,
    },
}

/// Newton's method from `start`. Converged when a step is at most `tolerance` (in units
/// of x) or f(x) is exactly zero. Fast but unguarded: use it when a good start is known,
/// as for an asphere starting from its base conic.
pub fn find_root(
    mut f: impl FnMut(f64) -> (f64, f64),
    start: f64,
    tolerance: f64,
    max_iterations: u32,
) -> Result<Root, RootError> {
    let mut x = start;
    let mut step = f64::INFINITY;
    for evaluations in 1..=max_iterations {
        let (value, slope) = f(x);
        if !value.is_finite() || !slope.is_finite() {
            return Err(RootError::NotFinite { x });
        }
        if value == 0.0 {
            return Ok(Root {
                x,
                residual: 0.0,
                last_step: if evaluations == 1 { 0.0 } else { step.abs() },
                evaluations,
            });
        }
        if slope == 0.0 {
            return Err(RootError::ZeroDerivative { x });
        }
        step = value / slope;
        x -= step;
        if step.abs() <= tolerance {
            let (residual, _) = f(x);
            return Ok(Root {
                x,
                residual,
                last_step: step.abs(),
                evaluations: evaluations + 1,
            });
        }
    }
    let (residual, _) = f(x);
    Err(RootError::NotConverged {
        best: Root {
            x,
            residual,
            last_step: step.abs(),
            evaluations: max_iterations + 1,
        },
    })
}

/// Newton's method kept inside the bracket `[lower, upper]`, falling back to bisection
/// when a Newton step would leave the bracket or shrink it too slowly. f must change
/// sign over the bracket. Always converges for a continuous f; converged when a step is
/// at most `tolerance` or f(x) is exactly zero.
pub fn find_root_bracketed(
    mut f: impl FnMut(f64) -> (f64, f64),
    lower: f64,
    upper: f64,
    tolerance: f64,
    max_iterations: u32,
) -> Result<Root, RootError> {
    let (f_lower, _) = f(lower);
    let (f_upper, _) = f(upper);
    if !f_lower.is_finite() {
        return Err(RootError::NotFinite { x: lower });
    }
    if !f_upper.is_finite() {
        return Err(RootError::NotFinite { x: upper });
    }
    for (x, value) in [(lower, f_lower), (upper, f_upper)] {
        if value == 0.0 {
            return Ok(Root {
                x,
                residual: 0.0,
                last_step: 0.0,
                evaluations: 2,
            });
        }
    }
    if (f_lower > 0.0) == (f_upper > 0.0) {
        return Err(RootError::NoSignChange {
            lower: f_lower,
            upper: f_upper,
        });
    }
    // Keep f(negative) < 0 < f(positive).
    let (mut negative, mut positive) = if f_lower < 0.0 {
        (lower, upper)
    } else {
        (upper, lower)
    };
    let mut x = 0.5 * (lower + upper);
    let mut step = (upper - lower).abs();
    let mut previous_step = step;
    let mut evaluations = 2;
    for _ in 0..max_iterations {
        let (value, slope) = f(x);
        evaluations += 1;
        if !value.is_finite() {
            return Err(RootError::NotFinite { x });
        }
        if value == 0.0 {
            return Ok(Root {
                x,
                residual: 0.0,
                last_step: step.abs(),
                evaluations,
            });
        }
        if value < 0.0 {
            negative = x;
        } else {
            positive = x;
        }
        let newton = x - value / slope;
        let (low, high) = (negative.min(positive), negative.max(positive));
        let newton_usable = slope.is_finite()
            && slope != 0.0
            && newton > low
            && newton < high
            && (2.0 * value).abs() <= (previous_step * slope).abs();
        previous_step = step;
        if newton_usable {
            step = x - newton;
            x = newton;
        } else {
            let middle = 0.5 * (negative + positive);
            step = x - middle;
            x = middle;
        }
        if step.abs() <= tolerance {
            let (residual, _) = f(x);
            return Ok(Root {
                x,
                residual,
                last_step: step.abs(),
                evaluations: evaluations + 1,
            });
        }
    }
    let (residual, _) = f(x);
    Err(RootError::NotConverged {
        best: Root {
            x,
            residual,
            last_step: step.abs(),
            evaluations: evaluations + 1,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference: √2 = 1.4142135623730951 (to f64 precision).
    const SQRT_2: f64 = core::f64::consts::SQRT_2;

    fn square_minus_two(x: f64) -> (f64, f64) {
        (x * x - 2.0, 2.0 * x)
    }

    #[test]
    fn newton_finds_square_root_of_two() {
        let root = find_root(square_minus_two, 1.0, 1e-14, 50).expect("converges");
        assert!((root.x - SQRT_2).abs() < 1e-14);
        assert!(root.residual.abs() < 1e-14);
        assert!(root.evaluations < 10);
    }

    #[test]
    fn newton_reports_a_zero_derivative() {
        let error = find_root(square_minus_two, 0.0, 1e-14, 50).err();
        assert_eq!(error, Some(RootError::ZeroDerivative { x: 0.0 }));
    }

    #[test]
    fn bracketed_finds_square_root_of_two_from_either_orientation() {
        for (a, b) in [(0.0, 2.0), (2.0, 0.0)] {
            let root = find_root_bracketed(square_minus_two, a, b, 1e-14, 100).expect("converges");
            assert!((root.x - SQRT_2).abs() < 1e-14);
        }
    }

    #[test]
    fn bracketed_survives_a_zero_derivative_inside_the_bracket() {
        // x³ has f'(0) = 0 at its root; plain Newton converges only linearly there and the
        // bracket must keep the iteration safe. Bracket midpoint is exactly the root.
        let cube = |x: f64| (x * x * x, 3.0 * x * x);
        let root = find_root_bracketed(cube, -1.0, 1.0, 1e-12, 200).expect("converges");
        assert!(root.x.abs() < 1e-4);
        // An off-centre bracket: falls back to bisection where Newton is too slow.
        let root = find_root_bracketed(cube, -1.0, 3.0, 1e-12, 200).expect("converges");
        assert!(root.x.abs() < 1e-4, "x = {}", root.x);
    }

    #[test]
    fn bracketed_handles_a_flat_then_steep_function() {
        // atan has a root at 0 and Newton from far out overshoots wildly (|x| ≳ 1.39);
        // the bracket must keep the iteration inside.
        let atan = |x: f64| (x.atan(), 1.0 / (1.0 + x * x));
        let root = find_root_bracketed(atan, -5.0, 20.0, 1e-14, 200).expect("converges");
        assert!(root.x.abs() < 1e-12);
        assert!(find_root(atan, 20.0, 1e-14, 50).is_err());
    }

    #[test]
    fn bracketed_rejects_a_bracket_without_sign_change() {
        let error = find_root_bracketed(square_minus_two, 2.0, 3.0, 1e-14, 100).err();
        assert!(matches!(error, Some(RootError::NoSignChange { .. })));
    }
}
