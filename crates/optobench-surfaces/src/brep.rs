//! Exact faces of boundary-represented solids (from STEP): surfaces, curves, trimming and
//! ray intersection with the residual of every hit.
//!
//! - Analytic surfaces (plane, cylinder, cone, sphere) intersect in closed form; a torus
//!   by bracketing sign changes of its implicit function along the ray and refining with
//!   safeguarded Newton; B-spline surfaces, surfaces of revolution and of linear extrusion
//!   by Newton's method on (u, v, t) from a grid of seeds over the face's parameter range.
//! - B-splines are evaluated with their basis functions and derivatives (the Cox–de Boor
//!   recursion; Piegl & Tiller, The NURBS Book, 2nd ed., ch. 2–4), rational ones in
//!   homogeneous coordinates. Written from the book's equations.
//! - Trimming is decided in 3D: a point is on the face if it lies to the left of the
//!   nearest boundary segment, seen from the side the face normal points to. Boundary
//!   loops are polylines oriented that way (ISO 10303-42 face bounds), sampled from the
//!   exact edges to a chord tolerance chosen by the caller.
//! - Every hit carries its residual: its distance from the exact surface (closed form
//!   for analytic surfaces, |S(u, v) − p| for parametric ones).

use std::f64::consts::TAU;

use optobench_math::{Frame, Rotation, Vec3, find_root_bracketed};

/// An axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Lower corner.
    pub min: Vec3,
    /// Upper corner.
    pub max: Vec3,
}

impl Aabb {
    /// A box containing nothing.
    pub const EMPTY: Self = Self {
        min: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        max: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    };

    /// The box grown to contain `p`.
    pub fn with(self, p: Vec3) -> Self {
        Self {
            min: Vec3::new(
                self.min.x.min(p.x),
                self.min.y.min(p.y),
                self.min.z.min(p.z),
            ),
            max: Vec3::new(
                self.max.x.max(p.x),
                self.max.y.max(p.y),
                self.max.z.max(p.z),
            ),
        }
    }

    /// The smallest box containing both.
    pub fn union(self, other: Self) -> Self {
        self.with(other.min).with(other.max)
    }

    /// The box grown by `margin` on every side.
    pub fn padded(self, margin: f64) -> Self {
        let m = Vec3::new(margin, margin, margin);
        Self {
            min: self.min - m,
            max: self.max + m,
        }
    }

    /// Whether the box contains anything.
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }

    /// Whether `p` lies in the box (boundary included).
    pub fn contains(&self, p: Vec3) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }

    /// The centre.
    pub fn centre(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Length of the diagonal.
    pub fn size(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            (self.max - self.min).length()
        }
    }

    /// The 8 corners.
    pub fn corners(&self) -> [Vec3; 8] {
        let (a, b) = (self.min, self.max);
        [
            Vec3::new(a.x, a.y, a.z),
            Vec3::new(b.x, a.y, a.z),
            Vec3::new(a.x, b.y, a.z),
            Vec3::new(b.x, b.y, a.z),
            Vec3::new(a.x, a.y, b.z),
            Vec3::new(b.x, a.y, b.z),
            Vec3::new(a.x, b.y, b.z),
            Vec3::new(b.x, b.y, b.z),
        ]
    }

    /// Where the line `origin + t·direction` is inside the box: (t_enter, t_exit), or
    /// `None` if it misses.
    pub fn ray_interval(&self, origin: Vec3, direction: Vec3) -> Option<(f64, f64)> {
        let mut low = f64::NEG_INFINITY;
        let mut high = f64::INFINITY;
        for (o, d, lo, hi) in [
            (origin.x, direction.x, self.min.x, self.max.x),
            (origin.y, direction.y, self.min.y, self.max.y),
            (origin.z, direction.z, self.min.z, self.max.z),
        ] {
            if d == 0.0 {
                if o < lo || o > hi {
                    return None;
                }
            } else {
                let (a, b) = ((lo - o) / d, (hi - o) / d);
                low = low.max(a.min(b));
                high = high.min(a.max(b));
            }
        }
        (low <= high).then_some((low, high))
    }
}

// ---------------------------------------------------------------- B-spline basis

/// The knot span index i with knots[i] ≤ t < knots[i+1], clamped to the valid range.
fn span(knots: &[f64], degree: usize, count: usize, t: f64) -> usize {
    // count = number of control points; valid spans are degree..count-1.
    if t >= knots[count] {
        return count - 1;
    }
    if t <= knots[degree] {
        return degree;
    }
    let (mut low, mut high) = (degree, count);
    while high - low > 1 {
        let mid = (low + high) / 2;
        if t < knots[mid] {
            high = mid;
        } else {
            low = mid;
        }
    }
    low
}

/// The highest B-spline degree supported (CAD data rarely exceeds 9).
pub const MAX_DEGREE: usize = 15;
const BASIS: usize = MAX_DEGREE + 1;

