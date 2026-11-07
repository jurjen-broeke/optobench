//! Exact faces for tracing, from a STEP model: every face of every placed body as an
//! [`optobench_surfaces::brep::BrepFace`] in its product's frame, with the placement that
//! puts it in the root frame.

use std::sync::Arc;

use optobench_math::Transform;
use optobench_surfaces::brep::{self, BrepFace, FaceSurface};

use super::model::{self, StepModel};

/// Boundary edges are sampled to this chord deviation (mm). Trimming is exact to this
/// distance from a face's edges; hits on the surfaces themselves are exact regardless.
pub const EDGE_TOLERANCE: f64 = 1e-3;

/// One face of one placed body.
#[derive(Debug, Clone)]
pub struct PlacedFace {
    /// Occurrence names from the root down.
    pub path: Vec<String>,
    /// Index into [`StepModel::products`].
    pub product: usize,
    /// Index into that product's bodies.
    pub body: usize,
    /// Index into that body's faces.
    pub face: usize,
    /// The face in its product's frame, shared by every placement of the product.
    pub geometry: Arc<BrepFace>,
    /// Product frame to root frame.
    pub placement: Transform,
}

/// Faces and what could not be converted.
#[derive(Debug, Clone, Default)]
pub struct Faces {
    /// Every converted face of every placed body.
    pub faces: Vec<PlacedFace>,
    /// Faces skipped because their surface or an edge curve is not supported (including
    /// B-splines above degree `brep::MAX_DEGREE`).
    pub skipped: usize,
}

fn curve(c: &model::Curve) -> Option<brep::Curve> {
    Some(match c {
        model::Curve::Line { point, direction } => brep::Curve::Line {
            point: *point,
            direction: *direction,
        },
        model::Curve::Circle { frame, radius } => brep::Curve::Circle {
            frame: *frame,
            radius: *radius,
        },
        model::Curve::Ellipse {
            frame,
            semi_axis_1,
            semi_axis_2,
        } => brep::Curve::Ellipse {
            frame: *frame,
            semi_axis_1: *semi_axis_1,
            semi_axis_2: *semi_axis_2,
        },
        model::Curve::BSpline(b) if b.degree > brep::MAX_DEGREE => return None,
        model::Curve::BSpline(b) => brep::Curve::BSpline(brep::BSplineCurve {
            degree: b.degree,
            points: b.control_points.clone(),
            weights: b.weights.clone(),
            knots: b.knots.clone(),
        }),
        model::Curve::Unsupported(_) => return None,
    })
}

fn surface(s: &model::Surface) -> Option<FaceSurface> {
    Some(match s {
        model::Surface::Plane(frame) => FaceSurface::Plane(*frame),
        model::Surface::Cylinder { frame, radius } => FaceSurface::Cylinder {
            frame: *frame,
            radius: *radius,
        },
        model::Surface::Cone {
            frame,
            radius,
            semi_angle,
        } => FaceSurface::Cone {
            frame: *frame,
            radius: *radius,
            semi_angle: *semi_angle,
        },
        model::Surface::Sphere { frame, radius } => FaceSurface::Sphere {
            frame: *frame,
            radius: *radius,
        },
        model::Surface::Torus {
            frame,
            major,
            minor,
        } => FaceSurface::Torus {
            frame: *frame,
            major: *major,
            minor: *minor,
        },
        model::Surface::BSpline(b) if b.u_degree.max(b.v_degree) > brep::MAX_DEGREE => return None,
        model::Surface::BSpline(b) => FaceSurface::BSpline(brep::BSplineSurface {
            u_degree: b.u_degree,
            v_degree: b.v_degree,
            points: b.control_points.clone(),
            weights: b.weights.clone(),
            u_knots: b.u_knots.clone(),
            v_knots: b.v_knots.clone(),
        }),
        model::Surface::Revolution {
            profile,
            axis_point,
            axis_direction,
        } => FaceSurface::Revolution {
            profile: curve(profile)?,
            axis_point: *axis_point,
            axis_direction: *axis_direction,
        },
        model::Surface::Extrusion { profile, direction } => FaceSurface::Extrusion {
            profile: curve(profile)?,
            direction: *direction,
        },
        model::Surface::Unsupported(_) => return None,
    })
}

