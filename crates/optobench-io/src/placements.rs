//! `export-placements`: from a standalone layout to CAD (docs/design.md, Standalone
//! scenes, "Upgrade path").
//!
//! Writes the pose of every element, proxy, source and detector (origin and axes in the
//! scene frame, mm) so the layout can be rebuilt in a CAD program, and a matching sidecar
//! skeleton: each element refers to a CAD body whose product (component) is named after
//! the element's id, with the layout's materials, and sources and detectors keep their
//! exact positions. Built in CAD with those names and exported to STEP, the design traces
//! from the sidecar like the layout did.

use optobench_math::Frame;
use optobench_scene::{ElementKind, Scene};
use serde_json::{Value, json};
use toml::Table;

fn pose(frame: &Frame) -> Value {
    // Rounded to 1e-12 so exact axes read as 0 and 1, not 6e-17.
    let r = |x: f64| {
        let rounded = (x * 1e12).round() / 1e12;
        if rounded == 0.0 { 0.0 } else { rounded }
    };
    let round = |p: optobench_math::Vec3| optobench_math::Vec3::new(r(p.x), r(p.y), r(p.z));
    let [x, y, z] = frame.axes().map(round);
    let o = round(frame.origin());
    let v = |p: optobench_math::Vec3| json!([p.x, p.y, p.z]);
    json!({
        "origin": v(o),
        "x_axis": v(x),
        "y_axis": v(y),
        "z_axis": v(z),
        // Local to scene, row-major 3 × 4 (rotation and translation), as CAD programs and
        // SolidCraft's occurrence.move take it.
        "matrix": [x.x, y.x, z.x, o.x, x.y, y.y, z.y, o.y, x.z, y.z, z.z, o.z],
    })
}

/// The placements document of a scene.
pub fn placements(scene: &Scene) -> Value {
    let elements: Vec<Value> = scene
        .elements
        .iter()
        .map(|e| {
            let mut entry = pose(&e.frame);
            entry["id"] = json!(e.id);
            entry["kind"] = json!(match e.kind {
                ElementKind::Lens { .. } => "lens",
                ElementKind::Mirror { .. } => "mirror",
                ElementKind::Objective { .. } => "objective",
                ElementKind::Sample { .. } => "sample",
            });
            entry
        })
        .collect();
    let others = |items: Vec<(&str, &Frame)>| -> Vec<Value> {
        items
            .into_iter()
            .map(|(id, frame)| {
                let mut entry = pose(frame);
                entry["id"] = json!(id);
                entry
            })
            .collect()
    };
    json!({
        "format": crate::format_tag(),
        "kind": "placements",
        "units": { "length": "mm" },
        "frame": "scene frame, Z up; each item's local +z is its optical axis (a mirror's normal)",
        "elements": elements,
        "proxies": others(scene.proxies.iter().map(|p| (p.id.as_str(), &p.frame)).collect()),
        "sources": others(scene.sources.iter().map(|s| (s.id.as_str(), &s.frame)).collect()),
        "detectors": others(scene.detectors.iter().map(|d| (d.id.as_str(), &d.frame)).collect()),
    })
}

/// A TOML basic string.
fn quoted(text: &str) -> String {
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A simple TOML value (string, number, boolean) as written in a file.
fn literal(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => quoted(s),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => format!("{f:?}"),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Array(items) => format!(
            "[{}]",
            items.iter().map(literal).collect::<Vec<_>>().join(", ")
        ),
        other => format!("{other:?}"),
    }
}

fn vector(v: optobench_math::Vec3) -> String {
    let n = |x: f64| {
        let r = (x * 1e9).round() / 1e9;
        if r == 0.0 {
            "0".to_owned()
        } else {
            format!("{r}")
        }
    };
    format!("[{}, {}, {}]", n(v.x), n(v.y), n(v.z))
}

