//! Sampled optical fields and their propagation (M10b; docs/design.md, Optical model,
//! Field propagation).
//!
//! Layer L4. A [`Field`] is a complex amplitude per polarization component (x, y of its
//! plane) on an N × N grid of spacing Δ, centred on the axis: sample (r, c) sits at
//! x = (c − N/2) Δ, y = (r − N/2) Δ. Its units make n |E|² an irradiance in W/mm², so the
//! power is Σ n |E|² Δ². A field may carry its mean wavefront curvature c = 1/R
//! analytically: the field is the stored samples times exp(i k (x² + y²) c / 2), so a
//! converging or diverging beam is sampled on what is left once its sphere is taken out
//! (A. E. Siegman, Lasers, 1986, ch. 20; the "spherical reference").
//!
//! Propagation over a distance z in a homogeneous medium (wavelength λ = λ₀ / n):
//!
//! - **Expanding coordinates.** Fresnel's integral of a field with curvature c over z
//!   equals that of its stored samples over z' = z / M in coordinates scaled by
//!   M = 1 + z c, divided by M, on a sphere of curvature c / M. Proof: with
//!   U₁ = u₁ e^{ik c x₁²/2}, the exponent k/(2z) [(x₂ − x₁)² + c z x₁²] is
//!   k c x₂² / (2M) + k M (x₁ − x₂/M)² / (2z). So propagation below is of flat fields.
//! - **Angular spectrum**, exact for scalar fields: the transform of the samples times
//!   exp(i z √(k² − k_x² − k_y²)), evanescent waves dropped (J. W. Goodman, Introduction to
//!   Fourier Optics, 3rd ed., §3.10). The grid stays; the transfer function is sampled
//!   well while the grid's Fresnel number N Δ² / (λ |z|) is at least 1 (D. G. Voelz,
//!   Computational Fourier Optics, SPIE 2011, §5.1).
//! - **Fresnel, one transform**, for longer distances (Goodman §4.2): u₂(x₂) =
//!   e^{ikz} / (iλz) · e^{ik x₂²/(2z)} ∫ u₁ e^{ik x₁²/(2z)} e^{−ik x₁x₂/z} dA, on a new grid
//!   of spacing λ |z| / (N Δ); the outgoing sphere e^{ik x₂²/(2z)} becomes the field's
//!   curvature, so it need not be sampled. The incoming chirp is sampled well exactly
//!   when the grid's Fresnel number is below 1, so the two methods meet.
//! - At a focus (M near 0) the incoming sphere and the propagation chirp cancel and the
//!   one-transform method is used directly: a lens and its focal distance give the far
//!   field, sampled at λ f / (N Δ).
//!
//! Every method conserves Σ n |E|² dA (checked in the tests). Phase screens (a thin lens,
//! Zernike terms) and apertures with exact edges act in a plane.

#![forbid(unsafe_code)]

pub mod aperture;
pub mod rays;
pub mod scene;
pub mod transform;

use optobench_analysis::colour::ColourScale;
use optobench_analysis::graph::{Axis, Graph, Map, Plot, Unit};
use optobench_math::Complex;

pub use aperture::Aperture;
use transform::{Direction, Transform, shift};

/// Why a field could not be made or propagated.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldError {
    /// The grid is not usable (size odd or too small, spacing not positive).
    Grid(String),
    /// The samples are too coarse for the field's phase.
    Undersampled(String),
}

impl core::fmt::Display for FieldError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FieldError::Grid(m) | FieldError::Undersampled(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for FieldError {}

/// Which of a field's own axes a plane is tilted about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TiltAxis {
    /// About x: y′ leans towards z.
    X,
    /// About y: x′ leans towards z.
    Y,
}

/// How a propagation was done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// The angular spectrum on the same grid.
    AngularSpectrum,
    /// Fresnel's integral by one transform, onto a new grid.
    Fresnel,
}

