//! M4c tests: sidecars on the STEP fixtures. A lens body from STEP must trace exactly like
//! the same lens defined inline; untagged bodies absorb; matching errors name the entry.

use std::path::{Path, PathBuf};

use optobench_io::sidecar::parse;
use optobench_materials::Medium;
use optobench_math::Vec3;
use optobench_scene::layout::frame_along;
use optobench_scene::{
    Detector, Element, ElementKind, LensSurface, Owner, Scene, Source, SourceKind,
};
use optobench_trace::Coating;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/step")
        .join(name)
}

fn lookup(name: &str) -> Result<Medium, String> {
    Err(format!("no material `{name}` in these tests"))
}

fn read(name: &str) -> Scene {
    let path = fixture(name);
    let text = std::fs::read_to_string(&path).expect("sidecar exists");
    parse(&text, &path, &lookup).expect("sidecar reads")
}

#[test]
fn step_lens_traces_like_the_same_lens_defined_inline() {
    let from_step = read("lens.optobench");
    // The same lens inline: first vertex (the curved face) at z = 5, axis down, R₁ = 30
    // with its centre further along the axis, R₂ flat, 5 mm thick, Ø25.4.
    let down = Vec3::new(0.0, 0.0, -1.0);
    let frame = |z: f64| frame_along(Vec3::new(0.0, 0.0, z), down).expect("valid");
    let inline = Scene {
        surrounding: 1.0.into(),
        elements: vec![Element {
            id: "L1".into(),
            frame: frame(5.0),
            kind: ElementKind::Lens {
                surfaces: vec![
                    LensSurface {
                        radius: 30.0,
                        conic: 0.0,
                    },
                    LensSurface {
                        radius: f64::INFINITY,
                        conic: 0.0,
                    },
                ],
                thicknesses: vec![5.0],
                materials: vec![1.5.into()],
                diameter: 25.4,
                clear_aperture: 25.4,
                coatings: vec![Coating::Uncoated; 2],
            },
        }],
        sources: vec![Source {
            id: "beam".into(),
            frame: frame(20.0),
            kind: SourceKind::Collimated { diameter: 4.0 },
            wavelength: 0.55,
            power: 0.001,
            rings: 6,
            spectrum: vec![],
            pulse: None,
        }],
        proxies: vec![],
        detectors: vec![Detector {
            id: "focus".into(),
            frame: frame(-56.666666666667),
            size: (2.0, 2.0),
            pixels: (40, 40),
            quantum_efficiency: None,
        }],
        solids: vec![],
        // As the sidecar has it by default.
        split_cutoff: Some(optobench_scene::DEFAULT_SPLIT_CUTOFF),
        threads: 0,
        dofs: Vec::new(),
        fluorophores: Vec::new(),
        thermal: Vec::new(),
        tolerances: Vec::new(),
    };
    let a = from_step.trace().expect("traces");
    let b = inline.trace().expect("traces");
    assert_eq!(a.records.len(), b.records.len());
    for (ra, rb) in a.records.iter().zip(&b.records) {
        assert!(
            (ra.ray.position - rb.ray.position).length() < 1e-9,
            "{:?} vs {:?}",
            ra.ray.position,
            rb.ray.position
        );
        assert!((ra.ray.power - rb.ray.power).abs() < 1e-15);
        assert!((ra.ray.optical_path - rb.ray.optical_path).abs() < 1e-9);
    }
    let (da, db) = (&a.detectors[0], &b.detectors[0]);
    assert!((da.power - db.power).abs() < 1e-15 && (da.rms_radius - db.rms_radius).abs() < 1e-9);
    // Focus at the back focal length: a 4 mm beam at f/15 leaves a spot of a few µm.
    assert!(da.rms_radius < 5e-3, "rms {}", da.rms_radius);
    // Two uncoated faces near normal incidence: 0.96² of the power arrives.
    assert!((da.power / 0.001 - 0.96 * 0.96).abs() < 2e-3);
    assert!(a.ledger.imbalance().abs() < 1e-15);
}

#[test]
fn untagged_bodies_absorb_and_are_reported_by_path() {
    let scene = read("assembly.optobench");
    assert_eq!(scene.solids.len(), 3, "plate and two rods");
    let trace = scene.trace().expect("traces");
    assert!(
        trace.detectors[0].power.abs() < 1e-18,
        "nothing passes the rod"
    );
    let clipped: Vec<(&str, Owner)> = trace
        .clipping
        .iter()
        .map(|c| (c.id.as_str(), c.owner))
        .collect();
    assert_eq!(clipped.len(), 1);
    assert_eq!(clipped[0].0, "rod:1/Rod");
    assert!((trace.clipping[0].power - 0.001).abs() < 1e-15);
}

fn error_for(sidecar: &str, step: &str) -> String {
    // Write a sidecar next to a fixture STEP file in a scratch directory.
    let directory = std::env::temp_dir().join(format!(
        "optobench-sidecar-{}-{}",
        std::process::id(),
        sidecar.len()
    ));
    std::fs::create_dir_all(&directory).expect("scratch");
    std::fs::copy(fixture(step), directory.join("design.step")).expect("copy");
    let path = directory.join("design.optobench");
    std::fs::write(&path, sidecar).expect("write");
    let result = parse(sidecar, &path, &lookup)
        .map(|_| ())
        .expect_err("rejected")
        .0;
    let _ = std::fs::remove_dir_all(&directory);
    result
}

