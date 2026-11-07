//! Non-sequential trace: each ray finds the nearest face in the whole scene, in any
//! order, until it is detected, clipped, escapes or reaches the interaction limit.
//!
//! # Faces
//!
//! A [`Face`] is a [`Shape`] in its own local frame, placed in the scene frame by a
//! transform and bounded by a [`Region`]. It separates two media, given as indices into
//! [`NonSequentialSystem::media`]:
//!
//! - for a surface profile, `negative_medium` lies on the −z side of the local frame and
//!   `positive_medium` on the +z side;
//! - for a cylinder, `negative_medium` is inside and `positive_medium` outside;
//! - for a B-rep face (from STEP), `negative_medium` is behind the face (inside the solid)
//!   and `positive_medium` on the side its normal points to.
//!
//! # Interactions and power
//!
//! The same physics as the sequential trace (`crate::sequential`): vector Snell's law;
//! at uncoated refracting faces the unpolarised Fresnel transmission, with the reflected
//! part leaving the path until ray splitting arrives in M5; total internal reflection is
//! followed; mirrors are ideal; media absorb by Beer–Lambert. Per ray,
//! launched = final power + reflected + absorbed, and the final power is booked as
//! detected, clipped, escaped or stopped.
//!
//! The nearest-hit search uses a BVH over the faces' bounding boxes once
//! [`NonSequentialSystem::build_index`] has been called, and tests every face otherwise.

use std::sync::Arc;

use optobench_materials::{MaterialError, Medium};
use optobench_math::{Transform, Vec3};
use optobench_surfaces::Surface;
use optobench_surfaces::brep::{Aabb, BrepFace};

use crate::bvh::Bvh;

use crate::interaction::{Refraction, fresnel_power, reflect, refract};
use crate::ray::Ray;
use crate::sequential::Coating;

/// Hits closer than this to a ray's origin are ignored, so a ray leaving a face does not
/// hit it again through rounding (mm).
pub const MINIMUM_DISTANCE: f64 = 1e-9;

/// Default limit on interactions per ray.
pub const DEFAULT_INTERACTION_LIMIT: usize = 1000;

/// The geometry of a face in its local frame.
#[derive(Debug, Clone)]
pub enum Shape {
    /// A rotationally symmetric profile z = sag(r), vertex at the origin.
    Profile(Arc<dyn Surface>),
    /// The inner or outer wall of a cylinder of `radius` around the local z axis, from
    /// z = 0 to z = `length`.
    Cylinder {
        /// Radius (mm).
        radius: f64,
        /// Length along local +z (mm).
        length: f64,
    },
    /// An exact trimmed face of a solid, from STEP; the region is not used.
    Brep(Arc<BrepFace>),
}

/// Where on a profile face the face exists, in local (x, y). Ignored for cylinders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Region {
    /// r ≤ radius.
    Disk {
        /// Radius (mm).
        radius: f64,
    },
    /// inner < r ≤ outer.
    Annulus {
        /// Hole radius (mm).
        inner: f64,
        /// Outer radius (mm).
        outer: f64,
    },
    /// |x| ≤ half_width and |y| ≤ half_height.
    Rectangle {
        /// Half the width along local x (mm).
        half_width: f64,
        /// Half the height along local y (mm).
        half_height: f64,
    },
    /// A square plate with a centred round hole: |x|, |y| ≤ half_size and r > hole_radius.
    SquareWithHole {
        /// Half the side of the plate (mm).
        half_size: f64,
        /// Radius of the hole (mm).
        hole_radius: f64,
    },
}

impl Region {
    /// Whether local point (x, y) lies on the face.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        let r = x.hypot(y);
        match *self {
            Region::Disk { radius } => r <= radius,
            Region::Annulus { inner, outer } => r > inner && r <= outer,
            Region::Rectangle {
                half_width,
                half_height,
            } => x.abs() <= half_width && y.abs() <= half_height,
            Region::SquareWithHole {
                half_size,
                hole_radius,
            } => x.abs() <= half_size && y.abs() <= half_size && r > hole_radius,
        }
    }
}

/// What a face does to a ray.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Role {
    /// Refraction between the face's two media. Hits beyond `clear_radius` (if given)
    /// are clipped at the element's edge instead.
    Refract {
        /// The coating.
        coating: Coating,
        /// Radius of the clear aperture (mm), or `None` if all of the region is clear.
        clear_radius: Option<f64>,
    },
    /// An ideal mirror on both sides. Hits beyond `clear_radius` are clipped.
    Mirror {
        /// Radius of the clear aperture (mm), or `None`.
        clear_radius: Option<f64>,
    },
    /// Absorbs and records clipping: mounts, tubes, plates, stops.
    Absorber,
    /// Absorbs and records a detection, for detector `detector`.
    Detector {
        /// The detector's index (meaning set by the caller).
        detector: usize,
    },
}