/// What a propagation did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Report {
    /// The method.
    pub method: Method,
    /// The distance it propagated the stored samples (z / M), mm.
    pub effective_distance: f64,
    /// The grid's Fresnel number N Δ² / (λ |z / M|) for that distance.
    pub fresnel_number: f64,
    /// The fraction of the power in the outer sixteenth of the grid afterwards: light
    /// near the edge may have wrapped round (pad the field if this is not small).
    pub edge_fraction: f64,
}

/// A sampled field.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    n: usize,
    spacing: f64,
    wavelength: f64,
    index: f64,
    curvature: f64,
    tilt: (f64, f64),
    components: [Vec<Complex>; 2],
}

/// The phase step between neighbouring samples that counts as too coarse (radians).
const COARSEST: f64 = core::f64::consts::FRAC_PI_2;

impl Field {
    /// A dark field: an N × N grid (N even, at least 4) of `spacing` mm, at vacuum
    /// wavelength `wavelength` µm in a medium of index `index`.
    pub fn new(n: usize, spacing: f64, wavelength: f64, index: f64) -> Result<Self, FieldError> {
        if n < 4 || n % 2 != 0 {
            return Err(FieldError::Grid(format!(
                "a field grid is N × N with N even and at least 4, not {n}"
            )));
        }
        if !(spacing > 0.0 && wavelength > 0.0 && index > 0.0) {
            return Err(FieldError::Grid(
                "spacing, wavelength and index must be positive".into(),
            ));
        }
        Ok(Self {
            n,
            spacing,
            wavelength,
            index,
            curvature: 0.0,
            tilt: (0.0, 0.0),
            components: [vec![Complex::ZERO; n * n], vec![Complex::ZERO; n * n]],
        })
    }

    /// A plane wave of irradiance `irradiance` (W/mm²) polarized along the Jones vector
    /// `polarization` (normalised here).
    pub fn plane(
        n: usize,
        spacing: f64,
        wavelength: f64,
        index: f64,
        irradiance: f64,
        polarization: (Complex, Complex),
    ) -> Result<Self, FieldError> {
        let mut field = Self::new(n, spacing, wavelength, index)?;
        let (jx, jy) = unit(polarization);
        let amplitude = (irradiance / index).sqrt();
        field.components[0].fill(jx * amplitude);
        field.components[1].fill(jy * amplitude);
        Ok(field)
    }

    /// A TEM₀₀ Gaussian beam of 1/e² waist radius `waist` (mm) and power `power` (W),
    /// `distance` (mm) past its waist, exactly (paraxially): radius w(z) = w₀ √(1 +
    /// (z/z_R)²), curvature z / (z² + z_R²), Gouy phase −atan(z/z_R) and the carrier
    /// e^{ikz} (Siegman, Lasers, §17.1).
    #[allow(clippy::too_many_arguments)]
    pub fn gaussian(
        n: usize,
        spacing: f64,
        wavelength: f64,
        index: f64,
        waist: f64,
        distance: f64,
        power: f64,
        polarization: (Complex, Complex),
    ) -> Result<Self, FieldError> {
        let mut field = Self::new(n, spacing, wavelength, index)?;
        let lambda = field.medium_wavelength();
        let rayleigh = core::f64::consts::PI * waist * waist / lambda;
        let z = distance;
        let w = waist * (1.0 + (z / rayleigh).powi(2)).sqrt();
        field.curvature = z / (z * z + rayleigh * rayleigh);
        let peak = (2.0 * power / (core::f64::consts::PI * waist * waist * index)).sqrt();
        let phase = field.k() * z - (z / rayleigh).atan();
        let carrier = Complex::new(phase.cos(), phase.sin()) * (peak * waist / w);
        let (jx, jy) = unit(polarization);
        for r in 0..n {
            for c in 0..n {
                let (x, y) = (field.position(c), field.position(r));
                let a = (-(x * x + y * y) / (w * w)).exp();
                field.components[0][r * n + c] = carrier * jx * a;
                field.components[1][r * n + c] = carrier * jy * a;
            }
        }
        Ok(field)
    }

    /// Samples per side.
    pub fn size(&self) -> usize {
        self.n
    }

