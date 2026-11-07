//! Zernike polynomials over the unit disk and least-squares fits.
//!
//! Ordering, sign and normalisation follow the ANSI/OSA standard (ANSI Z80.28; ISO
//! 24157:2008; Thibos et al., "Standards for reporting the optical aberrations of eyes",
//! J. Refract. Surg. 18, S652 (2002)):
//!
//! - single index j = (n(n + 2) + m) / 2, radial order n ≥ 0, azimuthal m from −n to n
//!   in steps of 2;
//! - Z_j(ρ, θ) = N · R_n^|m|(ρ) · cos(mθ) for m ≥ 0, N · R_n^|m|(ρ) · sin(|m|θ) for m < 0,
//!   with θ measured counter-clockwise from +x;
//! - N = √(2(n + 1) / (1 + δ_m0)), so every term has unit RMS over the disk and a
//!   coefficient is that term's RMS contribution.

use optobench_math::{LeastSquaresError, least_squares};

/// (n, m) of the ANSI single index j.
pub fn zernike_indices(j: usize) -> (usize, i64) {
    // n is the largest order with n(n + 1)/2 ≤ j.
    let mut n = 0;
    while (n + 1) * (n + 2) / 2 <= j {
        n += 1;
    }
    let m = 2 * j as i64 - (n * (n + 2)) as i64;
    (n, m)
}

fn factorial(k: usize) -> f64 {
    (1..=k).map(|i| i as f64).product()
}

/// Radial polynomial R_n^m(ρ) for m ≥ 0 and n − m even.
fn radial(n: usize, m: usize, rho: f64) -> f64 {
    (0..=(n - m) / 2)
        .map(|k| {
            let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
            let coefficient = sign * factorial(n - k)
                / (factorial(k) * factorial((n + m) / 2 - k) * factorial((n - m) / 2 - k));
            coefficient * rho.powi((n - 2 * k) as i32)
        })
        .sum()
}

/// The normalised Zernike polynomial with ANSI index `j` at Cartesian pupil point (x, y).
pub fn zernike(j: usize, x: f64, y: f64) -> f64 {
    let (n, m) = zernike_indices(j);
    let rho = x.hypot(y);
    let theta = y.atan2(x);
    let order = m.unsigned_abs() as usize;
    let normalisation = if m == 0 {
        ((n + 1) as f64).sqrt()
    } else {
        (2.0 * (n + 1) as f64).sqrt()
    };
    let angular = if m >= 0 {
        (order as f64 * theta).cos()
    } else {
        (order as f64 * theta).sin()
    };
    normalisation * radial(n, order, rho) * angular
}

/// Why a Zernike fit failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZernikeError {
    /// Too few samples, or samples that cannot separate the requested terms.
    NotEnoughSamples(LeastSquaresError),
}

/// Least-squares Zernike coefficients of `samples` (x, y, value) in the unit disk, for
/// every term up to radial order `max_order`, in ANSI order.
pub fn zernike_fit(
    samples: &[(f64, f64, f64)],
    max_order: usize,
) -> Result<Vec<f64>, ZernikeError> {
    let terms = (max_order + 1) * (max_order + 2) / 2;
    let rows: Vec<Vec<f64>> = samples
        .iter()
        .map(|&(x, y, _)| (0..terms).map(|j| zernike(j, x, y)).collect())
        .collect();
    let values: Vec<f64> = samples.iter().map(|s| s.2).collect();
    least_squares(&rows, &values).map_err(ZernikeError::NotEnoughSamples)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk_grid(points: usize) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        for i in 0..points {
            for k in 0..points {
                let x = -1.0 + (2 * i + 1) as f64 / points as f64;
                let y = -1.0 + (2 * k + 1) as f64 / points as f64;
                if x.hypot(y) <= 1.0 {
                    out.push((x, y));
                }
            }
        }
        out
    }

    #[test]
    fn ansi_indices_follow_the_standard_table() {
        // ANSI Z80.28 / Thibos et al. (2002), table of the first 10 terms.
        let expected = [
            (0, 0),
            (1, -1),
            (1, 1),
            (2, -2),
            (2, 0),
            (2, 2),
            (3, -3),
            (3, -1),
            (3, 1),
            (3, 3),
        ];
        for (j, &(n, m)) in expected.iter().enumerate() {
            assert_eq!(zernike_indices(j), (n, m), "j = {j}");
        }
    }

    #[test]
    fn low_order_terms_match_their_closed_forms() {
        // Thibos et al. (2002): Z₄ = √3 (2ρ² − 1) (defocus), Z₂ = 2ρ cos θ = 2x (tilt),
        // Z₁₂ = √5 (6ρ⁴ − 6ρ² + 1) (primary spherical).
        let (x, y) = (0.3, -0.5);
        let r2 = x * x + y * y;
        assert!((zernike(4, x, y) - 3f64.sqrt() * (2.0 * r2 - 1.0)).abs() < 1e-14);
        assert!((zernike(2, x, y) - 2.0 * x).abs() < 1e-14);
        assert!((zernike(12, x, y) - 5f64.sqrt() * (6.0 * r2 * r2 - 6.0 * r2 + 1.0)).abs() < 1e-13);
    }

    #[test]
    fn terms_are_orthonormal_over_the_disk() {
        // Mean of Z_i · Z_j over a fine uniform grid in the disk → δ_ij.
        let grid = disk_grid(200);
        let count = grid.len() as f64;
        let values: Vec<Vec<f64>> = (0..15)
            .map(|j| grid.iter().map(|&(x, y)| zernike(j, x, y)).collect())
            .collect();
        for i in 0..15 {
            for j in 0..15 {
                let mean = values[i]
                    .iter()
                    .zip(&values[j])
                    .map(|(a, b)| a * b)
                    .sum::<f64>()
                    / count;
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!((mean - expected).abs() < 2e-2, "i = {i}, j = {j}: {mean}");
            }
        }
    }

    #[test]
    fn fit_recovers_known_coefficients() {
        let truth = [
            0.1, 0.0, -0.2, 0.05, 0.3, 0.0, 0.0, -0.07, 0.0, 0.01, 0.0, 0.0, 0.15, 0.0, 0.0,
        ];
        let samples: Vec<(f64, f64, f64)> = disk_grid(25)
            .into_iter()
            .map(|(x, y)| {
                let value = truth
                    .iter()
                    .enumerate()
                    .map(|(j, c)| c * zernike(j, x, y))
                    .sum();
                (x, y, value)
            })
            .collect();
        let fit = zernike_fit(&samples, 4).expect("enough samples");
        for (j, (a, b)) in fit.iter().zip(&truth).enumerate() {
            assert!((a - b).abs() < 1e-12, "j = {j}: {a} vs {b}");
        }
    }
}
