//! Coatings: what fraction of the power an interface reflects, transmits and absorbs.
//!
//! - [`Coating::Uncoated`]: the bare interface, Fresnel's equations.
//! - [`Coating::Ideal`]: a perfect anti-reflection coating (all transmitted).
//! - [`Coating::Stack`]: a thin-film stack, by the characteristic-matrix method (H. A.
//!   Macleod, Thin-Film Optical Filters, 4th ed., ch. 2; Born & Wolf, Principles of
//!   Optics, 7th ed., ch. 1). Layers have complex indices N = n − ik, so absorbing layers
//!   work; s and p are computed separately and averaged for unpolarised light.
//! - [`Coating::Curve`]: measured reflectance and transmittance against wavelength, as
//!   vendors publish for filters, dichroics and beamsplitters. Taken as independent of the
//!   angle of incidence, which holds near the angle the curve was measured at.
//!
//! In every case reflectance + transmittance + absorptance = 1.

use std::sync::Arc;

use optobench_materials::{MaterialError, Medium};
use optobench_math::Complex;

use crate::interaction::fresnel_power;

/// One layer of a thin-film stack.
#[derive(Debug, Clone)]
pub struct Layer {
    /// Physical thickness (µm, like wavelengths).
    pub thickness: f64,
    /// The layer material (its n and k are used).
    pub material: Medium,
}

/// A thin-film stack. Layers are listed from the face's outer side (the side its normal
/// points to, its positive medium) to the substrate (its negative medium).
#[derive(Debug, Clone)]
pub struct ThinFilm {
    /// The layers, outer side first.
    pub layers: Vec<Layer>,
}

/// Measured reflectance and transmittance against vacuum wavelength.
#[derive(Debug, Clone, PartialEq)]
pub struct CoatingCurve {
    /// (wavelength µm, reflectance, transmittance), strictly increasing in wavelength,
    /// R and T in 0..=1 with R + T ≤ 1.
    points: Vec<(f64, f64, f64)>,
}

/// Why a curve was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveError {
    /// Fewer than two points.
    TooShort,
    /// Row `row` is not strictly after the previous one, or its R and T are not valid.
    InvalidRow {
        /// 0-based row.
        row: usize,
    },
}

impl CoatingCurve {
    /// A curve from (wavelength µm, R, T) rows.
    pub fn new(points: Vec<(f64, f64, f64)>) -> Result<Self, CurveError> {
        if points.len() < 2 {
            return Err(CurveError::TooShort);
        }
        for (row, &(w, r, t)) in points.iter().enumerate() {
            let valid = w > 0.0
                && w.is_finite()
                && (0.0..=1.0).contains(&r)
                && (0.0..=1.0).contains(&t)
                && r + t <= 1.0 + 1e-12
                && (row == 0 || w > points[row - 1].0);
            if !valid {
                return Err(CurveError::InvalidRow { row });
            }
        }
        Ok(Self { points })
    }

    /// (R, T) at a wavelength, linearly interpolated; outside the curve, an error.
    fn at(&self, wavelength: f64) -> Result<(f64, f64), MaterialError> {
        let range = (self.points[0].0, self.points[self.points.len() - 1].0);
        if !(range.0..=range.1).contains(&wavelength) {
            return Err(MaterialError::OutsideRange { wavelength, range });
        }
        let upper = self.points.partition_point(|p| p.0 < wavelength).max(1);
        let (w0, r0, t0) = self.points[upper - 1];
        let (w1, r1, t1) = self.points[upper];
        let s = (wavelength - w0) / (w1 - w0);
        Ok((r0 + s * (r1 - r0), t0 + s * (t1 - t0)))
    }
}

/// The coating on an interface.
#[derive(Debug, Clone, Default)]
pub enum Coating {
    /// Bare interface: Fresnel reflection.
    #[default]
    Uncoated,
    /// A perfect anti-reflection coating: no reflection.
    Ideal,
    /// A thin-film stack.
    Stack(Arc<ThinFilm>),
    /// A measured R/T curve.
    Curve(Arc<CoatingCurve>),
}

impl PartialEq for Coating {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Coating::Uncoated, Coating::Uncoated) | (Coating::Ideal, Coating::Ideal) => true,
            (Coating::Stack(a), Coating::Stack(b)) => Arc::ptr_eq(a, b),
            (Coating::Curve(a), Coating::Curve(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// Fractions of the incident power (unpolarised).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Response {
    /// Reflected.
    pub reflectance: f64,
    /// Transmitted.
    pub transmittance: f64,
}

impl Response {
    /// Absorbed in the coating: 1 − R − T.
    pub fn absorptance(&self) -> f64 {
        (1.0 - self.reflectance - self.transmittance).max(0.0)
    }
}

/// The incidence, for a coating's response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Incidence {
    /// Vacuum wavelength (µm).
    pub wavelength: f64,
    /// cos θ of the angle of incidence (0 to 1).
    pub cos_incidence: f64,
    /// Index of the medium the light comes from.
    pub index_from: f64,
    /// Index of the medium the light goes into.
    pub index_to: f64,
    /// Whether the light comes from the face's outer (positive) side, so the stack is met
    /// in its listed order.
    pub from_outside: bool,
}

