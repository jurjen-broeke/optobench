//! Where traced light focuses near a detector (M9a): for focal shift and spherical
//! aberration behind an objective, through mismatched layers.
//!
//! Each ray that reached the detector is a straight line there (its last segment, in the
//! detector's medium). In the detector's frame (+z its axis):
//!
//! - its axial crossing is the z where the line comes closest to the axis (where a
//!   meridional ray meets it), listed against the ray's launch height from its source's
//!   axis (its pupil height for a collimated beam);
//! - the RMS spot radius about the power-weighted centroid, a quadratic in z, is least at
//!   z* = −Σ w a·b / Σ w b·b, with a the positions on the detector and b the slopes, both
//!   taken from their weighted means: the best focus;
//! - the paraxial focus is the axial crossing extrapolated to zero height (a fit of
//!   z(h²) through the innermost heights), the marginal focus that of the outermost.

use optobench_math::Vec3;
use optobench_scene::{Scene, SceneTrace};
use optobench_trace::nonsequential::End;

/// An axial crossing: a ray's launch height and where it crosses the detector's axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    /// Distance of the ray's launch point from its source's axis (mm).
    pub height: f64,
    /// Axial position of the crossing, from the detector plane along its axis (mm).
    pub z: f64,
    /// How close the line comes to the axis there (mm): 0 for a meridional ray.
    pub miss: f64,
    /// Power (W).
    pub power: f64,
}

/// A detected ray's last segment in the detector's frame.
struct Line {
    point: Vec3,
    direction: Vec3,
    height: f64,
    power: f64,
}

/// Where light focuses near a detector.
#[derive(Debug, Clone, PartialEq)]
pub struct Focus {
    /// Every detected ray's crossing, by increasing height.
    pub crossings: Vec<Crossing>,
    /// The axial crossing extrapolated to zero height (mm from the detector plane).
    pub paraxial: f64,
    /// The crossing of the outermost rays (mm).
    pub marginal: f64,
    /// Where the RMS spot radius is least (mm from the detector plane).
    pub best: f64,
    /// That RMS radius (mm).
    pub best_rms: f64,
    /// The steepest detected ray's angle to the axis (radians).
    pub steepest: f64,
}

/// The focus of the light that reached detector `detector` (index into the scene's
/// detectors); `None` if fewer than two rays reached it.
pub fn focus(scene: &Scene, trace: &SceneTrace, detector: usize) -> Option<Focus> {
    let frame = scene.detectors.get(detector)?.frame;
    let mut lines: Vec<Line> = Vec::new();
    for record in &trace.records {
        let End::Detected { detector: d, .. } = record.end else {
            continue;
        };
        if d != detector || record.generation > 0 {
            continue;
        }
        let point = frame.to_local_point(record.ray.position);
        let direction = frame.to_local_vector(record.ray.direction);
        if direction.z.abs() < 1e-12 {
            continue;
        }
        // The launch height from the source the ray came from (the nearest source axis).
        let launch = record.launched.position;
        let height = scene
            .sources
            .iter()
            .map(|s| {
                let q = s.frame.to_local_point(launch);
                (q.z.abs(), q.x.hypot(q.y))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(0.0, |(_, h)| h);
        lines.push(Line {
            point,
            direction,
            height,
            power: record.ray.power,
        });
    }
    if lines.len() < 2 {
        return None;
    }

    let mut crossings: Vec<Crossing> = lines
        .iter()
        .map(|line| {
            let (p, d, height, power) = (line.point, line.direction, line.height, line.power);
            let dxy = (d.x, d.y);
            let along = dxy.0 * dxy.0 + dxy.1 * dxy.1;
            let t = if along > 0.0 {
                -(p.x * dxy.0 + p.y * dxy.1) / along
            } else {
                0.0
            };
            let at = p + d * t;
            Crossing {
                height,
                z: at.z,
                miss: at.x.hypot(at.y),
                power,
            }
        })
        .collect();
    crossings.sort_by(|a, b| a.height.total_cmp(&b.height));

    // Best focus: least power-weighted RMS about the centroid, along z.
    let w: f64 = lines.iter().map(|l| l.power).sum();
    let slope = |d: Vec3| (d.x / d.z, d.y / d.z);
    let mean = |f: &dyn Fn(&Line) -> (f64, f64)| {
        lines.iter().fold((0.0, 0.0), |s, l| {
            let v = f(l);
            (s.0 + l.power * v.0 / w, s.1 + l.power * v.1 / w)
        })
    };
    let a_mean = mean(&|l| (l.point.x, l.point.y));
    let b_mean = mean(&|l| slope(l.direction));
    let (mut ab, mut bb, mut aa) = (0.0, 0.0, 0.0);
    for l in &lines {
        let a = (l.point.x - a_mean.0, l.point.y - a_mean.1);
        let s = slope(l.direction);
        let b = (s.0 - b_mean.0, s.1 - b_mean.1);
        ab += l.power * (a.0 * b.0 + a.1 * b.1);
        bb += l.power * (b.0 * b.0 + b.1 * b.1);
        aa += l.power * (a.0 * a.0 + a.1 * a.1);
    }
    let best = if bb > 0.0 { -ab / bb } else { 0.0 };
    let best_rms = ((aa + 2.0 * ab * best + bb * best * best) / w)
        .max(0.0)
        .sqrt();

    // Paraxial focus: z against h² through the rays off the axis, a line through the
    // innermost two heights (exact for z = z₀ + k h², a first-order estimate otherwise).
    let off_axis: Vec<&Crossing> = crossings.iter().filter(|c| c.height > 1e-9).collect();
    let paraxial = match off_axis.as_slice() {
        [] => crossings[0].z,
        [only] => only.z,
        list => {
            let first = list[0];
            let next = list
                .iter()
                .find(|c| c.height > first.height * (1.0 + 1e-6))
                .copied()
                .unwrap_or(first);
            if next.height > first.height {
                let (h1, h2) = (first.height * first.height, next.height * next.height);
                first.z - (next.z - first.z) * h1 / (h2 - h1)
            } else {
                first.z
            }
        }
    };
    let outer = crossings.iter().map(|c| c.height).fold(0.0, f64::max);
    let marginal_rays: Vec<&Crossing> = crossings
        .iter()
        .filter(|c| c.height > outer * (1.0 - 1e-6))
        .collect();
    let marginal =
        marginal_rays.iter().map(|c| c.z).sum::<f64>() / marginal_rays.len().max(1) as f64;
    let steepest = lines
        .iter()
        .map(|l| (l.direction.x.hypot(l.direction.y)).atan2(l.direction.z.abs()))
        .fold(0.0, f64::max);
    Some(Focus {
        crossings,
        paraxial,
        marginal,
        best,
        best_rms,
        steepest,
    })
}
