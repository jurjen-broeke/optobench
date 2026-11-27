//! Fields in a scene (M10c): the hybrid of rays and fields.
//!
//! A field starts at a plane: the source's own (its exact field: a Gaussian beam, or a
//! collimated beam as a uniform disk) or a detector's, where the traced rays become a
//! field (`rays`). It is propagated along that plane's axis to a target detector,
//! meeting on the way:
//!
//! - ideal lenses (`ideal_lens` elements): a change of the reference sphere, their rim
//!   an aperture;
//! - mechanics: a plate (an iris on its mount) as an aperture at its plane; a tube as
//!   apertures of its bore at its two ends and seven planes between, so light spreading
//!   onto the wall is absorbed (reflection off the wall is not followed);
//! - bodies from CAD that absorb (every untagged body): cut at nine planes through their
//!   depth, each grid row crossed by four lines whose crossings with the body's faces
//!   tell inside from outside (`Face::crossings`), so the mask follows the body's own
//!   surfaces.
//!
//! Anything else on the axis (a real lens, a mirror, an objective, a sample, a tagged
//! lens or mirror body) is the ray trace's: start the field after it, or end it before.
//!
//! The target may be parallel to the start plane or tilted about its x or its y
//! ([`Field::tilted`]); a tilted field is an end point. There the field can be coupled
//! into a single-mode fibre (`rays::coupling`; on a tilted face, the tilt costs coupling),
//! and turned back into rays that the trace carries on through the rest of the scene from
//! just past the target plane: the detectors they reach are summarised.
//!
//! Detectors between the start and the target do not stop the field: they are planes
//! to look at, not obstacles.

use optobench_math::{Complex, Frame, Vec3};
use optobench_scene::{Built, ElementKind, Owner, ProxyKind, Scene, SolidRole, SourceKind};
use optobench_trace::Ray;
use optobench_trace::nonsequential::End;

use crate::rays::{RaySample, coupling, field_from_rays, rays_from_field};
use crate::{Aperture, Field, Report, TiltAxis};

/// Where the field starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// At the source's plane, as its exact field.
    Source,
    /// At a detector's plane, from the rays traced to it.
    Detector(usize),
}

/// What to do.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The source (index into the scene's sources): a collimated or Gaussian beam.
    pub source: usize,
    /// Where the field starts.
    pub start: Start,
    /// The detector (index) the field is carried to.
    pub target: usize,
    /// Samples per side of the field's grid (even).
    pub grid: usize,
    /// The grid's width over the beam's at the start (at least 1; 2 leaves room to
    /// spread).
    pub padding: f64,
    /// Rays across the beam when the start is a detector.
    pub samples: usize,
    /// The field's polarization (Jones vector in the start plane's x, y).
    pub polarization: (Complex, Complex),
    /// A single-mode fibre at the target: its mode's 1/e² radius (mm).
    pub fibre: Option<f64>,
    /// Carry on as rays from the target.
    pub rays: bool,
    /// Rays from cells at least this fraction of the peak irradiance.
    pub threshold: f64,
}

impl Request {
    /// A request with the usual settings: 256 samples, padding 2, 128 rays across.
    pub fn new(source: usize, start: Start, target: usize) -> Self {
        Self {
            source,
            start,
            target,
            grid: 256,
            padding: 2.0,
            samples: 128,
            polarization: (Complex::ONE, Complex::ZERO),
            fibre: None,
            rays: false,
            threshold: 1e-6,
        }
    }
}

/// One thing that happened to the field.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// Where along the axis (mm from the start plane).
    pub at: f64,
    /// What: `propagate`, or an element's or proxy's id and what it did.
    pub what: String,
    /// For a propagation, how it went.
    pub report: Option<Report>,
    /// The field's power after it (W).
    pub power: f64,
}

/// What the rays from the target delivered to one detector.
#[derive(Debug, Clone, PartialEq)]
pub struct Delivered {
    /// The detector's id.
    pub id: String,
    /// Power (W).
    pub power: f64,
    /// Rays.
    pub rays: usize,
    /// Power-weighted centroid in the detector's frame (mm).
    pub centroid: (f64, f64),
    /// Power-weighted RMS radius about it (mm).
    pub rms_radius: f64,
}

