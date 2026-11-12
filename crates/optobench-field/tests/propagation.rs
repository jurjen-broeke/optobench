//! M10b: field propagation against closed forms.
//!
//! - Gaussian beams by the q law: q = z − i z_R, u = (q₀/q) exp(i k r² / (2q)) e^{ikz}, a
//!   thin lens 1/q' = 1/q − 1/f (H. Kogelnik and T. Li, Appl. Opt. 5, 1550 (1966);
//!   A. E. Siegman, Lasers, 1986, §17.1–17.4).
//! - A plane wave through a circular aperture of radius a: on the axis at z,
//!   I = 4 I₀ sin²(π a² / (2 λ z)) (J. W. Goodman, Introduction to Fourier Optics, 3rd
//!   ed., §4.5, Fresnel diffraction; also Born and Wolf, Principles of Optics, §8.8).
//! - Its far field at a lens's focus, the Airy pattern I(r) = I(0) (2 J₁(v)/v)²,
//!   v = 2π a r / (λ f), I(0) = I₀ π² a⁴ / (λ² f²) (Born and Wolf, §8.5.2).
//! - A Gaussian truncated at radius a passes 1 − exp(−2a²/w²) of its power, and its far
//!   field on the axis is |∫E dA|² / (λ f)² with ∫E dA = E₀ π w² (1 − exp(−a²/w²)).
//! - A grating of period d images itself at the Talbot distance 2 d² / λ and shifted by
//!   half a period at half of it (Goodman, §4.5.2 and problem 4-16).
//! - Maréchal: a small RMS wavefront error σ (waves) lowers the focal peak by about
//!   exp(−(2π σ)²) (Born and Wolf, §9.3).
//!
//! Every propagation also conserves power, Σ n |E|² dA.

use optobench_field::{Aperture, Field, FieldError, Method, TiltAxis};
use optobench_math::Complex;

const X: (Complex, Complex) = (Complex::ONE, Complex::ZERO);

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance * b.abs().max(1e-300)
}

/// q-law Gaussian field: the amplitude factor and q at the plane, and the path so far.
#[derive(Clone, Copy)]
struct Beam {
    q: Complex,
    factor: Complex,
    path: f64,
}

impl Beam {
    fn at_waist(rayleigh: f64) -> Self {
        Self {
            q: Complex::new(0.0, -rayleigh),
            factor: Complex::ONE,
            path: 0.0,
        }
    }

    fn propagate(self, d: f64) -> Self {
        let q = self.q + Complex::real(d);
        Self {
            q,
            factor: self.factor * self.q / q,
            path: self.path + d,
        }
    }

    fn lens(self, f: f64) -> Self {
        Self {
            q: Complex::ONE / (Complex::ONE / self.q - Complex::real(1.0 / f)),
            ..self
        }
    }

    /// The field at radius² `r2`, for amplitude `peak` at the original waist.
    fn value(&self, r2: f64, k: f64, peak: f64) -> Complex {
        let phase = Complex::new(0.0, k * r2 / 2.0) / self.q;
        let carrier = k * self.path;
        self.factor * phase.exp() * Complex::new(carrier.cos(), carrier.sin()) * peak
    }
}

/// The largest difference between a field and the q-law beam over the grid, relative to
/// the beam's peak there.
fn worst(field: &Field, beam: &Beam, peak: f64) -> f64 {
    let n = field.size();
    let at_centre = beam.value(0.0, field.k(), peak).abs();
    let mut worst: f64 = 0.0;
    for r in (0..n).step_by(3) {
        for c in (0..n).step_by(3) {
            let (x, y) = (field.position(c), field.position(r));
            let expected = beam.value(x * x + y * y, field.k(), peak);
            worst = worst.max((field.value(0, r, c) - expected).abs() / at_centre);
        }
    }
    worst
}