/// One face of the scene.
#[derive(Debug, Clone)]
pub struct Face {
    /// The geometry in the local frame.
    pub shape: Shape,
    /// Local frame to scene frame.
    pub placement: Transform,
    /// Where on the profile the face exists.
    pub region: Region,
    /// What it does.
    pub role: Role,
    /// Medium on the local −z side (profile) or inside (cylinder).
    pub negative_medium: usize,
    /// Medium on the local +z side (profile) or outside (cylinder).
    pub positive_medium: usize,
    /// The element, proxy or detector the face belongs to (meaning set by the caller).
    pub owner: usize,
}

/// A local hit: distance, local point and local unit normal (z ≥ 0 for profiles,
/// outward for cylinders).
struct LocalHit {
    distance: f64,
    point: Vec3,
    normal: Vec3,
}

impl Face {
    fn intersect(&self, origin: Vec3, direction: Vec3) -> Option<LocalHit> {
        let to_local = self.placement.inverse();
        let p = to_local.apply_point(origin);
        let d = to_local.apply_vector(direction);
        match &self.shape {
            Shape::Profile(surface) => {
                let hit = surface.intersect(p, d)?;
                (hit.distance > MINIMUM_DISTANCE && self.region.contains(hit.point.x, hit.point.y))
                    .then_some(LocalHit {
                        distance: hit.distance,
                        point: hit.point,
                        normal: hit.normal,
                    })
            }
            Shape::Brep(face) => face.intersect(p, d, MINIMUM_DISTANCE).map(|hit| LocalHit {
                distance: hit.distance,
                point: hit.point,
                normal: hit.normal,
            }),
            Shape::Cylinder { radius, length } => {
                // (px + t dx)² + (py + t dy)² = r².
                let a = d.x * d.x + d.y * d.y;
                if a == 0.0 {
                    return None;
                }
                let half_b = p.x * d.x + p.y * d.y;
                let c = p.x * p.x + p.y * p.y - radius * radius;
                let discriminant = half_b * half_b - a * c;
                if discriminant < 0.0 {
                    return None;
                }
                let root = discriminant.sqrt();
                [(-half_b - root) / a, (-half_b + root) / a]
                    .into_iter()
                    .filter(|&t| t > MINIMUM_DISTANCE)
                    .map(|t| (t, p + d * t))
                    .find(|(_, q)| q.z >= 0.0 && q.z <= *length)
                    .map(|(t, q)| LocalHit {
                        distance: t,
                        point: q,
                        normal: Vec3::new(q.x / radius, q.y / radius, 0.0),
                    })
            }
        }
    }
}

impl Face {
    /// A bounding box of the face in the scene frame.
    pub fn bounds(&self) -> Aabb {
        let local = match &self.shape {
            Shape::Brep(face) => face.bounds(),
            Shape::Cylinder { radius, length } => Aabb {
                min: Vec3::new(-radius, -radius, 0.0),
                max: Vec3::new(*radius, *radius, *length),
            },
            Shape::Profile(surface) => {
                let (half_x, half_y) = match self.region {
                    Region::Disk { radius } => (radius, radius),
                    Region::Annulus { outer, .. } => (outer, outer),
                    Region::Rectangle {
                        half_width,
                        half_height,
                    } => (half_width, half_height),
                    Region::SquareWithHole { half_size, .. } => (half_size, half_size),
                };
                let reach = half_x.hypot(half_y);
                let (low, high) = (0..=32)
                    .filter_map(|k| surface.sag(reach * k as f64 / 32.0))
                    .fold((0.0_f64, 0.0_f64), |(lo, hi), z| (lo.min(z), hi.max(z)));
                // Between samples a profile can bulge a little further; pad by the sag span.
                let pad = 0.05 * (high - low).abs();
                Aabb {
                    min: Vec3::new(-half_x, -half_y, low - pad),
                    max: Vec3::new(half_x, half_y, high + pad),
                }
            }
        };
        local
            .corners()
            .iter()
            .fold(Aabb::EMPTY, |b, c| b.with(self.placement.apply_point(*c)))
            .padded(1e-6)
    }
}