    /// Sample spacing (mm).
    pub fn spacing(&self) -> f64 {
        self.spacing
    }

    /// Vacuum wavelength (µm).
    pub fn wavelength(&self) -> f64 {
        self.wavelength
    }

    /// The medium's index.
    pub fn index(&self) -> f64 {
        self.index
    }

    /// The reference sphere's curvature 1/R (1/mm): positive diverging.
    pub fn curvature(&self) -> f64 {
        self.curvature
    }

    /// The direction cosines (x, y) of the plane wave carried with the samples: the field
    /// is the samples times exp(i k (sₓ x + s_y y)) besides the sphere. Zero but on a
    /// tilted plane ([`Field::tilted`]), where a beam meets the plane obliquely.
    pub fn tilt(&self) -> (f64, f64) {
        self.tilt
    }

    /// The wavelength in the medium (mm).
    pub fn medium_wavelength(&self) -> f64 {
        self.wavelength * 1e-3 / self.index
    }

    /// The wavenumber in the medium (1/mm).
    pub fn k(&self) -> f64 {
        2.0 * core::f64::consts::PI / self.medium_wavelength()
    }

    /// The coordinate of column (or row) `i` (mm).
    pub fn position(&self, i: usize) -> f64 {
        (i as f64 - (self.n / 2) as f64) * self.spacing
    }

    /// The stored samples of component 0 (x) or 1 (y), row by row; the field is these
    /// times exp(i k (x² + y²) c / 2).
    pub fn samples(&self, component: usize) -> &[Complex] {
        &self.components[component]
    }

    /// The field itself, sphere included, at row `r`, column `c`.
    pub fn value(&self, component: usize, r: usize, c: usize) -> Complex {
        let (x, y) = (self.position(c), self.position(r));
        let phase =
            self.k() * ((x * x + y * y) * self.curvature / 2.0 + self.tilt.0 * x + self.tilt.1 * y);
        self.components[component][r * self.n + c] * Complex::new(phase.cos(), phase.sin())
    }

    /// The irradiance on the plane at row `r`, column `c` (W/mm²): n |E|², times the
    /// obliquity √(1 − sₓ² − s_y²) on a tilted plane, where the beam meets it at an angle
    /// and its power spreads over more of it.
    pub fn irradiance(&self, r: usize, c: usize) -> f64 {
        let i = r * self.n + c;
        self.index
            * self.obliquity()
            * (self.components[0][i].norm_squared() + self.components[1][i].norm_squared())
    }

    /// √(1 − sₓ² − s_y²): 1 but on a tilted plane.
    fn obliquity(&self) -> f64 {
        (1.0 - self.tilt.0 * self.tilt.0 - self.tilt.1 * self.tilt.1)
            .max(0.0)
            .sqrt()
    }

    /// The power through the plane, Σ irradiance Δ² (W).
    pub fn power(&self) -> f64 {
        let sum: f64 = (0..self.n * self.n)
            .map(|i| self.components[0][i].norm_squared() + self.components[1][i].norm_squared())
            .sum();
        self.index * self.obliquity() * sum * self.spacing * self.spacing
    }

    /// The beam's radius per axis from its second moments, 2σ (mm): the 1/e² radius of a
    /// Gaussian beam (ISO 11146's D4σ / 2).
    pub fn second_moment_radius(&self) -> (f64, f64) {
        let (mut total, mut sx, mut sy, mut sxx, mut syy) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for r in 0..self.n {
            for c in 0..self.n {
                let w = self.irradiance(r, c);
                let (x, y) = (self.position(c), self.position(r));
                total += w;
                sx += w * x;
                sy += w * y;
                sxx += w * x * x;
                syy += w * y * y;
            }
        }
        if total <= 0.0 {
            return (0.0, 0.0);
        }
        let (mx, my) = (sx / total, sy / total);
        (
            2.0 * (sxx / total - mx * mx).max(0.0).sqrt(),
            2.0 * (syy / total - my * my).max(0.0).sqrt(),
        )
    }

