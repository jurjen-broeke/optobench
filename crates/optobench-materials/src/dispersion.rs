//! Dispersion of a material for short pulses: the derivatives of the index, the group
//! index and the group-velocity dispersion, from the same models the trace uses.
//!
//! The group-velocity dispersion per unit length is
//! GVD = λ³ / (2π c²) · d²n/dλ² (λ the vacuum wavelength); a pulse crossing a length L
//! picks up the group-delay dispersion GDD = GVD · L (e.g. J.-C. Diels and W. Rudolph,
//! Ultrashort Laser Pulse Phenomena, 2nd ed., 2006, ch. 1). The derivatives are taken
//! numerically on the material's own formula: five-point central differences at steps h
//! and h/2, combined by Richardson extrapolation. A tabulated index (linear between rows)
//! has none and is refused.

use crate::{Material, MaterialError};

/// The speed of light in vacuum (m/s), exact.
pub const LIGHT_SPEED: f64 = 299_792_458.0;

/// The larger step of the differences (µm). Five points leave a truncation error of order
/// h⁴, which the extrapolation over h and h/2 removes, leaving h⁶; the rounding error of the
/// second derivative is about 10 ε n / (h/2)², near 10⁻⁹ /µm² against d²n/dλ² of order
/// 10⁻² for glass in the visible and near infrared.
const STEP: f64 = 4e-3;

/// The five-point first and second differences of `n` at `l` with step `h`.
fn differences(
    n: &dyn Fn(f64) -> Result<f64, MaterialError>,
    l: f64,
    h: f64,
) -> Result<(f64, f64, f64), MaterialError> {
    let (m2, m1, c, p1, p2) = (
        n(l - 2.0 * h)?,
        n(l - h)?,
        n(l)?,
        n(l + h)?,
        n(l + 2.0 * h)?,
    );
    let first = (m2 - 8.0 * m1 + 8.0 * p1 - p2) / (12.0 * h);
    let second = (-m2 + 16.0 * m1 - 30.0 * c + 16.0 * p1 - p2) / (12.0 * h * h);
    Ok((c, first, second))
}

/// n, dn/dλ (1/µm) and d²n/dλ² (1/µm²) at vacuum wavelength `wavelength` (µm). The
/// points at ±2h (8 nm) must lie in the model's range.
pub fn index_derivatives(
    material: &dyn Material,
    wavelength: f64,
) -> Result<(f64, f64, f64), MaterialError> {
    if !material.smooth_index() {
        return Err(MaterialError::NotSmooth { wavelength });
    }
    let n = |w: f64| material.refractive_index(w);
    let (c, first_h, second_h) = differences(&n, wavelength, STEP)?;
    let (_, first_half, second_half) = differences(&n, wavelength, STEP / 2.0)?;
    Ok((
        c,
        (16.0 * first_half - first_h) / 15.0,
        (16.0 * second_half - second_h) / 15.0,
    ))
}

/// The group index n − λ dn/dλ.
pub fn group_index(material: &dyn Material, wavelength: f64) -> Result<f64, MaterialError> {
    let (n, first, _) = index_derivatives(material, wavelength)?;
    Ok(n - wavelength * first)
}

