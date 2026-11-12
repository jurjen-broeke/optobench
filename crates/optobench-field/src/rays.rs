//! Rays to a field and back (M10c), in the plane of a field (local x, y; +z along it).
//!
//! **Rays to a field.** The rays come from a regular launch grid (a source's beam sampled
//! cell by cell), so each one stands for its launch cell, and its neighbours show what
//! area that cell covers where the rays cross the plane. A ray's irradiance there is its
//! power over that area, its phase k₀ times its optical path. Between neighbouring rays
//! the field is interpolated over triangles (two per launch cell), so it is the
//! geometric-optics field (the eikonal: amplitude from the ray density, phase from the
//! path; Born and Wolf, Principles of Optics, §3.1). The mean sphere of the rays (their
//! slopes against their positions, least squares) is taken out as the field's reference,
//! so a converging or diverging beam is sampled on what is left. Where triangles fold
//! over each other rays cross: a caustic, where this picture fails, and the conversion
//! is refused.
//!
//! **A field to rays.** One ray per grid cell lit above a threshold, along the local
//! direction of the wavefront (the phase gradient over k, the Poynting direction in the
//! paraxial limit), with the cell's power and an optical path from the unwrapped phase.
//! A ray whose cell sits by a dark one, or whose phase jumps there, is marked rough.

use optobench_math::{Complex, Vec3};

use crate::{Field, FieldError};

/// A ray where it crosses a field's plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RaySample {
    /// Its cell on the launch grid (column, row).
    pub cell: (usize, usize),
    /// Where it crosses the plane (local x, y; mm).
    pub point: (f64, f64),
    /// Its direction (local, unit, z > 0).
    pub direction: Vec3,
    /// Its power (W).
    pub power: f64,
    /// Its optical path from the source (mm).
    pub path: f64,
    /// For rays from a field: by a dark cell or a jump of phase.
    pub rough: bool,
}

