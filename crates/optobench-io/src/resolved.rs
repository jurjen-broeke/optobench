//! Embedded parameters: the `[resolved]` section at the end of a layout or sidecar
//! (docs/design.md, Sidecar and library formats, "Embedded parameters").
//!
//! It holds what a trace took from the user's own data, so a second user can trace the
//! design without it: library parts, imported R/T curves and imported materials, each
//! with the checksum of the file it came from.
//!
//! ```toml
//! [resolved.part."thorlabs/LA1951-ML"]
//! sha256 = "…"                     # of the part file
//! part = "LA1951-ML"               # … the part file's own keys
//!
//! [resolved.curve."mvf52a"]
//! sha256 = "…"                     # of the imported file
//! source = "…"
//! redistribute = false
//! points = [                       # [wavelength µm, reflectance, transmittance]
//!   [0.3, 0.988, 0.012],
//! ]
//!
//! [resolved.material."n-bk7"]
//! sha256 = "…"
//! source = "refractiveindex.info"
//! redistribute = false
//! data = "…"                       # the imported file, unchanged
//! ```
//!
//! When a file has an embedded value it is used, whether or not the user's own data has
//! the same item, so the design traces as it was saved; `trace` reports where the user's
//! data differs. Embedding values whose source does not allow redistribution needs the
//! user's consent (the `embed` command's `include_restricted`).

use optobench_library::{Library, Part};
use optobench_materials::Medium;
use optobench_trace::coating::CoatingCurve;
use toml::{Table, Value};

use crate::scene_file::{FileError, check_keys, error};

/// One embedded item.
#[derive(Debug, Clone)]
pub struct Entry<T> {
    /// Its name (a part's key, a curve's or material's name).
    pub name: String,
    /// The value.
    pub value: T,
    /// The checksum of the file it came from.
    pub sha256: String,
    /// The entry as written, to carry over when the file is embedded again.
    pub table: Table,
}

/// The `[resolved]` section of a file.
#[derive(Debug, Clone, Default)]
pub struct Embedded {
    /// Library parts.
    pub parts: Vec<Entry<Part>>,
    /// R/T curves.
    pub curves: Vec<Entry<CoatingCurve>>,
    /// Materials.
    pub materials: Vec<Entry<Medium>>,
}

fn entries<'a>(resolved: &'a Table, kind: &str) -> Result<Vec<(&'a String, &'a Table)>, FileError> {
    match resolved.get(kind) {
        None => Ok(Vec::new()),
        Some(Value::Table(t)) => t
            .iter()
            .map(|(name, v)| {
                v.as_table()
                    .map(|table| (name, table))
                    .ok_or_else(|| error(format!("[resolved.{kind}.\"{name}\"] must be a table")))
            })
            .collect(),
        Some(_) => Err(error(format!("[resolved.{kind}] must be a table"))),
    }
}

fn checksum(table: &Table, context: &str) -> Result<String, FileError> {
    table
        .get("sha256")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| error(format!("{context}: missing `sha256`")))
}

