//! The vectorial point-spread function behind an ideal objective (M9c2), fed by the traced
//! pupil field and valid in stratified media.
//!
//! A coherent source (a collimated or TEM₀₀ Gaussian beam) is sampled on a square grid of
//! rays and traced. Each ray that reaches the detector through an objective stands for a
//! plane wave in the medium at the focus, and the focal field is their sum: the angular
//! spectrum (Debye) form of the Richards–Wolf integral (B. Richards and E. Wolf, Proc. R.
//! Soc. A 253, 358 (1959)). Across flat layers the transverse wave vector k₀ p, p = n sin θ,
//! is kept, so the integral over the objective's pupil is an integral over p; the layers
//! enter through the phase and Fresnel transmission of each plane wave, which is the
//! stratified-media form of P. Török, P. Varga, Z. Laczik and G. R. Booker (J. Opt. Soc.
//! Am. A 12, 325 (1995)) and, for the phase, of S. F. Gibson and F. Lanni (J. Opt. Soc. Am.
//! A 8, 1601 (1991)). Here both come from the trace itself:
//!
//! - **Amplitude.** A ray's power at the pupil, P, over the pupil area it stands for
//!   (from its neighbours on the grid, so a relay's magnification is included) gives the
//!   plane wave's amplitude |Ã|² = P / (λ₀² n cos θ d²p), with d²p = d²ρ / f² (the sine
//!   condition, ρ the pupil coordinate). By Parseval's theorem the power through a plane
//!   in the focal region is then Σ P, and the result n |E|² is in W/mm²: the irradiance of
//!   a plane wave, and of the focus at low NA. The aplanatic apodization (√cos θ on the
//!   reference sphere) is this same energy balance.
//! - **Polarization.** The pupil field's polarization, given in the objective's own x and
//!   y, splits into its radial (p) and azimuthal (s) parts at each pupil point; the
//!   objective turns the radial part into the meridional direction across the converging
//!   ray and keeps the azimuthal one (Richards and Wolf 1959). At every face the ray
//!   crosses after the objective, the s and p parts are multiplied by the Fresnel
//!   amplitude transmissions there, and an ideal mirror reflects the field as a perfect
//!   conductor would.
//! - **Phase.** The ray's optical path where it reaches the detector, L, fixes the plane
//!   wave's phase there; at a point r the wave's phase is k₀ (L + n s·(r − r₀)). The
//!   objective sets the path so that design rays meet at the design focus in phase, so a
//!   mismatch of the layers appears as the traced path difference.
//!
//! Bulk absorption after the objective is left out (it is near uniform over the pupil in
//! thin layers); losses before the pupil are in P.

use optobench_math::{Complex, Vec3};
use optobench_scene::{Scene, SourceKind};
use optobench_trace::Ray;
use optobench_trace::fresnel_amplitudes;
use optobench_trace::nonsequential::{End, RayRecord, Role};

/// The polarization of the field at the objective's pupil, in the objective's own frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarization {
    /// Linear along the objective's x axis.
    X,
    /// Linear along its y axis.
    Y,
    /// Circular: (x + i y) / √2.
    Circular,
    /// Radial: along the pupil radius at every point.
    Radial,
    /// Azimuthal: across the pupil radius at every point.
    Azimuthal,
}

/// How to compute a PSF.
#[derive(Debug, Clone, PartialEq)]
pub struct PsfSettings {
    /// Rays across the source's beam (a square grid of `samples`²).
    pub samples: usize,
    /// The pupil field's polarization.
    pub polarization: Polarization,
    /// The axial scan, from and to (mm along the focus line, measured as the detector's z);
    /// by default the span of the rays' axial crossings plus two axial fringes either side.
    pub axial: Option<(f64, f64)>,
    /// Half-width of the lateral profiles (mm); by default 2.5 λ₀ / NA.
    pub lateral: Option<f64>,
    /// Points in the axial scan; by default eight per axial fringe.
    pub points: Option<usize>,
}

impl Default for PsfSettings {
    fn default() -> Self {
        Self {
            samples: 128,
            polarization: Polarization::X,
            axial: None,
            lateral: None,
            points: None,
        }
    }
}

/// A sampled profile: (position, n |E|²) pairs.
pub type Profile = Vec<(f64, f64)>;

