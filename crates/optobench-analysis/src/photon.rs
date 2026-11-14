//! Photon transport in scattering tissue by Monte Carlo (M15a): the weighted photon random
//! walk of S. A. Prahl, M. Keijzer, S. L. Jacques and A. J. Welch (SPIE IS 5, 102 (1989))
//! and L. Wang, S. L. Jacques and L. Zheng ("MCML", Comput. Methods Programs Biomed. 47,
//! 131 (1995)), as S. L. Jacques and L. Wang write it out in ch. 5 of A. J. Welch and
//! M. J. C. van Gemert, Optical-Thermal Response of Laser-Irradiated Tissue, 2nd ed., 2011.
//! Written from those descriptions; no code is adapted.
//!
//! A sample is a stack of flat layers (each with µa, µs, g and an index) between an
//! ambient medium above and one below, unbounded sideways. A packet of weight w (W)
//! travels a step s = −ln ξ / µt between interactions (ξ uniform on (0, 1]); at each
//! interaction it leaves w µa / µt where it is (absorbed) and scatters by the
//! Henyey–Greenstein phase function, cos θ sampled in closed form (ch. 5, Eq. (5.45)) and
//! the azimuth uniform. At an index step it is reflected with the unpolarised Fresnel
//! reflectance and otherwise refracted (Snell), chosen by chance, so its weight is
//! kept; through the first face upwards it escapes as reflectance, through the last as
//! transmittance. Below a weight threshold it plays Russian roulette: it survives one time
//! in m with its weight times m (ch. 5, Eq. (5.49)), which keeps the expected weight.
//! The step left over at a boundary carries on in the next layer as optical depth
//! (−ln ξ shared across layers).
//!
//! What it leaves is recorded on the r–z grid of [`crate::diffusion::RzGrid`]: the power
//! absorbed per cell, and the track length weighted by w per cell, whose sum over a cell
//! divided by its volume is the fluence rate there (the track-length estimator, valid where
//! µa is 0 too). Absorption beyond the grid is counted apart. The balance (absorbed +
//! reflected + transmitted + roulette's net effect = launched) closes exactly; roulette's
//! part is zero on average.
//!
//! Packets are launched in batches, each from its own random stream, so the result is the
//! same however batches are shared among threads.

use optobench_math::Vec3;
use optobench_math::random::Random;
use optobench_trace::fresnel_power;

use crate::diffusion::RzGrid;

/// One layer of the stack, z from the first face downwards (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TissueLayer {
    /// Its top (mm).
    pub top: f64,
    /// Its bottom (mm).
    pub bottom: f64,
    /// Refractive index.
    pub index: f64,
    /// µa (1/mm).
    pub absorption: f64,
    /// µs (1/mm).
    pub scattering: f64,
    /// Anisotropy g.
    pub anisotropy: f64,
}

/// The stack and its surroundings.
#[derive(Debug, Clone, PartialEq)]
pub struct Tissue {
    /// Layers, touching, from the first face down.
    pub layers: Vec<TissueLayer>,
    /// Index of the medium above the first face.
    pub above: f64,
    /// Index of the medium below the last face.
    pub below: f64,
}

/// A packet to launch: inside the first layer at the first face (z = 0), or anywhere
/// inside the stack, with its direction (unit) and power (W).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Launch {
    /// Position (mm; z down from the first face).
    pub position: Vec3,
    /// Direction (unit).
    pub direction: Vec3,
    /// Power (W).
    pub power: f64,
}

/// How to run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WalkSettings {
    /// Packets per launch.
    pub packets: usize,
    /// Below this fraction of its launch weight a packet plays Russian roulette.
    pub threshold: f64,
    /// It survives roulette one time in this many.
    pub chance: u32,
    /// The run's seed.
    pub seed: u64,
    /// Record the weighted track length per cell (the fluence); off, only absorption is
    /// recorded, which is all heating needs and much faster on fine grids.
    pub track: bool,
}

impl Default for WalkSettings {
    fn default() -> Self {
        Self {
            packets: 1000,
            threshold: 1e-4,
            chance: 10,
            seed: 1,
            track: true,
        }
    }
}

