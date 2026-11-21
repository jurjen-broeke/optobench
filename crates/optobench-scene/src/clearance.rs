//! How far a part can move before it runs into another (M18, rough placement).
//!
//! A DOF with a free range (`Dof::free`) is limited only by the space around its part:
//! the parts it moves (its target and those of the DOFs following it) may touch other
//! parts, but not sink into them by more than a tolerance (0.05 mm by default).
//!
//! The test works on points of the parts' surfaces and the parts' exact faces:
//!
//! - points: each part's faces tessellated (`BrepFace::tessellate`), their vertices and
//!   their triangles subdivided until no two neighbouring points are further apart than
//!   the spacing (1 mm by default); for the parts that stay, only points inside the
//!   region the moving parts sweep through;
//! - each point's path as the DOF changes: for a slide a straight line, for a tilt (or
//!   parts following at another ratio) chords short enough that no point moves more than
//!   the spacing along one; the points of the parts that stay move the other way relative
//!   to the moving parts;
//! - along a path, the exact faces of each part it may meet (`BrepFace::intersect`, their
//!   normals outward): entering a part at an angle θ to its surface normal, a point is the
//!   tolerance deep after tolerance / cos θ further along, which is where the motion
//!   stops, unless the path leaves the part again before that (it only grazes it). A
//!   point already inside a part where the motion starts is a contact (a rod in its hole,
//!   a plate on its post), ignored until its path leaves that part.
//!
//! On a straight path, points on one line parallel to the motion (along a rod, across a
//! plate's edge) share one ray. The work is shared out over the processor's cores, except
//! on wasm32.
//!
//! A layout has no CAD parts: its items stand in as simple solids (`Scene::stand_ins`):
//! a lens a disc of its diameter from its first vertex to its last (and as far as its
//! curved surfaces bulge at the rim), a mirror a disc 6 mm thick behind its face, an
//! ideal lens or objective a disc 2 mm thick, a screen a box of its size 1 mm thick, a
//! tube a wall 1 mm thick around its bore, an iris a plate 2 mm thick with its hole.
//! Sources and posts have no size in a layout and are left out.
//!
//! What is missed: features thinner than the spacing that cross each other between
//! points (two thin wires crossing), and a part that slides off the end of the rods it
//! runs on (leaving them is not an overlap).

use std::collections::{BTreeMap, HashMap};

use optobench_math::{Transform, Vec3};
use optobench_surfaces::brep::Fineness;

use optobench_math::Frame;
use optobench_surfaces::brep::{BrepFace, FaceSurface};

use crate::dof::{Motion, Target};
use crate::{ElementKind, ProxyKind, Scene, SceneError, Solid, SolidFace, SolidRole};

/// The key of the part a DOF target moves: a CAD occurrence's path, or for a layout's
/// stand-in its table and id (`element:L1`).
fn part_key(target: &Target) -> String {
    match target {
        Target::Occurrence(path) => path.clone(),
        Target::Element(id) => format!("element:{id}"),
        Target::Source(id) => format!("source:{id}"),
        Target::Detector(id) => format!("detector:{id}"),
    }
}

/// A circle of radius `r` about local z at height `z`, in `frame`'s scene coordinates.
fn circle(frame: &Frame, r: f64, z: f64) -> Vec<Vec3> {
    (0..48)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 48.0;
            frame.to_parent_point(Vec3::new(r * a.cos(), r * a.sin(), z))
        })
        .collect()
}

/// A rectangle of half-sides `w`, `h` about local z at height `z`.
fn rectangle(frame: &Frame, w: f64, h: f64, z: f64) -> Vec<Vec3> {
    [(-w, -h), (w, -h), (w, h), (-w, h)]
        .iter()
        .map(|&(x, y)| frame.to_parent_point(Vec3::new(x, y, z)))
        .collect()
}

/// A plane face across local z at `z`, its normal outward along `side` (±1).
fn across(frame: &Frame, z: f64, side: f64, loops: Vec<Vec<Vec3>>) -> SolidFace {
    let at = Frame::from_transform(
        frame.transform() * Transform::from_translation(Vec3::new(0.0, 0.0, z)),
    );
    SolidFace {
        geometry: std::sync::Arc::new(BrepFace::new(FaceSurface::Plane(at), side < 0.0, loops)),
        placement: Transform::IDENTITY,
    }
}

