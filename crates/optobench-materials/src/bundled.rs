//! Materials bundled with optobench, admitted by the licence review
//! (`docs/licence-review.md`, decisions of 2026-11-07).
//!
//! Every value is transcribed unchanged from the refractiveindex.info database (CC0 1.0),
//! which cites the original paper given with each material. **None of them has yet been
//! verified against the original paper**; [`Provenance::verified`] records that, and it
//! changes only when a value has been checked against the paper's own tables.

use crate::{Material, MaterialError, Sellmeier, TableError, Tabulated};

/// Where a bundled material's values come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provenance {
    /// The original publication.
    pub reference: &'static str,
    /// The file the values were transcribed from, relative to [`DATABASE`].
    pub transcribed_from: &'static str,
    /// Licence of the transcription.
    pub licence: &'static str,
    /// Whether the values have been checked against the original publication.
    pub verified: bool,
}

/// The database every [`Provenance::transcribed_from`] path is relative to.
pub const DATABASE: &str = "refractiveindex.info database, https://github.com/polyanskiy/refractiveindex.info-database (CC0 1.0)";

/// Fused silica at 20 °C, Sellmeier formula, 0.21–6.7 µm.
pub fn fused_silica() -> Sellmeier {
    // Formula 1 in the database: n² − 1 = Σ Bᵢ λ² / (λ² − Cᵢ²), Cᵢ in µm.
    let terms = [
        (0.6961663, 0.0684043),
        (0.4079426, 0.1162414),
        (0.8974794, 9.896161),
    ];
    Sellmeier {
        terms: terms.iter().map(|&(b, c)| (b, c * c)).collect(),
        range: (0.21, 6.7),
    }
}

/// Provenance of [`fused_silica`].
pub const FUSED_SILICA: Provenance = Provenance {
    reference: "I. H. Malitson, J. Opt. Soc. Am. 55, 1205-1208 (1965); range to 6.7 µm: C. Z. Tan, J. Non-Cryst. Solids 223, 158-163 (1998)",
    transcribed_from: "database/data/main/SiO2/nk/Malitson.yml",
    licence: "CC0-1.0",
    verified: false,
};

/// Standard dry air (15 °C, 101.325 kPa, 450 ppm CO₂), 0.23–1.69 µm:
/// n − 1 = k₁/(k₀ − λ⁻²) + k₃/(k₂ − λ⁻²). Other conditions need Ciddor's full method,
/// not yet implemented.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StandardAir;

impl Material for StandardAir {
    fn refractive_index(&self, wavelength: f64) -> Result<f64, MaterialError> {
        let range = (0.23, 1.69);
        if !(range.0..=range.1).contains(&wavelength) {
            return Err(MaterialError::OutsideRange { wavelength, range });
        }
        let sigma2 = 1.0 / (wavelength * wavelength);
        Ok(1.0 + 0.05792105 / (238.0185 - sigma2) + 0.00167917 / (57.362 - sigma2))
    }
}

/// Provenance of [`StandardAir`].
pub const STANDARD_AIR: Provenance = Provenance {
    reference: "P. E. Ciddor, Appl. Opt. 35, 1566-1573 (1996)",
    transcribed_from: "database/data/other/mixed gases/air/nk/Ciddor.yml",
    licence: "CC0-1.0",
    verified: false,
};

/// Daimon & Masumura's water Sellmeier coefficients (formula 2 in the database:
/// n² − 1 = Σ Bᵢ λ² / (λ² − Cᵢ), Cᵢ in µm²), per temperature in °C.
const DAIMON: [(f64, [(f64, f64); 4]); 4] = [
    (
        19.0,
        [
            (5.672526103E-1, 5.085550461E-3),
            (1.736581125E-1, 1.814938654E-2),
            (2.121531502E-2, 2.617260739E-2),
            (1.138493213E-1, 1.073888649E1),
        ],
    ),
    (
        20.0,
        [
            (5.684027565E-1, 5.101829712E-3),
            (1.726177391E-1, 1.821153936E-2),
            (2.086189578E-2, 2.620722293E-2),
            (1.130748688E-1, 1.069792721E1),
        ],
    ),
    (
        21.5,
        [
            (5.689093832E-1, 5.110301794E-3),
            (1.719708856E-1, 1.825180155E-2),
            (2.062501582E-2, 2.624158904E-2),
            (1.123965424E-1, 1.067505178E1),
        ],
    ),
    (
        24.0,
        [
            (5.666959820E-1, 5.084151894E-3),
            (1.731900098E-1, 1.818488474E-2),
            (2.095951857E-2, 2.625439472E-2),
            (1.125228406E-1, 1.073842352E1),
        ],
    ),
];