    /// Multiplies both components by `f(x, y)`.
    pub fn modulate(&mut self, f: impl Fn(f64, f64) -> Complex) {
        for r in 0..self.n {
            for c in 0..self.n {
                let m = f(self.position(c), self.position(r));
                let i = r * self.n + c;
                self.components[0][i] = self.components[0][i] * m;
                self.components[1][i] = self.components[1][i] * m;
            }
        }
    }

    /// Passes the field through an aperture with exact edges: each sample stands for the
    /// field's mean over its cell, so a cell an edge crosses keeps its open fraction of
    /// it. Diffraction integrals (Σ E ΔA) then see the exact open area; the power of an
    /// edge cell is undercounted by that fraction squared over the fraction, a small
    /// effect of the grid's sampling.
    pub fn apply(&mut self, aperture: &Aperture) {
        let size = self.spacing;
        self.modulate(|x, y| Complex::real(aperture.transmission(x, y, size)));
    }

    /// A perfect thin lens of focal length `focal_length` (mm, in this medium): the
    /// sphere's curvature drops by 1/f, nothing to sample.
    pub fn lens(&mut self, focal_length: f64) {
        self.curvature -= 1.0 / focal_length;
    }

    /// A wavefront error of Zernike terms (ANSI index j, coefficient in waves, each term's
    /// RMS) over a disk of `radius` (mm) about the axis; outside it nothing changes.
    pub fn zernike(&mut self, terms: &[(usize, f64)], radius: f64) {
        self.modulate(|x, y| {
            let (u, v) = (x / radius, y / radius);
            if u.hypot(v) > 1.0 {
                return Complex::ONE;
            }
            let waves: f64 = terms
                .iter()
                .map(|&(j, a)| a * optobench_analysis::zernike(j, u, v))
                .sum();
            let phase = 2.0 * core::f64::consts::PI * waves;
            Complex::new(phase.cos(), phase.sin())
        });
    }

    /// The field on a grid `factor` times as wide, same spacing, dark around it.
    pub fn padded(&self, factor: usize) -> Self {
        let big = self.n * factor.max(1);
        let offset = big / 2 - self.n / 2;
        let mut out = Self {
            n: big,
            components: [
                vec![Complex::ZERO; big * big],
                vec![Complex::ZERO; big * big],
            ],
            ..self.clone()
        };
        for k in 0..2 {
            for r in 0..self.n {
                for c in 0..self.n {
                    out.components[k][(r + offset) * big + c + offset] =
                        self.components[k][r * self.n + c];
                }
            }
        }
        out
    }

    /// The phase steps between neighbouring stored samples that are both clearly lit (a
    /// hundredth of the peak irradiance or more): the largest (radians), and the share of
    /// the lit power at steps beyond π/2. Between dimmer samples the phase may jump by π
    /// across a zero of the field (a dark ring), and by a hard edge a little light reaches
    /// the grid's finest frequencies: that is the field. A smooth phase too steep for the
    /// grid (a lens written into the samples) makes most steps coarse.
    pub fn phase_steps(&self) -> (f64, f64) {
        let n = self.n;
        let weight =
            |i: usize| self.components[0][i].norm_squared() + self.components[1][i].norm_squared();
        let peak = (0..n * n).map(weight).fold(0.0, f64::max);
        let lit = |i: usize| weight(i) >= 1e-2 * peak;
        let (mut largest, mut coarse, mut total) = (0.0f64, 0.0, 0.0);
        for s in &self.components {
            for r in 0..n {
                for c in 0..n {
                    let i = r * n + c;
                    if !lit(i) || s[i].norm_squared() == 0.0 {
                        continue;
                    }
                    for j in [i + 1, i + n] {
                        let neighbour = (j < n * n && (j != i + 1 || c + 1 < n)).then_some(j);
                        if let Some(j) = neighbour.filter(|&j| lit(j) && s[j].norm_squared() > 0.0)
                        {
                            let step = s[i] * s[j].conjugate();
                            let size = step.im.atan2(step.re).abs();
                            largest = largest.max(size);
                            let w = weight(i) + weight(j);
                            total += w;
                            if size > COARSEST {
                                coarse += w;
                            }
                        }
                    }
                }
            }
        }
        (largest, if total > 0.0 { coarse / total } else { 0.0 })
    }

