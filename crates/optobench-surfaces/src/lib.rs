//! Analytic optical surfaces with sag, normal and ray intersection.
//!
//! Layer L1. See `docs/design.md`, sections Architecture and Optical model.
//!
//! # Conventions
//!
//! - Every surface lives in its own local frame: the vertex is at the origin and +z is
//!   the optical axis. Placing it in a system is the caller's job (`optobench-trace`).
//! - The surface is `z = sag(r)` with `r = √(x² + y²)`, in millimetres.
//! - Curvature `c = 1/R`. A positive curvature puts the centre of curvature on +z.
//! - Conic constant `k`: 0 sphere, −1 paraboloid, < −1 hyperboloid, between −1 and 0 a
//!   prolate ellipsoid, > 0 an oblate ellipsoid.
//! - Rotationally symmetric profile (ISO 10110-12, aspheric surfaces):
//!   `sag(r) = c r² / (1 + √(1 − (1+k) c² r²)) + Σ aᵢ r^(2i+4)`, i = 0, 1, …
//! - Normals are unit vectors with a non-negative z component. Orienting a normal against
//!   an incoming ray is done in the trace.
//! - Intersections may lie behind the ray origin (negative distance). Sequential tracing
//!   needs this: with zero thickness, the second face of a thin lens can lie behind the
//!   first one off axis.

#![forbid(unsafe_code)]

pub mod brep;
mod segments;

use optobench_math::{Vec3, find_root};

/// Where a ray meets a surface, in the surface's local frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// Signed distance along the ray direction from its origin to the hit, in units of
    /// the direction's length (millimetres for a unit direction).
    pub distance: f64,
    /// The hit point.
    pub point: Vec3,
    /// Unit surface normal at the hit, with non-negative z component.
    pub normal: Vec3,
}

/// A rotationally symmetric surface in its local frame.
pub trait Surface: core::fmt::Debug + Send + Sync {
    /// Height of the surface above the vertex plane at radius `r` (mm), or `None` where
    /// the profile is not defined (beyond the edge of a closed conic).
    fn sag(&self, r: f64) -> Option<f64>;

    /// Derivative of the sag with respect to `r`, or `None` where it is not defined.
    fn slope(&self, r: f64) -> Option<f64>;

    /// Curvature at the vertex (1/mm), used by paraxial analysis.
    fn vertex_curvature(&self) -> f64;

    /// Unit normal at a point on the surface (only `x` and `y` are used), with
    /// non-negative z component. `None` where the profile is not defined.
    fn normal(&self, point: Vec3) -> Option<Vec3> {
        let r = point.x.hypot(point.y);
        if r == 0.0 {
            return Some(Vec3::Z);
        }
        let s = self.slope(r)?;
        Vec3::new(-s * point.x / r, -s * point.y / r, 1.0).normalized()
    }

    /// Where the line `origin + t · direction` meets the surface, or `None` if it does
    /// not. See the crate documentation for which hit is chosen.
    fn intersect(&self, origin: Vec3, direction: Vec3) -> Option<Hit>;
}

/// Sag of a conic, or `None` beyond the edge of a closed conic.
fn conic_sag(curvature: f64, conic: f64, r: f64) -> Option<f64> {
    let c = curvature;
    let root = 1.0 - (1.0 + conic) * c * c * r * r;
    (root >= 0.0).then(|| c * r * r / (1.0 + root.sqrt()))
}

/// Slope of a conic: dz/dr = c r / √(1 − (1+k) c² r²). Undefined at the edge, where it
/// is infinite.
fn conic_slope(curvature: f64, conic: f64, r: f64) -> Option<f64> {
    let c = curvature;
    let root = 1.0 - (1.0 + conic) * c * c * r * r;
    (root > 0.0).then(|| c * r / root.sqrt())
}