/// Non-zero basis functions N[span−degree ..= span] at t, and their first derivatives,
/// in fixed-size arrays (entries 0..=degree are used), so evaluation does not allocate.
fn basis(knots: &[f64], degree: usize, span: usize, t: f64) -> ([f64; BASIS], [f64; BASIS]) {
    // Triangular table of basis functions of degree 0..=p (The NURBS Book, A2.2/A2.3).
    let p = degree.min(MAX_DEGREE);
    let mut n = [[0.0; BASIS]; BASIS];
    n[0][0] = 1.0;
    let mut left = [0.0; BASIS];
    let mut right = [0.0; BASIS];
    for j in 1..=p {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            let denominator = right[r + 1] + left[j - r];
            let temp = if denominator == 0.0 {
                0.0
            } else {
                n[r][j - 1] / denominator
            };
            n[r][j] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j][j] = saved;
    }
    let mut values = [0.0; BASIS];
    for r in 0..=p {
        values[r] = n[r][p];
    }
    // First derivative: N'ᵢ,ₚ = p (Nᵢ,ₚ₋₁/(uᵢ₊ₚ − uᵢ) − Nᵢ₊₁,ₚ₋₁/(uᵢ₊ₚ₊₁ − uᵢ₊₁)).
    let mut derivatives = [0.0; BASIS];
    if p > 0 {
        for (r, derivative) in derivatives.iter_mut().enumerate().take(p + 1) {
            let i = span - p + r;
            let lower = if r > 0 { n[r - 1][p - 1] } else { 0.0 };
            let upper = if r < p { n[r][p - 1] } else { 0.0 };
            let a = knots[i + p] - knots[i];
            let b = knots[i + p + 1] - knots[i + 1];
            let term_a = if a == 0.0 { 0.0 } else { lower / a };
            let term_b = if b == 0.0 { 0.0 } else { upper / b };
            *derivative = p as f64 * (term_a - term_b);
        }
    }
    (values, derivatives)
}

/// A (possibly rational) B-spline curve, knots expanded.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplineCurve {
    /// Degree.
    pub degree: usize,
    /// Control points.
    pub points: Vec<Vec3>,
    /// Weights for a rational curve.
    pub weights: Option<Vec<f64>>,
    /// Knot vector (length = points + degree + 1).
    pub knots: Vec<f64>,
}

impl BSplineCurve {
    /// The parameter range.
    pub fn domain(&self) -> (f64, f64) {
        (self.knots[self.degree], self.knots[self.points.len()])
    }

    /// Point and first derivative at t.
    pub fn evaluate(&self, t: f64) -> (Vec3, Vec3) {
        let (low, high) = self.domain();
        let t = t.clamp(low, high);
        let k = span(&self.knots, self.degree, self.points.len(), t);
        let (n, dn) = basis(&self.knots, self.degree, k, t);
        let mut a = Vec3::ZERO;
        let mut da = Vec3::ZERO;
        let (mut w, mut dw) = (0.0, 0.0);
        for r in 0..=self.degree {
            let i = k - self.degree + r;
            let weight = self.weights.as_ref().map_or(1.0, |ws| ws[i]);
            a = a + self.points[i] * (n[r] * weight);
            da = da + self.points[i] * (dn[r] * weight);
            w += n[r] * weight;
            dw += dn[r] * weight;
        }
        let point = a * (1.0 / w);
        (point, (da - point * dw) * (1.0 / w))
    }
}

/// A (possibly rational) B-spline surface, knots expanded.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplineSurface {
    /// Degree in u.
    pub u_degree: usize,
    /// Degree in v.
    pub v_degree: usize,
    /// Control points, rows along u, each row along v.
    pub points: Vec<Vec<Vec3>>,
    /// Weights in the same layout, for a rational surface.
    pub weights: Option<Vec<Vec<f64>>>,
    /// Knots in u.
    pub u_knots: Vec<f64>,
    /// Knots in v.
    pub v_knots: Vec<f64>,
}

impl BSplineSurface {
    /// Parameter ranges ((u₀, u₁), (v₀, v₁)).
    pub fn domain(&self) -> ((f64, f64), (f64, f64)) {
        let nu = self.points.len();
        let nv = self.points[0].len();
        (
            (self.u_knots[self.u_degree], self.u_knots[nu]),
            (self.v_knots[self.v_degree], self.v_knots[nv]),
        )
    }

    /// Point and partial derivatives (S, ∂S/∂u, ∂S/∂v) at (u, v).
    pub fn evaluate(&self, u: f64, v: f64) -> (Vec3, Vec3, Vec3) {
        let ((u0, u1), (v0, v1)) = self.domain();
        let (u, v) = (u.clamp(u0, u1), v.clamp(v0, v1));
        let (nu, nv) = (self.points.len(), self.points[0].len());
        let ku = span(&self.u_knots, self.u_degree, nu, u);
        let kv = span(&self.v_knots, self.v_degree, nv, v);
        let (bu, dbu) = basis(&self.u_knots, self.u_degree, ku, u);
        let (bv, dbv) = basis(&self.v_knots, self.v_degree, kv, v);
        let (mut a, mut au, mut av) = (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);
        let (mut w, mut wu, mut wv) = (0.0, 0.0, 0.0);
        for (r, (nr, dnr)) in bu.iter().zip(&dbu).enumerate().take(self.u_degree + 1) {
            let i = ku - self.u_degree + r;
            for (s, (ns, dns)) in bv.iter().zip(&dbv).enumerate().take(self.v_degree + 1) {
                let j = kv - self.v_degree + s;
                let weight = self.weights.as_ref().map_or(1.0, |ws| ws[i][j]);
                let p = self.points[i][j];
                a = a + p * (nr * ns * weight);
                au = au + p * (dnr * ns * weight);
                av = av + p * (nr * dns * weight);
                w += nr * ns * weight;
                wu += dnr * ns * weight;
                wv += nr * dns * weight;
            }
        }
        let point = a * (1.0 / w);
        (
            point,
            (au - point * wu) * (1.0 / w),
            (av - point * wv) * (1.0 / w),
        )
    }
}