/// The PSF near a detector, in the detector's frame (+z its axis, z = 0 its plane).
#[derive(Debug, Clone, PartialEq)]
pub struct Psf {
    /// Vacuum wavelength (µm).
    pub wavelength: f64,
    /// Index of the medium at the focus.
    pub index: f64,
    /// The largest n sin θ of the plane waves that arrive (to the focus line).
    pub na: f64,
    /// Power at the pupil of the rays that arrive (W).
    pub power: f64,
    /// Rays (plane waves) in the sum.
    pub rays: usize,
    /// Where the intensity is greatest (mm, detector frame).
    pub peak: Vec3,
    /// n |E|² there (W/mm²).
    pub peak_intensity: f64,
    /// The peak over that of the same pupil field brought to focus in phase.
    pub strehl: f64,
    /// Full widths at half maximum through the peak: along the detector's x and y, and
    /// along the focus line (mm); `None` where the profile does not fall to half.
    pub fwhm: (Option<f64>, Option<f64>, Option<f64>),
    /// The focus line: through the rays' power-weighted centre on the detector plane,
    /// along their mean direction (unit, detector frame).
    pub line: (Vec3, Vec3),
    /// The axial scan: (z, n |E|²) along the focus line.
    pub axial: Profile,
    /// The lateral profiles through the peak along the detector's x and y: (offset,
    /// n |E|²).
    pub lateral: (Profile, Profile),
    /// The axial crossings of the rays with the focus line: least and greatest z (mm),
    /// the geometric span of the focus.
    pub crossings: (f64, f64),
}

/// A complex 3-vector.
type Field = [Complex; 3];

fn scale(e: Field, s: Complex) -> Field {
    [e[0] * s, e[1] * s, e[2] * s]
}

fn along(v: Vec3, s: Complex) -> Field {
    [s * v.x, s * v.y, s * v.z]
}

