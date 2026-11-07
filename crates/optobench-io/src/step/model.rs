//! Products, assembly structure, units and B-rep geometry from a STEP exchange structure.
//!
//! The subset read is what AP203, AP214 and AP242 mechanical exports use for solids
//! (ISO 10303-42 geometry and topology, ISO 10303-41 product structure, ISO 10303-43
//! representations), following the CAx-IF recommended practices for product structure:
//!
//! - `PRODUCT` ← `PRODUCT_DEFINITION_FORMATION` ← `PRODUCT_DEFINITION` ←
//!   `PRODUCT_DEFINITION_SHAPE` ← `SHAPE_DEFINITION_REPRESENTATION` → a shape
//!   representation, linked by plain `SHAPE_REPRESENTATION_RELATIONSHIP`s to the
//!   representations holding the solids;
//! - each placement of a child in a parent is a `NEXT_ASSEMBLY_USAGE_OCCURRENCE` with a
//!   `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION` whose relationship carries an
//!   `ITEM_DEFINED_TRANSFORMATION` between a placement in the child's representation and
//!   one in the parent's. Which representation is the child's is decided by membership,
//!   not by parameter order.
//!
//! Every length is converted to mm and every angle to radians using the units of the
//! representation context the item belongs to. Anything this reader does not support is
//! counted by entity type in [`StepModel::unsupported`], never skipped silently.

use std::collections::{BTreeMap, HashMap, HashSet};

use optobench_math::{Frame, Transform, Vec3};

use super::part21::{Exchange, Instance, Record, Value};

/// Why a model could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelError(pub String);

impl core::fmt::Display for ModelError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ModelError {}

fn error<T>(message: impl Into<String>) -> Result<T, ModelError> {
    Err(ModelError(message.into()))
}

/// Header facts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Header {
    /// FILE_SCHEMA names, e.g. `AUTOMOTIVE_DESIGN`.
    pub schemas: Vec<String>,
    /// FILE_NAME: the originating system (e.g. `SolidWorks 2018`).
    pub originating_system: String,
    /// FILE_NAME: the preprocessor version (e.g. `SwSTEP 2.0`).
    pub preprocessor: String,
}

/// An analytic or free-form surface, in the product's frame, mm.
#[derive(Debug, Clone, PartialEq)]
pub enum Surface {
    /// The local x–y plane of the frame.
    Plane(Frame),
    /// Radius about the frame's z axis.
    Cylinder {
        /// Axis frame.
        frame: Frame,
        /// Radius.
        radius: f64,
    },
    /// Radius in the frame's x–y plane, opening at `semi_angle` (radians) along +z.
    Cone {
        /// Axis frame.
        frame: Frame,
        /// Radius at the frame origin.
        radius: f64,
        /// Half the apex angle.
        semi_angle: f64,
    },
    /// Centred on the frame origin.
    Sphere {
        /// Centre frame.
        frame: Frame,
        /// Radius.
        radius: f64,
    },
    /// About the frame's z axis.
    Torus {
        /// Axis frame.
        frame: Frame,
        /// Distance from the axis to the tube centre.
        major: f64,
        /// Tube radius.
        minor: f64,
    },
    /// A (possibly rational) B-spline surface.
    BSpline(BSplineSurface),
    /// A profile curve swept about an axis (SURFACE_OF_REVOLUTION).
    Revolution {
        /// The profile.
        profile: Box<Curve>,
        /// A point on the axis.
        axis_point: Vec3,
        /// Unit direction of the axis.
        axis_direction: Vec3,
    },
    /// A profile curve swept along a vector (SURFACE_OF_LINEAR_EXTRUSION).
    Extrusion {
        /// The profile.
        profile: Box<Curve>,
        /// The sweep direction and length (mm).
        direction: Vec3,
    },
    /// A surface type this reader does not handle.
    Unsupported(String),
}

/// A B-spline surface with its knots expanded by multiplicity.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplineSurface {
    /// Degree in u.
    pub u_degree: usize,
    /// Degree in v.
    pub v_degree: usize,
    /// Control points, rows along u, each row along v (mm).
    pub control_points: Vec<Vec<Vec3>>,
    /// Weights in the same layout, for a rational surface.
    pub weights: Option<Vec<Vec<f64>>>,
    /// Knot vector in u, expanded.
    pub u_knots: Vec<f64>,
    /// Knot vector in v, expanded.
    pub v_knots: Vec<f64>,
}

/// A curve, in the product's frame, mm.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    /// Through `point` along unit `direction`.
    Line {
        /// A point on it.
        point: Vec3,
        /// Unit direction.
        direction: Vec3,
    },
    /// In the frame's x–y plane, centred on its origin.
    Circle {
        /// Centre frame.
        frame: Frame,
        /// Radius.
        radius: f64,
    },
    /// In the frame's x–y plane: semi-axes along x and y.
    Ellipse {
        /// Centre frame.
        frame: Frame,
        /// Semi-axis along local x.
        semi_axis_1: f64,
        /// Semi-axis along local y.
        semi_axis_2: f64,
    },
    /// A (possibly rational) B-spline curve.
    BSpline(BSplineCurve),
    /// A curve type this reader does not handle.
    Unsupported(String),
}

/// A B-spline curve with its knots expanded by multiplicity.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplineCurve {
    /// Degree.
    pub degree: usize,
    /// Control points (mm).
    pub control_points: Vec<Vec3>,
    /// Weights, for a rational curve.
    pub weights: Option<Vec<f64>>,
    /// Knot vector, expanded.
    pub knots: Vec<f64>,
}

