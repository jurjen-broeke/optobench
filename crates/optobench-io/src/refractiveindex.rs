//! Reader for material files from the refractiveindex.info database.
//!
//! The files are a small, regular subset of YAML (see the database's
//! `database/data/**/*.yml`); this reader accepts exactly that subset and rejects
//! anything else, naming what it did not expect:
//!
//! ```text
//! # comment lines (the licence notice)
//! REFERENCES: |
//!     block text
//! COMMENTS: |          (NOTES on the database's develop branch)
//!     block text
//! DATA:
//!   - type: formula 1 | tabulated n | tabulated k | tabulated nk
//!     wavelength_range: 0.21 6.7            (formulas)
//!     coefficients: 0 0.69 0.068 …          (formulas)
//!     data: |                               (tables: rows of λ µm and n and/or k)
//!         0.200 1.396 1.10E-7
//! CONDITIONS:
//!     temperature: 293
//! SPECS:
//!     …                                     (kept as text, not interpreted)
//! ```
//!
//! Formula numbers and their equations: `optobench_materials::FormulaKind`.

use std::sync::Arc;

use optobench_materials::{
    Composite, DispersionFormula, FormulaKind, Material, Medium, TableError, Tabulated,
};

/// One entry of the DATA list.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// A dispersion formula for n.
    Formula {
        /// Formula number, 1 to 9.
        number: u32,
        /// Valid range (µm).
        range: (f64, f64),
        /// C₁, C₂, …
        coefficients: Vec<f64>,
    },
    /// Rows of (λ µm, n).
    TabulatedN(Vec<(f64, f64)>),
    /// Rows of (λ µm, k).
    TabulatedK(Vec<(f64, f64)>),
    /// Rows of (λ µm, n, k).
    TabulatedNk(Vec<(f64, f64, f64)>),
}

/// A parsed material file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MaterialFile {
    /// The comment lines before the first key, without `#`: usually the licence notice.
    pub notice: Vec<String>,
    /// REFERENCES text.
    pub references: String,
    /// COMMENTS (or NOTES) text.
    pub comments: String,
    /// The DATA entries in order.
    pub entries: Vec<Entry>,
    /// CONDITIONS as (key, value) text pairs.
    pub conditions: Vec<(String, String)>,
    /// SPECS as raw text, not interpreted.
    pub specs: String,
}

/// Why a file could not be read or turned into a material.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadError {
    /// A top-level key this reader does not know.
    UnknownKey {
        /// 1-based line number.
        line: usize,
        /// The key.
        key: String,
    },
    /// A line that does not fit the expected structure.
    Malformed {
        /// 1-based line number.
        line: usize,
        /// What was expected.
        expected: &'static str,
    },
    /// A DATA entry type this reader does not know.
    UnknownType {
        /// 1-based line number.
        line: usize,
        /// The type text.
        kind: String,
    },
    /// The entries do not describe one material: no source of n, or more than one source
    /// of n or k.
    Inconsistent(&'static str),
    /// A table is not valid (order, values).
    Table(TableError),
}

impl core::fmt::Display for ReadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ReadError::UnknownKey { line, key } => write!(
                f,
                "line {line}: unknown key `{key}`; valid keys: REFERENCES, COMMENTS, NOTES, DATA, CONDITIONS, SPECS"
            ),
            ReadError::Malformed { line, expected } => {
                write!(f, "line {line}: expected {expected}")
            }
            ReadError::UnknownType { line, kind } => write!(
                f,
                "line {line}: unknown data type `{kind}`; valid: formula 1 to 9, tabulated n, tabulated k, tabulated nk"
            ),
            ReadError::Inconsistent(what) => write!(f, "{what}"),
            ReadError::Table(error) => write!(f, "invalid table: {error:?}"),
        }
    }
}

impl std::error::Error for ReadError {}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn numbers(text: &str, line: usize) -> Result<Vec<f64>, ReadError> {
    text.split_whitespace()
        .map(|v| v.parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| ReadError::Malformed {
            line,
            expected: "numbers separated by spaces",
        })
}