/// Reads the `[resolved]` section of a file's top-level table (empty without one).
pub fn read(root: &Table) -> Result<Embedded, FileError> {
    let Some(value) = root.get("resolved") else {
        return Ok(Embedded::default());
    };
    let resolved = value
        .as_table()
        .ok_or_else(|| error("[resolved] must be a table"))?;
    check_keys(resolved, &["part", "curve", "material"], "[resolved]")?;
    let mut embedded = Embedded::default();
    for (name, table) in entries(resolved, "part")? {
        let context = format!("[resolved.part.\"{name}\"]");
        let mut fields = table.clone();
        fields.remove("sha256");
        let text = format!(
            "format = \"{}\"\nkind = \"{}\"\n{}",
            crate::format_tag(),
            crate::library::KIND,
            toml::to_string(&fields).map_err(|e| error(format!("{context}: {e}")))?
        );
        let part = crate::library::parse(&text).map_err(|e| error(format!("{context}: {e}")))?;
        if !part.has_key(name) {
            return Err(error(format!("{context}: holds part `{}`", part.key())));
        }
        embedded.parts.push(Entry {
            name: name.clone(),
            value: part,
            sha256: checksum(table, &context)?,
            table: table.clone(),
        });
    }
    for (name, table) in entries(resolved, "curve")? {
        let context = format!("[resolved.curve.\"{name}\"]");
        check_keys(
            table,
            &[
                "sha256",
                "source",
                "redistribute",
                "angle_of_incidence_deg",
                "points",
            ],
            &context,
        )?;
        let points = table
            .get("points")
            .and_then(Value::as_array)
            .and_then(|rows| {
                rows.iter()
                    .map(|row| {
                        let row = row.as_array()?;
                        let n = |i: usize| {
                            row.get(i).and_then(|v| {
                                v.as_float().or_else(|| v.as_integer().map(|i| i as f64))
                            })
                        };
                        (row.len() == 3).then_some(())?;
                        Some((n(0)?, n(1)?, n(2)?))
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .ok_or_else(|| {
                error(format!(
                    "{context}: `points` is a list of [wavelength_um, reflectance, transmittance]"
                ))
            })?;
        let curve = CoatingCurve::new(points).map_err(|e| error(format!("{context}: {e:?}")))?;
        embedded.curves.push(Entry {
            name: name.clone(),
            value: curve,
            sha256: checksum(table, &context)?,
            table: table.clone(),
        });
    }
    for (name, table) in entries(resolved, "material")? {
        let context = format!("[resolved.material.\"{name}\"]");
        check_keys(
            table,
            &["sha256", "source", "redistribute", "references", "data"],
            &context,
        )?;
        let data = table
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| error(format!("{context}: missing `data`")))?;
        let medium = crate::refractiveindex::parse(data)
            .and_then(|file| file.material())
            .map_err(|e| error(format!("{context}: {e}")))?;
        embedded.materials.push(Entry {
            name: name.clone(),
            value: medium,
            sha256: checksum(table, &context)?,
            table: table.clone(),
        });
    }
    Ok(embedded)
}

impl Embedded {
    /// Whether nothing is embedded.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty() && self.curves.is_empty() && self.materials.is_empty()
    }

    /// The embedded material of this name.
    pub fn material(&self, name: &str) -> Option<Medium> {
        self.materials
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.value.clone())
    }

    /// The embedded curve of this name.
    pub fn curve(&self, name: &str) -> Option<CoatingCurve> {
        self.curves
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.value.clone())
    }

    /// `base` with the embedded parts in place of the library's own (and of any library
    /// part that claims the same STEP product).
    pub fn library(&self, base: &Library) -> Result<Library, FileError> {
        if self.parts.is_empty() {
            return Ok(base.clone());
        }
        let embedded: Vec<&Part> = self.parts.iter().map(|e| &e.value).collect();
        let mut parts: Vec<Part> = base
            .parts()
            .iter()
            .filter(|p| {
                !embedded.iter().any(|e| {
                    e.has_key(&p.key())
                        || core::iter::once(&p.number)
                            .chain(&p.products)
                            .any(|n| e.names_product(n))
                })
            })
            .cloned()
            .collect();
        parts.extend(embedded.into_iter().cloned());
        Library::new(parts).map_err(|e| error(format!("[resolved]: {e}")))
    }
}

/// The comment `write` puts above the section.
pub const COMMENT: &str = "# Embedded by optobench `embed`: the values this design takes from the user's own\n\
     # data, so it traces without them. Regenerate with `embed`; do not edit by hand.\n";

