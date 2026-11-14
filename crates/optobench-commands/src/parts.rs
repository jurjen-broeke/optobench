//! Commands for the parts library: `import-part`, `parts`, `import-curve` and `curves`.

use std::path::Path;

use optobench_io::library;
use optobench_io::sidecar::PartMatch;
use optobench_library::{AdjusterKind, Glass, Outline, Part, PartType, Reach};
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

/// A mount's adjusters as JSON (mm for slides, degrees for tilts).
fn adjusters_json(part: &Part) -> Value {
    json!(
        part.adjusters
            .iter()
            .map(|a| {
                let mut value = json!({
                    "name": a.name,
                    "kind": a.kind.as_str(),
                    "axis": [a.axis.x, a.axis.y, a.axis.z],
                    "unit": match a.kind {
                        AdjusterKind::Slide => "mm",
                        AdjusterKind::Tilt => "deg",
                    },
                });
                if let Some(p) = a.pivot {
                    value["pivot"] = json!([p.x, p.y, p.z]);
                }
                match a.reach {
                    Reach::Unlimited => value["range"] = Value::Null,
                    Reach::Range { low, high } => value["range"] = json!([low, high]),
                    Reach::Travel(travel) => {
                        value["range"] = json!([-0.5 * travel, 0.5 * travel]);
                        value["travel"] = json!(travel);
                    }
                }
                value
            })
            .collect::<Vec<_>>()
    )
}

