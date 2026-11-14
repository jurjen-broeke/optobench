//! The optical scene: elements, sources, detectors, degrees of freedom, mechanical proxies and the sample element.
//!
//! Layer L3. See `docs/design.md`, sections Architecture and Standalone scenes.
//!
//! M3 provides standalone scenes: items placed by a [`layout`] (planar or table), built
//! into a non-sequential system ([`Scene::build`]), traced from their sources
//! ([`Scene::trace`]) with per-detector results and a clipping list. Degrees of freedom
//! arrive in M6, the sample element in M11.
//!
//! Every item has a frame whose local +z is the optical axis at that point; for a
//! mirror it is the mirror's normal.

#![forbid(unsafe_code)]

pub mod dof;
pub mod layout;
pub mod tolerance;

use std::sync::Arc;

use optobench_materials::{Medium, Spectrum};
use optobench_math::{Frame, Vec3};
use optobench_surfaces::brep::{BrepFace, FaceSurface};
use optobench_surfaces::{Plane, Sphere, Surface};
use optobench_trace::Coating;
use optobench_trace::Ray;
use optobench_trace::nonsequential::{
    End, Face, Ledger, NonSequentialSystem, RayRecord, Region, Role, Shape,
};

/// Why a scene is not valid.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneError {
    /// A description of what is wrong, naming the item.
    Invalid(String),
}

impl core::fmt::Display for SceneError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SceneError::Invalid(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for SceneError {}

fn invalid(message: impl Into<String>) -> SceneError {
    SceneError::Invalid(message.into())
}

/// One surface of a lens: radius of curvature (mm; infinite for a plane) and conic
/// constant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LensSurface {
    /// Radius of curvature (mm). `f64::INFINITY` for a plane.
    pub radius: f64,
    /// Conic constant.
    pub conic: f64,
}

impl LensSurface {
    /// The surface for a sequential system.
    fn boxed(&self) -> Box<dyn Surface> {
        if self.radius.is_infinite() {
            Box::new(Plane)
        } else if self.conic == 0.0 {
            Box::new(Sphere::from_radius(self.radius))
        } else {
            Box::new(optobench_surfaces::Conic {
                curvature: 1.0 / self.radius,
                conic: self.conic,
            })
        }
    }

    fn shape(&self) -> Arc<dyn Surface> {
        if self.radius.is_infinite() {
            Arc::new(Plane)
        } else if self.conic == 0.0 {
            Arc::new(Sphere::from_radius(self.radius))
        } else {
            Arc::new(optobench_surfaces::Conic {
                curvature: 1.0 / self.radius,
                conic: self.conic,
            })
        }
    }
}

/// A lens's first-order properties at `wavelength` (µm) in `surrounding`: focal length,
/// back focal length and principal planes from its surfaces, centre thicknesses and
/// glasses (`optobench_trace::paraxial`), light travelling along its +z. `None` for an
/// element that is not a lens, a lens of zero power, or a glass not defined there.
pub fn lens_cardinal_points(
    kind: &ElementKind,
    surrounding: &Medium,
    wavelength: f64,
) -> Option<optobench_trace::CardinalPoints> {
    let ElementKind::Lens {
        surfaces,
        thicknesses,
        materials,
        ..
    } = kind
    else {
        return None;
    };
    let system = optobench_trace::SequentialSystem {
        object_medium: surrounding.clone(),
        surfaces: surfaces
            .iter()
            .enumerate()
            .map(|(i, s)| optobench_trace::SequentialSurface {
                surface: s.boxed(),
                aperture: optobench_surfaces::Aperture::Unlimited,
                interaction: optobench_trace::Interaction::Refract,
                coating: Coating::Ideal,
                thickness: thicknesses.get(i).copied().unwrap_or(0.0),
                medium_after: materials
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| surrounding.clone()),
            })
            .collect(),
    };
    system.cardinal_points(wavelength).ok().flatten()
}

/// What an element is.
#[derive(Debug, Clone)]
pub enum ElementKind {
    /// A lens: surfaces in order along its axis, thicknesses and materials between them.
    Lens {
        /// N surfaces.
        surfaces: Vec<LensSurface>,
        /// N − 1 centre thicknesses (mm).
        thicknesses: Vec<f64>,
        /// N − 1 materials, between consecutive surfaces.
        materials: Vec<Medium>,
        /// Mechanical diameter (mm).
        diameter: f64,
        /// Clear aperture diameter (mm).
        clear_aperture: f64,
        /// N coatings, one per surface; stacks listed from the air side (for a surface
        /// between two glasses, from the side of the earlier one).
        coatings: Vec<Coating>,
    },
    /// A mirror, reflecting on both sides.
    Mirror {
        /// Radius of curvature (mm), infinite for a flat mirror.
        radius: f64,
        /// Mechanical diameter (mm).
        diameter: f64,
        /// Clear aperture diameter (mm).
        clear_aperture: f64,
    },
    /// An ideal thin lens (a perfect paraxial lens): the frame's origin is its plane.
    IdealLens {
        /// Focal length (mm); negative for a diverging lens.
        focal_length: f64,
        /// Diameter (mm): rays beyond it are clipped.
        diameter: f64,
    },
    /// An ideal microscope objective (`optobench_trace::objective`): the frame's origin
    /// is its pupil, +z towards the sample.
    Objective {
        /// The objective's design.
        objective: optobench_trace::objective::IdealObjective,
        /// The barrel's diameter at the pupil (mm): rays inside it but beyond the pupil
        /// are stopped, rays outside it pass by.
        diameter: f64,
        /// The group-delay dispersion of its glass (s²), as its maker states it at the
        /// laser's wavelength: the ideal objective has no glass of its own, so pulses
        /// through it pick up this much (M11b). 0 if unknown.
        group_delay_dispersion: f64,
    },
    /// A sample: flat layers from the frame's origin along +z, between the medium above
    /// the first face (the immersion) and the one below the last.
    Sample {
        /// Layers in order: thickness (mm) and medium.
        layers: Vec<(f64, Medium)>,
        /// The medium above the first face; the surroundings if `None`.
        above: Option<Medium>,
        /// The medium below the last face; the surroundings if `None`.
        below: Option<Medium>,
        /// Lateral extent (mm).
        diameter: f64,
    },
}

/// An optical element.
#[derive(Debug, Clone)]
pub struct Element {
    /// Unique id within the scene.
    pub id: String,
    /// Frame at the first vertex; +z along the optical axis (the normal for a mirror).
    pub frame: Frame,
    /// What it is.
    pub kind: ElementKind,
}

