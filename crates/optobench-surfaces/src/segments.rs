//! A face's boundary segments in a bounding-volume tree, so the segment nearest a point is
//! found in logarithmic time (M7): `BrepFace::contains` asks it for every candidate hit,
//! and faces with finely sampled edges (threads, knurls, engraving) have thousands.
//!
//! The answer is exactly the one a scan in loop order gives: the nearest segment, the
//! first in loop order among segments equally near (within 1e-24 mm²).

use optobench_math::Vec3;

use crate::brep::Aabb;

/// A segment: its loop and the index of its first point (the next point closes it).
type Segment = (usize, usize);

#[derive(Debug, Clone)]
enum Node {
    Leaf {
        bounds: Aabb,
        start: usize,
        count: usize,
    },
    Inner {
        bounds: Aabb,
        left: usize,
        right: usize,
    },
}

impl Node {
    fn bounds(&self) -> &Aabb {
        match self {
            Node::Leaf { bounds, .. } | Node::Inner { bounds, .. } => bounds,
        }
    }
}

/// The nearest segment to a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Nearest {
    /// Its loop.
    pub(crate) polyline: usize,
    /// The index of its first point in the loop.
    pub(crate) k: usize,
    /// Where along it the nearest point is (0 at its start, 1 at its end).
    pub(crate) s: f64,
    /// The squared distance.
    pub(crate) distance: f64,
}

/// The tree. Built from the loops it will be asked about; `matches` tells whether they
/// are still the same lengths.
#[derive(Debug, Clone, Default)]
pub(crate) struct Segments {
    items: Vec<Segment>,
    nodes: Vec<Node>,
    lengths: Vec<usize>,
}

/// The tree is derived from the loops, so faces with equal loops have equal trees:
/// comparing faces compares their loops, not this.
impl PartialEq for Segments {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// Derived from the loops too (see `Segments`).
impl PartialEq for PlaneGrid {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

const LEAF: usize = 8;

fn segment_box(loops: &[Vec<Vec3>], (l, k): Segment) -> Aabb {
    let polyline = &loops[l];
    Aabb::EMPTY
        .with(polyline[k])
        .with(polyline[(k + 1) % polyline.len()])
}

fn distance_to_box(p: Vec3, b: &Aabb) -> f64 {
    let axis = |v: f64, low: f64, high: f64| (low - v).max(0.0).max(v - high);
    let (x, y, z) = (
        axis(p.x, b.min.x, b.max.x),
        axis(p.y, b.min.y, b.max.y),
        axis(p.z, b.min.z, b.max.z),
    );
    x * x + y * y + z * z
}

impl Segments {
    /// The tree of every non-degenerate segment of `loops`.
    pub(crate) fn new(loops: &[Vec<Vec3>]) -> Self {
        let mut items: Vec<Segment> = Vec::new();
        for (l, polyline) in loops.iter().enumerate() {
            let n = polyline.len();
            if n < 2 {
                continue;
            }
            for k in 0..n {
                if (polyline[(k + 1) % n] - polyline[k]).length_squared() > 0.0 {
                    items.push((l, k));
                }
            }
        }
        let mut tree = Self {
            items,
            nodes: Vec::new(),
            lengths: loops.iter().map(Vec::len).collect(),
        };
        if !tree.items.is_empty() {
            let count = tree.items.len();
            tree.build(loops, 0, count);
        }
        tree
    }

    /// Whether the tree was built from loops of these lengths.
    pub(crate) fn matches(&self, loops: &[Vec<Vec3>]) -> bool {
        self.lengths.len() == loops.len()
            && self.lengths.iter().zip(loops).all(|(&n, l)| n == l.len())
    }

    fn build(&mut self, loops: &[Vec<Vec3>], start: usize, count: usize) -> usize {
        let bounds = self.items[start..start + count]
            .iter()
            .fold(Aabb::EMPTY, |b, &s| b.union(segment_box(loops, s)));
        let index = self.nodes.len();
        if count <= LEAF {
            self.nodes.push(Node::Leaf {
                bounds,
                start,
                count,
            });
            return index;
        }
        // Split at the median of the segment centres along the box's longest side.
        let size = bounds.max - bounds.min;
        let axis = |v: Vec3| {
            if size.x >= size.y && size.x >= size.z {
                v.x
            } else if size.y >= size.z {
                v.y
            } else {
                v.z
            }
        };
        let centre = |&(l, k): &Segment| {
            let polyline = &loops[l];
            axis(polyline[k] + polyline[(k + 1) % polyline.len()])
        };
        self.items[start..start + count].sort_by(|a, b| centre(a).total_cmp(&centre(b)));
        self.nodes.push(Node::Leaf {
            bounds,
            start,
            count,
        });
        let half = count / 2;
        let left = self.build(loops, start, half);
        let right = self.build(loops, start + half, count - half);
        self.nodes[index] = Node::Inner {
            bounds,
            left,
            right,
        };
        index
    }