/// A cylinder face of radius `r` about local z from `z0` to `z1`, its normal outward
/// (away from the axis) or, for a bore, inward.
fn around(frame: &Frame, r: f64, z0: f64, z1: f64, bore: bool) -> SolidFace {
    SolidFace {
        geometry: std::sync::Arc::new(BrepFace::new(
            FaceSurface::Cylinder {
                frame: *frame,
                radius: r,
            },
            bore,
            vec![circle(frame, r, z0), circle(frame, r, z1)],
        )),
        placement: Transform::IDENTITY,
    }
}

/// A disc of radius `r` along local z from `z0` to `z1`.
fn disc(frame: &Frame, r: f64, z0: f64, z1: f64) -> Vec<SolidFace> {
    vec![
        around(frame, r, z0, z1, false),
        across(frame, z0, -1.0, vec![circle(frame, r, z0)]),
        across(frame, z1, 1.0, vec![circle(frame, r, z1)]),
    ]
}

/// A box of half-sides `w`, `h` along local z from `z0` to `z1`.
fn slab(frame: &Frame, w: f64, h: f64, z0: f64, z1: f64) -> Vec<SolidFace> {
    // A side face: the plane through local `axis · half`, its normal along `axis`, `a`
    // wide either way along `x`.
    let side = |axis: Vec3, x: Vec3, half: f64, a: f64| {
        let origin = frame.to_parent_point(axis * half);
        let at = Frame::new(
            origin,
            frame.to_parent_vector(axis),
            frame.to_parent_vector(x),
        )
        .unwrap_or(*frame);
        let corners: Vec<Vec3> = [(-a, z0), (a, z0), (a, z1), (-a, z1)]
            .iter()
            .map(|&(u, z)| frame.to_parent_point(axis * half + x * u + Vec3::new(0.0, 0.0, z)))
            .collect();
        SolidFace {
            geometry: std::sync::Arc::new(BrepFace::new(
                FaceSurface::Plane(at),
                false,
                vec![corners],
            )),
            placement: Transform::IDENTITY,
        }
    };
    vec![
        across(frame, z0, -1.0, vec![rectangle(frame, w, h, z0)]),
        across(frame, z1, 1.0, vec![rectangle(frame, w, h, z1)]),
        side(Vec3::X, Vec3::Y, w, h),
        side(-Vec3::X, Vec3::Y, w, h),
        side(Vec3::Y, Vec3::X, h, w),
        side(-Vec3::Y, Vec3::X, h, w),
    ]
}

/// How far a surface of `radius` bulges at rim radius `r`, along z (signed).
fn sag(radius: f64, r: f64) -> f64 {
    if !radius.is_finite() || radius.abs() <= r {
        return 0.0;
    }
    radius - radius.signum() * (radius * radius - r * r).sqrt()
}

/// How the clearance is found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// How deep a part may sink into another (mm).
    pub tolerance: f64,
    /// The largest distance between neighbouring surface points (mm).
    pub spacing: f64,
    /// How the faces are tessellated for their points.
    pub fineness: Fineness,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tolerance: 0.05,
            spacing: 1.0,
            fineness: Fineness {
                chord: 0.1,
                ..Fineness::default()
            },
        }
    }
}

/// How far a DOF can go from its value now, each way.
#[derive(Debug, Clone, PartialEq)]
pub struct Clearance {
    /// The lowest and highest values (mm or radians) without an overlap.
    pub range: (f64, f64),
    /// What stops it going lower and higher: the part (its id), or `None` when nothing
    /// does within the search (the scene's size, or half a turn).
    pub stopped_by: (Option<String>, Option<String>),
}

/// An axis-aligned box.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    low: Vec3,
    high: Vec3,
}