/// The group-velocity dispersion λ³ / (2π c²) · d²n/dλ², in s² per mm of path.
pub fn group_velocity_dispersion(
    material: &dyn Material,
    wavelength: f64,
) -> Result<f64, MaterialError> {
    let (_, _, second) = index_derivatives(material, wavelength)?;
    let lambda = wavelength * 1e-6; // m
    let per_metre =
        lambda.powi(3) / (2.0 * core::f64::consts::PI * LIGHT_SPEED * LIGHT_SPEED) * second * 1e12; // d²n/dλ² in 1/m²
    Ok(per_metre * 1e-3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundled::fused_silica;
    use crate::{Schott, Tabulated};

    /// d²n/dλ² of a Sellmeier glass differentiated by hand: with
    /// n² = 1 + Σ B λ² / (λ² − C), (n²)′ = −Σ 2 B C λ / (λ² − C)² and
    /// (n²)″ = Σ 2 B C (3 λ² + C) / (λ² − C)³; then n′ = (n²)′ / (2n) and
    /// n″ = ((n²)″ − 2 n′²) / (2n).
    fn sellmeier_by_hand(terms: &[(f64, f64)], l: f64) -> (f64, f64, f64) {
        let l2 = l * l;
        let square = 1.0 + terms.iter().map(|&(b, c)| b * l2 / (l2 - c)).sum::<f64>();
        let d1 = -terms
            .iter()
            .map(|&(b, c)| 2.0 * b * c * l / (l2 - c).powi(2))
            .sum::<f64>();
        let d2 = terms
            .iter()
            .map(|&(b, c)| 2.0 * b * c * (3.0 * l2 + c) / (l2 - c).powi(3))
            .sum::<f64>();
        let n = square.sqrt();
        let first = d1 / (2.0 * n);
        (n, first, (d2 - 2.0 * first * first) / (2.0 * n))
    }

    #[test]
    fn fused_silica_against_its_formula_differentiated_by_hand() {
        // The bundled Malitson (1965) formula.
        let silica = fused_silica();
        for l in [0.4, 0.8, 0.92, 1.3] {
            let (n, first, second) = index_derivatives(&silica, l).expect("in range");
            let (n0, first0, second0) = sellmeier_by_hand(&silica.terms, l);
            assert!((n - n0).abs() < 1e-15);
            assert!(
                (first - first0).abs() < 1e-9 * first0.abs(),
                "{first} {first0}"
            );
            assert!(
                (second - second0).abs() < 1e-6 * second0.abs(),
                "{second} {second0}"
            );
            // The GVD from the hand derivative, in fs²/mm.
            let gvd = group_velocity_dispersion(&silica, l).expect("in range") * 1e30;
            let by_hand = (l * 1e-6).powi(3) / (2.0 * core::f64::consts::PI * LIGHT_SPEED.powi(2))
                * second0
                * 1e12
                * 1e-3
                * 1e30;
            assert!((gvd - by_hand).abs() < 1e-6 * by_hand.abs());
        }
        // Positive (normal) dispersion in the near infrared; anomalous past the zero near
        // 1.27 µm (Malitson's formula).
        assert!(group_velocity_dispersion(&silica, 0.92).expect("in range") > 0.0);
        assert!(group_velocity_dispersion(&silica, 1.6).expect("in range") < 0.0);
    }

    #[test]
    fn a_schott_formula_against_its_derivatives_by_hand() {
        // Coefficients chosen for the test (not a catalogue glass): with
        // n² = A₀ + A₁λ² + A₂λ⁻² + A₃λ⁻⁴ + A₄λ⁻⁶ + A₅λ⁻⁸,
        // (n²)′ = 2A₁λ − 2A₂λ⁻³ − 4A₃λ⁻⁵ − 6A₄λ⁻⁷ − 8A₅λ⁻⁹ and
        // (n²)″ = 2A₁ + 6A₂λ⁻⁴ + 20A₃λ⁻⁶ + 42A₄λ⁻⁸ + 72A₅λ⁻¹⁰.
        let a = [2.27, -0.0101, 0.0112, 1.1e-4, -2.0e-6, 1.5e-7];
        let glass = Schott {
            coefficients: a,
            range: (0.3, 2.0),
        };
        for l in [0.5, 0.8, 1.0] {
            let square = a[0]
                + a[1] * l * l
                + a[2] / l.powi(2)
                + a[3] / l.powi(4)
                + a[4] / l.powi(6)
                + a[5] / l.powi(8);
            let d1 = 2.0 * a[1] * l
                - 2.0 * a[2] / l.powi(3)
                - 4.0 * a[3] / l.powi(5)
                - 6.0 * a[4] / l.powi(7)
                - 8.0 * a[5] / l.powi(9);
            let d2 = 2.0 * a[1]
                + 6.0 * a[2] / l.powi(4)
                + 20.0 * a[3] / l.powi(6)
                + 42.0 * a[4] / l.powi(8)
                + 72.0 * a[5] / l.powi(10);
            let n0 = square.sqrt();
            let first0 = d1 / (2.0 * n0);
            let second0 = (d2 - 2.0 * first0 * first0) / (2.0 * n0);
            let (n, first, second) = index_derivatives(&glass, l).expect("in range");
            assert!((n - n0).abs() < 1e-15);
            assert!((first - first0).abs() < 1e-9 * first0.abs());
            assert!((second - second0).abs() < 1e-6 * second0.abs());
            assert!((group_index(&glass, l).expect("in range") - (n0 - l * first0)).abs() < 1e-9);
        }
    }

    #[test]
    fn a_table_and_the_range_ends_are_refused() {
        let table = Tabulated::new(vec![(0.5, 1.5, 0.0), (1.0, 1.4, 0.0)]).expect("valid");
        assert_eq!(
            group_velocity_dispersion(&table, 0.8),
            Err(MaterialError::NotSmooth { wavelength: 0.8 })
        );
        assert!(matches!(
            index_derivatives(&fused_silica(), 6.699),
            Err(MaterialError::OutsideRange { .. })
        ));
    }
}