/// What a source emits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SourceKind {
    /// A collimated beam of uniform irradiance along local +z.
    Collimated {
        /// Beam diameter (mm).
        diameter: f64,
    },
    /// A point at the frame origin emitting into a cone around local +z, uniform in
    /// direction cosines.
    Point {
        /// Cone half-angle (radians).
        half_angle: f64,
    },
    /// A Gaussian beam with its waist at the frame origin, along local +z. Ray positions
    /// and slopes are sampled with Gaussian weights and scaled so their second moments
    /// are exactly the beam's: radius w₀ (1/e² of irradiance) and divergence
    /// θ = M²λ/(πw₀). The bundle's second-moment width then follows the embedded-Gaussian
    /// law W(z) = w₀ √(1 + (z/z_R)²), z_R = πw₀²/(M²λ) (A. E. Siegman, Lasers, ch. 17).
    Gaussian {
        /// Waist diameter 2w₀ (mm).
        waist_diameter: f64,
        /// Beam quality M² (1 for an ideal TEM₀₀ beam).
        m_squared: f64,
        /// Sampling of directions: rings of the slope grid.
        angle_rings: usize,
    },
    /// A fiber end at the frame origin: positions uniform over the core, directions
    /// uniform in direction cosines inside the numerical aperture.
    Fiber {
        /// Core diameter (mm).
        core_diameter: f64,
        /// Numerical aperture (sine of the half-angle of acceptance, in air).
        numerical_aperture: f64,
        /// Sampling of directions: rings of the direction-cosine grid.
        angle_rings: usize,
    },
    /// A point emitting the same power into every solid angle (a fluorophore, M11c),
    /// traced within `half_angle` of local +z (π for every direction). `power` is the
    /// total over all directions; the rays carry the cone's share, (1 − cos θ) / 2 of it.
    /// Directions are equal solid angles: the disk samples mapped onto the cap by
    /// cos θ = 1 − ρ² (1 − cos θ_max), which keeps areas in proportion.
    Isotropic {
        /// The traced cone's half-angle (radians, up to π).
        half_angle: f64,
    },
    /// A Lambertian emitter: a square of side `size` across local +z, radiance the same
    /// in every direction of the forward hemisphere (directions uniform in direction
    /// cosines, which is exactly Lambertian for rays of equal power).
    Led {
        /// Side of the square emitter (mm).
        size: f64,
        /// Sampling of directions: rings of the direction-cosine grid.
        angle_rings: usize,
    },
}

/// A light source.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    /// Unique id within the scene.
    pub id: String,
    /// Frame of the source; emission along local +z.
    pub frame: Frame,
    /// What it emits.
    pub kind: SourceKind,
    /// Vacuum wavelength (µm), used when `spectrum` is empty.
    pub wavelength: f64,
    /// Total power (W).
    pub power: f64,
    /// Sampling: hexapolar rings (1 + 3·rings·(rings + 1) rays); for extended sources,
    /// the rings (or grid) of positions.
    pub rings: usize,
    /// Spectral lines as (wavelength µm, relative weight): the power is shared between
    /// them in proportion to the weights. Empty for a single `wavelength`.
    pub spectrum: Vec<(f64, f64)>,
    /// For a pulsed laser, its pulses (M11b); `power` is then the mean power. `None` for
    /// continuous light.
    pub pulse: Option<Pulse>,
}

/// The shape of a laser pulse's intensity in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PulseShape {
    /// exp(−4 ln 2 t² / τ²).
    Gaussian,
    /// sech²(1.7627 t / τ), as from a mode-locked Ti:sapphire laser.
    SechSquared,
}

/// The pulses of a pulsed laser (M11b).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pulse {
    /// Pulses per second (Hz).
    pub repetition_rate: f64,
    /// Full width at half maximum of the intensity as the laser makes it, transform
    /// limited (s).
    pub width: f64,
    /// The shape in time.
    pub shape: PulseShape,
    /// Group-delay dispersion the pulses carry as they leave the source (s²), such as a
    /// negative prechirp that cancels the path's; 0 for transform-limited pulses.
    pub prechirp: f64,
}

/// A fluorophore (M11b): what it absorbs and emits, from the user's data with its
/// source, and where in the samples it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Fluorophore {
    /// Unique id within the scene.
    pub id: String,
    /// One-photon absorption cross-section σ₁ (mm²) against vacuum wavelength (µm);
    /// `None` if not given.
    pub one_photon: Option<Spectrum>,
    /// Two-photon absorption cross-section σ₂ (mm⁴ s) against the laser's vacuum
    /// wavelength (µm), counting two photons per excitation (1 GM = 10⁻⁴⁸ mm⁴ s); `None`
    /// if not given.
    pub two_photon: Option<Spectrum>,
    /// Emission spectrum (relative) against vacuum wavelength (µm); `None` if not given.
    pub emission: Option<Spectrum>,
    /// Photons emitted per excitation.
    pub quantum_yield: f64,
    /// Lifetime of the excited state (s); `None` if not given.
    pub lifetime: Option<f64>,
    /// Where the values come from.
    pub source: String,
    /// Where it is, and how much.
    pub concentrations: Vec<Concentration>,
}

/// A boundary of a sample's thermal domain (M11d).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeatBoundary {
    /// Held at the starting temperature.
    Fixed,
    /// No heat crosses it.
    Insulated,
    /// A heat transfer coefficient to a bath at the starting temperature (W mm⁻² K⁻¹).
    Transfer(f64),
}

/// The thermal properties of one layer of a sample (M11d).
#[derive(Debug, Clone, PartialEq)]
pub struct ThermalProperties {
    /// Thermal conductivity k (W mm⁻¹ K⁻¹).
    pub conductivity: f64,
    /// Volumetric heat capacity ρc (J mm⁻³ K⁻¹).
    pub capacity: f64,
    /// Perfusion ρ_b c_b w_b (W mm⁻³ K⁻¹); 0 without.
    pub perfusion: f64,
    /// Where the values come from.
    pub source: String,
}

/// A sample's thermal description (M11d): per layer its properties, and the boundaries of
/// the domain the layers make (the first face, the last and the side at the sample's
/// radius).
#[derive(Debug, Clone, PartialEq)]
pub struct SampleThermal {
    /// The sample element's id.
    pub sample: String,
    /// Per layer, in order; `None` where not given.
    pub layers: Vec<Option<ThermalProperties>>,
    /// At the first face.
    pub top: HeatBoundary,
    /// At the last face.
    pub bottom: HeatBoundary,
    /// At the side.
    pub side: HeatBoundary,
}

/// How much of a fluorophore is in one layer of a sample.
#[derive(Debug, Clone, PartialEq)]
pub struct Concentration {
    /// The sample element's id.
    pub sample: String,
    /// The layer, counted from 1.
    pub layer: usize,
    /// mol/L.
    pub molar: f64,
}

/// A mechanical proxy: absorbs whatever hits it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProxyKind {
    /// A tube's inner wall, from the frame origin along local +z.
    Tube {
        /// Inner diameter (mm).
        inner_diameter: f64,
        /// Length (mm).
        length: f64,
    },
    /// A square plate across the axis with a round hole: an aperture stop or iris.
    Plate {
        /// Hole diameter (mm).
        hole_diameter: f64,
        /// Side of the plate (mm).
        size: f64,
    },
}

