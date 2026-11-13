//! Heat in a sample (M11d): the temperature rise from absorbed light, steady or in time,
//! by finite volumes on a graded grid of rings about an axis.
//!
//! The rise ΔT above the starting temperature obeys the heat equation with Pennes'
//! perfusion term (H. H. Pennes, J. Appl. Physiol. 1, 93 (1948); in the form of
//! J. M. Stujenske, T. Spellman and J. A. Gordon, Cell Rep. 12, 525 (2015), Eq. (9)):
//!
//! ```text
//! ρc ∂ΔT/∂t = ∇·(k ∇ΔT) − w ΔT + q,     w = ρ_b c_b w_b
//! ```
//!
//! k the thermal conductivity (W mm⁻¹ K⁻¹), ρc the volumetric heat capacity
//! (J mm⁻³ K⁻¹), w the perfusion's heat exchange (W mm⁻³ K⁻¹; blood at the arterial
//! temperature) and q the absorbed power density (W mm⁻³). Written for the rise, the
//! baseline (metabolic heat balanced by perfusion and the surroundings) drops out; that
//! holds while the baseline is steady and every term is linear in T.
//!
//! Each cell exchanges heat with its neighbours through the conductance
//! area / (h₁ / (2k₁) + h₂ / (2k₂)) (half-cell resistances in series, so layers of
//! different k join correctly), loses w V ΔT to perfusion and takes in q V. Boundaries: fixed at the
//! starting temperature, insulated, or a heat transfer coefficient h to a bath at the
//! starting temperature. The steady state is solved by conjugate gradients; time steps by
//! Crank–Nicolson ((C/Δt + A/2) T¹ = (C/Δt − A/2) T⁰ + (Q⁰ + Q¹)/2), second order and
//! unconditionally stable. Heat absorbed, stored, conducted out through each boundary and
//! carried off by perfusion are summed from the same terms, so the balance closes to the
//! solver's tolerance.
//!
//! The grid is graded ([`graded`]): fine near the heat source, its cells growing by a
//! fixed ratio outwards, so a focus of micrometres and a domain of millimetres fit
//! together.

use crate::diffusion::conjugate_gradients;

/// Cell edges from `from` to `to`: steps of `step` within `fine` of `centre`, then each
/// step `growth` times the one before, the last shortened to end on `from` and `to`.
pub fn graded(from: f64, to: f64, centre: f64, step: f64, fine: f64, growth: f64) -> Vec<f64> {
    let centre = centre.clamp(from, to);
    let side = |end: f64| -> Vec<f64> {
        let sign = if end >= centre { 1.0 } else { -1.0 };
        let mut out = Vec::new();
        let (mut x, mut h) = (centre, step);
        while (end - x) * sign > 1e-12 * step {
            if (x - centre).abs() >= fine - 1e-12 * step {
                h *= growth;
            }
            let mut next = x + sign * h;
            if (end - next) * sign < 0.5 * h {
                next = end;
            }
            out.push(next);
            x = next;
        }
        out
    };
    let mut edges: Vec<f64> = side(from).into_iter().rev().collect();
    edges.push(centre);
    edges.extend(side(to));
    edges
}

/// Rings about an axis between radial edges `r` (from 0) and slices between axial edges
/// `z`, both increasing.
#[derive(Debug, Clone, PartialEq)]
pub struct ThermalGrid {
    /// Radial edges (mm), the first 0.
    pub r: Vec<f64>,
    /// Axial edges (mm).
    pub z: Vec<f64>,
}

impl ThermalGrid {
    /// Rings.
    pub fn rings(&self) -> usize {
        self.r.len() - 1
    }

    /// Slices.
    pub fn slices(&self) -> usize {
        self.z.len() - 1
    }

    /// Cells in all.
    pub fn cells(&self) -> usize {
        self.rings() * self.slices()
    }

    /// Index of cell (ring i, slice j).
    pub fn index(&self, i: usize, j: usize) -> usize {
        j * self.rings() + i
    }

    /// The centroid radius of ring `i`.
    pub fn r_at(&self, i: usize) -> f64 {
        let (a, b) = (self.r[i], self.r[i + 1]);
        2.0 * (b.powi(3) - a.powi(3)) / (3.0 * (b * b - a * a))
    }

