//! Heating of a sample by the light absorbed in it (M11d): the absorbed power density of
//! M11a (direct beam and diffuse light, [`crate::sample`]) as the heat source of the
//! thermal solver ([`crate::thermal`]) over the sample's layers, with their thermal
//! properties from the design.
//!
//! - **Grid.** The sample's layers, from its first face to its last and out to its radius,
//!   on a graded grid: fine (the light grid's steps) around the beam's axis and the focal
//!   depth, growing outwards, every layer interface a cell edge so each cell has one
//!   layer's properties. The light grid's absorbed power is moved onto it by the volumes
//!   the cells share, which keeps the power.
//! - **A scanned field.** A beam scanned over a square field of side a, frame after
//!   frame, heats as its absorbed density spread evenly over the field and weighted by
//!   the duty cycle (the fraction of time the beam is on the field), while a frame is
//!   much shorter than the time heat takes to cross the field, about a² / (4α) (seconds
//!   for 1 mm of water; α = k / ρc). On the axially symmetric grid the field is the disk
//!   of the same area, radius a / √π: the parked beam's density at each radius spread by
//!   the share of its ring that falls in the disk around each point.
//! - **Reports.** The power entering the sample's first face and crossing the focal plane
//!   (the traced rays' power there, ballistic), the power absorbed per layer, the peak
//!   rise and where, the rise at the focus and averaged over the field at the focal depth.
//!
//! What is left out: light absorbed outside the light grid (in clear layers when the
//! sample has turbid ones: the light grid covers the turbid run), and the diffusion
//! approximation's limits near the surface and the focus (until M15). Detectors are left
//! out of the trace, so none inside the sample stops the light.

use optobench_scene::{ElementKind, HeatBoundary, Owner, Scene};
use optobench_trace::nonsequential::NO_MEDIUM;

use crate::diffusion::RzGrid;
use crate::sample::{LightSettings, SampleLight, sample_light};
use crate::thermal::{Course, Thermal, ThermalBoundary, ThermalGrid, graded};

/// A beam scanned over a field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScanField {
    /// The square field's side (mm).
    pub side: f64,
    /// The fraction of time the beam is on the field (the duty cycle).
    pub duty: f64,
}

/// Steady state, or a time course.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeatingMode {
    /// The steady rise.
    Steady,
    /// The rise in time from switching on at t = 0.
    Time {
        /// How long (s).
        duration: f64,
        /// The time step (s).
        step: f64,
        /// Switched off at this time (s); `None`: on throughout.
        off_at: Option<f64>,
    },
}

/// How to compute the heating.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatingSettings {
    /// The light's grid.
    pub light: LightSettings,
    /// The thermal grid's growth ratio outside its fine region.
    pub growth: f64,
    /// A scanned field; `None` for a parked beam.
    pub field: Option<ScanField>,
    /// The focal depth from the first face (mm); by default where the direct fluence on
    /// the axis peaks.
    pub focal_depth: Option<f64>,
    /// Steady or in time.
    pub mode: HeatingMode,
    /// Points (radius, depth from the first face, mm) whose rise is followed in time; by
    /// default the axis at the focal depth and at the first face.
    pub probes: Vec<(f64, f64)>,
}

impl Default for HeatingSettings {
    fn default() -> Self {
        Self {
            light: LightSettings::default(),
            growth: 1.1,
            field: None,
            focal_depth: None,
            mode: HeatingMode::Steady,
            probes: Vec::new(),
        }
    }
}

/// The heating of a sample.
#[derive(Debug, Clone, PartialEq)]
pub struct Heating {
    /// The sample's id.
    pub sample: String,
    /// The light in it.
    pub light: SampleLight,
    /// The thermal grid (z the depth from the first face).
    pub grid: ThermalGrid,
    /// Power entering the sample's first face (W).
    pub surface_power: f64,
    /// The focal depth (mm).
    pub focal_depth: f64,
    /// Power crossing the focal plane in the traced rays (ballistic, W).
    pub focal_power: f64,
    /// Heat absorbed on the thermal grid (W), and per layer (`layer k`, W).
    pub absorbed: f64,
    /// Per layer.
    pub absorbed_by_layer: Vec<(String, f64)>,
    /// The heat source on the thermal grid (W mm⁻³), after spreading over the field.
    pub source: Vec<f64>,
    /// The rise per cell: steady, or at the end of the time course (K).
    pub rise: Vec<f64>,
    /// The greatest rise and where: (K, radius mm, depth mm).
    pub peak: (f64, f64, f64),
    /// The rise on the axis at the focal depth (K).
    pub at_focus: f64,
    /// The rise averaged over the field at the focal depth (K), for a scanned field.
    pub field_average: Option<f64>,
    /// Heat conducted out through the first face, the last and the side, and carried off
    /// by perfusion: W for the steady state, J over a time course.
    pub conducted: [f64; 3],
    /// Perfused (W or J).
    pub perfused: f64,
    /// Stored at the end of a time course (J).
    pub stored: Option<f64>,
    /// The time course, with one probe per point asked for.
    pub course: Option<Course>,
    /// The probes' cells (radius, depth of their centres, mm).
    pub probes: Vec<(f64, f64)>,
    /// What was left out.
    pub notes: Vec<String>,
}

