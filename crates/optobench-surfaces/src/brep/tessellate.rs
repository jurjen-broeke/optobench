//! Display tessellation of a trimmed face: triangles for drawing, not for tracing (rays
//! always meet the exact face).
//!
//! The face's boundary loops are mapped into its surface's parameter plane (u, v); on a
//! periodic surface (the angle of a cylinder, cone, sphere, torus or surface of
//! revolution) a loop that winds once around is cut at a seam. The loops become
//! constraint edges of a constrained Delaunay triangulation (the `spade` crate), together
//! with a grid of parameter points fine enough for the surface's curvature, so no
//! triangle crosses the boundary. A triangle is kept when the face's exact inside test
//! (`BrepFace::contains`) holds at its centre. Boundary vertices keep their exact 3D
//! points, so neighbouring faces meet without cracks.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use optobench_math::Vec3;
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

use super::{BrepFace, Curve, FaceSurface};

/// Triangles for drawing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// Vertex positions.
    pub positions: Vec<Vec3>,
    /// Unit vertex normals, along the face normal.
    pub normals: Vec<Vec3>,
    /// Triangles, counter-clockwise seen from the side the normals point to.
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    /// Appends another mesh.
    pub fn append(&mut self, other: &Mesh) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.triangles
            .extend(other.triangles.iter().map(|t| t.map(|i| i + base)));
    }

    /// Whether it has no triangles.
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }
}

/// How finely to tessellate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fineness {
    /// Largest distance between a chord and the surface (mm).
    pub chord: f64,
    /// Largest angle a curved direction turns between grid lines (radians).
    pub max_angle: f64,
    /// Smallest such angle, however tight the chord tolerance (radians).
    pub min_angle: f64,
}

impl Default for Fineness {
    fn default() -> Self {
        Self {
            chord: 0.05,
            max_angle: 22.5_f64.to_radians(),
            min_angle: 4.0_f64.to_radians(),
        }
    }
}

impl Fineness {
    /// The angle step on a circle of `radius` (mm).
    fn angle(&self, radius: f64) -> f64 {
        let r = radius.abs();
        let step = if r > self.chord {
            2.0 * (1.0 - self.chord / r).acos()
        } else {
            self.max_angle
        };
        step.clamp(self.min_angle, self.max_angle)
    }
}

/// How the grid runs along one parameter direction.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Spacing {
    /// The surface is straight this way: no grid lines between the ends.
    Straight,
    /// Grid lines at most this far apart (parameter units).
    Step(f64),
}

/// A vertex of the parameter plane: its parameters and, for a boundary point, its exact
/// 3D point.
#[derive(Debug, Clone, Copy)]
struct Node {
    uv: (f64, f64),
    exact: Option<Vec3>,
}