    /// The middle of slice `j`.
    pub fn z_at(&self, j: usize) -> f64 {
        0.5 * (self.z[j] + self.z[j + 1])
    }

    /// Volume of cell (i, j) (mm³).
    pub fn volume(&self, i: usize, j: usize) -> f64 {
        core::f64::consts::PI
            * (self.r[i + 1].powi(2) - self.r[i].powi(2))
            * (self.z[j + 1] - self.z[j])
    }

    /// The cell holding (r, z), if inside.
    pub fn cell(&self, r: f64, z: f64) -> Option<(usize, usize)> {
        let find = |edges: &[f64], v: f64| {
            if v < edges[0] || v > edges[edges.len() - 1] {
                return None;
            }
            Some(edges.partition_point(|e| *e <= v).clamp(1, edges.len() - 1) - 1)
        };
        Some((find(&self.r, r)?, find(&self.z, z)?))
    }

    fn check(&self) -> Result<(), String> {
        let increasing = |e: &[f64]| e.len() >= 2 && e.windows(2).all(|w| w[1] > w[0]);
        if !increasing(&self.r) || !increasing(&self.z) || self.r[0] != 0.0 {
            return Err("the grid's edges must increase, the radial ones from 0".into());
        }
        Ok(())
    }
}

/// A boundary of the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThermalBoundary {
    /// Held at the starting temperature (a large heat sink, a temperature-controlled
    /// bath).
    Fixed,
    /// No heat crosses it.
    Insulated,
    /// Heat leaves at h ΔT per area (W mm⁻² K⁻¹) to a bath at the starting temperature.
    Transfer(f64),
}

/// The problem: per cell k, ρc and w, and the boundaries.
#[derive(Debug, Clone, PartialEq)]
pub struct Thermal {
    /// The grid.
    pub grid: ThermalGrid,
    /// Thermal conductivity per cell (W mm⁻¹ K⁻¹).
    pub conductivity: Vec<f64>,
    /// Volumetric heat capacity ρc per cell (J mm⁻³ K⁻¹).
    pub capacity: Vec<f64>,
    /// Perfusion ρ_b c_b w_b per cell (W mm⁻³ K⁻¹); 0 without.
    pub perfusion: Vec<f64>,
    /// At the first z edge.
    pub top: ThermalBoundary,
    /// At the last z edge.
    pub bottom: ThermalBoundary,
    /// At the outer radius.
    pub side: ThermalBoundary,
}

/// A steady temperature rise.
#[derive(Debug, Clone, PartialEq)]
pub struct Heat {
    /// ΔT per cell (K).
    pub rise: Vec<f64>,
    /// Power absorbed (W).
    pub absorbed: f64,
    /// Conducted out through the top, bottom and side (W).
    pub conducted: [f64; 3],
    /// Carried off by perfusion (W).
    pub perfused: f64,
    /// Conjugate-gradient iterations.
    pub iterations: usize,
}

/// A temperature rise in time.
#[derive(Debug, Clone, PartialEq)]
pub struct Course {
    /// The times reached (s): 0 and every step's end.
    pub times: Vec<f64>,
    /// ΔT at each probe cell, at each time (K): `probes[p][t]`.
    pub probes: Vec<Vec<f64>>,
    /// ΔT per cell at the end (K).
    pub rise: Vec<f64>,
    /// Heat absorbed (J).
    pub absorbed: f64,
    /// Heat stored at the end, Σ ρc V ΔT (J).
    pub stored: f64,
    /// Conducted out through the top, bottom and side (J).
    pub conducted: [f64; 3],
    /// Carried off by perfusion (J).
    pub perfused: f64,
}

/// The assembled operator: links (neighbour, conductance), the diagonal (conduction,
/// perfusion and boundaries), and per cell its boundary conductances (top, bottom, side)
/// and perfusion conductance w V.
struct Operator {
    links: Vec<Vec<(usize, f64)>>,
    diagonal: Vec<f64>,
    boundary: Vec<[f64; 3]>,
    perfusion: Vec<f64>,
}