#[test]
fn a_gaussian_beam_spreads_and_focuses_as_the_q_law_says() {
    // λ = 0.633 µm, w₀ = 0.5 mm (z_R = 1240.8 mm), 1 mW.
    let (lambda, w0, power) = (0.633, 0.5, 1e-3);
    let rayleigh = core::f64::consts::PI * w0 * w0 / (lambda * 1e-3);
    let peak = (2.0 * power / (core::f64::consts::PI * w0 * w0)).sqrt();
    // Short (angular spectrum) and long (Fresnel) distances from the waist.
    for (z, expect) in [
        (50.0, Method::AngularSpectrum),
        (rayleigh, Method::Fresnel),
        (4.0 * rayleigh, Method::Fresnel),
    ] {
        let mut field = Field::gaussian(256, 0.04, lambda, 1.0, w0, 0.0, power, X).expect("field");
        let report = field.propagate(z).expect("propagates");
        assert_eq!(report.method, expect, "z = {z}");
        let beam = Beam::at_waist(rayleigh).propagate(z);
        let error = worst(&field, &beam, peak);
        assert!(error < 1e-6, "z = {z}: {error}");
        assert!(
            close(field.power(), power, 1e-9),
            "z = {z}: {}",
            field.power()
        );
        // The beam's second-moment radius is w(z).
        let w = w0 * (1.0 + (z / rayleigh).powi(2)).sqrt();
        let (wx, wy) = field.second_moment_radius();
        assert!(close(wx, w, 1e-4) && close(wy, w, 1e-4), "{wx} vs {w}");
    }

    // Through a 100 mm lens at the waist (w₀ = 1 mm): to the focal plane (the incoming
    // sphere and the propagation cancel), then before and past the beam's own focus.
    let w0 = 1.0;
    let rayleigh = core::f64::consts::PI * w0 * w0 / (lambda * 1e-3);
    let peak = (2.0 * power / (core::f64::consts::PI * w0 * w0)).sqrt();
    for d in [60.0, 100.0, 150.0] {
        let mut field = Field::gaussian(256, 0.03, lambda, 1.0, w0, 0.0, power, X).expect("field");
        field.lens(100.0);
        field.propagate(d).expect("propagates");
        let beam = Beam::at_waist(rayleigh).lens(100.0).propagate(d);
        let error = worst(&field, &beam, peak);
        assert!(error < 1e-6, "d = {d}: {error}");
        assert!(close(field.power(), power, 1e-9), "d = {d}");
    }
}

#[test]
fn a_circular_aperture_lights_its_axis_as_fresnel_s_formula_says() {
    // A plane wave (I₀ = 1 W/mm², λ = 0.5 µm) through a = 1 mm, at Fresnel numbers
    // a² / (λ z) from 1 (bright: 4 I₀) to 4 (dark).
    let (lambda, a) = (0.5, 1.0);
    for fresnel in [1.0, 1.5, 2.0, 3.0, 4.0] {
        let z = a * a / (lambda * 1e-3 * fresnel);
        let mut field = Field::plane(512, 0.008, lambda, 1.0, 1.0, X).expect("field");
        field.apply(&Aperture::Circle {
            radius: a,
            centre: (0.0, 0.0),
        });
        let before = field.power();
        field.propagate(z).expect("propagates");
        let centre = field.size() / 2;
        let on_axis = field.irradiance(centre, centre);
        let expected = 4.0
            * (core::f64::consts::PI * a * a / (2.0 * lambda * 1e-3 * z))
                .sin()
                .powi(2);
        assert!(
            (on_axis - expected).abs() < 2e-3 * 4.0,
            "N = {fresnel}: {on_axis} vs {expected}"
        );
        assert!(close(field.power(), before, 1e-9));
    }
}