/// Closed-form intersection of a line with a conic of revolution through the origin.
///
/// The conic is the vertex sheet of `F = c(x² + y² + (1+k) z²) − 2z = 0`. Substituting
/// `p + t d` gives `A t² + B t + C = 0`. A root lies on the vertex sheet when
/// `1 − c(1+k) z ≥ 0` (the square root in the sag formula is non-negative there). Of the
/// roots on that sheet, the one nearest the vertex tangent plane is chosen.
fn conic_intersect(curvature: f64, conic: f64, p: Vec3, d: Vec3) -> Option<Hit> {
    let c = curvature;
    let q = 1.0 + conic;
    let a = c * (d.x * d.x + d.y * d.y + q * d.z * d.z);
    let b = 2.0 * (c * (p.x * d.x + p.y * d.y + q * p.z * d.z) - d.z);
    let constant = c * (p.x * p.x + p.y * p.y + q * p.z * p.z) - 2.0 * p.z;

    let mut roots = [f64::NAN; 2];
    let scale = b.abs().max(constant.abs()).max(f64::MIN_POSITIVE);
    if a.abs() <= 1e-15 * scale {
        // Linear: a plane, or a line parallel to the axis of a paraboloid.
        if b == 0.0 {
            return None;
        }
        roots[0] = -constant / b;
    } else {
        let discriminant = b * b - 4.0 * a * constant;
        if discriminant < 0.0 {
            return None;
        }
        // Numerically stable pair of roots.
        let h = -0.5 * (b + b.signum() * discriminant.sqrt());
        if h == 0.0 {
            roots[0] = 0.0;
        } else {
            roots = [h / a, constant / h];
        }
    }

    // Distance to the vertex tangent plane, the reference for choosing between roots.
    let plane = if d.z == 0.0 { 0.0 } else { -p.z / d.z };
    let tolerance = 1e-12 * (1.0 + p.length());
    roots
        .into_iter()
        .filter(|t| t.is_finite())
        .map(|t| (t, p + d * t))
        .filter(|(_, point)| 1.0 - c * q * point.z >= -tolerance * c.abs())
        .min_by(|(t1, _), (t2, _)| (t1 - plane).abs().total_cmp(&(t2 - plane).abs()))
        .and_then(|(t, point)| {
            // Normal from the gradient of F, oriented to +z: (−cx, −cy, 1 − c(1+k)z).
            let normal =
                Vec3::new(-c * point.x, -c * point.y, 1.0 - c * q * point.z).normalized()?;
            Some(Hit {
                distance: t,
                point,
                normal,
            })
        })
}

/// The plane z = 0.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Plane;

impl Surface for Plane {
    fn sag(&self, _r: f64) -> Option<f64> {
        Some(0.0)
    }

    fn slope(&self, _r: f64) -> Option<f64> {
        Some(0.0)
    }

    fn vertex_curvature(&self) -> f64 {
        0.0
    }

    fn intersect(&self, origin: Vec3, direction: Vec3) -> Option<Hit> {
        if direction.z == 0.0 {
            return None;
        }
        let t = -origin.z / direction.z;
        let point = origin + direction * t;
        Some(Hit {
            distance: t,
            point: Vec3::new(point.x, point.y, 0.0),
            normal: Vec3::Z,
        })
    }
}

/// A sphere through the vertex. Curvature 0 is a plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere {
    /// Curvature 1/R (1/mm).
    pub curvature: f64,
}

impl Sphere {
    /// A sphere from its radius of curvature in mm; an infinite radius gives a plane.
    pub fn from_radius(radius: f64) -> Self {
        Self {
            curvature: 1.0 / radius,
        }
    }
}

impl Surface for Sphere {
    fn sag(&self, r: f64) -> Option<f64> {
        conic_sag(self.curvature, 0.0, r)
    }

    fn slope(&self, r: f64) -> Option<f64> {
        conic_slope(self.curvature, 0.0, r)
    }

    fn vertex_curvature(&self) -> f64 {
        self.curvature
    }

    fn intersect(&self, origin: Vec3, direction: Vec3) -> Option<Hit> {
        conic_intersect(self.curvature, 0.0, origin, direction)
    }
}

/// A conic of revolution through the vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Conic {
    /// Curvature 1/R at the vertex (1/mm).
    pub curvature: f64,
    /// Conic constant k (see the crate documentation).
    pub conic: f64,
}

impl Surface for Conic {
    fn sag(&self, r: f64) -> Option<f64> {
        conic_sag(self.curvature, self.conic, r)
    }

