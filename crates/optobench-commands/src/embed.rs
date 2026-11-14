//! `embed`: write a design's `[resolved]` section, so it traces without the user's own
//! data (docs/design.md, "Embedded parameters"; `optobench_io::resolved`).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use optobench_io::resolved::{self, Embedded};
use optobench_io::scene_file::{self, Lookups};
use optobench_io::sha256_hex;
use optobench_io::{curves, library, sidecar, store};
use optobench_library::{Library, Part};
use serde_json::{Value, json};
use toml::{Table, Value as Toml};

use crate::materials::{flag, required};
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn failed(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::Failed, message)
}

/// Where a value the design uses was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found {
    /// In the user's data directory.
    Data,
    /// Only in the file's earlier `[resolved]` section.
    Embedded,
}

/// What a design takes from outside itself, by kind and name.
#[derive(Default)]
struct Used {
    materials: BTreeMap<String, Found>,
    curves: BTreeMap<String, Found>,
}

/// Reads a design (without its `[resolved]` section) with the user's data first and the
/// earlier embedded values second, recording what it used and where it was found.
/// Returns the library part keys it used.
fn trace_uses(
    body: &str,
    path: &Path,
    data: Option<&Path>,
    old: &Embedded,
    library: &Library,
    used: &RefCell<Used>,
) -> Result<Vec<String>, CommandError> {
    let materials = |name: &str| {
        let from_data = store::material(data, name);
        match from_data {
            Ok(medium) => {
                if data.is_some_and(|root| store::imported(root, name).is_ok()) {
                    used.borrow_mut()
                        .materials
                        .insert(name.to_owned(), Found::Data);
                }
                Ok(medium)
            }
            Err(e) => old.material(name).map_or(Err(e), |m| {
                used.borrow_mut()
                    .materials
                    .insert(name.to_owned(), Found::Embedded);
                Ok(m)
            }),
        }
    };
    let curve_lookup = |name: &str| {
        let from_data = match data {
            Some(root) => curves::load(root, name),
            None => optobench_io::coatings::no_curves(name),
        };
        match from_data {
            Ok(curve) => {
                used.borrow_mut()
                    .curves
                    .insert(name.to_owned(), Found::Data);
                Ok(curve)
            }
            Err(e) => old.curve(name).map_or(Err(e), |c| {
                used.borrow_mut()
                    .curves
                    .insert(name.to_owned(), Found::Embedded);
                Ok(c)
            }),
        }
    };
    let lookups = Lookups {
        materials: &materials,
        curves: &curve_lookup,
        library,
    };
    let at = |e: optobench_io::scene_file::FileError| failed(format!("{}: {e}", path.display()));
    let root: Table = body.parse().map_err(|e| failed(format!("{e}")))?;
    let mut keys: Vec<String> = match sidecar::kind_of(body).as_deref() {
        Some("sidecar") => {
            let read = sidecar::read(body, path, &lookups).map_err(at)?;
            read.parts.into_iter().map(|m| m.part).collect()
        }
        _ => {
            scene_file::parse_with(body, &lookups).map_err(at)?;
            root.get("element")
                .and_then(Toml::as_array)
                .map(|elements| {
                    elements
                        .iter()
                        .filter_map(|e| e.get("library").and_then(Toml::as_str))
                        .filter_map(|key| library.get(key).map(Part::key))
                        .collect()
                })
                .unwrap_or_default()
        }
    };
    // Mounts named from the library (not the file's own `[[mount]]`).
    keys.extend(optobench_io::mounts::library_mounts(&root, library));
    Ok(keys)
}

/// A part file's keys as an embedded entry, with its checksum.
fn part_entry(text: &str) -> Result<Table, CommandError> {
    let mut table: Table = text
        .parse()
        .map_err(|e| failed(format!("part file: {e}")))?;
    table.remove("format");
    table.remove("kind");
    table.insert("sha256".into(), Toml::String(sha256_hex(text.as_bytes())));
    Ok(table)
}