impl BrepFace {
    /// The face as triangles for drawing; empty for an unbounded face (a plane, cylinder
    /// or cone without boundary loops).
    pub fn tessellate(&self, fineness: Fineness) -> Mesh {
        let map = Parameters::new(self);
        if self.loops.is_empty() && !map.closed_without_loops() {
            return Mesh::default();
        }

        // Boundary loops in the parameter plane, each continuous (periodic parameters
        // unwrapped along the loop).
        let mut loops: Vec<Vec<Node>> = Vec::new();
        for polyline in self.loops.iter().filter(|l| l.len() >= 2) {
            // The loops are sampled finely for tracing; drawing needs only the chord
            // tolerance.
            let polyline = thin(polyline, fineness.chord);
            let mut nodes: Vec<Node> = Vec::with_capacity(polyline.len() + 1);
            let mut hint = None;
            for &p in &polyline {
                let mut uv = map.uv(p, hint);
                if let Some(previous) = nodes.last() {
                    uv = map.unwrap_near(uv, previous.uv);
                    // On the axis (a pole, a cone's apex) the angle means nothing: keep
                    // the previous point's.
                    if map.on_axis(p) {
                        uv.0 = previous.uv.0;
                    }
                }
                hint = Some(uv);
                nodes.push(Node { uv, exact: Some(p) });
            }
            // Close the loop, continuing its unwrapping.
            if let (Some(first), Some(last)) = (nodes.first().copied(), nodes.last().copied()) {
                let uv = map.unwrap_near(first.uv, last.uv);
                nodes.push(Node {
                    uv,
                    exact: first.exact,
                });
            }
            // A loop starting on the axis takes the angle of its next point.
            if nodes.len() > 2 && map.on_axis(polyline[0]) {
                let u = nodes[1].uv.0;
                nodes[0].uv.0 = u;
                if let Some(last) = nodes.last_mut() {
                    last.uv.0 = u;
                }
            }
            loops.push(nodes);
        }

        // A periodic direction is triangulated over a whole period, cut at a seam in the
        // widest gap between the loops' points: the face may be the part of the surface
        // outside its loops (a torus less a patch), and a loop winding once round leaves
        // the face a cap reaching the pole or apex. The boundary decides which side is the
        // face's, below.
        let mut ranges = [(f64::INFINITY, f64::NEG_INFINITY); 2];
        let mut reach: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
        for (axis, range) in ranges.iter_mut().enumerate() {
            if !map.periodic[axis] {
                continue;
            }
            let seam = seam_at(&loops, axis);
            loops = loops
                .iter()
                .flat_map(|l| split_at_seam(l, axis, seam, &map))
                .collect();
            *range = (seam, seam + TAU);
            if axis == 0 && !map.periodic[1] {
                reach[1] = map.axis_crossings();
            }
        }
        for axis in 0..2 {
            if ranges[axis].0.is_finite() {
                continue;
            }
            ranges[axis] = loops
                .iter()
                .flatten()
                .map(|n| get(n.uv, axis))
                .chain(reach[axis].iter().copied())
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                    (lo.min(x), hi.max(x))
                });
            if !ranges[axis].0.is_finite() {
                ranges[axis] = map.full[axis];
            }
        }

        // Grid points and the constrained triangulation, in scaled parameters so that
        // triangles are well shaped on the surface.
        let scale = map.scale(ranges);
        let to_plane = |uv: (f64, f64)| Point2::new(uv.0 * scale.0, uv.1 * scale.1);
        let mut cdt: ConstrainedDelaunayTriangulation<Point2<f64>> = Default::default();
        let mut exact: HashMap<usize, Vec3> = HashMap::new();
        let spacing = map.spacing(fineness, ranges);
        let lines = |axis: usize| -> Vec<f64> {
            let (lo, hi) = ranges[axis];
            match spacing[axis] {
                Spacing::Straight => vec![lo, hi],
                Spacing::Step(step) => {
                    let n = (((hi - lo) / step).ceil() as usize).clamp(1, 512);
                    (0..=n)
                        .map(|k| lo + (hi - lo) * k as f64 / n as f64)
                        .collect()
                }
            }
        };
        let (us, vs) = (lines(0), lines(1));
        for &u in &us {
            for &v in &vs {
                let _ = cdt.insert(to_plane((u, v)));
            }
        }
        // Boundary segments (scaled from, to), for deciding which side is the face's.
        let mut segments: Vec<(Point2<f64>, Point2<f64>)> = Vec::new();
        for l in &loops {
            let mut previous: Option<(spade::handles::FixedVertexHandle, Point2<f64>)> = None;
            for n in l {
                let at = to_plane(n.uv);
                let Ok(handle) = cdt.insert(at) else {
                    previous = None;
                    continue;
                };
                if let Some(p) = n.exact {
                    exact.insert(handle.index(), p);
                }
                if let Some((from, from_at)) = previous {
                    if from != handle {
                        cdt.add_constraint_and_split(from, handle, |p| p);
                        segments.push((from_at, at));
                    }
                }
                previous = Some((handle, at));
            }
        }

        // Group the triangles into regions the boundary does not cross, and decide each
        // region by its boundary: the face lies left of its loops, seen along the face
        // normal, which in the parameter plane is the left where the parameters turn the
        // same way as the normal and the right where they turn the other way. A region
        // the boundary does not touch is decided by the exact inside test.
        let unscale = |p: Point2<f64>| (p.x / scale.0, p.y / scale.1);
        let mut region: HashMap<usize, usize> = HashMap::new();
        let mut regions = 0;
        for start in cdt.inner_faces() {
            if region.contains_key(&start.fix().index()) {
                continue;
            }
            let mut stack = vec![start];
            region.insert(start.fix().index(), regions);
            while let Some(face) = stack.pop() {
                for edge in face.adjacent_edges() {
                    if edge.is_constraint_edge() {
                        continue;
                    }
                    if let Some(next) = edge.rev().face().as_inner() {
                        if let std::collections::hash_map::Entry::Vacant(slot) =
                            region.entry(next.fix().index())
                        {
                            slot.insert(regions);
                            stack.push(next);
                        }
                    }
                }
            }
            regions += 1;
        }
        let mut votes = vec![0_i64; regions];
        let region_at = |p: Point2<f64>| match cdt.locate(p) {
            spade::PositionInTriangulation::OnFace(face) => region.get(&face.index()).copied(),
            _ => None,
        };
        for (a, b) in &segments {
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let length = dx.hypot(dy);
            if length == 0.0 {
                continue;
            }
            let middle = Point2::new(0.5 * (a.x + b.x), 0.5 * (a.y + b.y));
            let Some(turn) = map.orientation(unscale(middle)) else {
                continue;
            };
            let step = 1e-6 * length;
            let left = Point2::new(middle.x - dy / length * step, middle.y + dx / length * step);
            let right = Point2::new(middle.x + dy / length * step, middle.y - dx / length * step);
            let (inside, outside) = if turn > 0.0 {
                (left, right)
            } else {
                (right, left)
            };
            if let Some(r) = region_at(inside) {
                votes[r] += 1;
            }
            if let Some(r) = region_at(outside) {
                votes[r] -= 1;
            }
        }
        let mut decided: Vec<Option<bool>> =
            votes.iter().map(|&v| (v != 0).then_some(v > 0)).collect();

        // Keep the triangles inside the face.
        let mut mesh = Mesh::default();
        let mut index: HashMap<usize, u32> = HashMap::new();
        for face in cdt.inner_faces() {
            let vertices = face.vertices();
            let [a, b, c] = vertices.map(|v| unscale(v.position()));
            let centre = ((a.0 + b.0 + c.0) / 3.0, (a.1 + b.1 + c.1) / 3.0);
            let Some((point, normal)) = map.evaluate(centre) else {
                continue;
            };
            let Some(&r) = region.get(&face.fix().index()) else {
                continue;
            };
            let inside = *decided[r].get_or_insert_with(|| self.contains(point, normal));
            if !inside {
                continue;
            }
            let mut corners = [0_u32; 3];
            let mut points = [Vec3::ZERO; 3];
            let mut ok = true;
            for (k, v) in vertices.iter().enumerate() {
                let uv = unscale(v.position());
                let id = v.fix().index();
                let evaluated = map.evaluate(uv);
                let Some((p, n)) = exact
                    .get(&id)
                    .map(|p| (*p, map.normal_at(*p, uv)))
                    .or(evaluated)
                else {
                    ok = false;
                    break;
                };
                points[k] = p;
                corners[k] = *index.entry(id).or_insert_with(|| {
                    mesh.positions.push(p);
                    mesh.normals.push(n);
                    (mesh.positions.len() - 1) as u32
                });
            }
            if !ok {
                continue;
            }
            let facing = (points[1] - points[0])
                .cross(points[2] - points[0])
                .dot(normal);
            if facing < 0.0 {
                corners.swap(1, 2);
            }
            if corners[0] != corners[1] && corners[1] != corners[2] && corners[0] != corners[2] {
                mesh.triangles.push(corners);
            }
        }
        mesh
    }
}

