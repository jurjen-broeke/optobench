//! Gaussian beams through a traced system (M9c): the complex beam parameter q of a
//! Gaussian source carried to every element and detector its axis reaches.
//!
//! The system's paraxial ray-transfer (ABCD) matrix from the source to each plane comes
//! from the trace itself: the chief ray and four neighbours (offset and tilted a little in
//! each of the source's transverse directions) give the heights and slopes there, so it
//! holds for any optics the trace knows (real lenses, mirrors, an ideal objective, layers
//! of water). With real (not reduced) slopes, q transforms as q' = (A q + B) / (C q + D)
//! (H. Kogelnik and T. Li, Appl. Opt. 5, 1550 (1966)), with 1 / q = 1 / R − i λ / (π w²)
//! and λ the wavelength in the medium. A beam of quality M² propagates as a Gaussian of
//! wavelength M² λ (A. E. Siegman, Lasers, 1986, §17.6). Sources are taken to have their
//! waist at the source plane, as the trace launches them.

use optobench_math::Vec3;
use optobench_scene::{Built, Owner, Scene, SourceKind};
use optobench_trace::Ray;
use optobench_trace::nonsequential::{End, RayRecord, Role};

/// The beam at one plane, per transverse direction (x, y of the plane's frame).
#[derive(Debug, Clone, PartialEq)]
pub struct BeamAt {
    /// The element or detector id.
    pub id: String,
    /// `element` or `detector`.
    pub kind: &'static str,
    /// 1/e² intensity radius w (mm).
    pub radius: (f64, f64),
    /// Wavefront radius of curvature R (mm; infinite at a waist; positive diverging).
    pub curvature: (f64, f64),
    /// The waist radius this beam has or will have (mm).
    pub waist: (f64, f64),
    /// Distance from the plane to that waist along the beam (mm; negative: behind).
    pub to_waist: (f64, f64),
    /// Refractive index where the beam arrives.
    pub index: f64,
    /// For an objective: w / (f · NA), the fill of its pupil (mean over x and y).
    pub fill: Option<f64>,
    /// Path length along the chief ray from the source to the plane (mm).
    pub distance: f64,
    /// Length of the last straight stretch before the plane, in the medium the beam
    /// arrives through (mm): where the beam is free space before it.
    pub free: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    fn div(self, o: Complex) -> Complex {
        let d = o.re * o.re + o.im * o.im;
        Complex {
            re: (self.re * o.re + self.im * o.im) / d,
            im: (self.im * o.re - self.re * o.im) / d,
        }
    }
}

/// Where a record meets a plane: position and incoming direction (scene frame) and the
/// index of the medium it arrives through.
#[derive(Debug, Clone, Copy)]
struct Arrival {
    point: Vec3,
    direction: Vec3,
    index: f64,
    distance: f64,
    free: f64,
}

/// The arrival at the `occurrence`-th hit on `face` (the departure of an objective not
/// counted), or at the detector the ray ended on.
fn arrival(record: &RayRecord, face: Option<(usize, usize)>, built: &Built) -> Option<Arrival> {
    // Path length to point k + 1, and the last stretch before it.
    let along = |k: usize| -> (f64, f64) {
        let total = (0..=k)
            .map(|m| (record.points[m + 1] - record.points[m]).length())
            .sum();
        (total, (record.points[k + 1] - record.points[k]).length())
    };
    let index_of = |k: usize| {
        let length = (record.points[k + 1] - record.points[k]).length();
        if length > 0.0 {
            (record.optical_paths[k + 1] - record.optical_paths[k]) / length
        } else {
            1.0
        }
    };
    match face {
        Some((face, occurrence)) => {
            let mut seen = 0;
            for (k, &f) in record.faces.iter().enumerate() {
                let departure =
                    k > 0 && record.faces[k - 1] == f && built.system.faces[f].role.records_twice();
                if f != face || departure {
                    continue;
                }
                if seen == occurrence {
                    let (distance, free) = along(k);
                    return Some(Arrival {
                        point: record.points[k + 1],
                        direction: (record.points[k + 1] - record.points[k]).normalized()?,
                        index: index_of(k),
                        distance,
                        free,
                    });
                }
                seen += 1;
            }
            None
        }
        None => {
            let End::Detected { .. } = record.end else {
                return None;
            };
            let k = record.points.len().checked_sub(2)?;
            let (distance, free) = along(k);
            Some(Arrival {
                point: record.ray.position,
                direction: record.ray.direction,
                index: index_of(k),
                distance,
                free,
            })
        }
    }
}

/// A plane the beam is reported at: id, kind, frame, the face and its occurrence along
/// the chief ray (none for the detector it ends on), and an objective's pupil radius.
type Plane = (
    String,
    &'static str,
    optobench_math::Frame,
    Option<(usize, usize)>,
    Option<f64>,
);