// ---------------------------------------------------------------- curves

/// An edge or profile curve.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    /// Through `point` along unit `direction`; parameter = distance from the point.
    Line {
        /// A point on it.
        point: Vec3,
        /// Unit direction.
        direction: Vec3,
    },
    /// In the frame's x–y plane; parameter = angle from local x.
    Circle {
        /// Centre frame.
        frame: Frame,
        /// Radius.
        radius: f64,
    },
    /// In the frame's x–y plane; parameter = angle.
    Ellipse {
        /// Centre frame.
        frame: Frame,
        /// Semi-axis along local x.
        semi_axis_1: f64,
        /// Semi-axis along local y.
        semi_axis_2: f64,
    },
    /// A B-spline.
    BSpline(BSplineCurve),
}

impl Curve {
    /// Point and derivative at parameter s.
    pub fn evaluate(&self, s: f64) -> (Vec3, Vec3) {
        match self {
            Curve::Line { point, direction } => (*point + *direction * s, *direction),
            Curve::Circle { frame, radius } => {
                let [x, y, _] = frame.axes();
                let (sin, cos) = s.sin_cos();
                (
                    frame.origin() + (x * cos + y * sin) * *radius,
                    (y * cos - x * sin) * *radius,
                )
            }
            Curve::Ellipse {
                frame,
                semi_axis_1,
                semi_axis_2,
            } => {
                let [x, y, _] = frame.axes();
                let (sin, cos) = s.sin_cos();
                (
                    frame.origin() + x * (semi_axis_1 * cos) + y * (semi_axis_2 * sin),
                    y * (semi_axis_2 * cos) - x * (semi_axis_1 * sin),
                )
            }
            Curve::BSpline(b) => b.evaluate(s),
        }
    }

    /// The bounded parameter range, if the curve has one, and whether it is periodic.
    pub fn domain(&self) -> Option<((f64, f64), bool)> {
        match self {
            Curve::Line { .. } => None,
            Curve::Circle { .. } | Curve::Ellipse { .. } => Some(((0.0, TAU), true)),
            Curve::BSpline(b) => {
                let (low, high) = b.domain();
                let closed = (b.evaluate(low).0 - b.evaluate(high).0).length() < 1e-9;
                Some(((low, high), closed))
            }
        }
    }

    /// The parameter of the point on the curve nearest `p` (exact for lines, circles and
    /// ellipse angles; sampling then Newton for B-splines).
    pub fn parameter_of(&self, p: Vec3) -> f64 {
        match self {
            Curve::Line { point, direction } => (p - *point).dot(*direction),
            Curve::Circle { frame, .. } => {
                let q = frame.to_local_point(p);
                q.y.atan2(q.x).rem_euclid(TAU)
            }
            Curve::Ellipse {
                frame,
                semi_axis_1,
                semi_axis_2,
            } => {
                let q = frame.to_local_point(p);
                (q.y / semi_axis_2).atan2(q.x / semi_axis_1).rem_euclid(TAU)
            }
            Curve::BSpline(b) => {
                let (low, high) = b.domain();
                let samples = 8 * b.points.len().max(4);
                let distance = |s: f64| (b.evaluate(s).0 - p).length_squared();
                let mut best = low;
                for k in 0..=samples {
                    let s = low + (high - low) * k as f64 / samples as f64;
                    if distance(s) < distance(best) {
                        best = s;
                    }
                }
                // Newton on (C(s) − p)·C'(s) = 0, with a numerical second derivative.
                let mut s = best;
                for _ in 0..30 {
                    let h = 1e-7 * (high - low).max(1e-9);
                    let g = |s: f64| {
                        let (c, d) = b.evaluate(s);
                        (c - p).dot(d)
                    };
                    let slope = (g(s + h) - g(s - h)) / (2.0 * h);
                    if slope == 0.0 {
                        break;
                    }
                    let next = (s - g(s) / slope).clamp(low, high);
                    if (next - s).abs() < 1e-14 * (1.0 + s.abs()) {
                        s = next;
                        break;
                    }
                    s = next;
                }
                if distance(s) <= distance(best) {
                    s
                } else {
                    best
                }
            }
        }
    }

