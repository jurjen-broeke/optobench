//! The scene as a wireframe in 3D, for views that draw it with their own camera (the
//! user interface; M8): per item, polylines in the scene frame (mm). B-rep faces draw as
//! their boundary loops, thinned; lens surfaces as their rim and two cross-cuts; proxy
//! tubes as their end circles and four lines.

use optobench_math::Vec3;
use optobench_scene::{Built, Owner};
use optobench_trace::nonsequential::{Region, Shape};

/// Points on a rim circle.
const RIM: usize = 48;

fn circle(radius: f64, z: impl Fn(f64) -> Option<f64>) -> Vec<Vec3> {
    (0..=RIM)
        .filter_map(|i| {
            let a = core::f64::consts::TAU * i as f64 / RIM as f64;
            let (x, y) = (radius * a.cos(), radius * a.sin());
            Some(Vec3::new(x, y, z(radius)?))
        })
        .collect()
}

/// The polylines of every face, grouped by owner (in `built.owners` order), each loop
/// thinned to at most `max_points` points.
pub fn wireframe(built: &Built, max_points: usize) -> Vec<(Owner, Vec<Vec<Vec3>>)> {
    let mut items: Vec<(Owner, Vec<Vec<Vec3>>)> =
        built.owners.iter().map(|o| (*o, Vec::new())).collect();
    for face in &built.system.faces {
        let to_scene = |p: Vec3| face.placement.apply_point(p);
        let lines: Vec<Vec<Vec3>> = match &face.shape {
            Shape::Brep(brep) => brep
                .loops
                .iter()
                .map(|polyline| {
                    let step = polyline.len().div_ceil(max_points.max(2)).max(1);
                    polyline
                        .iter()
                        .step_by(step)
                        .chain(polyline.first())
                        .copied()
                        .collect()
                })
                .collect(),
            Shape::Cylinder { radius, length } => {
                let mut lines = vec![
                    circle(*radius, |_| Some(0.0)),
                    circle(*radius, |_| Some(*length)),
                ];
                for a in [0.0_f64, 0.25, 0.5, 0.75] {
                    let (x, y) = (
                        radius * (a * core::f64::consts::TAU).cos(),
                        radius * (a * core::f64::consts::TAU).sin(),
                    );
                    lines.push(vec![Vec3::new(x, y, 0.0), Vec3::new(x, y, *length)]);
                }
                lines
            }
            Shape::Profile(surface) => {
                let sag = |r: f64| surface.sag(r);
                let (outer, hole, square) = match face.region {
                    Region::Disk { radius } => (radius, 0.0, None),
                    Region::Annulus { inner, outer } => (outer, inner, None),
                    Region::Rectangle {
                        half_width,
                        half_height,
                    } => (
                        half_width.hypot(half_height),
                        0.0,
                        Some((half_width, half_height)),
                    ),
                    Region::SquareWithHole {
                        half_size,
                        hole_radius,
                    } => (
                        half_size * core::f64::consts::SQRT_2,
                        hole_radius,
                        Some((half_size, half_size)),
                    ),
                };
                let mut lines = Vec::new();
                match square {
                    None => lines.push(circle(outer, sag)),
                    Some((w, h)) => {
                        let corners = [(-w, -h), (w, -h), (w, h), (-w, h), (-w, -h)];
                        let edge: Vec<Vec3> = corners
                            .iter()
                            .filter_map(|&(x, y)| Some(Vec3::new(x, y, sag(f64::hypot(x, y))?)))
                            .collect();
                        lines.push(edge);
                    }
                }
                if hole > 0.0 {
                    lines.push(circle(hole, sag));
                }
                // Two cross-cuts through the axis show the surface's profile.
                let reach = square.map_or(outer, |(w, h)| w.min(h));
                for axis in [Vec3::X, Vec3::Y] {
                    let cut: Vec<Vec3> = (0..=24)
                        .filter_map(|k| {
                            let s = -reach + 2.0 * reach * k as f64 / 24.0;
                            if s.abs() < hole {
                                return None;
                            }
                            Some(axis * s + Vec3::new(0.0, 0.0, sag(s.abs())?))
                        })
                        .collect();
                    if cut.len() > 1 {
                        lines.push(cut);
                    }
                }
                lines
            }
        };
        if let Some(item) = items.get_mut(face.owner) {
            item.1.extend(
                lines
                    .into_iter()
                    .map(|l| l.into_iter().map(to_scene).collect()),
            );
        }
    }
    items
}
