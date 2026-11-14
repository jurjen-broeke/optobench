//! A lens's prescription read from its STEP file and made a library part (M16), on the
//! golden `lens.step` (crates/optobench-io/tests/step, made by SolidCraft 0.30.1: a Ø25.4
//! cylinder cut by a sphere of R 30, plano-convex, 5 mm centre thickness).

use std::path::{Path, PathBuf};

use optobench_commands::Registry;
use serde_json::{Value, json};

fn step() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../optobench-io/tests/step/lens.step")
}

fn run(parameters: Value) -> Result<Value, String> {
    Registry::standard()
        .run("part-from-step", &parameters)
        .map_err(|e| e.message)
}

#[test]
fn a_plano_convex_lens_is_read_curved_side_first_and_imported() {
    let found = run(json!({ "path": step().to_string_lossy() })).expect("read");
    assert_eq!(found["kind"], "singlet");
    assert_eq!(found["radii_mm"], json!([30.0, "inf"]));
    assert_eq!(found["thickness_mm"], json!([5.0]));
    assert_eq!(found["diameter_mm"], json!(25.4));
    assert_eq!(found["glasses"], 1);
    assert!(
        found.get("part_file").is_none(),
        "no part file without its glasses"
    );

    let e = run(json!({ "path": step().to_string_lossy(), "vendor": "Example" }))
        .expect_err("incomplete");
    assert!(e.contains("`glass`"), "{e}");
    let e = run(json!({
        "path": step().to_string_lossy(), "vendor": "Example", "part": "PC-30",
        "source": "golden lens", "glass": [1.5, 1.6],
    }))
    .expect_err("one glass body");
    assert!(e.contains("give 1 glasses"), "{e}");

    let data =
        std::env::temp_dir().join(format!("optobench-part-from-step-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let made = run(json!({
        "path": step().to_string_lossy(), "vendor": "Example", "part": "PC-30",
        "source": "golden lens (R 30, 5 mm)", "glass": [1.5], "clear_aperture": 22.9, "import": true,
        "data_directory": data.to_string_lossy(),
    }))
    .expect("imported");
    let text = made["part_file"].as_str().expect("a part file");
    assert!(text.contains("clear_aperture = 22.9"), "{text}");
    assert!(
        text.contains("surfaces = [{ radius = 30 }, { radius = \"inf\" }]"),
        "{text}"
    );
    assert!(
        text.contains("thickness = [5]") && text.contains("glass = [1.5]"),
        "{text}"
    );
    assert_eq!(made["part"]["key"], "example/PC-30");
    // Now in the library, it reads as that part; a sidecar would recognise its product.
    let listed = Registry::standard()
        .run(
            "parts",
            &json!({ "data_directory": data.to_string_lossy() }),
        )
        .expect("listed");
    let part = &listed["parts"][0];
    assert_eq!(part["radii_mm"], json!([30.0, "inf"]));
    assert!(
        part["products"]
            .as_array()
            .expect("names")
            .iter()
            .any(|p| p == "lens"),
        "{part}"
    );
    let _ = std::fs::remove_dir_all(&data);
}