fn boundary(b: HeatBoundary) -> ThermalBoundary {
    match b {
        HeatBoundary::Fixed => ThermalBoundary::Fixed,
        HeatBoundary::Insulated => ThermalBoundary::Insulated,
        HeatBoundary::Transfer(h) => ThermalBoundary::Transfer(h),
    }
}

/// `edges` with the interfaces added, dropping edges closer than a quarter step to one.
fn with_interfaces(edges: Vec<f64>, interfaces: &[f64]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::with_capacity(edges.len() + interfaces.len());
    for (n, &e) in edges.iter().enumerate() {
        let step = if n + 1 < edges.len() {
            edges[n + 1] - e
        } else {
            e - edges[n - 1]
        };
        let near = interfaces
            .iter()
            .any(|&f| (f - e).abs() < 0.25 * step && (f - e).abs() > 1e-12);
        if !near || n == 0 || n + 1 == edges.len() {
            out.push(e);
        }
    }
    out.extend_from_slice(interfaces);
    out.sort_by(f64::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    out
}

/// The share of a ring of radius `r` about a point at distance `rho` from the centre of a
/// disk of radius `big` that lies inside the disk.
fn inside_share(rho: f64, r: f64, big: f64) -> f64 {
    if r <= 1e-15 {
        return if rho <= big { 1.0 } else { 0.0 };
    }
    if rho + r <= big {
        return 1.0;
    }
    // The ring's points lie from |ρ − r| to ρ + r from the disk's centre.
    if (rho - r).abs() >= big {
        return 0.0;
    }
    let c = ((rho * rho + r * r - big * big) / (2.0 * rho * r)).clamp(-1.0, 1.0);
    c.acos() / core::f64::consts::PI
}

/// The density `q` on the light grid spread over a disk of radius `big` and weighted by
/// `duty`, each slice's power kept (times `duty`).
fn spread(grid: &RzGrid, q: &[f64], big: f64, duty: f64) -> Vec<f64> {
    let mut out = vec![0.0; q.len()];
    let area = core::f64::consts::PI * big * big;
    for j in 0..grid.slices {
        let powers: Vec<f64> = (0..grid.rings)
            .map(|i| q[grid.index(i, j)] * grid.volume(i))
            .collect();
        let total: f64 = powers.iter().sum();
        if total <= 0.0 {
            continue;
        }
        let mut spread_powers = vec![0.0; grid.rings];
        for (target, slot) in spread_powers.iter_mut().enumerate() {
            let rho = grid.r(target);
            let density: f64 = powers
                .iter()
                .enumerate()
                .map(|(i, p)| p * inside_share(rho, grid.r(i), big))
                .sum::<f64>()
                / (area * grid.dz());
            *slot = density * grid.volume(target);
        }
        let kept: f64 = spread_powers.iter().sum();
        let scale = if kept > 0.0 { total * duty / kept } else { 0.0 };
        for (i, p) in spread_powers.iter().enumerate() {
            out[grid.index(i, j)] = p * scale / grid.volume(i);
        }
    }
    out
}

/// The light in a sample for [`heating_with`]: from the scene as traced (detectors left
/// out), its system and its trace, for sample `sample`.
pub type LightSource<'a> = dyn Fn(
        &Scene,
        &optobench_scene::Built,
        &optobench_scene::SceneTrace,
        usize,
    ) -> Result<SampleLight, String>
    + 'a;

/// The heating of sample `sample` (index into the scene's elements), its light by the
/// diffusion approximation (M11a2).
pub fn heating(
    scene: &Scene,
    sample: usize,
    settings: &HeatingSettings,
) -> Result<Heating, String> {
    let light = |s: &Scene,
                 _: &optobench_scene::Built,
                 t: &optobench_scene::SceneTrace,
                 k: usize| sample_light(s, t, k, &settings.light);
    heating_with(scene, sample, settings, &light)
}