/// How a ray's trip ended.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum End {
    /// Absorbed by a detector face.
    Detected {
        /// The face.
        face: usize,
        /// The detector index from the face's role.
        detector: usize,
    },
    /// Absorbed by an absorber, or outside an element's clear aperture.
    Clipped {
        /// The face.
        face: usize,
    },
    /// Left the scene without hitting anything more.
    Escaped,
    /// Reached the interaction limit.
    Stopped,
    /// A medium could not be evaluated at the ray's wavelength.
    MaterialUndefined {
        /// Index into the system's media.
        medium: usize,
        /// Why.
        error: MaterialError,
    },
}

/// One traced ray.
#[derive(Debug, Clone, PartialEq)]
pub struct RayRecord {
    /// The ray as launched.
    pub launched: Ray,
    /// The ray at the end: position of the last hit (or the start, if it escaped at
    /// once), last direction and remaining power.
    pub ray: Ray,
    /// Start point and every hit point, in order (scene frame).
    pub points: Vec<Vec3>,
    /// How it ended.
    pub end: End,
    /// Power reflected out of the path at uncoated faces (W).
    pub reflected_power: f64,
    /// Power absorbed in media (W).
    pub absorbed_power: f64,
    /// For a detected ray: the hit in the detector face's local frame (x, y).
    pub detector_point: Option<(f64, f64)>,
}

/// Power totals over many rays (W).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Ledger {
    /// Launched.
    pub launched: f64,
    /// Arrived at detectors.
    pub detected: f64,
    /// Absorbed by absorbers and element edges.
    pub clipped: f64,
    /// Absorbed in media.
    pub absorbed: f64,
    /// Reflected out of the path at uncoated faces.
    pub reflected: f64,
    /// Left the scene.
    pub escaped: f64,
    /// Still carried by rays that reached the interaction limit or an undefined medium.
    pub stopped: f64,
}

impl Ledger {
    /// launched − (everything booked); zero up to rounding.
    pub fn imbalance(&self) -> f64 {
        self.launched
            - (self.detected
                + self.clipped
                + self.absorbed
                + self.reflected
                + self.escaped
                + self.stopped)
    }

    fn add(&mut self, record: &RayRecord) {
        self.launched += record.launched.power;
        self.absorbed += record.absorbed_power;
        self.reflected += record.reflected_power;
        let remaining = record.ray.power;
        match record.end {
            End::Detected { .. } => self.detected += remaining,
            End::Clipped { .. } => self.clipped += remaining,
            End::Escaped => self.escaped += remaining,
            End::Stopped | End::MaterialUndefined { .. } => self.stopped += remaining,
        }
    }
}

/// A scene's faces and media.
#[derive(Debug, Clone)]
pub struct NonSequentialSystem {
    /// Every medium in the scene; faces refer to them by index.
    pub media: Vec<Medium>,
    /// Every face.
    pub faces: Vec<Face>,
    /// The medium rays start in (usually the surrounding air).
    pub start_medium: usize,
    /// The most interactions a ray may have.
    pub interaction_limit: usize,
    /// The search index over `faces`, if built.
    index: Option<Bvh>,
}

impl NonSequentialSystem {
    /// An empty system with one surrounding medium.
    pub fn new(surrounding: Medium) -> Self {
        Self {
            media: vec![surrounding],
            faces: Vec::new(),
            start_medium: 0,
            interaction_limit: DEFAULT_INTERACTION_LIMIT,
            index: None,
        }
    }

    /// Adds a medium and returns its index.
    pub fn add_medium(&mut self, medium: Medium) -> usize {
        self.media.push(medium);
        self.media.len() - 1
    }

    fn medium(&self, index: usize, wavelength: f64) -> Result<(f64, f64), End> {
        let material = self.media[index].material();
        let fail = |error| End::MaterialUndefined {
            medium: index,
            error,
        };
        let n = material.refractive_index(wavelength).map_err(fail)?;
        let alpha = material.absorption_coefficient(wavelength).map_err(fail)?;
        Ok((n, alpha))
    }

    /// Builds the search index over the faces. Call it again after changing `faces`;
    /// a stale index (built over a different number of faces) is ignored.
    pub fn build_index(&mut self) {
        let boxes: Vec<Aabb> = self.faces.iter().map(Face::bounds).collect();
        self.index = Some(Bvh::build(&boxes));
    }

