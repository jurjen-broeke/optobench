//! A lens's prescription read from its glass bodies (M16): the bodies of a lens's STEP
//! file (bare glass, or a mounted lens with its housing and retaining rings), each with
//! its exact faces and its placement, give the surfaces in order along their common
//! axis, the centre thicknesses, which gaps are glass and which air, and the diameter.
//!
//! A body is glass when [`profile`] finds two optical surfaces across its axis and both
//! cover the axis: an odd number of each face's boundary loops go round it (a lens face's
//! rim does; a mount's ring-shaped face has two, its outer and inner edges). Glass bodies
//! must share one axis. Where one body's last surface touches the next one's first (within
//! [`TOUCHING`]) with the same radius, they are cemented: one surface; otherwise the gap
//! between them is air. The order puts the more strongly curved outer surface first, as
//! spec sheets do for a lens that focuses a collimated beam (R1 towards the beam); a flat
//! side never comes first unless both are flat.

use optobench_math::{Transform, Vec3};
use optobench_surfaces::brep::BrepFace;

use crate::profile;

/// Gaps below this are touching (mm).
pub const TOUCHING: f64 = 1e-3;

/// Two radii are the same cemented surface when they differ by less than this (mm).
const SAME_RADIUS: f64 = 1e-3;

/// What is between two consecutive surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gap {
    /// A glass body (the n-th found, counted from 0 in the order of the prescription).
    Glass(usize),
    /// Air between two glass bodies.
    Air,
}

/// A lens's prescription as its bodies give it, light travelling from the first surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Prescription {
    /// Signed radii (mm; infinite for flat), positive when the centre lies further along.
    pub radii: Vec<f64>,
    /// Centre thickness of each gap (mm).
    pub thickness: Vec<f64>,
    /// What each gap is.
    pub gaps: Vec<Gap>,
    /// The largest glass diameter (mm), when the bodies have rims.
    pub diameter: Option<f64>,
    /// Bodies left out as not glass (mounts, rings).
    pub left_out: usize,
}

impl Prescription {
    /// Glass bodies.
    pub fn glasses(&self) -> usize {
        self.gaps
            .iter()
            .filter(|g| matches!(g, Gap::Glass(_)))
            .count()
    }

    /// What it is: singlet, cemented doublet, air-spaced doublet, cemented triplet, …
    pub fn kind(&self) -> String {
        let glasses = self.glasses();
        let air = self.gaps.iter().any(|g| *g == Gap::Air);
        match (glasses, air) {
            (1, _) => "singlet".into(),
            (2, false) => "cemented doublet".into(),
            (2, true) => "air-spaced doublet".into(),
            (3, false) => "cemented triplet".into(),
            (3, true) => "air-spaced triplet".into(),
            (n, _) => format!("group of {n} glasses"),
        }
    }
}

/// One glass body along the common axis: its two vertices (position along the axis) and
/// radii, and its rim radius.
struct Glass {
    vertices: [f64; 2],
    radii: [f64; 2],
    rim: Option<f64>,
}

/// Whether a closed loop goes round the axis through `point` along `axis`: its winding
/// number about the axis, in the plane across it, is not zero.
fn around(loop_points: &[Vec3], point: Vec3, axis: Vec3) -> bool {
    let hint = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let u = axis.cross(hint).normalized().unwrap_or(Vec3::X);
    let v = axis.cross(u);
    let flat = |p: Vec3| {
        let d = p - point;
        (d.dot(u), d.dot(v))
    };
    let mut turned = 0.0;
    for (k, &p) in loop_points.iter().enumerate() {
        let q = loop_points[(k + 1) % loop_points.len()];
        let (a, b) = (flat(p), flat(q));
        let step = (a.0 * b.1 - a.1 * b.0).atan2(a.0 * b.0 + a.1 * b.1);
        turned += step;
    }
    turned.abs() > std::f64::consts::PI
}

/// Whether any of `faces` covers the axis through `point` along `axis`: an odd number of
/// its loops go round it (a lens face's rim does; a ring's outer and inner edges both do).
fn covers(faces: &[&BrepFace], indices: &[usize], point: Vec3, axis: Vec3) -> bool {
    indices.iter().any(|&i| {
        let rounds = faces[i]
            .loops
            .iter()
            .filter(|l| l.len() > 2 && around(l, point, axis))
            .count();
        rounds % 2 == 1
    })
}