    /// Points from `start` to `end` along the curve, forward or against its parameter
    /// direction, with chord deviation at most `tolerance` (a closed edge, start = end,
    /// goes all the way round).
    pub fn sample(&self, start: Vec3, end: Vec3, forward: bool, tolerance: f64) -> Vec<Vec3> {
        if let Curve::Line { .. } = self {
            return vec![start, end];
        }
        let s0 = self.parameter_of(start);
        let mut s1 = self.parameter_of(end);
        let periodic = self.domain();
        let closed_edge = (start - end).length() < 1e-9;
        if let Some(((low, high), true)) = periodic {
            let period = high - low;
            if forward {
                while s1 <= s0 + 1e-12 && (closed_edge || s1 < s0) {
                    s1 += period;
                }
            } else {
                while s1 >= s0 - 1e-12 && (closed_edge || s1 > s0) {
                    s1 -= period;
                }
            }
        }
        // Step count from the curvature bound: a chord over angle θ of a circle of
        // radius r deviates r(1 − cos θ/2); sample densely enough everywhere.
        let length_estimate = {
            let n = 64;
            (0..n)
                .map(|k| {
                    let a = s0 + (s1 - s0) * k as f64 / n as f64;
                    let b = s0 + (s1 - s0) * (k + 1) as f64 / n as f64;
                    (self.evaluate(b).0 - self.evaluate(a).0).length()
                })
                .sum::<f64>()
        };
        let curvature_radius = match self {
            Curve::Circle { radius, .. } => *radius,
            Curve::Ellipse {
                semi_axis_1,
                semi_axis_2,
                ..
            } => semi_axis_1.min(*semi_axis_2).powi(2) / semi_axis_1.max(*semi_axis_2),
            _ => (length_estimate / 8.0).max(1e-6),
        };
        let step = (8.0 * curvature_radius * tolerance).sqrt().max(1e-9);
        let count = ((length_estimate / step).ceil() as usize).clamp(4, 20_000);
        let mut points: Vec<Vec3> = (0..=count)
            .map(|k| self.evaluate(s0 + (s1 - s0) * k as f64 / count as f64).0)
            .collect();
        // End exactly on the vertices.
        points[0] = start;
        if let Some(last) = points.last_mut() {
            *last = end;
        }
        points
    }
}

// ---------------------------------------------------------------- surfaces

/// The surface a face lies on.
#[derive(Debug, Clone, PartialEq)]
pub enum FaceSurface {
    /// The frame's x–y plane; normal +z.
    Plane(Frame),
    /// Radius about the frame's z axis; normal outward.
    Cylinder {
        /// Axis frame.
        frame: Frame,
        /// Radius.
        radius: f64,
    },
    /// ρ = radius + z·tan(semi_angle); normal away from the axis.
    Cone {
        /// Axis frame.
        frame: Frame,
        /// Radius at z = 0.
        radius: f64,
        /// Half the apex angle (radians).
        semi_angle: f64,
    },
    /// Centred on the frame origin; normal outward.
    Sphere {
        /// Centre frame.
        frame: Frame,
        /// Radius.
        radius: f64,
    },
    /// About the frame's z axis; normal outward from the tube.
    Torus {
        /// Axis frame.
        frame: Frame,
        /// Axis to tube centre.
        major: f64,
        /// Tube radius.
        minor: f64,
    },
    /// S(u, v); normal ∂S/∂u × ∂S/∂v.
    BSpline(BSplineSurface),
    /// The profile C(v) turned by angle u about the axis; normal ∂S/∂u × ∂S/∂v.
    Revolution {
        /// The profile.
        profile: Curve,
        /// A point on the axis.
        axis_point: Vec3,
        /// Unit axis direction.
        axis_direction: Vec3,
    },
    /// S(u, v) = C(u) + v·direction; normal ∂S/∂u × ∂S/∂v.
    Extrusion {
        /// The profile.
        profile: Curve,
        /// Sweep vector.
        direction: Vec3,
    },
}

/// A ray–face intersection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceHit {
    /// Distance along the (unit) ray direction.
    pub distance: f64,
    /// The hit point.
    pub point: Vec3,
    /// Unit face normal (the surface normal, reversed for a reversed face).
    pub normal: Vec3,
    /// Distance of the point from the exact surface (mm).
    pub residual: f64,
}

/// Parameter ranges of a parametric face, for seeding Newton's method.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Domain {
    u: (f64, f64),
    v: (f64, f64),
    u_periodic: bool,
}

/// A trimmed face.
#[derive(Debug, Clone, PartialEq)]
pub struct BrepFace {
    /// The surface.
    pub surface: FaceSurface,
    /// Whether the face normal is opposite to the surface normal.
    pub reversed: bool,
    /// Boundary loops: closed polylines (last point joins the first), the face on their
    /// left seen along the face normal. Empty for a face covering its whole surface.
    pub loops: Vec<Vec<Vec3>>,
    bounds: Aabb,
    domain: Option<Domain>,
    segments: crate::segments::Segments,
    plane_grid: Option<crate::segments::PlaneGrid>,
}

const NEWTON_TOLERANCE: f64 = 1e-12;
const SEEDS: usize = 7;

fn rotation(axis: Vec3, angle: f64) -> Rotation {
    Rotation::from_axis_angle(axis, angle).unwrap_or(Rotation::IDENTITY)
}