/// J₁ by Bessel's integral (Watson, Theory of Bessel Functions, §2.2), Simpson's rule.
fn bessel_j1(v: f64) -> f64 {
    let n = 400;
    let h = core::f64::consts::PI / f64::from(n);
    let f = |t: f64| (t - v * t.sin()).cos();
    let mut sum = f(0.0) + f(core::f64::consts::PI);
    for k in 1..n {
        sum += f(f64::from(k) * h) * if k % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0 / core::f64::consts::PI
}

#[test]
fn a_lens_focuses_an_aperture_to_the_airy_pattern() {
    let (lambda, a, f) = (0.5, 1.0, 200.0);
    let mut field = Field::plane(512, 0.008, lambda, 1.0, 1.0, X).expect("field");
    field.apply(&Aperture::Circle {
        radius: a,
        centre: (0.0, 0.0),
    });
    field.lens(f);
    let report = field.propagate(f).expect("propagates");
    assert_eq!(report.method, Method::Fresnel);
    let l = lambda * 1e-3;
    let peak = core::f64::consts::PI.powi(2) * a.powi(4) / (l * l * f * f);
    let centre = field.size() / 2;
    assert!(
        close(field.irradiance(centre, centre), peak, 2e-3),
        "{} vs {peak}",
        field.irradiance(centre, centre)
    );
    for k in 1..8 {
        let r = field.position(centre + k);
        let v = 2.0 * core::f64::consts::PI * a * r / (l * f);
        let airy = peak * (2.0 * bessel_j1(v) / v).powi(2);
        let got = field.irradiance(centre, centre + k);
        assert!((got - airy).abs() < 2e-3 * peak, "r = {r}: {got} vs {airy}");
    }
}

#[test]
fn a_truncated_gaussian_passes_and_focuses_as_its_closed_forms_say() {
    let (lambda, w, power, f) = (0.633, 1.0, 1.0, 100.0);
    for a in [1.0, 1.5] {
        let mut field = Field::gaussian(512, 0.01, lambda, 1.0, w, 0.0, power, X).expect("field");
        field.apply(&Aperture::Circle {
            radius: a * w,
            centre: (0.0, 0.0),
        });
        let passed = 1.0 - (-2.0 * a * a).exp();
        assert!(
            close(field.power(), passed * power, 2e-3),
            "a = {a}w: {} vs {passed}",
            field.power()
        );
        field.lens(f);
        field.propagate(f).expect("propagates");
        let e0 = (2.0 * power / (core::f64::consts::PI * w * w)).sqrt();
        let integral = e0 * core::f64::consts::PI * w * w * (1.0 - (-a * a).exp());
        let l = lambda * 1e-3;
        let expected = (integral / (l * f)).powi(2);
        let centre = field.size() / 2;
        assert!(
            close(field.irradiance(centre, centre), expected, 1e-3),
            "a = {a}w"
        );
    }
}

#[test]
fn a_grating_images_itself_at_the_talbot_distance() {
    // Amplitude (1 + cos 2πx/d)/2, d = 0.1 mm, λ = 0.5 µm: z_T = 2d²/λ = 40 mm. The grid
    // holds eight whole periods, so it is the infinite grating; the angular spectrum
    // (its three lines) is used whatever the distance.
    let (lambda, d) = (0.5, 0.1);
    let talbot = 2.0 * d * d / (lambda * 1e-3);
    let n = 256;
    let make = || {
        let mut field = Field::plane(n, d / 32.0, lambda, 1.0, 1.0, X).expect("field");
        field.modulate(|x, _| {
            Complex::real(0.5 * (1.0 + (2.0 * core::f64::consts::PI * x / d).cos()))
        });
        field
    };
    let start = make();
    let mut whole = make();
    whole
        .propagate_with(talbot, Some(Method::AngularSpectrum))
        .expect("propagates");
    let mut half = make();
    half.propagate_with(talbot / 2.0, Some(Method::AngularSpectrum))
        .expect("propagates");
    let row = n / 2;
    for c in 0..n {
        let original = start.irradiance(row, c);
        assert!(
            (whole.irradiance(row, c) - original).abs() < 1e-6,
            "c = {c}"
        );
        // Shifted by half a period (16 samples).
        let shifted = start.irradiance(row, (c + 16) % n);
        assert!((half.irradiance(row, c) - shifted).abs() < 1e-6, "c = {c}");
    }
    assert!(close(whole.power(), start.power(), 1e-9));
}

#[test]
fn a_small_wavefront_error_lowers_the_focus_as_marechal_says() {
    // 0.05 waves RMS of primary spherical aberration (ANSI j = 12) over the aperture:
    // the focal peak drops to about exp(−(2π · 0.05)²) = 0.906 of the perfect one.
    let (lambda, a, f) = (0.5, 1.0, 200.0);
    let focus = |sigma: f64| {
        let mut field = Field::plane(256, 0.016, lambda, 1.0, 1.0, X).expect("field");
        field.apply(&Aperture::Circle {
            radius: a,
            centre: (0.0, 0.0),
        });
        field.zernike(&[(12, sigma)], a);
        field.lens(f);
        field.propagate(f).expect("propagates");
        let c = field.size() / 2;
        field.irradiance(c, c)
    };
    let ratio = focus(0.05) / focus(0.0);
    let marechal = (-(2.0 * core::f64::consts::PI * 0.05).powi(2)).exp();
    assert!((ratio - marechal).abs() < 0.01, "{ratio} vs {marechal}");
}

#[test]
fn a_field_too_coarse_for_its_phase_is_refused() {
    // A strong lens written into the samples instead of the reference sphere.
    let mut field = Field::plane(64, 0.05, 0.5, 1.0, 1.0, X).expect("field");
    let k = field.k();
    field.modulate(|x, y| {
        let phase = -k * (x * x + y * y) / (2.0 * 20.0);
        Complex::new(phase.cos(), phase.sin())
    });
    match field.propagate(20.0) {
        Err(FieldError::Undersampled(message)) => {
            assert!(message.contains("reference sphere"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert!(Field::new(63, 0.1, 0.5, 1.0).is_err());
}

#[test]
fn light_reaching_the_grid_edge_is_reported_and_padding_keeps_the_field() {
    // A 50 µm pinhole spreads λ z / a = 0.5 mm in 50 mm, past the edge of a 1.28 mm grid:
    // the report says how much power sits at the edge (where it may have wrapped round).
    let mut small = Field::plane(128, 0.01, 0.5, 1.0, 1.0, X).expect("field");
    small.apply(&Aperture::Circle {
        radius: 0.05,
        centre: (0.0, 0.0),
    });
    let padded = small.padded(4);
    assert_eq!(padded.size(), 512);
    assert!(close(padded.power(), small.power(), 1e-12));
    let c = padded.size() / 2;
    assert_eq!(padded.irradiance(c, c), small.irradiance(64, 64));
    let report = small
        .propagate_with(50.0, Some(Method::AngularSpectrum))
        .expect("propagates");
    assert!(report.edge_fraction > 0.01, "{report:?}");
    // A beam well inside its grid leaves the edge dark.
    let mut beam = Field::gaussian(256, 0.04, 0.633, 1.0, 0.5, 0.0, 1.0, X).expect("field");
    let report = beam.propagate(100.0).expect("propagates");
    assert!(report.edge_fraction < 1e-9, "{report:?}");
}

#[test]
fn a_tilted_plane_sees_the_beam_where_it_crosses_it() {
    // A Gaussian beam (w₀ = 0.1 mm, λ = 0.633 µm) at its waist, on planes tilted by 30°
    // about y and by −20° about x: each sample is the beam at its point of the tilted plane,
    // (x′ cos α, y′, x′ sin α), with the q law's amplitude, curvature, Gouy phase and carrier
    // for that z.
    let (lambda, w0, power) = (0.633, 0.1, 1e-3);
    let rayleigh = core::f64::consts::PI * w0 * w0 / (lambda * 1e-3);
    let peak = (2.0 * power / (core::f64::consts::PI * w0 * w0)).sqrt();
    let field = Field::gaussian(128, 0.01, lambda, 1.0, w0, 0.0, power, X).expect("field");
    for (angle, axis) in [
        (30f64.to_radians(), TiltAxis::Y),
        ((-20f64).to_radians(), TiltAxis::X),
    ] {
        let tilted = field.tilted(angle, axis).expect("tilts");
        let n = tilted.size();
        let mut worst: f64 = 0.0;
        for r in (0..n).step_by(2) {
            for c in (0..n).step_by(2) {
                let (u, v) = (tilted.position(c), tilted.position(r));
                let (x, y, z) = match axis {
                    TiltAxis::Y => (u * angle.cos(), v, u * angle.sin()),
                    TiltAxis::X => (u, v * angle.cos(), v * angle.sin()),
                };
                let beam = Beam::at_waist(rayleigh).propagate(z);
                let expected = beam.value(x * x + y * y, tilted.k(), peak);
                worst = worst.max((tilted.value(0, r, c) - expected).abs() / peak);
            }
        }
        assert!(worst < 1e-6, "{axis:?}: {worst}");
        // The oblique incidence is carried as the tilt, not sampled.
        let tilt = tilted.tilt();
        match axis {
            TiltAxis::Y => assert!((tilt.0 - angle.sin()).abs() < 1e-12 && tilt.1 == 0.0),
            TiltAxis::X => assert!((tilt.1 - angle.sin()).abs() < 1e-12 && tilt.0 == 0.0),
        }
        // Its footprint is stretched by 1 / cos α along the tilted axis.
        let (wx, wy) = tilted.second_moment_radius();
        let stretched = w0 / angle.cos();
        match axis {
            TiltAxis::Y => {
                assert!((wx / stretched - 1.0).abs() < 1e-3 && (wy / w0 - 1.0).abs() < 1e-3)
            }
            TiltAxis::X => {
                assert!((wy / stretched - 1.0).abs() < 1e-3 && (wx / w0 - 1.0).abs() < 1e-3)
            }
        }
        // The same power crosses the tilted plane.
        assert!(
            (tilted.power() / field.power() - 1.0).abs() < 1e-6,
            "{}",
            tilted.power()
        );
        // An end point: it cannot be propagated further.
        assert!(tilted.clone().propagate(10.0).is_err());
    }
}