/// Whether two edges of a loop are the two traversals of one seam: the same curve,
/// run in opposite directions between the same vertices.
fn seam_pair(a: &model::Edge, b: &model::Edge) -> bool {
    a.curve == b.curve
        && (a.start - b.end).length() < 1e-9
        && (a.end - b.start).length() < 1e-9
        && a.forward != b.forward
}

/// A face's boundary as closed polylines. Seam edges (traversed twice, in opposite
/// directions, where a periodic surface closes on itself) are not boundaries: the face
/// lies on both sides. They are left out, which splits a loop into the closed chains
/// between them, for example the two rims of a cylinder band.
fn loops(face: &model::Face, tolerance: f64) -> Option<Vec<Vec<optobench_math::Vec3>>> {
    let mut out = Vec::new();
    for bound in &face.bounds {
        let edges = &bound.edges;
        if edges.is_empty() {
            // A vertex loop (the apex of a cone) bounds nothing.
            continue;
        }
        let seam: Vec<bool> = (0..edges.len())
            .map(|i| (0..edges.len()).any(|j| j != i && seam_pair(&edges[i], &edges[j])))
            .collect();
        // Start just after a seam edge, so each chain is collected whole.
        let first = seam.iter().position(|s| *s).map_or(0, |i| i + 1);
        let mut chain: Vec<optobench_math::Vec3> = Vec::new();
        let mut finish = |chain: &mut Vec<optobench_math::Vec3>| {
            if chain.len() > 1 && (chain[0] - chain[chain.len() - 1]).length() < 1e-9 {
                chain.pop();
            }
            if chain.len() > 1 {
                out.push(core::mem::take(chain));
            }
            chain.clear();
        };
        for k in 0..edges.len() {
            let i = (first + k) % edges.len();
            if seam[i] {
                finish(&mut chain);
                continue;
            }
            let edge = &edges[i];
            let points = curve(&edge.curve)?.sample(edge.start, edge.end, edge.forward, tolerance);
            // Consecutive edges share their joining vertex.
            let skip = usize::from(!chain.is_empty());
            chain.extend(points.into_iter().skip(skip));
        }
        finish(&mut chain);
    }
    Some(out)
}

/// The exact faces of every placed body, edges sampled to `tolerance` (mm).
pub fn faces(model: &StepModel, tolerance: f64) -> Faces {
    // Convert each product's faces once; placements share them.
    let mut converted: Vec<Vec<Vec<Option<Arc<BrepFace>>>>> = Vec::new();
    let mut skipped_by_product = Vec::new();
    for product in &model.products {
        let mut skipped = 0;
        let bodies = product
            .bodies
            .iter()
            .map(|body| {
                body.faces
                    .iter()
                    .map(|face| {
                        let result = surface(&face.surface)
                            .zip(loops(face, tolerance))
                            .map(|(s, l)| Arc::new(BrepFace::new(s, !face.same_sense, l)));
                        if result.is_none() {
                            skipped += 1;
                        }
                        result
                    })
                    .collect()
            })
            .collect();
        converted.push(bodies);
        skipped_by_product.push(skipped);
    }
    let mut out = Faces::default();
    for instance in model.instances() {
        if instance.body == 0 {
            out.skipped += skipped_by_product[instance.product];
        }
        for (index, face) in converted[instance.product][instance.body]
            .iter()
            .enumerate()
        {
            if let Some(geometry) = face {
                out.faces.push(PlacedFace {
                    path: instance.path.clone(),
                    product: instance.product,
                    body: instance.body,
                    face: index,
                    geometry: geometry.clone(),
                    placement: instance.transform,
                });
            }
        }
    }
    out
}
