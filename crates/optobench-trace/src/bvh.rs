//! A bounding volume hierarchy over axis-aligned boxes, for nearest-hit queries.
//!
//! Built top-down by splitting at the median of the box centres along the longest axis
//! of their spread, down to leaves of a few boxes. A query visits only nodes the ray
//! enters before the nearest hit found so far.

use optobench_math::Vec3;
use optobench_surfaces::brep::Aabb;

const LEAF_SIZE: usize = 4;

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
    fn bounds(&self) -> Aabb {
        match self {
            Node::Leaf { bounds, .. } | Node::Inner { bounds, .. } => *bounds,
        }
    }
}

/// A BVH over a list of boxes; queries return indices into that list.
#[derive(Debug, Clone, Default)]
pub struct Bvh {
    nodes: Vec<Node>,
    order: Vec<usize>,
    count: usize,
}

impl Bvh {
    /// Builds the hierarchy over `boxes`.
    pub fn build(boxes: &[Aabb]) -> Self {
        let mut bvh = Self {
            nodes: Vec::new(),
            order: (0..boxes.len()).collect(),
            count: boxes.len(),
        };
        if !boxes.is_empty() {
            bvh.split(boxes, 0, boxes.len());
        }
        bvh
    }

    /// How many boxes it was built over.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether it was built over no boxes.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn split(&mut self, boxes: &[Aabb], start: usize, end: usize) -> usize {
        let bounds = self.order[start..end]
            .iter()
            .fold(Aabb::EMPTY, |b, &i| b.union(boxes[i]));
        let index = self.nodes.len();
        if end - start <= LEAF_SIZE {
            self.nodes.push(Node::Leaf {
                bounds,
                start,
                count: end - start,
            });
            return index;
        }
        let centres = self.order[start..end]
            .iter()
            .fold(Aabb::EMPTY, |b, &i| b.with(boxes[i].centre()));
        let spread = centres.max - centres.min;
        let key = |v: Vec3| {
            if spread.x >= spread.y && spread.x >= spread.z {
                v.x
            } else if spread.y >= spread.z {
                v.y
            } else {
                v.z
            }
        };
        let middle = (start + end) / 2;
        self.order[start..end].select_nth_unstable_by(middle - start, |&a, &b| {
            key(boxes[a].centre()).total_cmp(&key(boxes[b].centre()))
        });
        // Reserve this node, fill in its children afterwards.
        self.nodes.push(Node::Leaf {
            bounds,
            start,
            count: 0,
        });
        let left = self.split(boxes, start, middle);
        let right = self.split(boxes, middle, end);
        self.nodes[index] = Node::Inner {
            bounds,
            left,
            right,
        };
        index
    }

    /// Indices of the boxes that contain `point`.
    pub fn containing(&self, point: Vec3) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack = Vec::with_capacity(64);
        if !self.nodes.is_empty() {
            stack.push(0);
        }
        while let Some(node) = stack.pop() {
            match &self.nodes[node] {
                Node::Leaf {
                    bounds,
                    start,
                    count,
                } => {
                    if bounds.contains(point) {
                        out.extend(&self.order[*start..start + count]);
                    }
                }
                Node::Inner {
                    bounds,
                    left,
                    right,
                } => {
                    if bounds.contains(point) {
                        stack.push(*left);
                        stack.push(*right);
                    }
                }
            }
        }
        out
    }

    /// The nearest hit: `test(i)` returns the hit distance on box `i`'s object, if any.
    /// Returns the index and distance of the nearest one.
    pub fn nearest(
        &self,
        origin: Vec3,
        direction: Vec3,
        mut test: impl FnMut(usize) -> Option<f64>,
    ) -> Option<(usize, f64)> {
        self.nearest_within(origin, direction, |i, _| test(i))
    }

    /// As [`Bvh::nearest`], telling `test` the distance of the nearest hit so far, so it
    /// can skip work on hits that cannot be nearer.
    pub fn nearest_within(
        &self,
        origin: Vec3,
        direction: Vec3,
        mut test: impl FnMut(usize, f64) -> Option<f64>,
    ) -> Option<(usize, f64)> {
        let mut best: Option<(usize, f64)> = None;
        let mut stack = Vec::with_capacity(64);
        if !self.nodes.is_empty() {
            stack.push(0);
        }
        while let Some(node) = stack.pop() {
            let limit = best.map_or(f64::INFINITY, |(_, d)| d);
            match self.nodes[node].bounds().ray_interval(origin, direction) {
                Some((enter, exit)) if exit >= 0.0 && enter <= limit => {}
                _ => continue,
            }
            match &self.nodes[node] {
                Node::Leaf { start, count, .. } => {
                    for &i in &self.order[*start..start + count] {
                        if let Some(d) = test(i, best.map_or(f64::INFINITY, |(_, b)| b)) {
                            if d < best.map_or(f64::INFINITY, |(_, b)| b) {
                                best = Some((i, d));
                            }
                        }
                    }
                }
                Node::Inner { left, right, .. } => {
                    // Visit the nearer child first (pushed last).
                    let entry = |n: usize| {
                        self.nodes[n]
                            .bounds()
                            .ray_interval(origin, direction)
                            .map_or(f64::INFINITY, |(e, _)| e)
                    };
                    if entry(*left) <= entry(*right) {
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

    #[test]
    fn finds_the_same_nearest_box_as_a_linear_search() {
        // A row of unit cubes along x; a ray along x from the left must hit cube 0 first,
        // and from the right the last one.
        let boxes: Vec<Aabb> = (0..37)
            .map(|i| Aabb {
                min: Vec3::new(i as f64 * 2.0, 0.0, 0.0),
                max: Vec3::new(i as f64 * 2.0 + 1.0, 1.0, 1.0),
            })
            .collect();
        let bvh = Bvh::build(&boxes);
        let boxes = &boxes;
        let test = |origin: Vec3, direction: Vec3| {
            move |i: usize| {
                boxes[i]
                    .ray_interval(origin, direction)
                    .map(|(e, _)| e)
                    .filter(|e| *e >= 0.0)
            }
        };
        let from_left = (Vec3::new(-5.0, 0.5, 0.5), Vec3::X);
        assert_eq!(
            bvh.nearest(from_left.0, from_left.1, test(from_left.0, from_left.1))
                .map(|h| h.0),
            Some(0)
        );
        let from_right = (Vec3::new(100.0, 0.5, 0.5), -Vec3::X);
        assert_eq!(
            bvh.nearest(from_right.0, from_right.1, test(from_right.0, from_right.1))
                .map(|h| h.0),
            Some(36)
        );
        let miss = (Vec3::new(-5.0, 3.0, 0.5), Vec3::X);
        assert_eq!(bvh.nearest(miss.0, miss.1, test(miss.0, miss.1)), None);
    }
}
