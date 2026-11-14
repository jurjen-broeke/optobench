//! Where to put alignment irises on a table layout (M16): an output for the bench, not
//! part of the scene. The beam's last straight run before the detector, followed along
//! its central ray, gets two irises on breadboard holes as far apart as the mounts at its
//! ends leave room for, at the beam's height there: closed down, they fix the beam's line,
//! and the last two mirrors walk the beam through them (the first onto the near iris, the
//! second onto the far one, in turn).

use optobench_math::Vec3;
use optobench_scene::{Built, Owner, Scene, SceneTrace};
use optobench_trace::nonsequential::{End, RayRecord};
use serde_json::{Value, json};

/// Room left free at each end of the run, in pitches (a mirror mount, a detector).
const CLEARANCE: f64 = 1.5;

/// How near a hole's centre must be to the beam's line to count as on it, in pitches.
const ON_LINE: f64 = 0.05;

/// The table layout's pitch, from the file's text; `None` for other files.
pub(crate) fn pitch(source: &str) -> Option<f64> {
    let root: toml::Table = source.parse().ok()?;
    if root.get("layout")?.as_str()? != "table" {
        return None;
    }
    root.get("table")?.get("pitch")?.as_float().or_else(|| {
        root.get("table")?
            .get("pitch")?
            .as_integer()
            .map(|p| p as f64)
    })
}

fn owner_id(scene: &Scene, owner: Owner) -> String {
    match owner {
        Owner::Element(i) => scene.elements[i].id.clone(),
        Owner::Proxy(i) => scene.proxies[i].id.clone(),
        Owner::Detector(i) => scene.detectors[i].id.clone(),
        Owner::Solid(i) => scene.solids[i].id.clone(),
    }
}

/// The central ray of the first source that reached a detector: launched nearest its
/// axis, in the direction nearest it.
fn central<'a>(scene: &Scene, traced: &'a SceneTrace) -> Option<&'a RayRecord> {
    let source = scene.sources.first()?;
    let origin = source.frame.origin();
    let axis = source.frame.axes()[2];
    traced
        .records
        .iter()
        .filter(|r| r.generation == 0 && matches!(r.end, End::Detected { .. }))
        .min_by(|a, b| {
            let score = |r: &RayRecord| {
                let offset = (r.launched.position - origin).cross(axis).length();
                let angle = r.launched.direction.dot(axis).clamp(-1.0, 1.0).acos();
                offset + 100.0 * angle
            };
            score(a).total_cmp(&score(b))
        })
}

/// Where to put two alignment irises, or `None` when the design is not a table layout or
/// no light reached a detector.
pub(crate) fn alignment(
    source: &str,
    scene: &Scene,
    built: &Built,
    traced: &SceneTrace,
) -> Option<Value> {
    let pitch = pitch(source)?;
    let ray = central(scene, traced)?;
    let n = ray.points.len();
    if n < 2 {
        return None;
    }
    let (a, b) = (ray.points[n - 2], ray.points[n - 1]);
    let owner_of = |hit: usize| {
        let face = ray.faces.get(hit)?;
        Some(built.owners[built.system.faces[*face].owner])
    };
    // points[k] for k ≥ 1 is the hit faces[k − 1].
    let from = if n >= 3 {
        owner_of(n - 3).map(|o| owner_id(scene, o))?
    } else {
        scene.sources.first()?.id.clone()
    };
    let to = owner_of(n - 2).map(|o| owner_id(scene, o))?;
    // The mirrors the beam met, last first: the pair that steers it through the irises.
    let mirrors: Vec<String> = (0..n - 2)
        .rev()
        .filter_map(owner_of)
        .filter_map(|o| match o {
            Owner::Element(i)
                if matches!(
                    scene.elements[i].kind,
                    optobench_scene::ElementKind::Mirror { .. }
                ) =>
            {
                Some(scene.elements[i].id.clone())
            }
            _ => None,
        })
        .take(2)
        .collect();

    let run = b - a;
    let length = run.length();
    let flat = Vec3::new(run.x, run.y, 0.0);
    let clearance = CLEARANCE * pitch;
    let at = |t: f64| a + run * t;
    let iris = |t: f64, hole: (i64, i64), on_grid: bool| {
        let p = at(t);
        json!({
            "hole": [hole.0, hole.1],
            "height": round(p.z),
            "position_mm": [round(p.x), round(p.y), round(p.z)],
            "on_grid": on_grid,
            "from_start_mm": round(t * length),
        })
    };
    let mut found: Vec<(f64, (i64, i64))> = Vec::new();
    if flat.length() > 1e-9 && length > 2.0 * clearance {
        let (lo_c, hi_c) = (a.x.min(b.x) / pitch, a.x.max(b.x) / pitch);
        let (lo_r, hi_r) = (a.y.min(b.y) / pitch, a.y.max(b.y) / pitch);
        for c in lo_c.floor() as i64..=hi_c.ceil() as i64 {
            for r in lo_r.floor() as i64..=hi_r.ceil() as i64 {
                let hole = Vec3::new(c as f64 * pitch, r as f64 * pitch, 0.0);
                let from_a = Vec3::new(hole.x - a.x, hole.y - a.y, 0.0);
                let t = from_a.dot(flat) / flat.dot(flat);
                let off = (from_a - flat * t).length();
                if off <= ON_LINE * pitch
                    && t * length >= clearance
                    && (1.0 - t) * length >= clearance
                {
                    found.push((t, (c, r)));
                }
            }
        }
    }
    found.sort_by(|x, y| x.0.total_cmp(&y.0));
    let irises: Vec<Value> = match found.as_slice() {
        [] => {
            // Off the grid (a run at an angle): a third and two thirds along, each by
            // its nearest hole (a post on a clamping fork).
            if length <= 2.0 * clearance {
                Vec::new()
            } else {
                [1.0 / 3.0, 2.0 / 3.0]
                    .into_iter()
                    .map(|t| {
                        let p = at(t);
                        iris(
                            t,
                            ((p.x / pitch).round() as i64, (p.y / pitch).round() as i64),
                            false,
                        )
                    })
                    .collect()
            }
        }
        [only] => vec![iris(only.0, only.1, true)],
        [first, .., last] => [first, last]
            .into_iter()
            .map(|(t, h)| iris(*t, *h, true))
            .collect(),
    };
    let spacing = match irises.as_slice() {
        [p, q] => (q["from_start_mm"].as_f64()? - p["from_start_mm"].as_f64()?).abs(),
        _ => 0.0,
    };
    let advice = match (irises.len(), mirrors.as_slice()) {
        (0, _) => format!(
            "The run from {from} to {to} is too short for irises clear of their mounts ({:.0} mm)",
            length
        ),
        (1, _) => format!(
            "Only one hole on the run from {from} to {to} is clear of the mounts: one iris fixes a point, not the line"
        ),
        (_, [last, before]) => format!(
            "Two irises {spacing:.0} mm apart on the run from {from} to {to}: close them down, centre the beam on the near one with {before} and on the far one with {last}, and repeat until it passes both"
        ),
        _ => format!(
            "Two irises {spacing:.0} mm apart on the run from {from} to {to}: with fewer than two mirrors before them, they check the line but cannot steer it"
        ),
    };
    Some(json!({
        "segment": {
            "from": from,
            "to": to,
            "length_mm": round(length),
            "direction": [round(run.x / length), round(run.y / length), round(run.z / length)],
        },
        "steering": mirrors.iter().rev().collect::<Vec<_>>(),
        "irises": irises,
        "advice": advice,
    }))
}

fn round(x: f64) -> f64 {
    (x * 1e4).round() / 1e4
}