impl Bounds {
    fn of(points: impl IntoIterator<Item = Vec3>) -> Option<Self> {
        let mut it = points.into_iter();
        let first = it.next()?;
        let mut b = Self {
            low: first,
            high: first,
        };
        for p in it {
            b.low = Vec3::new(b.low.x.min(p.x), b.low.y.min(p.y), b.low.z.min(p.z));
            b.high = Vec3::new(b.high.x.max(p.x), b.high.y.max(p.y), b.high.z.max(p.z));
        }
        Some(b)
    }

    fn grown(self, by: f64) -> Self {
        let d = Vec3::new(by, by, by);
        Self {
            low: self.low - d,
            high: self.high + d,
        }
    }

    fn contains(&self, p: Vec3) -> bool {
        p.x >= self.low.x
            && p.x <= self.high.x
            && p.y >= self.low.y
            && p.y <= self.high.y
            && p.z >= self.low.z
            && p.z <= self.high.z
    }

    fn corners(&self) -> [Vec3; 8] {
        let (l, h) = (self.low, self.high);
        [
            Vec3::new(l.x, l.y, l.z),
            Vec3::new(h.x, l.y, l.z),
            Vec3::new(l.x, h.y, l.z),
            Vec3::new(h.x, h.y, l.z),
            Vec3::new(l.x, l.y, h.z),
            Vec3::new(h.x, l.y, h.z),
            Vec3::new(l.x, h.y, h.z),
            Vec3::new(h.x, h.y, h.z),
        ]
    }

    /// Whether the segment from `a` to `b` passes through the box (slab test).
    fn meets_segment(&self, a: Vec3, b: Vec3) -> bool {
        let d = b - a;
        let (mut enter, mut exit) = (0.0_f64, 1.0_f64);
        for (o, v, lo, hi) in [
            (a.x, d.x, self.low.x, self.high.x),
            (a.y, d.y, self.low.y, self.high.y),
            (a.z, d.z, self.low.z, self.high.z),
        ] {
            if v.abs() < 1e-15 {
                if o < lo || o > hi {
                    return false;
                }
            } else {
                let (t0, t1) = ((lo - o) / v, (hi - o) / v);
                enter = enter.max(t0.min(t1));
                exit = exit.min(t0.max(t1));
                if enter > exit {
                    return false;
                }
            }
        }
        true
    }
}

/// A solid's faces as placed triangles, tessellated once.
struct Shape<'a> {
    solid: &'a Solid,
    triangles: Vec<[Vec3; 3]>,
    bounds: Bounds,
}

impl<'a> Shape<'a> {
    fn of(solid: &'a Solid, fineness: Fineness) -> Option<Self> {
        let mut triangles = Vec::new();
        for face in &solid.faces {
            let mesh = face.geometry.tessellate(fineness);
            let placed: Vec<Vec3> = mesh
                .positions
                .iter()
                .map(|p| face.placement.apply_point(*p))
                .collect();
            for t in &mesh.triangles {
                triangles.push(t.map(|i| placed[i as usize]));
            }
        }
        let bounds = Bounds::of(triangles.iter().flatten().copied())?;
        Some(Self {
            solid,
            triangles,
            bounds,
        })
    }

    /// Points on its surface: the triangles subdivided to `spacing`, kept where `keep`
    /// says.
    fn points(&self, spacing: f64, keep: &dyn Fn(Vec3) -> bool) -> Vec<Vec3> {
        let mut points = Vec::new();
        for &[a, b, c] in &self.triangles {
            let longest = (b - a).length().max((c - b).length()).max((a - c).length());
            let n = (longest / spacing).ceil().max(1.0) as usize;
            for i in 0..=n {
                for j in 0..=(n - i) {
                    let (u, v) = (i as f64 / n as f64, j as f64 / n as f64);
                    let p = a + (b - a) * u + (c - a) * v;
                    if keep(p) {
                        points.push(p);
                    }
                }
            }
        }
        points
    }
}

