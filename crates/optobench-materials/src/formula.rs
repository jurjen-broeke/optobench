//! The dispersion formulas of the refractiveindex.info database, and materials that
//! combine an index model with a separate extinction model.
//!
//! Definitions from the database's "Dispersion formulas" document (RefractiveIndex.INFO,
//! 2014-06-29, `database/doc/Dispersion formulas.pdf`, CC0). λ in µm; coefficients
//! C₁, C₂, … as listed in a file; missing trailing coefficients are zero.
//!
//! 1. Sellmeier: n² − 1 = C₁ + Σ C₂ᵢ λ² / (λ² − C₂ᵢ₊₁²)
//! 2. Sellmeier-2: n² − 1 = C₁ + Σ C₂ᵢ λ² / (λ² − C₂ᵢ₊₁)
//! 3. Polynomial: n² = C₁ + Σ C₂ᵢ λ^C₂ᵢ₊₁
//! 4. RefractiveIndex.INFO: n² = C₁ + C₂λ^C₃/(λ² − C₄^C₅) + C₆λ^C₇/(λ² − C₈^C₉) + Σᵢ≥₅ C₂ᵢ λ^C₂ᵢ₊₁
//! 5. Cauchy: n = C₁ + Σ C₂ᵢ λ^C₂ᵢ₊₁
//! 6. Gases: n − 1 = C₁ + Σ C₂ᵢ / (C₂ᵢ₊₁ − λ⁻²)
//! 7. Herzberger: n = C₁ + C₂/(λ² − 0.028) + C₃ (1/(λ² − 0.028))² + C₄λ² + C₅λ⁴ + C₆λ⁶
//! 8. Retro: (n² − 1)/(n² + 2) = C₁ + C₂λ²/(λ² − C₃) + C₄λ²
//! 9. Exotic: n² = C₁ + C₂/(λ² − C₃) + C₄(λ − C₅)/((λ − C₅)² + C₆)
//!
//! Sums run over the pairs present, up to C₁₇ (C₁₁ for Cauchy and Gases).

use std::sync::Arc;

use crate::{Material, MaterialError, check_range, index_from_square};

/// Which of the nine formulas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaKind {
    /// 1: Sellmeier with squared resonance wavelengths.
    Sellmeier,
    /// 2: Sellmeier with resonance terms in µm².
    SellmeierSquaredTerms,
    /// 3: Polynomial in n².
    Polynomial,
    /// 4: The database's own general form.
    RefractiveIndexInfo,
    /// 5: Cauchy with free exponents.
    Cauchy,
    /// 6: Gases.
    Gases,
    /// 7: Herzberger.
    Herzberger,
    /// 8: Retro (Lorentz–Lorenz form).
    Retro,
    /// 9: Exotic.
    Exotic,
}

impl FormulaKind {
    /// The kind for the database's formula number, 1 to 9.
    pub fn from_number(number: u32) -> Option<Self> {
        Some(match number {
            1 => Self::Sellmeier,
            2 => Self::SellmeierSquaredTerms,
            3 => Self::Polynomial,
            4 => Self::RefractiveIndexInfo,
            5 => Self::Cauchy,
            6 => Self::Gases,
            7 => Self::Herzberger,
            8 => Self::Retro,
            9 => Self::Exotic,
            _ => return None,
        })
    }

    /// The most coefficients the formula uses.
    pub fn max_coefficients(self) -> usize {
        match self {
            Self::Cauchy | Self::Gases => 11,
            Self::Herzberger => 6,
            Self::Retro => 4,
            Self::Exotic => 6,
            _ => 17,
        }
    }
}

/// A dispersion formula with its coefficients and valid range.
#[derive(Debug, Clone, PartialEq)]
pub struct DispersionFormula {
    /// Which formula.
    pub kind: FormulaKind,
    /// C₁, C₂, … in order.
    pub coefficients: Vec<f64>,
    /// Valid wavelength range (µm), inclusive.
    pub range: (f64, f64),
}

impl DispersionFormula {
    /// C_i (1-based), zero if not given.
    fn c(&self, i: usize) -> f64 {
        self.coefficients.get(i - 1).copied().unwrap_or(0.0)
    }

