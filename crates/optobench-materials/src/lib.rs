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
pub mod dispersion;
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
    /// The index is a table, interpolated linearly: it has no second derivative, so no
    /// group-delay dispersion ([`dispersion`]).
    NotSmooth {
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
            MaterialError::NotSmooth { wavelength } => write!(
                f,
                "the index at {wavelength} µm comes from a table, which gives no dispersion \
                 (d²n/dλ²); use a dispersion formula"
            ),
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

    /// Scattering coefficient µs, in 1/mm: the rate at which light leaves the direct beam
    /// by scattering. Zero for a clear medium.
    fn scattering_coefficient(&self, wavelength: f64) -> Result<f64, MaterialError> {
        self.refractive_index(wavelength).map(|_| 0.0)
    }

    /// Anisotropy g, the mean cosine of the scattering angle (−1 to 1; 0 isotropic).
    fn anisotropy(&self, wavelength: f64) -> Result<f64, MaterialError> {
        self.refractive_index(wavelength).map(|_| 0.0)
    }

    /// Whether the index is a smooth function of wavelength (a formula), so that its
    /// derivatives, and the dispersion of a pulse ([`dispersion`]), are defined. False for
    /// a linearly interpolated table.
    fn smooth_index(&self) -> bool {
        true
    }
}

/// A quantity over wavelength: one value, or a table (vacuum wavelength µm, value)
/// interpolated linearly between its rows, undefined outside them.
#[derive(Debug, Clone, PartialEq)]
pub enum Spectrum {
    /// The same at every wavelength.
    Constant(f64),
    /// Rows in increasing wavelength.
    Table(Vec<(f64, f64)>),
}

impl Spectrum {
    /// The value at `wavelength` (µm).
    pub fn at(&self, wavelength: f64) -> Result<f64, MaterialError> {
        match self {
            Spectrum::Constant(v) => Ok(*v),
            Spectrum::Table(rows) => {
                let (first, last) = match (rows.first(), rows.last()) {
                    (Some(f), Some(l)) => (f.0, l.0),
                    _ => return Err(MaterialError::Undefined { wavelength }),
                };
                check_range(wavelength, (first, last))?;
                let i = rows.partition_point(|r| r.0 < wavelength);
                if i == 0 {
                    return Ok(rows[0].1);
                }
                let (a, b) = (rows[i - 1], rows[i.min(rows.len() - 1)]);
                // Rows not increasing (made in code, not read from a file): take the later.
                if b.0 <= a.0 {
                    return Ok(b.1);
                }
                Ok(a.1 + (b.1 - a.1) * (wavelength - a.0) / (b.0 - a.0))
            }
        }
    }
}

/// A turbid medium (tissue, a scattering phantom): a base medium for the index, with
/// absorption µa (in place of the base's own, if given), scattering µs and anisotropy g,
/// each in 1/mm or as a spectrum, and where those values come from.
#[derive(Debug, Clone)]
pub struct Turbid {
    /// The medium that gives the index (and the absorption, if `absorption` is `None`).
    pub base: Medium,
    /// µa (1/mm), in place of the base's.
    pub absorption: Option<Spectrum>,
    /// µs (1/mm).
    pub scattering: Spectrum,
    /// g.
    pub anisotropy: Spectrum,
    /// Where the values come from (a paper, a measurement).
    pub source: String,
}

impl Material for Turbid {
    fn refractive_index(&self, wavelength: f64) -> Result<f64, MaterialError> {
        self.base.material().refractive_index(wavelength)
    }

    fn extinction(&self, wavelength: f64) -> Result<f64, MaterialError> {
        match &self.absorption {
            None => self.base.material().extinction(wavelength),
            // k from µa = 4πk/λ.
            Some(a) => Ok(a.at(wavelength)? * wavelength * 1e-3 / (4.0 * core::f64::consts::PI)),
        }
    }

    fn absorption_coefficient(&self, wavelength: f64) -> Result<f64, MaterialError> {
        match &self.absorption {
            None => self.base.material().absorption_coefficient(wavelength),
            Some(a) => a.at(wavelength),
        }
    }

    fn scattering_coefficient(&self, wavelength: f64) -> Result<f64, MaterialError> {
        self.scattering.at(wavelength)
    }