/// An edge of a face's boundary, in the direction the loop runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// The underlying curve.
    pub curve: Curve,
    /// Where the edge starts along the loop (mm).
    pub start: Vec3,
    /// Where it ends.
    pub end: Vec3,
    /// Whether the loop runs along the curve's own direction.
    pub forward: bool,
}

/// One boundary loop of a face.
#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    /// Whether it is the outer bound (FACE_OUTER_BOUND).
    pub outer: bool,
    /// Its edges in order; empty for a vertex loop.
    pub edges: Vec<Edge>,
    /// For a vertex loop (the apex of a cone): the vertex.
    pub vertex: Option<Vec3>,
}

/// A face: part of a surface bounded by loops.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// Its name in the file.
    pub name: String,
    /// The surface it lies on.
    pub surface: Surface,
    /// Whether the face normal agrees with the surface normal.
    pub same_sense: bool,
    /// Its bounds.
    pub bounds: Vec<Loop>,
}

/// A solid or a surface model.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// Its name in the file.
    pub name: String,
    /// Whether it is a closed solid (MANIFOLD_SOLID_BREP, BREP_WITH_VOIDS).
    pub solid: bool,
    /// Every face, outer shell first.
    pub faces: Vec<Face>,
}

/// A child placed in a product.
#[derive(Debug, Clone, PartialEq)]
pub struct Child {
    /// The occurrence's name (NEXT_ASSEMBLY_USAGE_OCCURRENCE name), e.g. `er10:3`.
    pub name: String,
    /// Index of the child product in [`StepModel::products`].
    pub product: usize,
    /// The child's frame in the parent's frame (mm).
    pub transform: Transform,
}

/// A product: a part or an assembly.
#[derive(Debug, Clone, PartialEq)]
pub struct Product {
    /// PRODUCT id.
    pub id: String,
    /// PRODUCT name.
    pub name: String,
    /// Its own bodies, in its own frame (mm).
    pub bodies: Vec<Body>,
    /// Its placed children.
    pub children: Vec<Child>,
}

/// One placed body of the flattened assembly.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance3d {
    /// Occurrence names from the root down, e.g. `["er10:3"]`; empty for a root's own
    /// bodies.
    pub path: Vec<String>,
    /// Index into [`StepModel::products`].
    pub product: usize,
    /// Index into that product's bodies.
    pub body: usize,
    /// The body's frame in the root frame.
    pub transform: Transform,
}

/// What was read from a file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StepModel {
    /// Header facts.
    pub header: Header,
    /// Every product.
    pub products: Vec<Product>,
    /// Products that are no other product's child.
    pub roots: Vec<usize>,
    /// Entity types met in the geometry that this reader does not support, with counts.
    pub unsupported: BTreeMap<String, usize>,
}

impl StepModel {
    /// Every body of the assembly, placed in the root frame.
    pub fn instances(&self) -> Vec<Instance3d> {
        let mut out = Vec::new();
        for &root in &self.roots {
            self.collect(root, &mut Vec::new(), Transform::IDENTITY, &mut out, 0);
        }
        out
    }

    fn collect(
        &self,
        product: usize,
        path: &mut Vec<String>,
        transform: Transform,
        out: &mut Vec<Instance3d>,
        depth: usize,
    ) {
        if depth > 64 {
            return; // a cycle in a malformed file
        }
        for body in 0..self.products[product].bodies.len() {
            out.push(Instance3d {
                path: path.clone(),
                product,
                body,
                transform,
            });
        }
        for child in &self.products[product].children {
            path.push(child.name.clone());
            self.collect(
                child.product,
                path,
                transform * child.transform,
                out,
                depth + 1,
            );
            path.pop();
        }
    }
}

/// Typed access to the exchange structure, with unit factors per context.
struct Reader<'a> {
    x: &'a Exchange,
    /// Length factor to mm and angle factor to radians, per context entity.
    units: HashMap<u64, (f64, f64)>,
    unsupported: BTreeMap<String, usize>,
}

const SI_PREFIX: &[(&str, f64)] = &[
    ("EXA", 1e18),
    ("PETA", 1e15),
    ("TERA", 1e12),
    ("GIGA", 1e9),
    ("MEGA", 1e6),
    ("KILO", 1e3),
    ("HECTO", 1e2),
    ("DECA", 1e1),
    ("DECI", 1e-1),
    ("CENTI", 1e-2),
    ("MILLI", 1e-3),
    ("MICRO", 1e-6),
    ("NANO", 1e-9),
    ("PICO", 1e-12),
];

