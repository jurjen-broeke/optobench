//! Dense linear least squares by Householder QR.
//!
//! Solves min ‖A x − b‖₂ for a tall or square matrix A with full column rank. QR is used
//! rather than the normal equations AᵀA x = Aᵀb because it does not square the condition
//! number (Golub & Van Loan, Matrix Computations, 4th ed., ch. 5). Written from the
//! textbook algorithm.

/// Why a least-squares problem could not be solved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeastSquaresError {
    /// Rows have different lengths, or `b` does not match the number of rows.
    ShapeMismatch,
    /// Fewer rows than unknowns.
    Underdetermined,
    /// A column is (numerically) a combination of the others.
    RankDeficient {
        /// The column where it was detected.
        column: usize,
    },
}

/// Least-squares solution of A x ≈ b. `rows` holds A row by row.
pub fn least_squares(rows: &[Vec<f64>], b: &[f64]) -> Result<Vec<f64>, LeastSquaresError> {
    let m = rows.len();
    let n = rows.first().map_or(0, Vec::len);
    if b.len() != m || rows.iter().any(|r| r.len() != n) {
        return Err(LeastSquaresError::ShapeMismatch);
    }
    if m < n {
        return Err(LeastSquaresError::Underdetermined);
    }
    // Column-major copy of A, and b, transformed in place.
    let mut a: Vec<Vec<f64>> = (0..n)
        .map(|j| rows.iter().map(|r| r[j]).collect())
        .collect();
    let mut y = b.to_vec();
    let scale = a
        .iter()
        .flatten()
        .fold(0.0_f64, |s, v| s.max(v.abs()))
        .max(f64::MIN_POSITIVE);

    for k in 0..n {
        // Householder vector v for column k, rows k..m, so that H = I − 2vvᵀ/(vᵀv)
        // maps the column to (∓‖x‖, 0, …).
        let norm = a[k][k..].iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm <= 1e-13 * scale {
            return Err(LeastSquaresError::RankDeficient { column: k });
        }
        let alpha = if a[k][k] > 0.0 { -norm } else { norm };
        let mut v: Vec<f64> = a[k][k..].to_vec();
        v[0] -= alpha;
        let vv: f64 = v.iter().map(|x| x * x).sum();
        let reflect = |column: &mut [f64]| {
            let dot: f64 = v.iter().zip(column.iter()).map(|(p, q)| p * q).sum();
            let factor = 2.0 * dot / vv;
            for (c, p) in column.iter_mut().zip(&v) {
                *c -= factor * p;
            }
        };
        for column in a.iter_mut().skip(k) {
            reflect(&mut column[k..]);
        }
        reflect(&mut y[k..]);
    }

    // Back substitution on the upper-triangular R (rows 0..n).
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let sum: f64 = ((i + 1)..n).map(|j| a[j][i] * x[j]).sum();
        x[i] = (y[i] - sum) / a[i][i];
    }
    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_system_is_solved_exactly() {
        // 2x + y = 5, x − 3y = −8 → x = 1, y = 3.
        let x = least_squares(&[vec![2.0, 1.0], vec![1.0, -3.0]], &[5.0, -8.0]).expect("solvable");
        assert!((x[0] - 1.0).abs() < 1e-14 && (x[1] - 3.0).abs() < 1e-14);
    }

    #[test]
    fn line_fit_through_exact_points_recovers_the_line() {
        // Points on y = 2 − 0.5 t.
        let rows: Vec<Vec<f64>> = (0..10).map(|t| vec![1.0, t as f64]).collect();
        let b: Vec<f64> = (0..10).map(|t| 2.0 - 0.5 * t as f64).collect();
        let x = least_squares(&rows, &b).expect("solvable");
        assert!((x[0] - 2.0).abs() < 1e-13 && (x[1] + 0.5).abs() < 1e-13);
    }

    #[test]
    fn residual_is_orthogonal_to_the_columns() {
        // The least-squares residual r = b − Ax satisfies Aᵀr = 0.
        let rows = vec![
            vec![1.0, 0.0],
            vec![1.0, 1.0],
            vec![1.0, 2.0],
            vec![1.0, 4.0],
        ];
        let b = [1.0, 2.0, 2.5, 7.0];
        let x = least_squares(&rows, &b).expect("solvable");
        for j in 0..2 {
            let dot: f64 = rows
                .iter()
                .zip(&b)
                .map(|(r, bi)| r[j] * (bi - (r[0] * x[0] + r[1] * x[1])))
                .sum();
            assert!(dot.abs() < 1e-12);
        }
    }

    #[test]
    fn bad_problems_are_rejected() {
        assert_eq!(
            least_squares(&[vec![1.0, 2.0]], &[1.0]),
            Err(LeastSquaresError::Underdetermined)
        );
        assert_eq!(
            least_squares(
                &[vec![1.0, 2.0], vec![2.0, 4.0], vec![3.0, 6.0]],
                &[1.0, 2.0, 3.0]
            ),
            Err(LeastSquaresError::RankDeficient { column: 1 })
        );
        assert_eq!(
            least_squares(&[vec![1.0], vec![1.0, 2.0]], &[1.0, 2.0]),
            Err(LeastSquaresError::ShapeMismatch)
        );
    }
}