/// The field the rays make on a grid of `size` × `size` samples of `spacing` (mm), at
/// vacuum wavelength `wavelength` (µm) in a medium of index `index`, polarized along
/// `polarization`. Returns the field and the optical path its zero phase stands for.
pub fn field_from_rays(
    rays: &[RaySample],
    size: usize,
    spacing: f64,
    wavelength: f64,
    index: f64,
    polarization: (Complex, Complex),
) -> Result<(Field, f64), FieldError> {
    let mut field = Field::new(size, spacing, wavelength, index)?;
    if rays.len() < 3 {
        return Err(FieldError::Grid("too few rays reach the plane".into()));
    }
    let side = rays
        .iter()
        .map(|r| r.cell.0.max(r.cell.1))
        .max()
        .unwrap_or(0)
        + 1;
    let mut at: Vec<Option<usize>> = vec![None; side * side];
    for (i, r) in rays.iter().enumerate() {
        at[r.cell.1 * side + r.cell.0] = Some(i);
    }
    let get = |c: isize, r: isize| -> Option<&RaySample> {
        if c < 0 || r < 0 || c >= side as isize || r >= side as isize {
            return None;
        }
        at[r as usize * side + c as usize].map(|i| &rays[i])
    };
    // The area each ray stands for on the plane, from its neighbours.
    let derivative = |c: isize, r: isize, dc: isize, dr: isize| -> Option<(f64, f64)> {
        let here = get(c, r)?.point;
        match (get(c + dc, r + dr), get(c - dc, r - dr)) {
            (Some(p), Some(m)) => {
                Some(((p.point.0 - m.point.0) / 2.0, (p.point.1 - m.point.1) / 2.0))
            }
            (Some(p), None) => Some((p.point.0 - here.0, p.point.1 - here.1)),
            (None, Some(m)) => Some((here.0 - m.point.0, here.1 - m.point.1)),
            (None, None) => None,
        }
    };
    let area = |ray: &RaySample| -> Option<f64> {
        let (c, r) = (ray.cell.0 as isize, ray.cell.1 as isize);
        let u = derivative(c, r, 1, 0)?;
        let v = derivative(c, r, 0, 1)?;
        Some((u.0 * v.1 - u.1 * v.0).abs())
    };
    let total: f64 = rays.iter().map(|r| r.power).sum();
    // The mean sphere: slopes against positions about the axis.
    let (mut slope_position, mut position_position) = (0.0, 0.0);
    for r in rays {
        let d = r.direction;
        if d.z <= 0.0 {
            return Err(FieldError::Grid("a ray crosses the plane backwards".into()));
        }
        let (sx, sy) = (d.x / d.z, d.y / d.z);
        slope_position += r.power * (r.point.0 * sx + r.point.1 * sy);
        position_position += r.power * (r.point.0 * r.point.0 + r.point.1 * r.point.1);
    }
    let curvature = if position_position > 0.0 {
        slope_position / position_position
    } else {
        0.0
    };
    let reference = rays.iter().map(|r| r.power * r.path).sum::<f64>() / total;
    let k0 = 2.0 * core::f64::consts::PI / (wavelength * 1e-3);
    let k = k0 * index;
    // Per ray: amplitude and phase less the sphere.
    let values: Vec<Option<(f64, f64)>> = rays
        .iter()
        .map(|r| {
            let a = area(r)?;
            if a <= 0.0 {
                return None;
            }
            let rho2 = r.point.0 * r.point.0 + r.point.1 * r.point.1;
            let phase = k0 * (r.path - reference) - k * curvature * rho2 / 2.0;
            Some(((r.power / (a * index)).sqrt(), phase))
        })
        .collect();
    let value = |c: isize, r: isize| -> Option<((f64, f64), (f64, f64))> {
        if c < 0 || r < 0 || c >= side as isize || r >= side as isize {
            return None;
        }
        let i = at[r as usize * side + c as usize]?;
        Some((rays[i].point, values[i]?))
    };
    let (jx, jy) = crate::unit(polarization);
    let n = field.n;
    let mut orientation = 0.0f64;
    for r in 0..side as isize - 1 {
        for c in 0..side as isize - 1 {
            let corners = [
                value(c, r),
                value(c + 1, r),
                value(c + 1, r + 1),
                value(c, r + 1),
            ];
            let [Some(a), Some(b), Some(cc), Some(d)] = corners else {
                continue;
            };
            for triangle in [[a, b, cc], [a, cc, d]] {
                let [(p0, v0), (p1, v1), (p2, v2)] = triangle;
                let det = (p1.0 - p0.0) * (p2.1 - p0.1) - (p2.0 - p0.0) * (p1.1 - p0.1);
                if det.abs() < 1e-300 {
                    continue;
                }
                // The first triangle's turning sense; every other must turn the same way.
                if orientation == 0.0 {
                    orientation = det.signum();
                } else if det * orientation < 0.0 {
                    return Err(FieldError::Grid(
                        "rays cross each other near this plane (a caustic): convert the rays to a field elsewhere".into(),
                    ));
                }
                let (lo_x, hi_x) = (p0.0.min(p1.0).min(p2.0), p0.0.max(p1.0).max(p2.0));
                let (lo_y, hi_y) = (p0.1.min(p1.1).min(p2.1), p0.1.max(p1.1).max(p2.1));
                let index_of = |x: f64| x / spacing + (n / 2) as f64;
                let c0 = index_of(lo_x).ceil().max(0.0) as usize;
                let c1 = (index_of(hi_x).floor() as isize).min(n as isize - 1);
                let r0 = index_of(lo_y).ceil().max(0.0) as usize;
                let r1 = (index_of(hi_y).floor() as isize).min(n as isize - 1);
                if c1 < 0 || r1 < 0 {
                    continue;
                }
                for gr in r0..=r1 as usize {
                    for gc in c0..=c1 as usize {
                        let (x, y) = (field.position(gc), field.position(gr));
                        let l1 = ((x - p0.0) * (p2.1 - p0.1) - (p2.0 - p0.0) * (y - p0.1)) / det;
                        let l2 = ((p1.0 - p0.0) * (y - p0.1) - (x - p0.0) * (p1.1 - p0.1)) / det;
                        let l0 = 1.0 - l1 - l2;
                        if l0 < -1e-12 || l1 < -1e-12 || l2 < -1e-12 {
                            continue;
                        }
                        let amplitude = l0 * v0.0 + l1 * v1.0 + l2 * v2.0;
                        let phase = l0 * v0.1 + l1 * v1.1 + l2 * v2.1;
                        let e = Complex::new(phase.cos(), phase.sin()) * amplitude;
                        field.components[0][gr * n + gc] = jx * e;
                        field.components[1][gr * n + gc] = jy * e;
                    }
                }
            }
        }
    }
    field.curvature = curvature;
    Ok((field, reference))
}