/// A closed polyline with the points dropped that lie within `tolerance` of the chord
/// between the points kept around them (Douglas–Peucker, from the point farthest from the
/// first).
fn thin(points: &[Vec3], tolerance: f64) -> Vec<Vec3> {
    let n = points.len();
    if n <= 4 {
        return points.to_vec();
    }
    let far = (1..n)
        .max_by(|&a, &b| {
            (points[a] - points[0])
                .length_squared()
                .total_cmp(&(points[b] - points[0]).length_squared())
        })
        .unwrap_or(n / 2);
    let mut keep = vec![false; n + 1];
    keep[0] = true;
    keep[far] = true;
    keep[n] = true;
    let at = |i: usize| points[i % n];
    let mut stack = vec![(0, far), (far, n)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (at(a), at(b));
        let chord = pb - pa;
        let length2 = chord.length_squared();
        let mut worst = (0.0, a);
        for i in a + 1..b {
            let d = at(i) - pa;
            let off = if length2 > 0.0 {
                (d - chord * (d.dot(chord) / length2)).length()
            } else {
                d.length()
            };
            if off > worst.0 {
                worst = (off, i);
            }
        }
        if worst.0 > tolerance {
            keep[worst.1] = true;
            stack.push((a, worst.1));
            stack.push((worst.1, b));
        }
    }
    (0..n).filter(|&i| keep[i]).map(|i| points[i]).collect()
}

fn get(uv: (f64, f64), axis: usize) -> f64 {
    if axis == 0 { uv.0 } else { uv.1 }
}

fn set(uv: &mut (f64, f64), axis: usize, value: f64) {
    if axis == 0 {
        uv.0 = value;
    } else {
        uv.1 = value;
    }
}

/// Where to cut a periodic direction: in the widest gap between the loops' points.
fn seam_at(loops: &[Vec<Node>], axis: usize) -> f64 {
    let mut angles: Vec<f64> = loops
        .iter()
        .flatten()
        .map(|n| get(n.uv, axis).rem_euclid(TAU))
        .collect();
    if angles.is_empty() {
        return -PI;
    }
    angles.sort_by(f64::total_cmp);
    let mut best = (
        angles[0] + TAU - angles[angles.len() - 1],
        angles[angles.len() - 1],
    );
    for w in angles.windows(2) {
        if w[1] - w[0] > best.0 {
            best = (w[1] - w[0], w[0]);
        }
    }
    best.1 + 0.5 * best.0
}