impl Operator {
    fn apply(&self, x: &[f64], out: &mut [f64]) {
        for (k, o) in out.iter_mut().enumerate() {
            let mut v = self.diagonal[k] * x[k];
            for &(n, c) in &self.links[k] {
                v -= c * x[n];
            }
            *o = v;
        }
    }
}

/// The conductance per area from a cell's middle (half-width `half`, conductivity `k`)
/// through a boundary.
fn to_boundary(boundary: ThermalBoundary, k: f64, half: f64) -> f64 {
    match boundary {
        ThermalBoundary::Fixed => k / half,
        ThermalBoundary::Insulated => 0.0,
        ThermalBoundary::Transfer(h) => 1.0 / (half / k + 1.0 / h),
    }
}

impl Thermal {
    /// The same material in every cell.
    pub fn uniform(
        grid: ThermalGrid,
        conductivity: f64,
        capacity: f64,
        perfusion: f64,
        boundaries: [ThermalBoundary; 3],
    ) -> Self {
        let n = grid.cells();
        Self {
            grid,
            conductivity: vec![conductivity; n],
            capacity: vec![capacity; n],
            perfusion: vec![perfusion; n],
            top: boundaries[0],
            bottom: boundaries[1],
            side: boundaries[2],
        }
    }

    fn check(&self, source: &[f64]) -> Result<(), String> {
        self.grid.check()?;
        let n = self.grid.cells();
        if [
            self.conductivity.len(),
            self.capacity.len(),
            self.perfusion.len(),
            source.len(),
        ]
        .iter()
        .any(|&l| l != n)
        {
            return Err("the properties and the source must give one value per cell".into());
        }
        if self
            .conductivity
            .iter()
            .any(|k| !(*k > 0.0 && k.is_finite()))
            || self.capacity.iter().any(|c| !(*c > 0.0 && c.is_finite()))
            || self.perfusion.iter().any(|w| !(*w >= 0.0 && w.is_finite()))
        {
            return Err("k and ρc must be positive and w not negative in every cell".into());
        }
        for b in [self.top, self.bottom, self.side] {
            if let ThermalBoundary::Transfer(h) = b {
                if !(h > 0.0 && h.is_finite()) {
                    return Err("a heat transfer coefficient must be positive".into());
                }
            }
        }
        Ok(())
    }

    fn assemble(&self) -> Operator {
        let g = &self.grid;
        let pi = core::f64::consts::PI;
        let n = g.cells();
        let mut links = vec![Vec::with_capacity(4); n];
        let mut diagonal = vec![0.0; n];
        let mut boundary = vec![[0.0; 3]; n];
        let mut perfusion = vec![0.0; n];
        let (rings, slices) = (g.rings(), g.slices());
        for j in 0..slices {
            let dz = g.z[j + 1] - g.z[j];
            for i in 0..rings {
                let k = g.index(i, j);
                let dr = g.r[i + 1] - g.r[i];
                let kk = self.conductivity[k];
                perfusion[k] = self.perfusion[k] * g.volume(i, j);
                diagonal[k] += perfusion[k];
                // Outward radial face.
                let area_r = 2.0 * pi * g.r[i + 1] * dz;
                if i + 1 < rings {
                    let m = g.index(i + 1, j);
                    let dr_next = g.r[i + 2] - g.r[i + 1];
                    let c = area_r / (0.5 * dr / kk + 0.5 * dr_next / self.conductivity[m]);
                    links[k].push((m, c));
                    links[m].push((k, c));
                    diagonal[k] += c;
                    diagonal[m] += c;
                } else {
                    let c = area_r * to_boundary(self.side, kk, 0.5 * dr);
                    diagonal[k] += c;
                    boundary[k][2] += c;
                }
                // Axial face towards larger z.
                let area_z = pi * (g.r[i + 1].powi(2) - g.r[i].powi(2));
                if j + 1 < slices {
                    let m = g.index(i, j + 1);
                    let dz_next = g.z[j + 2] - g.z[j + 1];
                    let c = area_z / (0.5 * dz / kk + 0.5 * dz_next / self.conductivity[m]);
                    links[k].push((m, c));
                    links[m].push((k, c));
                    diagonal[k] += c;
                    diagonal[m] += c;
                } else {
                    let c = area_z * to_boundary(self.bottom, kk, 0.5 * dz);
                    diagonal[k] += c;
                    boundary[k][1] += c;
                }
                if j == 0 {
                    let c = area_z * to_boundary(self.top, kk, 0.5 * dz);
                    diagonal[k] += c;
                    boundary[k][0] += c;
                }
            }
        }
        Operator {
            links,
            diagonal,
            boundary,
            perfusion,
        }
    }

