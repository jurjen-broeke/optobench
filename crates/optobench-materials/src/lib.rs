//! Dispersion models (Sellmeier, Schott, Cauchy, tabulated n and k) and formula-defined materials such as air, fused silica and water.
//!
//! Layer L1. See `docs/design.md`, sections Architecture and Optical model.
//!
//! # Conventions
//!
//! - Wavelengths are vacuum wavelengths in micrometres (µm).
//! - The complex refractive index is n + i·k: n is the refractive index, k the
//!   extinction coefficient. Bulk absorption follows Beer–Lambert with the absorption
//!   coefficient α = 4πk/λ (Born & Wolf, Principles of Optics, 7th ed., ch. 14).
//! - Every model has a valid wavelength range. Outside it, evaluation is an error rather
//!   than a silent extrapolation.
//! - This crate holds models only. Coefficients for real glasses come from catalogues the
//!   user downloads (docs/design.md, Ground rules). The few bundled materials admitted by
//!   the licence review are in [`bundled`].

#![forbid(unsafe_code)]

pub mod bundled;
mod formula;

pub use formula::{Composite, DispersionFormula, FormulaKind};

use std::sync::Arc;

/// Why a material could not be evaluated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaterialError {
    /// The wavelength lies outside the model's valid range.
    OutsideRange {
        /// The requested wavelength (µm).
        wavelength: f64,
        /// The valid range (µm), inclusive.
        range: (f64, f64),
    },
    /// The formula gives no real index here (n² ≤ 0, for example at a Sellmeier pole).
    Undefined {
        /// The requested wavelength (µm).
        wavelength: f64,
    },
}

impl core::fmt::Display for MaterialError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MaterialError::OutsideRange { wavelength, range } => write!(
                f,
                "wavelength {wavelength} µm is outside the valid range {} to {} µm",
                range.0, range.1
            ),
            MaterialError::Undefined { wavelength } => {
                write!(f, "no real refractive index at {wavelength} µm")
            }
        }
    }
}

impl std::error::Error for MaterialError {}

/// An optical material: refractive index and extinction against wavelength.
pub trait Material: core::fmt::Debug + Send + Sync {
    /// Refractive index n at a vacuum wavelength (µm).
    fn refractive_index(&self, wavelength: f64) -> Result<f64, MaterialError>;

    /// Extinction coefficient k at a vacuum wavelength (µm). Zero for a lossless model.
    fn extinction(&self, wavelength: f64) -> Result<f64, MaterialError> {
        self.refractive_index(wavelength).map(|_| 0.0)
    }

    /// Bulk absorption coefficient α = 4πk/λ, in 1/mm.
    fn absorption_coefficient(&self, wavelength: f64) -> Result<f64, MaterialError> {
        let k = self.extinction(wavelength)?;
        Ok(4.0 * core::f64::consts::PI * k / (wavelength * 1e-3))
    }
}

/// Checks a wavelength against an inclusive range.
fn check_range(wavelength: f64, range: (f64, f64)) -> Result<(), MaterialError> {
    if wavelength >= range.0 && wavelength <= range.1 {
        Ok(())
    } else {
        Err(MaterialError::OutsideRange { wavelength, range })
    }
}

/// n from n², rejecting n² ≤ 0 and non-finite values.
fn index_from_square(square: f64, wavelength: f64) -> Result<f64, MaterialError> {
    if square > 0.0 && square.is_finite() {
        Ok(square.sqrt())
    } else {
        Err(MaterialError::Undefined { wavelength })
    }
}

/// A medium with the same index at every wavelength (vacuum, an idealised gap).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Constant {
    /// Refractive index.
    pub index: f64,
}

impl Material for Constant {
    fn refractive_index(&self, _wavelength: f64) -> Result<f64, MaterialError> {
        Ok(self.index)
    }
}