/// A part as JSON.
pub(crate) fn part_json(part: &Part, file: Option<&Path>) -> Value {
    if part.part_type == PartType::Mount {
        let mut value = json!({
            "key": part.key(),
            "part": part.number,
            "vendor": part.vendor,
            "type": part.part_type.as_str(),
            "source": part.source,
            "adjusters": adjusters_json(part),
        });
        if let Some(file) = file {
            value["file"] = json!(file.to_string_lossy());
        }
        return value;
    }
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
        Some(Outline::Round { diameter }) => value["diameter_mm"] = json!(diameter),
        Some(Outline::Rectangular { width, height }) => {
            value["size_mm"] = json!([width, height]);
        }
        None => {}
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

/// `part-from-step`: a lens's prescription read from its STEP file (bare glass, or mounted:
/// its housing and rings are left out): the radii in order, the centre thicknesses, which
/// gaps are glass and which air, the diameter, and what it is (singlet, cemented or
/// air-spaced doublet, triplet). With `vendor`, `part`, `source` (its spec sheet) and
/// `glass` (a name or index per glass body, in order), and optionally `clear_aperture`
/// (mm, from the spec sheet; layouts need it), it also gives the part file, and
/// with `import` stores it in the parts library (`replace` to overwrite). Parameter
/// `path`; optionally those and `data_directory`.
pub(crate) fn part_from_step(parameters: &Parameters) -> Result<Value, CommandError> {
    use optobench_library::prescription::Gap;
    check_keys(
        parameters,
        &[
            "path",
            "vendor",
            "part",
            "source",
            "glass",
            "clear_aperture",
            "import",
            "replace",
            "data_directory",
        ],
    )?;
    let path = Path::new(required(parameters, "path")?);
    let failed =
        |e: String| CommandError::new(ErrorCode::Failed, format!("{}: {e}", path.display()));
    let text = std::fs::read_to_string(path).map_err(|e| failed(e.to_string()))?;
    let (prescription, products) = library::prescription_from_step(&text).map_err(failed)?;
    let micron = |x: f64| (x * 1e3).round() / 1e3;
    let mut result = json!({
        "kind": prescription.kind(),
        "radii_mm": prescription.radii.iter().map(|r| length(micron(*r))).collect::<Vec<_>>(),
        "thickness_mm": prescription.thickness.iter().map(|t| micron(*t)).collect::<Vec<_>>(),
        "gaps": prescription.gaps.iter().map(|g| match g {
            Gap::Glass(n) => json!(format!("glass {}", n + 1)),
            Gap::Air => json!("air"),
        }).collect::<Vec<_>>(),
        "glasses": prescription.glasses(),
        "diameter_mm": prescription.diameter.map(micron),
        "left_out": prescription.left_out,
        "products": products,
    });
    let given = ["vendor", "part", "source", "glass"]
        .iter()
        .filter(|k| parameters.contains_key(**k))
        .count();
    if given == 0 {
        return Ok(result);
    }
    if given < 4 {
        return Err(CommandError::new(
            ErrorCode::InvalidParameters,
            "a part file needs `vendor`, `part`, `source` (its spec sheet) and `glass` (one per glass body)",
        ));
    }
    let glasses: Vec<toml::Value> = parameters["glass"]
        .as_array()
        .ok_or_else(|| {
            CommandError::new(
                ErrorCode::InvalidParameters,
                "`glass` is a list of names or indices",
            )
        })?
        .iter()
        .map(|g| match g {
            Value::String(s) => Ok(toml::Value::String(s.clone())),
            Value::Number(n) => n.as_f64().map(toml::Value::Float).ok_or(()),
            _ => Err(()),
        })
        .collect::<Result<_, ()>>()
        .map_err(|()| {
            CommandError::new(
                ErrorCode::InvalidParameters,
                "`glass` is a list of names or indices",
            )
        })?;
    let clear_aperture = match parameters.get("clear_aperture") {
        None => None,
        Some(v) => Some(v.as_f64().filter(|c| *c > 0.0).ok_or_else(|| {
            CommandError::new(
                ErrorCode::InvalidParameters,
                "`clear_aperture` is a diameter in mm",
            )
        })?),
    };
    let part_file = library::lens_part_text(
        required(parameters, "vendor")?,
        required(parameters, "part")?,
        required(parameters, "source")?,
        &prescription,
        &glasses,
        &products,
        clear_aperture,
    )
    .map_err(|e| CommandError::new(ErrorCode::InvalidParameters, e))?;
    result["part_file"] = json!(part_file);
    if flag(parameters, "import")? {
        let root = data_directory(parameters)?;
        let (part, file) = library::import_text(&root, &part_file, flag(parameters, "replace")?)
            .map_err(|e| CommandError::new(ErrorCode::Failed, e))?;
        result["stored"] = json!(true);
        result["part"] = part_json(&part, Some(&file));
    }
    Ok(result)
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

/// `import-curve`: read a measured R/T table (a spreadsheet `.xlsx`, as vendors publish
/// them, or delimited text) and store it in the parts library, or every table of a ZIP
/// archive (a vendor's filter set). Parameter `path`; optionally `name` (the curve's name,
/// or a prefix for an archive's curves; default: named after the files), `sheet`
/// (default the first), `columns` (default: from a
/// header row naming the wavelength, else `["wavelength", "transmittance"]`; also
/// `reflectance` and `skip`), `wavelength_unit` (`nm` or `um`; default from the header,
/// else nm), `percent` (default: from the header, else from the values), `reflectance` (without a reflectance column: `complement`, default, R = 1 − T,
/// or `zero` for absorbing filters), `source` (where it came from), `redistribute`
/// (default false), `replace` and `data_directory`.
pub(crate) fn import_curve(parameters: &Parameters) -> Result<Value, CommandError> {
    use optobench_io::curves::{self, Column, Options};
    check_keys(
        parameters,
        &[
            "path",
            "name",
            "columns",
            "wavelength_unit",
            "percent",
            "reflectance",
            "sheet",
            "source",
            "redistribute",
            "replace",
            "data_directory",
        ],
    )?;
    let invalid = |message: String| CommandError::new(ErrorCode::InvalidParameters, message);
    let path = Path::new(required(parameters, "path")?);
    let name = match parameters.get("name") {
        None => None,
        Some(Value::String(s)) => Some(s.as_str()),
        Some(_) => return Err(invalid("`name` must be text".into())),
    };
    let mut options = Options::default();
    if let Some(columns) = parameters.get("columns") {
        options.columns = Some(
            columns
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .map(|c| c.as_str().and_then(Column::parse))
                        .collect::<Option<Vec<_>>>()
                })
                .ok_or_else(|| {
                    invalid("`columns` lists wavelength, transmittance, reflectance or skip".into())
                })?,
        );
    }
    options.nanometres = match parameters.get("wavelength_unit") {
        None => None,
        Some(v) => match v.as_str() {
            Some("nm") => Some(true),
            Some("um") => Some(false),
            _ => return Err(invalid("`wavelength_unit` is \"nm\" or \"um\"".into())),
        },
    };
    options.percent = match parameters.get("percent") {
        None => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(invalid("`percent` is true or false".into())),
    };
    options.reflectance_zero = match parameters.get("reflectance") {
        None => false,
        Some(v) => match v.as_str() {
            Some("complement") => false,
            Some("zero") => true,
            _ => {
                return Err(invalid(
                    "`reflectance` is \"complement\" or \"zero\"".into(),
                ));
            }
        },
    };
    options.sheet = match parameters.get("sheet") {
        None => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err(invalid("`sheet` must be text".into())),
    };
    let source = match parameters.get("source") {
        None => "",
        Some(Value::String(s)) => s,
        Some(_) => return Err(invalid("`source` must be text".into())),
    };
    let root = data_directory(parameters)?;
    let records = curves::import(
        &root,
        path,
        name,
        &options,
        source,
        flag(parameters, "redistribute")?,
        flag(parameters, "replace")?,
    )
    .map_err(|e| CommandError::new(ErrorCode::Failed, e))?;
    // One table gives one curve; an archive (a filter set) one per table.
    Ok(match <[Value; 1]>::try_from(records) {
        Ok([record]) if !path.to_string_lossy().to_lowercase().ends_with(".zip") => {
            json!({ "stored": true, "curve": record })
        }
        Ok(one) => json!({ "stored": true, "curves": one }),
        Err(many) => json!({ "stored": true, "curves": many }),
    })
}

/// `curves`: every R/T curve in the parts library.
pub(crate) fn curves(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(parameters, &["data_directory"])?;
    let root = data_directory(parameters)?;
    let list =
        optobench_io::curves::list(&root).map_err(|e| CommandError::new(ErrorCode::Failed, e))?;
    Ok(json!({
        "directory": optobench_io::curves::curves_directory(&root).to_string_lossy(),
        "curves": list,
    }))
}