    /// Heat in per cell (W) from a power density (W mm⁻³).
    fn powers(&self, source: &[f64]) -> Vec<f64> {
        let g = &self.grid;
        let mut out = vec![0.0; g.cells()];
        for j in 0..g.slices() {
            for i in 0..g.rings() {
                let k = g.index(i, j);
                out[k] = source[k] * g.volume(i, j);
            }
        }
        out
    }

    /// The steady rise from power density `source` (W mm⁻³ per cell).
    pub fn steady(&self, source: &[f64]) -> Result<Heat, String> {
        self.check(source)?;
        let operator = self.assemble();
        let anchored = operator.boundary.iter().any(|b| b.iter().any(|c| *c > 0.0))
            || operator.perfusion.iter().any(|w| *w > 0.0);
        if !anchored {
            return Err(
                "with every boundary insulated and no perfusion there is no steady state".into(),
            );
        }
        let q = self.powers(source);
        let (rise, iterations) = conjugate_gradients(
            &|x, out| operator.apply(x, out),
            &operator.diagonal,
            &q,
            vec![0.0; q.len()],
        );
        let mut conducted = [0.0; 3];
        let mut perfused = 0.0;
        for (k, t) in rise.iter().enumerate() {
            for (side, c) in conducted.iter_mut().zip(operator.boundary[k]) {
                *side += c * t;
            }
            perfused += operator.perfusion[k] * t;
        }
        Ok(Heat {
            absorbed: q.iter().sum(),
            rise,
            conducted,
            perfused,
            iterations,
        })
    }