/// R and T of a thin-film stack for one polarisation.
fn stack_one(layers: &[(Complex, f64)], incidence: &Incidence, p_polarised: bool) -> (f64, f64) {
    let n0 = incidence.index_from;
    let sin0 = (1.0 - incidence.cos_incidence * incidence.cos_incidence)
        .max(0.0)
        .sqrt();
    let invariant = Complex::real(n0 * sin0); // n sin θ, the same in every layer (Snell)
    // cos θ in a medium of index N, on the branch where the wave decays or propagates
    // forward (Im(N cos θ) ≤ 0 with N = n − ik; Macleod, ch. 2).
    let cosine = |n: Complex| {
        let s = invariant / n;
        let mut c = (Complex::ONE - s * s).sqrt();
        if (n * c).im > 0.0 {
            c = -c;
        }
        c
    };
    let admittance = |n: Complex| {
        let c = cosine(n);
        if p_polarised { n / c } else { n * c }
    };
    let eta0 = admittance(Complex::real(n0));
    let eta_s = admittance(Complex::real(incidence.index_to));
    // [B; C] = Π Mⱼ · [1; η_s], Mⱼ = [[cos δ, i sin δ / η], [i η sin δ, cos δ]].
    let (mut b, mut c) = (Complex::ONE, eta_s);
    for &(n, thickness) in layers.iter().rev() {
        let eta = admittance(n);
        let delta = n * cosine(n) * (core::f64::consts::TAU * thickness / incidence.wavelength);
        let (cos_d, sin_d) = (delta.cos(), delta.sin());
        let i_sin = Complex::I * sin_d;
        let (nb, nc) = (cos_d * b + (i_sin / eta) * c, (i_sin * eta) * b + cos_d * c);
        b = nb;
        c = nc;
    }
    let denominator = eta0 * b + c;
    let r = (eta0 * b - c) / denominator;
    let reflectance = r.norm_squared();
    let transmittance = 4.0 * eta0.re * eta_s.re / denominator.norm_squared();
    (reflectance, transmittance.max(0.0))
}

impl Coating {
    /// The same coating seen from its other side: a stack's layers in reverse order.
    pub fn turned(&self) -> Coating {
        match self {
            Coating::Stack(film) => Coating::Stack(Arc::new(ThinFilm {
                layers: film.layers.iter().rev().cloned().collect(),
            })),
            other => other.clone(),
        }
    }

