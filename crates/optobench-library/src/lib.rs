//! Parts libraries that the user builds or imports. Nothing in this crate is vendor data.
//!
//! Layer L3. See `docs/design.md`, Sidecar and library formats ("Parts library").
//!
//! - [`Part`]: one catalogue part as its spec sheet gives it (radii, centre thicknesses,
//!   glasses, outline), with the spec sheet as its source.
//! - [`Library`]: the parts the user has, looked up by key (`thorlabs/LA1951-ML`) or by
//!   the name of a STEP product.
//! - [`profile`] and [`compare`]: the prescription read back from a body's exact faces
//!   and compared with a part, so simplified or wrong vendor models are reported (any
//!   difference above [`CHECK_TOLERANCE`]) and the glass bodies of a mounted part are
//!   told from its mount.
//!
//! Reading and writing part files is `optobench-io`'s; this crate has no file system
//! access.

#![forbid(unsafe_code)]

use optobench_math::Vec3;
use optobench_surfaces::brep::{BrepFace, FaceSurface};

/// Differences between a part and its STEP geometry above this are reported (mm).
pub const CHECK_TOLERANCE: f64 = 0.05;

/// What a part does optically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartType {
    /// One or more glasses between curved or flat surfaces: singlets, cemented doublets.
    Lens,
    /// A plate that transmits: windows, filters.
    Filter,
    /// A reflecting surface on a substrate.
    Mirror,
}

impl PartType {
    /// The word in a part file's `type`.
    pub fn as_str(self) -> &'static str {
        match self {
            PartType::Lens => "lens",
            PartType::Filter => "filter",
            PartType::Mirror => "mirror",
        }
    }

    /// Whether light passes through it (lenses and filters).
    pub fn transmits(self) -> bool {
        !matches!(self, PartType::Mirror)
    }
}

/// A glass: a material name (bundled or imported) or a constant index.
#[derive(Debug, Clone, PartialEq)]
pub enum Glass {
    /// A material name, as the spec sheet writes it (`N-BK7`).
    Named(String),
    /// A constant refractive index.
    Index(f64),
}

/// The part's outline, seen along its axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outline {
    /// Round, of this diameter (mm).
    Round {
        /// Diameter.
        diameter: f64,
    },
    /// Rectangular (mm).
    Rectangular {
        /// Width.
        width: f64,
        /// Height.
        height: f64,
    },
}

/// One surface of a part's prescription.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartSurface {
    /// Radius of curvature (mm), positive when the centre lies after the vertex in the
    /// direction light travels; infinite for a flat surface.
    pub radius: f64,
    /// Conic constant (0 for a sphere).
    pub conic: f64,
}

impl PartSurface {
    /// A flat surface.
    pub const FLAT: Self = Self {
        radius: f64::INFINITY,
        conic: 0.0,
    };
}

/// How thick a thin-film layer is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayerThickness {
    /// Physical thickness (µm).
    Physical(f64),
    /// A quarter wave of optical thickness at this vacuum wavelength (µm): λ / (4 n(λ)).
    QuarterWave(f64),
}

/// One layer of a thin-film coating, by name.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerSpec {
    /// The layer material.
    pub material: Glass,
    /// Its thickness.
    pub thickness: LayerThickness,
}

/// A coating as files write it: names, resolved to an optical coating by `optobench-io`.
#[derive(Debug, Clone, PartialEq)]
pub enum CoatingSpec {
    /// A bare interface (Fresnel reflection).
    Uncoated,
    /// A perfect anti-reflection coating.
    Ideal,
    /// A measured R/T curve, by its name in the library.
    Curve(String),
    /// A thin-film stack, outer side first.
    Layers(Vec<LayerSpec>),
}