/// Lines indented more than `parent`, starting at `start`; returns them with their
/// 1-based numbers and the index after the block. Blank lines inside are kept.
fn block<'a>(lines: &[&'a str], start: usize, parent: usize) -> (Vec<(usize, &'a str)>, usize) {
    let mut end = start;
    while end < lines.len() {
        let line = lines[end];
        if !line.trim().is_empty() && indent(line) <= parent {
            break;
        }
        end += 1;
    }
    let taken = (start..end).map(|i| (i + 1, lines[i])).collect();
    (taken, end)
}

/// Block text with the common indentation removed, trailing blank lines dropped.
fn dedent(lines: &[(usize, &str)]) -> String {
    let common = lines
        .iter()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(_, l)| indent(l))
        .min()
        .unwrap_or(0);
    let text: Vec<&str> = lines
        .iter()
        .map(|(_, l)| {
            if l.len() >= common {
                &l[common..]
            } else {
                l.trim()
            }
        })
        .collect();
    text.join("\n").trim_end().to_owned()
}

fn rows(lines: &[(usize, &str)], columns: usize) -> Result<Vec<Vec<f64>>, ReadError> {
    lines
        .iter()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|&(number, l)| {
            let values = numbers(l, number)?;
            if values.len() == columns {
                Ok(values)
            } else {
                Err(ReadError::Malformed {
                    line: number,
                    expected: "one value per column in every table row",
                })
            }
        })
        .collect()
}

fn parse_entry(lines: &[(usize, &str)]) -> Result<Entry, ReadError> {
    // Lines of one entry: the first starts with "- ", the rest are indented under it.
    let mut kind: Option<(usize, String)> = None;
    let mut range = None;
    let mut coefficients = None;
    let mut data = Vec::new();
    let item_indent = lines.first().map_or(0, |(_, l)| indent(l) + 2);
    let mut i = 0;
    while i < lines.len() {
        let (number, raw) = lines[i];
        let text = raw.trim_start().trim_start_matches("- ").trim();
        if text.is_empty() {
            i += 1;
            continue;
        }
        let (key, value) = text.split_once(':').ok_or(ReadError::Malformed {
            line: number,
            expected: "`key: value` inside a DATA entry",
        })?;
        let value = value.trim();
        match key.trim() {
            "type" => kind = Some((number, value.to_owned())),
            "wavelength_range" => {
                let v = numbers(value, number)?;
                let [low, high] = v[..] else {
                    return Err(ReadError::Malformed {
                        line: number,
                        expected: "two numbers for wavelength_range",
                    });
                };
                range = Some((low, high));
            }
            "coefficients" => coefficients = Some(numbers(value, number)?),
            "data" if value == "|" => {
                let rest: Vec<&str> = lines[i + 1..].iter().map(|(_, l)| *l).collect();
                let (taken, end) = block(&rest, 0, item_indent);
                data = taken
                    .into_iter()
                    .map(|(offset, l)| (number + offset, l))
                    .collect();
                i += end;
            }
            _ => {
                return Err(ReadError::Malformed {
                    line: number,
                    expected: "type, wavelength_range, coefficients or data in a DATA entry",
                });
            }
        }
        i += 1;
    }
    let (line, kind) = kind.ok_or(ReadError::Malformed {
        line: lines.first().map_or(0, |(n, _)| *n),
        expected: "a `type:` in every DATA entry",
    })?;
    if let Some(number) = kind.strip_prefix("formula ") {
        let number: u32 = number.trim().parse().map_err(|_| ReadError::UnknownType {
            line,
            kind: kind.clone(),
        })?;
        if FormulaKind::from_number(number).is_none() {
            return Err(ReadError::UnknownType { line, kind });
        }
        return Ok(Entry::Formula {
            number,
            range: range.ok_or(ReadError::Malformed {
                line,
                expected: "wavelength_range for a formula",
            })?,
            coefficients: coefficients.ok_or(ReadError::Malformed {
                line,
                expected: "coefficients for a formula",
            })?,
        });
    }
    let pairs = |rows: Vec<Vec<f64>>| rows.into_iter().map(|r| (r[0], r[1])).collect();
    match kind.as_str() {
        "tabulated n" => Ok(Entry::TabulatedN(pairs(rows(&data, 2)?))),
        "tabulated k" => Ok(Entry::TabulatedK(pairs(rows(&data, 2)?))),
        "tabulated nk" => Ok(Entry::TabulatedNk(
            rows(&data, 3)?
                .into_iter()
                .map(|r| (r[0], r[1], r[2]))
                .collect(),
        )),
        _ => Err(ReadError::UnknownType { line, kind }),
    }
}

