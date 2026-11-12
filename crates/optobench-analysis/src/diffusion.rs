//! The diffusion approximation for scattered light (M11a2), on a grid of rings about an
//! axis: (r, z) cells, axially symmetric.
//!
//! The diffuse fluence rate Φ (W/mm²) obeys −∇·(D ∇Φ) + µa Φ = S, with D = 1/(3 (µa + µs′)),
//! µs′ = µs (1 − g), and S the power scattered out of the direct beam per volume (W/mm³)
//! (e.g. A. J. Welch and M. J. C. van Gemert, Optical-Thermal Response of Laser-Irradiated
//! Tissue, 2nd ed., 2011, ch. 6). It is solved by finite volumes: each cell exchanges
//! D ∇Φ through its faces (D between cells by its harmonic mean), absorbs µa Φ over its
//! volume and takes in its share of S. The resulting system is symmetric and positive
//! definite and is solved by conjugate gradients.
//!
//! Boundaries: at the side (r = R) the fluence is taken as zero (the grid reaches far
//! enough for that); at the top and bottom either zero, or the partial-current condition
//! of a face between the turbid medium and a clear one, outward flux Φ / (2A) with
//! A = (1 + R_eff) / (1 − R_eff) for the relative index n = n_inside / n_outside and
//! R_eff ≈ −1.440/n² + 0.710/n + 0.668 + 0.0636 n (R. A. J. Groenhuis, H. A. Ferwerda and
//! J. J. Ten Bosch, Appl. Opt. 22, 2456 (1983); used so by R. C. Haskell et al., J. Opt.
//! Soc. Am. A 11, 2727 (1994)).
//!
//! The light that leaves through each boundary and the light absorbed are summed from the
//! same flux terms, so the power balance closes to the solver's tolerance.

/// Rings about an axis: radius `radius` in `rings` cells, z from `z.0` to `z.1` in
/// `layers` cells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RzGrid {
    /// Outer radius (mm).
    pub radius: f64,
    /// Cells across the radius.
    pub rings: usize,
    /// The z range (mm).
    pub z: (f64, f64),
    /// Cells along z.
    pub slices: usize,
}

impl RzGrid {
    /// The radial step (mm).
    pub fn dr(&self) -> f64 {
        self.radius / self.rings as f64
    }

    /// The axial step (mm).
    pub fn dz(&self) -> f64 {
        (self.z.1 - self.z.0) / self.slices as f64
    }

    /// Cells in all.
    pub fn cells(&self) -> usize {
        self.rings * self.slices
    }

    /// Index of cell (ring i, slice j).
    pub fn index(&self, i: usize, j: usize) -> usize {
        j * self.rings + i
    }

    /// The radius at the middle of ring `i` (by area: half the ring's area inside).
    pub fn r(&self, i: usize) -> f64 {
        (i as f64 + 0.5) * self.dr()
    }

    /// z at the middle of slice `j`.
    pub fn z_at(&self, j: usize) -> f64 {
        self.z.0 + (j as f64 + 0.5) * self.dz()
    }

    /// Volume of ring `i`'s cells (mm³).
    pub fn volume(&self, i: usize) -> f64 {
        let (a, b) = (i as f64 * self.dr(), (i + 1) as f64 * self.dr());
        core::f64::consts::PI * (b * b - a * a) * self.dz()
    }

    /// The cell holding (r, z), if inside.
    pub fn cell(&self, r: f64, z: f64) -> Option<(usize, usize)> {
        if !(0.0..self.radius).contains(&r) || !(self.z.0..self.z.1).contains(&z) {
            return None;
        }
        let i = ((r / self.dr()) as usize).min(self.rings - 1);
        let j = (((z - self.z.0) / self.dz()) as usize).min(self.slices - 1);
        Some((i, j))
    }
}

/// A top or bottom boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Boundary {
    /// Fluence zero at the face.
    Zero,
    /// Partial current: outward flux Φ / (2A).
    Partial {
        /// A = (1 + R_eff) / (1 − R_eff).
        a: f64,
    },
}