    /// The nearest face hit by the ray, with the local hit.
    fn nearest(&self, ray: &Ray) -> Option<(usize, LocalHit)> {
        if let Some(index) = self.index.as_ref().filter(|i| i.len() == self.faces.len()) {
            let mut found: Option<LocalHit> = None;
            let best = index.nearest(ray.position, ray.direction, |i| {
                let hit = self.faces[i].intersect(ray.position, ray.direction)?;
                let distance = hit.distance;
                if found.as_ref().is_none_or(|f| distance < f.distance) {
                    found = Some(hit);
                }
                Some(distance)
            })?;
            // `found` holds the hit with the smallest distance seen, which is `best`.
            return found.map(|hit| (best.0, hit));
        }
        self.faces
            .iter()
            .enumerate()
            .filter_map(|(i, face)| face.intersect(ray.position, ray.direction).map(|h| (i, h)))
            .min_by(|a, b| a.1.distance.total_cmp(&b.1.distance))
    }

    /// Traces one ray.
    pub fn trace(&self, launched: Ray) -> RayRecord {
        let mut record = RayRecord {
            launched,
            ray: launched,
            points: vec![launched.position],
            end: End::Escaped,
            reflected_power: 0.0,
            absorbed_power: 0.0,
            detector_point: None,
        };
        let mut medium = self.start_medium;
        for _ in 0..self.interaction_limit {
            let (index_here, alpha) = match self.medium(medium, launched.wavelength) {
                Ok(values) => values,
                Err(end) => {
                    record.end = end;
                    return record;
                }
            };
            let Some((face_index, hit)) = self.nearest(&record.ray) else {
                record.end = End::Escaped;
                return record;
            };
            let face = &self.faces[face_index];
            let ray = &mut record.ray;

            // Propagate to the hit.
            ray.optical_path += index_here * hit.distance;
            let kept = (-alpha * hit.distance).exp();
            record.absorbed_power += ray.power * (1.0 - kept);
            ray.power *= kept;
            ray.position = face.placement.apply_point(hit.point);
            record.points.push(ray.position);

            let r = hit.point.x.hypot(hit.point.y);
            let outside_clear = |clear: Option<f64>| clear.is_some_and(|c| r > c);
            match face.role {
                Role::Absorber => {
                    record.end = End::Clipped { face: face_index };
                    return record;
                }
                Role::Detector { detector } => {
                    record.end = End::Detected {
                        face: face_index,
                        detector,
                    };
                    record.detector_point = Some((hit.point.x, hit.point.y));
                    return record;
                }
                Role::Mirror { clear_radius } => {
                    if outside_clear(clear_radius) {
                        record.end = End::Clipped { face: face_index };
                        return record;
                    }
                    let normal = face.placement.apply_vector(hit.normal);
                    ray.direction = reflect(ray.direction, normal);
                }
                Role::Refract {
                    coating,
                    clear_radius,
                } => {
                    if outside_clear(clear_radius) {
                        record.end = End::Clipped { face: face_index };
                        return record;
                    }
                    let normal = face.placement.apply_vector(hit.normal);
                    // Which side the ray comes from decides the two media.
                    let towards_positive = ray.direction.dot(normal) > 0.0;
                    let (from, to) = if towards_positive {
                        (face.negative_medium, face.positive_medium)
                    } else {
                        (face.positive_medium, face.negative_medium)
                    };
                    let (n1, n2) = match (
                        self.medium(from, launched.wavelength),
                        self.medium(to, launched.wavelength),
                    ) {
                        (Ok((n1, _)), Ok((n2, _))) => (n1, n2),
                        (Err(end), _) | (_, Err(end)) => {
                            record.end = end;
                            return record;
                        }
                    };
                    match refract(ray.direction, normal, n1, n2) {
                        Refraction::Refracted(direction) => {
                            if coating == Coating::Uncoated {
                                let cosine = normal.dot(ray.direction).abs();
                                let transmitted =
                                    fresnel_power(cosine, n1, n2).unpolarised_transmittance();
                                record.reflected_power += ray.power * (1.0 - transmitted);
                                ray.power *= transmitted;
                            }
                            ray.direction = direction;
                            medium = to;
                        }
                        Refraction::TotalInternalReflection(direction) => {
                            ray.direction = direction;
                            medium = from;
                        }
                    }
                }
            }
        }
        record.end = End::Stopped;
        record
    }

    /// Traces many rays and totals their power.
    pub fn trace_all(&self, rays: impl IntoIterator<Item = Ray>) -> (Vec<RayRecord>, Ledger) {
        let mut ledger = Ledger::default();
        let records: Vec<RayRecord> = rays
            .into_iter()
            .map(|ray| {
                let record = self.trace(ray);
                ledger.add(&record);
                record
            })
            .collect();
        (records, ledger)
    }
}