impl FaceSurface {
    /// Point and partial derivatives of a parametric surface at (u, v).
    fn parametric(&self, u: f64, v: f64) -> Option<(Vec3, Vec3, Vec3)> {
        match self {
            FaceSurface::BSpline(b) => Some(b.evaluate(u, v)),
            FaceSurface::Revolution {
                profile,
                axis_point,
                axis_direction,
            } => {
                let (c, dc) = profile.evaluate(v);
                let r = rotation(*axis_direction, u);
                let p = *axis_point + r.apply(c - *axis_point);
                Some((p, axis_direction.cross(p - *axis_point), r.apply(dc)))
            }
            FaceSurface::Extrusion { profile, direction } => {
                let (c, dc) = profile.evaluate(u);
                Some((c + *direction * v, dc, *direction))
            }
            _ => None,
        }
    }

    /// Surface normal at a point on an analytic surface (not normalised to the face).
    fn analytic_normal(&self, p: Vec3) -> Option<Vec3> {
        match self {
            FaceSurface::Plane(frame) => Some(frame.axes()[2]),
            FaceSurface::Cylinder { frame, .. } => {
                let q = frame.to_local_point(p);
                frame
                    .to_parent_vector(Vec3::new(q.x, q.y, 0.0))
                    .normalized()
            }
            FaceSurface::Cone {
                frame, semi_angle, ..
            } => {
                let q = frame.to_local_point(p);
                let rho = q.x.hypot(q.y);
                if rho == 0.0 {
                    return Some(frame.axes()[2]);
                }
                let radial = Vec3::new(q.x / rho, q.y / rho, 0.0);
                let (s, c) = semi_angle.sin_cos();
                frame
                    .to_parent_vector(radial * c - Vec3::Z * s)
                    .normalized()
            }
            FaceSurface::Sphere { frame, .. } => (p - frame.origin()).normalized(),
            FaceSurface::Torus { frame, major, .. } => {
                let q = frame.to_local_point(p);
                let rho = q.x.hypot(q.y);
                let centre = if rho == 0.0 {
                    Vec3::ZERO
                } else {
                    Vec3::new(q.x / rho * major, q.y / rho * major, 0.0)
                };
                frame.to_parent_vector(q - centre).normalized()
            }
            _ => None,
        }
    }

    /// Distance of p from an analytic surface.
    fn analytic_residual(&self, p: Vec3) -> f64 {
        match self {
            FaceSurface::Plane(frame) => frame.to_local_point(p).z.abs(),
            FaceSurface::Cylinder { frame, radius } => {
                let q = frame.to_local_point(p);
                (q.x.hypot(q.y) - radius).abs()
            }
            FaceSurface::Cone {
                frame,
                radius,
                semi_angle,
            } => {
                let q = frame.to_local_point(p);
                ((q.x.hypot(q.y) - (radius + q.z * semi_angle.tan())) * semi_angle.cos()).abs()
            }
            FaceSurface::Sphere { frame, radius } => ((p - frame.origin()).length() - radius).abs(),
            FaceSurface::Torus {
                frame,
                major,
                minor,
            } => {
                let q = frame.to_local_point(p);
                ((q.x.hypot(q.y) - major).hypot(q.z) - minor).abs()
            }
            _ => f64::INFINITY,
        }
    }

    /// Closed-form roots t of the line on an analytic surface (no torus).
    fn closed_form(&self, origin: Vec3, direction: Vec3) -> Option<Vec<f64>> {
        let quadratic = |a: f64, b: f64, c: f64| -> Vec<f64> {
            if a.abs() < 1e-300 {
                return if b == 0.0 { vec![] } else { vec![-c / b] };
            }
            let discriminant = b * b - 4.0 * a * c;
            if discriminant < 0.0 {
                return vec![];
            }
            let q = -0.5 * (b + b.signum() * discriminant.sqrt());
            if q == 0.0 {
                return vec![0.0];
            }
            vec![q / a, c / q]
        };
        match self {
            FaceSurface::Plane(frame) => {
                let n = frame.axes()[2];
                let d = direction.dot(n);
                Some(if d == 0.0 {
                    vec![]
                } else {
                    vec![(frame.origin() - origin).dot(n) / d]
                })
            }
            FaceSurface::Cylinder { frame, radius } => {
                let p = frame.to_local_point(origin);
                let d = frame.to_local_vector(direction);
                Some(quadratic(
                    d.x * d.x + d.y * d.y,
                    2.0 * (p.x * d.x + p.y * d.y),
                    p.x * p.x + p.y * p.y - radius * radius,
                ))
            }
            FaceSurface::Cone {
                frame,
                radius,
                semi_angle,
            } => {
                // x² + y² = (r + z·k)², k = tan α, on the nappe where r + z·k ≥ 0.
                let p = frame.to_local_point(origin);
                let d = frame.to_local_vector(direction);
                let k = semi_angle.tan();
                let (e, f) = (radius + p.z * k, d.z * k);
                let roots = quadratic(
                    d.x * d.x + d.y * d.y - f * f,
                    2.0 * (p.x * d.x + p.y * d.y - e * f),
                    p.x * p.x + p.y * p.y - e * e,
                );
                Some(roots.into_iter().filter(|t| e + f * t >= -1e-9).collect())
            }
            FaceSurface::Sphere { frame, radius } => {
                let p = origin - frame.origin();
                Some(quadratic(
                    direction.length_squared(),
                    2.0 * p.dot(direction),
                    p.length_squared() - radius * radius,
                ))
            }
            _ => None,
        }
    }