fn text_of(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

/// `embed`: parameters `path` (a layout or sidecar), optionally `include_restricted`
/// (also embed values whose source does not allow redistribution; default false: they are
/// listed instead), `out` (where to write; default the file itself) and
/// `data_directory`. Rewrites only the `[resolved]` section at the end of the file.
pub(crate) fn embed(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(
        parameters,
        &["path", "include_restricted", "out", "data_directory"],
    )?;
    let path = PathBuf::from(required(parameters, "path")?);
    let include_restricted = flag(parameters, "include_restricted")?;
    let out = match parameters.get("out") {
        None => path.clone(),
        Some(Value::String(s)) => PathBuf::from(s),
        Some(_) => {
            return Err(CommandError::new(
                ErrorCode::InvalidParameters,
                "`out` must be a path",
            ));
        }
    };
    let data: Option<PathBuf> = match parameters.get("data_directory") {
        Some(Value::String(s)) => Some(PathBuf::from(s)),
        Some(_) => {
            return Err(CommandError::new(
                ErrorCode::InvalidParameters,
                "`data_directory` must be a path",
            ));
        }
        None => store::default_data_directory(),
    };
    let text =
        std::fs::read_to_string(&path).map_err(|e| failed(format!("{}: {e}", path.display())))?;
    let (body, _) =
        resolved::split(&text).map_err(|e| failed(format!("{}: {e}", path.display())))?;
    let root: Table = text
        .parse()
        .map_err(|e| failed(format!("{}: not valid TOML: {e}", path.display())))?;
    let old = resolved::read(&root).map_err(|e| failed(format!("{}: {e}", path.display())))?;

    // The user's library first; parts only embedded before second.
    let listed = match &data {
        Some(root) => library::list(root).map_err(failed)?,
        None => Vec::new(),
    };
    let mut parts: Vec<Part> = listed.iter().map(|(p, _)| p.clone()).collect();
    for entry in &old.parts {
        if !parts.iter().any(|p| p.has_key(&entry.name)) {
            parts.push(entry.value.clone());
        }
    }
    let library = Library::new(parts).map_err(failed)?;
    let used = RefCell::new(Used::default());
    let part_keys = trace_uses(body, &path, data.as_deref(), &old, &library, &used)?;
    let used = used.into_inner();

    let mut not_embedded: Vec<Value> = Vec::new();
    let mut carried: Vec<Value> = Vec::new();
    let mut part_entries: Vec<(String, Table)> = Vec::new();
    let mut seen = Vec::new();
    for key in part_keys {
        if seen.contains(&key) {
            continue;
        }
        seen.push(key.clone());
        match listed.iter().find(|(p, _)| p.has_key(&key)) {
            Some((_, file)) => {
                let text = std::fs::read_to_string(file)
                    .map_err(|e| failed(format!("{}: {e}", file.display())))?;
                part_entries.push((key, part_entry(&text)?));
            }
            None => {
                let entry = old.parts.iter().find(|e| e.name == key).ok_or_else(|| {
                    failed(format!(
                        "part `{key}` is in neither the library nor the file"
                    ))
                })?;
                carried.push(json!({ "kind": "part", "name": key }));
                part_entries.push((key, entry.table.clone()));
            }
        }
    }
    let mut curve_entries: Vec<(String, Table)> = Vec::new();
    for (name, found) in &used.curves {
        match (found, &data) {
            (Found::Data, Some(root)) => {
                let record = curves::record(root, name).map_err(failed)?;
                if record["redistribute"] != true && !include_restricted {
                    not_embedded.push(json!({
                        "kind": "curve",
                        "name": name,
                        "source": text_of(&record, "source"),
                        "reason": "its source does not allow redistribution (redistribute = false)",
                    }));
                    continue;
                }
                let curve = curves::load(root, name).map_err(failed)?;
                let mut table = Table::new();
                table.insert("sha256".into(), Toml::String(text_of(&record, "sha256")));
                table.insert("source".into(), Toml::String(text_of(&record, "source")));
                table.insert(
                    "redistribute".into(),
                    Toml::Boolean(record["redistribute"] == true),
                );
                if let Some(aoi) = record["angle_of_incidence_deg"].as_f64() {
                    table.insert("angle_of_incidence_deg".into(), Toml::Float(aoi));
                }
                table.insert(
                    "points".into(),
                    Toml::Array(
                        curve
                            .points()
                            .iter()
                            .map(|&(w, r, t)| {
                                Toml::Array(vec![Toml::Float(w), Toml::Float(r), Toml::Float(t)])
                            })
                            .collect(),
                    ),
                );
                curve_entries.push((name.clone(), table));
            }
            _ => {
                if let Some(entry) = old.curves.iter().find(|e| &e.name == name) {
                    carried.push(json!({ "kind": "curve", "name": name }));
                    curve_entries.push((name.clone(), entry.table.clone()));
                }
            }
        }
    }
    let mut material_entries: Vec<(String, Table)> = Vec::new();
    for (name, found) in &used.materials {
        match (found, &data) {
            (Found::Data, Some(root)) => {
                let (file, record) =
                    store::imported(root, name).map_err(|e| failed(e.to_string()))?;
                if !record.redistribute && !include_restricted {
                    not_embedded.push(json!({
                        "kind": "material",
                        "name": name,
                        "source": record.source,
                        "reason": "its source does not allow redistribution (redistribute = false)",
                    }));
                    continue;
                }
                let mut table = Table::new();
                table.insert("sha256".into(), Toml::String(record.sha256.clone()));
                table.insert("source".into(), Toml::String(record.source.clone()));
                table.insert("redistribute".into(), Toml::Boolean(record.redistribute));
                table.insert("references".into(), Toml::String(record.references.clone()));
                table.insert("data".into(), Toml::String(file));
                material_entries.push((name.clone(), table));
            }
            _ => {
                if let Some(entry) = old.materials.iter().find(|e| &e.name == name) {
                    carried.push(json!({ "kind": "material", "name": name }));
                    material_entries.push((name.clone(), entry.table.clone()));
                }
            }
        }
    }

    let mut new_text = body.trim_end().to_owned();
    new_text.push('\n');
    let nothing =
        part_entries.is_empty() && curve_entries.is_empty() && material_entries.is_empty();
    if !nothing {
        new_text.push('\n');
        new_text.push_str(&resolved::write(
            &part_entries,
            &curve_entries,
            &material_entries,
        ));
    }

    // Self-contained: it reads with nothing but the bundled materials.
    let no_library = Library::default();
    let bundled_only = |name: &str| store::material(None, name);
    let alone = Lookups {
        materials: &bundled_only,
        curves: &optobench_io::coatings::no_curves,
        library: &no_library,
    };
    let standalone = match sidecar::kind_of(&new_text).as_deref() {
        Some("sidecar") => sidecar::read(&new_text, &path, &alone).map(|_| ()),
        _ => scene_file::parse_with(&new_text, &alone).map(|_| ()),
    };
    std::fs::write(&out, &new_text).map_err(|e| failed(format!("{}: {e}", out.display())))?;
    let names = |list: &[(String, Table)]| list.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    let mut result = json!({
        "path": out.to_string_lossy(),
        "embedded": {
            "parts": names(&part_entries),
            "curves": names(&curve_entries),
            "materials": names(&material_entries),
        },
        "carried_over": carried,
        "not_embedded": not_embedded,
        "self_contained": standalone.is_ok(),
    });
    if let Err(e) = standalone {
        result["needs"] = json!(e.to_string());
    }
    Ok(result)
}

/// For `trace`: how each embedded value compares with the user's data — `same`,
/// `differs` (the file's value is used; `embed` again to take the new one) or `absent`.
pub(crate) fn compare(text: &str, data: Option<&Path>) -> Result<Value, CommandError> {
    let root: Table = text.parse().map_err(|e| failed(format!("{e}")))?;
    let embedded = resolved::read(&root).map_err(|e| failed(e.to_string()))?;
    let status = |theirs: Option<String>, ours: &str| match theirs {
        None => "absent",
        Some(sha) if sha == ours => "same",
        Some(_) => "differs",
    };
    let listed = match data {
        Some(root) => library::list(root).unwrap_or_default(),
        None => Vec::new(),
    };
    let mut out = Vec::new();
    for e in &embedded.parts {
        let theirs = listed
            .iter()
            .find(|(p, _)| p.has_key(&e.name))
            .and_then(|(_, file)| std::fs::read(file).ok())
            .map(|bytes| sha256_hex(&bytes));
        out.push(json!({ "kind": "part", "name": e.name, "library": status(theirs, &e.sha256) }));
    }
    for e in &embedded.curves {
        let theirs = data
            .and_then(|root| curves::record(root, &e.name).ok())
            .map(|r| text_of(&r, "sha256"));
        out.push(json!({ "kind": "curve", "name": e.name, "library": status(theirs, &e.sha256) }));
    }
    for e in &embedded.materials {
        let theirs = data
            .and_then(|root| store::imported(root, &e.name).ok())
            .map(|(_, r)| r.sha256);
        out.push(
            json!({ "kind": "material", "name": e.name, "library": status(theirs, &e.sha256) }),
        );
    }
    Ok(json!(out))
}