/// One catalogue part.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// The vendor as written (`Thorlabs`).
    pub vendor: String,
    /// The vendor's part number (`LA1951-ML`).
    pub number: String,
    /// What it does.
    pub part_type: PartType,
    /// Where the numbers came from: the spec sheet's address or title.
    pub source: String,
    /// The outline.
    pub outline: Outline,
    /// Clear aperture diameter (mm), when the spec sheet gives one.
    pub clear_aperture: Option<f64>,
    /// Surfaces in the order light meets them.
    pub surfaces: Vec<PartSurface>,
    /// Centre thickness between consecutive surfaces (mm); for a mirror, its substrate.
    pub thickness: Vec<f64>,
    /// Glass between consecutive surfaces; for a mirror, its substrate.
    pub glass: Vec<Glass>,
    /// Coating of every surface, when given (for a filter, a measured curve is the whole
    /// part's response; see [`Part::surface_coatings`]).
    pub coating: Option<CoatingSpec>,
    /// Coatings surface by surface, when given (instead of `coating`).
    pub coatings: Vec<CoatingSpec>,
    /// Other names of its STEP product, beyond the part number (a vendor's file name).
    pub products: Vec<String>,
}

impl Part {
    /// The key it is looked up by: lower-case vendor and part number, `thorlabs/LA1951-ML`.
    pub fn key(&self) -> String {
        format!("{}/{}", self.vendor.to_lowercase(), self.number)
    }

    /// Whether `key` names this part (case is ignored).
    pub fn has_key(&self, key: &str) -> bool {
        key.eq_ignore_ascii_case(&self.key())
    }

    /// Whether a STEP product of this name is this part: its part number or one of its
    /// `products`, case ignored.
    pub fn names_product(&self, product: &str) -> bool {
        product.eq_ignore_ascii_case(&self.number)
            || self
                .products
                .iter()
                .any(|p| product.eq_ignore_ascii_case(p))
    }

    /// The largest dimension of the outline (mm).
    pub fn size(&self) -> f64 {
        match self.outline {
            Outline::Round { diameter } => diameter,
            Outline::Rectangular { width, height } => width.max(height),
        }
    }

    /// Checks that the prescription is complete and consistent.
    pub fn check(&self) -> Result<(), String> {
        let n = self.surfaces.len();
        match self.part_type {
            PartType::Lens | PartType::Filter => {
                if n < 2 {
                    return Err(format!(
                        "a {} has at least two surfaces",
                        self.part_type.as_str()
                    ));
                }
                if self.thickness.len() != n - 1 || self.glass.len() != n - 1 {
                    return Err(format!(
                        "{n} surfaces need {} thicknesses and {} glasses",
                        n - 1,
                        n - 1
                    ));
                }
                if self.part_type == PartType::Filter
                    && self.surfaces.iter().any(|s| s.radius.is_finite())
                {
                    return Err("a filter's surfaces are flat".into());
                }
            }
            PartType::Mirror => {
                if n != 1 {
                    return Err("a mirror has one surface".into());
                }
                if self.thickness.len() > 1 || self.glass.len() > 1 {
                    return Err("a mirror has at most one substrate thickness and glass".into());
                }
            }
        }
        if self.thickness.iter().any(|t| !(*t > 0.0 && t.is_finite())) {
            return Err("thicknesses must be positive".into());
        }
        if self
            .surfaces
            .iter()
            .any(|s| s.radius == 0.0 || s.radius.is_nan() || !s.conic.is_finite())
        {
            return Err("radii must be non-zero (\"inf\" for flat)".into());
        }
        let size = match self.outline {
            Outline::Round { diameter } => diameter,
            Outline::Rectangular { width, height } => width.min(height),
        };
        if !(size > 0.0 && size.is_finite()) {
            return Err("the outline must have a positive size".into());
        }
        if !self.coatings.is_empty() && self.coatings.len() != n {
            return Err(format!("{n} surfaces need {n} coatings"));
        }
        if !self.coatings.is_empty() && self.coating.is_some() {
            return Err("give `coating` or `coatings`, not both".into());
        }
        if let Some(ca) = self.clear_aperture {
            if !(ca > 0.0 && ca <= self.size()) {
                return Err("the clear aperture must be positive and fit in the outline".into());
            }
        }
        Ok(())
    }