/// A sidecar skeleton for the scene once rebuilt in CAD and exported as `step_name`.
/// `layout` is the layout file's TOML, for the materials as written there.
pub fn sidecar(scene: &Scene, layout: &Table, step_name: &str) -> String {
    let mut out = format!(
        "format = {}\nkind = \"sidecar\"\nstep = {}\n\n\
         # Written by export-placements. Build each element in CAD as a component named after\n\
         # its id (a mirror with its reflecting face normal along its own +z), place it with\n\
         # the matrix in the placements file, and export the assembly as STEP. Mounts and other\n\
         # parts need no entry: every body not named here absorbs.\n",
        quoted(&crate::format_tag()),
        quoted(step_name)
    );
    let element_tables: Vec<&Table> = layout
        .get("element")
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_table).collect())
        .unwrap_or_default();
    for element in &scene.elements {
        let written = element_tables
            .iter()
            .find(|t| t.get("id").and_then(toml::Value::as_str) == Some(element.id.as_str()));
        out.push_str(&format!(
            "\n[[element]]\nid = {}\nbody = {{ product = {} }}\n",
            quoted(&element.id),
            quoted(&element.id)
        ));
        let library = written
            .and_then(|t| t.get("library"))
            .and_then(toml::Value::as_str);
        if let (Some(key), ElementKind::Lens { .. }) = (library, &element.kind) {
            // The part gives the glass; the sidecar picks its glass bodies by geometry.
            out.push_str(&format!("library = {}\n", quoted(key)));
            continue;
        }
        match &element.kind {
            ElementKind::Lens { materials, .. } => {
                out.push_str("kind = \"lens\"\n");
                let material = written
                    .and_then(|t| t.get("material"))
                    .and_then(toml::Value::as_array);
                match material.map(Vec::as_slice) {
                    Some([one]) => out.push_str(&format!("material = {}\n", literal(one))),
                    Some(several) => out.push_str(&format!(
                        "# {} glasses in the layout ({}): one CAD body per glass, named in `materials`.\nmaterials = {{ }}\n",
                        materials.len(),
                        several.iter().map(literal).collect::<Vec<_>>().join(", ")
                    )),
                    None => out.push_str("material = 1.5  # not found in the layout: check\n"),
                }
            }
            ElementKind::Mirror { .. } => out.push_str("kind = \"mirror\"\nreflect = [0, 0, 1]\n"),
            // Ideal objectives and samples are not CAD bodies: they stay in the layout.
            ElementKind::Objective { .. } | ElementKind::Sample { .. } => out.push_str(
                "# an ideal objective or a sample: not a CAD body; keep it in a layout\n",
            ),
        }
    }
    let source_tables: Vec<&Table> = layout
        .get("source")
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_table).collect())
        .unwrap_or_default();
    for source in &scene.sources {
        out.push_str(&format!("\n[[source]]\nid = {}\n", quoted(&source.id)));
        if let Some(t) = source_tables
            .iter()
            .find(|t| t.get("id").and_then(toml::Value::as_str) == Some(source.id.as_str()))
        {
            for key in [
                "kind",
                "wavelength_um",
                "power_w",
                "diameter",
                "half_angle_deg",
                "waist_diameter",
                "m_squared",
                "core_diameter",
                "na",
                "size",
                "angle_rings",
                "spectrum",
                "rings",
            ] {
                if let Some(v) = t.get(key) {
                    out.push_str(&format!("{key} = {}\n", literal(v)));
                }
            }
        }
        out.push_str(&format!(
            "position = {}\naxis = {}\n",
            vector(source.frame.origin()),
            vector(source.frame.axes()[2])
        ));
    }
    for detector in &scene.detectors {
        out.push_str(&format!(
            "\n[[detector]]\nid = {}\nposition = {}\naxis = {}\nsize = [{}, {}]\npixels = [{}, {}]\n",
            quoted(&detector.id),
            vector(detector.frame.origin()),
            vector(detector.frame.axes()[2]),
            detector.size.0,
            detector.size.1,
            detector.pixels.0,
            detector.pixels.1
        ));
    }
    out
}
