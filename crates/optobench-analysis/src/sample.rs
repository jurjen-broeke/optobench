//! Light in a sample (M11a2): the direct beam from the traced rays and the diffuse light
//! it feeds, on a grid of rings about the sample's axis.
//!
//! - The direct (ballistic) fluence rate per cell is the power the rays carry through the
//!   cell times the length they travel in it, over its volume (the track-length
//!   estimator), from each ray segment walked in short pieces with Beer–Lambert decay.
//! - The power scattered out of the direct beam along each piece is the diffuse light's
//!   source there ([`crate::diffusion`]); the diffuse light then spreads with D =
//!   1/(3 (µa + µs (1 − g))). Taking the first scattering as isotropic (the source where
//!   the light left the beam, the anisotropy only in D) is the usual diffusion
//!   approximation; it is poor within a few transport lengths of the beam's entry and of
//!   boundaries, where M15's Monte Carlo transport takes over.
//! - The absorbed power density is µa (Φ_direct + Φ_diffuse) (W/mm³): the heat source of
//!   M11d.
//!
//! The diffusion domain is the run of turbid layers (those that scatter), which must touch
//! one another; its top and bottom faces lose light to the clear media beyond them by the
//! partial-current condition of their index step. Light reaching the grid's edge is lost
//! (the side is taken as dark); rays outside the grid's radius are not counted.

use optobench_scene::{ElementKind, Scene, SceneTrace};

use crate::diffusion::{Boundary, DiffuseLight, Diffusion, RzGrid};

/// How finely to grid a sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightSettings {
    /// The grid's radius (mm); by default half the sample's diameter.
    pub radius: Option<f64>,
    /// Rings across the radius.
    pub rings: usize,
    /// Slices through the turbid layers.
    pub slices: usize,
}

impl Default for LightSettings {
    fn default() -> Self {
        Self {
            radius: None,
            rings: 100,
            slices: 200,
        }
    }
}

/// What one part of the sample absorbs on the grid (W).
#[derive(Debug, Clone, PartialEq)]
pub struct PartLight {
    /// `layer 1`, `layer 2`, …
    pub part: String,
    /// From the direct beam.
    pub direct: f64,
    /// From the diffuse light.
    pub diffuse: f64,
}

/// The light in a sample.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleLight {
    /// The sample's id.
    pub id: String,
    /// The grid, z measured from the sample's first face along its axis.
    pub grid: RzGrid,
    /// Vacuum wavelength (µm).
    pub wavelength: f64,
    /// Direct fluence rate per cell (W/mm²).
    pub direct: Vec<f64>,
    /// Diffuse fluence rate per cell (W/mm²).
    pub diffuse: Vec<f64>,
    /// Absorbed power density per cell (W/mm³).
    pub absorbed: Vec<f64>,
    /// Power scattered out of the direct beam on the grid (W): the diffuse light's source.
    pub scattered: f64,
    /// Absorbed from the direct beam on the grid (W).
    pub direct_absorbed: f64,
    /// The diffuse light's outcome (absorbed, out through top, bottom and side).
    pub diffuse_light: Option<DiffuseLight>,
    /// Per turbid layer.
    pub parts: Vec<PartLight>,
}

