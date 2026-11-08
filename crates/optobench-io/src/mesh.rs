//! The scene as triangles for a shaded view (the user interface; M8b): per item, a mesh in
//! the scene frame (mm). B-rep faces are tessellated from their exact surfaces and
//! boundaries (`BrepFace::tessellate`); lens and mirror surfaces as a grid over their
//! region; proxy tubes as a ring of quads.

use optobench_math::Vec3;
use optobench_scene::{Built, Owner};
use optobench_surfaces::brep::{Fineness, Mesh};
use optobench_trace::nonsequential::{Region, Shape};

/// Segments around a tube or a lens rim.
const AROUND: usize = 64;
/// Rings or rows across a lens or mirror surface.
const ACROSS: usize = 24;

/// A mesh from a grid of points; `keep` decides each triangle by its centre (x, y).
fn grid(rows: &[Vec<(Vec3, Vec3)>], wrap: bool, keep: impl Fn(f64, f64) -> bool) -> Mesh {
    let mut mesh = Mesh::default();
    let width = rows.first().map_or(0, Vec::len);
    for row in rows {
        for &(p, n) in row {
            mesh.positions.push(p);
            mesh.normals.push(n);
        }
    }
    let at = |i: usize, j: usize| (i * width + j % width) as u32;
    let columns = if wrap { width } else { width.saturating_sub(1) };
    for i in 0..rows.len().saturating_sub(1) {
        for j in 0..columns {
            for t in [
                [at(i, j), at(i, j + 1), at(i + 1, j + 1)],
                [at(i, j), at(i + 1, j + 1), at(i + 1, j)],
            ] {
                let c = t
                    .iter()
                    .map(|&k| mesh.positions[k as usize])
                    .fold(Vec3::ZERO, |s, p| s + p)
                    * (1.0 / 3.0);
                if keep(c.x, c.y) {
                    mesh.triangles.push(t);
                }
            }
        }
    }
    mesh
}

/// A profile surface over its region, in its local frame.
fn profile(surface: &dyn optobench_surfaces::Surface, region: Region) -> Mesh {
    let point = |x: f64, y: f64| -> Option<(Vec3, Vec3)> {
        let z = surface.sag(x.hypot(y))?;
        let p = Vec3::new(x, y, z);
        Some((p, surface.normal(p).unwrap_or(Vec3::Z)))
    };
    let rows: Vec<Vec<(Vec3, Vec3)>> = match region {
        Region::Disk { .. } | Region::Annulus { .. } => {
            let (inner, outer) = match region {
                Region::Annulus { inner, outer } => (inner, outer),
                Region::Disk { radius } => (0.0, radius),
                _ => (0.0, 0.0),
            };
            (0..=ACROSS)
                .map(|i| {
                    let r = inner + (outer - inner) * i as f64 / ACROSS as f64;
                    (0..AROUND)
                        .filter_map(|j| {
                            let a = core::f64::consts::TAU * j as f64 / AROUND as f64;
                            point(r * a.cos(), r * a.sin())
                        })
                        .collect::<Vec<_>>()
                })
                .filter(|row| row.len() == AROUND)
                .collect()
        }
        Region::Rectangle {
            half_width,
            half_height,
        } => (0..=ACROSS)
            .map(|i| {
                let y = -half_height + 2.0 * half_height * i as f64 / ACROSS as f64;
                (0..=ACROSS)
                    .filter_map(|j| {
                        point(-half_width + 2.0 * half_width * j as f64 / ACROSS as f64, y)
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|row| row.len() == ACROSS + 1)
            .collect(),
        Region::SquareWithHole { half_size, .. } => {
            let n = 2 * ACROSS;
            (0..=n)
                .map(|i| {
                    let y = -half_size + 2.0 * half_size * i as f64 / n as f64;
                    (0..=n)
                        .filter_map(|j| {
                            point(-half_size + 2.0 * half_size * j as f64 / n as f64, y)
                        })
                        .collect::<Vec<_>>()
                })
                .filter(|row| row.len() == n + 1)
                .collect()
        }
    };
    let round = matches!(region, Region::Disk { .. } | Region::Annulus { .. });
    grid(&rows, round, |x, y| round || region.contains(x, y))
}

/// A tube of `radius` from z = 0 to z = `length`, normals outward.
fn tube(radius: f64, length: f64) -> Mesh {
    let rows: Vec<Vec<(Vec3, Vec3)>> = [0.0, length]
        .iter()
        .map(|&z| {
            (0..AROUND)
                .map(|j| {
                    let a = core::f64::consts::TAU * j as f64 / AROUND as f64;
                    let n = Vec3::new(a.cos(), a.sin(), 0.0);
                    (Vec3::new(radius * n.x, radius * n.y, z), n)
                })
                .collect()
        })
        .collect();
    grid(&rows, true, |_, _| true)
}

/// Every face as triangles, grouped by owner (in `built.owners` order), in the scene
/// frame.
pub fn meshes(built: &Built, fineness: Fineness) -> Vec<(Owner, Mesh)> {
    let mut items: Vec<(Owner, Mesh)> =
        built.owners.iter().map(|o| (*o, Mesh::default())).collect();
    for face in &built.system.faces {
        let local = match &face.shape {
            Shape::Brep(brep) => brep.tessellate(fineness),
            Shape::Cylinder { radius, length } => tube(*radius, *length),
            Shape::Profile(surface) => profile(surface.as_ref(), face.region),
        };
        if local.is_empty() {
            continue;
        }
        let placed = Mesh {
            positions: local
                .positions
                .iter()
                .map(|p| face.placement.apply_point(*p))
                .collect(),
            normals: local
                .normals
                .iter()
                .map(|n| face.placement.apply_vector(*n))
                .collect(),
            triangles: local.triangles,
        };
        if let Some(item) = items.get_mut(face.owner) {
            item.1.append(&placed);
        }
    }
    items
}