    /// The coating of each surface, in order. With `coatings`, those. With `coating`: on
    /// a filter a curve is the measured response of the whole part, so it goes on the
    /// first surface and the others are ideal; anything else goes on every surface.
    /// Uncoated otherwise.
    pub fn surface_coatings(&self) -> Vec<CoatingSpec> {
        let n = self.surfaces.len();
        if !self.coatings.is_empty() {
            return self.coatings.clone();
        }
        match &self.coating {
            Some(curve @ CoatingSpec::Curve(_)) if self.part_type == PartType::Filter => {
                core::iter::once(curve.clone())
                    .chain(core::iter::repeat_n(
                        CoatingSpec::Ideal,
                        n.saturating_sub(1),
                    ))
                    .collect()
            }
            Some(coating) => vec![coating.clone(); n],
            None => vec![CoatingSpec::Uncoated; n],
        }
    }

    /// The same part turned around: surfaces, thicknesses and glasses in reverse order,
    /// radii negated.
    pub fn reversed(&self) -> Part {
        let mut part = self.clone();
        part.surfaces = self
            .surfaces
            .iter()
            .rev()
            .map(|s| PartSurface {
                radius: -s.radius,
                conic: s.conic,
            })
            .collect();
        part.thickness.reverse();
        part.glass.reverse();
        part.coatings = self.surface_coatings();
        part.coatings.reverse();
        part
    }
}

/// The parts a user has.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Library {
    parts: Vec<Part>,
}

impl Library {
    /// A library of `parts`. Two parts with one key, or two parts naming the same STEP
    /// product, are an error.
    pub fn new(parts: Vec<Part>) -> Result<Self, String> {
        for (i, a) in parts.iter().enumerate() {
            for b in &parts[..i] {
                if a.has_key(&b.key()) {
                    return Err(format!("two parts are `{}`", a.key()));
                }
                let names = core::iter::once(&a.number).chain(&a.products);
                if let Some(name) = names.into_iter().find(|n| b.names_product(n)) {
                    return Err(format!(
                        "`{}` and `{}` both name the STEP product `{name}`",
                        b.key(),
                        a.key()
                    ));
                }
            }
        }
        Ok(Self { parts })
    }

    /// Every part.
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    /// The part with this key (case ignored).
    pub fn get(&self, key: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.has_key(key))
    }

    /// The part a STEP product of this name is, if any.
    pub fn for_product(&self, product: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.names_product(product))
    }
}

/// The prescription of one body, read from its exact faces.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// A point on the optical axis, in the body's frame.
    pub origin: Vec3,
    /// The optical axis (unit), in the body's frame; its sign is arbitrary.
    pub axis: Vec3,
    /// Radius of the rim (the largest cylinder about the axis), for round bodies.
    pub rim_radius: Option<f64>,
    /// Optical surfaces along the axis, in increasing position: (vertex position along
    /// `axis` from `origin`, signed radius for light travelling along `axis`; infinite
    /// when flat).
    pub surfaces: Vec<(f64, f64)>,
    /// For each of `surfaces`, the faces it is made of (indices into the faces given to
    /// [`profile`]).
    pub surface_faces: Vec<Vec<usize>>,
}

impl Profile {
    /// The same profile along the opposite axis direction.
    pub fn flipped(&self) -> Profile {
        let mut surfaces: Vec<(f64, f64)> = self.surfaces.iter().map(|&(t, r)| (-t, -r)).collect();
        surfaces.reverse();
        let mut surface_faces = self.surface_faces.clone();
        surface_faces.reverse();
        Profile {
            origin: self.origin,
            axis: -self.axis,
            rim_radius: self.rim_radius,
            surfaces,
            surface_faces,
        }
    }
}