impl<'a> Reader<'a> {
    fn instance(&self, id: u64) -> Result<&'a Instance, ModelError> {
        match self.x.entities.get(&id) {
            Some(i) => Ok(i),
            None => error(format!("#{id} is referenced but not defined")),
        }
    }

    /// The record `name` of entity `id`, or an error naming both.
    fn record(&self, id: u64, name: &str) -> Result<&'a Record, ModelError> {
        let instance = self.instance(id)?;
        instance.record(name).ok_or_else(|| {
            ModelError(format!(
                "#{id} is {}, expected {name}",
                instance.type_name()
            ))
        })
    }

    fn parameter(record: &Record, index: usize, id: u64) -> Result<&Value, ModelError> {
        record
            .parameters
            .get(index)
            .ok_or_else(|| ModelError(format!("#{id} {} has too few parameters", record.name)))
    }

    fn reference(record: &Record, index: usize, id: u64) -> Result<u64, ModelError> {
        Self::parameter(record, index, id)?
            .reference()
            .ok_or_else(|| {
                ModelError(format!(
                    "#{id} {}: parameter {} is not a reference",
                    record.name,
                    index + 1
                ))
            })
    }

    fn number(record: &Record, index: usize, id: u64) -> Result<f64, ModelError> {
        Self::parameter(record, index, id)?.number().ok_or_else(|| {
            ModelError(format!(
                "#{id} {}: parameter {} is not a number",
                record.name,
                index + 1
            ))
        })
    }

    fn references(record: &Record, index: usize, id: u64) -> Result<Vec<u64>, ModelError> {
        let list = Self::parameter(record, index, id)?.list().ok_or_else(|| {
            ModelError(format!(
                "#{id} {}: parameter {} is not a list",
                record.name,
                index + 1
            ))
        })?;
        Ok(list.iter().filter_map(Value::reference).collect())
    }

    fn text(record: &Record, index: usize) -> String {
        record
            .parameters
            .get(index)
            .and_then(Value::text)
            .unwrap_or("")
            .to_owned()
    }

    fn note_unsupported(&mut self, type_name: String) {
        *self.unsupported.entry(type_name).or_default() += 1;
    }

    // ---- units ----

    /// Factor to mm (length) or radians (angle) of a unit entity.
    fn unit_factor(&self, id: u64, depth: usize) -> Result<Option<(bool, f64)>, ModelError> {
        if depth > 8 {
            return error(format!("#{id}: unit definitions nest too deeply"));
        }
        let unit = self.instance(id)?;
        let length = unit.is("LENGTH_UNIT");
        let angle = unit.is("PLANE_ANGLE_UNIT");
        if !length && !angle {
            return Ok(None);
        }
        if let Some(si) = unit.record("SI_UNIT") {
            let prefix = match si.parameters.first().and_then(Value::enumeration) {
                Some(p) => SI_PREFIX
                    .iter()
                    .find(|(name, _)| *name == p)
                    .map(|(_, f)| *f)
                    .ok_or_else(|| ModelError(format!("#{id}: unknown SI prefix {p}")))?,
                None => 1.0,
            };
            let name = si
                .parameters
                .get(1)
                .and_then(Value::enumeration)
                .unwrap_or("");
            return Ok(Some(match (length, name) {
                (true, "METRE") => (true, prefix * 1000.0),
                (false, "RADIAN") => (false, prefix),
                _ => {
                    return error(format!(
                        "#{id}: SI unit {name} is not a length or angle unit"
                    ));
                }
            }));
        }
        if let Some(conversion) = unit.record("CONVERSION_BASED_UNIT") {
            // CONVERSION_BASED_UNIT(name, conversion_factor): a measure with a unit.
            let measure_id = Self::reference(conversion, 1, id)?;
            let measure = self.instance(measure_id)?;
            let record = match measure {
                Instance::Simple(r) => r,
                Instance::Complex(records) => records
                    .iter()
                    .find(|r| r.name.ends_with("MEASURE_WITH_UNIT"))
                    .ok_or_else(|| ModelError(format!("#{measure_id}: not a measure with unit")))?,
            };
            let value = Self::number(record, 0, measure_id)?;
            let base = Self::reference(record, 1, measure_id)?;
            let base = self.unit_factor(base, depth + 1)?.ok_or_else(|| {
                ModelError(format!(
                    "#{measure_id}: conversion to a non-length, non-angle unit"
                ))
            })?;
            return Ok(Some((length, value * base.1)));
        }
        error(format!(
            "#{id}: {} is a unit this reader does not know",
            unit.type_name()
        ))
    }

    /// Length and angle factors of a representation context (mm, radians); mm and
    /// radians when the context assigns none.
    fn context_units(&mut self, context: u64) -> Result<(f64, f64), ModelError> {
        if let Some(units) = self.units.get(&context) {
            return Ok(*units);
        }
        let mut length = 1.0;
        let mut angle = 1.0;
        if let Some(assigned) = self
            .instance(context)?
            .record("GLOBAL_UNIT_ASSIGNED_CONTEXT")
        {
            for unit in Self::references(assigned, 0, context)? {
                match self.unit_factor(unit, 0)? {
                    Some((true, f)) => length = f,
                    Some((false, f)) => angle = f,
                    None => {}
                }
            }
        }
        self.units.insert(context, (length, angle));
        Ok((length, angle))
    }

    // ---- geometry ----

    fn point(&self, id: u64, scale: f64) -> Result<Vec3, ModelError> {
        let record = self.record(id, "CARTESIAN_POINT")?;
        let coordinates = Self::parameter(record, 1, id)?
            .list()
            .ok_or_else(|| ModelError(format!("#{id}: coordinates are not a list")))?;
        let c: Vec<f64> = coordinates.iter().filter_map(Value::number).collect();
        match c[..] {
            [x, y, z] => Ok(Vec3::new(x * scale, y * scale, z * scale)),
            [x, y] => Ok(Vec3::new(x * scale, y * scale, 0.0)),
            _ => error(format!("#{id}: a point needs 2 or 3 coordinates")),
        }
    }

    fn direction(&self, id: u64) -> Result<Vec3, ModelError> {
        let record = self.record(id, "DIRECTION")?;
        let ratios: Vec<f64> = Self::parameter(record, 1, id)?
            .list()
            .map(|l| l.iter().filter_map(Value::number).collect())
            .unwrap_or_default();
        let v = match ratios[..] {
            [x, y, z] => Vec3::new(x, y, z),
            [x, y] => Vec3::new(x, y, 0.0),
            _ => return error(format!("#{id}: a direction needs 2 or 3 ratios")),
        };
        v.normalized()
            .ok_or_else(|| ModelError(format!("#{id}: zero-length direction")))
    }

    /// AXIS2_PLACEMENT_3D(name, location, axis?, ref_direction?) as a frame.
    fn placement(&self, id: u64, scale: f64) -> Result<Frame, ModelError> {
        let record = self.record(id, "AXIS2_PLACEMENT_3D")?;
        let origin = self.point(Self::reference(record, 1, id)?, scale)?;
        let z = match record.parameters.get(2).and_then(Value::reference) {
            Some(r) => self.direction(r)?,
            None => Vec3::Z,
        };
        let hint = match record.parameters.get(3).and_then(Value::reference) {
            Some(r) => self.direction(r)?,
            None => {
                if z.cross(Vec3::X).length() > 1e-6 {
                    Vec3::X
                } else {
                    Vec3::Y
                }
            }
        };
        Frame::new(origin, z, hint)
            .or_else(|| {
                Frame::new(
                    origin,
                    z,
                    if z.cross(Vec3::X).length() > 1e-6 {
                        Vec3::X
                    } else {
                        Vec3::Y
                    },
                )
            })
            .ok_or_else(|| ModelError(format!("#{id}: degenerate placement")))
    }

    /// The record that carries a B-spline's or other entity's parameters, for simple or
    /// complex instances: the first of `names` present.
    fn first<'r>(instance: &'r Instance, names: &[&str]) -> Option<&'r Record> {
        names.iter().find_map(|n| instance.record(n))
    }

    fn expand_knots(multiplicities: &[Value], knots: &[Value]) -> Vec<f64> {
        multiplicities
            .iter()
            .zip(knots)
            .flat_map(|(m, k)| {
                let count = m.number().unwrap_or(0.0).max(0.0) as usize;
                core::iter::repeat_n(k.number().unwrap_or(0.0), count)
            })
            .collect()
    }

    fn surface(&mut self, id: u64, scale: f64, angle: f64) -> Result<Surface, ModelError> {
        let instance = self.instance(id)?;
        if let Instance::Simple(record) = instance {
            let p = |i| Self::reference(record, i, id);
            let n = |i| Self::number(record, i, id);
            match record.name.as_str() {
                "PLANE" => return Ok(Surface::Plane(self.placement(p(1)?, scale)?)),
                "CYLINDRICAL_SURFACE" => {
                    return Ok(Surface::Cylinder {
                        frame: self.placement(p(1)?, scale)?,
                        radius: n(2)? * scale,
                    });
                }
                "CONICAL_SURFACE" => {
                    return Ok(Surface::Cone {
                        frame: self.placement(p(1)?, scale)?,
                        radius: n(2)? * scale,
                        semi_angle: n(3)? * angle,
                    });
                }
                "SPHERICAL_SURFACE" => {
                    return Ok(Surface::Sphere {
                        frame: self.placement(p(1)?, scale)?,
                        radius: n(2)? * scale,
                    });
                }
                "SURFACE_OF_REVOLUTION" => {
                    // (name, swept_curve, axis_position: AXIS1_PLACEMENT(name, location, axis))
                    let profile = self.curve(p(1)?, scale, 0)?;
                    let axis_id = p(2)?;
                    let axis = self.record(axis_id, "AXIS1_PLACEMENT")?;
                    let axis_point = self.point(Self::reference(axis, 1, axis_id)?, scale)?;
                    let axis_direction = match axis.parameters.get(2).and_then(Value::reference) {
                        Some(d) => self.direction(d)?,
                        None => Vec3::Z,
                    };
                    return Ok(Surface::Revolution {
                        profile: Box::new(profile),
                        axis_point,
                        axis_direction,
                    });
                }
                "SURFACE_OF_LINEAR_EXTRUSION" => {
                    // (name, swept_curve, extrusion_axis: VECTOR(name, direction, magnitude))
                    let profile = self.curve(p(1)?, scale, 0)?;
                    let vector_id = p(2)?;
                    let vector = self.record(vector_id, "VECTOR")?;
                    let direction = self.direction(Self::reference(vector, 1, vector_id)?)?
                        * (Self::number(vector, 2, vector_id)? * scale);
                    return Ok(Surface::Extrusion {
                        profile: Box::new(profile),
                        direction,
                    });
                }
                "TOROIDAL_SURFACE" => {
                    return Ok(Surface::Torus {
                        frame: self.placement(p(1)?, scale)?,
                        major: n(2)? * scale,
                        minor: n(3)? * scale,
                    });
                }
                _ => {}
            }
        }
        if instance.is("B_SPLINE_SURFACE_WITH_KNOTS") {
            return self
                .bspline_surface(id, instance, scale)
                .map(Surface::BSpline);
        }
        let name = instance.type_name();
        self.note_unsupported(name.clone());
        Ok(Surface::Unsupported(name))
    }

    fn bspline_surface(
        &self,
        id: u64,
        instance: &Instance,
        scale: f64,
    ) -> Result<BSplineSurface, ModelError> {
        // Simple: B_SPLINE_SURFACE_WITH_KNOTS(name, u_deg, v_deg, points, form, u_closed,
        // v_closed, self_intersect, u_mults, v_mults, u_knots, v_knots, spec).
        // Complex: B_SPLINE_SURFACE(u_deg, v_deg, points, form, …) and
        // B_SPLINE_SURFACE_WITH_KNOTS(u_mults, v_mults, u_knots, v_knots, spec), with
        // RATIONAL_B_SPLINE_SURFACE(weights) for a rational one.
        let (degrees_and_points, offset, knots, knot_offset): (&Record, usize, &Record, usize) =
            match instance {
                Instance::Simple(r) => (r, 1, r, 8),
                Instance::Complex(_) => {
                    let base = instance.record("B_SPLINE_SURFACE").ok_or_else(|| {
                        ModelError(format!("#{id}: complex B-spline without B_SPLINE_SURFACE"))
                    })?;
                    let k = instance
                        .record("B_SPLINE_SURFACE_WITH_KNOTS")
                        .ok_or_else(|| ModelError(format!("#{id}: B-spline without knots")))?;
                    (base, 0, k, 0)
                }
            };
        let u_degree = Self::number(degrees_and_points, offset, id)? as usize;
        let v_degree = Self::number(degrees_and_points, offset + 1, id)? as usize;
        let rows = Self::parameter(degrees_and_points, offset + 2, id)?
            .list()
            .ok_or_else(|| ModelError(format!("#{id}: control points are not a list")))?;
        let control_points = rows
            .iter()
            .map(|row| {
                row.list()
                    .unwrap_or(&[])
                    .iter()
                    .map(|p| {
                        p.reference()
                            .ok_or_else(|| {
                                ModelError(format!("#{id}: control point is not a reference"))
                            })
                            .and_then(|r| self.point(r, scale))
                    })
                    .collect::<Result<Vec<Vec3>, ModelError>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let list = |i: usize| -> Result<&[Value], ModelError> {
            Self::parameter(knots, knot_offset + i, id)?
                .list()
                .ok_or_else(|| ModelError(format!("#{id}: knot data is not a list")))
        };
        let u_knots = Self::expand_knots(list(0)?, list(2)?);
        let v_knots = Self::expand_knots(list(1)?, list(3)?);
        let weights = match instance.record("RATIONAL_B_SPLINE_SURFACE") {
            Some(r) => Some(
                Self::parameter(r, 0, id)?
                    .list()
                    .unwrap_or(&[])
                    .iter()
                    .map(|row| {
                        row.list()
                            .unwrap_or(&[])
                            .iter()
                            .filter_map(Value::number)
                            .collect()
                    })
                    .collect(),
            ),
            None => None,
        };
        let surface = BSplineSurface {
            u_degree,
            v_degree,
            control_points,
            weights,
            u_knots,
            v_knots,
        };
        let (nu, nv) = (
            surface.control_points.len(),
            surface.control_points.first().map_or(0, Vec::len),
        );
        if surface.u_knots.len() != nu + u_degree + 1 || surface.v_knots.len() != nv + v_degree + 1
        {
            return error(format!(
                "#{id}: B-spline knot count does not match its control points and degree"
            ));
        }
        Ok(surface)
    }

    fn curve(&mut self, id: u64, scale: f64, depth: usize) -> Result<Curve, ModelError> {
        if depth > 8 {
            return error(format!("#{id}: curve definitions nest too deeply"));
        }
        let instance = self.instance(id)?;
        if let Instance::Simple(record) = instance {
            match record.name.as_str() {
                "LINE" => {
                    let point = self.point(Self::reference(record, 1, id)?, scale)?;
                    let vector_id = Self::reference(record, 2, id)?;
                    let vector = self.record(vector_id, "VECTOR")?;
                    let direction = self.direction(Self::reference(vector, 1, vector_id)?)?;
                    return Ok(Curve::Line { point, direction });
                }
                "CIRCLE" => {
                    return Ok(Curve::Circle {
                        frame: self.placement(Self::reference(record, 1, id)?, scale)?,
                        radius: Self::number(record, 2, id)? * scale,
                    });
                }
                "ELLIPSE" => {
                    return Ok(Curve::Ellipse {
                        frame: self.placement(Self::reference(record, 1, id)?, scale)?,
                        semi_axis_1: Self::number(record, 2, id)? * scale,
                        semi_axis_2: Self::number(record, 3, id)? * scale,
                    });
                }
                // The 3D curve of a curve on surfaces; the parameter-space curves are not
                // needed.
                "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" => {
                    return self.curve(Self::reference(record, 1, id)?, scale, depth + 1);
                }
                // The trim is given by the edge's vertices.
                "TRIMMED_CURVE" => {
                    return self.curve(Self::reference(record, 1, id)?, scale, depth + 1);
                }
                _ => {}
            }
        }
        if instance.is("B_SPLINE_CURVE_WITH_KNOTS") {
            return self.bspline_curve(id, instance, scale).map(Curve::BSpline);
        }
        let name = instance.type_name();
        self.note_unsupported(name.clone());
        Ok(Curve::Unsupported(name))
    }

    fn bspline_curve(
        &self,
        id: u64,
        instance: &Instance,
        scale: f64,
    ) -> Result<BSplineCurve, ModelError> {
        // Simple: B_SPLINE_CURVE_WITH_KNOTS(name, degree, points, form, closed,
        // self_intersect, mults, knots, spec). Complex: B_SPLINE_CURVE(degree, points, …)
        // and B_SPLINE_CURVE_WITH_KNOTS(mults, knots, spec), RATIONAL_B_SPLINE_CURVE(weights).
        let (base, offset, knots, knot_offset): (&Record, usize, &Record, usize) = match instance {
            Instance::Simple(r) => (r, 1, r, 6),
            Instance::Complex(_) => (
                Self::first(instance, &["B_SPLINE_CURVE"]).ok_or_else(|| {
                    ModelError(format!("#{id}: complex B-spline without B_SPLINE_CURVE"))
                })?,
                0,
                instance
                    .record("B_SPLINE_CURVE_WITH_KNOTS")
                    .ok_or_else(|| ModelError(format!("#{id}: B-spline without knots")))?,
                0,
            ),
        };
        let degree = Self::number(base, offset, id)? as usize;
        let control_points = Self::references(base, offset + 1, id)?
            .into_iter()
            .map(|r| self.point(r, scale))
            .collect::<Result<Vec<_>, _>>()?;
        let multiplicities = Self::parameter(knots, knot_offset, id)?
            .list()
            .unwrap_or(&[]);
        let values = Self::parameter(knots, knot_offset + 1, id)?
            .list()
            .unwrap_or(&[]);
        let curve = BSplineCurve {
            degree,
            knots: Self::expand_knots(multiplicities, values),
            weights: instance
                .record("RATIONAL_B_SPLINE_CURVE")
                .and_then(|r| r.parameters.first())
                .and_then(Value::list)
                .map(|w| w.iter().filter_map(Value::number).collect()),
            control_points,
        };
        if curve.knots.len() != curve.control_points.len() + degree + 1 {
            return error(format!(
                "#{id}: B-spline knot count does not match its control points and degree"
            ));
        }
        Ok(curve)
    }

    fn vertex(&self, id: u64, scale: f64) -> Result<Vec3, ModelError> {
        let record = self.record(id, "VERTEX_POINT")?;
        self.point(Self::reference(record, 1, id)?, scale)
    }

    fn face_loop(&mut self, bound_id: u64, scale: f64) -> Result<Loop, ModelError> {
        let bound_instance = self.instance(bound_id)?;
        let (bound, outer) = match bound_instance.record("FACE_OUTER_BOUND") {
            Some(r) => (r, true),
            None => (self.record(bound_id, "FACE_BOUND")?, false),
        };
        let loop_id = Self::reference(bound, 1, bound_id)?;
        let orientation = bound
            .parameters
            .get(2)
            .and_then(Value::boolean)
            .unwrap_or(true);
        let loop_instance = self.instance(loop_id)?;
        if let Some(vertex_loop) = loop_instance.record("VERTEX_LOOP") {
            let vertex = self.vertex(Self::reference(vertex_loop, 1, loop_id)?, scale)?;
            return Ok(Loop {
                outer,
                edges: Vec::new(),
                vertex: Some(vertex),
            });
        }
        let Some(edge_loop) = loop_instance.record("EDGE_LOOP") else {
            self.note_unsupported(loop_instance.type_name());
            return Ok(Loop {
                outer,
                edges: Vec::new(),
                vertex: None,
            });
        };
        let mut edges = Vec::new();
        for oriented_id in Self::references(edge_loop, 1, loop_id)? {
            // ORIENTED_EDGE(name, *, *, edge_element, orientation)
            let oriented = self.record(oriented_id, "ORIENTED_EDGE")?;
            let edge_id = Self::reference(oriented, 3, oriented_id)?;
            let along = oriented
                .parameters
                .get(4)
                .and_then(Value::boolean)
                .unwrap_or(true);
            // EDGE_CURVE(name, start, end, curve, same_sense)
            let edge = self.record(edge_id, "EDGE_CURVE")?;
            let start = self.vertex(Self::reference(edge, 1, edge_id)?, scale)?;
            let end = self.vertex(Self::reference(edge, 2, edge_id)?, scale)?;
            let curve = self.curve(Self::reference(edge, 3, edge_id)?, scale, 0)?;
            let same_sense = edge
                .parameters
                .get(4)
                .and_then(Value::boolean)
                .unwrap_or(true);
            let forward = along == same_sense;
            let (start, end) = if along { (start, end) } else { (end, start) };
            edges.push(Edge {
                curve,
                start,
                end,
                forward,
            });
        }
        if !orientation {
            edges.reverse();
            for edge in &mut edges {
                core::mem::swap(&mut edge.start, &mut edge.end);
                edge.forward = !edge.forward;
            }
        }
        Ok(Loop {
            outer,
            edges,
            vertex: None,
        })
    }

    fn face(&mut self, id: u64, scale: f64, angle: f64) -> Result<Option<Face>, ModelError> {
        let instance = self.instance(id)?;
        let Some(record) = Self::first(instance, &["ADVANCED_FACE", "FACE_SURFACE"]) else {
            self.note_unsupported(instance.type_name());
            return Ok(None);
        };
        let name = Self::text(record, 0);
        let bounds = Self::references(record, 1, id)?;
        let surface = self.surface(Self::reference(record, 2, id)?, scale, angle)?;
        let same_sense = record
            .parameters
            .get(3)
            .and_then(Value::boolean)
            .unwrap_or(true);
        let bounds = bounds
            .into_iter()
            .map(|b| self.face_loop(b, scale))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(Face {
            name,
            surface,
            same_sense,
            bounds,
        }))
    }

    fn shell_faces(&mut self, shell: u64, scale: f64, angle: f64) -> Result<Vec<Face>, ModelError> {
        let instance = self.instance(shell)?;
        let Some(record) = Self::first(instance, &["CLOSED_SHELL", "OPEN_SHELL"]) else {
            self.note_unsupported(instance.type_name());
            return Ok(Vec::new());
        };
        let mut faces = Vec::new();
        for face in Self::references(record, 1, shell)? {
            if let Some(face) = self.face(face, scale, angle)? {
                faces.push(face);
            }
        }
        Ok(faces)
    }

    /// A representation item as a body, if it is one.
    fn body(&mut self, id: u64, scale: f64, angle: f64) -> Result<Option<Body>, ModelError> {
        let instance = self.instance(id)?;
        if let Some(solid) = instance.record("MANIFOLD_SOLID_BREP") {
            let name = Self::text(solid, 0);
            let mut faces = self.shell_faces(Self::reference(solid, 1, id)?, scale, angle)?;
            if let Some(voids) = instance.record("BREP_WITH_VOIDS") {
                for void in Self::references(voids, 0, id)? {
                    faces.extend(self.shell_faces(void, scale, angle)?);
                }
            }
            return Ok(Some(Body {
                name,
                solid: true,
                faces,
            }));
        }
        if let Some(model) = instance.record("SHELL_BASED_SURFACE_MODEL") {
            let name = Self::text(model, 0);
            let mut faces = Vec::new();
            for shell in Self::references(model, 1, id)? {
                faces.extend(self.shell_faces(shell, scale, angle)?);
            }
            return Ok(Some(Body {
                name,
                solid: false,
                faces,
            }));
        }
        let name = instance.type_name();
        // Placements, styled items and the like are expected; anything else is reported.
        if !matches!(name.as_str(), "AXIS2_PLACEMENT_3D" | "CARTESIAN_POINT") {
            self.note_unsupported(name);
        }
        Ok(None)
    }
}