/// Parses a material file.
pub fn parse(text: &str) -> Result<MaterialFile, ReadError> {
    let lines: Vec<&str> = text.lines().collect();
    let mut file = MaterialFile::default();
    let mut i = 0;
    // Leading comments: the notice.
    while i < lines.len() && (lines[i].trim().is_empty() || lines[i].starts_with('#')) {
        if let Some(comment) = lines[i].strip_prefix('#') {
            file.notice.push(comment.trim().to_owned());
        }
        i += 1;
    }
    while i < lines.len() {
        let line = lines[i];
        let number = i + 1;
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            i += 1;
            continue;
        }
        if indent(line) != 0 {
            return Err(ReadError::Malformed {
                line: number,
                expected: "a top-level key at the start of the line",
            });
        }
        let (key, value) = line.split_once(':').ok_or(ReadError::Malformed {
            line: number,
            expected: "`KEY:` at the top level",
        })?;
        let (body, next) = block(&lines, i + 1, 0);
        let value = value.trim();
        match key.trim() {
            "REFERENCES" => file.references = dedent(&body),
            "COMMENTS" | "NOTES" => file.comments = dedent(&body),
            "SPECS" => file.specs = dedent(&body),
            "CONDITIONS" => {
                for &(n, l) in body.iter().filter(|(_, l)| !l.trim().is_empty()) {
                    let (k, v) = l.trim().split_once(':').ok_or(ReadError::Malformed {
                        line: n,
                        expected: "`key: value` in CONDITIONS",
                    })?;
                    file.conditions
                        .push((k.trim().to_owned(), v.trim().to_owned()));
                }
            }
            "DATA" => {
                if !value.is_empty() {
                    return Err(ReadError::Malformed {
                        line: number,
                        expected: "a list of entries under DATA",
                    });
                }
                // Split the block into entries at lines starting with "- ".
                let starts: Vec<usize> = body
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, l))| l.trim_start().starts_with("- "))
                    .map(|(k, _)| k)
                    .collect();
                if starts.first() != Some(&0) {
                    return Err(ReadError::Malformed {
                        line: number + 1,
                        expected: "`- type: …` as the first line of every DATA entry",
                    });
                }
                for (k, &start) in starts.iter().enumerate() {
                    let end = starts.get(k + 1).copied().unwrap_or(body.len());
                    file.entries.push(parse_entry(&body[start..end])?);
                }
            }
            other => {
                return Err(ReadError::UnknownKey {
                    line: number,
                    key: other.to_owned(),
                });
            }
        }
        i = next;
    }
    if file.entries.is_empty() {
        return Err(ReadError::Inconsistent("the file has no DATA entries"));
    }
    Ok(file)
}