impl Boundary {
    /// The partial-current boundary of a face from a medium of index `inside` to one of
    /// `outside` (Groenhuis et al. 1983).
    pub fn between(inside: f64, outside: f64) -> Self {
        let n = inside / outside;
        let reflection = -1.440 / (n * n) + 0.710 / n + 0.668 + 0.0636 * n;
        Boundary::Partial {
            a: (1.0 + reflection) / (1.0 - reflection),
        }
    }
}

/// The problem: per cell µa (1/mm) and D (mm), and the boundaries.
#[derive(Debug, Clone, PartialEq)]
pub struct Diffusion {
    /// The grid.
    pub grid: RzGrid,
    /// µa per cell.
    pub absorption: Vec<f64>,
    /// D per cell.
    pub diffusion: Vec<f64>,
    /// At z = z.0.
    pub top: Boundary,
    /// At z = z.1.
    pub bottom: Boundary,
}

/// The diffuse light.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffuseLight {
    /// Fluence rate per cell (W/mm²).
    pub fluence: Vec<f64>,
    /// Power absorbed (W).
    pub absorbed: f64,
    /// Power leaving through the top (W).
    pub top: f64,
    /// Through the bottom (W).
    pub bottom: f64,
    /// Through the side (W).
    pub side: f64,
    /// Conjugate-gradient iterations taken.
    pub iterations: usize,
}

/// The assembled system: per cell its (neighbour, conductance) links, its diagonal term
/// (absorption, links and boundaries) and its boundary conductances (top, bottom, side).
type Assembled = (Vec<Vec<(usize, f64)>>, Vec<f64>, Vec<[f64; 3]>);

/// Conductance of a half-cell of width `h` and diffusion `d`, per unit area, in series
/// with a boundary.
fn to_boundary(boundary: Boundary, d: f64, h: f64) -> f64 {
    let inside = 2.0 * d / h;
    match boundary {
        Boundary::Zero => inside,
        Boundary::Partial { a } => {
            let out = 1.0 / (2.0 * a);
            inside * out / (inside + out)
        }
    }
}

impl Diffusion {
    /// A problem with µa and D per slice (the same in every ring), for layered media.
    pub fn layered(
        grid: RzGrid,
        per_slice: &[(f64, f64)],
        top: Boundary,
        bottom: Boundary,
    ) -> Self {
        let mut absorption = vec![0.0; grid.cells()];
        let mut diffusion = vec![0.0; grid.cells()];
        for (j, &(mu_a, d)) in per_slice.iter().enumerate().take(grid.slices) {
            for i in 0..grid.rings {
                let k = grid.index(i, j);
                absorption[k] = mu_a;
                diffusion[k] = d;
            }
        }
        Self {
            grid,
            absorption,
            diffusion,
            top,
            bottom,
        }
    }

    /// The couplings of each cell: (neighbour, conductance) and its own diagonal term
    /// (absorption plus boundaries), and the boundary conductances (top, bottom, side).
    fn assemble(&self) -> Assembled {
        let g = &self.grid;
        let (dr, dz) = (g.dr(), g.dz());
        let pi = core::f64::consts::PI;
        let mut links = vec![Vec::with_capacity(4); g.cells()];
        let mut diagonal = vec![0.0; g.cells()];
        let mut boundary = vec![[0.0; 3]; g.cells()];
        let harmonic = |a: f64, b: f64| {
            if a + b > 0.0 {
                2.0 * a * b / (a + b)
            } else {
                0.0
            }
        };
        for j in 0..g.slices {
            for i in 0..g.rings {
                let k = g.index(i, j);
                let d = self.diffusion[k];
                diagonal[k] += self.absorption[k] * g.volume(i);
                // Outward radial face at r = (i + 1) dr.
                let area_r = 2.0 * pi * (i + 1) as f64 * dr * dz;
                if i + 1 < g.rings {
                    let n = g.index(i + 1, j);
                    let c = area_r * harmonic(d, self.diffusion[n]) / dr;
                    links[k].push((n, c));
                    links[n].push((k, c));
                    diagonal[k] += c;
                    diagonal[n] += c;
                } else {
                    let c = area_r * to_boundary(Boundary::Zero, d, dr);
                    diagonal[k] += c;
                    boundary[k][2] += c;
                }
                // Axial face below (towards larger z).
                let area_z = pi * ((i + 1) as f64 * dr).powi(2) - pi * (i as f64 * dr).powi(2);
                if j + 1 < g.slices {
                    let n = g.index(i, j + 1);
                    let c = area_z * harmonic(d, self.diffusion[n]) / dz;
                    links[k].push((n, c));
                    links[n].push((k, c));
                    diagonal[k] += c;
                    diagonal[n] += c;
                } else {
                    let c = area_z * to_boundary(self.bottom, d, dz);
                    diagonal[k] += c;
                    boundary[k][1] += c;
                }
                if j == 0 {
                    let c = area_z * to_boundary(self.top, d, dz);
                    diagonal[k] += c;
                    boundary[k][0] += c;
                }
            }
        }
        (links, diagonal, boundary)
    }