/// Distance from the axis within which a sphere's centre counts as on it (mm).
const ON_AXIS: f64 = 1e-3;

/// Reads a body's prescription: the axis is the axis of its largest cylinder (the rim),
/// or for a body without one, the normal of its closest pair of opposite flat faces (a
/// rectangular plate). Its optical surfaces are the spheres centred on the axis and the
/// flat faces across it. `None` when no axis is found.
pub fn profile(faces: &[&BrepFace]) -> Option<Profile> {
    let largest_cylinder = faces
        .iter()
        .filter_map(|f| match &f.surface {
            FaceSurface::Cylinder { frame, radius } => Some((frame, *radius)),
            _ => None,
        })
        .max_by(|a, b| a.1.total_cmp(&b.1));
    let (origin, axis, rim_radius) = match largest_cylinder {
        Some((frame, radius)) => (frame.origin(), frame.axes()[2], Some(radius)),
        None => {
            let planes: Vec<(Vec3, Vec3)> = faces
                .iter()
                .filter_map(|f| match &f.surface {
                    FaceSurface::Plane(frame) => {
                        let n = frame.axes()[2];
                        Some((frame.origin(), if f.reversed { -n } else { n }))
                    }
                    _ => None,
                })
                .collect();
            let mut closest: Option<(f64, Vec3, Vec3)> = None;
            for (i, &(p, n)) in planes.iter().enumerate() {
                for &(q, m) in &planes[i + 1..] {
                    if n.dot(m) < -(1.0 - 1e-9) {
                        let gap = (q - p).dot(n).abs();
                        if gap > 1e-9 && closest.is_none_or(|c| gap < c.0) {
                            closest = Some((gap, p, n));
                        }
                    }
                }
            }
            let (_, p, n) = closest?;
            (p, n, None)
        }
    };
    let along = |p: Vec3| (p - origin).dot(axis);
    let mut found: Vec<(f64, f64, usize)> = Vec::new();
    for (index, face) in faces.iter().enumerate() {
        match &face.surface {
            FaceSurface::Plane(frame) if frame.axes()[2].dot(axis).abs() > 1.0 - 1e-9 => {
                found.push((along(frame.origin()), f64::INFINITY, index));
            }
            FaceSurface::Sphere { frame, radius } => {
                let centre = frame.origin();
                let t_centre = along(centre);
                let off_axis = (centre - origin) - axis * t_centre;
                let points: Vec<Vec3> = face.loops.iter().flatten().copied().collect();
                if off_axis.length() > ON_AXIS || points.is_empty() {
                    continue;
                }
                // The cap lies on the side of the centre where its boundary is.
                let mean = points.iter().map(|&p| along(p)).sum::<f64>() / points.len() as f64;
                let side = if mean >= t_centre { 1.0 } else { -1.0 };
                found.push((t_centre + side * radius, -side * radius, index));
            }
            _ => {}
        }
    }
    // One surface may be several faces (a cap split in halves): merge them.
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut surfaces: Vec<(f64, f64)> = Vec::new();
    let mut surface_faces: Vec<Vec<usize>> = Vec::new();
    for (t, r, index) in found {
        let same = surfaces.last().is_some_and(|&(u, q)| {
            (u - t).abs() < 1e-6 && ((q.is_infinite() && r.is_infinite()) || (q - r).abs() < 1e-6)
        });
        if same {
            if let Some(last) = surface_faces.last_mut() {
                last.push(index);
            }
        } else {
            surfaces.push((t, r));
            surface_faces.push(vec![index]);
        }
    }
    Some(Profile {
        origin,
        axis,
        rim_radius,
        surfaces,
        surface_faces,
    })
}