/// Sellmeier formula: n² = 1 + Σ Bᵢ λ² / (λ² − Cᵢ), with λ in µm and Cᵢ in µm².
/// This is the form glass catalogues publish (e.g. Schott, TIE-29, Refractive index and
/// dispersion, 2016).
#[derive(Debug, Clone, PartialEq)]
pub struct Sellmeier {
    /// The (Bᵢ, Cᵢ) pairs.
    pub terms: Vec<(f64, f64)>,
    /// Valid wavelength range (µm), inclusive.
    pub range: (f64, f64),
}

impl Material for Sellmeier {
    fn refractive_index(&self, wavelength: f64) -> Result<f64, MaterialError> {
        check_range(wavelength, self.range)?;
        let l2 = wavelength * wavelength;
        let square = 1.0
            + self
                .terms
                .iter()
                .map(|&(b, c)| b * l2 / (l2 - c))
                .sum::<f64>();
        index_from_square(square, wavelength)
    }
}

/// Schott formula: n² = A₀ + A₁λ² + A₂λ⁻² + A₃λ⁻⁴ + A₄λ⁻⁶ + A₅λ⁻⁸, λ in µm. The older
/// catalogue form, still found in published data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Schott {
    /// A₀ … A₅.
    pub coefficients: [f64; 6],
    /// Valid wavelength range (µm), inclusive.
    pub range: (f64, f64),
}

impl Material for Schott {
    fn refractive_index(&self, wavelength: f64) -> Result<f64, MaterialError> {
        check_range(wavelength, self.range)?;
        let [a0, a1, a2, a3, a4, a5] = self.coefficients;
        let l2 = wavelength * wavelength;
        let inverse = 1.0 / l2;
        // A₂λ⁻² + A₃λ⁻⁴ + A₄λ⁻⁶ + A₅λ⁻⁸ by Horner in λ⁻².
        let tail = inverse * (a2 + inverse * (a3 + inverse * (a4 + inverse * a5)));
        index_from_square(a0 + a1 * l2 + tail, wavelength)
    }
}

/// Cauchy formula: n = A₀ + A₁/λ² + A₂/λ⁴ + …, λ in µm.
#[derive(Debug, Clone, PartialEq)]
pub struct Cauchy {
    /// A₀, A₁, A₂, …
    pub coefficients: Vec<f64>,
    /// Valid wavelength range (µm), inclusive.
    pub range: (f64, f64),
}

impl Material for Cauchy {
    fn refractive_index(&self, wavelength: f64) -> Result<f64, MaterialError> {
        check_range(wavelength, self.range)?;
        let inverse = 1.0 / (wavelength * wavelength);
        let n = self
            .coefficients
            .iter()
            .rev()
            .fold(0.0, |sum, &a| sum * inverse + a);
        if n > 0.0 && n.is_finite() {
            Ok(n)
        } else {
            Err(MaterialError::Undefined { wavelength })
        }
    }
}

/// Measured n and k at listed wavelengths, linearly interpolated in wavelength.
#[derive(Debug, Clone, PartialEq)]
pub struct Tabulated {
    /// (wavelength µm, n, k), strictly increasing in wavelength.
    points: Vec<(f64, f64, f64)>,
}

/// Why a table was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableError {
    /// Fewer than two points.
    TooShort,
    /// Wavelengths are not strictly increasing at this row (0-based).
    NotIncreasing {
        /// The offending row.
        row: usize,
    },
    /// A wavelength or n is not positive and finite, or k is negative or not finite.
    InvalidValue {
        /// The offending row.
        row: usize,
    },
}

impl Tabulated {
    /// A table of (wavelength µm, n, k) rows.
    pub fn new(points: Vec<(f64, f64, f64)>) -> Result<Self, TableError> {
        if points.len() < 2 {
            return Err(TableError::TooShort);
        }
        for (row, &(wavelength, n, k)) in points.iter().enumerate() {
            let valid = wavelength > 0.0
                && wavelength.is_finite()
                && n > 0.0
                && n.is_finite()
                && k >= 0.0
                && k.is_finite();
            if !valid {
                return Err(TableError::InvalidValue { row });
            }
            if row > 0 && wavelength <= points[row - 1].0 {
                return Err(TableError::NotIncreasing { row });
            }
        }
        Ok(Self { points })
    }