    /// Solves for the diffuse fluence of `source` (W per cell), to a relative residual of
    /// 10⁻¹².
    pub fn solve(&self, source: &[f64]) -> Result<DiffuseLight, String> {
        let g = &self.grid;
        if source.len() != g.cells()
            || self.absorption.len() != g.cells()
            || self.diffusion.len() != g.cells()
        {
            return Err("the source and coefficients must give one value per cell".into());
        }
        // Not positive, or NaN.
        if self
            .diffusion
            .iter()
            .any(|d| d.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater))
        {
            return Err("D must be positive in every cell (the medium must scatter)".into());
        }
        let (links, diagonal, boundary) = self.assemble();
        let apply = |x: &[f64], out: &mut [f64]| {
            for (k, o) in out.iter_mut().enumerate() {
                let mut v = diagonal[k] * x[k];
                for &(n, c) in &links[k] {
                    v -= c * x[n];
                }
                *o = v;
            }
        };
        // Conjugate gradients with the diagonal as preconditioner.
        let n = g.cells();
        let mut x = vec![0.0; n];
        let mut r: Vec<f64> = source.to_vec();
        let mut z: Vec<f64> = r.iter().zip(&diagonal).map(|(r, d)| r / d).collect();
        let mut p = z.clone();
        let mut rz: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
        let norm = source.iter().map(|s| s * s).sum::<f64>().sqrt();
        let mut ap = vec![0.0; n];
        let mut iterations = 0;
        if norm > 0.0 {
            for _ in 0..20 * n.max(100) {
                apply(&p, &mut ap);
                let pap: f64 = p.iter().zip(&ap).map(|(a, b)| a * b).sum();
                if pap <= 0.0 {
                    break;
                }
                let alpha = rz / pap;
                for k in 0..n {
                    x[k] += alpha * p[k];
                    r[k] -= alpha * ap[k];
                }
                iterations += 1;
                if r.iter().map(|v| v * v).sum::<f64>().sqrt() <= 1e-12 * norm {
                    break;
                }
                for k in 0..n {
                    z[k] = r[k] / diagonal[k];
                }
                let next: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
                let beta = next / rz;
                rz = next;
                for k in 0..n {
                    p[k] = z[k] + beta * p[k];
                }
            }
        }
        // Cell powers are W; fluence rate is power per cell over ... the unknown is Φ.
        let (mut absorbed, mut top, mut bottom, mut side) = (0.0, 0.0, 0.0, 0.0);
        for j in 0..g.slices {
            for i in 0..g.rings {
                let k = g.index(i, j);
                absorbed += self.absorption[k] * g.volume(i) * x[k];
                top += boundary[k][0] * x[k];
                bottom += boundary[k][1] * x[k];
                side += boundary[k][2] * x[k];
            }
        }
        Ok(DiffuseLight {
            fluence: x,
            absorbed,
            top,
            bottom,
            side,
            iterations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_source_in_an_infinite_medium() {
        // µa 0.01, µs′ 1 /mm: D = 1/3.03 mm, µ_eff = √(µa / D) ≈ 0.174 /mm. 1 W at the
        // centre of a grid reaching ~5/µ_eff each way; the fluence along the axis against
        // Φ = P exp(−µ_eff r) / (4π D r) (Welch and van Gemert, ch. 6).
        let (mu_a, mu_s_reduced): (f64, f64) = (0.01, 1.0);
        let d = 1.0 / (3.0 * (mu_a + mu_s_reduced));
        let mu_eff = (mu_a / d).sqrt();
        let grid = RzGrid {
            radius: 30.0,
            rings: 120,
            z: (-30.0, 30.0),
            slices: 240,
        };
        let problem = Diffusion::layered(
            grid,
            &vec![(mu_a, d); grid.slices],
            Boundary::Zero,
            Boundary::Zero,
        );
        let mut source = vec![0.0; grid.cells()];
        let (_, centre) = grid.cell(0.0, 0.0).expect("inside");
        source[grid.index(0, centre)] = 1.0;
        let light = problem.solve(&source).expect("solves");
        for j in (centre + 8)..(centre + 60) {
            let r = grid.z_at(j) - grid.z_at(centre);
            let expected = (-mu_eff * r).exp() / (4.0 * core::f64::consts::PI * d * r);
            let got = light.fluence[grid.index(0, j)];
            assert!(
                (got / expected - 1.0).abs() < 0.02,
                "r = {r}: {got} vs {expected}"
            );
        }
        // Everything is absorbed or leaves at the far boundaries.
        let total = light.absorbed + light.top + light.bottom + light.side;
        assert!((total - 1.0).abs() < 1e-9, "{total}");
        // A few per cent reach the grid's edge, ~5/µ_eff away.
        assert!(light.absorbed > 0.95, "{}", light.absorbed);
    }

    #[test]
    fn a_point_source_under_a_matched_surface_and_its_image() {
        // The same medium as a half-space below z = 0 with a partial-current face to a
        // clear medium of the same index (A ≈ 1), a source 1/µs′ deep: against the
        // extrapolated-boundary image solution Φ = P/(4πD) (e^{−µ r₁}/r₁ − e^{−µ r₂}/r₂),
        // the image at −(z₀ + 2 z_b), z_b = 2AD (T. J. Farrell, M. S. Patterson and
        // B. Wilson, Med. Phys. 19, 879 (1992)), away from the source and the face.
        let (mu_a, mu_s_reduced): (f64, f64) = (0.01, 1.0);
        let d = 1.0 / (3.0 * (mu_a + mu_s_reduced));
        let mu_eff = (mu_a / d).sqrt();
        let grid = RzGrid {
            radius: 30.0,
            rings: 120,
            z: (0.0, 30.0),
            slices: 120,
        };
        let surface = Boundary::between(1.0, 1.0);
        let Boundary::Partial { a } = surface else {
            panic!("partial")
        };
        assert!((a - 1.0).abs() < 0.01, "{a}");
        let problem =
            Diffusion::layered(grid, &vec![(mu_a, d); grid.slices], surface, Boundary::Zero);
        let z0 = 1.0 / mu_s_reduced;
        let mut source = vec![0.0; grid.cells()];
        let (_, j0) = grid.cell(0.0, z0).expect("inside");
        source[grid.index(0, j0)] = 1.0;
        let light = problem.solve(&source).expect("solves");
        let zs = grid.z_at(j0);
        let image = -(zs + 4.0 * a * d);
        for j in (j0 + 8)..(j0 + 40) {
            let z = grid.z_at(j);
            let (r1, r2) = (z - zs, z - image);
            let expected = ((-mu_eff * r1).exp() / r1 - (-mu_eff * r2).exp() / r2)
                / (4.0 * core::f64::consts::PI * d);
            let got = light.fluence[grid.index(0, j)];
            assert!(
                (got / expected - 1.0).abs() < 0.03,
                "z = {z}: {got} vs {expected}"
            );
        }
        let total = light.absorbed + light.top + light.bottom + light.side;
        assert!((total - 1.0).abs() < 1e-9, "{total}");
        assert!(
            light.top > 0.05,
            "some light leaves through the surface: {}",
            light.top
        );
    }

    /// Star's diffuse fluence rate under a wide collimated beam on a half-space, for unit
    /// irradiance entering the medium: φs = 2 (F₊ + F₋) with F± from W. M. Star, "Diffusion
    /// theory of light transport", ch. 6 of Welch and van Gemert (2nd ed., 2011), Eqs.
    /// (6.72), (6.73), (6.77) and (6.82a, b), boundary F₊(0) = r₂₁ F₋(0). Taken with g = 0,
    /// where his source terms and ours (first scattering isotropic) are the same.
    fn star_half_space(mu_a: f64, mu_s: f64, r21: f64, z: f64) -> f64 {
        let mu_t = mu_a + mu_s;
        let mu_eff = (3.0 * mu_a * mu_t).sqrt();
        let s_plus = mu_s / 4.0 * (5.0 * mu_a + 5.0 * mu_s);
        let s_minus = mu_s / 4.0 * (mu_a + mu_s);
        let q = (mu_eff - 2.0 * mu_a) / (mu_eff + 2.0 * mu_a);
        let k = mu_t * mu_t - mu_eff * mu_eff;
        let homogeneous = (s_plus - r21 * s_minus) / (k * (1.0 - r21 * q));
        let f_plus = homogeneous * (-mu_eff * z).exp() - s_plus / k * (-mu_t * z).exp();
        let f_minus = q * homogeneous * (-mu_eff * z).exp() - s_minus / k * (-mu_t * z).exp();
        2.0 * (f_plus + f_minus)
    }

    #[test]
    fn a_collimated_beam_on_a_half_space() {
        // µa 0.1, µs 1 /mm, g = 0: µ_eff ≈ 0.574 /mm. A beam of 1 W/mm² over the whole
        // grid (radius ~8.6/µ_eff, so the dark side is far from the axis), scattered out of
        // the direct beam at µs e^{−µt z}; the diffuse fluence on the axis against Star's
        // closed form (above), index matched (r₂₁ ≈ 0) and for n = 1.4 against air, with
        // r₂₁ = (A − 1)/(A + 1) from the A the solver uses.
        let (mu_a, mu_s): (f64, f64) = (0.1, 1.0);
        let mu_t = mu_a + mu_s;
        let d = 1.0 / (3.0 * mu_t);
        let grid = RzGrid {
            radius: 15.0,
            rings: 60,
            z: (0.0, 15.0),
            slices: 300,
        };
        let mut source = vec![0.0; grid.cells()];
        for j in 0..grid.slices {
            let (za, zb) = (
                grid.z.0 + j as f64 * grid.dz(),
                grid.z.0 + (j + 1) as f64 * grid.dz(),
            );
            let per_area = mu_s / mu_t * ((-mu_t * za).exp() - (-mu_t * zb).exp());
            for i in 0..grid.rings {
                source[grid.index(i, j)] = per_area * grid.volume(i) / grid.dz();
            }
        }
        let scattered: f64 = source.iter().sum();
        for inside in [1.0, 1.4] {
            let surface = Boundary::between(inside, 1.0);
            let Boundary::Partial { a } = surface else {
                panic!("partial")
            };
            let r21 = (a - 1.0) / (a + 1.0);
            let problem =
                Diffusion::layered(grid, &vec![(mu_a, d); grid.slices], surface, Boundary::Zero);
            let light = problem.solve(&source).expect("solves");
            for j in (0..grid.slices).step_by(10).take(14) {
                let z = grid.z_at(j);
                let expected = star_half_space(mu_a, mu_s, r21, z);
                let got = light.fluence[grid.index(0, j)];
                assert!(
                    (got / expected - 1.0).abs() < 0.01,
                    "n = {inside}, z = {z}: {got} vs {expected}"
                );
            }
            let total = light.absorbed + light.top + light.bottom + light.side;
            assert!(
                (total / scattered - 1.0).abs() < 1e-9,
                "{total} vs {scattered}"
            );
        }
    }

    #[test]
    fn the_effective_reflection_of_an_index_mismatch() {
        // R_eff for n = 1.4 is about 0.53, so A ≈ 3.25 (Haskell et al. 1994, table 1 range);
        // matched (n = 1): A ≈ 1.
        let Boundary::Partial { a } = Boundary::between(1.4, 1.0) else {
            panic!()
        };
        assert!((a - 3.25).abs() < 0.02, "{a}");
        let Boundary::Partial { a } = Boundary::between(1.0, 1.0) else {
            panic!()
        };
        assert!((a - 1.0).abs() < 0.01, "{a}");
    }
}