/// The light in sample `sample` (index into the scene's elements) of a traced scene.
pub fn sample_light(
    scene: &Scene,
    trace: &SceneTrace,
    sample: usize,
    settings: &LightSettings,
) -> Result<SampleLight, String> {
    let element = scene.elements.get(sample).ok_or("no such element")?;
    let ElementKind::Sample {
        layers,
        above,
        below,
        diameter,
    } = &element.kind
    else {
        return Err(format!("`{}` is not a sample", element.id));
    };
    // One wavelength: that of the rays that reach the sample.
    let mut wavelength: Option<f64> = None;
    for r in &trace.records {
        let w = r.launched.wavelength;
        match wavelength {
            None => wavelength = Some(w),
            Some(v) if (v - w).abs() > 1e-12 => {
                return Err(
                    "the light in a sample is for one wavelength; the scene has several".into(),
                );
            }
            Some(_) => {}
        }
    }
    let wavelength = wavelength.ok_or("no rays were traced")?;
    let surrounding = scene
        .surrounding
        .material()
        .refractive_index(wavelength)
        .map_err(|e| e.to_string())?;
    // Per layer: z range, n, µa, µs, g.
    let mut z = 0.0;
    let mut coefficients = Vec::with_capacity(layers.len());
    for (thickness, medium) in layers {
        let m = medium.material();
        let fail = |e: optobench_materials::MaterialError| e.to_string();
        coefficients.push((
            (z, z + thickness),
            m.refractive_index(wavelength).map_err(fail)?,
            m.absorption_coefficient(wavelength).map_err(fail)?,
            m.scattering_coefficient(wavelength).map_err(fail)?,
            m.anisotropy(wavelength).map_err(fail)?,
        ));
        z += thickness;
    }
    let turbid: Vec<usize> = (0..coefficients.len())
        .filter(|&k| coefficients[k].3 > 0.0)
        .collect();
    let (first, last) = match (turbid.first(), turbid.last()) {
        (Some(&a), Some(&b)) => (a, b),
        _ => (0, coefficients.len() - 1),
    };
    if let Some(gap) = (first..=last).find(|k| coefficients[*k].3 <= 0.0) {
        return Err(format!(
            "`{}`: the diffusion approximation needs the turbid layers to touch; layer {} between them is clear",
            element.id,
            gap + 1
        ));
    }
    let index_of = |m: &Option<optobench_materials::Medium>| -> Result<f64, String> {
        match m {
            Some(m) => m
                .material()
                .refractive_index(wavelength)
                .map_err(|e| e.to_string()),
            None => Ok(surrounding),
        }
    };
    let outside_top = if first == 0 {
        index_of(above)?
    } else {
        coefficients[first - 1].1
    };
    let outside_bottom = if last + 1 == coefficients.len() {
        index_of(below)?
    } else {
        coefficients[last + 1].1
    };
    let grid = RzGrid {
        radius: settings.radius.unwrap_or(diameter / 2.0),
        rings: settings.rings.max(4),
        z: (coefficients[first].0.0, coefficients[last].0.1),
        slices: settings.slices.max(4),
    };
    let layer_at = |zz: f64| -> Option<usize> {
        coefficients
            .iter()
            .position(|c| zz >= c.0.0 && zz < c.0.1)
            .or_else(|| (zz >= z && zz <= z + 1e-12).then(|| coefficients.len() - 1))
    };

    // Walk every ray segment in the sample.
    let frame = element.frame;
    let mut track = vec![0.0; grid.cells()]; // W·mm
    let mut source = vec![0.0; grid.cells()]; // W
    let mut direct_absorbed = 0.0;
    let piece = 0.5 * grid.dr().min(grid.dz());
    for record in &trace.records {
        for k in 0..record.points.len().saturating_sub(1) {
            let (a, b) = (
                frame.to_local_point(record.points[k]),
                frame.to_local_point(record.points[k + 1]),
            );
            let length = (b - a).length();
            if length <= 0.0 {
                continue;
            }
            let Some(layer) = layer_at(0.5 * (a.z + b.z)) else {
                continue;
            };
            let (_, _, mu_a, mu_s, _) = coefficients[layer];
            let mu_t = mu_a + mu_s;
            let arrival = record.powers.get(k).copied().unwrap_or(record.ray.power);
            let start = arrival * (mu_t * length).exp();
            // The part of the segment inside the grid's z range.
            let (dz_seg, z_a) = (b.z - a.z, a.z);
            let (mut t0, mut t1) = (0.0, 1.0);
            if dz_seg.abs() > 1e-15 {
                let (u, v) = ((grid.z.0 - z_a) / dz_seg, (grid.z.1 - z_a) / dz_seg);
                t0 = f64::max(t0, u.min(v));
                t1 = f64::min(t1, u.max(v));
            } else if !(grid.z.0..=grid.z.1).contains(&z_a) {
                continue;
            }
            if t1 <= t0 {
                continue;
            }
            let span = (t1 - t0) * length;
            let pieces = (span / piece).ceil().max(1.0) as usize;
            let step = span / pieces as f64;
            for m in 0..pieces {
                let s = t0 * length + m as f64 * step;
                let p = start * (-mu_t * s).exp();
                let mid = a + (b - a) * ((s + step / 2.0) / length);
                let Some((i, j)) = grid.cell(mid.x.hypot(mid.y), mid.z) else {
                    continue;
                };
                let cell = grid.index(i, j);
                let (carried, lost) = if mu_t > 0.0 {
                    let lost = p * (1.0 - (-mu_t * step).exp());
                    (lost / mu_t, lost)
                } else {
                    (p * step, 0.0)
                };
                track[cell] += carried;
                if mu_t > 0.0 {
                    source[cell] += lost * mu_s / mu_t;
                    direct_absorbed += lost * mu_a / mu_t;
                }
            }
        }
    }
    let direct: Vec<f64> = (0..grid.cells())
        .map(|c| track[c] / grid.volume(c % grid.rings))
        .collect();
    let scattered: f64 = source.iter().sum();
    let per_slice: Vec<(f64, f64, f64)> = (0..grid.slices)
        .map(|j| {
            let layer = layer_at(grid.z_at(j)).unwrap_or(first);
            let (_, _, mu_a, mu_s, g) = coefficients[layer];
            (mu_a, mu_s, g)
        })
        .collect();
    let diffuse_light = if turbid.is_empty() {
        None
    } else {
        let problem = Diffusion::layered(
            grid,
            &per_slice
                .iter()
                .map(|&(mu_a, mu_s, g)| (mu_a, 1.0 / (3.0 * (mu_a + mu_s * (1.0 - g)))))
                .collect::<Vec<_>>(),
            Boundary::between(coefficients[first].1, outside_top),
            Boundary::between(coefficients[last].1, outside_bottom),
        );
        Some(problem.solve(&source)?)
    };
    let diffuse = diffuse_light
        .as_ref()
        .map_or_else(|| vec![0.0; grid.cells()], |d| d.fluence.clone());
    let absorbed: Vec<f64> = (0..grid.cells())
        .map(|c| per_slice[c / grid.rings].0 * (direct[c] + diffuse[c]))
        .collect();
    let parts = (first..=last)
        .map(|layer| {
            let (mut d, mut f) = (0.0, 0.0);
            for (j, &(mu_a, _, _)) in per_slice.iter().enumerate() {
                if layer_at(grid.z_at(j)) != Some(layer) {
                    continue;
                }
                for i in 0..grid.rings {
                    let c = grid.index(i, j);
                    let v = grid.volume(i) * mu_a;
                    d += v * direct[c];
                    f += v * diffuse[c];
                }
            }
            PartLight {
                part: format!("layer {}", layer + 1),
                direct: d,
                diffuse: f,
            }
        })
        .collect();
    Ok(SampleLight {
        id: element.id.clone(),
        grid,
        wavelength,
        direct,
        diffuse,
        absorbed,
        scattered,
        direct_absorbed,
        diffuse_light,
        parts,
    })
}