    /// The rise in time from power density `source` (W mm⁻³ per cell) switched by
    /// `drive(t)` (its fraction on at time t: 1 on, 0 off, between for a duty cycle),
    /// from no rise at t = 0, in steps of `step` to `duration` (the last shortened);
    /// `probes` are cells whose rise is kept at every step.
    pub fn transient(
        &self,
        source: &[f64],
        drive: &dyn Fn(f64) -> f64,
        duration: f64,
        step: f64,
        probes: &[usize],
    ) -> Result<Course, String> {
        self.check(source)?;
        if !(step > 0.0 && duration > 0.0) {
            return Err("the step and the duration must be positive".into());
        }
        if probes.iter().any(|&p| p >= self.grid.cells()) {
            return Err("a probe outside the grid".into());
        }
        let operator = self.assemble();
        let q = self.powers(source);
        let g = &self.grid;
        let mut heat = vec![0.0; g.cells()]; // ρc V per cell
        for j in 0..g.slices() {
            for i in 0..g.rings() {
                let k = g.index(i, j);
                heat[k] = self.capacity[k] * g.volume(i, j);
            }
        }
        let n = g.cells();
        let mut t_now = vec![0.0; n];
        let mut course = Course {
            times: vec![0.0],
            probes: probes.iter().map(|_| vec![0.0]).collect(),
            rise: Vec::new(),
            absorbed: 0.0,
            stored: 0.0,
            conducted: [0.0; 3],
            perfused: 0.0,
        };
        let mut time = 0.0;
        let mut a_t = vec![0.0; n];
        let mut rhs = vec![0.0; n];
        while time < duration - 1e-12 * step {
            let dt = step.min(duration - time);
            let (on0, on1) = (drive(time), drive(time + dt));
            operator.apply(&t_now, &mut a_t);
            for k in 0..n {
                rhs[k] = heat[k] / dt * t_now[k] - 0.5 * a_t[k] + 0.5 * q[k] * (on0 + on1);
            }
            let diagonal: Vec<f64> = (0..n)
                .map(|k| heat[k] / dt + 0.5 * operator.diagonal[k])
                .collect();
            let apply = |x: &[f64], out: &mut [f64]| {
                operator.apply(x, out);
                for k in 0..n {
                    out[k] = heat[k] / dt * x[k] + 0.5 * out[k];
                }
            };
            let (t_next, _) = conjugate_gradients(&apply, &diagonal, &rhs, t_now.clone());
            // Balances over the step (trapezoidal, as the scheme).
            for k in 0..n {
                let mean = 0.5 * (t_now[k] + t_next[k]);
                for (side, c) in course.conducted.iter_mut().zip(operator.boundary[k]) {
                    *side += dt * c * mean;
                }
                course.perfused += dt * operator.perfusion[k] * mean;
                course.absorbed += dt * q[k] * 0.5 * (on0 + on1);
            }
            t_now = t_next;
            time += dt;
            course.times.push(time);
            for (history, &p) in course.probes.iter_mut().zip(probes) {
                history.push(t_now[p]);
            }
        }
        course.stored = heat.iter().zip(&t_now).map(|(c, t)| c * t).sum();
        course.rise = t_now;
        Ok(course)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Water near 37 °C: k 0.623 W m⁻¹ K⁻¹ (J. W. Valvano, Table 12.9, in A. J. Welch and
    /// M. J. C. van Gemert, Optical-Thermal Response of Laser-Irradiated Tissue, 2nd ed.,
    /// 2011, 6.233 mW/(cm °C) at 310 K) and ρc = 993 kg m⁻³ × 4.178 kJ kg⁻¹ K⁻¹ (K. R.
    /// Diller, ibid., Table 10.2), in mm units.
    const K: f64 = 0.623e-3;
    const RHO_C: f64 = 993.0 * 4178.0 * 1e-9;
    const SIGMA: f64 = 0.01;
    const POWER: f64 = 1e-3;

    /// The error function, to 1.5 × 10⁻⁷ (M. Abramowitz and I. A. Stegun, Handbook of
    /// Mathematical Functions, 1964, 7.1.26).
    fn erf(x: f64) -> f64 {
        let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
        let y = 1.0
            - t * (0.254_829_592
                + t * (-0.284_496_736
                    + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))))
                * (-x * x).exp();
        y.copysign(x)
    }

    /// A spherical Gaussian of `POWER` and width σ at (0, z0), per cell (W mm⁻³), from
    /// its value at the cell centroid.
    fn gaussian(grid: &ThermalGrid, z0: f64) -> Vec<f64> {
        let peak = POWER / ((2.0 * core::f64::consts::PI).powf(1.5) * SIGMA.powi(3));
        let mut q = vec![0.0; grid.cells()];
        for j in 0..grid.slices() {
            for i in 0..grid.rings() {
                let (r, z) = (grid.r_at(i), grid.z_at(j) - z0);
                q[grid.index(i, j)] = peak * (-(r * r + z * z) / (2.0 * SIGMA * SIGMA)).exp();
            }
        }
        // Normalised to the power on the grid, so the cell sampling does not change it.
        let total: f64 = (0..grid.slices())
            .flat_map(|j| (0..grid.rings()).map(move |i| (i, j)))
            .map(|(i, j)| q[grid.index(i, j)] * grid.volume(i, j))
            .sum();
        q.iter().map(|v| v * POWER / total).collect()
    }

    fn infinite() -> ThermalGrid {
        ThermalGrid {
            r: graded(0.0, 2.0, 0.0, SIGMA / 5.0, 4.0 * SIGMA, 1.12),
            z: graded(-2.0, 2.0, 0.0, SIGMA / 5.0, 4.0 * SIGMA, 1.12),
        }
    }

    #[test]
    fn graded_edges_end_on_their_limits() {
        let e = graded(-1.0, 2.0, 0.0, 0.01, 0.05, 1.2);
        assert_eq!((e[0], e[e.len() - 1]), (-1.0, 2.0));
        assert!(e.windows(2).all(|w| w[1] > w[0]));
        assert!(e.contains(&0.0));
        let steps: Vec<f64> = e.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(steps.iter().cloned().fold(f64::INFINITY, f64::min) > 0.009);
        let r = graded(0.0, 1.0, 0.0, 0.1, 0.0, 1.0);
        assert_eq!(r.len(), 11);
    }