    /// Refuses a field whose phase is too steep for its grid (see [`Field::phase_steps`]).
    fn check_sampling(&self, what: &str) -> Result<(), FieldError> {
        let (largest, share) = self.phase_steps();
        if share > 0.02 {
            return Err(FieldError::Undersampled(format!(
                "{what}: the phase steps by up to {largest:.2} rad between neighbouring samples, \
                 beyond {COARSEST:.2} for {:.0} % of the lit field; use a finer grid, or carry the \
                 curvature as a reference sphere",
                share * 100.0
            )));
        }
        Ok(())
    }

    /// Propagates the field over `z` (mm, either way), choosing the method from the grid's
    /// Fresnel number.
    pub fn propagate(&mut self, z: f64) -> Result<Report, FieldError> {
        self.propagate_with(z, None)
    }

    /// As [`Field::propagate`], with the method of the stored samples' propagation given
    /// (`None`: chosen). Forcing the angular spectrum suits periodic fields (a grating)
    /// whose spectrum has only a few lines, whatever the distance.
    pub fn propagate_with(&mut self, z: f64, method: Option<Method>) -> Result<Report, FieldError> {
        if self.tilt != (0.0, 0.0) {
            return Err(FieldError::Grid(
                "a field on a tilted plane is an end point: graph it, couple it or turn it into rays".into(),
            ));
        }
        self.check_sampling("the field")?;
        if z == 0.0 {
            return Ok(Report {
                method: Method::AngularSpectrum,
                effective_distance: 0.0,
                fresnel_number: f64::INFINITY,
                edge_fraction: self.edge_fraction(),
            });
        }
        let lambda = self.medium_wavelength();
        let k = self.k();
        let c = self.curvature;
        let m = 1.0 + z * c;
        let grid_area = self.n as f64 * self.spacing * self.spacing;
        // Through a focus: propagate directly with the incoming sphere in the chirp.
        if m.abs() < 1e-6 {
            let chirp = c + 1.0 / z;
            let reach = k * chirp.abs() * (self.n as f64 / 2.0) * self.spacing * self.spacing;
            if reach > COARSEST {
                return Err(FieldError::Undersampled(format!(
                    "near the focus the remaining sphere changes the phase by {reach:.2} rad between samples"
                )));
            }
            self.fresnel(z, chirp);
            return Ok(Report {
                method: Method::Fresnel,
                effective_distance: z,
                fresnel_number: grid_area / (lambda * z.abs()),
                edge_fraction: self.edge_fraction(),
            });
        }
        let effective = z / m;
        let fresnel_number = grid_area / (lambda * effective.abs());
        let method = method.unwrap_or(if fresnel_number >= 1.0 {
            Method::AngularSpectrum
        } else {
            Method::Fresnel
        });
        self.curvature = 0.0;
        match method {
            Method::AngularSpectrum => self.angular_spectrum(effective),
            Method::Fresnel => self.fresnel(effective, 1.0 / effective),
        }
        // Back from expanding coordinates: scale by M, amplitude / M, carrier for the
        // rest of the distance, sphere c / M plus the inner one seen at the new scale.
        let inner = self.curvature;
        self.curvature = c / m + inner / (m * m);
        self.spacing *= m.abs();
        let rest = k * (z - effective);
        let factor = Complex::new(rest.cos(), rest.sin()) * (1.0 / m);
        for component in &mut self.components {
            for v in component.iter_mut() {
                *v = *v * factor;
            }
        }
        if m < 0.0 {
            // Past a focus the image is inverted: x → −x about the centre sample.
            self.invert();
        }
        Ok(Report {
            method,
            effective_distance: effective,
            fresnel_number,
            edge_fraction: self.edge_fraction(),
        })
    }

