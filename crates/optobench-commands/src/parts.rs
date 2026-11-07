//! Commands for the parts library: `import-part` and `parts`.

use std::path::Path;

use optobench_io::library;
use optobench_io::sidecar::PartMatch;
use optobench_library::{Glass, Outline, Part};
use serde_json::{Value, json};

use crate::materials::{data_directory, flag, required};
use crate::{CommandError, ErrorCode, Parameters, check_keys};

/// A length for JSON: infinite (flat) as `"inf"`.
pub(crate) fn length(value: f64) -> Value {
    if value.is_finite() {
        json!(value)
    } else if value > 0.0 {
        json!("inf")
    } else {
        json!("-inf")
    }
}

/// A part as JSON.
pub(crate) fn part_json(part: &Part, file: Option<&Path>) -> Value {
    let mut value = json!({
        "key": part.key(),
        "part": part.number,
        "vendor": part.vendor,
        "type": part.part_type.as_str(),
        "source": part.source,
        "radii_mm": part.surfaces.iter().map(|s| length(s.radius)).collect::<Vec<_>>(),
        "thickness_mm": part.thickness,
        "glass": part.glass.iter().map(|g| match g {
            Glass::Named(name) => json!(name),
            Glass::Index(n) => json!(n),
        }).collect::<Vec<_>>(),
        "products": part.products,
    });
    match part.outline {
        Outline::Round { diameter } => value["diameter_mm"] = json!(diameter),
        Outline::Rectangular { width, height } => value["size_mm"] = json!([width, height]),
    }
    if let Some(file) = file {
        value["file"] = json!(file.to_string_lossy());
    }
    value
}

/// Library elements of a traced sidecar, with their geometry check.
pub(crate) fn matches_json(parts: &[PartMatch]) -> Value {
    json!(
        parts
            .iter()
            .map(|m| json!({
                "element": m.element,
                "part": m.part,
                "automatic": m.automatic,
                "bodies": m.bodies.iter().map(|b| {
                    let mut body = json!({ "solid": b.solid, "glass": b.glass + 1 });
                    match &b.comparison {
                        Some(c) => {
                            body["reversed"] = json!(c.reversed);
                            body["differences"] = json!(c.differences.iter().map(|d| json!({
                                "quantity": d.quantity,
                                "part_mm": length(d.part),
                                "step_mm": length(d.step),
                            })).collect::<Vec<_>>());
                        }
                        None => {
                            body["differences"] = json!([]);
                            body["note"] = json!("no prescription could be read from this body's faces; not checked");
                        }
                    }
                    body
                }).collect::<Vec<_>>(),
                "missing_glasses": m.missing.iter().map(|g| g + 1).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>()
    )
}

/// `import-part`: check a part file and copy it into the parts library in the data
/// directory. Parameters `path`, optionally `replace` (overwrite a different file for the
/// same part) and `data_directory`.
pub(crate) fn import_part(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(parameters, &["path", "replace", "data_directory"])?;
    let path = Path::new(required(parameters, "path")?);
    let replace = flag(parameters, "replace")?;
    let root = data_directory(parameters)?;
    let (part, file) = library::import(&root, path, replace)
        .map_err(|e| CommandError::new(ErrorCode::Failed, e))?;
    Ok(json!({ "stored": true, "part": part_json(&part, Some(&file)) }))
}

/// `parts`: every part in the parts library.
pub(crate) fn parts(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(parameters, &["data_directory"])?;
    let root = data_directory(parameters)?;
    let list = library::list(&root).map_err(|e| CommandError::new(ErrorCode::Failed, e))?;
    Ok(json!({
        "directory": library::parts_directory(&root).to_string_lossy(),
        "parts": list.iter().map(|(p, f)| part_json(p, Some(f))).collect::<Vec<_>>(),
    }))
}