/// The rays of a field: one per cell whose irradiance is at least `threshold` times the
/// peak. `reference` is the optical path the field's zero phase stands for.
pub fn rays_from_field(field: &Field, threshold: f64, reference: f64) -> Vec<RaySample> {
    let n = field.n;
    let peak = (0..n * n)
        .map(|i| field.irradiance(i / n, i % n))
        .fold(0.0, f64::max);
    let lit = |r: usize, c: usize| field.irradiance(r, c) >= threshold * peak && peak > 0.0;
    // The stronger component carries the phase.
    let component = usize::from(
        field.components[1]
            .iter()
            .map(|v| v.norm_squared())
            .sum::<f64>()
            > field.components[0]
                .iter()
                .map(|v| v.norm_squared())
                .sum::<f64>(),
    );
    let s = &field.components[component];
    // Unwrap the stored phase outwards from the brightest lit cell, step by step.
    let start = (0..n * n)
        .max_by(|&a, &b| {
            field
                .irradiance(a / n, a % n)
                .total_cmp(&field.irradiance(b / n, b % n))
        })
        .unwrap_or(0);
    let mut phase = vec![f64::NAN; n * n];
    let mut rough = vec![false; n * n];
    let mut queue = std::collections::VecDeque::new();
    if lit(start / n, start % n) {
        phase[start] = s[start].im.atan2(s[start].re);
        queue.push_back(start);
    }
    while let Some(i) = queue.pop_front() {
        let (r, c) = (i / n, i % n);
        let neighbours = [
            (r > 0).then(|| i - n),
            (r + 1 < n).then(|| i + n),
            (c > 0).then(|| i - 1),
            (c + 1 < n).then(|| i + 1),
        ];
        for j in neighbours.into_iter().flatten() {
            if !lit(j / n, j % n) {
                rough[i] = true;
                continue;
            }
            let step = s[j] * s[i].conjugate();
            let jump = step.im.atan2(step.re);
            if jump.abs() > core::f64::consts::FRAC_PI_2 {
                rough[i] = true;
                rough[j] = true;
            }
            if phase[j].is_nan() {
                phase[j] = phase[i] + jump;
                queue.push_back(j);
            }
        }
    }
    let k0 = 2.0 * core::f64::consts::PI / (field.wavelength * 1e-3);
    let k = k0 * field.index;
    let dx = field.spacing;
    let mut out = Vec::new();
    for r in 0..n {
        for c in 0..n {
            let i = r * n + c;
            if phase[i].is_nan() {
                continue;
            }
            let along = |a: Option<usize>, b: Option<usize>, h: f64| -> f64 {
                match (
                    a.filter(|&a| !phase[a].is_nan()),
                    b.filter(|&b| !phase[b].is_nan()),
                ) {
                    (Some(a), Some(b)) => (phase[a] - phase[b]) / (2.0 * h),
                    (Some(a), None) => (phase[a] - phase[i]) / h,
                    (None, Some(b)) => (phase[i] - phase[b]) / h,
                    (None, None) => 0.0,
                }
            };
            let (x, y) = (field.position(c), field.position(r));
            let gx = along((c + 1 < n).then(|| i + 1), (c > 0).then(|| i - 1), dx)
                + k * field.curvature * x;
            let gy = along((r + 1 < n).then(|| i + n), (r > 0).then(|| i - n), dx)
                + k * field.curvature * y;
            let (tx, ty) = (gx / k, gy / k);
            let tz2 = 1.0 - tx * tx - ty * ty;
            if tz2 <= 0.0 {
                continue;
            }
            out.push(RaySample {
                cell: (c, r),
                point: (x, y),
                direction: Vec3::new(tx, ty, tz2.sqrt()),
                power: field.irradiance(r, c) * dx * dx,
                path: reference + (phase[i] + k * field.curvature * (x * x + y * y) / 2.0) / k0,
                rough: rough[i],
            });
        }
    }
    out
}

/// The fraction of a field's power a single-mode fibre takes up: the overlap with its
/// mode, a Gaussian of 1/e² radius `mode_radius` (mm, half the mode-field diameter)
/// centred at `centre` with a flat phase, summed over the two polarizations,
/// |∫ E* M dA|² / (∫ |E|² dA ∫ |M|² dA) (e.g. A. W. Snyder and J. D. Love, Optical
/// Waveguide Theory, 1983, §20).
pub fn coupling(field: &Field, mode_radius: f64, centre: (f64, f64)) -> f64 {
    let n = field.n;
    let (mut overlap, mut field_power, mut mode_power) = ([Complex::ZERO; 2], 0.0, 0.0);
    for r in 0..n {
        for c in 0..n {
            let (x, y) = (field.position(c) - centre.0, field.position(r) - centre.1);
            let m = (-(x * x + y * y) / (mode_radius * mode_radius)).exp();
            mode_power += m * m;
            for (k, o) in overlap.iter_mut().enumerate() {
                let e = field.value(k, r, c);
                *o = *o + e.conjugate() * m;
                field_power += e.norm_squared();
            }
        }
    }
    if field_power <= 0.0 || mode_power <= 0.0 {
        return 0.0;
    }
    (overlap[0].norm_squared() + overlap[1].norm_squared()) / (field_power * mode_power)
}