/// Wavelength range of Daimon & Masumura's formulas (µm).
const DAIMON_RANGE: (f64, f64) = (0.182, 1.129);

/// Temperatures (°C) at which Daimon & Masumura give coefficients.
pub const WATER_TEMPERATURES: (f64, f64) = (19.0, 24.0);

/// Distilled water at a temperature (°C) between 19 and 24.
///
/// - n: Daimon & Masumura's formula, interpolated linearly in temperature between the
///   two nearest measured temperatures.
/// - k: Hale & Querry's table at 25 °C (absorption barely changes over this range).
///
/// Valid where both apply: 0.2–1.129 µm.
#[derive(Debug, Clone, PartialEq)]
pub struct Water {
    temperature: f64,
    absorption: Tabulated,
}

impl Water {
    /// Water at `temperature` °C, or `None` outside 19–24 °C.
    pub fn new(temperature: f64) -> Option<Self> {
        let (low, high) = WATER_TEMPERATURES;
        if !(low..=high).contains(&temperature) {
            return None;
        }
        Some(Self {
            temperature,
            absorption: water_hale_querry(),
        })
    }

    /// The temperature (°C).
    pub fn temperature(&self) -> f64 {
        self.temperature
    }

    fn index_at(set: &[(f64, f64); 4], wavelength: f64) -> Result<f64, MaterialError> {
        Sellmeier {
            terms: set.to_vec(),
            range: DAIMON_RANGE,
        }
        .refractive_index(wavelength)
    }

    fn range(&self) -> (f64, f64) {
        (
            self.absorption.range().0.max(DAIMON_RANGE.0),
            DAIMON_RANGE.1,
        )
    }
}

impl Material for Water {
    fn refractive_index(&self, wavelength: f64) -> Result<f64, MaterialError> {
        let range = self.range();
        if !(range.0..=range.1).contains(&wavelength) {
            return Err(MaterialError::OutsideRange { wavelength, range });
        }
        let upper = DAIMON
            .iter()
            .position(|(t, _)| *t >= self.temperature)
            .unwrap_or(DAIMON.len() - 1)
            .max(1);
        let (t0, set0) = &DAIMON[upper - 1];
        let (t1, set1) = &DAIMON[upper];
        let n0 = Self::index_at(set0, wavelength)?;
        let n1 = Self::index_at(set1, wavelength)?;
        let s = (self.temperature - t0) / (t1 - t0);
        Ok(n0 + s * (n1 - n0))
    }

    fn extinction(&self, wavelength: f64) -> Result<f64, MaterialError> {
        let range = self.range();
        if !(range.0..=range.1).contains(&wavelength) {
            return Err(MaterialError::OutsideRange { wavelength, range });
        }
        self.absorption.extinction(wavelength)
    }
}

/// Provenance of the index in [`Water`].
pub const WATER_INDEX: Provenance = Provenance {
    reference: "M. Daimon and A. Masumura, Appl. Opt. 46, 3811-3820 (2007)",
    transcribed_from: "database/data/main/H2O/nk/Daimon-{19.0,20.0,21.5,24.0}C.yml",
    licence: "CC0-1.0",
    verified: false,
};

/// The bundled asset file with Hale & Querry's table.
const HALE_QUERRY: &str = include_str!("../../../assets/materials/water-hale-querry-1973.csv");

/// Liquid water at 25 °C, n and k tabulated from 0.2 to 200 µm (Hale & Querry).
pub fn water_hale_querry() -> Tabulated {
    // The bundled file is checked by this crate's tests, so parsing cannot fail at run
    // time; an empty fallback keeps the function total.
    parse_table(HALE_QUERRY).unwrap_or_else(|_| Tabulated {
        points: vec![(1.0, 1.0, 0.0), (2.0, 1.0, 0.0)],
    })
}