    /// Σ over pairs (C_i, C_{i+1}) for i = first, first + 2, … ≤ last, of term(C_i, C_{i+1}).
    fn pairs(&self, first: usize, last: usize, term: impl Fn(f64, f64) -> f64) -> f64 {
        (first..=last)
            .step_by(2)
            .map(|i| term(self.c(i), self.c(i + 1)))
            .sum()
    }
}

impl Material for DispersionFormula {
    fn refractive_index(&self, wavelength: f64) -> Result<f64, MaterialError> {
        check_range(wavelength, self.range)?;
        let l = wavelength;
        let l2 = l * l;
        let undefined = Err(MaterialError::Undefined { wavelength });
        let n = match self.kind {
            FormulaKind::Sellmeier => {
                let s = self.c(1) + self.pairs(2, 16, |b, c| b * l2 / (l2 - c * c));
                return index_from_square(1.0 + s, wavelength);
            }
            FormulaKind::SellmeierSquaredTerms => {
                let s = self.c(1) + self.pairs(2, 16, |b, c| b * l2 / (l2 - c));
                return index_from_square(1.0 + s, wavelength);
            }
            FormulaKind::Polynomial => {
                let s = self.c(1) + self.pairs(2, 16, |a, e| a * l.powf(e));
                return index_from_square(s, wavelength);
            }
            FormulaKind::RefractiveIndexInfo => {
                let s = self.c(1)
                    + self.c(2) * l.powf(self.c(3)) / (l2 - self.c(4).powf(self.c(5)))
                    + self.c(6) * l.powf(self.c(7)) / (l2 - self.c(8).powf(self.c(9)))
                    + self.pairs(10, 16, |a, e| a * l.powf(e));
                return index_from_square(s, wavelength);
            }
            FormulaKind::Cauchy => self.c(1) + self.pairs(2, 10, |a, e| a * l.powf(e)),
            FormulaKind::Gases => 1.0 + self.c(1) + self.pairs(2, 10, |b, c| b / (c - 1.0 / l2)),
            FormulaKind::Herzberger => {
                let d = 1.0 / (l2 - 0.028);
                self.c(1)
                    + self.c(2) * d
                    + self.c(3) * d * d
                    + self.c(4) * l2
                    + self.c(5) * l2 * l2
                    + self.c(6) * l2 * l2 * l2
            }
            FormulaKind::Retro => {
                // (n² − 1)/(n² + 2) = r  ⇒  n² = (1 + 2r)/(1 − r).
                let r = self.c(1) + self.c(2) * l2 / (l2 - self.c(3)) + self.c(4) * l2;
                if r >= 1.0 {
                    return undefined;
                }
                return index_from_square((1.0 + 2.0 * r) / (1.0 - r), wavelength);
            }
            FormulaKind::Exotic => {
                let shifted = l - self.c(5);
                let s = self.c(1)
                    + self.c(2) / (l2 - self.c(3))
                    + self.c(4) * shifted / (shifted * shifted + self.c(6));
                return index_from_square(s, wavelength);
            }
        };
        if n > 0.0 && n.is_finite() {
            Ok(n)
        } else {
            undefined
        }
    }
}

/// A material whose index and extinction come from different models, as in database
/// files that give a formula for n and a table for k.
#[derive(Debug, Clone)]
pub struct Composite {
    /// Supplies n.
    pub index: Arc<dyn Material>,
    /// Supplies k; `None` for a lossless material.
    pub extinction: Option<Arc<dyn Material>>,
}

impl Material for Composite {
    fn refractive_index(&self, wavelength: f64) -> Result<f64, MaterialError> {
        if let Some(extinction) = &self.extinction {
            // Valid only where both models are.
            extinction.extinction(wavelength)?;
        }
        self.index.refractive_index(wavelength)
    }