/// What the packets did (W unless said).
#[derive(Debug, Clone, PartialEq)]
pub struct Transport {
    /// The grid.
    pub grid: RzGrid,
    /// Absorbed per cell.
    pub absorbed: Vec<f64>,
    /// Weight × track length per cell (W mm).
    pub track: Vec<f64>,
    /// Absorbed outside the grid.
    pub absorbed_outside: f64,
    /// Absorbed per layer.
    pub absorbed_by_layer: Vec<f64>,
    /// Out through the first face.
    pub reflected: f64,
    /// Out through the last face.
    pub transmitted: f64,
    /// Launched.
    pub launched: f64,
    /// Interactions, over all packets.
    pub interactions: u64,
    /// Russian roulette's net effect: the weight of packets it ended less the weight it
    /// added to the survivors. Zero on average (the game is fair); it closes the balance
    /// exactly.
    pub roulette: f64,
    /// Whether the track (fluence) was recorded.
    pub tracking: bool,
}

impl Transport {
    /// No packets yet.
    pub fn empty(grid: RzGrid, layers: usize) -> Self {
        Self {
            grid,
            absorbed: vec![0.0; grid.cells()],
            track: vec![0.0; grid.cells()],
            absorbed_outside: 0.0,
            absorbed_by_layer: vec![0.0; layers],
            reflected: 0.0,
            transmitted: 0.0,
            launched: 0.0,
            interactions: 0,
            roulette: 0.0,
            tracking: true,
        }
    }

    /// Adds another run's results (on the same grid).
    pub fn absorb(&mut self, other: &Transport) {
        for (a, b) in self.absorbed.iter_mut().zip(&other.absorbed) {
            *a += b;
        }
        for (a, b) in self.track.iter_mut().zip(&other.track) {
            *a += b;
        }
        for (a, b) in self
            .absorbed_by_layer
            .iter_mut()
            .zip(&other.absorbed_by_layer)
        {
            *a += b;
        }
        self.absorbed_outside += other.absorbed_outside;
        self.reflected += other.reflected;
        self.transmitted += other.transmitted;
        self.launched += other.launched;
        self.interactions += other.interactions;
        self.roulette += other.roulette;
    }

    /// The fluence rate per cell (W/mm²): weighted track length over the cell's volume.
    pub fn fluence(&self) -> Vec<f64> {
        let g = &self.grid;
        (0..g.cells())
            .map(|c| self.track[c] / g.volume(c % g.rings))
            .collect()
    }

    /// Launched less everything accounted for, roulette's net effect included (W): zero
    /// up to rounding.
    pub fn imbalance(&self) -> f64 {
        self.launched
            - self.absorbed_by_layer.iter().sum::<f64>()
            - self.reflected
            - self.transmitted
            - self.roulette
    }
}

/// cos θ of a Henyey–Greenstein scattering with anisotropy `g`, from ξ uniform on [0, 1)
/// (ch. 5, Eq. (5.45)); isotropic for g = 0.
pub fn henyey_greenstein(g: f64, xi: f64) -> f64 {
    if g.abs() < 1e-9 {
        2.0 * xi - 1.0
    } else {
        let f = (1.0 - g * g) / (1.0 - g + 2.0 * g * xi);
        ((1.0 + g * g - f * f) / (2.0 * g)).clamp(-1.0, 1.0)
    }
}

/// `u` turned by polar angle θ (cos θ given) and azimuth φ about itself.
fn turn(u: Vec3, cos_theta: f64, phi: f64) -> Vec3 {
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    let (sin_phi, cos_phi) = phi.sin_cos();
    let next = if u.z.abs() > 0.99999 {
        Vec3::new(
            sin_theta * cos_phi,
            sin_theta * sin_phi,
            cos_theta * u.z.signum(),
        )
    } else {
        let t = (1.0 - u.z * u.z).sqrt();
        Vec3::new(
            sin_theta * (u.x * u.z * cos_phi - u.y * sin_phi) / t + u.x * cos_theta,
            sin_theta * (u.y * u.z * cos_phi + u.x * sin_phi) / t + u.y * cos_theta,
            -sin_theta * cos_phi * t + u.z * cos_theta,
        )
    };
    next.normalized().unwrap_or(u)
}

/// Adds `weight × length` of the segment from `a` to `b` to the track per cell, in
/// pieces no longer than half a cell.
fn track(out: &mut Transport, a: Vec3, b: Vec3, weight: f64) {
    if !out.tracking {
        return;
    }
    let g = out.grid;
    let length = (b - a).length();
    if length <= 0.0 {
        return;
    }
    let piece = 0.5 * g.dr().min(g.dz());
    let n = (length / piece).ceil().max(1.0) as usize;
    let step = length / n as f64;
    for k in 0..n {
        let mid = a + (b - a) * ((k as f64 + 0.5) / n as f64);
        if let Some((i, j)) = g.cell(mid.x.hypot(mid.y), mid.z) {
            out.track[g.index(i, j)] += weight * step;
        }
    }
}