    /// The coating's response to light arriving as described.
    pub fn response(&self, incidence: &Incidence) -> Result<Response, MaterialError> {
        Ok(match self {
            Coating::Uncoated => {
                let f = fresnel_power(
                    incidence.cos_incidence,
                    incidence.index_from,
                    incidence.index_to,
                );
                Response {
                    reflectance: f.unpolarised_reflectance(),
                    transmittance: f.unpolarised_transmittance(),
                }
            }
            Coating::Ideal => Response {
                reflectance: 0.0,
                transmittance: 1.0,
            },
            Coating::Curve(curve) => {
                let (r, t) = curve.at(incidence.wavelength)?;
                Response {
                    reflectance: r,
                    transmittance: t,
                }
            }
            Coating::Stack(film) => {
                let mut layers = film
                    .layers
                    .iter()
                    .map(|layer| {
                        let m = layer.material.material();
                        let n = m.refractive_index(incidence.wavelength)?;
                        let k = m.extinction(incidence.wavelength)?;
                        Ok((Complex::new(n, -k), layer.thickness))
                    })
                    .collect::<Result<Vec<_>, MaterialError>>()?;
                if !incidence.from_outside {
                    layers.reverse();
                }
                let (rs, ts) = stack_one(&layers, incidence, false);
                let (rp, tp) = stack_one(&layers, incidence, true);
                Response {
                    reflectance: 0.5 * (rs + rp),
                    transmittance: 0.5 * (ts + tp),
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use optobench_materials::Tabulated;

    fn normal(n_from: f64, n_to: f64) -> Incidence {
        Incidence {
            wavelength: 0.55,
            cos_incidence: 1.0,
            index_from: n_from,
            index_to: n_to,
            from_outside: true,
        }
    }

    fn stack(layers: Vec<(f64, f64)>) -> Coating {
        Coating::Stack(Arc::new(ThinFilm {
            layers: layers
                .into_iter()
                .map(|(n, d)| Layer {
                    thickness: d,
                    material: n.into(),
                })
                .collect(),
        }))
    }

    #[test]
    fn empty_stack_equals_the_bare_interface() {
        for (n1, n2) in [(1.0, 1.5), (1.5, 1.0), (1.33, 1.7)] {
            for degrees in [0.0_f64, 20.0, 40.0, 60.0, 80.0] {
                let incidence = Incidence {
                    cos_incidence: degrees.to_radians().cos(),
                    ..normal(n1, n2)
                };
                if n1 > n2 && degrees.to_radians().sin() * n1 / n2 >= 1.0 {
                    continue;
                }
                let bare = Coating::Uncoated.response(&incidence).expect("defined");
                let film = stack(vec![]).response(&incidence).expect("defined");
                assert!(
                    (bare.reflectance - film.reflectance).abs() < 1e-12,
                    "{n1}→{n2} at {degrees}°"
                );
                assert!((bare.transmittance - film.transmittance).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn quarter_wave_layer_follows_the_closed_form() {
        // A quarter-wave layer of index n₁ on a substrate nₛ at normal incidence reflects
        // R = ((n₀nₛ − n₁²)/(n₀nₛ + n₁²))² (Macleod, Thin-Film Optical Filters, ch. 3):
        // MgF₂ (1.38) on crown glass (1.52) in air gives the classic 1.26 %.
        let (n1, ns) = (1.38, 1.52);
        let quarter = 0.55 / (4.0 * n1);
        let r = stack(vec![(n1, quarter)])
            .response(&normal(1.0, ns))
            .expect("defined");
        let expected = ((ns - n1 * n1) / (ns + n1 * n1)).powi(2);
        assert!((r.reflectance - expected).abs() < 1e-12);
        assert!((expected - 0.0126).abs() < 1e-4);
        assert!(
            (r.reflectance + r.transmittance - 1.0).abs() < 1e-12,
            "lossless"
        );
    }

    #[test]
    fn half_wave_layer_is_absent() {
        // A half-wave layer changes nothing at its design wavelength (an absentee layer;
        // Macleod, ch. 2).
        let half = 0.55 / (2.0 * 2.1);
        let with = stack(vec![(2.1, half)])
            .response(&normal(1.0, 1.52))
            .expect("defined");
        let bare = Coating::Uncoated
            .response(&normal(1.0, 1.52))
            .expect("defined");
        assert!((with.reflectance - bare.reflectance).abs() < 1e-12);
    }

    #[test]
    fn quarter_wave_mirror_follows_the_closed_form() {
        // (HL)ᴺ H at normal incidence: admittance Y = (n_H/n_L)²ᴺ n_H²/nₛ, R = ((n₀ − Y)/(n₀ + Y))²
        // (Macleod, ch. 6).
        let (nh, nl, ns, pairs) = (2.35, 1.38, 1.52, 5);
        let mut layers = Vec::new();
        for _ in 0..pairs {
            layers.push((nh, 0.55 / (4.0 * nh)));
            layers.push((nl, 0.55 / (4.0 * nl)));
        }
        layers.push((nh, 0.55 / (4.0 * nh)));
        let r = stack(layers).response(&normal(1.0, ns)).expect("defined");
        let y = (nh / nl).powi(2 * pairs) * nh * nh / ns;
        let expected = ((1.0 - y) / (1.0 + y)).powi(2);
        assert!(
            (r.reflectance - expected).abs() < 1e-10,
            "{} vs {expected}",
            r.reflectance
        );
        assert!(r.reflectance > 0.99);
    }

    #[test]
    fn absorbing_layer_conserves_power_and_transmits_alike_both_ways() {
        // A thin metal-like layer (n = 0.5, k = 3, 20 nm) between air and glass: R + T + A = 1
        // with A > 0, and transmittance is the same in both directions (reciprocity;
        // Macleod, ch. 2), while reflectance differs.
        let metal = Tabulated::new(vec![(0.4, 0.5, 3.0), (0.7, 0.5, 3.0)]).expect("valid");
        let film = Coating::Stack(Arc::new(ThinFilm {
            layers: vec![Layer {
                thickness: 0.02,
                material: metal.into(),
            }],
        }));
        let forward = film.response(&normal(1.0, 1.52)).expect("defined");
        let back = film
            .response(&Incidence {
                from_outside: false,
                ..normal(1.52, 1.0)
            })
            .expect("defined");
        assert!(forward.absorptance() > 0.01);
        assert!(
            (forward.reflectance + forward.transmittance + forward.absorptance() - 1.0).abs()
                < 1e-12
        );
        assert!((forward.transmittance - back.transmittance).abs() < 1e-12);
        assert!((forward.reflectance - back.reflectance).abs() > 1e-3);
    }

    #[test]
    fn curve_interpolates_and_rejects_bad_rows() {
        let curve = CoatingCurve::new(vec![(0.5, 0.9, 0.1), (0.6, 0.1, 0.9)]).expect("valid");
        let coating = Coating::Curve(Arc::new(curve));
        let at = |w: f64| {
            coating.response(&Incidence {
                wavelength: w,
                ..normal(1.0, 1.5)
            })
        };
        let mid = at(0.55).expect("in range");
        assert!((mid.reflectance - 0.5).abs() < 1e-12 && (mid.transmittance - 0.5).abs() < 1e-12);
        assert!(at(0.7).is_err());
        assert_eq!(
            CoatingCurve::new(vec![(0.5, 0.9, 0.2), (0.6, 0.1, 0.9)]),
            Err(CurveError::InvalidRow { row: 0 })
        );
    }
}