    fn extinction(&self, wavelength: f64) -> Result<f64, MaterialError> {
        self.index.refractive_index(wavelength)?;
        match &self.extinction {
            Some(extinction) => extinction.extinction(wavelength),
            None => Ok(0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundled::{StandardAir, fused_silica};

    const RANGE: (f64, f64) = (0.3, 2.0);

    fn formula(number: u32, coefficients: &[f64]) -> DispersionFormula {
        DispersionFormula {
            kind: FormulaKind::from_number(number).expect("1 to 9"),
            coefficients: coefficients.to_vec(),
            range: RANGE,
        }
    }

    fn n(m: &dyn Material, w: f64) -> f64 {
        m.refractive_index(w).expect("defined")
    }

    #[test]
    fn formula_1_and_2_match_the_sellmeier_models() {
        // The bundled fused silica uses formula 1 coefficients; formula 1 with the raw
        // values must equal it. Formula 2 with squared values must too.
        let raw = [
            0.0, 0.6961663, 0.0684043, 0.4079426, 0.1162414, 0.8974794, 9.896161,
        ];
        let squared: Vec<f64> = raw
            .iter()
            .enumerate()
            .map(|(i, &c)| if i >= 2 && i % 2 == 0 { c * c } else { c })
            .collect();
        let silica = fused_silica();
        for w in [0.4, 0.6, 1.5] {
            assert!((n(&formula(1, &raw), w) - n(&silica, w)).abs() < 1e-15);
            assert!((n(&formula(2, &squared), w) - n(&silica, w)).abs() < 1e-15);
        }
        // The constant C₁ adds to n² − 1.
        let shifted = formula(1, &[0.25]);
        assert!((n(&shifted, 0.5) - 1.25_f64.sqrt()).abs() < 1e-15);
    }

    #[test]
    fn formula_6_matches_the_bundled_air() {
        let air = formula(6, &[0.0, 0.05792105, 238.0185, 0.00167917, 57.362]);
        for w in [0.4, 0.6, 1.5] {
            assert!((n(&air, w) - n(&StandardAir, w)).abs() < 1e-15);
        }
    }

    #[test]
    fn remaining_formulas_evaluate_by_hand() {
        // Each formula at λ = 0.5 µm (λ² = 0.25), with values chosen so the result can be
        // worked out by hand.
        let w = 0.5;
        // 3: n² = 2 + 0.25·λ⁻² = 2 + 1 = 3.
        assert!((n(&formula(3, &[2.0, 0.25, -2.0]), w) - 3f64.sqrt()).abs() < 1e-15);
        // 4: n² = 1 + 1·λ²/(λ² − 0.05¹) + 0·… + 0.5·λ⁰ = 1 + 0.25/0.2 + 0.5 = 2.75.
        let f4 = formula(4, &[1.0, 1.0, 2.0, 0.05, 1.0, 0.0, 0.0, 0.0, 1.0, 0.5, 0.0]);
        assert!((n(&f4, w) - 2.75f64.sqrt()).abs() < 1e-15);
        // 5: n = 1.4 + 0.01·λ⁻² = 1.44.
        assert!((n(&formula(5, &[1.4, 0.01, -2.0]), w) - 1.44).abs() < 1e-15);
        // 7: n = 1.5 + 0.0111/(0.25 − 0.028) = 1.5 + 0.05 = 1.55.
        assert!((n(&formula(7, &[1.5, 0.0111]), w) - 1.55).abs() < 1e-14);
        // 8: r = 0.25 ⇒ n² = 1.5/0.75 = 2.
        assert!((n(&formula(8, &[0.25]), w) - 2f64.sqrt()).abs() < 1e-15);
        // 9: n² = 2 + 0.1/(0.25 − 0.15) = 3.
        assert!((n(&formula(9, &[2.0, 0.1, 0.15]), w) - 3f64.sqrt()).abs() < 1e-15);
        assert!(FormulaKind::from_number(10).is_none());
    }

    #[test]
    fn composite_takes_n_and_k_from_separate_models() {
        let table = crate::Tabulated::new(vec![(0.4, 1.0, 1e-6), (0.8, 1.0, 3e-6)]).expect("valid");
        let composite = Composite {
            index: Arc::new(fused_silica()),
            extinction: Some(Arc::new(table)),
        };
        assert!((n(&composite, 0.6) - n(&fused_silica(), 0.6)).abs() < 1e-15);
        assert!((composite.extinction(0.6).expect("defined") - 2e-6).abs() < 1e-18);
        // Outside the table, neither is defined.
        assert!(composite.refractive_index(1.0).is_err());
    }
}