/// Splits a file into what comes before its `[resolved]` section and that section (with
/// the comment `write` puts above it). The section must be the last thing in the file.
pub fn split(text: &str) -> Result<(&str, Option<&str>), FileError> {
    let mut offset = 0;
    let mut start = None;
    let mut in_string = false;
    for line in text.split_inclusive('\n') {
        // Lines inside multi-line strings are not headers.
        let quotes = line.matches("\"\"\"").count() + line.matches("'''").count();
        let was_in_string = in_string;
        if quotes % 2 == 1 {
            in_string = !in_string;
        }
        let trimmed = line.trim_end();
        // A table header: `[name]` or `[[name]]` from the first column, nothing after it
        // but a comment (array values such as `  [0.4, 0.5, 0.5],` are not).
        let header = trimmed.split('#').next().unwrap_or("").trim_end();
        if !was_in_string && header.starts_with('[') && header.ends_with(']') {
            let resolved = header.starts_with("[resolved]") || header.starts_with("[resolved.");
            match (start, resolved) {
                (None, true) => start = Some(offset),
                (Some(_), false) => {
                    return Err(error(
                        "[resolved] must be the last section of the file; move the tables after it above it",
                    ));
                }
                _ => {}
            }
        }
        offset += line.len();
    }
    Ok(match start {
        Some(s) => {
            let before = text[..s].trim_end_matches(['\n', '\r']);
            let s = match before.strip_suffix(COMMENT.trim_end()) {
                Some(rest) => rest.len(),
                None => s,
            };
            (&text[..s], Some(&text[s..]))
        }
        None => (text, None),
    })
}

fn quoted(key: &str) -> String {
    Value::String(key.to_owned()).to_string()
}

/// Writes entries as a `[resolved]` section, each as `[resolved.<kind>."<name>"]`, keys
/// in the order given; curve points one per line.
pub fn write(
    parts: &[(String, Table)],
    curves: &[(String, Table)],
    materials: &[(String, Table)],
) -> String {
    let mut out = String::from(COMMENT);
    for (kind, list) in [("part", parts), ("curve", curves), ("material", materials)] {
        for (name, table) in list {
            out.push_str(&format!("\n[resolved.{kind}.{}]\n", quoted(name)));
            for (key, value) in table {
                match (key.as_str(), value) {
                    ("points", Value::Array(rows)) => {
                        out.push_str("points = [\n");
                        for row in rows {
                            out.push_str(&format!("  {row},\n"));
                        }
                        out.push_str("]\n");
                    }
                    _ => out.push_str(&format!("{} = {value}\n", key_text(key))),
                }
            }
        }
    }
    out
}

/// A bare TOML key when it is one, else quoted.
fn key_text(key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        key.to_owned()
    } else {
        quoted(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_finds_the_last_section() {
        let text = "format = \"optobench/1\"\nkind = \"layout\"\n[[element]]\nid = \"a\"\n[resolved.curve.\"x\"]\nsha256 = \"0\"\n";
        let (body, resolved) = split(text).expect("splits");
        assert!(body.ends_with("id = \"a\"\n"));
        assert!(resolved.expect("there").starts_with("[resolved.curve"));
        assert!(split("[resolved.curve.x]\n[[element]]\n").is_err());
        // Rows of a points array are not headers.
        let points =
            "kind = 1\n[resolved.curve.x]\npoints = [\n  [0.4, 0.5, 0.5],\n  [0.7, 0.1, 0.9]\n]\n";
        assert!(split(points).expect("splits").1.is_some());
        assert_eq!(split("kind = 1\n").expect("splits").1, None);
    }

    #[test]
    fn written_sections_read_back() {
        let mut curve = Table::new();
        curve.insert("sha256".into(), Value::String("ab".into()));
        curve.insert("redistribute".into(), Value::Boolean(false));
        curve.insert(
            "points".into(),
            Value::Array(
                [[0.4, 0.5, 0.5], [0.7, 0.1, 0.9]]
                    .iter()
                    .map(|r| Value::Array(r.iter().map(|&v| Value::Float(v)).collect()))
                    .collect(),
            ),
        );
        let text = write(&[], &[("my curve".into(), curve)], &[]);
        let root: Table = text.parse().expect("valid TOML");
        let embedded = read(&root).expect("reads");
        assert_eq!(embedded.curves[0].name, "my curve");
        assert_eq!(embedded.curves[0].sha256, "ab");
        assert!(embedded.curve("my curve").is_some());
        assert!(text.contains("points = [\n  [0.4, 0.5, 0.5],\n"));
    }
}