/// Every hit of the segment from `a` to `b` with a solid's faces: (fraction along the
/// segment, cosine of the angle between the segment and the outward normal; negative
/// entering, positive leaving), sorted.
fn hits(solid: &Solid, a: Vec3, b: Vec3) -> Vec<(f64, f64)> {
    let along = b - a;
    let length = along.length();
    if length < 1e-12 {
        return Vec::new();
    }
    let direction = along * (1.0 / length);
    let mut out = Vec::new();
    for face in &solid.faces {
        let local = face.placement.inverse();
        let (o, d) = (local.apply_point(a), local.apply_vector(direction));
        let mut minimum = 0.0;
        while let Some(hit) = face.geometry.intersect(o, d, minimum) {
            if hit.distance > length {
                break;
            }
            let normal = face.placement.apply_vector(hit.normal);
            out.push((hit.distance / length, normal.dot(direction)));
            minimum = hit.distance + 1e-9 * (1.0 + hit.distance);
        }
    }
    out.sort_by(|x, y| x.0.total_cmp(&y.0));
    out
}

/// The event logic along a path: hits as (change, cosine) in the order met, from a start
/// inside (`contact`) or not; the change at which the point is `tolerance` deep, given the
/// path length per unit change. `None` if it never is.
fn deep_after(
    events: impl Iterator<Item = (f64, f64, f64)>,
    mut contact: bool,
    tolerance: f64,
    forward: bool,
    end: f64,
) -> Option<f64> {
    let beyond = |x: f64, y: f64| if forward { x > y } else { x < y };
    let mut deep_at: Option<f64> = None;
    for (change, cosine, per_change) in events {
        if let Some(deep) = deep_at {
            if !beyond(deep, change) {
                return Some(deep);
            }
        }
        if cosine > 0.0 {
            // Leaving: a contact ends; an entry that never got deep only grazed.
            contact = false;
            deep_at = None;
        } else if !contact {
            let along = tolerance / (-cosine).max(1e-6) / per_change.max(1e-300);
            deep_at = Some(if forward {
                change + along
            } else {
                change - along
            });
        }
    }
    deep_at.filter(|deep| !beyond(*deep, end))
}

/// Along a polyline path (the change at each point, the first where the motion starts),
/// the change at which the point is `tolerance` deep in `solid`.
fn along_path(solid: &Solid, path: &[(f64, Vec3)], tolerance: f64, reach: f64) -> Option<f64> {
    let (c_start, start) = path[0];
    let (c_end, _) = *path.last()?;
    let first = path.get(1)?.1 - start;
    if first.length() < 1e-12 {
        return None;
    }
    let towards = first * (1.0 / first.length());
    // Inside at the start: the nearest hit of a long ray leaves the solid.
    let contact = hits(solid, start, start + towards * reach)
        .first()
        .is_some_and(|h| h.1 > 0.0);
    let mut events = Vec::new();
    for window in path.windows(2) {
        let ((c0, a), (c1, b)) = (window[0], window[1]);
        let per_change = (b - a).length() / (c1 - c0).abs().max(1e-300);
        for (fraction, cosine) in hits(solid, a, b) {
            events.push((c0 + (c1 - c0) * fraction, cosine, per_change));
        }
    }
    deep_after(
        events.into_iter(),
        contact,
        tolerance,
        c_end > c_start,
        c_end,
    )
}

/// The interval of the line `q + direction · s` inside a box, if it meets it.
fn line_in_box(q: Vec3, direction: Vec3, b: &Bounds) -> Option<(f64, f64)> {
    let (mut enter, mut exit) = (f64::NEG_INFINITY, f64::INFINITY);
    for (o, v, lo, hi) in [
        (q.x, direction.x, b.low.x, b.high.x),
        (q.y, direction.y, b.low.y, b.high.y),
        (q.z, direction.z, b.low.z, b.high.z),
    ] {
        if v.abs() < 1e-15 {
            if o < lo || o > hi {
                return None;
            }
        } else {
            let (t0, t1) = ((lo - o) / v, (hi - o) / v);
            enter = enter.max(t0.min(t1));
            exit = exit.min(t0.max(t1));
        }
    }
    (enter <= exit).then_some((enter, exit))
}