    fn slope(&self, r: f64) -> Option<f64> {
        conic_slope(self.curvature, self.conic, r)
    }

    fn vertex_curvature(&self) -> f64 {
        self.curvature
    }

    fn intersect(&self, origin: Vec3, direction: Vec3) -> Option<Hit> {
        conic_intersect(self.curvature, self.conic, origin, direction)
    }
}

/// An even asphere: a base conic plus polynomial terms a₀ r⁴ + a₁ r⁶ + a₂ r⁸ + …
#[derive(Debug, Clone, PartialEq)]
pub struct EvenAsphere {
    /// Curvature 1/R at the vertex (1/mm).
    pub curvature: f64,
    /// Conic constant k of the base conic.
    pub conic: f64,
    /// Coefficients of r⁴, r⁶, r⁸, … in mm^(1−n) for the rⁿ term.
    pub coefficients: Vec<f64>,
}

/// Newton iteration limits for asphere intersection. The start (the base conic hit) is
/// close, so a handful of iterations is normal; the limit only stops runaway cases.
const ASPHERE_TOLERANCE: f64 = 1e-13;
const ASPHERE_MAX_ITERATIONS: u32 = 50;

impl EvenAsphere {
    fn polynomial(&self, r: f64) -> (f64, f64) {
        // Σ aᵢ r^(2i+4) and its derivative Σ (2i+4) aᵢ r^(2i+3), by Horner in r².
        let r2 = r * r;
        let mut value = 0.0;
        let mut derivative = 0.0;
        for (i, &a) in self.coefficients.iter().enumerate().rev() {
            value = value * r2 + a;
            derivative = derivative * r2 + a * (2 * i + 4) as f64;
        }
        (value * r2 * r2, derivative * r2 * r)
    }
}

impl Surface for EvenAsphere {
    fn sag(&self, r: f64) -> Option<f64> {
        Some(conic_sag(self.curvature, self.conic, r)? + self.polynomial(r).0)
    }

    fn slope(&self, r: f64) -> Option<f64> {
        Some(conic_slope(self.curvature, self.conic, r)? + self.polynomial(r).1)
    }

    fn vertex_curvature(&self) -> f64 {
        self.curvature
    }

    /// Newton's method on `g(t) = z(t) − sag(r(t))`, starting from the base conic hit
    /// (or the vertex plane if the conic is missed).
    fn intersect(&self, origin: Vec3, direction: Vec3) -> Option<Hit> {
        let start = conic_intersect(self.curvature, self.conic, origin, direction)
            .map(|hit| hit.distance)
            .or_else(|| (direction.z != 0.0).then(|| -origin.z / direction.z))?;
        let g = |t: f64| {
            let p = origin + direction * t;
            let r = p.x.hypot(p.y);
            match (self.sag(r), self.slope(r)) {
                (Some(sag), Some(slope)) => {
                    let radial = if r == 0.0 {
                        0.0
                    } else {
                        (p.x * direction.x + p.y * direction.y) / r
                    };
                    (p.z - sag, direction.z - slope * radial)
                }
                _ => (f64::NAN, f64::NAN),
            }
        };
        let root = find_root(g, start, ASPHERE_TOLERANCE, ASPHERE_MAX_ITERATIONS).ok()?;
        let point = origin + direction * root.x;
        Some(Hit {
            distance: root.x,
            point,
            normal: self.normal(point)?,
        })
    }
}

/// The clear aperture of a surface: the part of it light may pass.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Aperture {
    /// No limit.
    #[default]
    Unlimited,
    /// A centred circle; hits with r ≤ semi-diameter pass (see [`Aperture::contains`]).
    Circular {
        /// Radius of the clear aperture (mm).
        semi_diameter: f64,
    },
}

/// What happened when a ray reached a surface with an aperture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Intersection {
    /// The ray hit the surface inside the clear aperture.
    Hit(Hit),
    /// The ray met the surface's profile, but outside the clear aperture: it is blocked
    /// by the edge or mount.
    Vignetted(Hit),
    /// The ray does not meet the surface's profile at all.
    Miss,
}