/// Walks `settings.packets` packets for each launch through `tissue`, recording on `grid`;
/// `batch` picks the random stream (one per batch, so batches can run anywhere).
pub fn walk(
    tissue: &Tissue,
    grid: &RzGrid,
    launches: &[Launch],
    settings: &WalkSettings,
    batch: u64,
) -> Transport {
    let mut out = Transport::empty(*grid, tissue.layers.len());
    out.tracking = settings.track;
    let mut random = Random::stream(settings.seed, batch);
    let layers = &tissue.layers;
    if layers.is_empty() || settings.packets == 0 {
        return out;
    }
    let layer_at = |z: f64| -> usize {
        layers
            .iter()
            .position(|l| z >= l.top && z < l.bottom)
            .unwrap_or(if z < layers[0].top {
                0
            } else {
                layers.len() - 1
            })
    };
    for launch in launches {
        let w0 = launch.power / settings.packets as f64;
        for _ in 0..settings.packets {
            out.launched += w0;
            let mut p = launch.position;
            let mut u = launch.direction.normalized().unwrap_or(Vec3::Z);
            let mut w = w0;
            let mut layer = layer_at(p.z);
            // Optical depth to the next interaction.
            let mut depth = -(1.0 - random.uniform()).ln();
            loop {
                let l = layers[layer];
                let mu_t = l.absorption + l.scattering;
                let to_boundary = if u.z > 0.0 {
                    (l.bottom - p.z) / u.z
                } else if u.z < 0.0 {
                    (l.top - p.z) / u.z
                } else {
                    f64::INFINITY
                };
                let step = if mu_t > 0.0 {
                    depth / mu_t
                } else {
                    f64::INFINITY
                };
                if step < to_boundary {
                    // Hop, drop, spin.
                    let q = p + u * step;
                    track(&mut out, p, q, w);
                    p = q;
                    depth = -(1.0 - random.uniform()).ln();
                    out.interactions += 1;
                    let dropped = w * l.absorption / mu_t;
                    w -= dropped;
                    out.absorbed_by_layer[layer] += dropped;
                    match grid.cell(p.x.hypot(p.y), p.z) {
                        Some((i, j)) => out.absorbed[grid.index(i, j)] += dropped,
                        None => out.absorbed_outside += dropped,
                    }
                    if w <= 0.0 {
                        break; // all absorbed (no scattering)
                    }
                    let cos = henyey_greenstein(l.anisotropy, random.uniform());
                    let phi = 2.0 * core::f64::consts::PI * random.uniform();
                    u = turn(u, cos, phi);
                    if w < settings.threshold * w0 {
                        if random.uniform() * settings.chance as f64 <= 1.0 {
                            out.roulette -= w * (settings.chance as f64 - 1.0);
                            w *= settings.chance as f64;
                        } else {
                            // Lost here, made up on average by the survivors' extra.
                            out.roulette += w;
                            break;
                        }
                    }
                    continue;
                }
                // To the boundary.
                let q = p + u * to_boundary;
                track(&mut out, p, q, w);
                p = q;
                if mu_t > 0.0 {
                    depth -= to_boundary * mu_t;
                }
                let downwards = u.z > 0.0;
                let next = if downwards {
                    (layer + 1 < layers.len()).then_some(layer + 1)
                } else {
                    layer.checked_sub(1)
                };
                let n1 = l.index;
                let n2 = match next {
                    Some(k) => layers[k].index,
                    None if downwards => tissue.below,
                    None => tissue.above,
                };
                let cos_i = u.z.abs();
                let reflectance = if (n1 - n2).abs() < 1e-12 {
                    0.0
                } else {
                    fresnel_power(cos_i, n1, n2).unpolarised_reflectance()
                };
                if random.uniform() < reflectance {
                    u = Vec3::new(u.x, u.y, -u.z);
                    continue;
                }
                // Refracted.
                let ratio = n1 / n2;
                let sin2 = ratio * ratio * (1.0 - cos_i * cos_i);
                let cos_t = (1.0 - sin2).max(0.0).sqrt();
                u = Vec3::new(u.x * ratio, u.y * ratio, cos_t * u.z.signum())
                    .normalized()
                    .unwrap_or(u);
                match next {
                    Some(k) => layer = k,
                    None => {
                        if downwards {
                            out.transmitted += w;
                        } else {
                            out.reflected += w;
                        }
                        break;
                    }
                }
            }
        }
    }
    out
}