/// For points that move along the unit `direction` by the change (a slide), the smallest
/// change at which one of them is `tolerance` deep in `solid`, if below `bound` (the
/// smallest found so far, shared between jobs). Points on one line parallel to the motion
/// share one ray, cut to the other part's box `near`: along a rod, across a plate's edge.
fn along_lines(
    solid: &Solid,
    points: &[Vec3],
    direction: Vec3,
    tolerance: f64,
    near: &Bounds,
    bound: &dyn Fn() -> f64,
) -> Option<f64> {
    let key = |p: Vec3| {
        let q = p - direction * p.dot(direction);
        let r = |x: f64| (x * 1e6).round() as i64;
        (r(q.x), r(q.y), r(q.z))
    };
    let mut lines: HashMap<(i64, i64, i64), (Vec3, Vec<f64>)> = HashMap::new();
    for p in points {
        let s = p.dot(direction);
        lines
            .entry(key(*p))
            .or_insert_with(|| (*p - direction * s, Vec::new()))
            .1
            .push(s);
    }
    let mut best: Option<f64> = None;
    for (q, starts) in lines.values() {
        let Some((enter, exit)) = line_in_box(*q, direction, near) else {
            continue;
        };
        let end = best.unwrap_or(f64::INFINITY).min(bound());
        let low = starts.iter().copied().fold(f64::INFINITY, f64::min);
        let high = starts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        // Hits exist only inside the box: from its entry (to see what each start is
        // inside) to the last start's end, or the box's exit.
        let to = exit.min(high + end);
        if to <= enter || exit < low {
            continue;
        }
        let (a, b) = (*q + direction * enter, *q + direction * to);
        let length = (b - a).length();
        let line: Vec<(f64, f64)> = hits(solid, a, b)
            .into_iter()
            .map(|(f, c)| (enter + f * length, c))
            .collect();
        if line.is_empty() {
            continue;
        }
        for &s in starts {
            let end = best.unwrap_or(f64::INFINITY).min(bound());
            // Inside at the start: the last hit behind it entered.
            let contact = line
                .iter()
                .rev()
                .find(|h| h.0 < s - 1e-9)
                .is_some_and(|h| h.1 < 0.0);
            let ahead = line
                .iter()
                .filter(|h| h.0 >= s - 1e-9)
                .map(|h| (h.0 - s, h.1, 1.0));
            if let Some(c) = deep_after(ahead, contact, tolerance, true, end) {
                if best.is_none_or(|b| c < b) {
                    best = Some(c);
                }
            }
        }
    }
    best
}

/// One piece of work: the points of one part against another part.
struct Job<'a> {
    /// What the points run into.
    target: &'a Solid,
    /// The other part's box.
    near: Bounds,
    /// The points, where the motion starts.
    points: Vec<Vec3>,
    /// The mover whose motion moves them (relative to the target when `inverse`).
    mover: usize,
    inverse: bool,
    /// The part named when this stops the motion: the one that stays.
    who: String,
}

/// Runs `f` on every job, on the processor's cores (not on wasm32).
fn run_all<T: Sync, R: Send>(jobs: &[T], f: &(dyn Fn(&T) -> R + Sync)) -> Vec<R> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let threads = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(jobs.len());
        if threads > 1 {
            use std::sync::atomic::{AtomicUsize, Ordering};
            let next = AtomicUsize::new(0);
            let mut done: Vec<(usize, R)> = std::thread::scope(|scope| {
                let workers: Vec<_> = (0..threads)
                    .map(|_| {
                        scope.spawn(|| {
                            let mut out = Vec::new();
                            loop {
                                let k = next.fetch_add(1, Ordering::Relaxed);
                                let Some(job) = jobs.get(k) else { break };
                                out.push((k, f(job)));
                            }
                            out
                        })
                    })
                    .collect();
                workers
                    .into_iter()
                    .flat_map(|w| w.join().unwrap_or_default())
                    .collect()
            });
            done.sort_by_key(|d| d.0);
            return done.into_iter().map(|d| d.1).collect();
        }
    }
    jobs.iter().map(f).collect()
}