#[test]
fn matching_errors_name_the_entry() {
    let header = "format = \"optobench/1\"\nkind = \"sidecar\"\n";
    let lens = |body: &str| {
        format!("{header}[[element]]\nid = \"L\"\nkind = \"lens\"\nbody = {body}\nmaterial = 1.5\n")
    };
    let unknown = error_for(&lens("{ product = \"nothing\" }"), "assembly.step");
    assert!(
        unknown.contains("`L`") && unknown.contains("matches no body"),
        "{unknown}"
    );
    let ambiguous = error_for(&lens("{ product = \"rod\" }"), "assembly.step");
    assert!(
        ambiguous.contains("2 placements") && ambiguous.contains("occurrence"),
        "{ambiguous}"
    );
    // With the occurrence it is fine; by fingerprint after a rename it is found too.
    let by_path = error_for(
        &format!(
            "{}junk = 1\n",
            lens("{ product = \"rod\", occurrence = \"rod:2\" }")
        ),
        "assembly.step",
    );
    assert!(
        by_path.contains("unknown key `junk`"),
        "only the stray key is wrong: {by_path}"
    );
    let no_step = {
        let directory =
            std::env::temp_dir().join(format!("optobench-sidecar-missing-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("scratch");
        let path = directory.join("lonely.optobench");
        let text = header.to_string();
        let message = parse(&text, &path, &lookup)
            .map(|_| ())
            .expect_err("rejected")
            .0;
        let _ = std::fs::remove_dir_all(&directory);
        message
    };
    assert!(no_step.contains("no STEP file"), "{no_step}");
}

#[test]
fn renamed_part_is_found_by_fingerprint() {
    let header = "format = \"optobench/1\"\nkind = \"sidecar\"\n";
    let text = format!(
        "{header}[[element]]\nid = \"L\"\nkind = \"lens\"\nbody = {{ product = \"renamed\", fingerprint = \"{}\" }}\nmaterial = 1.5\n",
        // The plate's fingerprint, as the `bodies` command lists it.
        "be6001eb9fc56f34"
    );
    let directory =
        std::env::temp_dir().join(format!("optobench-sidecar-fp-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("scratch");
    std::fs::copy(fixture("assembly.step"), directory.join("design.step")).expect("copy");
    let path = directory.join("design.optobench");
    let scene = parse(&text, &path, &lookup).expect("found by fingerprint");
    let _ = std::fs::remove_dir_all(&directory);
    assert!(scene.solids.iter().any(|s| s.id == "L"));
}

#[test]
fn library_part_coatings_land_on_their_own_faces() {
    // The golden relay's lens (R 30 curved side first, then flat) as a library part with a
    // curve on its curved surface and an ideal flat surface: in each placed lens the curve
    // must sit on the spherical face and "ideal" on the flat one, whichever way the lens
    // faces.
    use optobench_io::scene_file::Lookups;
    use optobench_library::{CoatingSpec, Glass, Library, Outline, Part, PartSurface, PartType};
    use optobench_surfaces::brep::FaceSurface;
    use optobench_trace::coating::CoatingCurve;
    let part = Part {
        vendor: "Example".into(),
        number: "PC-30".into(),
        part_type: PartType::Lens,
        source: "hand-written for the golden cage relay".into(),
        outline: Outline::Round { diameter: 25.4 },
        clear_aperture: Some(22.9),
        surfaces: vec![
            PartSurface {
                radius: 30.0,
                conic: 0.0,
            },
            PartSurface::FLAT,
        ],
        thickness: vec![5.0],
        glass: vec![Glass::Index(1.5)],
        coating: None,
        coatings: vec![CoatingSpec::Curve("c".into()), CoatingSpec::Ideal],
        products: vec!["pc-lens".into()],
    };
    let library = Library::new(vec![part]).expect("one part");
    let materials = |name: &str| Err(format!("no material `{name}`"));
    let curves = |_: &str| {
        CoatingCurve::new(vec![(0.4, 0.5, 0.5), (0.7, 0.5, 0.5)]).map_err(|e| format!("{e:?}"))
    };
    let lookups = Lookups {
        materials: &materials,
        curves: &curves,
        library: &library,
    };
    let text = "format = \"optobench/1\"\nkind = \"sidecar\"\nstep = \"cage-relay.step\"\n";
    let read =
        optobench_io::sidecar::read(text, &fixture("library.optobench"), &lookups).expect("reads");
    assert_eq!(read.parts.len(), 2);
    for id in ["pc-lens:1", "pc-lens:2"] {
        let solid = read
            .scene
            .solids
            .iter()
            .find(|s| s.id == id)
            .expect("tagged");
        let optobench_scene::SolidRole::Lens { face_coatings, .. } = &solid.role else {
            panic!("{id} is a lens")
        };
        assert!(!face_coatings.is_empty());
        for (face, coating) in face_coatings {
            let surface = &solid.faces[*face].geometry.surface;
            match coating {
                Coating::Curve(_) => assert!(matches!(surface, FaceSurface::Sphere { .. }), "{id}"),
                Coating::Ideal => assert!(matches!(surface, FaceSurface::Plane(_)), "{id}"),
                other => panic!("{id}: {other:?}"),
            }
        }
    }
}
