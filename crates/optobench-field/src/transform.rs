//! Discrete Fourier transforms of square grids: the only place that knows which FFT does
//! the work (`rustfft` today, MIT OR Apache-2.0; design.md, Field propagation, FFT), so
//! it can be tuned or replaced without touching the rest.
//!
//! Conventions: the forward transform is X_k = Σ_j x_j e^{−2πi jk/N} along each axis; the
//! inverse has e^{+2πi jk/N} and the factor 1/N per axis, so inverse(forward(x)) = x.

use std::sync::Arc;

use optobench_math::Complex;
use rustfft::num_complex::Complex as Number;
use rustfft::{Fft, FftPlanner};

/// Which way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// e^{−2πi jk/N}.
    Forward,
    /// e^{+2πi jk/N}, divided by N per axis.
    Inverse,
}

/// Plans for transforming N × N grids, made once and reused.
#[derive(Clone)]
pub struct Transform {
    n: usize,
    forward: Arc<dyn Fft<f64>>,
    inverse: Arc<dyn Fft<f64>>,
}

impl core::fmt::Debug for Transform {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Transform({0} × {0})", self.n)
    }
}

impl Transform {
    /// Plans for N × N grids.
    pub fn new(n: usize) -> Self {
        let mut planner = FftPlanner::new();
        Self {
            n,
            forward: planner.plan_fft_forward(n),
            inverse: planner.plan_fft_inverse(n),
        }
    }

    /// The grid size.
    pub fn size(&self) -> usize {
        self.n
    }

    /// Transforms `data` (row by row, N × N) in place.
    pub fn apply(&self, data: &mut [Complex], direction: Direction) {
        let n = self.n;
        assert_eq!(data.len(), n * n, "a grid of N × N");
        let plan = match direction {
            Direction::Forward => &self.forward,
            Direction::Inverse => &self.inverse,
        };
        let mut buffer: Vec<Number<f64>> = data.iter().map(|c| Number::new(c.re, c.im)).collect();
        // Rows, then columns (through a transpose and back).
        for row in buffer.chunks_exact_mut(n) {
            plan.process(row);
        }
        let mut transposed = vec![Number::new(0.0, 0.0); n * n];
        for r in 0..n {
            for c in 0..n {
                transposed[c * n + r] = buffer[r * n + c];
            }
        }
        for row in transposed.chunks_exact_mut(n) {
            plan.process(row);
        }
        let scale = match direction {
            Direction::Forward => 1.0,
            Direction::Inverse => 1.0 / (n * n) as f64,
        };
        for r in 0..n {
            for c in 0..n {
                let v = transposed[c * n + r];
                data[r * n + c] = Complex::new(v.re * scale, v.im * scale);
            }
        }
    }
}

/// Swaps the halves of a grid along both axes (even N), so the sample at index N/2
/// moves to index 0 and back: between centred grids and the transform's order.
pub fn shift(data: &mut [Complex], n: usize) {
    let h = n / 2;
    for r in 0..n {
        for c in 0..h {
            data.swap(r * n + c, r * n + c + h);
        }
    }
    for r in 0..h {
        for c in 0..n {
            data.swap(r * n + c, (r + h) * n + c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 2D DFT by its definition.
    fn direct(data: &[Complex], n: usize, sign: f64) -> Vec<Complex> {
        let mut out = vec![Complex::ZERO; n * n];
        for kr in 0..n {
            for kc in 0..n {
                let mut sum = Complex::ZERO;
                for r in 0..n {
                    for c in 0..n {
                        let angle = sign
                            * 2.0
                            * core::f64::consts::PI
                            * ((kr * r) as f64 + (kc * c) as f64)
                            / n as f64;
                        sum = sum + data[r * n + c] * Complex::new(angle.cos(), angle.sin());
                    }
                }
                out[kr * n + kc] = sum;
            }
        }
        out
    }

    #[test]
    fn matches_the_definition_and_inverts() {
        // A mixed-radix size (12 = 2² · 3) and a power of two.
        for n in [12, 16] {
            let data: Vec<Complex> = (0..n * n)
                .map(|i| Complex::new((i as f64 * 0.37).sin(), (i as f64 * 0.11).cos()))
                .collect();
            let mut forward = data.clone();
            let t = Transform::new(n);
            t.apply(&mut forward, Direction::Forward);
            let expected = direct(&data, n, -1.0);
            for (a, b) in forward.iter().zip(&expected) {
                assert!((*a - *b).abs() < 1e-10, "{a:?} vs {b:?}");
            }
            t.apply(&mut forward, Direction::Inverse);
            for (a, b) in forward.iter().zip(&data) {
                assert!((*a - *b).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn shifting_twice_is_the_identity_and_centres_index_half() {
        let n = 8;
        let mut data: Vec<Complex> = (0..n * n).map(|i| Complex::real(i as f64)).collect();
        let original = data.clone();
        shift(&mut data, n);
        assert_eq!(data[0], original[(n / 2) * n + n / 2]);
        shift(&mut data, n);
        assert_eq!(data, original);
    }
}