impl Scene {
    /// A layout's items as simple solids for the clearance (see the module), each keyed
    /// by its table and id (`element:L1`).
    pub fn stand_ins(&self) -> Vec<Solid> {
        let solid = |id: &str, key: String, faces: Vec<SolidFace>| Solid {
            id: id.to_owned(),
            faces,
            role: SolidRole::Absorber,
            occurrence: key,
        };
        let mut out = Vec::new();
        for e in &self.elements {
            let f = &e.frame;
            let key = format!("element:{}", e.id);
            let faces = match &e.kind {
                ElementKind::Lens {
                    surfaces,
                    thicknesses,
                    diameter,
                    ..
                } => {
                    let r = 0.5 * diameter;
                    let length: f64 = thicknesses.iter().sum();
                    let first = surfaces.first().map_or(0.0, |s| sag(s.radius, r));
                    let last = surfaces.last().map_or(0.0, |s| sag(s.radius, r));
                    disc(f, r, first.min(0.0), (length + last).max(length))
                }
                ElementKind::Mirror { diameter, .. } => disc(f, 0.5 * diameter, -6.0, 0.0),
                ElementKind::IdealLens { diameter, .. }
                | ElementKind::Objective { diameter, .. } => disc(f, 0.5 * diameter, -1.0, 1.0),
                _ => continue,
            };
            out.push(solid(&e.id, key, faces));
        }
        for d in &self.detectors {
            let (w, h) = (0.5 * d.size.0, 0.5 * d.size.1);
            out.push(solid(
                &d.id,
                format!("detector:{}", d.id),
                slab(&d.frame, w, h, -1.0, 0.0),
            ));
        }
        for p in &self.proxies {
            let f = &p.frame;
            let faces = match p.kind {
                ProxyKind::Tube {
                    inner_diameter,
                    length,
                } => {
                    let (inner, outer) = (0.5 * inner_diameter, 0.5 * inner_diameter + 1.0);
                    vec![
                        around(f, outer, 0.0, length, false),
                        around(f, inner, 0.0, length, true),
                        across(
                            f,
                            0.0,
                            -1.0,
                            vec![circle(f, outer, 0.0), circle(f, inner, 0.0)],
                        ),
                        across(
                            f,
                            length,
                            1.0,
                            vec![circle(f, outer, length), circle(f, inner, length)],
                        ),
                    ]
                }
                ProxyKind::Plate {
                    hole_diameter,
                    size,
                } => {
                    let (half, hole) = (0.5 * size, 0.5 * hole_diameter);
                    let mut faces = slab(f, half, half, -1.0, 1.0);
                    // The two broad faces get the hole; the bore lines it.
                    faces[0] = across(
                        f,
                        -1.0,
                        -1.0,
                        vec![rectangle(f, half, half, -1.0), circle(f, hole, -1.0)],
                    );
                    faces[1] = across(
                        f,
                        1.0,
                        1.0,
                        vec![rectangle(f, half, half, 1.0), circle(f, hole, 1.0)],
                    );
                    faces.push(around(f, hole, -1.0, 1.0, true));
                    faces
                }
            };
            out.push(solid(&p.id, format!("proxy:{}", p.id), faces));
        }
        out
    }