    /// Valid wavelength range: the first and last tabulated wavelengths.
    pub fn range(&self) -> (f64, f64) {
        let first = self.points.first().map_or(0.0, |p| p.0);
        let last = self.points.last().map_or(0.0, |p| p.0);
        (first, last)
    }

    /// Interpolated (n, k).
    fn evaluate(&self, wavelength: f64) -> Result<(f64, f64), MaterialError> {
        check_range(wavelength, self.range())?;
        // The first row whose wavelength is ≥ the request; ≥ 1 after the range check
        // unless the request equals the first wavelength.
        let upper = self.points.partition_point(|p| p.0 < wavelength).max(1);
        let (l0, n0, k0) = self.points[upper - 1];
        let (l1, n1, k1) = self.points[upper];
        let s = (wavelength - l0) / (l1 - l0);
        Ok((n0 + s * (n1 - n0), k0 + s * (k1 - k0)))
    }
}

impl Material for Tabulated {
    fn refractive_index(&self, wavelength: f64) -> Result<f64, MaterialError> {
        self.evaluate(wavelength).map(|(n, _)| n)
    }

    fn extinction(&self, wavelength: f64) -> Result<f64, MaterialError> {
        self.evaluate(wavelength).map(|(_, k)| k)
    }
}

/// A shared handle to any material, as stored in systems and scenes.
#[derive(Debug, Clone)]
pub struct Medium(Arc<dyn Material>);

impl Medium {
    /// Wraps a material.
    pub fn new(material: impl Material + 'static) -> Self {
        Self(Arc::new(material))
    }

    /// The material.
    pub fn material(&self) -> &dyn Material {
        self.0.as_ref()
    }

    /// Whether two handles share one material (the same model, not merely equal values).
    pub fn same(&self, other: &Medium) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl From<f64> for Medium {
    /// A constant-index medium.
    fn from(index: f64) -> Self {
        Self::new(Constant { index })
    }
}

macro_rules! medium_from {
    ($($model:ty),*) => {$(
        impl From<$model> for Medium {
            fn from(material: $model) -> Self {
                Self::new(material)
            }
        }
    )*};
}
medium_from!(
    Constant,
    Sellmeier,
    Schott,
    Cauchy,
    Tabulated,
    DispersionFormula,
    Composite
);

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-14;
    const VISIBLE: (f64, f64) = (0.4, 0.8);

    #[test]
    fn sellmeier_term_without_resonance_is_a_constant_shift() {
        // With C = 0 a term adds B to n² at every wavelength: n = √(1 + B).
        let material = Sellmeier {
            terms: vec![(1.25, 0.0)],
            range: VISIBLE,
        };
        for wavelength in [0.4, 0.55, 0.8] {
            let n = material.refractive_index(wavelength).expect("in range");
            assert!((n - 2.25_f64.sqrt()).abs() < EPS);
        }
    }

    #[test]
    fn sellmeier_with_ultraviolet_resonance_shows_normal_dispersion() {
        // Illustrative coefficients, not a real glass: one resonance at √C = 0.1 µm.
        // At longer wavelengths n falls as λ grows (normal dispersion), and the value
        // matches the formula evaluated by hand at 0.5 µm:
        // n² = 1 + 1.2 · 0.25 / (0.25 − 0.01) = 2.25, n = 1.5.
        let material = Sellmeier {
            terms: vec![(1.2, 0.01)],
            range: VISIBLE,
        };
        let n = |w: f64| material.refractive_index(w).expect("in range");
        assert!((n(0.5) - 1.5).abs() < EPS);
        assert!(n(0.4) > n(0.5) && n(0.5) > n(0.8));
    }