    /// The stored samples propagated over `z` by the angular spectrum (same grid, flat).
    fn angular_spectrum(&mut self, z: f64) {
        let n = self.n;
        let k = self.k();
        let transform = Transform::new(n);
        let frequency = |p: usize| {
            let p = if p < n / 2 {
                p as f64
            } else {
                p as f64 - n as f64
            };
            2.0 * core::f64::consts::PI * p / (n as f64 * self.spacing)
        };
        for component in &mut self.components {
            if component.iter().all(|v| *v == Complex::ZERO) {
                continue;
            }
            transform.apply(component, Direction::Forward);
            for r in 0..n {
                let ky = frequency(r);
                for c in 0..n {
                    let kx = frequency(c);
                    let kz2 = k * k - kx * kx - ky * ky;
                    let i = r * n + c;
                    component[i] = if kz2 > 0.0 {
                        let phase = z * kz2.sqrt();
                        component[i] * Complex::new(phase.cos(), phase.sin())
                    } else {
                        Complex::ZERO // evanescent
                    };
                }
            }
            transform.apply(component, Direction::Inverse);
        }
    }

    /// The stored samples, times the incoming chirp exp(i k chirp r² / 2), propagated over
    /// `z` by Fresnel's integral in one transform: a new grid of spacing λ |z| / (N Δ) and
    /// the outgoing sphere 1/z as the field's curvature.
    fn fresnel(&mut self, z: f64, chirp: f64) {
        let n = self.n;
        let k = self.k();
        let lambda = self.medium_wavelength();
        let transform = Transform::new(n);
        let dx = self.spacing;
        let incoming: Vec<Complex> = (0..n * n)
            .map(|i| {
                let (x, y) = (self.position(i % n), self.position(i / n));
                let phase = k * chirp * (x * x + y * y) / 2.0;
                Complex::new(phase.cos(), phase.sin())
            })
            .collect();
        // e^{ikz} / (iλz) · Δ²; the inverse transform for z < 0 (the kernel's sign turns),
        // without its 1/N².
        let (direction, scale) = if z > 0.0 {
            (Direction::Forward, 1.0)
        } else {
            (Direction::Inverse, (n * n) as f64)
        };
        let carrier = k * z;
        let front = Complex::new(carrier.cos(), carrier.sin())
            * Complex::new(0.0, -1.0 / (lambda * z))
            * (dx * dx * scale);
        for component in &mut self.components {
            if component.iter().all(|v| *v == Complex::ZERO) {
                continue;
            }
            for (v, w) in component.iter_mut().zip(&incoming) {
                *v = *v * *w;
            }
            shift(component, n);
            transform.apply(component, direction);
            shift(component, n);
            for v in component.iter_mut() {
                *v = *v * front;
            }
        }
        self.spacing = lambda * z.abs() / (n as f64 * dx);
        self.curvature = 1.0 / z;
    }

    /// Mirrors the grid through its centre sample (x → −x, y → −y).
    fn invert(&mut self) {
        let n = self.n;
        for component in &mut self.components {
            let old = component.clone();
            for r in 0..n {
                for c in 0..n {
                    let (rr, cc) = ((n - r) % n, (n - c) % n);
                    component[r * n + c] = old[rr * n + cc];
                }
            }
        }
    }

    /// The fraction of the power in the outer sixteenth of the grid.
    pub fn edge_fraction(&self) -> f64 {
        let n = self.n;
        let band = (n / 16).max(1);
        let (mut edge, mut total) = (0.0, 0.0);
        for r in 0..n {
            for c in 0..n {
                let w = self.irradiance(r, c);
                total += w;
                if r < band || c < band || r >= n - band || c >= n - band {
                    edge += w;
                }
            }
        }
        if total > 0.0 { edge / total } else { 0.0 }
    }