/// Provenance of [`water_hale_querry`] and the extinction in [`Water`].
pub const WATER_ABSORPTION: Provenance = Provenance {
    reference: "G. M. Hale and M. R. Querry, Appl. Opt. 12, 555-563 (1973)",
    transcribed_from: "database/data/main/H2O/nk/Hale.yml",
    licence: "CC0-1.0",
    verified: false,
};

/// Why a table file could not be read.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    /// A data line does not hold three numbers.
    BadLine {
        /// 1-based line number.
        line: usize,
    },
    /// The rows do not form a valid table.
    Table(TableError),
}

/// Reads a `wavelength_um,n,k` table: `#` comment lines, one header line, then rows.
pub fn parse_table(text: &str) -> Result<Tabulated, ParseError> {
    let mut rows = Vec::new();
    let mut header_seen = false;
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !header_seen {
            header_seen = true;
            continue;
        }
        let values: Vec<f64> = line
            .split(',')
            .map(|v| v.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| ParseError::BadLine { line: number + 1 })?;
        let [wavelength, n, k] = values[..] else {
            return Err(ParseError::BadLine { line: number + 1 });
        };
        rows.push((wavelength, n, k));
    }
    Tabulated::new(rows).map_err(ParseError::Table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_table_parses_completely() {
        // 169 rows from 0.2 to 200 µm, first and last rows as in the source file.
        let table = parse_table(HALE_QUERRY).expect("bundled file is valid");
        assert_eq!(table.points.len(), 169);
        assert_eq!(table.range(), (0.2, 200.0));
        assert_eq!(table.points[0], (0.2, 1.396, 1.10e-7));
        assert_eq!(table.points[168], (200.0, 2.130, 0.504));
    }

    #[test]
    fn coefficients_follow_the_database_formulas() {
        // Spot checks that the database's formula types were mapped correctly: formula 1
        // squares Cᵢ, formula 2 does not. Values computed by hand from the coefficients.
        let silica = fused_silica();
        assert!((silica.terms[0].1 - 0.0684043 * 0.0684043).abs() < 1e-18);
        let n = |m: &dyn Material, w: f64| m.refractive_index(w).expect("in range");
        let water = Water::new(20.0).expect("in range");
        // Interpolation reproduces the measured temperature exactly.
        let direct = Water::index_at(&DAIMON[1].1, 0.5893).expect("in range");
        assert!((n(&water, 0.5893) - direct).abs() < 1e-15);
        assert!(Water::new(30.0).is_none());
    }

    #[test]
    fn values_are_physically_consistent() {
        // Not a verification against the papers (they are not yet available): checks
        // that would catch a transcription or formula-type error.
        let n = |m: &dyn Material, w: f64| m.refractive_index(w).expect("in range");
        // Normal dispersion in the visible for all three materials.
        let silica = fused_silica();
        let air = StandardAir;
        let water = Water::new(20.0).expect("in range");
        for m in [&silica as &dyn Material, &air, &water] {
            assert!(n(m, 0.45) > n(m, 0.55) && n(m, 0.55) > n(m, 0.65));
        }
        // Rough magnitudes every optics text agrees on.
        assert!((n(&silica, 0.5876) - 1.458).abs() < 1e-3);
        assert!((n(&air, 0.5876) - 1.000277).abs() < 2e-6);
        assert!((n(&water, 0.5876) - 1.333).abs() < 1e-3);
        // Water index falls as it warms, by roughly 1e-4 per kelvin.
        let warm = Water::new(24.0).expect("in range");
        let slope = (n(&warm, 0.5876) - n(&water, 0.5876)) / 4.0;
        assert!((-2e-4..-5e-5).contains(&slope), "dn/dT = {slope}");
        // Two independent water datasets agree in the visible and near infrared: Daimon
        // (20 °C) against Hale & Querry (25 °C, three decimals), within table rounding plus
        // 5 K of temperature change. Below 0.45 µm they diverge (0.028 at 0.2 µm); see
        // docs/licence-review.md, known differences.
        let hale = water_hale_querry();
        for w in [0.5, 0.6, 0.7, 0.8, 0.9, 1.0] {
            assert!((n(&water, w) - n(&hale, w)).abs() < 2e-3, "λ = {w}");
        }
        // Water absorbs far more in the near infrared than in the blue-green.
        let alpha = |w: f64| water.absorption_coefficient(w).expect("in range");
        assert!(alpha(0.98) > 100.0 * alpha(0.45));
    }
}