    /// Roots of the torus F(t) = (|q|² + R² − r²)² − 4R²(qx² + qy²) on [t0, t1].
    fn torus_roots(
        frame: &Frame,
        major: f64,
        minor: f64,
        origin: Vec3,
        direction: Vec3,
        t0: f64,
        t1: f64,
    ) -> Vec<f64> {
        let p = frame.to_local_point(origin);
        let d = frame.to_local_vector(direction);
        let f = |t: f64| {
            let q = p + d * t;
            let s = q.length_squared() + major * major - minor * minor;
            let value = s * s - 4.0 * major * major * (q.x * q.x + q.y * q.y);
            let slope = 4.0 * s * q.dot(d) - 8.0 * major * major * (q.x * d.x + q.y * d.y);
            (value, slope)
        };
        let samples = 256;
        let mut roots = Vec::new();
        let mut previous = (t0, f(t0).0);
        for k in 1..=samples {
            let t = t0 + (t1 - t0) * k as f64 / samples as f64;
            let value = f(t).0;
            if value == 0.0 {
                roots.push(t);
            } else if (previous.1 < 0.0) != (value < 0.0) && previous.1 != 0.0 {
                if let Ok(root) =
                    find_root_bracketed(f, previous.0, t, 1e-14 * (1.0 + t.abs()), 200)
                {
                    roots.push(root.x);
                }
            }
            previous = (t, value);
        }
        roots
    }
}

impl BrepFace {
    /// A face from its surface, orientation and boundary loops.
    pub fn new(surface: FaceSurface, reversed: bool, loops: Vec<Vec<Vec3>>) -> Self {
        let mut face = Self {
            surface,
            reversed,
            loops,
            bounds: Aabb::EMPTY,
            domain: None,
            segments: crate::segments::Segments::default(),
            plane_grid: None,
        };
        face.segments = crate::segments::Segments::new(&face.loops);
        if let FaceSurface::Plane(frame) = &face.surface {
            face.plane_grid = crate::segments::PlaneGrid::new(*frame, &face.loops);
        }
        face.domain = face.parameter_domain();
        face.bounds = face.compute_bounds();
        face
    }

    /// A conservative bounding box of the face.
    pub fn bounds(&self) -> Aabb {
        self.bounds
    }

    fn boundary_box(&self) -> Aabb {
        self.loops
            .iter()
            .flatten()
            .fold(Aabb::EMPTY, |b, p| b.with(*p))
    }