    /// The field on a plane through the grid's centre tilted by `angle` (radians) about
    /// the field's own `axis` (x or y): the target plane's x′ (or y′) runs along
    /// (cos α, 0, sin α) (or (0, cos α, sin α)). Every sample is the angular spectrum summed
    /// exactly at its point on the tilted plane (each plane wave propagated to its height
    /// there), so nothing is interpolated: the plane-wave rotation of K. Matsushima,
    /// H. Schimmel and F. Wyrowski (J. Opt. Soc. Am. A 20, 1755 (2003)) evaluated directly,
    /// N³ operations. The oblique incidence becomes the field's tilt carrier, so the samples
    /// stay smooth. Components keep their x and y (scalar diffraction). The sphere must be
    /// sampled on the grid: tilt near a waist, or on a finer grid.
    pub fn tilted(&self, angle: f64, axis: TiltAxis) -> Result<Field, FieldError> {
        if self.tilt != (0.0, 0.0) {
            return Err(FieldError::Grid(
                "the field is already on a tilted plane".into(),
            ));
        }
        // About x: the same as about y with x and y swapped.
        if axis == TiltAxis::X {
            let mut swapped = self.transposed();
            swapped = swapped.tilted(angle, TiltAxis::Y)?;
            return Ok(swapped.transposed());
        }
        let n = self.n;
        let k = self.k();
        let dx = self.spacing;
        // The whole field, sphere included, sampled.
        let mut full = self.clone();
        full.curvature = 0.0;
        for (comp, samples) in full.components.iter_mut().enumerate() {
            for r in 0..n {
                for c in 0..n {
                    samples[r * n + c] = self.value(comp, r, c);
                }
            }
        }
        full.check_sampling(
            "with its sphere, to tilt it (tilt nearer a waist, or on a finer grid)",
        )?;
        let transform = Transform::new(n);
        let frequency = |p: usize| {
            let p = if p < n / 2 {
                p as f64
            } else {
                p as f64 - n as f64
            };
            2.0 * core::f64::consts::PI * p / (n as f64 * dx)
        };
        let x0 = self.position(0);
        let (cos, sin) = (angle.cos(), angle.sin());
        let mut out = Self {
            curvature: 0.0,
            tilt: (sin, 0.0),
            components: [vec![Complex::ZERO; n * n], vec![Complex::ZERO; n * n]],
            ..self.clone()
        };
        for comp in 0..2 {
            let mut spectrum = full.components[comp].clone();
            if spectrum.iter().all(|v| *v == Complex::ZERO) {
                continue;
            }
            transform.apply(&mut spectrum, Direction::Forward);
            for c in 0..n {
                let xp = self.position(c);
                let (x, z) = (xp * cos, xp * sin);
                // Over kx for this column, then over ky by one 1-D transform.
                let mut column = vec![Complex::ZERO; n];
                for (q, b) in column.iter_mut().enumerate() {
                    let ky = frequency(q);
                    let mut sum = Complex::ZERO;
                    for p in 0..n {
                        let kx = frequency(p);
                        let kz2 = k * k - kx * kx - ky * ky;
                        if kz2 <= 0.0 {
                            continue;
                        }
                        let phase = kx * (x - x0) + kz2.sqrt() * z;
                        sum = sum + spectrum[q * n + p] * Complex::new(phase.cos(), phase.sin());
                    }
                    *b = sum;
                }
                transform::inverse_line(&mut column);
                // Less the tilt carrier along x′, and the inverse DFT's 1/N².
                let carrier = -k * sin * xp;
                let factor = Complex::new(carrier.cos(), carrier.sin()) * (1.0 / (n * n) as f64);
                for (r, v) in column.iter().enumerate() {
                    out.components[comp][r * n + c] = *v * factor;
                }
            }
        }
        Ok(out)
    }

    /// The field with x and y swapped (rows for columns).
    fn transposed(&self) -> Self {
        let n = self.n;
        let mut out = self.clone();
        out.tilt = (self.tilt.1, self.tilt.0);
        out.components = [self.components[1].clone(), self.components[0].clone()];
        for comp in 0..2 {
            for r in 0..n {
                for c in 0..n {
                    out.components[comp][c * n + r] = self.components[1 - comp][r * n + c];
                }
            }
        }
        out
    }