fn add(a: Field, b: Field) -> Field {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// e · v (no conjugate).
fn dot(e: Field, v: Vec3) -> Complex {
    e[0] * v.x + e[1] * v.y + e[2] * v.z
}

/// A plane wave of the focal field: E(r) = amplitude · exp(i (phase + k·r)).
#[derive(Debug, Clone, Copy)]
struct Wave {
    amplitude: Field,
    phase: f64,
    k: Vec3,
}

/// The field of all waves at `r` (detector frame).
fn field(waves: &[Wave], r: Vec3) -> Field {
    let mut e = [Complex::ZERO; 3];
    for w in waves {
        let (s, c) = (w.phase + w.k.dot(r)).sin_cos();
        let f = Complex::new(c, s);
        e = add(e, scale(w.amplitude, f));
    }
    e
}

fn intensity(waves: &[Wave], index: f64, r: Vec3) -> f64 {
    let e = field(waves, r);
    index * (e[0].norm_squared() + e[1].norm_squared() + e[2].norm_squared())
}

/// The field after a face where the ray goes from direction `d_in` in index `n_in` to
/// `d_out` in `n_out`: Fresnel transmission of its s and p parts, or a perfect mirror.
fn across(e: Field, d_in: Vec3, n_in: f64, d_out: Vec3, n_out: f64) -> Field {
    if (n_in - n_out).abs() < 1e-12 {
        let turn = d_out - d_in;
        let Some(normal) = turn.normalized().filter(|_| turn.length() > 1e-12) else {
            return e; // no face to speak of (a detector, a dummy plane)
        };
        // A perfect conductor: E' = 2 (E·N) N − E.
        return add(
            along(normal, dot(e, normal) * 2.0),
            scale(e, Complex::real(-1.0)),
        );
    }
    // Vector Snell's law: n_out d_out − n_in d_in lies along the normal.
    let difference = d_out * n_out - d_in * n_in;
    let mut normal = if difference.length() > 1e-12 * n_in {
        difference.normalized().unwrap_or(d_in)
    } else {
        d_in
    };
    if normal.dot(d_in) < 0.0 {
        normal = -normal;
    }
    let cos_in = normal.dot(d_in).clamp(0.0, 1.0);
    let Some(t) = fresnel_amplitudes(cos_in, n_in, n_out) else {
        return [Complex::ZERO; 3]; // totally reflected
    };
    let s = d_in.cross(normal);
    let Some(s) = s.normalized().filter(|_| s.length() > 1e-9) else {
        // Normal incidence: s and p alike.
        return scale(e, Complex::real(t.transmission_s));
    };
    let (p_in, p_out) = (s.cross(d_in), s.cross(d_out));
    add(
        along(s, dot(e, s) * t.transmission_s),
        along(p_out, dot(e, p_in) * t.transmission_p),
    )
}

/// The index between points k and k + 1 of a record (from the optical path) and the
/// direction there.
fn segment(record: &RayRecord, k: usize) -> Option<(Vec3, f64)> {
    let step = record.points[k + 1] - record.points[k];
    let length = step.length();
    if length <= 0.0 {
        return None;
    }
    let index = (record.optical_paths[k + 1] - record.optical_paths[k]) / length;
    Some((step.normalized()?, index))
}

/// A traced ray that reached the detector through the objective.
struct Arrived {
    cell: (usize, usize),
    record: RayRecord,
    /// The record's hit index at the pupil.
    pupil_hit: usize,
    /// Pupil coordinate (objective frame, mm).
    pupil: (f64, f64),
}

/// Golden-section search for the greatest value of `f` on [a, b].
fn maximise(f: &dyn Fn(f64) -> f64, mut a: f64, mut b: f64, tolerance: f64) -> f64 {
    let g = 0.5 * (5f64.sqrt() - 1.0);
    let mut c = b - g * (b - a);
    let mut d = a + g * (b - a);
    let (mut fc, mut fd) = (f(c), f(d));
    while (b - a).abs() > tolerance {
        if fc > fd {
            b = d;
            d = c;
            fd = fc;
            c = b - g * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + g * (b - a);
            fd = f(d);
        }
    }
    0.5 * (a + b)
}

/// Where `f` falls to `level` going from `from` in steps of `step` (sign gives the side),
/// refined by bisection; `None` within `limit` steps.
fn falls_to(f: &dyn Fn(f64) -> f64, level: f64, from: f64, step: f64, limit: usize) -> Option<f64> {
    let mut inside = from;
    for _ in 0..limit {
        let next = inside + step;
        if f(next) < level {
            let (mut a, mut b) = (inside, next);
            for _ in 0..60 {
                let m = 0.5 * (a + b);
                if f(m) < level {
                    b = m;
                } else {
                    a = m;
                }
            }
            return Some(0.5 * (a + b));
        }
        inside = next;
    }
    None
}

/// The PSF of source `source` (index into the scene's sources; a collimated or Gaussian
/// beam) through the scene's objective near detector `detector`.
pub fn psf(
    scene: &Scene,
    source: usize,
    detector: usize,
    settings: &PsfSettings,
) -> Result<Psf, String> {
    let src = scene.sources.get(source).ok_or("no such source")?;
    let det = scene.detectors.get(detector).ok_or("no such detector")?;
    if !src.spectrum.is_empty() {
        return Err(format!(
            "source `{}` has a spectrum; the PSF is for one wavelength",
            src.id
        ));
    }
    // The beam's irradiance profile (per unit power) and the half-width of the grid.
    let (half, profile): (f64, Box<dyn Fn(f64) -> f64>) = match src.kind {
        SourceKind::Collimated { diameter } => {
            let a = diameter / 2.0;
            let level = 1.0 / (core::f64::consts::PI * a * a);
            (a, Box::new(move |r: f64| if r <= a { level } else { 0.0 }))
        }
        SourceKind::Gaussian {
            waist_diameter,
            m_squared,
            ..
        } => {
            if (m_squared - 1.0).abs() > 1e-9 {
                return Err(format!(
                    "source `{}`: the PSF needs a coherent TEM₀₀ beam (M² = 1), not M² = {m_squared}",
                    src.id
                ));
            }
            let w = waist_diameter / 2.0;
            let level = 2.0 / (core::f64::consts::PI * w * w);
            // Out to 2w the beam holds all but e⁻⁸ of its power.
            (
                2.0 * w,
                Box::new(move |r: f64| level * (-2.0 * r * r / (w * w)).exp()),
            )
        }
        _ => {
            return Err(format!(
                "source `{}`: the PSF needs a coherent beam, a collimated or Gaussian source",
                src.id
            ));
        }
    };
    let n = settings.samples.max(8);
    let h = 2.0 * half / n as f64;
    let built = scene.build().map_err(|e| e.to_string())?;
    let [ax, ay, az] = src.frame.axes();
    let origin = src.frame.origin();

    // Trace the grid; keep the rays that reach the detector through an objective.
    let mut objective_face: Option<usize> = None;
    let mut arrived: Vec<Arrived> = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let u = -half + (i as f64 + 0.5) * h;
            let v = -half + (j as f64 + 0.5) * h;
            let level = profile(u.hypot(v));
            if level <= 0.0 {
                continue;
            }
            let Some(ray) = Ray::new(
                origin + ax * u + ay * v,
                az,
                src.wavelength,
                src.power * level * h * h,
            ) else {
                return Err("a source axis of zero length".into());
            };
            let record = built.system.trace(ray);
            let End::Detected { detector: d, .. } = record.end else {
                continue;
            };
            if d != detector {
                continue;
            }
            let Some(k) = record
                .faces
                .iter()
                .position(|&f| matches!(built.system.faces[f].role, Role::Objective { .. }))
            else {
                continue;
            };
            let face = record.faces[k];
            match objective_face {
                None => objective_face = Some(face),
                Some(f) if f != face => {
                    return Err("rays reach the detector through different objectives".into());
                }
                Some(_) => {}
            }
            // Pupil arrival, then the departure from the front plane, then onwards.
            if record.faces.get(k + 1) != Some(&face) || record.points.len() < k + 4 {
                continue;
            }
            let local = built.system.faces[face]
                .placement
                .inverse()
                .apply_point(record.points[k + 1]);
            arrived.push(Arrived {
                cell: (i, j),
                record,
                pupil_hit: k,
                pupil: (local.x, local.y),
            });
        }
    }
    let face = objective_face.ok_or_else(|| {
        format!(
            "no ray of source `{}` reaches detector `{}` through an objective",
            src.id, det.id
        )
    })?;
    if arrived.len() < 3 {
        return Err("too few rays reach the detector for a PSF".into());
    }
    let objective_face = &built.system.faces[face];
    let Role::Objective { objective, .. } = &objective_face.role else {
        return Err("not an objective".into());
    };
    let f = objective.focal_length;
    let placement = objective_face.placement;
    let (ex, ey, ez) = (
        placement.apply_vector(Vec3::X),
        placement.apply_vector(Vec3::Y),
        placement.apply_vector(Vec3::Z),
    );

    // Pupil area per grid cell, from the neighbours on the grid.
    let mut grid: Vec<Option<usize>> = vec![None; n * n];
    for (a, r) in arrived.iter().enumerate() {
        grid[r.cell.0 * n + r.cell.1] = Some(a);
    }
    let at = |i: isize, j: isize| -> Option<(f64, f64)> {
        if i < 0 || j < 0 || i >= n as isize || j >= n as isize {
            return None;
        }
        grid[i as usize * n + j as usize].map(|a| arrived[a].pupil)
    };
    let derivative = |i: isize, j: isize, di: isize, dj: isize| -> Option<(f64, f64)> {
        let here = at(i, j)?;
        match (at(i + di, j + dj), at(i - di, j - dj)) {
            (Some(p), Some(m)) => Some(((p.0 - m.0) / 2.0, (p.1 - m.1) / 2.0)),
            (Some(p), None) => Some((p.0 - here.0, p.1 - here.1)),
            (None, Some(m)) => Some((here.0 - m.0, here.1 - m.1)),
            (None, None) => None,
        }
    };
    let areas: Vec<Option<f64>> = arrived
        .iter()
        .map(|r| {
            let (i, j) = (r.cell.0 as isize, r.cell.1 as isize);
            let du = derivative(i, j, 1, 0)?;
            let dv = derivative(i, j, 0, 1)?;
            Some((du.0 * dv.1 - du.1 * dv.0).abs())
        })
        .collect();
    let mut known: Vec<f64> = areas.iter().flatten().copied().collect();
    known.sort_by(f64::total_cmp);
    let typical = known.get(known.len() / 2).copied().ok_or("no pupil area")?;

    // The plane waves, in the detector's frame.
    let wavelength = src.wavelength * 1e-3; // mm
    let k0 = 2.0 * core::f64::consts::PI / wavelength;
    let frame = det.frame;
    let reference = arrived[0]
        .record
        .optical_paths
        .last()
        .copied()
        .unwrap_or(0.0);
    let mut waves: Vec<Wave> = Vec::with_capacity(arrived.len());
    let mut directions: Vec<(Vec3, Vec3, f64)> = Vec::with_capacity(arrived.len()); // point, direction, power
    let mut index: Option<f64> = None;
    let mut power = 0.0;
    for (r, area) in arrived.iter().zip(&areas) {
        let record = &r.record;
        let k = r.pupil_hit;
        let p_pupil = record.powers[k];
        let d2p = area.unwrap_or(typical) / (f * f);
        // Pupil polarization, split into radial (p) and azimuthal (s) parts.
        let rho = r.pupil.0.hypot(r.pupil.1);
        let (c, s) = if rho > 1e-12 * f {
            (r.pupil.0 / rho, r.pupil.1 / rho)
        } else {
            (1.0, 0.0)
        };
        let (px, py) = match settings.polarization {
            Polarization::X => (Complex::ONE, Complex::ZERO),
            Polarization::Y => (Complex::ZERO, Complex::ONE),
            Polarization::Circular => (
                Complex::real(core::f64::consts::FRAC_1_SQRT_2),
                Complex::new(0.0, core::f64::consts::FRAC_1_SQRT_2),
            ),
            Polarization::Radial => (Complex::real(c), Complex::real(s)),
            Polarization::Azimuthal => (Complex::real(-s), Complex::real(c)),
        };
        let e_radial = px * c + py * s;
        let e_azimuthal = py * c - px * s;
        let azimuthal = ex * (-s) + ey * c;
        // Leaving the front plane (segment from the departure point).
        let Some((d1, n1)) = segment(record, k + 2) else {
            continue;
        };
        let meridional = azimuthal.cross(d1);
        let cos1 = d1.dot(ez);
        if cos1 <= 0.0 {
            continue;
        }
        let amplitude = (p_pupil / (wavelength * wavelength * n1 * cos1 * d2p)).sqrt();
        let mut e = add(along(meridional, e_radial), along(azimuthal, e_azimuthal));
        e = scale(e, Complex::real(amplitude));
        // Every face after the objective.
        let last = record.points.len() - 2; // the segment that ends on the detector
        let (mut d, mut n_here) = (d1, n1);
        for m in (k + 3)..=last {
            let Some((d_next, n_next)) = segment(record, m) else {
                continue;
            };
            e = across(e, d, n_here, d_next, n_next);
            d = d_next;
            n_here = n_next;
        }
        match index {
            None => index = Some(n_here),
            Some(n0) if (n0 - n_here).abs() > 1e-9 => {
                return Err("rays reach the detector through different media".into());
            }
            Some(_) => {}
        }
        // To the detector's frame.
        let point = frame.to_local_point(record.points[last + 1]);
        let direction = frame.to_local_vector(d);
        let to_local = |v: Field| -> Field {
            let re = frame.to_local_vector(Vec3::new(v[0].re, v[1].re, v[2].re));
            let im = frame.to_local_vector(Vec3::new(v[0].im, v[1].im, v[2].im));
            [
                Complex::new(re.x, im.x),
                Complex::new(re.y, im.y),
                Complex::new(re.z, im.z),
            ]
        };
        let path = record.optical_paths[last + 1] - reference;
        let kv = direction * (k0 * n_here);
        waves.push(Wave {
            amplitude: scale(to_local(e), Complex::real(d2p)),
            phase: k0 * path - kv.dot(point),
            k: kv,
        });
        directions.push((point, direction, p_pupil));
        power += p_pupil;
    }
    let index = index.ok_or("no ray reaches the detector")?;

    // The focus line: through the power-weighted centre, along the mean direction.
    let (mut centre, mut mean) = (Vec3::ZERO, Vec3::ZERO);
    for (p, d, w) in &directions {
        centre = centre + *p * (*w / power);
        mean = mean + *d * *w;
    }
    let mean = mean.normalized().ok_or("the rays cancel in direction")?;
    if mean.z.abs() < 1e-6 {
        return Err("the light runs along the detector plane".into());
    }
    let on_line = |z: f64| centre + mean * ((z - centre.z) / mean.z);
    let along_line = |z: f64| intensity(&waves, index, on_line(z));
    // Aperture and axial crossings with the line.
    let mut na: f64 = 0.0;
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    let slope = |d: Vec3| (d.x / d.z, d.y / d.z);
    let line_slope = slope(mean);
    for (p, d, _) in &directions {
        let cos = d.dot(mean).clamp(-1.0, 1.0);
        na = na.max(index * (1.0 - cos * cos).sqrt());
        let q0 = (p.x - centre.x, p.y - centre.y);
        let s = slope(*d);
        let g = (s.0 - line_slope.0, s.1 - line_slope.1);
        let gg = g.0 * g.0 + g.1 * g.1;
        if gg > 1e-18 {
            let z = -(q0.0 * g.0 + q0.1 * g.1) / gg;
            low = low.min(z);
            high = high.max(z);
        }
    }
    if !low.is_finite() {
        (low, high) = (0.0, 0.0);
    }
    if na <= 0.0 {
        return Err("the light at the detector is not converging".into());
    }
    let cos_max = (1.0 - (na / index).powi(2)).max(0.0).sqrt();
    let fringe = wavelength / (index * (1.0 - cos_max)); // axial period of the focus
    let (from, to) = settings
        .axial
        .unwrap_or((low - 2.0 * fringe, high + 2.0 * fringe));
    if to <= from {
        return Err("the axial scan runs from low to high".into());
    }
    let points = settings
        .points
        .unwrap_or(((to - from) / (fringe / 8.0)).ceil() as usize + 1)
        .clamp(3, 100_001);
    let step = (to - from) / (points - 1) as f64;
    let axial: Vec<(f64, f64)> = (0..points)
        .map(|i| {
            let z = from + step * i as f64;
            (z, along_line(z))
        })
        .collect();
    let best = (0..points)
        .max_by(|&a, &b| axial[a].1.total_cmp(&axial[b].1))
        .unwrap_or(0);
    let z_peak = maximise(
        &along_line,
        axial[best.saturating_sub(1)].0,
        axial[(best + 1).min(points - 1)].0,
        1e-6 * fringe,
    );
    let peak = on_line(z_peak);
    let peak_intensity = intensity(&waves, index, peak);

    // Widths through the peak, found on the field itself.
    let lateral_scale = wavelength / na;
    let width = |direction: Vec3, scale: f64| -> Option<f64> {
        let f = |t: f64| intensity(&waves, index, peak + direction * t);
        let level = peak_intensity / 2.0;
        let step = scale / 16.0;
        let right = falls_to(&f, level, 0.0, step, 400)?;
        let left = falls_to(&f, level, 0.0, -step, 400)?;
        Some(right - left)
    };
    let fwhm = (
        width(Vec3::X, lateral_scale),
        width(Vec3::Y, lateral_scale),
        width(mean, fringe).map(|w| w * mean.z.abs()),
    );
    let half_width = settings.lateral.unwrap_or(2.5 * lateral_scale);
    let lateral_points = 81;
    let profile = |direction: Vec3| -> Vec<(f64, f64)> {
        (0..lateral_points)
            .map(|i| {
                let t = -half_width + 2.0 * half_width * i as f64 / (lateral_points - 1) as f64;
                (t, intensity(&waves, index, peak + direction * t))
            })
            .collect()
    };
    let lateral = (profile(Vec3::X), profile(Vec3::Y));

    // The same pupil field in phase.
    let in_phase = waves
        .iter()
        .fold([Complex::ZERO; 3], |e, w| add(e, w.amplitude));
    let ideal = index
        * (in_phase[0].norm_squared() + in_phase[1].norm_squared() + in_phase[2].norm_squared());
    let strehl = if ideal > 0.0 {
        peak_intensity / ideal
    } else {
        0.0
    };

    Ok(Psf {
        wavelength: src.wavelength,
        index,
        na,
        power,
        rays: waves.len(),
        peak,
        peak_intensity,
        strehl,
        fwhm,
        line: (centre, mean),
        axial,
        lateral,
        crossings: (low, high),
    })
}
