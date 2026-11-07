//! The optical scene: elements, sources, detectors, degrees of freedom, mechanical proxies and the sample element.
//!
//! Layer L3. See `docs/design.md`, sections Architecture and Standalone scenes.
//!
//! M3 provides standalone scenes: items placed by a [`layout`] (planar or table), built
//! into a non-sequential system ([`Scene::build`]), traced from their sources
//! ([`Scene::trace`]) with per-detector results and a clipping list. Degrees of freedom
//! arrive in M6, the sample element in M8.
//!
//! Every item has a frame whose local +z is the optical axis at that point; for a
//! mirror it is the mirror's normal.

#![forbid(unsafe_code)]

pub mod layout;

use std::sync::Arc;

use optobench_materials::Medium;
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
        /// Coating on every surface.
        coating: Coating,
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
    /// Vacuum wavelength (µm).
    pub wavelength: f64,
    /// Total power (W).
    pub power: f64,
    /// Sampling: hexapolar rings (1 + 3·rings·(rings + 1) rays).
    pub rings: usize,
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
        /// Coating on every optical face.
        coating: Coating,
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
                    coating,
                } => {
                    let id = &element.id;
                    if surfaces.len() < 2
                        || thicknesses.len() != surfaces.len() - 1
                        || materials.len() != surfaces.len() - 1
                    {
                        return Err(invalid(format!(
                            "lens `{id}`: N surfaces need N − 1 thicknesses and N − 1 materials"
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
                                coating: coating.clone(),
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
                    SolidRole::Lens { coating, .. } if !edge => Role::Refract {
                        coating: coating.clone(),
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
        Ok(Built { system, owners })
    }

    /// The rays a source launches, each with an equal share of its power.
    pub fn launch(source: &Source) -> Vec<Ray> {
        let points = hexapolar(source.rings);
        let share = source.power / points.len() as f64;
        let frame = &source.frame;
        points
            .into_iter()
            .filter_map(|(u, v)| {
                let (position, direction) = match source.kind {
                    SourceKind::Collimated { diameter } => {
                        let r = diameter / 2.0;
                        (Vec3::new(u * r, v * r, 0.0), Vec3::Z)
                    }
                    SourceKind::Point { half_angle } => {
                        let s = half_angle.sin();
                        let (a, b) = (u * s, v * s);
                        (Vec3::ZERO, Vec3::new(a, b, (1.0 - a * a - b * b).sqrt()))
                    }
                };
                Ray::new(
                    frame.to_parent_point(position),
                    frame.to_parent_vector(direction),
                    source.wavelength,
                    share,
                )
            })
            .collect()
    }

    /// Builds the scene, traces every source and collects the results.
    pub fn trace(&self) -> Result<SceneTrace, SceneError> {
        let built = self.build()?;
        let rays: Vec<Ray> = self.sources.iter().flat_map(Self::launch).collect();
        let (records, ledger) = built.system.trace_all(rays);

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
        let footprints = self.footprints(&built, &records);
        Ok(SceneTrace {
            records,
            ledger,
            detectors,
            clipping,
            footprints,
        })
    }
}

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
            for ((point, face), power) in record.points[1..]
                .iter()
                .zip(&record.faces)
                .zip(&record.powers)
            {
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
    /// Rays received.
    pub rays: usize,
    /// Power-weighted centroid in the detector's local (x, y) (mm).
    pub centroid: (f64, f64),
    /// Power-weighted RMS distance from the centroid (mm).
    pub rms_radius: f64,
    /// Irradiance per pixel (W/m²), row by row from −y to +y, each row from −x to +x.
    pub irradiance: Vec<f64>,
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
}

#[cfg(test)]
mod tests;