/// A collimated packet arriving from above at the first face: the part the first face
/// reflects (specular, Fresnel) and the launch inside, refracted.
pub fn entering(tissue: &Tissue, position: Vec3, direction: Vec3, power: f64) -> (f64, Launch) {
    let u = direction.normalized().unwrap_or(Vec3::Z);
    let n1 = tissue.above;
    let n2 = tissue.layers.first().map_or(n1, |l| l.index);
    let cos_i = u.z.abs();
    let reflectance = if (n1 - n2).abs() < 1e-12 {
        0.0
    } else {
        fresnel_power(cos_i, n1, n2).unpolarised_reflectance()
    };
    let ratio = n1 / n2;
    let sin2 = ratio * ratio * (1.0 - cos_i * cos_i);
    let inside = Vec3::new(u.x * ratio, u.y * ratio, (1.0 - sin2).max(0.0).sqrt())
        .normalized()
        .unwrap_or(Vec3::Z);
    (
        power * reflectance,
        Launch {
            position: Vec3::new(position.x, position.y, 0.0),
            direction: inside,
            power: power * (1.0 - reflectance),
        },
    )
}

/// The stack of sample `sample` (index into the scene's elements) at vacuum wavelength
/// `wavelength` (µm), with the indices above and below it.
pub fn sample_tissue(
    scene: &optobench_scene::Scene,
    sample: usize,
    wavelength: f64,
) -> Result<Tissue, String> {
    let element = scene.elements.get(sample).ok_or("no such element")?;
    let optobench_scene::ElementKind::Sample {
        layers,
        above,
        below,
        ..
    } = &element.kind
    else {
        return Err(format!("`{}` is not a sample", element.id));
    };
    let fail = |e: optobench_materials::MaterialError| e.to_string();
    let index = |m: &Option<optobench_materials::Medium>| -> Result<f64, String> {
        m.as_ref()
            .unwrap_or(&scene.surrounding)
            .material()
            .refractive_index(wavelength)
            .map_err(fail)
    };
    let mut z = 0.0;
    let mut out = Vec::with_capacity(layers.len());
    for (thickness, medium) in layers {
        let m = medium.material();
        out.push(TissueLayer {
            top: z,
            bottom: z + thickness,
            index: m.refractive_index(wavelength).map_err(fail)?,
            absorption: m.absorption_coefficient(wavelength).map_err(fail)?,
            scattering: m.scattering_coefficient(wavelength).map_err(fail)?,
            anisotropy: m.anisotropy(wavelength).map_err(fail)?,
        });
        z += thickness;
    }
    Ok(Tissue {
        layers: out,
        above: index(above)?,
        below: index(below)?,
    })
}