    fn anisotropy(&self, wavelength: f64) -> Result<f64, MaterialError> {
        self.anisotropy.at(wavelength)
    }

    fn smooth_index(&self) -> bool {
        self.base.material().smooth_index()
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

/// The d line, helium (µm): where a glass's nd is given (ISO 7944, Optics and optical
/// instruments: reference wavelengths).
pub const LINE_D: f64 = 0.587_561_8;
/// The F line, hydrogen β (µm).
pub const LINE_F: f64 = 0.486_132_7;
/// The C line, hydrogen α (µm).
pub const LINE_C: f64 = 0.656_272_5;

/// A glass known only by its catalogue numbers: nd, its index at the d line, and the Abbe
/// number Vd = (nd − 1)/(nF − nC). Between them the index follows the two-term Cauchy
/// formula n = A + B/λ² through nd with nF − nC = (nd − 1)/Vd, so
/// B = (nd − 1) / (Vd (λF⁻² − λC⁻²)) and A = nd − B/λd². It is a model: right at d and in
/// the spread from F to C, close across the visible, and only indicative beyond it (its
/// range, 0.35–2.5 µm, is wide so a design can be traced at any laser line).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelGlass {
    /// Index at the d line.
    pub nd: f64,
    /// Abbe number Vd.
    pub vd: f64,
}

impl ModelGlass {
    /// The model's range (µm).
    pub const RANGE: (f64, f64) = (0.35, 2.5);

    /// A glass from nd and Vd; `None` unless nd > 1 and Vd > 0 (both finite).
    pub fn new(nd: f64, vd: f64) -> Option<Self> {
        (nd.is_finite() && vd.is_finite() && nd > 1.0 && vd > 0.0).then_some(Self { nd, vd })
    }

    /// The Cauchy coefficients (A, B), B in µm².
    pub fn coefficients(&self) -> (f64, f64) {
        let b = (self.nd - 1.0) / (self.vd * (LINE_F.powi(-2) - LINE_C.powi(-2)));
        (self.nd - b / (LINE_D * LINE_D), b)
    }
}

impl Material for ModelGlass {
    fn refractive_index(&self, wavelength: f64) -> Result<f64, MaterialError> {
        check_range(wavelength, Self::RANGE)?;
        let (a, b) = self.coefficients();
        Ok(a + b / (wavelength * wavelength))
    }
}

/// A material's nd and Abbe number Vd = (nd − 1)/(nF − nC), from its own model at the
/// d, F and C lines.
pub fn abbe(material: &dyn Material) -> Result<(f64, f64), MaterialError> {
    let nd = material.refractive_index(LINE_D)?;
    let spread = material.refractive_index(LINE_F)? - material.refractive_index(LINE_C)?;
    if spread.abs() < 1e-12 {
        return Err(MaterialError::Undefined { wavelength: LINE_D });
    }
    Ok((nd, (nd - 1.0) / spread))
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

    fn smooth_index(&self) -> bool {
        false
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

impl From<Turbid> for Medium {
    fn from(material: Turbid) -> Self {
        Self::new(material)
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
    fn a_model_glass_gives_back_its_catalogue_numbers() {
        // A crown of nd 1.5168, Vd 64.17 and a dense flint of nd 1.6727, Vd 32.25
        // (typical catalogue values): the model passes through nd and spreads F to C by
        // (nd − 1)/Vd, by its construction.
        for (nd, vd) in [(1.5168, 64.17), (1.6727, 32.25)] {
            let glass = ModelGlass::new(nd, vd).expect("a glass");
            let n = |l: f64| glass.refractive_index(l).expect("in range");
            assert!((n(LINE_D) - nd).abs() < 1e-12);
            assert!((n(LINE_F) - n(LINE_C) - (nd - 1.0) / vd).abs() < 1e-12);
            let (back_nd, back_vd) = abbe(&glass).expect("dispersive");
            assert!((back_nd - nd).abs() < 1e-12 && (back_vd - vd).abs() < 1e-9);
            // Normal dispersion: the index falls with wavelength.
            assert!(n(0.4) > n(0.7) && n(0.7) > n(1.064));
        }
        assert!(ModelGlass::new(1.0, 50.0).is_none() && ModelGlass::new(1.5, 0.0).is_none());
        assert!(abbe(&Constant { index: 1.5 }).is_err());
    }

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