/// A continuous polyline cut where it crosses `seam + k·2π`, each piece moved by whole
/// turns into [seam, seam + 2π].
fn split_at_seam(line: &[Node], axis: usize, seam: f64, map: &Parameters<'_>) -> Vec<Vec<Node>> {
    // Which period a parameter is in: 0 for [seam, seam + 2π).
    let turn = |x: f64| ((x - seam) / TAU).floor() as i64;
    let shifted = |n: Node, k: i64| {
        let mut n = n;
        let x = get(n.uv, axis) - k as f64 * TAU;
        set(&mut n.uv, axis, x);
        n
    };
    let mut pieces = Vec::new();
    let Some(&first) = line.first() else {
        return pieces;
    };
    let mut k = turn(get(first.uv, axis));
    let mut piece = vec![shifted(first, k)];
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let kb = turn(get(b.uv, axis));
        if kb != k {
            // Cross every seam line between a and b.
            let (xa, xb) = (get(a.uv, axis), get(b.uv, axis));
            let step = if kb > k { 1 } else { -1 };
            let mut kk = k;
            while kk != kb {
                // Going up, the line at the top of period kk; going down, at its bottom.
                let crossed = if step > 0 { kk + 1 } else { kk };
                let line_at = seam + crossed as f64 * TAU;
                let t = if (xb - xa).abs() > 0.0 {
                    ((line_at - xa) / (xb - xa)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let uv = (
                    a.uv.0 + (b.uv.0 - a.uv.0) * t,
                    a.uv.1 + (b.uv.1 - a.uv.1) * t,
                );
                let crossing = Node {
                    uv,
                    exact: map.evaluate(uv).map(|(p, _)| p),
                };
                piece.push(shifted(crossing, kk));
                pieces.push(std::mem::take(&mut piece));
                kk += step;
                piece.push(shifted(crossing, kk));
            }
            k = kb;
        }
        piece.push(shifted(b, k));
    }
    if piece.len() >= 2 {
        pieces.push(piece);
    }
    pieces
}

/// A face's surface as a map from parameters to points, and back.
struct Parameters<'a> {
    face: &'a BrepFace,
    /// Whether u and v are angles (period 2π).
    periodic: [bool; 2],
    /// The full parameter ranges, for a face without loops.
    full: [(f64, f64); 2],
}

impl<'a> Parameters<'a> {
    fn new(face: &'a BrepFace) -> Self {
        let (periodic, full) = match &face.surface {
            FaceSurface::Plane(_) => ([false, false], [(0.0, 0.0); 2]),
            FaceSurface::Cylinder { .. } | FaceSurface::Cone { .. } => {
                ([true, false], [(-PI, PI), (0.0, 0.0)])
            }
            FaceSurface::Sphere { .. } => ([true, false], [(-PI, PI), (-0.5 * PI, 0.5 * PI)]),
            FaceSurface::Torus { .. } => ([true, true], [(-PI, PI), (-PI, PI)]),
            FaceSurface::Revolution { .. } => {
                let v = face.domain.map_or((0.0, 1.0), |d| d.v);
                ([true, false], [(-PI, PI), v])
            }
            FaceSurface::BSpline(b) => {
                let (u, v) = b.domain();
                ([false, false], [u, v])
            }
            FaceSurface::Extrusion { .. } => {
                let d = face.domain.map_or(((0.0, 1.0), (0.0, 1.0)), |d| (d.u, d.v));
                ([false, false], [d.0, d.1])
            }
        };
        Self {
            face,
            periodic,
            full,
        }
    }

    /// Whether a face without loops is a whole closed surface (sphere, torus, a closed
    /// parametric patch), drawn over its full range.
    fn closed_without_loops(&self) -> bool {
        matches!(
            self.face.surface,
            FaceSurface::Sphere { .. } | FaceSurface::Torus { .. } | FaceSurface::BSpline(_)
        )
    }

    /// Whether a point lies on the axis of an angle parameter, where the angle is not
    /// defined: a sphere's pole, a cone's apex, a surface of revolution meeting its axis.
    fn on_axis(&self, p: Vec3) -> bool {
        const NEAR: f64 = 1e-7;
        match &self.face.surface {
            FaceSurface::Cone { frame, .. } | FaceSurface::Sphere { frame, .. } => {
                let q = frame.to_local_point(p);
                q.x.hypot(q.y) < NEAR
            }
            FaceSurface::Revolution {
                axis_point,
                axis_direction,
                ..
            } => {
                let r = p - *axis_point;
                (r - *axis_direction * r.dot(*axis_direction)).length() < NEAR
            }
            _ => false,
        }
    }