    /// The segment nearest `point`, as a scan of the loops in order would find it.
    pub(crate) fn nearest(&self, loops: &[Vec<Vec3>], point: Vec3) -> Option<Nearest> {
        let mut best: Option<Nearest> = None;
        let mut stack = Vec::with_capacity(32);
        if !self.nodes.is_empty() {
            stack.push(0);
        }
        let earlier = |a: &Nearest, b: &Nearest| (a.polyline, a.k) < (b.polyline, b.k);
        while let Some(node) = stack.pop() {
            let limit = best.map_or(f64::INFINITY, |b| b.distance + 1e-24);
            if distance_to_box(point, self.nodes[node].bounds()) > limit {
                continue;
            }
            match &self.nodes[node] {
                Node::Leaf { start, count, .. } => {
                    for &(l, k) in &self.items[*start..start + count] {
                        let polyline = &loops[l];
                        let a = polyline[k];
                        let ab = polyline[(k + 1) % polyline.len()] - a;
                        let s = ((point - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
                        let distance = (point - (a + ab * s)).length_squared();
                        let candidate = Nearest {
                            polyline: l,
                            k,
                            s,
                            distance,
                        };
                        let better = match &best {
                            None => true,
                            Some(b) => {
                                distance < b.distance - 1e-24
                                    || (distance <= b.distance + 1e-24 && earlier(&candidate, b))
                            }
                        };
                        if better {
                            best = Some(candidate);
                        }
                    }
                }
                Node::Inner { left, right, .. } => {
                    // The nearer child first (pushed last).
                    let (dl, dr) = (
                        distance_to_box(point, self.nodes[*left].bounds()),
                        distance_to_box(point, self.nodes[*right].bounds()),
                    );
                    if dl <= dr {
                        stack.push(*right);
                        stack.push(*left);
                    } else {
                        stack.push(*left);
                        stack.push(*right);
                    }
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scan in loop order, as `contains` did before the tree.
    fn scan(loops: &[Vec<Vec3>], point: Vec3) -> Option<(usize, usize, f64)> {
        let mut best: Option<(usize, usize, f64)> = None;
        for (l, polyline) in loops.iter().enumerate() {
            let n = polyline.len();
            for k in 0..n {
                let a = polyline[k];
                let ab = polyline[(k + 1) % n] - a;
                if ab.length_squared() == 0.0 {
                    continue;
                }
                let s = ((point - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
                let d = (point - (a + ab * s)).length_squared();
                if best.is_none_or(|b| d < b.2 - 1e-24) {
                    best = Some((l, k, d));
                }
            }
        }
        best
    }

    #[test]
    fn finds_the_segment_a_scan_finds() {
        // A finely sampled wavy ring (outer loop) and a square hole (inner loop), with a
        // repeated point; points all over and on the boundary.
        let outer: Vec<Vec3> = (0..2000)
            .map(|i| {
                let a = core::f64::consts::TAU * i as f64 / 2000.0;
                let r = 10.0 + 0.3 * (12.0 * a).sin();
                Vec3::new(r * a.cos(), r * a.sin(), 0.01 * (5.0 * a).cos())
            })
            .collect();
        let mut inner = vec![
            Vec3::new(-2.0, -2.0, 0.0),
            Vec3::new(-2.0, 2.0, 0.0),
            Vec3::new(2.0, 2.0, 0.0),
            Vec3::new(2.0, -2.0, 0.0),
        ];
        inner.insert(2, inner[1]);
        let loops = vec![outer, inner];
        let tree = Segments::new(&loops);
        assert!(tree.matches(&loops));
        let mut seed = 12345u64;
        let mut random = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut points: Vec<Vec3> = (0..3000)
            .map(|_| {
                Vec3::new(
                    24.0 * random() - 12.0,
                    24.0 * random() - 12.0,
                    0.1 * random(),
                )
            })
            .collect();
        // On the boundary: vertices and midpoints.
        points.extend(loops[1].iter().copied());
        points.push(Vec3::new(0.0, 2.0, 0.0));
        points.extend(loops[0].iter().step_by(97).copied());
        for p in points {
            let expected = scan(&loops, p).expect("segments");
            let found = tree.nearest(&loops, p).expect("segments");
            assert_eq!(
                (found.polyline, found.k),
                (expected.0, expected.1),
                "at {p:?}"
            );
            assert!((found.distance - expected.2).abs() < 1e-12);
        }
        assert!(!tree.matches(&loops[..1]));
    }
}

/// A flat face's boundary in its own plane, edges binned in rows across y, for an
/// inside test by counting crossings (even–odd; outer loop and holes alike). Each test
/// looks only at the edges in the point's row. Points within [`NEAR`] of an edge are left
/// to the nearest-segment test, so the boundary classifies exactly as before.
#[derive(Debug, Clone)]
pub(crate) struct PlaneGrid {
    frame: optobench_math::Frame,
    edges: Vec<[f64; 4]>,
    rows: Vec<Vec<u32>>,
    low: f64,
    height: f64,
}

/// Within this distance of an edge (mm) the crossing count is not trusted.
const NEAR: f64 = 1e-6;

impl PlaneGrid {
    /// The grid of a flat face whose loops all lie in `frame`'s x–y plane (to 1e-9 mm);
    /// `None` otherwise.
    pub(crate) fn new(frame: optobench_math::Frame, loops: &[Vec<Vec3>]) -> Option<Self> {
        let mut edges: Vec<[f64; 4]> = Vec::new();
        for polyline in loops {
            let n = polyline.len();
            if n < 2 {
                continue;
            }
            let local: Vec<Vec3> = polyline.iter().map(|&p| frame.to_local_point(p)).collect();
            if local.iter().any(|p| p.z.abs() > 1e-9) {
                return None;
            }
            for k in 0..n {
                let (a, b) = (local[k], local[(k + 1) % n]);
                if (b - a).length_squared() > 0.0 {
                    edges.push([a.x, a.y, b.x, b.y]);
                }
            }
        }
        if edges.is_empty() {
            return None;
        }
        let low = edges
            .iter()
            .map(|e| e[1].min(e[3]))
            .fold(f64::INFINITY, f64::min)
            - NEAR;
        let high = edges
            .iter()
            .map(|e| e[1].max(e[3]))
            .fold(f64::NEG_INFINITY, f64::max)
            + NEAR;
        let count = ((edges.len() as f64).sqrt().ceil() as usize).clamp(1, 4096);
        let height = (high - low) / count as f64;
        let mut rows = vec![Vec::new(); count];
        let row = |y: f64| (((y - low) / height).floor().max(0.0) as usize).min(count - 1);
        for (i, e) in edges.iter().enumerate() {
            let (a, b) = (e[1].min(e[3]) - NEAR, e[1].max(e[3]) + NEAR);
            for bin in &mut rows[row(a)..=row(b)] {
                bin.push(i as u32);
            }
        }
        Some(Self {
            frame,
            edges,
            rows,
            low,
            height,
        })
    }

    /// Whether a point of the plane lies inside the loops; `None` within [`NEAR`] of an
    /// edge.
    pub(crate) fn classify(&self, point: Vec3) -> Option<bool> {
        let p = self.frame.to_local_point(point);
        let r = ((p.y - self.low) / self.height).floor();
        if r < 0.0 || r >= self.rows.len() as f64 {
            return Some(false);
        }
        let mut inside = false;
        for &i in &self.rows[r as usize] {
            let [x1, y1, x2, y2] = self.edges[i as usize];
            // Near an edge: leave it to the exact test.
            let (dx, dy) = (x2 - x1, y2 - y1);
            let s = (((p.x - x1) * dx + (p.y - y1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            let (qx, qy) = (x1 + dx * s - p.x, y1 + dy * s - p.y);
            if qx * qx + qy * qy < NEAR * NEAR {
                return None;
            }
            if (y1 > p.y) != (y2 > p.y) && p.x < x1 + (p.y - y1) * dx / dy {
                inside = !inside;
            }
        }
        Some(inside)
    }
}

#[cfg(test)]
mod plane_tests {
    use super::*;
    use optobench_math::Frame;

    #[test]
    fn rows_count_the_same_crossings_as_all_edges() {
        // A star-shaped loop with a triangular hole; crossings counted over every edge.
        let star: Vec<Vec3> = (0..40)
            .map(|i| {
                let a = core::f64::consts::TAU * i as f64 / 40.0;
                let r = if i % 2 == 0 { 10.0 } else { 4.0 };
                Vec3::new(r * a.cos(), r * a.sin(), 0.0)
            })
            .collect();
        let hole = vec![
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.5, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
        ];
        let loops = vec![star, hole];
        let grid = PlaneGrid::new(Frame::PARENT, &loops).expect("flat");
        let all = |p: Vec3| {
            let mut inside = false;
            for l in &loops {
                for k in 0..l.len() {
                    let (a, b) = (l[k], l[(k + 1) % l.len()]);
                    if (a.y > p.y) != (b.y > p.y)
                        && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y)
                    {
                        inside = !inside;
                    }
                }
            }
            inside
        };
        for i in 0..200 {
            for j in 0..200 {
                let p = Vec3::new(-12.0 + 0.1203 * i as f64, -12.0 + 0.1197 * j as f64, 0.0);
                if let Some(inside) = grid.classify(p) {
                    assert_eq!(inside, all(p), "at {p:?}");
                }
            }
        }
        // Next to an edge it declines; off the plane it does not apply.
        assert_eq!(grid.classify(Vec3::new(10.0, 0.0, 0.0)), None);
        let tilted = vec![vec![Vec3::ZERO, Vec3::X, Vec3::new(0.0, 1.0, 0.1)]];
        assert!(PlaneGrid::new(Frame::PARENT, &tilted).is_none());
    }
}
