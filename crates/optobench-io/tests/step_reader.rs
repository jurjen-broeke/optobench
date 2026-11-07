//! M4a tests of the STEP reader. Committed fixtures (`tests/step/`, made by SolidCraft from
//! the scripts beside them) give exact geometry; a hand-written file checks units per
//! context; the corpus (local only, skipped when absent) checks real vendor and SolidCraft
//! exports.

use optobench_io::step::{Curve, StepModel, Surface, read};
use optobench_math::Vec3;

const EPS: f64 = 1e-9;

fn fixture(name: &str) -> StepModel {
    let path = format!("{}/tests/step/{name}.step", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).expect("fixture exists");
    read(&text).expect("fixture reads")
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < EPS
}

/// Every vertex of every face of the first body of the first product with bodies.
fn vertices(model: &StepModel) -> Vec<Vec3> {
    let body = model
        .products
        .iter()
        .find_map(|p| p.bodies.first())
        .expect("a body");
    body.faces
        .iter()
        .flat_map(|f| f.bounds.iter())
        .flat_map(|l| {
            l.edges
                .iter()
                .flat_map(|e| [e.start, e.end])
                .chain(l.vertex)
        })
        .collect()
}

#[test]
fn block_has_six_planes_and_its_corners() {
    let model = fixture("block");
    assert!(model.header.schemas[0].starts_with("AP242"));
    assert_eq!(model.header.originating_system, "SolidCraft");
    let body = &model
        .products
        .iter()
        .find(|p| !p.bodies.is_empty())
        .expect("a part")
        .bodies[0];
    assert!(body.solid);
    assert_eq!(body.faces.len(), 6);
    assert!(
        body.faces
            .iter()
            .all(|f| matches!(f.surface, Surface::Plane(_)))
    );
    let corners = vertices(&model);
    for corner in [
        Vec3::ZERO,
        Vec3::new(10.0, 20.0, 30.0),
        Vec3::new(10.0, 0.0, 30.0),
    ] {
        assert!(
            corners.iter().any(|v| close(*v, corner)),
            "corner {corner:?}"
        );
    }
    assert!(model.unsupported.is_empty());
}

#[test]
fn primitives_have_their_exact_surfaces() {
    let radius_of = |name: &str| -> Vec<Surface> {
        fixture(name)
            .products
            .into_iter()
            .flat_map(|p| p.bodies)
            .flat_map(|b| b.faces)
            .map(|f| f.surface)
            .collect()
    };
    assert!(radius_of("cylinder").iter().any(
        |s| matches!(s, Surface::Cylinder { radius, frame } if (radius - 6.0).abs() < EPS && close(frame.axes()[2], Vec3::Z))
    ));
    assert!(
        radius_of("sphere")
            .iter()
            .all(|s| matches!(s, Surface::Sphere { radius, .. } if (radius - 8.0).abs() < EPS))
    );
    assert!(radius_of("torus").iter().all(
        |s| matches!(s, Surface::Torus { major, minor, .. } if (major - 20.0).abs() < EPS && (minor - 4.0).abs() < EPS)
    ));
    // A cone from Ø20 to Ø8 over 15 mm has semi-angle atan(6/15).
    let expected = (6.0_f64 / 15.0).atan();
    assert!(radius_of("cone").iter().any(
        |s| matches!(s, Surface::Cone { semi_angle, .. } if (semi_angle.abs() - expected).abs() < 1e-9)
    ));
}

#[test]
fn lens_has_its_sphere_plane_and_rim() {
    // Cylinder Ø25.4 ∩ sphere R 30 centred at z = −25: a plano-convex lens with its flat
    // face at z = 0 and its vertex at z = 5.
    let surfaces: Vec<Surface> = fixture("lens")
        .products
        .into_iter()
        .flat_map(|p| p.bodies)
        .flat_map(|b| b.faces)
        .map(|f| f.surface)
        .collect();
    assert_eq!(surfaces.len(), 3);
    assert!(
        surfaces
            .iter()
            .any(|s| matches!(s, Surface::Sphere { radius, frame }
        if (radius - 30.0).abs() < EPS && close(frame.origin(), Vec3::new(0.0, 0.0, -25.0))))
    );
    assert!(
        surfaces
            .iter()
            .any(|s| matches!(s, Surface::Plane(frame) if frame.origin().z.abs() < EPS))
    );
    assert!(
        surfaces
            .iter()
            .any(|s| matches!(s, Surface::Cylinder { radius, .. } if (radius - 12.7).abs() < EPS))
    );
}