/// The packets the traced rays bring into sample `sample`: where each crosses the sample's
/// first face going in, its direction just inside (sample frame, z down into the sample)
/// and its power after the face (its Fresnel loss there taken off), with the wavelength of
/// the rays (one only).
pub fn sample_launches(
    scene: &optobench_scene::Scene,
    built: &optobench_scene::Built,
    trace: &optobench_scene::SceneTrace,
    sample: usize,
) -> Result<(Vec<Launch>, f64), String> {
    let element = scene.elements.get(sample).ok_or("no such element")?;
    let frame = element.frame;
    let faces = &built.system.faces;
    let mut wavelength: Option<f64> = None;
    let mut launches = Vec::new();
    for record in &trace.records {
        let w = record.launched.wavelength;
        match wavelength {
            None => wavelength = Some(w),
            Some(v) if (v - w).abs() > 1e-12 => {
                return Err(
                    "the light in a sample is for one wavelength; the scene has several".into(),
                );
            }
            Some(_) => {}
        }
        for (k, &f) in record.faces.iter().enumerate() {
            if built.owners[faces[f].owner] != optobench_scene::Owner::Element(sample) {
                continue;
            }
            let at = frame.to_local_point(record.points[k + 1]);
            if at.z.abs() > 1e-9 || record.points.len() < k + 3 {
                continue;
            }
            let next = frame.to_local_point(record.points[k + 2]);
            let Some(direction) = (next - at).normalized() else {
                continue;
            };
            if direction.z <= 0.0 {
                continue; // leaving, or reflected
            }
            let lost: f64 = record
                .face_losses
                .iter()
                .filter(|(face, _)| *face == f)
                .map(|(_, l)| l)
                .sum();
            let power = record.powers[k] - lost;
            if power > 0.0 {
                launches.push(Launch {
                    position: Vec3::new(at.x, at.y, 0.0),
                    direction,
                    power,
                });
            }
            break;
        }
    }
    Ok((launches, wavelength.ok_or("no rays were traced")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn henyey_greenstein_has_its_mean_cosine_and_distribution() {
        // ⟨cos θ⟩ = g; and P(cos θ ≤ c) = (1 − g²)/(2g) (1/(1 + g² − 2gc)^{1/2} − 1/(1 + g))
        // (the phase function integrated; ch. 5 Eqs. (5.43), (5.44)).
        let mut r = Random::new(9);
        for g in [0.0, 0.5, 0.9, -0.3] {
            let n = 200_000;
            let samples: Vec<f64> = (0..n).map(|_| henyey_greenstein(g, r.uniform())).collect();
            let mean = samples.iter().sum::<f64>() / n as f64;
            assert!((mean - g).abs() < 0.005, "g {g}: {mean}");
            let c = 0.5;
            let below = samples.iter().filter(|&&x| x <= c).count() as f64 / n as f64;
            let exact = if g == 0.0 {
                (c + 1.0) / 2.0
            } else {
                (1.0 - g * g) / (2.0 * g)
                    * (1.0 / (1.0 + g * g - 2.0 * g * c).sqrt() - 1.0 / (1.0 + g))
            };
            assert!((below - exact).abs() < 0.005, "g {g}: {below} {exact}");
        }
    }

    #[test]
    fn turning_keeps_the_direction_a_unit_vector_at_the_angle() {
        let mut r = Random::new(4);
        for _ in 0..1000 {
            let u = Vec3::new(r.normal(), r.normal(), r.normal())
                .normalized()
                .expect("unit");
            let cos = 2.0 * r.uniform() - 1.0;
            let v = turn(u, cos, 2.0 * core::f64::consts::PI * r.uniform());
            assert!((v.length() - 1.0).abs() < 1e-12);
            assert!((v.dot(u) - cos).abs() < 1e-9);
        }
    }

    fn pencil(tissue: &Tissue, grid: &RzGrid, packets: usize, seed: u64) -> (f64, Transport) {
        let (specular, launch) = entering(tissue, Vec3::ZERO, Vec3::Z, 1.0);
        let settings = WalkSettings {
            packets,
            seed,
            ..Default::default()
        };
        (specular, walk(tissue, grid, &[launch], &settings, 0))
    }

    #[test]
    fn an_absorbing_slab_between_index_steps() {
        // No scattering, n = 1.5 in air, µa d = 0.5: with r = ((n − 1)/(n + 1))² and
        // a = e^{−µa d}, R = r + (1 − r)² r a² / (1 − r² a²) and
        // T = (1 − r)² a / (1 − r² a²) (the reflections summed in power).
        let tissue = Tissue {
            layers: vec![TissueLayer {
                top: 0.0,
                bottom: 1.0,
                index: 1.5,
                absorption: 0.5,
                scattering: 0.0,
                anisotropy: 0.0,
            }],
            above: 1.0,
            below: 1.0,
        };
        let grid = RzGrid {
            radius: 1.0,
            rings: 2,
            z: (0.0, 1.0),
            slices: 4,
        };
        let n = 200_000;
        let (specular, t) = pencil(&tissue, &grid, n, 3);
        let r = (0.5f64 / 2.5).powi(2);
        let a = (-0.5f64).exp();
        let reflected = r + (1.0 - r).powi(2) * r * a * a / (1.0 - r * r * a * a);
        let transmitted = (1.0 - r).powi(2) * a / (1.0 - r * r * a * a);
        let sigma = |p: f64| (p * (1.0 - p) / n as f64).sqrt();
        assert!((specular - r).abs() < 1e-12);
        assert!(
            (specular + t.reflected - reflected).abs() < 4.0 * sigma(reflected),
            "{}",
            specular + t.reflected
        );
        assert!(
            (t.transmitted - transmitted).abs() < 4.0 * sigma(transmitted),
            "{}",
            t.transmitted
        );
        // Exact but for rounding over the packets' small weights.
        assert!(t.imbalance().abs() < 1e-10, "imbalance {:e}", t.imbalance());
    }

    /// Gauss–Legendre nodes and weights on [0, 1].
    fn gauss_legendre(n: usize) -> Vec<(f64, f64)> {
        (0..n)
            .map(|i| {
                let mut x = (core::f64::consts::PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
                let mut derivative = 0.0;
                for _ in 0..100 {
                    let (mut p0, mut p1) = (1.0, x);
                    for k in 2..=n {
                        let p2 = ((2 * k - 1) as f64 * x * p1 - (k - 1) as f64 * p0) / k as f64;
                        p0 = p1;
                        p1 = p2;
                    }
                    derivative = n as f64 * (x * p1 - p0) / (x * x - 1.0);
                    let dx = p1 / derivative;
                    x -= dx;
                    if dx.abs() < 1e-15 {
                        break;
                    }
                }
                let w = 2.0 / ((1.0 - x * x) * derivative * derivative);
                (0.5 * (x + 1.0), 0.5 * w)
            })
            .collect()
    }

    /// Chandrasekhar's H-function for isotropic scattering with albedo ω at µ, from
    /// 1/H(µ) = √(1 − ω) + (ω/2) ∫₀¹ µ′ H(µ′) / (µ + µ′) dµ′ (S. Chandrasekhar, Radiative
    /// Transfer, 1950, ch. V), iterated on Gauss–Legendre nodes.
    fn chandrasekhar_h(omega: f64, mu: f64) -> f64 {
        let nodes = gauss_legendre(64);
        let mut h = vec![1.0; nodes.len()];
        let at = |m: f64, h: &[f64]| {
            let integral: f64 = nodes
                .iter()
                .zip(h)
                .map(|(&(x, w), &hx)| w * x * hx / (m + x))
                .sum();
            1.0 / ((1.0 - omega).sqrt() + 0.5 * omega * integral)
        };
        for _ in 0..500 {
            let next: Vec<f64> = nodes.iter().map(|&(x, _)| at(x, &h)).collect();
            let change = next
                .iter()
                .zip(&h)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f64::max);
            h = next;
            if change < 1e-14 {
                break;
            }
        }
        at(mu, &h)
    }

    #[test]
    fn a_semi_infinite_isotropic_scatterer_reflects_by_chandrasekhar() {
        // Albedo 0.9, no index step: the plane albedo for normal incidence is
        // 1 − H(1) √(1 − ω) (Chandrasekhar, Radiative Transfer, ch. V). 60 mean free paths
        // deep stands for semi-infinite (the diffusion length is 1.8 of them).
        let omega = 0.9;
        let tissue = Tissue {
            layers: vec![TissueLayer {
                top: 0.0,
                bottom: 60.0,
                index: 1.0,
                absorption: 1.0 - omega,
                scattering: omega,
                anisotropy: 0.0,
            }],
            above: 1.0,
            below: 1.0,
        };
        let grid = RzGrid {
            radius: 20.0,
            rings: 4,
            z: (0.0, 60.0),
            slices: 6,
        };
        let n = 100_000;
        let (specular, t) = pencil(&tissue, &grid, n, 5);
        assert_eq!(specular, 0.0);
        let exact = 1.0 - chandrasekhar_h(omega, 1.0) * (1.0 - omega).sqrt();
        let sigma = (exact * (1.0 - exact) / n as f64).sqrt();
        assert!(
            (t.reflected - exact).abs() < 4.0 * sigma,
            "{} {exact}",
            t.reflected
        );
        assert!(t.transmitted < 1e-6);
        // The balance closes; roulette's net effect is small against what it moves.
        // Exact but for rounding over the packets' small weights.
        assert!(t.imbalance().abs() < 1e-10, "imbalance {:e}", t.imbalance());
        assert!(t.roulette.abs() < 0.01);
        // The solver itself: H's zeroth moment, (ω/2) ∫₀¹ H(µ) dµ = 1 − √(1 − ω)
        // (Chandrasekhar, ch. V).
        let moment: f64 = gauss_legendre(64)
            .iter()
            .map(|&(x, w)| w * chandrasekhar_h(omega, x))
            .sum::<f64>()
            * omega
            / 2.0;
        assert!(
            (moment - (1.0 - (1.0 - omega).sqrt())).abs() < 1e-9,
            "{moment}"
        );
    }
}
