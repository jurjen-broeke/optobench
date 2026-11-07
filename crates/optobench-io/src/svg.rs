//! 2D layout view as SVG: a top view (scene x, y) or a side view (scene x, z) of every
//! face and of the traced rays, coloured by how they ended.

use optobench_math::Vec3;
use optobench_scene::{Built, Owner, Scene, SceneTrace};
use optobench_trace::nonsequential::{End, Region, Shape};

/// Which projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Looking down: scene x to the right, y up.
    Top,
    /// Looking along +y: scene x to the right, z up.
    Side,
}

const WIDTH: f64 = 1000.0;

fn project(view: View, p: Vec3) -> (f64, f64) {
    match view {
        View::Top => (p.x, p.y),
        View::Side => (p.x, p.z),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Outline polylines of a face in the scene frame: cuts through the local x–z and y–z
/// planes, so a face seen edge-on draws as its profile. A cut whose direction is mostly
/// along the line of sight would collapse onto the axis and is left out.
fn outline(built: &Built, face: usize, view: View) -> Vec<Vec<Vec3>> {
    let face = &built.system.faces[face];
    let to_scene = |p: Vec3| face.placement.apply_point(p);
    let samples = 24;
    let in_view = |axis: Vec3| {
        let (u, v) = project(view, face.placement.apply_vector(axis));
        u.hypot(v)
    };
    let largest = in_view(Vec3::X).max(in_view(Vec3::Y));
    let shown = |axis: Vec3| in_view(axis) >= 0.5 * largest;
    match &face.shape {
        Shape::Cylinder { radius, length } => [(Vec3::X, *radius), (Vec3::Y, *radius)]
            .into_iter()
            .filter(|(axis, _)| shown(*axis))
            .flat_map(|(axis, r)| [axis * r, axis * -r])
            .map(|p| vec![to_scene(p), to_scene(p + Vec3::new(0.0, 0.0, *length))])
            .collect(),
        Shape::Profile(surface) => {
            // Extent along local x and y, and a hole to skip.
            let (along_x, along_y, hole) = match face.region {
                Region::Disk { radius } => (radius, radius, 0.0),
                Region::Annulus { inner, outer } => (outer, outer, inner),
                Region::Rectangle {
                    half_width,
                    half_height,
                } => (half_width, half_height, 0.0),
                Region::SquareWithHole {
                    half_size,
                    hole_radius,
                } => (half_size, half_size, hole_radius),
            };
            let mut lines = Vec::new();
            for (extent, axis) in [(along_x, Vec3::X), (along_y, Vec3::Y)] {
                if !shown(axis) {
                    continue;
                }
                for side in [-1.0, 1.0] {
                    let line: Vec<Vec3> = (0..=samples)
                        .map(|k| hole + (extent - hole) * k as f64 / samples as f64)
                        .filter_map(|s| {
                            let sag = surface.sag(s)?;
                            Some(to_scene(axis * (side * s) + Vec3::new(0.0, 0.0, sag)))
                        })
                        .collect();
                    if line.len() > 1 {
                        lines.push(line);
                    }
                }
            }
            lines
        }
    }
}

/// The layout view of a traced scene, showing at most `max_rays` rays.
pub fn layout(
    scene: &Scene,
    built: &Built,
    trace: &SceneTrace,
    view: View,
    max_rays: usize,
) -> String {
    let faces: Vec<(Owner, Vec<Vec<Vec3>>)> = (0..built.system.faces.len())
        .map(|i| {
            (
                built.owners[built.system.faces[i].owner],
                outline(built, i, view),
            )
        })
        .collect();
    let step = (trace.records.len() / max_rays.max(1)).max(1);
    let rays: Vec<(&End, Vec<Vec3>)> = trace
        .records
        .iter()
        .step_by(step)
        .map(|r| (&r.end, r.points.clone()))
        .collect();

    // Bounds over everything drawn.
    let mut points: Vec<(f64, f64)> = faces
        .iter()
        .flat_map(|(_, lines)| lines.iter().flatten())
        .chain(rays.iter().flat_map(|(_, p)| p.iter()))
        .map(|&p| project(view, p))
        .collect();
    if points.is_empty() {
        points.push((0.0, 0.0));
    }
    let (mut min_x, mut max_x, mut min_y, mut max_y) = points.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(a, b, c, d), &(x, y)| (a.min(x), b.max(x), c.min(y), d.max(y)),
    );
    let span = (max_x - min_x).max(max_y - min_y).max(1.0);
    let margin = 0.05 * span;
    min_x -= margin;
    max_x += margin;
    min_y -= margin;
    max_y += margin;
    let scale = WIDTH / (max_x - min_x);
    let height = ((max_y - min_y) * scale).max(50.0);
    let to_svg = |p: Vec3| {
        let (x, y) = project(view, p);
        ((x - min_x) * scale, height - (y - min_y) * scale)
    };
    let path = |line: &[Vec3]| -> String {
        line.iter()
            .map(|&p| {
                let (x, y) = to_svg(p);
                format!("{x:.2},{y:.2}")
            })
            .collect::<Vec<_>>()
            .join(" ")
    };

    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH:.0}\" height=\"{height:.0}\" \
         viewBox=\"0 0 {WIDTH:.0} {height:.0}\" font-family=\"sans-serif\" font-size=\"12\">\n\
         <title>optobench layout, {} view</title>\n<rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n",
        match view {
            View::Top => "top",
            View::Side => "side",
        }
    );
    for (end, line) in &rays {
        let colour = match end {
            End::Detected { .. } => "#2b7bba",
            End::Clipped { .. } => "#e4572e",
            _ => "#9a9a9a",
        };
        svg.push_str(&format!(
            "<polyline points=\"{}\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"0.6\" stroke-opacity=\"0.7\"/>\n",
            path(line)
        ));
    }
    for (owner, lines) in &faces {
        let (colour, width) = match owner {
            Owner::Element(_) => ("#222222", 1.5),
            Owner::Proxy(_) => ("#6b4f2a", 2.0),
            Owner::Detector(_) => ("#2a9d8f", 2.5),
        };
        for line in lines {
            svg.push_str(&format!(
                "<polyline points=\"{}\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"{width}\"/>\n",
                path(line)
            ));
        }
    }
    let labels = scene
        .elements
        .iter()
        .map(|e| (&e.id, e.frame.origin()))
        .chain(scene.proxies.iter().map(|p| (&p.id, p.frame.origin())))
        .chain(scene.detectors.iter().map(|d| (&d.id, d.frame.origin())))
        .chain(scene.sources.iter().map(|s| (&s.id, s.frame.origin())));
    for (id, origin) in labels {
        let (x, y) = to_svg(origin);
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"#444444\">{}</text>\n",
            x + 4.0,
            y - 6.0,
            escape(id)
        ));
    }
    svg.push_str("</svg>\n");
    svg
}