/// The outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The field at the target (in the start plane's x, y).
    pub field: Field,
    /// Its power at the start (W).
    pub start_power: f64,
    /// Everything on the way, in order.
    pub steps: Vec<Step>,
    /// The fibre's coupling efficiency, if one was asked for.
    pub coupling: Option<f64>,
    /// Where the rays from the target went, by detector.
    pub delivered: Vec<Delivered>,
    /// Rays launched from the target, and how many of them were rough.
    pub launched: (usize, usize),
}

/// A ray of the launch grid: its cell (column, row) and the ray.
type Launched = ((usize, usize), Ray);

/// A square grid of rays over a source's beam: (launch cell, ray), each ray the power of
/// its cell.
fn launch(scene: &Scene, source: usize, samples: usize) -> Result<Vec<Launched>, String> {
    let src = &scene.sources[source];
    let (half, profile): (f64, Box<dyn Fn(f64) -> f64>) = match src.kind {
        SourceKind::Collimated { diameter } => {
            let a = diameter / 2.0;
            let level = 1.0 / (core::f64::consts::PI * a * a);
            (a, Box::new(move |r: f64| if r <= a { level } else { 0.0 }))
        }
        SourceKind::Gaussian { waist_diameter, .. } => {
            let w = waist_diameter / 2.0;
            let level = 2.0 / (core::f64::consts::PI * w * w);
            (
                2.0 * w,
                Box::new(move |r: f64| level * (-2.0 * r * r / (w * w)).exp()),
            )
        }
        _ => {
            return Err(format!(
                "source `{}`: a field needs a collimated or Gaussian beam",
                src.id
            ));
        }
    };
    let n = samples.max(8);
    let h = 2.0 * half / n as f64;
    let [ax, ay, az] = src.frame.axes();
    let origin = src.frame.origin();
    let mut out = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let (u, v) = (-half + (i as f64 + 0.5) * h, -half + (j as f64 + 0.5) * h);
            let level = profile(u.hypot(v));
            if level <= 0.0 {
                continue;
            }
            let ray = Ray::new(
                origin + ax * u + ay * v,
                az,
                src.wavelength,
                src.power * level * h * h,
            )
            .ok_or("a source axis of zero length")?;
            out.push(((i, j), ray));
        }
    }
    Ok(out)
}

/// The scene's elements and mechanics on the field's axis between the start (s = 0) and
/// `end`, as (s, what it does, id) in order.
enum Event {
    Lens(f64),
    Mask(Aperture),
    /// A body from CAD that absorbs, cut by the plane: `scene.solids[i]`.
    Solid(usize),
}

/// The faces of `scene.solids[solid]` in the built system.
fn solid_faces(built: &Built, solid: usize) -> Vec<usize> {
    (0..built.system.faces.len())
        .filter(|&f| built.owners.get(built.system.faces[f].owner) == Some(&Owner::Solid(solid)))
        .collect()
}

/// A solid's extent along the axis and across it (x and y ranges), from its faces' boxes.
fn extent(built: &Built, faces: &[usize], frame: &Frame) -> ((f64, f64), (f64, f64), (f64, f64)) {
    let [ex, ey, axis] = frame.axes();
    let origin = frame.origin();
    let mut along = (f64::INFINITY, f64::NEG_INFINITY);
    let mut x = along;
    let mut y = along;
    for &f in faces {
        let b = built.system.faces[f].bounds();
        for i in 0..8 {
            let corner = Vec3::new(
                if i & 1 == 0 { b.min.x } else { b.max.x },
                if i & 2 == 0 { b.min.y } else { b.max.y },
                if i & 4 == 0 { b.min.z } else { b.max.z },
            ) - origin;
            let (s, u, v) = (corner.dot(axis), corner.dot(ex), corner.dot(ey));
            along = (along.0.min(s), along.1.max(s));
            x = (x.0.min(u), x.1.max(u));
            y = (y.0.min(v), y.1.max(v));
        }
    }
    (along, x, y)
}