    #[test]
    fn a_gaussian_source_in_an_infinite_medium() {
        // ΔT(r) = P erf(r / (√2 σ)) / (4π k r), ΔT(0) = P / ((2π)^{3/2} k σ): the Green's
        // function 1 / (4π k r) integrated over the source (e.g. H. S. Carslaw and J. C.
        // Jaeger, Conduction of Heat in Solids, 2nd ed., 1959, ch. 10). The grid ends 2 mm
        // out held at 0, which lowers the whole field by about P / (4π k · 2 mm), 0.6 % of
        // ΔT(0); differences from the centre do not see it.
        let grid = infinite();
        let problem = Thermal::uniform(grid.clone(), K, RHO_C, 0.0, [ThermalBoundary::Fixed; 3]);
        let heat = problem.steady(&gaussian(&grid, 0.0)).expect("solves");
        let exact = |r: f64| {
            if r < 1e-9 {
                POWER / ((2.0 * core::f64::consts::PI).powf(1.5) * K * SIGMA)
            } else {
                POWER * erf(r / (2f64.sqrt() * SIGMA)) / (4.0 * core::f64::consts::PI * K * r)
            }
        };
        let (_, j0) = grid.cell(0.0, 1e-9).expect("inside");
        let centre = 0.5 * (heat.rise[grid.index(0, j0)] + heat.rise[grid.index(0, j0 - 1)]);
        assert!(
            (centre / exact(0.0) - 1.0).abs() < 0.015,
            "{centre} {}",
            exact(0.0)
        );
        for j in j0..grid.slices() {
            let z = grid.z_at(j);
            if !(2.0 * SIGMA..20.0 * SIGMA).contains(&z) {
                continue;
            }
            let found = centre - heat.rise[grid.index(0, j)];
            let expected = exact(0.0) - exact(z);
            assert!(
                (found / expected - 1.0).abs() < 0.01,
                "z {z}: {found} {expected}"
            );
        }
        // Absorbed = conducted out.
        let out: f64 = heat.conducted.iter().sum::<f64>() + heat.perfused;
        assert!(
            (out / heat.absorbed - 1.0).abs() < 1e-9,
            "{out} {}",
            heat.absorbed
        );
        assert!((heat.absorbed - POWER).abs() < 1e-15);
    }