/// The prescription of a lens from its bodies (each its faces in its own frame and its
/// placement). An error says why there is none.
pub fn prescription(bodies: &[(Vec<&BrepFace>, Transform)]) -> Result<Prescription, String> {
    let mut common: Option<(Vec3, Vec3)> = None;
    let mut glasses: Vec<Glass> = Vec::new();
    let mut left_out = 0;
    for (faces, placement) in bodies {
        let Some(p) = profile(faces) else {
            left_out += 1;
            continue;
        };
        if p.surfaces.len() != 2
            || !p
                .surfaces
                .iter()
                .zip(&p.surface_faces)
                .all(|(&(t, _), indices)| covers(faces, indices, p.origin + p.axis * t, p.axis))
        {
            left_out += 1;
            continue;
        }
        let origin = placement.apply_point(p.origin);
        let axis = placement.apply_vector(p.axis);
        let (o, a) = *common.get_or_insert((origin, axis));
        let sign = if axis.dot(a) < 0.0 { -1.0 } else { 1.0 };
        if axis.dot(a).abs() < 1.0 - 1e-6 || (origin - o).cross(a).length() > 1e-3 {
            return Err("its glass bodies do not share one axis".into());
        }
        let shift = (origin - o).dot(a);
        let mut vertices = [0.0; 2];
        let mut radii = [0.0; 2];
        for (k, &(t, r)) in p.surfaces.iter().enumerate() {
            vertices[k] = shift + sign * t;
            radii[k] = sign * r;
        }
        if vertices[0] > vertices[1] {
            vertices.swap(0, 1);
            radii.swap(0, 1);
        }
        glasses.push(Glass {
            vertices,
            radii,
            rim: p.rim_radius,
        });
    }
    if glasses.is_empty() {
        return Err("no glass body: no body has two optical faces across its axis".into());
    }
    glasses.sort_by(|a, b| a.vertices[0].total_cmp(&b.vertices[0]));
    let mut radii = vec![glasses[0].radii[0]];
    let mut thickness = Vec::new();
    let mut gaps = Vec::new();
    for (n, g) in glasses.iter().enumerate() {
        if n > 0 {
            let before = &glasses[n - 1];
            let gap = g.vertices[0] - before.vertices[1];
            let same = (g.radii[0].is_infinite() && before.radii[1].is_infinite())
                || (g.radii[0] - before.radii[1]).abs() < SAME_RADIUS;
            if gap < -TOUCHING {
                return Err("its glass bodies overlap along the axis".into());
            }
            if gap > TOUCHING || !same {
                if gap <= TOUCHING {
                    return Err(
                        "two glass bodies touch with different radii: not a cemented surface"
                            .into(),
                    );
                }
                thickness.push(gap);
                gaps.push(Gap::Air);
                radii.push(g.radii[0]);
            }
        }
        thickness.push(g.vertices[1] - g.vertices[0]);
        gaps.push(Gap::Glass(n));
        radii.push(g.radii[1]);
    }
    let diameter = glasses
        .iter()
        .filter_map(|g| g.rim)
        .fold(None, |m: Option<f64>, r| Some(m.map_or(r, |m| m.max(r))))
        .map(|r| 2.0 * r);
    // Flat is flat, whichever way the light goes.
    let radii = radii
        .into_iter()
        .map(|r| if r.is_finite() { r } else { f64::INFINITY })
        .collect();
    let mut out = Prescription {
        radii,
        thickness,
        gaps,
        diameter,
        left_out,
    };
    // The more strongly curved outer surface first.
    let curvature = |r: f64| if r.is_finite() { 1.0 / r.abs() } else { 0.0 };
    let (first, last) = (out.radii[0], out.radii[out.radii.len() - 1]);
    if curvature(last) > curvature(first) {
        out.radii = out
            .radii
            .iter()
            .rev()
            .map(|r| if r.is_finite() { -r } else { *r })
            .collect();
        out.thickness.reverse();
        out.gaps.reverse();
        let count = out.glasses();
        for g in &mut out.gaps {
            if let Gap::Glass(n) = g {
                *n = count - 1 - *n;
            }
        }
    }
    Ok(out)
}