/// [`heating`] with the light from `light` (the Monte Carlo of M15, run where a GPU is).
pub fn heating_with(
    scene: &Scene,
    sample: usize,
    settings: &HeatingSettings,
    light: &LightSource,
) -> Result<Heating, String> {
    let element = scene.elements.get(sample).ok_or("no such element")?;
    let ElementKind::Sample {
        layers, diameter, ..
    } = &element.kind
    else {
        return Err(format!("`{}` is not a sample", element.id));
    };
    let thermal = scene
        .thermal
        .iter()
        .find(|t| t.sample == element.id)
        .ok_or_else(|| {
            format!(
                "`{}` has no thermal properties: give each layer `thermal = {{ … }}`",
                element.id
            )
        })?;
    let properties: Vec<&optobench_scene::ThermalProperties> = thermal
        .layers
        .iter()
        .enumerate()
        .map(|(k, p)| {
            p.as_ref()
                .ok_or_else(|| format!("`{}` layer {} has no `thermal`", element.id, k + 1))
        })
        .collect::<Result<_, _>>()?;
    let mut notes = vec![format!(
        "the heat source is the light absorbed on the light grid (z {} to {} mm)",
        0.0, 0.0
    )];

    // Trace without detectors (none inside the sample may stop the light).
    let mut alone = scene.clone();
    alone.detectors.clear();
    let built = alone.build().map_err(|e| e.to_string())?;
    let trace = alone.trace_built(&built);
    let light = light(&alone, &built, &trace, sample)?;
    let lg = light.grid;
    notes[0] = format!(
        "the heat source is the light absorbed on the light grid (depth {:.4} to {:.4} mm, radius {:.4} mm, {}); absorption elsewhere is not counted",
        lg.z.0,
        lg.z.1,
        lg.radius,
        match &light.monte_carlo {
            Some(m) => format!(
                "Monte Carlo, {} packets{}",
                m.packets,
                m.gpu.as_ref().map_or(String::new(), |g| format!(" on {g}"))
            ),
            None => "diffusion approximation".to_owned(),
        }
    );
    let wavelength = light.wavelength;

    // Power entering the first face, and crossing the focal plane.
    let faces = &built.system.faces;
    let frame = element.frame;
    let mut surface_power = 0.0;
    for record in &trace.records {
        if let Some(k) = record
            .faces
            .iter()
            .position(|&f| built.owners[faces[f].owner] == Owner::Element(sample))
        {
            let local = frame.to_local_point(record.points[k + 1]);
            if local.z.abs() < 1e-9 {
                surface_power += record.powers[k];
            }
        }
    }
    let focal_depth = match settings.focal_depth {
        Some(d) => d,
        None => {
            // Where the direct light peaks on the axis; a Monte Carlo run for heating
            // records no fluence, and then where the absorption does.
            let along: &[f64] = if light.direct.iter().any(|v| *v > 0.0) {
                &light.direct
            } else {
                &light.absorbed
            };
            (0..lg.slices)
                .max_by(|&a, &b| along[lg.index(0, a)].total_cmp(&along[lg.index(0, b)]))
                .map_or(0.5 * (lg.z.0 + lg.z.1), |j| lg.z_at(j))
        }
    };
    let mut focal_power = 0.0;
    for record in &trace.records {
        for k in 0..record.points.len().saturating_sub(1) {
            let (a, b) = (
                frame.to_local_point(record.points[k]),
                frame.to_local_point(record.points[k + 1]),
            );
            if !(a.z < focal_depth && b.z >= focal_depth) {
                continue;
            }
            let medium = record.media.get(k).copied().unwrap_or(NO_MEDIUM);
            let mu_t = if medium == NO_MEDIUM {
                0.0
            } else {
                let m = built.system.media[medium].material();
                m.absorption_coefficient(wavelength).unwrap_or(0.0)
                    + m.scattering_coefficient(wavelength).unwrap_or(0.0)
            };
            let length = (b - a).length();
            let to_end = length * (b.z - focal_depth) / (b.z - a.z);
            focal_power += record.powers[k] * (mu_t * to_end).exp();
        }
    }

    // The source on the light grid, spread over a scanned field.
    let field_radius = settings
        .field
        .map(|f| f.side / core::f64::consts::PI.sqrt());
    let q_light = match settings.field {
        Some(f) => {
            if !(f.side > 0.0 && (0.0..=1.0).contains(&f.duty)) {
                return Err("a field needs a positive side and a duty cycle from 0 to 1".into());
            }
            spread(
                &lg,
                &light.absorbed,
                f.side / core::f64::consts::PI.sqrt(),
                f.duty,
            )
        }
        None => light.absorbed.clone(),
    };

    // The thermal grid.
    let thickness: f64 = layers.iter().map(|l| l.0).sum();
    let radius = diameter / 2.0;
    let fine_r = field_radius.unwrap_or(0.0).max(5.0 * lg.dr()) * 1.2;
    let mut interfaces = Vec::new();
    let mut depth = 0.0;
    for (t, _) in layers.iter().take(layers.len().saturating_sub(1)) {
        depth += t;
        interfaces.push(depth);
    }
    let grid = ThermalGrid {
        r: graded(0.0, radius, 0.0, lg.dr(), fine_r, settings.growth),
        z: with_interfaces(
            graded(
                0.0,
                thickness,
                focal_depth,
                lg.dz(),
                5.0 * lg.dz(),
                settings.growth,
            ),
            &interfaces,
        ),
    };
    let layer_of = |z: f64| -> usize {
        let mut d = 0.0;
        for (k, (t, _)) in layers.iter().enumerate() {
            d += t;
            if z < d {
                return k;
            }
        }
        layers.len() - 1
    };

    // Move the source onto it by shared volumes.
    let pi = core::f64::consts::PI;
    let overlap = |a0: f64, a1: f64, b0: f64, b1: f64| (a1.min(b1) - a0.max(b0)).max(0.0);
    let ring_shares: Vec<Vec<(usize, f64)>> = (0..grid.rings())
        .map(|i| {
            (0..lg.rings)
                .filter_map(|l| {
                    let (b0, b1) = (l as f64 * lg.dr(), (l + 1) as f64 * lg.dr());
                    let (lo, hi) = (grid.r[i].max(b0), grid.r[i + 1].min(b1));
                    (hi > lo).then_some((l, pi * (hi * hi - lo * lo)))
                })
                .collect()
        })
        .collect();
    let slice_shares: Vec<Vec<(usize, f64)>> = (0..grid.slices())
        .map(|j| {
            (0..lg.slices)
                .filter_map(|m| {
                    let (b0, b1) = (
                        lg.z.0 + m as f64 * lg.dz(),
                        lg.z.0 + (m + 1) as f64 * lg.dz(),
                    );
                    let o = overlap(grid.z[j], grid.z[j + 1], b0, b1);
                    (o > 0.0).then_some((m, o))
                })
                .collect()
        })
        .collect();
    let n = grid.cells();
    let mut source = vec![0.0; n];
    let mut absorbed_by_layer = vec![0.0; layers.len()];
    for (j, slices) in slice_shares.iter().enumerate() {
        for (i, rings) in ring_shares.iter().enumerate() {
            let mut power = 0.0;
            for &(m, length) in slices {
                for &(l, area) in rings {
                    power += q_light[lg.index(l, m)] * area * length;
                }
            }
            let k = grid.index(i, j);
            source[k] = power / grid.volume(i, j);
            absorbed_by_layer[layer_of(grid.z_at(j))] += power;
        }
    }
    let mut problem = Thermal::uniform(
        grid.clone(),
        1.0,
        1.0,
        0.0,
        [
            boundary(thermal.top),
            boundary(thermal.bottom),
            boundary(thermal.side),
        ],
    );
    for j in 0..grid.slices() {
        let p = properties[layer_of(grid.z_at(j))];
        for i in 0..grid.rings() {
            let k = grid.index(i, j);
            problem.conductivity[k] = p.conductivity;
            problem.capacity[k] = p.capacity;
            problem.perfusion[k] = p.perfusion;
        }
    }
    let probes = if settings.probes.is_empty() {
        vec![(0.0, focal_depth.clamp(0.0, thickness)), (0.0, 0.0)]
    } else {
        settings.probes.clone()
    };
    let probe_cells: Vec<(usize, (f64, f64))> = probes
        .iter()
        .map(|&(r, z)| {
            grid.cell(r, z)
                .map(|(i, j)| (grid.index(i, j), (grid.r_at(i), grid.z_at(j))))
                .ok_or_else(|| format!("probe at r {r} mm, depth {z} mm is outside the sample"))
        })
        .collect::<Result<_, _>>()?;
    let (rise, conducted, perfused, stored, course) = match settings.mode {
        HeatingMode::Steady => {
            let heat = problem.steady(&source)?;
            (heat.rise, heat.conducted, heat.perfused, None, None)
        }
        HeatingMode::Time {
            duration,
            step,
            off_at,
        } => {
            let off = off_at.unwrap_or(f64::INFINITY);
            let cells: Vec<usize> = probe_cells.iter().map(|p| p.0).collect();
            let course = problem.transient(
                &source,
                &|t| if t < off { 1.0 } else { 0.0 },
                duration,
                step,
                &cells,
            )?;
            (
                course.rise.clone(),
                course.conducted,
                course.perfused,
                Some(course.stored),
                Some(course),
            )
        }
    };
    let (mut peak, mut at) = (f64::NEG_INFINITY, (0.0, 0.0));
    for j in 0..grid.slices() {
        for i in 0..grid.rings() {
            let v = rise[grid.index(i, j)];
            if v > peak {
                peak = v;
                at = (grid.r_at(i), grid.z_at(j));
            }
        }
    }
    let (_, jf) = grid
        .cell(0.0, focal_depth.clamp(0.0, thickness))
        .ok_or("the focal depth is outside the sample")?;
    let at_focus = rise[grid.index(0, jf)];
    let field_average = field_radius.map(|big| {
        let (mut sum, mut area) = (0.0, 0.0);
        for i in 0..grid.rings() {
            let (a, b) = (grid.r[i], grid.r[i + 1].min(big));
            if b <= a {
                break;
            }
            let share = pi * (b * b - a * a);
            sum += share * rise[grid.index(i, jf)];
            area += share;
        }
        sum / area
    });
    if settings.field.is_some() {
        let alpha = properties
            .iter()
            .map(|p| p.conductivity / p.capacity)
            .fold(f64::INFINITY, f64::min);
        let side = settings.field.map_or(0.0, |f| f.side);
        notes.push(format!(
            "a scanned field is taken as its time average: right while a frame is much shorter than {:.3} s (side² / (4α), α the layers' least diffusivity)",
            side * side / (4.0 * alpha)
        ));
    }
    Ok(Heating {
        sample: element.id.clone(),
        light,
        grid,
        surface_power,
        focal_depth,
        focal_power,
        absorbed: absorbed_by_layer.iter().sum(),
        absorbed_by_layer: absorbed_by_layer
            .into_iter()
            .enumerate()
            .map(|(k, p)| (format!("layer {}", k + 1), p))
            .collect(),
        source,
        rise,
        peak: (peak, at.0, at.1),
        at_focus,
        field_average,
        conducted,
        perfused,
        stored,
        course,
        probes: probe_cells.iter().map(|p| p.1).collect(),
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_share_inside_a_disk() {
        // A point at the centre: rings inside count whole, outside not at all.
        assert_eq!(inside_share(0.0, 0.5, 1.0), 1.0);
        assert_eq!(inside_share(0.0, 1.5, 1.0), 0.0);
        // On the disk's edge, a small ring is half inside.
        assert!((inside_share(1.0, 1e-3, 1.0) - 0.5).abs() < 1e-3);
        // Far outside, nothing; a ring around the disk's centre point entirely enclosing it.
        assert_eq!(inside_share(3.0, 0.5, 1.0), 0.0);
        assert_eq!(inside_share(0.2, 2.0, 1.0), 0.0);
    }

    #[test]
    fn spreading_keeps_each_slices_power_times_the_duty() {
        let grid = RzGrid {
            radius: 2.0,
            rings: 40,
            z: (0.0, 1.0),
            slices: 2,
        };
        let mut q = vec![0.0; grid.cells()];
        q[grid.index(0, 0)] = 1.0;
        q[grid.index(1, 1)] = 2.0;
        let out = spread(&grid, &q, 0.5, 0.7);
        for j in 0..2 {
            let before: f64 = (0..grid.rings)
                .map(|i| q[grid.index(i, j)] * grid.volume(i))
                .sum();
            let after: f64 = (0..grid.rings)
                .map(|i| out[grid.index(i, j)] * grid.volume(i))
                .sum();
            assert!((after - 0.7 * before).abs() < 1e-12 * before);
        }
        // Even inside the disk, nothing far outside it.
        let inner = out[grid.index(2, 0)];
        assert!((out[grid.index(5, 0)] / inner - 1.0).abs() < 1e-9);
        assert_eq!(out[grid.index(20, 0)], 0.0);
    }
}