#[test]
fn assembly_places_each_occurrence() {
    // rod:1 moved by (15, 15, 4); rod:2 turned 90° about x, then moved by (−15, 0, 30).
    // The rod's top centre (0, 0, 100) lands at (15, 15, 104) and (−15, −100, 30).
    let model = fixture("assembly");
    assert_eq!(model.roots.len(), 1);
    let root = &model.products[model.roots[0]];
    let names: Vec<&str> = root.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["plate:1", "rod:1", "rod:2"]);
    let top = Vec3::new(0.0, 0.0, 100.0);
    let place = |name: &str| {
        root.children
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.transform.apply_point(top))
            .expect("placed")
    };
    assert!(close(place("rod:1"), Vec3::new(15.0, 15.0, 104.0)));
    assert!(close(place("rod:2"), Vec3::new(-15.0, -100.0, 30.0)));
    let instances = model.instances();
    assert_eq!(instances.len(), 3);
    assert!(instances.iter().any(|i| i.path == vec!["rod:2".to_owned()]));
}

/// An inch assembly placing a part modelled in metres: one sphere face of radius 0.01 m
/// (10 mm) at x = 0.02 m, placed 2 in (50.8 mm) along x.
const MIXED_UNITS: &str = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''),'2;1');
FILE_NAME('mixed.step','',(''),(''),'hand','optobench tests','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=APPLICATION_CONTEXT('');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT_DEFINITION_CONTEXT('',#1,'design');
#10=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#11=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#10);
#12=(CONVERSION_BASED_UNIT('INCH',#11)LENGTH_UNIT()NAMED_UNIT(#13));
#13=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);
#14=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));
#15=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#12,#14))REPRESENTATION_CONTEXT('inch',''));
#16=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));
#17=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#16,#14))REPRESENTATION_CONTEXT('metre',''));
#20=PRODUCT('top','top','',(#2));
#21=PRODUCT_DEFINITION_FORMATION('','',#20);
#22=PRODUCT_DEFINITION('design','',#21,#3);
#23=PRODUCT_DEFINITION_SHAPE('','',#22);
#24=SHAPE_REPRESENTATION('',(#40),#15);
#25=SHAPE_DEFINITION_REPRESENTATION(#23,#24);
#30=PRODUCT('ball','ball','',(#2));
#31=PRODUCT_DEFINITION_FORMATION('','',#30);
#32=PRODUCT_DEFINITION('design','',#31,#3);
#33=PRODUCT_DEFINITION_SHAPE('','',#32);
#34=SHAPE_REPRESENTATION('',(#50,#60),#17);
#35=SHAPE_DEFINITION_REPRESENTATION(#33,#34);
#40=AXIS2_PLACEMENT_3D('',#41,$,$);
#41=CARTESIAN_POINT('',(2.,0.,0.));
#50=AXIS2_PLACEMENT_3D('',#51,$,$);
#51=CARTESIAN_POINT('',(0.,0.,0.));
#60=MANIFOLD_SOLID_BREP('ball',#61);
#61=CLOSED_SHELL('',(#62));
#62=ADVANCED_FACE('',(#63),#66,.T.);
#63=FACE_OUTER_BOUND('',#64,.T.);
#64=VERTEX_LOOP('',#65);
#65=VERTEX_POINT('',#69);
#66=SPHERICAL_SURFACE('',#67,0.01);
#67=AXIS2_PLACEMENT_3D('',#68,$,$);
#68=CARTESIAN_POINT('',(0.02,0.,0.));
#69=CARTESIAN_POINT('',(0.02,0.,0.01));
#70=NEXT_ASSEMBLY_USAGE_OCCURRENCE('ball:1','ball:1','',#22,#32,$);
#71=PRODUCT_DEFINITION_SHAPE('','',#70);
#72=ITEM_DEFINED_TRANSFORMATION('','',#50,#40);
#73=(REPRESENTATION_RELATIONSHIP('','',#34,#24)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#72)SHAPE_REPRESENTATION_RELATIONSHIP());
#74=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#73,#71);
ENDSEC;
END-ISO-10303-21;
";

#[test]
fn units_are_resolved_per_context() {
    let model = read(MIXED_UNITS).expect("reads");
    let ball = model
        .products
        .iter()
        .find(|p| p.name == "ball")
        .expect("ball");
    let face = &ball.bodies[0].faces[0];
    match &face.surface {
        Surface::Sphere { frame, radius } => {
            assert!((radius - 10.0).abs() < EPS, "0.01 m is 10 mm");
            assert!(close(frame.origin(), Vec3::new(20.0, 0.0, 0.0)));
        }
        other => panic!("expected a sphere, got {other:?}"),
    }
    assert_eq!(
        face.bounds[0]
            .vertex
            .map(|v| close(v, Vec3::new(20.0, 0.0, 10.0))),
        Some(true)
    );
    // Placed 2 in = 50.8 mm along x in the inch parent: the centre lands at 70.8 mm.
    let instance = &model.instances()[0];
    assert!(close(
        instance.transform.apply_point(Vec3::new(20.0, 0.0, 0.0)),
        Vec3::new(70.8, 0.0, 0.0)
    ));
}

#[test]
fn malformed_structure_is_reported() {
    let missing = MIXED_UNITS.replace("#74=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#73,#71);\n", "");
    let error = read(&missing).expect_err("rejected");
    assert!(error.to_string().contains("ball:1"), "{error}");
    let dangling =
        MIXED_UNITS.replace("#61=CLOSED_SHELL('',(#62));", "#61=CLOSED_SHELL('',(#99));");
    assert!(
        read(&dangling)
            .expect_err("rejected")
            .to_string()
            .contains("#99")
    );
}

// ---- corpus (local only) ----

fn corpus(relative: &str) -> Option<StepModel> {
    let path = format!("{}/../../corpus/{relative}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).ok()?;
    Some(read(&text).expect("corpus file reads"))
}

#[test]
fn corpus_vendor_lens_radii_are_converted_to_millimetres() {
    // AC254-030-A is stored in inches: 0.82244, 0.65866 and 3.14173 in. In mm these are
    // the radii Thorlabs publishes for the lens, 20.89, 16.73 and 79.80 mm (to be
    // confirmed against the spec sheet before any optical test relies on them).
    let Some(model) = corpus("vendor/thorlabs/ac254-030-a.step") else {
        eprintln!("corpus absent: skipped");
        return;
    };
    let mut radii: Vec<f64> = model
        .products
        .iter()
        .flat_map(|p| &p.bodies)
        .flat_map(|b| &b.faces)
        .filter_map(|f| match &f.surface {
            Surface::Sphere { radius, .. } => Some((radius * 100.0).round() / 100.0),
            _ => None,
        })
        .collect();
    radii.sort_by(f64::total_cmp);
    radii.dedup();
    assert_eq!(radii, vec![16.73, 20.89, 79.8]);
    assert!(model.unsupported.is_empty());
}

#[test]
fn corpus_golden_assembly_reads_completely() {
    let Some(model) = corpus("solidcraft/thorlabs-cage.step") else {
        eprintln!("corpus absent: skipped");
        return;
    };
    assert!(model.unsupported.is_empty(), "{:?}", model.unsupported);
    let root = &model.products[model.roots[0]];
    assert_eq!(root.name, "thorlabs-cage");
    let instances = model.instances();
    assert_eq!(instances.len(), 116, "as SolidCraft counts the bodies");
    for rod in 1..=8 {
        let name = format!("er10:{rod}");
        assert!(instances.iter().any(|i| i.path == [name.clone()]), "{name}");
    }
    // Every edge of every face ends where the next begins, within the file's precision.
    for product in &model.products {
        for face in product.bodies.iter().flat_map(|b| &b.faces) {
            for bound in &face.bounds {
                for pair in bound.edges.windows(2) {
                    assert!((pair[0].end - pair[1].start).length() < 1e-6);
                }
                assert!(
                    bound
                        .edges
                        .iter()
                        .all(|e| !matches!(e.curve, Curve::Unsupported(_)))
                );
            }
        }
    }
}