    #[test]
    fn the_rise_at_the_centre_in_time() {
        // Switched on at t = 0: ΔT(0, t) = P / ((2π)^{3/2} k) (1/σ − 1/√(σ² + 2αt)), α = k/ρc
        // (the Gaussian's spread σ² + 2αs integrated over the time since emission).
        let grid = infinite();
        let problem = Thermal::uniform(grid.clone(), K, RHO_C, 0.0, [ThermalBoundary::Fixed; 3]);
        let alpha = K / RHO_C;
        let tau = SIGMA * SIGMA / alpha;
        let (_, j0) = grid.cell(0.0, 1e-9).expect("inside");
        let probes = [grid.index(0, j0), grid.index(0, j0 - 1)];
        let course = problem
            .transient(
                &gaussian(&grid, 0.0),
                &|_| 1.0,
                10.0 * tau,
                tau / 16.0,
                &probes,
            )
            .expect("runs");
        let scale = POWER / ((2.0 * core::f64::consts::PI).powf(1.5) * K);
        for (n, &t) in course.times.iter().enumerate().skip(1) {
            if t < 0.5 * tau {
                continue;
            }
            let found = 0.5 * (course.probes[0][n] + course.probes[1][n]);
            let expected = scale * (1.0 / SIGMA - 1.0 / (SIGMA * SIGMA + 2.0 * alpha * t).sqrt());
            assert!(
                (found / expected - 1.0).abs() < 0.015,
                "t {t}: {found} {expected}"
            );
        }
        // Absorbed = stored + conducted out.
        let out = course.stored + course.conducted.iter().sum::<f64>() + course.perfused;
        assert!(
            (out / course.absorbed - 1.0).abs() < 1e-9,
            "{out} {}",
            course.absorbed
        );
        // Switched off, the heat drains away: an on–off drive ends lower than staying on.
        let pulsed = problem
            .transient(
                &gaussian(&grid, 0.0),
                &|t| if t < 5.0 * tau { 1.0 } else { 0.0 },
                10.0 * tau,
                tau / 16.0,
                &probes,
            )
            .expect("runs");
        assert!(pulsed.probes[0].last() < course.probes[0].last());
        let out = pulsed.stored + pulsed.conducted.iter().sum::<f64>();
        assert!((out / pulsed.absorbed - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_source_under_a_held_surface_and_its_image() {
        // A half-space z > 0 with its surface held at the starting temperature, the source
        // d = 0.2 mm deep: ΔT = P / (4π k) (1/r₁ − 1/r₂), the image at −d (Carslaw and
        // Jaeger, ch. 10), on the axis away from the source.
        let d = 20.0 * SIGMA;
        let grid = ThermalGrid {
            r: graded(0.0, 3.0, 0.0, SIGMA / 4.0, 4.0 * SIGMA, 1.12),
            z: graded(0.0, 3.0, d, SIGMA / 4.0, 4.0 * SIGMA, 1.12),
        };
        let problem = Thermal::uniform(grid.clone(), K, RHO_C, 0.0, [ThermalBoundary::Fixed; 3]);
        let heat = problem.steady(&gaussian(&grid, d)).expect("solves");
        let mut checked = 0;
        for j in 0..grid.slices() {
            let z = grid.z_at(j);
            let r1 = (z - d).abs();
            if r1 < 5.0 * SIGMA || z > 0.6 {
                continue;
            }
            let r2 = z + d;
            let expected = POWER / (4.0 * core::f64::consts::PI * K) * (1.0 / r1 - 1.0 / r2);
            let found = heat.rise[grid.index(0, j)];
            assert!(
                (found / expected - 1.0).abs() < 0.02,
                "z {z}: {found} {expected}"
            );
            checked += 1;
        }
        assert!(checked > 10);
        // Most of the heat leaves through the near surface.
        assert!(
            heat.conducted[0] > 0.8 * heat.absorbed,
            "{:?}",
            heat.conducted
        );
    }

    #[test]
    fn perfusion_shortens_the_reach_of_the_heat() {
        // Pennes in an infinite medium: ΔT = P e^{−r/L} / (4π k r), L = √(k / w), times
        // e^{σ² / (2L²)} for a Gaussian source seen from r ≫ σ (its moment generating
        // function); L = 0.1 mm here.
        let l: f64 = 0.1;
        let w = K / (l * l);
        let grid = infinite();
        let problem = Thermal::uniform(grid.clone(), K, RHO_C, w, [ThermalBoundary::Fixed; 3]);
        let heat = problem.steady(&gaussian(&grid, 0.0)).expect("solves");
        let (_, j0) = grid.cell(0.0, 1e-9).expect("inside");
        for j in j0..grid.slices() {
            let r = grid.z_at(j);
            if !(6.0 * SIGMA..40.0 * SIGMA).contains(&r) {
                continue;
            }
            let expected = POWER * (SIGMA * SIGMA / (2.0 * l * l)).exp() * (-r / l).exp()
                / (4.0 * core::f64::consts::PI * K * r);
            let found = heat.rise[grid.index(0, j)];
            assert!(
                (found / expected - 1.0).abs() < 0.02,
                "r {r}: {found} {expected}"
            );
        }
        // Absorbed = perfused + conducted; nearly all perfused.
        let out = heat.perfused + heat.conducted.iter().sum::<f64>();
        assert!((out / heat.absorbed - 1.0).abs() < 1e-9);
        assert!(heat.perfused > 0.99 * heat.absorbed);
        // No steady state when nothing takes the heat away.
        let closed = Thermal::uniform(grid.clone(), K, RHO_C, 0.0, [ThermalBoundary::Insulated; 3]);
        assert!(closed.steady(&gaussian(&grid, 0.0)).is_err());
    }
}