    /// The rows and columns worth drawing: a square about the lit region (a thousandth
    /// of the peak irradiance and up) with a quarter of its size spare each side.
    fn lit(&self) -> (usize, usize) {
        let n = self.n;
        let peak = (0..n * n)
            .map(|i| self.irradiance(i / n, i % n))
            .fold(0.0, f64::max);
        let mut reach = 0usize; // farthest lit sample from the centre, in samples
        for r in 0..n {
            for c in 0..n {
                if self.irradiance(r, c) >= 1e-3 * peak {
                    reach = reach.max(r.abs_diff(n / 2)).max(c.abs_diff(n / 2));
                }
            }
        }
        let half = (reach + reach / 4 + 2).min(n / 2);
        (n / 2 - half, (n / 2 + half).min(n))
    }

    /// `value(r, c)` on the lit square as a map.
    fn map(
        &self,
        value_at: impl Fn(usize, usize) -> f64,
        value: Axis,
        colours: ColourScale,
    ) -> Map {
        let (from, to) = self.lit();
        let size = to - from;
        let mut values = Vec::with_capacity(size * size);
        for r in from..to {
            for c in from..to {
                values.push(value_at(r, c));
            }
        }
        let edge = |i: usize| self.position(i) - self.spacing / 2.0;
        Map {
            columns: size,
            rows: size,
            x: (edge(from), edge(to)),
            y: (edge(from), edge(to)),
            values,
            value,
            colours,
        }
    }

    /// The irradiance n |E|² as a map.
    pub fn irradiance_graph(&self, title: impl Into<String>) -> Graph {
        Graph {
            title: title.into(),
            x: Axis::new("x", Unit::Length),
            y: Axis::new("y", Unit::Length),
            plot: Plot::Map(self.map(
                |r, c| self.irradiance(r, c),
                Axis::new("n |E|²", Unit::Irradiance),
                ColourScale::Sequential,
            )),
            equal_aspect: true,
            markers: Vec::new(),
            notes: vec![format!(
                "{:.6} W; {} × {} samples of {:.4} µm",
                self.power(),
                self.n,
                self.n,
                self.spacing * 1e3
            )],
        }
    }

    /// The phase of the x component (or y, if x is dark) relative to the reference sphere,
    /// blank where the irradiance is below a thousandth of its peak.
    pub fn phase_graph(&self, title: impl Into<String>) -> Graph {
        let component = usize::from(self.components[0].iter().all(|v| *v == Complex::ZERO));
        let peak = (0..self.n * self.n)
            .map(|i| self.irradiance(i / self.n, i % self.n))
            .fold(0.0, f64::max);
        let phase = |r: usize, c: usize| {
            if self.irradiance(r, c) < 1e-3 * peak {
                f64::NAN
            } else {
                let v = self.components[component][r * self.n + c];
                v.im.atan2(v.re)
            }
        };
        let mut radius = if self.curvature == 0.0 {
            "flat".to_owned()
        } else {
            format!("R = {:.4} mm", 1.0 / self.curvature)
        };
        if self.tilt != (0.0, 0.0) {
            radius.push_str(&format!(
                ", tilted plane wave of direction cosines ({:.4}, {:.4})",
                self.tilt.0, self.tilt.1
            ));
        }
        Graph {
            title: title.into(),
            x: Axis::new("x", Unit::Length),
            y: Axis::new("y", Unit::Length),
            plot: Plot::Map(self.map(phase, Axis::new("phase", Unit::Phase), ColourScale::Cyclic)),
            equal_aspect: true,
            markers: Vec::new(),
            notes: vec![format!("relative to the reference sphere ({radius})")],
        }
    }
}

pub(crate) fn unit(p: (Complex, Complex)) -> (Complex, Complex) {
    let size = (p.0.norm_squared() + p.1.norm_squared()).sqrt();
    if size > 0.0 {
        (p.0 * (1.0 / size), p.1 * (1.0 / size))
    } else {
        (Complex::ONE, Complex::ZERO)
    }
}