/// Blocks what of the field lies inside an absorbing body at distance `at` along the
/// axis. Each grid row is crossed by four lines across the plane; counting a line's
/// crossings with the body's faces tells inside from outside exactly along it, so a cell
/// keeps the open fraction of its four lines (exact along x, sampled four times in y).
fn solid_mask(field: &mut Field, built: &Built, faces: &[usize], frame: &Frame, at: f64) {
    let [ex, ey, axis] = frame.axes();
    let n = field.size();
    let dx = field.spacing();
    let (_, x_range, y_range) = extent(built, faces, frame);
    let start = field.position(0).min(x_range.0) - dx - 1.0;
    let length = field.position(n - 1).max(x_range.1) + dx + 1.0 - start;
    let base = frame.origin() + axis * at;
    let mut open = vec![1.0; n * n];
    let lines = 4;
    for r in 0..n {
        let y_row = field.position(r);
        if y_row + dx < y_range.0 || y_row - dx > y_range.1 {
            continue;
        }
        let mut blocked = vec![0.0; n];
        for k in 0..lines {
            let y = y_row - dx / 2.0 + (f64::from(k) + 0.5) * dx / f64::from(lines);
            let origin = base + ey * y + ex * start;
            let mut crossings: Vec<f64> = faces
                .iter()
                .flat_map(|&f| built.system.faces[f].crossings(origin, ex, length))
                .collect();
            crossings.sort_by(f64::total_cmp);
            crossings.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            // Inside between the first and second crossing, the third and fourth, …
            for pair in crossings.chunks_exact(2) {
                let (a, b) = (start + pair[0], start + pair[1]);
                for (c, cell) in blocked.iter_mut().enumerate() {
                    let x = field.position(c);
                    let overlap = (b.min(x + dx / 2.0) - a.max(x - dx / 2.0)).max(0.0);
                    *cell += overlap / dx / f64::from(lines);
                }
            }
        }
        for (c, b) in blocked.iter().enumerate() {
            open[r * n + c] = (1.0 - b).clamp(0.0, 1.0);
        }
    }
    let spacing = dx;
    let half = n as f64 / 2.0;
    field.modulate(|x, y| {
        let c = (x / spacing + half).round() as usize;
        let r = (y / spacing + half).round() as usize;
        Complex::real(open[r.min(n - 1) * n + c.min(n - 1)])
    });
}