/// A representation's items and context.
fn representation(reader: &Reader, id: u64) -> Result<(Vec<u64>, u64), ModelError> {
    let instance = reader.instance(id)?;
    let record = match instance {
        Instance::Simple(r) => r,
        Instance::Complex(records) => records
            .iter()
            .find(|r| r.name.ends_with("REPRESENTATION"))
            .ok_or_else(|| ModelError(format!("#{id}: not a representation")))?,
    };
    Ok((
        Reader::references(record, 1, id)?,
        Reader::reference(record, 2, id)?,
    ))
}

/// Reads products, structure and geometry.
pub fn model(x: &Exchange) -> Result<StepModel, ModelError> {
    let mut reader = Reader {
        x,
        units: HashMap::new(),
        unsupported: BTreeMap::new(),
    };
    let header = Header {
        schemas: x
            .header_record("FILE_SCHEMA")
            .and_then(|r| r.parameters.first())
            .and_then(Value::list)
            .map(|l| {
                l.iter()
                    .filter_map(Value::text)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        originating_system: x
            .header_record("FILE_NAME")
            .map(|r| Reader::text(r, 5))
            .unwrap_or_default(),
        preprocessor: x
            .header_record("FILE_NAME")
            .map(|r| Reader::text(r, 4))
            .unwrap_or_default(),
    };

    // Index entities by type for the few lookups that go "backwards".
    let mut by_type: HashMap<&str, Vec<u64>> = HashMap::new();
    let mut ids: Vec<u64> = x.entities.keys().copied().collect();
    ids.sort_unstable();
    for &id in &ids {
        match &x.entities[&id] {
            Instance::Simple(r) => by_type.entry(r.name.as_str()).or_default().push(id),
            Instance::Complex(records) => {
                for r in records {
                    by_type.entry(r.name.as_str()).or_default().push(id);
                }
            }
        }
    }
    let of = |name: &str| by_type.get(name).cloned().unwrap_or_default();

    // Products, and product definitions to products.
    let mut products = Vec::new();
    let mut product_index: HashMap<u64, usize> = HashMap::new();
    for id in of("PRODUCT") {
        let record = reader.record(id, "PRODUCT")?;
        product_index.insert(id, products.len());
        products.push(Product {
            id: Reader::text(record, 0),
            name: Reader::text(record, 1),
            bodies: Vec::new(),
            children: Vec::new(),
        });
    }
    let mut formation_product: HashMap<u64, usize> = HashMap::new();
    for name in [
        "PRODUCT_DEFINITION_FORMATION",
        "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE",
    ] {
        for id in of(name) {
            let record = reader.record(id, name)?;
            if let Some(&p) = product_index.get(&Reader::reference(record, 2, id)?) {
                formation_product.insert(id, p);
            }
        }
    }
    let mut definition_product: HashMap<u64, usize> = HashMap::new();
    for name in [
        "PRODUCT_DEFINITION",
        "PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS",
    ] {
        for id in of(name) {
            let record = reader.record(id, name)?;
            if let Some(&p) = formation_product.get(&Reader::reference(record, 2, id)?) {
                definition_product.insert(id, p);
            }
        }
    }

    // Shape representations per product (through PRODUCT_DEFINITION_SHAPE and
    // SHAPE_DEFINITION_REPRESENTATION), then everything linked to them by plain
    // relationships (no transformation).
    let mut shape_of: HashMap<u64, u64> = HashMap::new(); // PRODUCT_DEFINITION_SHAPE → definition
    for id in of("PRODUCT_DEFINITION_SHAPE") {
        let record = reader.record(id, "PRODUCT_DEFINITION_SHAPE")?;
        shape_of.insert(id, Reader::reference(record, 2, id)?);
    }
    let mut product_reps: HashMap<usize, Vec<u64>> = HashMap::new();
    for id in of("SHAPE_DEFINITION_REPRESENTATION") {
        let record = reader.record(id, "SHAPE_DEFINITION_REPRESENTATION")?;
        let definition = Reader::reference(record, 0, id)?;
        let rep = Reader::reference(record, 1, id)?;
        if let Some(&p) = shape_of
            .get(&definition)
            .and_then(|d| definition_product.get(d))
        {
            product_reps.entry(p).or_default().push(rep);
        }
    }
    let mut links: HashMap<u64, Vec<u64>> = HashMap::new();
    for id in of("SHAPE_REPRESENTATION_RELATIONSHIP") {
        let instance = reader.instance(id)?;
        if instance.is("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION") {
            continue;
        }
        let record = Reader::first(
            instance,
            &[
                "SHAPE_REPRESENTATION_RELATIONSHIP",
                "REPRESENTATION_RELATIONSHIP",
            ],
        )
        .ok_or_else(|| ModelError(format!("#{id}: not a relationship")))?;
        let (a, b) = (
            Reader::reference(record, 2, id)?,
            Reader::reference(record, 3, id)?,
        );
        links.entry(a).or_default().push(b);
        links.entry(b).or_default().push(a);
    }
    let mut rep_owner: HashMap<u64, usize> = HashMap::new();
    for (&product, reps) in &product_reps {
        let mut stack = reps.clone();
        let mut seen = HashSet::new();
        while let Some(rep) = stack.pop() {
            if !seen.insert(rep) {
                continue;
            }
            rep_owner.insert(rep, product);
            stack.extend(links.get(&rep).into_iter().flatten().copied());
        }
    }

    // Bodies: items of every representation a product owns, in its context's units.
    let mut owned: Vec<(u64, usize)> = rep_owner.iter().map(|(&r, &p)| (r, p)).collect();
    owned.sort_unstable();
    for (rep, product) in owned {
        let (items, context) = representation(&reader, rep)?;
        let (scale, angle) = reader.context_units(context)?;
        for item in items {
            if let Some(body) = reader.body(item, scale, angle)? {
                products[product].bodies.push(body);
            }
        }
    }

    // Assembly structure.
    let mut usage: HashMap<u64, (String, usize, usize)> = HashMap::new(); // NAUO → (name, parent, child)
    for id in of("NEXT_ASSEMBLY_USAGE_OCCURRENCE") {
        let record = reader.record(id, "NEXT_ASSEMBLY_USAGE_OCCURRENCE")?;
        let parent = definition_product
            .get(&Reader::reference(record, 3, id)?)
            .copied();
        let child = definition_product
            .get(&Reader::reference(record, 4, id)?)
            .copied();
        if let (Some(parent), Some(child)) = (parent, child) {
            let name = Reader::text(record, 1);
            let name = if name.is_empty() {
                Reader::text(record, 0)
            } else {
                name
            };
            usage.insert(id, (name, parent, child));
        }
    }
    let mut placed: HashSet<u64> = HashSet::new();
    for id in of("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION") {
        let record = reader.record(id, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION")?;
        let relationship = Reader::reference(record, 0, id)?;
        let shape = Reader::reference(record, 1, id)?;
        let Some(&nauo) = shape_of.get(&shape) else {
            continue;
        };
        let Some((name, parent, child)) = usage.get(&nauo).cloned() else {
            continue;
        };
        let rel = reader.instance(relationship)?;
        let base = rel.record("REPRESENTATION_RELATIONSHIP").ok_or_else(|| {
            ModelError(format!(
                "#{relationship}: not a representation relationship"
            ))
        })?;
        let (rep_1, rep_2) = (
            Reader::reference(base, 2, relationship)?,
            Reader::reference(base, 3, relationship)?,
        );
        let with = rel
            .record("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")
            .ok_or_else(|| {
                ModelError(format!(
                    "#{relationship}: placement without a transformation"
                ))
            })?;
        let transformation = Reader::reference(with, 0, relationship)?;
        let Some(idt) = reader
            .instance(transformation)?
            .record("ITEM_DEFINED_TRANSFORMATION")
        else {
            reader.note_unsupported(reader.instance(transformation)?.type_name());
            continue;
        };
        let (item_1, item_2) = (
            Reader::reference(idt, 2, transformation)?,
            Reader::reference(idt, 3, transformation)?,
        );
        // The child's representation is the one the child product owns.
        let child_first =
            rep_owner.get(&rep_1) == Some(&child) || rep_owner.get(&rep_2) == Some(&parent);
        let (child_rep, parent_rep, from, to) = if child_first {
            (rep_1, rep_2, item_1, item_2)
        } else {
            (rep_2, rep_1, item_2, item_1)
        };
        let (_, child_context) = representation(&reader, child_rep)?;
        let (_, parent_context) = representation(&reader, parent_rep)?;
        let (child_scale, _) = reader.context_units(child_context)?;
        let (parent_scale, _) = reader.context_units(parent_context)?;
        let from = reader.placement(from, child_scale)?.transform();
        let to = reader.placement(to, parent_scale)?.transform();
        products[parent].children.push(Child {
            name,
            product: child,
            transform: to * from.inverse(),
        });
        placed.insert(nauo);
    }
    let children: HashSet<usize> = usage.values().map(|(_, _, c)| *c).collect();
    let roots = (0..products.len())
        .filter(|p| !children.contains(p))
        .collect();
    for (nauo, (name, _, _)) in &usage {
        if !placed.contains(nauo) {
            return error(format!("occurrence `{name}` (#{nauo}) has no placement"));
        }
    }
    Ok(StepModel {
        header,
        products,
        roots,
        unsupported: reader.unsupported,
    })
}