/// A mechanical proxy.
#[derive(Debug, Clone, PartialEq)]
pub struct Proxy {
    /// Unique id within the scene.
    pub id: String,
    /// Frame: tube start, or the plate's centre, +z along the axis.
    pub frame: Frame,
    /// What it is.
    pub kind: ProxyKind,
}

/// A detector's quantum efficiency: the fraction of arriving photons it counts, as a
/// number or over wavelength, and where it comes from (a datasheet, a measurement).
#[derive(Debug, Clone, PartialEq)]
pub struct QuantumEfficiency {
    /// From 0 to 1.
    pub curve: optobench_materials::Spectrum,
    /// Where the values come from.
    pub source: String,
}

/// A detector: a rectangle across the axis that absorbs and records.
#[derive(Debug, Clone, PartialEq)]
pub struct Detector {
    /// Unique id within the scene.
    pub id: String,
    /// Frame at the detector centre, +z along the axis; local x and y are its axes.
    pub frame: Frame,
    /// Width (local x) and height (local y) (mm).
    pub size: (f64, f64),
    /// Pixels along local x and y.
    pub pixels: (usize, usize),
    /// What fraction of the photons it counts (M11c); `None` where only power matters.
    pub quantum_efficiency: Option<QuantumEfficiency>,
}

/// What a solid from CAD (STEP) does.
#[derive(Debug, Clone)]
pub enum SolidRole {
    /// Absorbs and records clipping on every face: mounts, posts, rods, and every body a
    /// sidecar does not mention.
    Absorber,
    /// Refracts on its optical faces, with `material` inside. Its cylindrical and conical
    /// faces (the ground edge and bevels) absorb as clipping.
    Lens {
        /// The glass.
        material: Medium,
        /// Coating on every optical face not in `face_coatings`.
        coating: Coating,
        /// Coatings of particular faces (indices into the solid's faces), such as the one
        /// coated face of a filter.
        face_coatings: Vec<(usize, Coating)>,
    },
    /// Reflects on the faces listed (indices into the solid's faces); the others absorb.
    Mirror {
        /// The reflecting faces.
        reflecting: Vec<usize>,
    },
}

/// One exact face of a solid, placed in the scene frame.
#[derive(Debug, Clone)]
pub struct SolidFace {
    /// The face in its body's own frame.
    pub geometry: Arc<BrepFace>,
    /// Body frame to scene frame.
    pub placement: optobench_math::Transform,
}

/// A solid from CAD.
#[derive(Debug, Clone)]
pub struct Solid {
    /// The element id for a tagged body; for an untagged one, its assembly path and body
    /// name (`er10:3/19229-001-1-solid1`).
    pub id: String,
    /// Its faces.
    pub faces: Vec<SolidFace>,
    /// What it does.
    pub role: SolidRole,
    /// The CAD occurrence it belongs to (assembly path, or product name for a body at the
    /// root): DOFs move whole occurrences.
    pub occurrence: String,
}

/// A standalone scene.
#[derive(Debug, Clone)]
pub struct Scene {
    /// The medium around everything.
    pub surrounding: Medium,
    /// Optical elements.
    pub elements: Vec<Element>,
    /// Light sources.
    pub sources: Vec<Source>,
    /// Mechanical proxies.
    pub proxies: Vec<Proxy>,
    /// Detectors.
    pub detectors: Vec<Detector>,
    /// Solids from CAD (STEP), with their roles.
    pub solids: Vec<Solid>,
    /// Ray splitting at partly reflecting faces: reflected parts carrying at least this
    /// fraction of the total source power are followed as child rays; `None` follows the
    /// transmitted part only. The default is [`DEFAULT_SPLIT_CUTOFF`] (docs/design.md,
    /// Optical model).
    pub split_cutoff: Option<f64>,
    /// Degrees of freedom (see [`dof`]).
    pub dofs: Vec<dof::Dof>,
    /// Fluorophores in the samples (M11b).
    pub fluorophores: Vec<Fluorophore>,
    /// The samples' thermal descriptions (M11d).
    pub thermal: Vec<SampleThermal>,
    /// Tolerances (M14).
    pub tolerances: Vec<tolerance::Tolerance>,
}

/// Default splitting cutoff, as a fraction of the total source power.
pub const DEFAULT_SPLIT_CUTOFF: f64 = 1e-6;

/// Who a face belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// `scene.elements[i]`.
    Element(usize),
    /// `scene.proxies[i]`.
    Proxy(usize),
    /// `scene.detectors[i]`.
    Detector(usize),
    /// `scene.solids[i]`.
    Solid(usize),
}

/// The non-sequential system for a scene, with the owner of each face.
#[derive(Debug, Clone)]
pub struct Built {
    /// The faces and media.
    pub system: NonSequentialSystem,
    /// `owners[face.owner]` for every face.
    pub owners: Vec<Owner>,
    /// The media of samples' layers: (element index, part, medium index), the part
    /// `above`, `layer 1`, `layer 2`, … or `below`.
    pub sample_media: Vec<(usize, String, usize)>,
}

/// What the direct light left in one part of a sample (W).
#[derive(Debug, Clone, PartialEq)]
pub struct LayerAccount {
    /// The sample's id.
    pub sample: String,
    /// `above`, `layer 1`, …, `below`.
    pub part: String,
    /// Absorbed in its bulk.
    pub absorbed: f64,
    /// Scattered out of the direct beam (the diffuse light's source).
    pub scattered: f64,
}

/// Points in the unit disk with the area each stands for, normalised to sum to 1: the
/// centre and `rings` rings of 6i points, ring i at radius i/(rings + ½) standing for the
/// annulus out to (i + ½)/(rings + ½). Every point lies inside the disk.
pub fn disk_samples(rings: usize) -> Vec<(f64, f64, f64)> {
    let n = rings as f64 + 0.5;
    let edge = |i: f64| ((i + 0.5) / n).min(1.0);
    let mut out = vec![(0.0, 0.0, edge(0.0).powi(2))];
    for ring in 1..=rings {
        let r = ring as f64 / n;
        let count = 6 * ring;
        let area = edge(ring as f64).powi(2) - edge(ring as f64 - 1.0).powi(2);
        for k in 0..count {
            let a = core::f64::consts::TAU * k as f64 / count as f64;
            out.push((r * a.cos(), r * a.sin(), area / count as f64));
        }
    }
    out
}

/// Gaussian samples: points of a disk grid out to 2.5σ, weighted by exp(−r²/2σ²) times
/// their area, and scaled so the weighted second moment per axis is exactly σ².
fn gaussian_samples(rings: usize, sigma: f64) -> Vec<(f64, f64, f64)> {
    let reach = 2.5;
    let mut points: Vec<(f64, f64, f64)> = disk_samples(rings)
        .into_iter()
        .map(|(u, v, a)| {
            let (x, y) = (u * reach, v * reach);
            (x, y, a * (-(x * x + y * y) / 2.0).exp())
        })
        .collect();
    let total: f64 = points.iter().map(|p| p.2).sum();
    let moment: f64 = points.iter().map(|p| p.2 * p.0 * p.0).sum::<f64>() / total;
    let scale = sigma / moment.sqrt();
    for p in &mut points {
        p.0 *= scale;
        p.1 *= scale;
        p.2 /= total;
    }
    points
}