    /// Parameter ranges for the seeds of a parametric face, from its boundary.
    fn parameter_domain(&self) -> Option<Domain> {
        let boundary: Vec<Vec3> = self.loops.iter().flatten().copied().collect();
        let range = |values: &mut dyn Iterator<Item = f64>| {
            values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(v), hi.max(v))
            })
        };
        match &self.surface {
            FaceSurface::BSpline(b) => {
                let (u, v) = b.domain();
                Some(Domain {
                    u,
                    v,
                    u_periodic: false,
                })
            }
            FaceSurface::Revolution { profile, .. } => {
                // u: the angle of each boundary point relative to the profile's start;
                // v: the profile parameter of the point turned back into the profile's
                // meridian plane.
                let v = match profile.domain() {
                    Some((range, _)) => range,
                    None => {
                        let (lo, hi) =
                            range(&mut boundary.iter().map(|p| profile.parameter_of(*p)));
                        let pad = 0.05 * (hi - lo).abs() + 1e-6;
                        (lo - pad, hi + pad)
                    }
                };
                Some(Domain {
                    u: (0.0, TAU),
                    v,
                    u_periodic: true,
                })
            }
            FaceSurface::Extrusion { profile, direction } => {
                let u = match profile.domain() {
                    Some((range, _)) => range,
                    None => {
                        let (lo, hi) =
                            range(&mut boundary.iter().map(|p| profile.parameter_of(*p)));
                        let pad = 0.05 * (hi - lo).abs() + 1e-6;
                        (lo - pad, hi + pad)
                    }
                };
                let length2 = direction.length_squared();
                let (c0, _) = profile.evaluate(u.0);
                let (lo, hi) =
                    range(&mut boundary.iter().map(|p| (*p - c0).dot(*direction) / length2));
                let profile_span = (0..=16)
                    .map(|k| {
                        let s = u.0 + (u.1 - u.0) * k as f64 / 16.0;
                        ((profile.evaluate(s).0 - c0).dot(*direction) / length2).abs()
                    })
                    .fold(0.0, f64::max);
                Some(Domain {
                    u,
                    v: (lo - profile_span - 1e-6, hi + profile_span + 1e-6),
                    u_periodic: false,
                })
            }
            _ => None,
        }
    }

    fn compute_bounds(&self) -> Aabb {
        let boundary = self.boundary_box();
        let local_box = |frame: &Frame, half: Vec3, centre_z: f64, depth: f64| {
            let local = Aabb {
                min: Vec3::new(-half.x, -half.y, centre_z - depth),
                max: Vec3::new(half.x, half.y, centre_z + depth),
            };
            local
                .corners()
                .iter()
                .fold(Aabb::EMPTY, |b, c| b.with(frame.to_parent_point(*c)))
        };
        let bounds = match &self.surface {
            // A face without loops covers its whole (unbounded) surface.
            FaceSurface::Plane(_) | FaceSurface::Cylinder { .. } | FaceSurface::Cone { .. }
                if boundary.is_empty() =>
            {
                let far = Vec3::new(1e12, 1e12, 1e12);
                Aabb {
                    min: -far,
                    max: far,
                }
            }
            // The sampled boundary can fall short of a curved edge's true extreme by the
            // sampling deviation; 1% of the face's size is far more than that.
            FaceSurface::Plane(_) | FaceSurface::Cylinder { .. } | FaceSurface::Cone { .. } => {
                boundary.padded(0.01 * boundary.size())
            }
            FaceSurface::Sphere { frame, radius } => {
                let r = Vec3::new(*radius, *radius, *radius);
                Aabb {
                    min: frame.origin() - r,
                    max: frame.origin() + r,
                }
            }
            FaceSurface::Torus {
                frame,
                major,
                minor,
            } => {
                let outer = major + minor;
                local_box(frame, Vec3::new(outer, outer, 0.0), 0.0, *minor)
            }
            FaceSurface::BSpline(b) => b
                .points
                .iter()
                .flatten()
                .fold(Aabb::EMPTY, |a, p| a.with(*p)),
            FaceSurface::Revolution { .. } | FaceSurface::Extrusion { .. } => {
                let domain = self.domain.unwrap_or(Domain {
                    u: (0.0, 1.0),
                    v: (0.0, 1.0),
                    u_periodic: false,
                });
                let n = 32;
                let mut b = boundary;
                for i in 0..=n {
                    for j in 0..=n {
                        let u = domain.u.0 + (domain.u.1 - domain.u.0) * i as f64 / n as f64;
                        let v = domain.v.0 + (domain.v.1 - domain.v.0) * j as f64 / n as f64;
                        if let Some((p, _, _)) = self.surface.parametric(u, v) {
                            b = b.with(p);
                        }
                    }
                }
                b.padded(0.02 * b.size())
            }
        };
        let bounds = if bounds.is_empty() { boundary } else { bounds };
        bounds.padded(1e-6 * (1.0 + bounds.size()))
    }

    /// Unit face normal at a point of the face.
    fn face_normal(&self, point: Vec3, parametric: Option<(Vec3, Vec3)>) -> Option<Vec3> {
        let n = match parametric {
            Some((su, sv)) => su.cross(sv).normalized()?,
            None => self.surface.analytic_normal(point)?,
        };
        Some(if self.reversed { -n } else { n })
    }

    /// Whether a point on the surface lies on the face: to the left of the nearest
    /// boundary segment, seen along the face normal.
    pub fn contains(&self, point: Vec3, normal: Vec3) -> bool {
        if self.loops.is_empty() {
            return true;
        }
        // A flat face: count crossings, unless the point is next to an edge.
        let unchanged = self.segments.matches(&self.loops);
        if let (true, Some(grid)) = (unchanged, &self.plane_grid) {
            if let Some(inside) = grid.classify(point) {
                return inside;
            }
        }
        // The nearest segment from the tree, or by a scan if the loops were changed
        // after the face was made.
        let nearest = if unchanged {
            self.segments
                .nearest(&self.loops, point)
                .map(|n| (n.polyline, n.k, n.s, n.distance))
        } else {
            self.nearest_by_scan(point)
        };
        let Some((l, k, s, distance)) = nearest else {
            return false;
        };
        self.lean(l, k, s, point, normal) > 0.0 || distance < 1e-20
    }

    /// The nearest boundary segment by a scan in loop order: (loop, first point, position
    /// along it, squared distance).
    fn nearest_by_scan(&self, point: Vec3) -> Option<(usize, usize, f64, f64)> {
        let mut best: Option<(usize, usize, f64, f64)> = None;
        for (l, polyline) in self.loops.iter().enumerate() {
            let n = polyline.len();
            if n < 2 {
                continue;
            }
            for k in 0..n {
                let a = polyline[k];
                let ab = polyline[(k + 1) % n] - a;
                let length2 = ab.length_squared();
                if length2 == 0.0 {
                    continue;
                }
                let s = ((point - a).dot(ab) / length2).clamp(0.0, 1.0);
                let distance = (point - (a + ab * s)).length_squared();
                if best.is_none_or(|b| distance < b.3 - 1e-24) {
                    best = Some((l, k, s, distance));
                }
            }
        }
        best
    }

    /// How far the point leans to the left of segment `k` of loop `l` (positive: inside),
    /// seen along the face normal. At a shared vertex both neighbouring segments count,
    /// so corners classify correctly.
    fn lean(&self, l: usize, k: usize, s: f64, point: Vec3, normal: Vec3) -> f64 {
        let polyline = &self.loops[l];
        let n = polyline.len();
        let (a, b) = (polyline[k], polyline[(k + 1) % n]);
        let q = a + (b - a) * s;
        let left =
            |from: Vec3, to: Vec3| normal.cross(to - from).normalized().unwrap_or(Vec3::ZERO);
        if s <= 0.0 {
            let previous = polyline[(k + n - 1) % n];
            (left(previous, a) + left(a, b)).dot(point - q)
        } else if s >= 1.0 {
            let next = polyline[(k + 2) % n];
            (left(a, b) + left(b, next)).dot(point - q)
        } else {
            left(a, b).dot(point - q)
        }
    }

    /// Every intersection of the ray with the surface (not yet trimmed), as hits.
    fn surface_hits(&self, origin: Vec3, direction: Vec3) -> Vec<FaceHit> {
        let hit_at = |t: f64, parametric: Option<(Vec3, Vec3, f64)>| -> Option<FaceHit> {
            let point = origin + direction * t;
            let (normal, residual) = match parametric {
                Some((su, sv, residual)) => (self.face_normal(point, Some((su, sv)))?, residual),
                None => (
                    self.face_normal(point, None)?,
                    self.surface.analytic_residual(point),
                ),
            };
            Some(FaceHit {
                distance: t,
                point,
                normal,
                residual,
            })
        };
        if let Some(roots) = self.surface.closed_form(origin, direction) {
            return roots.into_iter().filter_map(|t| hit_at(t, None)).collect();
        }
        let Some((t0, t1)) = self.bounds.ray_interval(origin, direction) else {
            return Vec::new();
        };
        if let FaceSurface::Torus {
            frame,
            major,
            minor,
        } = &self.surface
        {
            return FaceSurface::torus_roots(frame, *major, *minor, origin, direction, t0, t1)
                .into_iter()
                .filter_map(|t| hit_at(t, None))
                .collect();
        }
        // Parametric: Newton on S(u, v) − (o + t·d) = 0 from a grid of seeds.
        let Some(domain) = self.domain else {
            return Vec::new();
        };
        let mut hits: Vec<FaceHit> = Vec::new();
        let scale = 1.0 + self.bounds.size();
        for i in 0..SEEDS {
            for j in 0..SEEDS {
                let mut u =
                    domain.u.0 + (domain.u.1 - domain.u.0) * (i as f64 + 0.5) / SEEDS as f64;
                let mut v =
                    domain.v.0 + (domain.v.1 - domain.v.0) * (j as f64 + 0.5) / SEEDS as f64;
                let Some((s, _, _)) = self.surface.parametric(u, v) else {
                    continue;
                };
                let mut t = (s - origin).dot(direction);
                let mut converged = None;
                for _ in 0..40 {
                    let Some((s, su, sv)) = self.surface.parametric(u, v) else {
                        break;
                    };
                    let g = s - (origin + direction * t);
                    if g.length() < NEWTON_TOLERANCE * scale {
                        converged = Some((su, sv, g.length()));
                        break;
                    }
                    // Solve [su sv −d]·(du, dv, dt) = −g by Cramer's rule.
                    let m = -direction;
                    let determinant = su.dot(sv.cross(m));
                    if determinant.abs() < 1e-300 {
                        break;
                    }
                    let du = -g.dot(sv.cross(m)) / determinant;
                    let dv = -su.dot(g.cross(m)) / determinant;
                    let dt = -su.dot(sv.cross(g)) / determinant;
                    u += du;
                    v += dv;
                    t += dt;
                    if domain.u_periodic {
                        u = domain.u.0 + (u - domain.u.0).rem_euclid(domain.u.1 - domain.u.0);
                    }
                    let margin_u = 1e-9 * (domain.u.1 - domain.u.0).abs();
                    let margin_v = 1e-9 * (domain.v.1 - domain.v.0).abs();
                    if (!domain.u_periodic
                        && (u < domain.u.0 - margin_u || u > domain.u.1 + margin_u))
                        || v < domain.v.0 - margin_v
                        || v > domain.v.1 + margin_v
                        || !t.is_finite()
                    {
                        break;
                    }
                }
                if let Some((su, sv, residual)) = converged {
                    if hits.iter().all(|h| (h.distance - t).abs() > 1e-7 * scale) {
                        if let Some(hit) = hit_at(t, Some((su, sv, residual))) {
                            hits.push(hit);
                        }
                    }
                }
            }
        }
        hits
    }

    /// The nearest hit on the face with distance greater than `minimum`.
    pub fn intersect(&self, origin: Vec3, direction: Vec3, minimum: f64) -> Option<FaceHit> {
        self.intersect_within(origin, direction, minimum, f64::INFINITY)
    }

    /// As [`BrepFace::intersect`], ignoring hits at `maximum` or beyond (a nearer hit on
    /// another face is already known): the boundary test, the costly part, runs only on
    /// hits that could win, nearest first.
    pub fn intersect_within(
        &self,
        origin: Vec3,
        direction: Vec3,
        minimum: f64,
        maximum: f64,
    ) -> Option<FaceHit> {
        if self
            .bounds
            .ray_interval(origin, direction)
            .is_none_or(|(enter, exit)| exit < minimum || enter >= maximum)
        {
            return None;
        }
        let mut hits: Vec<FaceHit> = self
            .surface_hits(origin, direction)
            .into_iter()
            .filter(|h| h.distance > minimum && h.distance < maximum)
            .collect();
        hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        // The side test of `contains` holds near the face; far along an unbounded
        // surface (a cylinder beyond the face's end) the bounding box decides first.
        hits.into_iter()
            .find(|h| self.bounds.contains(h.point) && self.contains(h.point, h.normal))
    }
}

#[cfg(test)]
mod tests;