    #[test]
    fn sellmeier_rejects_wavelengths_outside_its_range_and_at_a_pole() {
        let material = Sellmeier {
            terms: vec![(1.2, 0.01)],
            range: VISIBLE,
        };
        assert_eq!(
            material.refractive_index(1.0),
            Err(MaterialError::OutsideRange {
                wavelength: 1.0,
                range: VISIBLE
            })
        );
        let pole = Sellmeier {
            terms: vec![(1.2, 0.25)],
            range: VISIBLE,
        };
        assert_eq!(
            pole.refractive_index(0.5),
            Err(MaterialError::Undefined { wavelength: 0.5 })
        );
    }

    #[test]
    fn schott_and_cauchy_reduce_to_their_leading_terms() {
        let schott = Schott {
            coefficients: [2.25, 0.0, 0.0, 0.0, 0.0, 0.0],
            range: VISIBLE,
        };
        assert!((schott.refractive_index(0.6).expect("in range") - 1.5).abs() < EPS);
        // n² = A₀ + A₂/λ²: at λ = 0.5 µm, 2.2 + 0.0125 · 4 = 2.25.
        let schott = Schott {
            coefficients: [2.2, 0.0, 0.0125, 0.0, 0.0, 0.0],
            range: VISIBLE,
        };
        assert!((schott.refractive_index(0.5).expect("in range") - 1.5).abs() < EPS);
        // n = A₀ + A₁/λ² + A₂/λ⁴: at λ = 0.5 µm, 1.45 + 0.01 · 4 + 0.000625 · 16 = 1.5.
        let cauchy = Cauchy {
            coefficients: vec![1.45, 0.01, 0.000625],
            range: VISIBLE,
        };
        assert!((cauchy.refractive_index(0.5).expect("in range") - 1.5).abs() < EPS);
    }

    #[test]
    fn table_interpolates_linearly_and_is_exact_at_nodes() {
        let table = Tabulated::new(vec![
            (0.4, 1.34, 1e-9),
            (0.6, 1.33, 3e-9),
            (1.0, 1.32, 3e-6),
        ])
        .expect("valid table");
        assert!((table.refractive_index(0.4).expect("in range") - 1.34).abs() < EPS);
        assert!((table.refractive_index(1.0).expect("in range") - 1.32).abs() < EPS);
        assert!((table.refractive_index(0.5).expect("in range") - 1.335).abs() < EPS);
        assert!((table.extinction(0.8).expect("in range") - 1.5015e-6).abs() < 1e-18);
        assert!(table.refractive_index(1.2).is_err());
    }

    #[test]
    fn table_rejects_bad_rows() {
        assert_eq!(
            Tabulated::new(vec![(0.5, 1.5, 0.0)]),
            Err(TableError::TooShort)
        );
        assert_eq!(
            Tabulated::new(vec![(0.5, 1.5, 0.0), (0.5, 1.4, 0.0)]),
            Err(TableError::NotIncreasing { row: 1 })
        );
        assert_eq!(
            Tabulated::new(vec![(0.5, 1.5, 0.0), (0.6, 1.4, -1.0)]),
            Err(TableError::InvalidValue { row: 1 })
        );
    }

    #[test]
    fn absorption_coefficient_follows_beer_lambert() {
        // α = 4πk/λ: k = 1e-6 at λ = 1 µm (1e-3 mm) gives α = 4π · 1e-3 per mm.
        let table = Tabulated::new(vec![(0.9, 1.3, 1e-6), (1.1, 1.3, 1e-6)]).expect("valid");
        let alpha = table.absorption_coefficient(1.0).expect("in range");
        assert!((alpha - 4.0 * core::f64::consts::PI * 1e-3).abs() < 1e-15);
        assert_eq!(Constant { index: 1.0 }.absorption_coefficient(0.5), Ok(0.0));
    }
}