/// One quantity where a part and its STEP geometry disagree.
#[derive(Debug, Clone, PartialEq)]
pub struct Difference {
    /// What differs: `radius 2`, `thickness 1`, `diameter` (surfaces and thicknesses
    /// counted from 1 in the part's order).
    pub quantity: String,
    /// The part's value (mm); infinite for flat.
    pub part: f64,
    /// The value read from STEP (mm).
    pub step: f64,
}

/// How a body compares with one glass of a part.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// Which glass of the part (0-based: between surfaces `glass` and `glass + 1`).
    pub glass: usize,
    /// Whether the body faces the other way along its axis.
    pub reversed: bool,
    /// Whether it is close enough to be this glass (radii within 2 % or 0.5 mm,
    /// thickness within 10 % or 0.3 mm, rim within 0.5 mm).
    pub fits: bool,
    /// Differences above [`CHECK_TOLERANCE`].
    pub differences: Vec<Difference>,
    /// Sum of the absolute differences (mm), to rank comparisons.
    pub score: f64,
}

/// Radii flatter than this count as flat (mm).
const FLAT: f64 = 1e5;

/// Compares a body's profile with glass `glass` of `part`, the body facing along its
/// axis (`reversed` false) or against it. `None` if the profile does not have exactly
/// two optical surfaces or the part has no such glass.
pub fn compare(part: &Part, profile: &Profile, glass: usize, reversed: bool) -> Option<Comparison> {
    let [(t0, r0), (t1, r1)] = profile.surfaces[..] else {
        return None;
    };
    if glass + 1 >= part.surfaces.len() || part.part_type == PartType::Mirror {
        return None;
    }
    let step_radii = if reversed { [-r1, -r0] } else { [r0, r1] };
    let mut fits = true;
    let mut score = 0.0;
    let mut differences = Vec::new();
    for (k, &step) in step_radii.iter().enumerate() {
        let expected = part.surfaces[glass + k].radius;
        let flat = |r: f64| !r.is_finite() || r.abs() > FLAT;
        let difference = match (flat(expected), flat(step)) {
            (true, true) => 0.0,
            (false, false) => (expected - step).abs(),
            _ => f64::INFINITY,
        };
        fits &= difference <= (0.02 * expected.abs()).max(0.5);
        score += difference.min(1e6);
        if difference > CHECK_TOLERANCE {
            differences.push(Difference {
                quantity: format!("radius {}", glass + k + 1),
                part: expected,
                step,
            });
        }
    }
    let thickness = t1 - t0;
    let expected = part.thickness[glass];
    let difference = (thickness - expected).abs();
    fits &= difference <= (0.1 * expected).max(0.3);
    score += difference;
    if difference > CHECK_TOLERANCE {
        differences.push(Difference {
            quantity: format!("thickness {}", glass + 1),
            part: expected,
            step: thickness,
        });
    }
    if let (Outline::Round { diameter }, Some(rim)) = (part.outline, profile.rim_radius) {
        let difference = (2.0 * rim - diameter).abs();
        fits &= difference <= 1.0;
        score += difference;
        if difference > CHECK_TOLERANCE {
            differences.push(Difference {
                quantity: "diameter".into(),
                part: diameter,
                step: 2.0 * rim,
            });
        }
    }
    Some(Comparison {
        glass,
        reversed,
        fits,
        differences,
        score,
    })
}

/// The closest glass of `part` for a body, over every glass and, unless `reversed` is
/// given, both directions. `None` if the profile cannot be compared.
pub fn closest(part: &Part, profile: &Profile, reversed: Option<bool>) -> Option<Comparison> {
    let directions: &[bool] = match reversed {
        Some(false) => &[false],
        Some(true) => &[true],
        None => &[false, true],
    };
    (0..part.glass.len())
        .flat_map(|glass| directions.iter().map(move |&r| (glass, r)))
        .filter_map(|(glass, r)| compare(part, profile, glass, r))
        .min_by(|a, b| b.fits.cmp(&a.fits).then(a.score.total_cmp(&b.score)))
}

#[cfg(test)]
mod tests;