/// Hexapolar points in the unit disk: centre plus `rings` rings of 6i points.
pub fn hexapolar(rings: usize) -> Vec<(f64, f64)> {
    let mut points = vec![(0.0, 0.0)];
    for ring in 1..=rings {
        let radius = ring as f64 / rings as f64;
        let count = 6 * ring;
        for k in 0..count {
            let angle = core::f64::consts::TAU * k as f64 / count as f64;
            points.push((radius * angle.cos(), radius * angle.sin()));
        }
    }
    points
}

impl Scene {
    fn check_ids(&self) -> Result<(), SceneError> {
        let mut ids: Vec<&str> = self
            .elements
            .iter()
            .map(|e| e.id.as_str())
            .chain(self.sources.iter().map(|s| s.id.as_str()))
            .chain(self.proxies.iter().map(|p| p.id.as_str()))
            .chain(self.detectors.iter().map(|d| d.id.as_str()))
            .chain(self.solids.iter().map(|s| s.id.as_str()))
            .collect();
        ids.sort_unstable();
        if let Some(pair) = ids.windows(2).find(|w| w[0] == w[1]) {
            return Err(invalid(format!("id `{}` is used more than once", pair[0])));
        }
        Ok(())
    }

    /// Builds the non-sequential system.
    pub fn build(&self) -> Result<Built, SceneError> {
        self.check_ids()?;
        let mut system = NonSequentialSystem::new(self.surrounding.clone());
        let mut owners = Vec::new();
        let air = 0;
        // The first face of each sample, for finding what is in front of an objective.
        let mut sample_tops: Vec<usize> = Vec::new();
        let mut sample_media: Vec<(usize, String, usize)> = Vec::new();

        for (index, element) in self.elements.iter().enumerate() {
            let owner = owners.len();
            owners.push(Owner::Element(index));
            let transform = element.frame.transform();
            match &element.kind {
                ElementKind::Lens {
                    surfaces,
                    thicknesses,
                    materials,
                    diameter,
                    clear_aperture,
                    coatings,
                } => {
                    let id = &element.id;
                    if surfaces.len() < 2
                        || thicknesses.len() != surfaces.len() - 1
                        || materials.len() != surfaces.len() - 1
                        || coatings.len() != surfaces.len()
                    {
                        return Err(invalid(format!(
                            "lens `{id}`: N surfaces need N − 1 thicknesses, N − 1 materials and N coatings"
                        )));
                    }
                    if !(*clear_aperture > 0.0 && clear_aperture <= diameter) {
                        return Err(invalid(format!(
                            "lens `{id}`: clear aperture must be positive and at most the diameter"
                        )));
                    }
                    let rim = diameter / 2.0;
                    let media: Vec<usize> = core::iter::once(air)
                        .chain(materials.iter().map(|m| system.add_medium(m.clone())))
                        .chain(core::iter::once(air))
                        .collect();
                    let mut z = 0.0;
                    let mut edge = Vec::new();
                    for (i, surface) in surfaces.iter().enumerate() {
                        let shape = surface.shape();
                        let sag = shape.sag(rim).ok_or_else(|| {
                            invalid(format!(
                                "lens `{id}`: surface {} (R = {} mm) is smaller than the lens diameter",
                                i + 1,
                                surface.radius
                            ))
                        })?;
                        edge.push(z + sag);
                        system.faces.push(Face {
                            shape: Shape::Profile(shape),
                            placement: transform
                                * optobench_math::Transform::from_translation(Vec3::new(
                                    0.0, 0.0, z,
                                )),
                            region: Region::Disk { radius: rim },
                            role: Role::Refract {
                                // Stacks are written from the air side; the first
                                // surface has the air on its negative side.
                                coating: if i == 0 {
                                    coatings[i].turned()
                                } else {
                                    coatings[i].clone()
                                },
                                clear_radius: Some(clear_aperture / 2.0),
                            },
                            negative_medium: media[i],
                            positive_medium: media[i + 1],
                            owner,
                        });
                        z += thicknesses.get(i).copied().unwrap_or(0.0);
                    }
                    let (first, last) = (edge[0], edge[edge.len() - 1]);
                    if last < first {
                        return Err(invalid(format!(
                            "lens `{id}`: negative edge thickness ({:.4} mm)",
                            last - first
                        )));
                    }
                    if last > first {
                        // The ground edge: an absorbing cylinder between the rims.
                        system.faces.push(Face {
                            shape: Shape::Cylinder {
                                radius: rim,
                                length: last - first,
                            },
                            placement: transform
                                * optobench_math::Transform::from_translation(Vec3::new(
                                    0.0, 0.0, first,
                                )),
                            region: Region::Disk { radius: rim },
                            role: Role::Absorber,
                            negative_medium: air,
                            positive_medium: air,
                            owner,
                        });
                    }
                }
                ElementKind::IdealLens {
                    focal_length,
                    diameter,
                } => {
                    let id = &element.id;
                    if !(focal_length.is_finite() && *focal_length != 0.0 && *diameter > 0.0) {
                        return Err(invalid(format!(
                            "ideal lens `{id}`: needs a finite, non-zero focal length and a positive diameter"
                        )));
                    }
                    let flat = LensSurface {
                        radius: f64::INFINITY,
                        conic: 0.0,
                    };
                    system.faces.push(Face {
                        shape: Shape::Profile(flat.shape()),
                        placement: transform,
                        region: Region::Disk {
                            radius: diameter / 2.0 * 1.0001,
                        },
                        role: Role::IdealLens {
                            focal_length: *focal_length,
                            clear_radius: Some(diameter / 2.0),
                        },
                        negative_medium: air,
                        positive_medium: air,
                        owner,
                    });
                }
                ElementKind::Objective {
                    objective,
                    diameter,
                    ..
                } => {
                    let id = &element.id;
                    if !(objective.focal_length > 0.0
                        && objective.na > 0.0
                        && objective.pupil_distance > 0.0)
                    {
                        return Err(invalid(format!(
                            "objective `{id}`: focal length, NA and pupil distance must be positive"
                        )));
                    }
                    if *diameter < 2.0 * objective.pupil_radius() {
                        return Err(invalid(format!(
                            "objective `{id}`: the diameter ({diameter} mm) is smaller than the pupil (f · NA = {:.3} mm radius)",
                            objective.pupil_radius()
                        )));
                    }
                    let flat = LensSurface {
                        radius: f64::INFINITY,
                        conic: 0.0,
                    };
                    system.faces.push(Face {
                        shape: Shape::Profile(flat.shape()),
                        placement: transform,
                        region: Region::Disk {
                            radius: diameter / 2.0,
                        },
                        role: Role::Objective {
                            objective: std::sync::Arc::new(objective.clone()),
                            exit_medium: air,
                        },
                        negative_medium: air,
                        positive_medium: air,
                        owner,
                    });
                    // Its front plane, for light coming back from the sample (M11c).
                    system.faces.push(Face {
                        shape: Shape::Profile(flat.shape()),
                        placement: transform
                            * optobench_math::Transform::from_translation(Vec3::new(
                                0.0,
                                0.0,
                                objective.pupil_distance,
                            )),
                        region: Region::Disk {
                            radius: diameter / 2.0,
                        },
                        role: Role::ObjectiveFront {
                            objective: std::sync::Arc::new(objective.clone()),
                        },
                        negative_medium: air,
                        positive_medium: air,
                        owner,
                    });
                }
                ElementKind::Sample {
                    layers,
                    above,
                    below,
                    diameter,
                } => {
                    let id = &element.id;
                    if layers.is_empty() || layers.iter().any(|(t, _)| t.is_nan() || *t <= 0.0) {
                        return Err(invalid(format!(
                            "sample `{id}`: at least one layer, each thicker than zero"
                        )));
                    }
                    let flat = LensSurface {
                        radius: f64::INFINITY,
                        conic: 0.0,
                    };
                    let medium_or_air = |m: &Option<Medium>, system: &mut NonSequentialSystem| {
                        m.as_ref().map_or(air, |m| system.add_medium(m.clone()))
                    };
                    let mut media = vec![medium_or_air(above, &mut system)];
                    for (_, m) in layers {
                        media.push(system.add_medium(m.clone()));
                    }
                    media.push(medium_or_air(below, &mut system));
                    let parts = std::iter::once("above".to_owned())
                        .chain((1..=layers.len()).map(|k| format!("layer {k}")))
                        .chain(std::iter::once("below".to_owned()));
                    for (part, &m) in parts.zip(&media) {
                        if m != air {
                            sample_media.push((index, part, m));
                        }
                    }
                    let mut z = 0.0;
                    for i in 0..=layers.len() {
                        if i == 0 {
                            sample_tops.push(system.faces.len());
                        }
                        system.faces.push(Face {
                            shape: Shape::Profile(flat.shape()),
                            placement: transform
                                * optobench_math::Transform::from_translation(Vec3::new(
                                    0.0, 0.0, z,
                                )),
                            region: Region::Disk {
                                radius: diameter / 2.0,
                            },
                            role: Role::Refract {
                                coating: Coating::Uncoated,
                                clear_radius: None,
                            },
                            negative_medium: media[i],
                            positive_medium: media[i + 1],
                            owner,
                        });
                        z += layers.get(i).map_or(0.0, |(t, _)| *t);
                    }
                }
                ElementKind::Mirror {
                    radius,
                    diameter,
                    clear_aperture,
                } => {
                    let surface = LensSurface {
                        radius: *radius,
                        conic: 0.0,
                    };
                    system.faces.push(Face {
                        shape: Shape::Profile(surface.shape()),
                        placement: transform,
                        region: Region::Disk {
                            radius: diameter / 2.0,
                        },
                        role: Role::Mirror {
                            clear_radius: Some(clear_aperture / 2.0),
                        },
                        negative_medium: air,
                        positive_medium: air,
                        owner,
                    });
                }
            }
        }

        // Each objective's rays go on in the medium in front of it: above the first sample
        // face its axis meets from the front plane, else the surroundings.
        let objectives: Vec<usize> = (0..system.faces.len())
            .filter(|&i| matches!(system.faces[i].role, Role::Objective { .. }))
            .collect();
        for i in objectives {
            let Role::Objective { objective, .. } = &system.faces[i].role else {
                continue;
            };
            let front = system.faces[i].placement.apply_point(Vec3::new(
                0.0,
                0.0,
                objective.pupil_distance,
            ));
            let axis = system.faces[i].placement.apply_vector(Vec3::Z);
            let medium = sample_tops
                .iter()
                .filter_map(|&f| {
                    let face = &system.faces[f];
                    let local = face.placement.inverse();
                    let o = local.apply_point(front);
                    let d = local.apply_vector(axis);
                    // The top face is the plane z = 0 of its frame.
                    (d.z.abs() > 1e-12)
                        .then(|| -o.z / d.z)
                        .filter(|t| *t >= -1e-9)
                        .map(|t| (t, face.negative_medium))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map_or(air, |(_, m)| m);
            if let Role::Objective { exit_medium, .. } = &mut system.faces[i].role {
                *exit_medium = medium;
            }
        }

        for (index, proxy) in self.proxies.iter().enumerate() {
            let owner = owners.len();
            owners.push(Owner::Proxy(index));
            let (shape, region) = match proxy.kind {
                ProxyKind::Tube {
                    inner_diameter,
                    length,
                } => (
                    Shape::Cylinder {
                        radius: inner_diameter / 2.0,
                        length,
                    },
                    Region::Disk { radius: 0.0 },
                ),
                ProxyKind::Plate {
                    hole_diameter,
                    size,
                } => (
                    Shape::Profile(Arc::new(Plane)),
                    Region::SquareWithHole {
                        half_size: size / 2.0,
                        hole_radius: hole_diameter / 2.0,
                    },
                ),
            };
            system.faces.push(Face {
                shape,
                placement: proxy.frame.transform(),
                region,
                role: Role::Absorber,
                negative_medium: air,
                positive_medium: air,
                owner,
            });
        }

        for (index, detector) in self.detectors.iter().enumerate() {
            if detector.pixels.0 == 0 || detector.pixels.1 == 0 {
                return Err(invalid(format!(
                    "detector `{}` needs at least one pixel",
                    detector.id
                )));
            }
            let owner = owners.len();
            owners.push(Owner::Detector(index));
            system.faces.push(Face {
                shape: Shape::Profile(Arc::new(Plane)),
                placement: detector.frame.transform(),
                region: Region::Rectangle {
                    half_width: detector.size.0 / 2.0,
                    half_height: detector.size.1 / 2.0,
                },
                role: Role::Detector { detector: index },
                negative_medium: air,
                positive_medium: air,
                owner,
            });
        }
        for (index, solid) in self.solids.iter().enumerate() {
            let owner = owners.len();
            owners.push(Owner::Solid(index));
            let inside = match &solid.role {
                SolidRole::Lens { material, .. } => system.add_medium(material.clone()),
                _ => air,
            };
            for (face_index, face) in solid.faces.iter().enumerate() {
                let edge = matches!(
                    face.geometry.surface,
                    FaceSurface::Cylinder { .. } | FaceSurface::Cone { .. }
                );
                let role = match &solid.role {
                    SolidRole::Absorber => Role::Absorber,
                    SolidRole::Lens {
                        coating,
                        face_coatings,
                        ..
                    } if !edge => Role::Refract {
                        coating: face_coatings
                            .iter()
                            .find(|(k, _)| *k == face_index)
                            .map_or(coating, |(_, c)| c)
                            .clone(),
                        clear_radius: None,
                    },
                    SolidRole::Lens { .. } => Role::Absorber,
                    SolidRole::Mirror { reflecting } if reflecting.contains(&face_index) => {
                        Role::Mirror { clear_radius: None }
                    }
                    SolidRole::Mirror { .. } => Role::Absorber,
                };
                system.faces.push(Face {
                    shape: Shape::Brep(face.geometry.clone()),
                    placement: face.placement,
                    region: Region::Disk { radius: 0.0 },
                    role,
                    negative_medium: inside,
                    positive_medium: air,
                    owner,
                });
            }
        }
        let total: f64 = self.sources.iter().map(|s| s.power).sum();
        system.split_cutoff = self.split_cutoff.map(|fraction| fraction * total);
        system.build_index();
        Ok(Built {
            system,
            owners,
            sample_media,
        })
    }

    /// The rays a source launches, sharing its power: for each spectral line, every
    /// combination of a position sample and a direction sample.
    pub fn launch(source: &Source) -> Vec<Ray> {
        let lines: Vec<(f64, f64)> = if source.spectrum.is_empty() {
            vec![(source.wavelength, 1.0)]
        } else {
            let total: f64 = source.spectrum.iter().map(|l| l.1).sum();
            source
                .spectrum
                .iter()
                .map(|&(w, p)| (w, p / total))
                .collect()
        };
        let frame = &source.frame;
        let mut rays = Vec::new();
        for (wavelength, share) in lines {
            // (position, direction, fraction of the line's power)
            let bundle: Vec<(Vec3, Vec3, f64)> = match source.kind {
                SourceKind::Collimated { diameter } => {
                    let points = hexapolar(source.rings);
                    let each = 1.0 / points.len() as f64;
                    points
                        .into_iter()
                        .map(|(u, v)| {
                            (
                                Vec3::new(u * diameter / 2.0, v * diameter / 2.0, 0.0),
                                Vec3::Z,
                                each,
                            )
                        })
                        .collect()
                }
                SourceKind::Point { half_angle } => {
                    let points = hexapolar(source.rings);
                    let each = 1.0 / points.len() as f64;
                    let s = half_angle.sin();
                    points
                        .into_iter()
                        .map(|(u, v)| {
                            let (a, b) = (u * s, v * s);
                            (
                                Vec3::ZERO,
                                Vec3::new(a, b, (1.0 - a * a - b * b).sqrt()),
                                each,
                            )
                        })
                        .collect()
                }
                SourceKind::Isotropic { half_angle } => {
                    let cap = 1.0 - half_angle.min(core::f64::consts::PI).cos();
                    disk_samples(source.rings)
                        .into_iter()
                        .map(|(u, v, a)| {
                            let rho2 = u * u + v * v;
                            let cos = 1.0 - rho2 * cap;
                            let sin = (1.0 - cos * cos).max(0.0).sqrt();
                            let (cu, su) = if rho2 > 0.0 {
                                let rho = rho2.sqrt();
                                (u / rho, v / rho)
                            } else {
                                (1.0, 0.0)
                            };
                            (
                                Vec3::ZERO,
                                Vec3::new(sin * cu, sin * su, cos),
                                a * cap / 2.0,
                            )
                        })
                        .collect()
                }
                SourceKind::Gaussian {
                    waist_diameter,
                    m_squared,
                    angle_rings,
                } => {
                    let w0 = waist_diameter / 2.0;
                    let theta = m_squared * wavelength * 1e-3 / (core::f64::consts::PI * w0);
                    let positions = gaussian_samples(source.rings, w0 / 2.0);
                    let slopes = gaussian_samples(angle_rings, theta / 2.0);
                    positions
                        .iter()
                        .flat_map(|&(x, y, a)| {
                            slopes.iter().map(move |&(sx, sy, b)| {
                                (Vec3::new(x, y, 0.0), Vec3::new(sx, sy, 1.0), a * b)
                            })
                        })
                        .collect()
                }
                SourceKind::Fiber {
                    core_diameter,
                    numerical_aperture,
                    angle_rings,
                } => {
                    let positions = disk_samples(source.rings);
                    let directions = disk_samples(angle_rings);
                    let r = core_diameter / 2.0;
                    positions
                        .iter()
                        .flat_map(|&(x, y, a)| {
                            directions.iter().map(move |&(u, v, b)| {
                                let (u, v) = (u * numerical_aperture, v * numerical_aperture);
                                (
                                    Vec3::new(x * r, y * r, 0.0),
                                    Vec3::new(u, v, (1.0 - u * u - v * v).sqrt()),
                                    a * b,
                                )
                            })
                        })
                        .collect()
                }
                SourceKind::Led { size, angle_rings } => {
                    // A square grid of (2·rings + 1)² cell centres over the emitter.
                    let n = 2 * source.rings + 1;
                    let cell = size / n as f64;
                    let directions = disk_samples(angle_rings);
                    let mut out = Vec::new();
                    for i in 0..n {
                        for j in 0..n {
                            let x = -size / 2.0 + cell * (i as f64 + 0.5);
                            let y = -size / 2.0 + cell * (j as f64 + 0.5);
                            for &(u, v, b) in &directions {
                                out.push((
                                    Vec3::new(x, y, 0.0),
                                    Vec3::new(u, v, (1.0 - u * u - v * v).max(0.0).sqrt()),
                                    b / (n * n) as f64,
                                ));
                            }
                        }
                    }
                    out
                }
            };
            rays.extend(
                bundle
                    .into_iter()
                    .filter_map(|(position, direction, fraction)| {
                        Ray::new(
                            frame.to_parent_point(position),
                            frame.to_parent_vector(direction),
                            wavelength,
                            source.power * share * fraction,
                        )
                    }),
            );
        }
        rays
    }

    /// The medium a source's rays start in: a sample layer's, if the source sits inside
    /// one (an emitter in the sample, M11c), else the system's start medium. The media
    /// above and below a sample reach without end in the model, so a source there is
    /// taken to be in the surroundings.
    pub fn start_medium(&self, built: &Built, source: &Source) -> usize {
        let origin = source.frame.origin();
        for (index, element) in self.elements.iter().enumerate() {
            let ElementKind::Sample {
                layers, diameter, ..
            } = &element.kind
            else {
                continue;
            };
            let local = element.frame.to_local_point(origin);
            if local.x.hypot(local.y) > diameter / 2.0 {
                continue;
            }
            let mut z = 0.0;
            for (k, (thickness, _)) in layers.iter().enumerate() {
                if local.z > z && local.z < z + thickness {
                    let part = format!("layer {}", k + 1);
                    if let Some((_, _, medium)) = built
                        .sample_media
                        .iter()
                        .find(|(e, p, _)| *e == index && *p == part)
                    {
                        return *medium;
                    }
                }
                z += thickness;
            }
        }
        built.system.start_medium
    }

    /// Builds the scene, traces every source and collects the results.
    pub fn trace(&self) -> Result<SceneTrace, SceneError> {
        let built = self.build()?;
        Ok(self.trace_built(&built))
    }

    /// Traces every source through `built` (this scene's [`Scene::build`]) and collects
    /// the results.
    pub fn trace_built(&self, built: &Built) -> SceneTrace {
        let rays: Vec<(Ray, usize)> = self
            .sources
            .iter()
            .flat_map(|s| {
                let medium = self.start_medium(built, s);
                Self::launch(s).into_iter().map(move |r| (r, medium))
            })
            .collect();
        let (records, ledger) = built.system.trace_all_from(rays);

        let detectors = self
            .detectors
            .iter()
            .enumerate()
            .map(|(index, detector)| DetectorResult::collect(index, detector, &records))
            .collect();

        let mut clipping: Vec<Clipping> = Vec::new();
        for record in &records {
            let End::Clipped { face } = record.end else {
                continue;
            };
            let owner = built.owners[built.system.faces[face].owner];
            let id = match owner {
                Owner::Element(i) => &self.elements[i].id,
                Owner::Proxy(i) => &self.proxies[i].id,
                Owner::Detector(i) => &self.detectors[i].id,
                Owner::Solid(i) => &self.solids[i].id,
            };
            match clipping.iter_mut().find(|c| c.owner == owner) {
                Some(entry) => {
                    entry.power += record.ray.power;
                    entry.rays += 1;
                }
                None => clipping.push(Clipping {
                    id: id.clone(),
                    owner,
                    power: record.ray.power,
                    rays: 1,
                }),
            }
        }
        let footprints = self.footprints(built, &records);
        let layers = built
            .sample_media
            .iter()
            .map(|(element, part, medium)| {
                let (mut absorbed, mut scattered) = (0.0, 0.0);
                for d in records.iter().flat_map(|r| &r.deposits) {
                    if d.medium == *medium {
                        absorbed += d.absorbed;
                        scattered += d.scattered;
                    }
                }
                LayerAccount {
                    sample: self.elements[*element].id.clone(),
                    part: part.clone(),
                    absorbed,
                    scattered,
                }
            })
            .collect();
        SceneTrace {
            records,
            ledger,
            detectors,
            clipping,
            footprints,
            layers,
        }
    }
}

/// Width of the bins of a detector's angular distribution (degrees).
pub const ANGULAR_BIN_DEGREES: f64 = 5.0;

/// Power-weighted centroid and RMS radius of detector hits.
fn centroid_and_rms(hits: &[((f64, f64), f64)]) -> ((f64, f64), f64) {
    let power: f64 = hits.iter().map(|h| h.1).sum();
    let weight = if power > 0.0 { power } else { 1.0 };
    let centroid = (
        hits.iter().map(|((x, _), w)| x * w).sum::<f64>() / weight,
        hits.iter().map(|((_, y), w)| y * w).sum::<f64>() / weight,
    );
    let rms = (hits
        .iter()
        .map(|((x, y), w)| w * ((x - centroid.0).powi(2) + (y - centroid.1).powi(2)))
        .sum::<f64>()
        / weight)
        .sqrt();
    (centroid, rms)
}

/// Where the direct beam (rays' main paths, not ghosts) lands on one optical element.
#[derive(Debug, Clone, PartialEq)]
pub struct Footprint {
    /// The element's id (for a doublet from CAD, the id before the `/`).
    pub id: String,
    /// Hits on the element's faces.
    pub hits: usize,
    /// Power arriving at those hits (W); a ray crossing two faces counts twice.
    pub power: f64,
    /// Power-weighted centre of the hits (scene frame).
    pub centroid: Vec3,
    /// The beam's radius on the element: on each of its faces, the largest distance of a
    /// hit from that face's centroid; the largest over the faces (mm).
    pub radius: f64,
    /// For an inline element: the largest distance of a hit from the element's axis as a
    /// fraction of the clear aperture radius (1 = the beam fills the clear aperture).
    pub clear_aperture_used: Option<f64>,
}

/// Hit points with their power and face, and the inline element they belong to (if any).
type FootprintHits = (Vec<(Vec3, f64, usize)>, Option<usize>);

impl Scene {
    /// Footprints on every optical element that light reached, in scene order.
    fn footprints(&self, built: &Built, records: &[RayRecord]) -> Vec<Footprint> {
        // Key: element index for inline elements, then tagged solids by element id.
        let mut order: Vec<String> = Vec::new();
        let mut hits: std::collections::BTreeMap<String, FootprintHits> = Default::default();
        // The beam itself: direct light only, not ghosts split off at reflecting faces.
        for record in records.iter().filter(|r| r.generation == 0) {
            for (k, ((point, face), power)) in record.points[1..]
                .iter()
                .zip(&record.faces)
                .zip(&record.powers)
                .enumerate()
            {
                // An objective's second entry is where the ray leaves its front plane,
                // not a point on its pupil.
                let departure = k > 0
                    && record.faces[k - 1] == *face
                    && built.system.faces[*face].role.records_twice();
                if departure {
                    continue;
                }
                let owner = built.owners[built.system.faces[*face].owner];
                let (key, element) = match owner {
                    Owner::Element(i) => (self.elements[i].id.clone(), Some(i)),
                    Owner::Solid(i) if !matches!(self.solids[i].role, SolidRole::Absorber) => {
                        let id = &self.solids[i].id;
                        (id.split('/').next().unwrap_or(id).to_owned(), None)
                    }
                    _ => continue,
                };
                let entry = hits.entry(key.clone()).or_insert_with(|| {
                    order.push(key);
                    (Vec::new(), element)
                });
                entry.0.push((*point, *power, *face));
            }
        }
        order
            .into_iter()
            .filter_map(|id| {
                let (points, element) = hits.remove(&id)?;
                let power: f64 = points.iter().map(|p| p.1).sum();
                let weighted_centre = |hits: &[&(Vec3, f64, usize)]| {
                    let total: f64 = hits.iter().map(|h| h.1).sum();
                    let weight = if total > 0.0 { total } else { 1.0 };
                    hits.iter()
                        .fold(Vec3::ZERO, |c, (p, w, _)| c + *p * (*w / weight))
                };
                let centroid = weighted_centre(&points.iter().collect::<Vec<_>>());
                // The beam's size on each face (each face lies across the beam), the
                // largest of them; not the spread over all faces, which would include
                // the element's thickness.
                let mut faces: Vec<usize> = points.iter().map(|p| p.2).collect();
                faces.sort_unstable();
                faces.dedup();
                let radius = faces
                    .iter()
                    .map(|&f| {
                        let on_face: Vec<&(Vec3, f64, usize)> =
                            points.iter().filter(|p| p.2 == f).collect();
                        let centre = weighted_centre(&on_face);
                        on_face
                            .iter()
                            .map(|(p, _, _)| (*p - centre).length())
                            .fold(0.0, f64::max)
                    })
                    .fold(0.0, f64::max);
                let clear_aperture_used = element.and_then(|i| {
                    let e = &self.elements[i];
                    let clear = match &e.kind {
                        ElementKind::Lens { clear_aperture, .. }
                        | ElementKind::Mirror { clear_aperture, .. } => clear_aperture / 2.0,
                        ElementKind::Objective { objective, .. } => objective.pupil_radius(),
                        ElementKind::IdealLens { diameter, .. } => diameter / 2.0,
                        ElementKind::Sample { diameter, .. } => diameter / 2.0,
                    };
                    let axis = e.frame.axes()[2];
                    let reach = points
                        .iter()
                        .map(|(p, _, _)| {
                            let d = *p - e.frame.origin();
                            (d - axis * d.dot(axis)).length()
                        })
                        .fold(0.0, f64::max);
                    (clear > 0.0).then(|| reach / clear)
                });
                Some(Footprint {
                    id,
                    hits: points.len(),
                    power,
                    centroid,
                    radius,
                    clear_aperture_used,
                })
            })
            .collect()
    }
}

/// What reached one detector.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectorResult {
    /// The detector's id.
    pub id: String,
    /// Power received (W).
    pub power: f64,
    /// Of that, the power arriving on rays' main paths (generation 0); the rest arrived
    /// on rays split off at partly reflecting faces (ghosts).
    pub direct_power: f64,
    /// Power-weighted centroid of the direct light only (mm).
    pub direct_centroid: (f64, f64),
    /// Power-weighted RMS distance of the direct light from its centroid (mm).
    pub direct_rms_radius: f64,
    /// Power-weighted mean slope (dx/dz, dy/dz) of the direct light's directions in the
    /// detector's frame: where the beam points (radians, small angles).
    pub direct_mean_slope: (f64, f64),
    /// Power-weighted RMS spread of the direct light's slopes about their mean: zero for
    /// a collimated beam (radians).
    pub direct_rms_slope: f64,
    /// Rays received.
    pub rays: usize,
    /// Power-weighted centroid in the detector's local (x, y) (mm).
    pub centroid: (f64, f64),
    /// Power-weighted RMS distance from the centroid (mm).
    pub rms_radius: f64,
    /// Irradiance per pixel (W/m²), row by row from −y to +y, each row from −x to +x.
    pub irradiance: Vec<f64>,
    /// Power arriving at angles from the detector's normal in bins of
    /// [`ANGULAR_BIN_DEGREES`], from 0° (normal) to 90° (W).
    pub angular: Vec<f64>,
}

impl DetectorResult {
    fn collect(index: usize, detector: &Detector, records: &[RayRecord]) -> Self {
        let hits: Vec<((f64, f64), f64)> = records
            .iter()
            .filter(|r| matches!(r.end, End::Detected { detector, .. } if detector == index))
            .filter_map(|r| r.detector_point.map(|p| (p, r.ray.power)))
            .collect();
        let power: f64 = hits.iter().map(|h| h.1).sum();
        let direct: Vec<((f64, f64), f64)> = records
            .iter()
            .filter(|r| r.generation == 0)
            .filter(|r| matches!(r.end, End::Detected { detector, .. } if detector == index))
            .filter_map(|r| r.detector_point.map(|p| (p, r.ray.power)))
            .collect();
        let direct_power: f64 = direct.iter().map(|h| h.1).sum();
        let (direct_centroid, direct_rms_radius) = centroid_and_rms(&direct);
        // Slopes of the direct rays in the detector's frame.
        let slopes: Vec<((f64, f64), f64)> = records
            .iter()
            .filter(|r| r.generation == 0)
            .filter(|r| matches!(r.end, End::Detected { detector, .. } if detector == index))
            .filter_map(|r| {
                let d = detector.frame.to_local_vector(r.ray.direction);
                (d.z.abs() > 1e-12).then(|| ((d.x / d.z, d.y / d.z), r.ray.power))
            })
            .collect();
        let (direct_mean_slope, direct_rms_slope) = centroid_and_rms(&slopes);
        let bins = (90.0 / ANGULAR_BIN_DEGREES).round() as usize;
        let mut angular = vec![0.0; bins];
        let normal = detector.frame.axes()[2];
        for record in records
            .iter()
            .filter(|r| matches!(r.end, End::Detected { detector, .. } if detector == index))
        {
            let angle = record
                .ray
                .direction
                .dot(normal)
                .abs()
                .min(1.0)
                .acos()
                .to_degrees();
            let bin = ((angle / ANGULAR_BIN_DEGREES) as usize).min(bins - 1);
            angular[bin] += record.ray.power;
        }
        let weight = if power > 0.0 { power } else { 1.0 };
        let centroid = (
            hits.iter().map(|((x, _), w)| x * w).sum::<f64>() / weight,
            hits.iter().map(|((_, y), w)| y * w).sum::<f64>() / weight,
        );
        let rms_radius = (hits
            .iter()
            .map(|((x, y), w)| w * ((x - centroid.0).powi(2) + (y - centroid.1).powi(2)))
            .sum::<f64>()
            / weight)
            .sqrt();
        let (nx, ny) = detector.pixels;
        let (width, height) = detector.size;
        let pixel_area_m2 = (width / nx as f64) * (height / ny as f64) * 1e-6;
        let mut irradiance = vec![0.0; nx * ny];
        for ((x, y), w) in &hits {
            let column = (((x + width / 2.0) / width) * nx as f64).floor() as isize;
            let row = (((y + height / 2.0) / height) * ny as f64).floor() as isize;
            let column = column.clamp(0, nx as isize - 1) as usize;
            let row = row.clamp(0, ny as isize - 1) as usize;
            irradiance[row * nx + column] += w / pixel_area_m2;
        }
        Self {
            id: detector.id.clone(),
            power,
            direct_power,
            direct_centroid,
            direct_rms_radius,
            direct_mean_slope,
            direct_rms_slope,
            angular,
            rays: hits.len(),
            centroid,
            rms_radius,
            irradiance,
        }
    }
}

/// Power clipped by one element or proxy.
#[derive(Debug, Clone, PartialEq)]
pub struct Clipping {
    /// The item's id.
    pub id: String,
    /// Which item.
    pub owner: Owner,
    /// Power clipped (W).
    pub power: f64,
    /// Rays clipped.
    pub rays: usize,
}

/// The result of tracing a scene.
#[derive(Debug, Clone)]
pub struct SceneTrace {
    /// Every ray.
    pub records: Vec<RayRecord>,
    /// Power totals.
    pub ledger: Ledger,
    /// Per detector, in scene order.
    pub detectors: Vec<DetectorResult>,
    /// Per clipping item, in order of first clip.
    pub clipping: Vec<Clipping>,
    /// Per optical element that light reached.
    pub footprints: Vec<Footprint>,
    /// Per part of each sample: what the direct light left there.
    pub layers: Vec<LayerAccount>,
}

#[cfg(test)]
mod tests;