impl MaterialFile {
    /// The material the entries describe: one source of n (a formula, `tabulated n` or
    /// `tabulated nk`) and at most one source of k.
    pub fn material(&self) -> Result<Medium, ReadError> {
        let mut index: Option<Arc<dyn Material>> = None;
        let mut extinction: Option<Arc<dyn Material>> = None;
        let set = |slot: &mut Option<Arc<dyn Material>>, m: Arc<dyn Material>, what| {
            if slot.replace(m).is_some() {
                Err(ReadError::Inconsistent(what))
            } else {
                Ok(())
            }
        };
        const TWO_N: &str = "more than one source of the refractive index n";
        const TWO_K: &str = "more than one source of the extinction coefficient k";
        for entry in &self.entries {
            match entry {
                Entry::Formula {
                    number,
                    range,
                    coefficients,
                } => {
                    let kind = FormulaKind::from_number(*number)
                        .ok_or(ReadError::Inconsistent("unknown formula number"))?;
                    let formula = DispersionFormula {
                        kind,
                        coefficients: coefficients.clone(),
                        range: *range,
                    };
                    set(&mut index, Arc::new(formula), TWO_N)?;
                }
                Entry::TabulatedN(rows) => {
                    let table = Tabulated::new(rows.iter().map(|&(w, n)| (w, n, 0.0)).collect())
                        .map_err(ReadError::Table)?;
                    set(&mut index, Arc::new(table), TWO_N)?;
                }
                Entry::TabulatedK(rows) => {
                    // n is a placeholder; only the extinction of this table is used.
                    let table = Tabulated::new(rows.iter().map(|&(w, k)| (w, 1.0, k)).collect())
                        .map_err(ReadError::Table)?;
                    set(&mut extinction, Arc::new(table), TWO_K)?;
                }
                Entry::TabulatedNk(rows) => {
                    let table = Arc::new(Tabulated::new(rows.clone()).map_err(ReadError::Table)?);
                    set(&mut index, table.clone(), TWO_N)?;
                    set(&mut extinction, table, TWO_K)?;
                }
            }
        }
        let index = index.ok_or(ReadError::Inconsistent(
            "no source of the refractive index n (only k is given)",
        ))?;
        Ok(Medium::new(Composite { index, extinction }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shapes copied from the database files `main/SiO2/nk/Malitson.yml` and
    // `main/H2O/nk/Hale.yml` (CC0), shortened.
    const FORMULA: &str = "\
# this file is part of refractiveindex.info database
# refractiveindex.info database is in the public domain
# copyright and related rights waived via CC0 1.0

REFERENCES: |
    1) I. H. Malitson.
    <a href=\"https://doi.org/10.1364/JOSA.55.001205\">J. Opt. Soc. Am. 55, 1205-1208 (1965)</a><br>
COMMENTS: |
    Fused silica, 20 °C
DATA:
  - type: formula 1
    wavelength_range: 0.21 6.7
    coefficients: 0 0.6961663 0.0684043 0.4079426 0.1162414 0.8974794 9.896161
CONDITIONS:
    temperature: 293
";

    const TABLE: &str = "\
REFERENCES: |
    G. M. Hale and M. R. Querry.
COMMENTS: |
    Liquid water at 25 °C
DATA:
  - type: tabulated nk
    data: |
        0.200 1.396 1.10E-7
        0.225 1.373 4.90E-8
        0.250 1.362 3.35E-8
CONDITIONS:
    temperature: 298
";

    #[test]
    fn reads_a_formula_file_and_matches_the_bundled_material() {
        let file = parse(FORMULA).expect("valid");
        assert_eq!(file.notice.len(), 3);
        assert!(file.notice[2].contains("CC0"));
        assert!(file.references.contains("Malitson"));
        assert_eq!(file.comments, "Fused silica, 20 °C");
        assert_eq!(
            file.conditions,
            vec![("temperature".to_owned(), "293".to_owned())]
        );
        let medium = file.material().expect("one material");
        let bundled = optobench_materials::bundled::fused_silica();
        for w in [0.3, 0.6, 2.0] {
            let a = medium.material().refractive_index(w).expect("in range");
            let b = bundled.refractive_index(w).expect("in range");
            assert!((a - b).abs() < 1e-15);
        }
    }

    #[test]
    fn reads_a_table_file() {
        let file = parse(TABLE).expect("valid");
        assert_eq!(
            file.entries,
            vec![Entry::TabulatedNk(vec![
                (0.2, 1.396, 1.10e-7),
                (0.225, 1.373, 4.90e-8),
                (0.25, 1.362, 3.35e-8),
            ])]
        );
        let medium = file.material().expect("one material");
        let k = medium.material().extinction(0.225).expect("in range");
        assert!((k - 4.90e-8).abs() < 1e-20);
    }

    #[test]
    fn formula_with_separate_k_table_and_notes_key() {
        let text = "\
NOTES: |
    develop-branch name for COMMENTS
DATA:
  - type: formula 2
    wavelength_range: 0.4 0.8
    coefficients: 0 1.25 0
  - type: tabulated k
    data: |
        0.4 1e-6
        0.8 3e-6
";
        let file = parse(text).expect("valid");
        assert_eq!(file.comments, "develop-branch name for COMMENTS");
        let medium = file.material().expect("one material");
        let m = medium.material();
        assert!((m.refractive_index(0.6).expect("in range") - 1.5).abs() < 1e-15);
        assert!((m.extinction(0.6).expect("in range") - 2e-6).abs() < 1e-18);
    }

    #[test]
    fn rejects_what_it_does_not_know() {
        assert!(matches!(
            parse("REFERENCE: |\n    typo\n"),
            Err(ReadError::UnknownKey { line: 1, .. })
        ));
        assert!(matches!(
            parse(
                "DATA:\n  - type: formula 12\n    wavelength_range: 0.4 0.8\n    coefficients: 1\n"
            ),
            Err(ReadError::UnknownType { line: 2, .. })
        ));
        assert!(matches!(
            parse("DATA:\n  - type: tabulated nk\n    data: |\n        0.4 1.5\n"),
            Err(ReadError::Malformed { line: 4, .. })
        ));
        let only_k = parse(
            "DATA:\n  - type: tabulated k\n    data: |\n        0.4 1e-6\n        0.5 1e-6\n",
        )
        .expect("parses");
        assert!(matches!(only_k.material(), Err(ReadError::Inconsistent(_))));
        assert!(matches!(
            parse("# only comments\n"),
            Err(ReadError::Inconsistent(_))
        ));
    }
}