/// The beam of Gaussian source `source` (index into the scene's sources) at every
/// element and detector its chief ray reaches, in order.
pub fn gaussian(scene: &Scene, source: usize) -> Result<Vec<BeamAt>, String> {
    let src = scene.sources.get(source).ok_or("no such source")?;
    let SourceKind::Gaussian {
        waist_diameter,
        m_squared,
        ..
    } = src.kind
    else {
        return Err(format!("source `{}` is not a Gaussian beam", src.id));
    };
    let built = scene.build().map_err(|e| e.to_string())?;
    let wavelength = src.wavelength * 1e-3; // mm
    let n_source = scene
        .surrounding
        .material()
        .refractive_index(src.wavelength)
        .map_err(|e| e.to_string())?;
    let w0 = waist_diameter / 2.0;
    let q0 = Complex {
        re: 0.0,
        im: core::f64::consts::PI * n_source * w0 * w0 / (m_squared * wavelength),
    };

    // The chief ray and its neighbours.
    let [ax, ay, az] = src.frame.axes();
    let origin = src.frame.origin();
    let (dh, du) = (1e-4, 1e-6);
    let launch = |p: Vec3, d: Vec3| {
        Ray::new(p, d, src.wavelength, 1.0).ok_or("a source axis of zero length".to_owned())
    };
    let rays = [
        launch(origin, az)?,
        launch(origin + ax * dh, az)?,
        launch(origin, az + ax * du)?,
        launch(origin + ay * dh, az)?,
        launch(origin, az + ay * du)?,
    ];
    let records: Vec<RayRecord> = rays.iter().map(|r| built.system.trace(*r)).collect();
    let chief = &records[0];

    // The planes: each element the chief ray meets (its first face there), then the
    // detector it ends on.
    let mut planes: Vec<Plane> = Vec::new();
    let mut counted: Vec<usize> = Vec::new();
    for (k, &f) in chief.faces.iter().enumerate() {
        let face = &built.system.faces[f];
        let departure = k > 0 && chief.faces[k - 1] == f && face.role.records_twice();
        if departure {
            continue;
        }
        let occurrence = counted.iter().filter(|&&c| c == f).count();
        counted.push(f);
        if let Some(Owner::Element(e)) = built.owners.get(face.owner) {
            let element = &scene.elements[*e];
            if planes.iter().any(|p| p.0 == element.id) {
                continue;
            }
            let pupil = match &face.role {
                Role::Objective { objective, .. } => Some(objective.pupil_radius()),
                _ => None,
            };
            planes.push((
                element.id.clone(),
                "element",
                element.frame,
                Some((f, occurrence)),
                pupil,
            ));
        }
    }
    if let End::Detected { detector, .. } = chief.end {
        let d = &scene.detectors[detector];
        planes.push((d.id.clone(), "detector", d.frame, None, None));
    }

    let mut out = Vec::new();
    for (id, kind, frame, face, pupil) in planes {
        let at: Vec<Arrival> = match records
            .iter()
            .map(|r| arrival(r, face, &built))
            .collect::<Option<Vec<_>>>()
        {
            Some(a) => a,
            None => break, // a neighbour went elsewhere: beyond the paraxial region
        };
        let local = |a: &Arrival| {
            let p = frame.to_local_point(a.point);
            let d = frame.to_local_vector(a.direction);
            (p.x, p.y, d.x / d.z, d.y / d.z)
        };
        let c = local(&at[0]);
        let n = at[0].index;
        // Per source direction: the response in the plane's x and y; the plane's axis that
        // responds is the one that direction maps to (folds may turn or flip it).
        let mut per_axis = [(0.0, 0.0, 0.0, 0.0); 2];
        let mut axis_of = [0usize; 2];
        for (s, (height, angle)) in [(1, 2), (3, 4)].into_iter().enumerate() {
            let h = local(&at[height]);
            let u = local(&at[angle]);
            // Heights and slopes of both neighbours: at a focus the offset ray's height
            // does not change (A = 0), so heights alone cannot tell.
            let response = |dx: f64, dy: f64| dx.abs() + dy.abs();
            let in_x = response(h.0 - c.0, u.0 - c.0) / dh + response(h.2 - c.2, u.2 - c.2) / du;
            let in_y = response(h.1 - c.1, u.1 - c.1) / dh + response(h.3 - c.3, u.3 - c.3) / du;
            let responds_in_x = in_x >= in_y;
            let t = if responds_in_x { 0 } else { 1 };
            axis_of[s] = t;
            let pick = |v: (f64, f64, f64, f64), slope: bool| match (t, slope) {
                (0, false) => v.0 - c.0,
                (0, true) => v.2 - c.2,
                (_, false) => v.1 - c.1,
                (_, true) => v.3 - c.3,
            };
            per_axis[s] = (
                pick(h, false) / dh,
                pick(u, false) / du,
                pick(h, true) / dh,
                pick(u, true) / du,
            );
        }
        let mut radius = [0.0; 2];
        let mut curvature = [0.0; 2];
        let mut waist = [0.0; 2];
        let mut to_waist = [0.0; 2];
        for s in 0..2 {
            let (a, b, cc, d) = per_axis[s];
            let q = Complex {
                re: a * q0.re + b,
                im: a * q0.im,
            }
            .div(Complex {
                re: cc * q0.re + d,
                im: cc * q0.im,
            });
            let inverse = Complex { re: 1.0, im: 0.0 }.div(q);
            let t = axis_of[s];
            radius[t] = (-m_squared * wavelength / (core::f64::consts::PI * n * inverse.im))
                .max(0.0)
                .sqrt();
            curvature[t] = if inverse.re.abs() > 1e-300 {
                1.0 / inverse.re
            } else {
                f64::INFINITY
            };
            waist[t] = (q.im * m_squared * wavelength / (core::f64::consts::PI * n))
                .max(0.0)
                .sqrt();
            to_waist[t] = -q.re;
        }
        let fill = pupil.map(|p| 0.5 * (radius[0] + radius[1]) / p);
        out.push(BeamAt {
            id,
            kind,
            radius: (radius[0], radius[1]),
            curvature: (curvature[0], curvature[1]),
            waist: (waist[0], waist[1]),
            to_waist: (to_waist[0], to_waist[1]),
            index: n,
            fill,
            distance: at[0].distance,
            free: at[0].free,
        });
    }
    Ok(out)
}