impl Aperture {
    /// Whether a point (local frame) lies within the clear aperture. A ray aimed at the
    /// edge lands there only to rounding, so the edge has a tolerance of
    /// [`Aperture::EDGE_TOLERANCE`].
    pub fn contains(&self, point: Vec3) -> bool {
        match *self {
            Aperture::Unlimited => true,
            Aperture::Circular { semi_diameter } => {
                point.x.hypot(point.y) <= semi_diameter + Self::EDGE_TOLERANCE
            }
        }
    }

    /// How far past the edge a hit still counts as inside (mm): far below any
    /// mechanical tolerance, far above rounding.
    pub const EDGE_TOLERANCE: f64 = 1e-9;

    /// Intersects `surface` and classifies the result against this aperture.
    pub fn intersect(&self, surface: &dyn Surface, origin: Vec3, direction: Vec3) -> Intersection {
        match surface.intersect(origin, direction) {
            None => Intersection::Miss,
            Some(hit) if self.contains(hit.point) => Intersection::Hit(hit),
            Some(hit) => Intersection::Vignetted(hit),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-12;

    fn on_surface(surface: &dyn Surface, hit: &Hit) -> bool {
        let r = hit.point.x.hypot(hit.point.y);
        surface
            .sag(r)
            .is_some_and(|sag| (sag - hit.point.z).abs() < EPS)
    }

    #[test]
    fn plane_hit_and_parallel_miss() {
        let hit = Plane
            .intersect(Vec3::new(1.0, 2.0, -5.0), Vec3::Z)
            .expect("hits");
        assert!((hit.distance - 5.0).abs() < EPS);
        assert!((hit.point - Vec3::new(1.0, 2.0, 0.0)).length() < EPS);
        assert!(
            Plane
                .intersect(Vec3::new(0.0, 0.0, -5.0), Vec3::X)
                .is_none()
        );
    }

    #[test]
    fn sphere_hit_matches_geometry() {
        // A convex sphere R = 50 mm (centre at z = +50) hit at height h = 10 mm along +z.
        // Geometry: z = R − √(R² − h²); normal (−h/R, 0, √(R² − h²)/R).
        let (radius, h) = (50.0_f64, 10.0_f64);
        let sphere = Sphere::from_radius(radius);
        let hit = sphere
            .intersect(Vec3::new(h, 0.0, -20.0), Vec3::Z)
            .expect("hits");
        let z = radius - (radius * radius - h * h).sqrt();
        assert!((hit.point.z - z).abs() < EPS);
        assert!((hit.distance - (z + 20.0)).abs() < EPS);
        let normal = Vec3::new(-h / radius, 0.0, (radius * radius - h * h).sqrt() / radius);
        assert!((hit.normal - normal).length() < EPS);
        assert!(on_surface(&sphere, &hit));
    }

    #[test]
    fn sphere_takes_the_vertex_branch_for_both_signs_of_curvature() {
        // A line along z at height h meets the full sphere twice: z = R ∓ √(R² − h²).
        // Only the root on the vertex side (|z| < |R|) belongs to the surface.
        for radius in [50.0, -50.0] {
            let sphere = Sphere::from_radius(radius);
            for start in [-100.0, 0.0, 100.0] {
                let hit = sphere
                    .intersect(Vec3::new(30.0, 0.0, start), Vec3::Z)
                    .expect("hits");
                assert!(
                    hit.point.z.abs() < radius.abs(),
                    "R = {radius}, start = {start}"
                );
                assert!(on_surface(&sphere, &hit));
                // Also reached travelling the other way.
                let back = sphere
                    .intersect(Vec3::new(30.0, 0.0, start), -Vec3::Z)
                    .expect("hits");
                assert!((back.point - hit.point).length() < EPS);
            }
        }
    }

    #[test]
    fn sphere_missed_beyond_its_radius() {
        let sphere = Sphere::from_radius(10.0);
        assert!(
            sphere
                .intersect(Vec3::new(10.5, 0.0, -5.0), Vec3::Z)
                .is_none()
        );
    }

    #[test]
    fn hits_behind_the_origin_have_negative_distance() {
        let sphere = Sphere::from_radius(-40.0);
        let hit = sphere
            .intersect(Vec3::new(5.0, 0.0, 10.0), Vec3::Z)
            .expect("hits");
        assert!(hit.distance < 0.0);
        assert!(on_surface(&sphere, &hit));
    }

    #[test]
    fn paraboloid_sag_is_quadratic_and_tilted_rays_hit_it() {
        // k = −1: sag = c r² / 2 exactly.
        let parabola = Conic {
            curvature: -1.0 / 200.0,
            conic: -1.0,
        };
        for r in [0.0, 1.0, 25.0, 300.0] {
            let sag = parabola.sag(r).expect("defined everywhere");
            assert!((sag - (-r * r / 400.0)).abs() < EPS * (1.0 + r * r));
        }
        let direction = Vec3::new(0.1, -0.05, 1.0).normalized().expect("non-zero");
        let hit = parabola
            .intersect(Vec3::new(3.0, 7.0, -50.0), direction)
            .expect("hits");
        assert!(on_surface(&parabola, &hit));
    }

    #[test]
    fn ellipsoid_and_hyperboloid_hits_lie_on_the_surface() {
        let direction = Vec3::new(0.02, 0.03, 1.0).normalized().expect("non-zero");
        for conic in [-0.5, 0.7, -2.5] {
            for curvature in [1.0 / 30.0, -1.0 / 30.0] {
                let surface = Conic { curvature, conic };
                let hit = surface
                    .intersect(Vec3::new(4.0, -3.0, -10.0), direction)
                    .expect("hits");
                assert!(on_surface(&surface, &hit), "k = {conic}, c = {curvature}");
            }
        }
    }

    #[test]
    fn normal_from_slope_matches_finite_differences() {
        let asphere = EvenAsphere {
            curvature: 1.0 / 25.0,
            conic: -0.6,
            coefficients: vec![2e-6, -3e-9],
        };
        let r = 6.0;
        let step = 1e-6;
        let numeric = (asphere.sag(r + step).expect("defined")
            - asphere.sag(r - step).expect("defined"))
            / (2.0 * step);
        let slope = asphere.slope(r).expect("defined");
        assert!((numeric - slope).abs() < 1e-8);
        let normal = asphere.normal(Vec3::new(r, 0.0, 0.0)).expect("defined");
        assert!((normal.x / normal.z + slope).abs() < 1e-12);
    }

    #[test]
    fn asphere_hit_lies_on_the_surface_and_reduces_to_the_conic() {
        let asphere = EvenAsphere {
            curvature: 1.0 / 25.0,
            conic: -0.6,
            coefficients: vec![2e-5, -3e-8, 1e-11],
        };
        let direction = Vec3::new(0.05, 0.02, 1.0).normalized().expect("non-zero");
        let origin = Vec3::new(5.0, 3.0, -10.0);
        let hit = asphere.intersect(origin, direction).expect("hits");
        assert!(on_surface(&asphere, &hit));
        assert!(hit.normal.z > 0.0 && (hit.normal.length() - 1.0).abs() < EPS);

        let bare = EvenAsphere {
            coefficients: vec![],
            ..asphere
        };
        let conic = Conic {
            curvature: bare.curvature,
            conic: bare.conic,
        };
        let a = bare.intersect(origin, direction).expect("hits");
        let b = conic.intersect(origin, direction).expect("hits");
        assert!((a.point - b.point).length() < EPS);
        assert!((a.normal - b.normal).length() < EPS);
    }

    #[test]
    fn aperture_separates_hit_vignetted_and_miss() {
        let sphere = Sphere::from_radius(20.0);
        let aperture = Aperture::Circular { semi_diameter: 8.0 };
        let classify = |h: f64| aperture.intersect(&sphere, Vec3::new(h, 0.0, -5.0), Vec3::Z);
        assert!(matches!(classify(7.9), Intersection::Hit(_)));
        assert!(matches!(classify(8.0), Intersection::Hit(_)));
        assert!(matches!(classify(8.1), Intersection::Vignetted(_)));
        assert!(matches!(classify(25.0), Intersection::Miss));
        assert!(matches!(
            Aperture::Unlimited.intersect(&sphere, Vec3::new(19.0, 0.0, -5.0), Vec3::Z),
            Intersection::Hit(_)
        ));
    }
}