fn events(
    scene: &Scene,
    built: &Built,
    frame: &Frame,
    end: f64,
    reach: f64,
) -> Result<Vec<(f64, Event, String)>, String> {
    let [ex, ey, axis] = frame.axes();
    let origin = frame.origin();
    let place = |f: &Frame| -> (f64, (f64, f64), bool) {
        let rel = f.origin() - origin;
        let s = rel.dot(axis);
        let parallel = f.axes()[2].dot(axis).abs() > 1.0 - 1e-9;
        (s, (rel.dot(ex), rel.dot(ey)), parallel)
    };
    let mut out = Vec::new();
    for e in &scene.elements {
        let (s, centre, parallel) = place(&e.frame);
        if s <= 1e-9 || s >= end - 1e-9 {
            continue;
        }
        let on_axis = centre.0.hypot(centre.1) < 1e-6;
        match &e.kind {
            ElementKind::IdealLens {
                focal_length,
                diameter,
            } if on_axis && parallel => {
                out.push((s, Event::Lens(*focal_length), e.id.clone()));
                out.push((
                    s,
                    Event::Mask(Aperture::Circle {
                        radius: diameter / 2.0,
                        centre: (0.0, 0.0),
                    }),
                    format!("{} rim", e.id),
                ));
            }
            _ => {
                return Err(format!(
                    "`{}` lies between the field's start and its target: fields pass ideal lenses (on the axis, square to it) and mechanics; start the field after it or end it before",
                    e.id
                ));
            }
        }
    }
    for p in &scene.proxies {
        let (s, centre, parallel) = place(&p.frame);
        match p.kind {
            ProxyKind::Plate {
                hole_diameter,
                size,
                ..
            } => {
                if s <= 1e-9 || s >= end - 1e-9 {
                    continue;
                }
                if !parallel {
                    return Err(format!(
                        "plate `{}` is not square to the field's axis",
                        p.id
                    ));
                }
                out.push((
                    s,
                    Event::Mask(Aperture::Plate {
                        hole: hole_diameter / 2.0,
                        half: size / 2.0,
                        centre,
                    }),
                    p.id.clone(),
                ));
            }
            ProxyKind::Tube {
                inner_diameter,
                length,
            } => {
                if !parallel {
                    return Err(format!("tube `{}` is not along the field's axis", p.id));
                }
                let direction = p.frame.axes()[2].dot(axis).signum();
                for k in 0..=8 {
                    let at = s + direction * length * f64::from(k) / 8.0;
                    if at > 1e-9 && at < end - 1e-9 {
                        out.push((
                            at,
                            Event::Mask(Aperture::Circle {
                                radius: inner_diameter / 2.0,
                                centre,
                            }),
                            format!("{} bore", p.id),
                        ));
                    }
                }
            }
        }
    }
    // Bodies from CAD: absorbers are cut at nine planes across their depth (from their
    // faces' boxes, which may reach a little beyond the body: a plane there cuts nothing);
    // a lens or a mirror in the way is the trace's.
    for (i, solid) in scene.solids.iter().enumerate() {
        let faces = solid_faces(built, i);
        if faces.is_empty() {
            continue;
        }
        let (along, x, y) = extent(built, &faces, frame);
        let across = x.1 >= -reach && x.0 <= reach && y.1 >= -reach && y.0 <= reach;
        if along.1 <= 1e-9 || along.0 >= end - 1e-9 || !across {
            continue;
        }
        if !matches!(solid.role, SolidRole::Absorber) {
            return Err(format!(
                "body `{}` lies between the field's start and its target: fields pass ideal lenses and absorbing mechanics; start the field after it or end it before",
                solid.id
            ));
        }
        let (from, to) = (along.0.max(1e-6), along.1.min(end - 1e-6));
        for k in 0..=8 {
            let at = from + (to - from) * f64::from(k) / 8.0;
            out.push((at, Event::Solid(i), solid.id.clone()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(out)
}

/// Carries a source's field to a detector through the scene (see the module's notes).
pub fn run(scene: &Scene, request: &Request) -> Result<Outcome, String> {
    let src = scene.sources.get(request.source).ok_or("no such source")?;
    if !src.spectrum.is_empty() {
        return Err(format!(
            "source `{}` has a spectrum; a field is for one wavelength",
            src.id
        ));
    }
    let target = scene
        .detectors
        .get(request.target)
        .ok_or("no such target detector")?;
    let wavelength = src.wavelength;
    let index = scene
        .surrounding
        .material()
        .refractive_index(wavelength)
        .map_err(|e| e.to_string())?;
    let padding = request.padding.max(1.0);
    let grid = request.grid;

    // The starting plane and field.
    let (frame, mut field, reference) = match request.start {
        Start::Source => {
            let frame = src.frame;
            let field = match src.kind {
                SourceKind::Gaussian {
                    waist_diameter,
                    m_squared,
                    ..
                } => {
                    if (m_squared - 1.0).abs() > 1e-9 {
                        return Err(format!(
                            "source `{}`: a field is a TEM₀₀ beam (M² = 1)",
                            src.id
                        ));
                    }
                    let w = waist_diameter / 2.0;
                    let spacing = padding * 4.0 * w / grid as f64;
                    Field::gaussian(
                        grid,
                        spacing,
                        wavelength,
                        index,
                        w,
                        0.0,
                        src.power,
                        request.polarization,
                    )
                }
                SourceKind::Collimated { diameter } => {
                    let a = diameter / 2.0;
                    let spacing = padding * diameter / grid as f64;
                    Field::plane(
                        grid,
                        spacing,
                        wavelength,
                        index,
                        src.power / (core::f64::consts::PI * a * a),
                        request.polarization,
                    )
                    .map(|mut f| {
                        f.apply(&Aperture::Circle {
                            radius: a,
                            centre: (0.0, 0.0),
                        });
                        f
                    })
                }
                _ => {
                    return Err(format!(
                        "source `{}`: a field needs a collimated or Gaussian beam",
                        src.id
                    ));
                }
            }
            .map_err(|e| e.to_string())?;
            (frame, field, 0.0)
        }
        Start::Detector(d) => {
            let detector = scene.detectors.get(d).ok_or("no such start detector")?;
            let frame = detector.frame;
            let built = scene.build().map_err(|e| e.to_string())?;
            let mut samples = Vec::new();
            for (cell, ray) in launch(scene, request.source, request.samples)? {
                let record = built.system.trace(ray);
                let End::Detected { detector: hit, .. } = record.end else {
                    continue;
                };
                if hit != d || record.generation > 0 {
                    continue;
                }
                let point = frame.to_local_point(record.ray.position);
                samples.push(RaySample {
                    cell,
                    point: (point.x, point.y),
                    direction: frame.to_local_vector(record.ray.direction),
                    power: record.ray.power,
                    path: record.optical_paths.last().copied().unwrap_or(0.0),
                    rough: false,
                });
            }
            let reach = samples
                .iter()
                .map(|s| s.point.0.abs().max(s.point.1.abs()))
                .fold(0.0, f64::max);
            if reach <= 0.0 {
                return Err(format!("no ray of `{}` reaches `{}`", src.id, detector.id));
            }
            let spacing = padding * 2.0 * reach / grid as f64;
            let (field, reference) = field_from_rays(
                &samples,
                grid,
                spacing,
                wavelength,
                index,
                request.polarization,
            )
            .map_err(|e| e.to_string())?;
            (frame, field, reference)
        }
    };
    let start_power = field.power();

    // The target: on the axis; parallel to the start plane with the same x and y, or
    // tilted about its x or its y.
    let [ex, ey, axis] = frame.axes();
    let [tx, ty, tz] = target.frame.axes();
    let rel = target.frame.origin() - frame.origin();
    let end = rel.dot(axis);
    let level = 1.0 - 1e-9;
    let tilt = if tz.dot(axis) > level && tx.dot(ex) > level && ty.dot(ey) > level {
        None
    } else if ty.dot(ey) > level && tx.dot(ey).abs() < 1e-9 {
        Some((tx.dot(axis).atan2(tx.dot(ex)), TiltAxis::Y))
    } else if tx.dot(ex) > level && ty.dot(ex).abs() < 1e-9 {
        Some((ty.dot(axis).atan2(ty.dot(ey)), TiltAxis::X))
    } else {
        return Err(format!(
            "the target `{}` must share the start plane's x and y, or be tilted about one of them",
            target.id
        ));
    };
    if (rel - axis * end).length() > 1e-6 || end <= 0.0 {
        return Err(format!(
            "the target `{}` must lie ahead on the field's axis",
            target.id
        ));
    }

    // Along the axis.
    let mut steps = Vec::new();
    let mut here = 0.0;
    let built = scene.build().map_err(|e| e.to_string())?;
    let reach = 4.0 * field.size() as f64 * field.spacing();
    for (at, event, id) in events(scene, &built, &frame, end, reach)? {
        if at > here {
            let report = field
                .propagate(at - here)
                .map_err(|e| format!("before `{id}`: {e}"))?;
            here = at;
            steps.push(Step {
                at,
                what: "propagate".into(),
                report: Some(report),
                power: field.power(),
            });
        }
        let what = match event {
            Event::Lens(f) => {
                field.lens(f);
                format!("{id}: lens, f = {f} mm")
            }
            Event::Mask(aperture) => {
                field.apply(&aperture);
                format!("{id}: aperture")
            }
            Event::Solid(i) => {
                solid_mask(&mut field, &built, &solid_faces(&built, i), &frame, at);
                format!("{id}: body")
            }
        };
        steps.push(Step {
            at,
            what,
            report: None,
            power: field.power(),
        });
    }
    if end > here {
        let report = field
            .propagate(end - here)
            .map_err(|e| format!("to `{}`: {e}", target.id))?;
        steps.push(Step {
            at: end,
            what: "propagate".into(),
            report: Some(report),
            power: field.power(),
        });
    }

    if let Some((angle, about)) = tilt {
        field = field
            .tilted(angle, about)
            .map_err(|e| format!("onto `{}`: {e}", target.id))?;
        steps.push(Step {
            at: end,
            what: format!(
                "{}: tilted {:.4}° about {}",
                target.id,
                angle.to_degrees(),
                if about == TiltAxis::X { "x" } else { "y" }
            ),
            report: None,
            power: field.power(),
        });
    }

    let coupling = request.fibre.map(|r| coupling(&field, r, (0.0, 0.0)));

    // On as rays.
    let mut delivered: Vec<Delivered> = Vec::new();
    let mut launched = (0, 0);
    if request.rays {
        // In the target's own frame (tilted or not).
        let origin = target.frame.origin() + tz * 1e-6;
        let mut sums: Vec<(f64, usize, f64, f64, f64)> =
            vec![(0.0, 0, 0.0, 0.0, 0.0); scene.detectors.len()];
        for sample in rays_from_field(&field, request.threshold, reference) {
            launched.0 += 1;
            launched.1 += usize::from(sample.rough);
            let d = sample.direction;
            let Some(mut ray) = Ray::new(
                origin + tx * sample.point.0 + ty * sample.point.1,
                tx * d.x + ty * d.y + tz * d.z,
                wavelength,
                sample.power,
            ) else {
                continue;
            };
            ray.optical_path = sample.path;
            let record = built.system.trace(ray);
            if let End::Detected { detector, .. } = record.end {
                let local = scene.detectors[detector]
                    .frame
                    .to_local_point(record.ray.position);
                let w = record.ray.power;
                let s = &mut sums[detector];
                s.0 += w;
                s.1 += 1;
                s.2 += w * local.x;
                s.3 += w * local.y;
                s.4 += w * (local.x * local.x + local.y * local.y);
            }
        }
        for (i, s) in sums.iter().enumerate() {
            if s.1 == 0 || s.0 <= 0.0 {
                continue;
            }
            let centroid = (s.2 / s.0, s.3 / s.0);
            delivered.push(Delivered {
                id: scene.detectors[i].id.clone(),
                power: s.0,
                rays: s.1,
                centroid,
                rms_radius: (s.4 / s.0 - centroid.0 * centroid.0 - centroid.1 * centroid.1)
                    .max(0.0)
                    .sqrt(),
            });
        }
    }
    Ok(Outcome {
        field,
        start_power,
        steps,
        coupling,
        delivered,
        launched,
    })
}

/// A field's irradiance (W/mm²) on a detector's pixels, row by row from −y, each from −x:
/// each field cell's power shared among the pixels it overlaps by the overlap's area, so
/// cells and pixels of different sizes make no moiré.
pub fn on_pixels(field: &Field, size: (f64, f64), pixels: (usize, usize)) -> Vec<f64> {
    let (nx, ny) = pixels;
    let (px, py) = (size.0 / nx as f64, size.1 / ny as f64);
    let mut out = vec![0.0; nx * ny];
    let n = field.size();
    let dx = field.spacing();
    // The pixels a cell [centre ± dx/2] overlaps along one axis, with the fraction of the
    // cell's width in each.
    let shares = |centre: f64, width: f64, pitch: f64, count: usize| -> Vec<(usize, f64)> {
        let (low, high) = (
            centre - dx / 2.0 + width / 2.0,
            centre + dx / 2.0 + width / 2.0,
        );
        let first = (low / pitch).floor().max(0.0) as usize;
        let last = ((high / pitch).ceil() as isize).min(count as isize);
        (first..last.max(0) as usize)
            .filter_map(|i| {
                let overlap =
                    (high.min((i + 1) as f64 * pitch) - low.max(i as f64 * pitch)).max(0.0);
                (overlap > 0.0).then_some((i, overlap / dx))
            })
            .collect()
    };
    for r in 0..n {
        let rows = shares(field.position(r), size.1, py, ny);
        if rows.is_empty() {
            continue;
        }
        for c in 0..n {
            let power = field.irradiance(r, c) * dx * dx;
            if power == 0.0 {
                continue;
            }
            for (i, wx) in shares(field.position(c), size.0, px, nx) {
                for &(j, wy) in &rows {
                    out[j * nx + i] += power * wx * wy;
                }
            }
        }
    }
    out.iter().map(|p| p / (px * py)).collect()
}