    /// Where on the surface the angle's axis is: v of a sphere's poles, a cone's apex,
    /// or the ends of a surface of revolution's profile that lie on the axis. A loop
    /// winding round the axis leaves the cap beyond it to one side or the other; the grid
    /// reaches both, and the inside test keeps the face's.
    fn axis_crossings(&self) -> Vec<f64> {
        match &self.face.surface {
            FaceSurface::Sphere { .. } => vec![-0.5 * PI, 0.5 * PI],
            FaceSurface::Cone {
                radius, semi_angle, ..
            } => {
                let t = semi_angle.tan();
                if t.abs() > 1e-12 {
                    vec![-radius / t]
                } else {
                    Vec::new()
                }
            }
            FaceSurface::Revolution {
                profile,
                axis_point,
                axis_direction,
            } => {
                let (lo, hi) = self.full[1];
                let off_axis = |v: f64| {
                    let r = profile.evaluate(v).0 - *axis_point;
                    (r - *axis_direction * r.dot(*axis_direction)).length()
                };
                [lo, hi]
                    .into_iter()
                    .filter(|v| off_axis(*v) < 1e-6)
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// The periodic parameters moved by whole turns to lie nearest `near`.
    fn unwrap_near(&self, uv: (f64, f64), near: (f64, f64)) -> (f64, f64) {
        let mut out = uv;
        for axis in 0..2 {
            if self.periodic[axis] {
                let x = get(uv, axis);
                let n = get(near, axis);
                set(&mut out, axis, x - ((x - n) / TAU).round() * TAU);
            }
        }
        out
    }

    /// The parameters of a point on the surface (`hint`: those of a nearby point).
    fn uv(&self, p: Vec3, hint: Option<(f64, f64)>) -> (f64, f64) {
        match &self.face.surface {
            FaceSurface::Plane(frame) => {
                let q = frame.to_local_point(p);
                (q.x, q.y)
            }
            FaceSurface::Cylinder { frame, .. } | FaceSurface::Cone { frame, .. } => {
                let q = frame.to_local_point(p);
                (q.y.atan2(q.x), q.z)
            }
            FaceSurface::Sphere { frame, .. } => {
                let q = frame.to_local_point(p);
                (q.y.atan2(q.x), q.z.atan2(q.x.hypot(q.y)))
            }
            FaceSurface::Torus { frame, major, .. } => {
                let q = frame.to_local_point(p);
                (q.y.atan2(q.x), q.z.atan2(q.x.hypot(q.y) - major))
            }
            FaceSurface::Revolution {
                profile,
                axis_point,
                axis_direction,
            } => {
                // u: the angle from the profile's meridian half-plane about the axis;
                // v: the profile parameter of the point turned back into it.
                let radial = |x: Vec3| {
                    let r = x - *axis_point;
                    r - *axis_direction * r.dot(*axis_direction)
                };
                let middle = self.full[1].0 + 0.5 * (self.full[1].1 - self.full[1].0);
                let reference = radial(profile.evaluate(middle).0);
                let here = radial(p);
                let u = axis_direction
                    .dot(reference.cross(here))
                    .atan2(reference.dot(here));
                let back = super::rotation(*axis_direction, -u);
                let q = *axis_point + back.apply(p - *axis_point);
                (u, profile.parameter_of(q))
            }
            FaceSurface::Extrusion { profile, direction } => {
                let length2 = direction.length_squared().max(1e-300);
                let mut u = hint.map_or_else(|| profile.parameter_of(p), |h| h.0);
                let mut v = 0.0;
                for _ in 0..6 {
                    v = (p - profile.evaluate(u).0).dot(*direction) / length2;
                    u = profile.parameter_of(p - *direction * v);
                }
                (u, v)
            }
            FaceSurface::BSpline(b) => {
                let ((u0, u1), (v0, v1)) = b.domain();
                let seed = hint.unwrap_or_else(|| {
                    // The nearest point of a coarse grid.
                    let mut best = ((u0, v0), f64::INFINITY);
                    for i in 0..=12 {
                        for j in 0..=12 {
                            let uv = (
                                u0 + (u1 - u0) * i as f64 / 12.0,
                                v0 + (v1 - v0) * j as f64 / 12.0,
                            );
                            let d = (b.evaluate(uv.0, uv.1).0 - p).length_squared();
                            if d < best.1 {
                                best = (uv, d);
                            }
                        }
                    }
                    best.0
                });
                // Gauss–Newton on |S(u, v) − p|², kept in the domain.
                let (mut u, mut v) = seed;
                for _ in 0..12 {
                    let (s, su, sv) = b.evaluate(u, v);
                    let r = s - p;
                    let (a, bb, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
                    let det = a * c - bb * bb;
                    if det.abs() < 1e-300 {
                        break;
                    }
                    let (gu, gv) = (su.dot(r), sv.dot(r));
                    let du = (c * gu - bb * gv) / det;
                    let dv = (a * gv - bb * gu) / det;
                    u = (u - du).clamp(u0, u1);
                    v = (v - dv).clamp(v0, v1);
                    if du.abs() + dv.abs() < 1e-12 * (1.0 + u.abs() + v.abs()) {
                        break;
                    }
                }
                (u, v)
            }
        }
    }

    /// +1 where ∂S/∂u × ∂S/∂v points along the face normal, −1 where against it (by
    /// central differences), `None` where the surface is singular.
    fn orientation(&self, (u, v): (f64, f64)) -> Option<f64> {
        let h = 1e-6;
        let (_, normal) = self.evaluate((u, v))?;
        let du = self.evaluate((u + h, v))?.0 - self.evaluate((u - h, v))?.0;
        let dv = self.evaluate((u, v + h))?.0 - self.evaluate((u, v - h))?.0;
        let turn = du.cross(dv).dot(normal);
        (turn.abs() > 1e-18).then(|| turn.signum())
    }

    /// The point and unit face normal at (u, v).
    fn evaluate(&self, (u, v): (f64, f64)) -> Option<(Vec3, Vec3)> {
        let face = self.face;
        let (point, normal) = match &face.surface {
            FaceSurface::Plane(frame) => {
                (frame.to_parent_point(Vec3::new(u, v, 0.0)), frame.axes()[2])
            }
            FaceSurface::Cylinder { frame, radius } => {
                let (s, c) = u.sin_cos();
                (
                    frame.to_parent_point(Vec3::new(radius * c, radius * s, v)),
                    frame.to_parent_vector(Vec3::new(c, s, 0.0)),
                )
            }
            FaceSurface::Cone {
                frame,
                radius,
                semi_angle,
            } => {
                let (s, c) = u.sin_cos();
                let rho = radius + v * semi_angle.tan();
                let (sa, ca) = semi_angle.sin_cos();
                (
                    frame.to_parent_point(Vec3::new(rho * c, rho * s, v)),
                    frame.to_parent_vector(Vec3::new(c * ca, s * ca, -sa)),
                )
            }
            FaceSurface::Sphere { frame, radius } => {
                let (su, cu) = u.sin_cos();
                let (sv, cv) = v.sin_cos();
                let n = Vec3::new(cv * cu, cv * su, sv);
                (
                    frame.to_parent_point(n * *radius),
                    frame.to_parent_vector(n),
                )
            }
            FaceSurface::Torus {
                frame,
                major,
                minor,
            } => {
                let (su, cu) = u.sin_cos();
                let (sv, cv) = v.sin_cos();
                let rho = major + minor * cv;
                (
                    frame.to_parent_point(Vec3::new(rho * cu, rho * su, minor * sv)),
                    frame.to_parent_vector(Vec3::new(cv * cu, cv * su, sv)),
                )
            }
            _ => {
                let (p, su, sv) = face.surface.parametric(u, v)?;
                (p, su.cross(sv))
            }
        };
        let normal = normal.normalized()?;
        Some((point, if face.reversed { -normal } else { normal }))
    }

    /// The face normal at a boundary point (its parameters `uv` approximate).
    fn normal_at(&self, p: Vec3, uv: (f64, f64)) -> Vec3 {
        let analytic = self
            .face
            .surface
            .analytic_normal(p)
            .map(|n| if self.face.reversed { -n } else { n });
        analytic
            .or_else(|| self.evaluate(uv).map(|(_, n)| n))
            .unwrap_or(Vec3::Z)
    }

    /// Scale factors from parameters to millimetres, roughly, so the triangulation is
    /// well shaped on the surface.
    fn scale(&self, ranges: [(f64, f64); 2]) -> (f64, f64) {
        let middle = (
            0.5 * (ranges[0].0 + ranges[0].1),
            0.5 * (ranges[1].0 + ranges[1].1),
        );
        let clean = |x: f64| if x.is_finite() && x > 1e-9 { x } else { 1.0 };
        match &self.face.surface {
            FaceSurface::Plane(_) => (1.0, 1.0),
            FaceSurface::Cylinder { radius, .. } => (clean(*radius), 1.0),
            FaceSurface::Cone {
                radius, semi_angle, ..
            } => (
                clean((radius + middle.1 * semi_angle.tan()).abs()),
                clean(1.0 / semi_angle.cos()),
            ),
            FaceSurface::Sphere { radius, .. } => (clean(*radius), clean(*radius)),
            FaceSurface::Torus { major, minor, .. } => (clean(major + minor), clean(*minor)),
            _ => match self.face.surface.parametric(middle.0, middle.1) {
                Some((_, su, sv)) => (clean(su.length()), clean(sv.length())),
                None => (1.0, 1.0),
            },
        }
    }

    /// The grid spacing along u and v.
    fn spacing(&self, fineness: Fineness, ranges: [(f64, f64); 2]) -> [Spacing; 2] {
        let curved = |radius: f64| Spacing::Step(fineness.angle(radius));
        let along_curve = |curve: &Curve, range: (f64, f64)| match curve {
            Curve::Line { .. } => Spacing::Straight,
            Curve::Circle { radius, .. } => curved(*radius),
            _ => Spacing::Step(((range.1 - range.0) / 24.0).abs().max(1e-12)),
        };
        match &self.face.surface {
            FaceSurface::Plane(_) => [Spacing::Straight, Spacing::Straight],
            FaceSurface::Cylinder { radius, .. } => [curved(*radius), Spacing::Straight],
            FaceSurface::Cone {
                radius, semi_angle, ..
            } => {
                let widest = [ranges[1].0, ranges[1].1]
                    .map(|v| (radius + v * semi_angle.tan()).abs())
                    .into_iter()
                    .fold(0.0, f64::max);
                [curved(widest), Spacing::Straight]
            }
            FaceSurface::Sphere { radius, .. } => [curved(*radius), curved(*radius)],
            FaceSurface::Torus { major, minor, .. } => [curved(major + minor), curved(*minor)],
            FaceSurface::Revolution {
                profile,
                axis_point,
                axis_direction,
            } => {
                let widest = (0..=8)
                    .map(|k| {
                        let v = ranges[1].0 + (ranges[1].1 - ranges[1].0) * k as f64 / 8.0;
                        let r = profile.evaluate(v).0 - *axis_point;
                        (r - *axis_direction * r.dot(*axis_direction)).length()
                    })
                    .fold(0.0, f64::max);
                [curved(widest), along_curve(profile, ranges[1])]
            }
            FaceSurface::Extrusion { profile, .. } => {
                [along_curve(profile, ranges[0]), Spacing::Straight]
            }
            FaceSurface::BSpline(b) => {
                let spans = |knots: &[f64]| {
                    let mut distinct = knots.to_vec();
                    distinct.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
                    distinct.len().saturating_sub(1).max(1)
                };
                // Length on the surface along each direction, through the middle.
                let middle = [
                    0.5 * (ranges[0].0 + ranges[0].1),
                    0.5 * (ranges[1].0 + ranges[1].1),
                ];
                let length = |axis: usize| {
                    let at = |t: f64| {
                        let x = ranges[axis].0 + (ranges[axis].1 - ranges[axis].0) * t;
                        if axis == 0 {
                            b.evaluate(x, middle[1]).0
                        } else {
                            b.evaluate(middle[0], x).0
                        }
                    };
                    (1..=8)
                        .map(|k| (at(k as f64 / 8.0) - at((k - 1) as f64 / 8.0)).length())
                        .sum::<f64>()
                };
                // Lines per knot span and degree, but none closer than half a millimetre.
                let step = |axis: usize, knots: &[f64], degree: usize| {
                    let by_size = ((length(axis) / 0.5).ceil() as usize).max(2);
                    let n = (spans(knots) * degree.max(1)).clamp(4, 24).min(by_size);
                    let range = ranges[axis];
                    Spacing::Step(((range.1 - range.0) / n as f64).abs().max(1e-12))
                };
                [
                    step(0, &b.u_knots, b.u_degree),
                    step(1, &b.v_knots, b.v_degree),
                ]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use optobench_math::{Frame, Transform};

    fn area(mesh: &Mesh) -> f64 {
        mesh.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
                0.5 * (b - a).cross(c - a).length()
            })
            .sum()
    }

    fn circle(radius: f64, z: f64, n: usize, clockwise: bool) -> Vec<Vec3> {
        (0..n)
            .map(|k| {
                let a = TAU * k as f64 / n as f64 * if clockwise { -1.0 } else { 1.0 };
                Vec3::new(radius * a.cos(), radius * a.sin(), z)
            })
            .collect()
    }

    /// Every triangle faces along the face normal, and every normal is a unit vector.
    fn check_orientation(mesh: &Mesh) {
        for t in &mesh.triangles {
            let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
            let n = (b - a).cross(c - a);
            let average = t
                .iter()
                .map(|&i| mesh.normals[i as usize])
                .fold(Vec3::ZERO, |s, x| s + x);
            assert!(n.dot(average) > 0.0, "triangle against its normals");
        }
        for n in &mesh.normals {
            assert!((n.length() - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn a_square_plate_with_a_hole_has_its_area() {
        // A 20 mm square with a 5 mm radius hole: area 400 − π·25 (to the polygon's
        // area for the sampled hole).
        let outer = vec![
            Vec3::new(-10.0, -10.0, 0.0),
            Vec3::new(10.0, -10.0, 0.0),
            Vec3::new(10.0, 10.0, 0.0),
            Vec3::new(-10.0, 10.0, 0.0),
        ];
        let n = 64;
        let hole = circle(5.0, 0.0, n, true);
        let face = BrepFace::new(
            FaceSurface::Plane(Frame::from_transform(Transform::IDENTITY)),
            false,
            vec![outer, hole],
        );
        // A tolerance below the hole polygon's chord error, so no point is dropped.
        let mesh = face.tessellate(Fineness {
            chord: 1e-4,
            ..Fineness::default()
        });
        let polygon = 0.5 * n as f64 * 25.0 * (TAU / n as f64).sin();
        assert!(
            (area(&mesh) - (400.0 - polygon)).abs() < 1e-9,
            "{}",
            area(&mesh)
        );
        check_orientation(&mesh);
        assert!(mesh.normals.iter().all(|n| (*n - Vec3::Z).length() < 1e-12));
    }

    #[test]
    fn a_reversed_plane_faces_down() {
        let outer = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
        ];
        let face = BrepFace::new(
            FaceSurface::Plane(Frame::from_transform(Transform::IDENTITY)),
            true,
            vec![outer],
        );
        let mesh = face.tessellate(Fineness::default());
        assert!((area(&mesh) - 1.0).abs() < 1e-12);
        check_orientation(&mesh);
        assert!(mesh.normals.iter().all(|n| (*n + Vec3::Z).length() < 1e-12));
    }

    #[test]
    fn a_full_cylinder_band_without_a_seam_edge_wraps() {
        // Radius 4, height 10, bounded by two circles (no seam edge): the loops wind
        // once around, so the face is cut at a seam. Area 2π·4·10 to the chord error.
        let n = 96;
        let bottom = circle(4.0, 0.0, n, false);
        let mut top = circle(4.0, 10.0, n, false);
        top.reverse();
        let face = BrepFace::new(
            FaceSurface::Cylinder {
                frame: Frame::from_transform(Transform::IDENTITY),
                radius: 4.0,
            },
            false,
            vec![bottom, top],
        );
        let mesh = face.tessellate(Fineness::default());
        let exact = TAU * 4.0 * 10.0;
        assert!(
            (area(&mesh) - exact).abs() < 0.01 * exact,
            "{} vs {exact}",
            area(&mesh)
        );
        check_orientation(&mesh);
        // Every vertex lies on the cylinder.
        for p in &mesh.positions {
            assert!((p.x.hypot(p.y) - 4.0).abs() < 1e-9);
        }
    }

    #[test]
    fn a_half_cylinder_with_seam_edges_keeps_to_its_half() {
        // The half x ≥ 0 of a radius-3 cylinder, height 2, bounded by a loop of two
        // half circles and two straight edges.
        let n = 32;
        let half = |z: f64| -> Vec<Vec3> {
            (0..=n)
                .map(|k| {
                    let a = -0.5 * PI + PI * k as f64 / n as f64;
                    Vec3::new(3.0 * a.cos(), 3.0 * a.sin(), z)
                })
                .collect()
        };
        let mut boundary = half(0.0);
        let mut top = half(2.0);
        top.reverse();
        boundary.extend(top);
        let face = BrepFace::new(
            FaceSurface::Cylinder {
                frame: Frame::from_transform(Transform::IDENTITY),
                radius: 3.0,
            },
            false,
            vec![boundary],
        );
        let mesh = face.tessellate(Fineness::default());
        let exact = PI * 3.0 * 2.0;
        assert!(
            (area(&mesh) - exact).abs() < 0.01 * exact,
            "{}",
            area(&mesh)
        );
        assert!(mesh.positions.iter().all(|p| p.x > -1e-9));
        check_orientation(&mesh);
    }

    #[test]
    fn a_whole_sphere_has_its_area() {
        let face = BrepFace::new(
            FaceSurface::Sphere {
                frame: Frame::from_transform(Transform::IDENTITY),
                radius: 2.0,
            },
            false,
            Vec::new(),
        );
        let mesh = face.tessellate(Fineness::default());
        // Facets inscribed in the sphere, grid steps of at most 22.5°: each facet's area
        // is short of the surface's by at most about 1 − cos²(11.25°) ≈ 3.8 %.
        let exact = 4.0 * PI * 4.0;
        let bound = 1.0 - (11.25_f64.to_radians().cos()).powi(2);
        assert!(area(&mesh) < exact, "{}", area(&mesh));
        assert!(area(&mesh) > (1.0 - bound) * exact, "{}", area(&mesh));
        check_orientation(&mesh);
        for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
            assert!((p.length() - 2.0).abs() < 1e-9);
            assert!((*n - *p * 0.5).length() < 1e-9);
        }
    }

    #[test]
    fn an_unbounded_plane_draws_nothing() {
        let face = BrepFace::new(
            FaceSurface::Plane(Frame::from_transform(Transform::IDENTITY)),
            false,
            Vec::new(),
        );
        assert!(face.tessellate(Fineness::default()).is_empty());
    }
}