    /// How far DOF `name` can go each way from its value now before its parts (its
    /// target's and those of the DOFs following it) overlap another part (see the
    /// module). For slides up to twice the scene's size, for tilts up to half a turn.
    pub fn clearance(&self, name: &str, settings: &Settings) -> Result<Clearance, SceneError> {
        let invalid = |m: String| SceneError::Invalid(m);
        let dof = self
            .dofs
            .iter()
            .find(|d| d.name == name)
            .ok_or_else(|| invalid(format!("no degree of freedom `{name}`")))?;
        if dof.follows.is_some() {
            return Err(invalid(format!(
                "`{name}` follows another; ask for that one"
            )));
        }
        let now: BTreeMap<String, f64> = self.dof_values(&BTreeMap::new())?;
        // Each moving target with its own DOF (its motion and ratio to the leader).
        let movers: Vec<(&Target, Motion, f64, f64)> = self
            .dofs
            .iter()
            .filter(|d| d.name == name || d.follows.as_ref().is_some_and(|(l, _)| l == name))
            .map(|d| {
                let ratio = d.follows.as_ref().map_or(1.0, |(_, r)| *r);
                (&d.target, d.motion, ratio, now[&d.name])
            })
            .collect();
        let mover_of = |s: &Solid| {
            movers
                .iter()
                .position(|(t, ..)| part_key(t) == s.occurrence)
        };
        // The move of mover `k` when the leader changes by `change`.
        let moved = |k: usize, change: f64| -> Transform {
            let (_, motion, ratio, value) = movers[k];
            motion.transform(value + ratio * change) * motion.transform(value).inverse()
        };
        // A layout's items stand in as simple solids; a CAD design has its parts.
        let stand_ins;
        let solids: &[Solid] = if self.solids.is_empty() {
            stand_ins = self.stand_ins();
            &stand_ins
        } else {
            &self.solids
        };
        let shapes: Vec<Shape> = solids
            .iter()
            .filter_map(|s| Shape::of(s, settings.fineness))
            .collect();
        let everything = Bounds::of(shapes.iter().flat_map(|s| [s.bounds.low, s.bounds.high]));
        let reach = everything
            .map_or(1000.0, |b| (b.high - b.low).length())
            .max(10.0)
            * 2.0;
        let moving: Vec<(usize, &Shape, Vec<Vec3>)> = shapes
            .iter()
            .filter_map(|s| Some((mover_of(s.solid)?, s, s.points(settings.spacing, &|_| true))))
            .collect();
        if moving.is_empty() {
            return Err(invalid(format!(
                "`{name}` moves no part with a size to find the space around (a layout's source has none)"
            )));
        }
        // A straight path: a slide, every part moving with it alike.
        let straight = match movers.first().map(|m| m.1) {
            Some(Motion::Slide { axis }) => movers
                .iter()
                .all(|(_, m, r, _)| {
                    matches!(m, Motion::Slide { axis: a } if (*a - axis).length() < 1e-12)
                        && (*r - 1.0).abs() < 1e-12
                })
                .then_some(axis),
            _ => None,
        };
        let (limit, step) = match dof.motion {
            Motion::Tilt { pivot, .. } => {
                let r = moving
                    .iter()
                    .flat_map(|(_, s, _)| s.bounds.corners())
                    .map(|c| (c - pivot).length())
                    .fold(1.0_f64, f64::max);
                (std::f64::consts::PI, settings.spacing / r)
            }
            _ => (reach, reach),
        };
        let mut range = (f64::NEG_INFINITY, f64::INFINITY);
        let mut stopped_by: (Option<String>, Option<String>) = (None, None);
        for sign in [1.0, -1.0] {
            let n = (limit / step).ceil().max(1.0) as usize;
            let grid: Vec<f64> = (0..=n)
                .map(|i| sign * limit * i as f64 / n as f64)
                .collect();
            // The region the moving parts sweep through.
            let Some(swept) = Bounds::of(moving.iter().flat_map(|(k, s, _)| {
                grid.iter()
                    .flat_map(move |c| s.bounds.corners().map(|q| moved(*k, *c).apply_point(q)))
            }))
            .map(|b| b.grown(settings.tolerance + 1e-6)) else {
                continue;
            };
            let mut jobs: Vec<Job> = Vec::new();
            for other in shapes.iter().filter(|s| mover_of(s.solid).is_none()) {
                if !(other.bounds.low.x <= swept.high.x
                    && other.bounds.high.x >= swept.low.x
                    && other.bounds.low.y <= swept.high.y
                    && other.bounds.high.y >= swept.low.y
                    && other.bounds.low.z <= swept.high.z
                    && other.bounds.high.z >= swept.low.z)
                {
                    continue;
                }
                let near = other.bounds.grown(settings.tolerance);
                // On a straight path, a point matters only if its line, ahead of it,
                // passes through the other part's box.
                let ahead = |p: Vec3, direction: Vec3, b: &Bounds| match straight {
                    None => true,
                    Some(_) => line_in_box(p, direction, b).is_some_and(|(_, exit)| exit >= 0.0),
                };
                let forward = straight.map(|axis| axis * sign);
                // This part's points in the swept region, sampled once.
                let theirs = other.points(settings.spacing, &|p| swept.contains(p));
                for (k, part, points) in &moving {
                    // The moving part's points into this one.
                    let mine: Vec<Vec3> = points
                        .iter()
                        .copied()
                        .filter(|p| forward.is_none_or(|d| ahead(*p, d, &near)))
                        .collect();
                    if !mine.is_empty() {
                        jobs.push(Job {
                            target: other.solid,
                            near,
                            points: mine,
                            mover: *k,
                            inverse: false,
                            who: other.solid.id.clone(),
                        });
                    }
                    // This part's points into the moving one, moving the other way.
                    let part_box = part.bounds.grown(settings.tolerance);
                    let into: Vec<Vec3> = theirs
                        .iter()
                        .copied()
                        .filter(|q| forward.is_none_or(|d| ahead(*q, -d, &part_box)))
                        .collect();
                    if !into.is_empty() {
                        jobs.push(Job {
                            target: part.solid,
                            near: part_box,
                            points: into,
                            mover: *k,
                            inverse: true,
                            who: other.solid.id.clone(),
                        });
                    }
                }
            }
            // Nearest parts first: the limit they set cuts the others' rays short.
            let centre = |b: &Bounds| (b.low + b.high) * 0.5;
            let from = Bounds::of(
                moving
                    .iter()
                    .flat_map(|(_, s, _)| [s.bounds.low, s.bounds.high]),
            )
            .map_or(Vec3::ZERO, |b| centre(&b));
            jobs.sort_by(|a, b| {
                (centre(&a.near) - from)
                    .length()
                    .total_cmp(&(centre(&b.near) - from).length())
            });
            // The smallest change found so far, shared between the threads (a non-negative
            // f64's bits order as the number does).
            let shared = std::sync::atomic::AtomicU64::new(limit.to_bits());
            let bound = || f64::from_bits(shared.load(std::sync::atomic::Ordering::Relaxed));
            let lower = |c: f64| {
                shared.fetch_min(c.abs().to_bits(), std::sync::atomic::Ordering::Relaxed);
            };
            let found = run_all(&jobs, &|job: &Job| -> Option<f64> {
                match straight {
                    Some(axis) => {
                        let direction = if job.inverse {
                            -axis * sign
                        } else {
                            axis * sign
                        };
                        let c = along_lines(
                            job.target,
                            &job.points,
                            direction,
                            settings.tolerance,
                            &job.near,
                            &bound,
                        )?;
                        lower(c);
                        Some(c * sign)
                    }
                    None => {
                        let mut best: Option<f64> = None;
                        for p in &job.points {
                            // Up to the smallest limit found so far, and one step past it.
                            let reach_now = bound();
                            let upto = grid
                                .iter()
                                .position(|c| c.abs() > reach_now)
                                .map_or(grid.len(), |i| i + 1);
                            let path: Vec<(f64, Vec3)> = grid[..upto]
                                .iter()
                                .map(|c| {
                                    let t = moved(job.mover, *c);
                                    let t = if job.inverse { t.inverse() } else { t };
                                    (*c, t.apply_point(*p))
                                })
                                .collect();
                            if !path
                                .windows(2)
                                .any(|w| job.near.meets_segment(w[0].1, w[1].1))
                            {
                                continue;
                            }
                            if let Some(c) =
                                along_path(job.target, &path, settings.tolerance, reach)
                            {
                                if best.is_none_or(|b| c.abs() < b.abs()) {
                                    best = Some(c);
                                    lower(c);
                                }
                            }
                        }
                        best
                    }
                }
            });
            let mut best = sign * limit;
            let mut by: Option<String> = None;
            for (job, change) in jobs.iter().zip(found) {
                if let Some(c) = change.filter(|c| c.abs() < best.abs()) {
                    best = c;
                    by = Some(job.who.clone());
                }
            }
            let value = now[name];
            if sign > 0.0 {
                range.1 = value + best;
                stopped_by.1 = by;
            } else {
                range.0 = value + best;
                stopped_by.0 = by;
            }
        }
        Ok(Clearance { range, stopped_by })
    }
}
