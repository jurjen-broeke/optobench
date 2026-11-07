//! Imported material files in the user's data directory.
//!
//! Layout (docs/design.md, File naming and versioning, "Downloaded sources"):
//!
//! ```text
//! <data directory>/optobench/sources/<source>/<version>/
//!     <name>.yml          the imported file, unchanged
//!     provenance.json     where it came from, its checksum, notice and terms
//! ```
//!
//! `<version>` is the first 16 hexadecimal digits of the file's SHA-256, so re-importing
//! the same file is harmless and a changed file never overwrites an older one. Never in a
//! project folder, never in a shared design.
//!
//! Every import is marked `redistribute = false` unless the user says otherwise: the
//! database's literature and manufacturer files carry the same CC0 header, so the file
//! cannot tell which is which (docs/licence-review.md, recommendations 2 and 3).

use std::fs;
use std::path::{Path, PathBuf};

use optobench_materials::Medium;
use serde_json::{Value, json};

use crate::refractiveindex::{ReadError, parse};
use crate::sha256::sha256_hex;

/// The source id for files from the refractiveindex.info database.
pub const REFRACTIVEINDEX: &str = "refractiveindex.info";

/// The platform's per-user data directory: `$XDG_DATA_HOME` or `~/.local/share` on Linux
/// and other Unix systems, `~/Library/Application Support` on macOS, `%APPDATA%` on
/// Windows. `None` if the environment does not say.
pub fn default_data_directory() -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(windows) {
        return var("APPDATA");
    }
    if cfg!(target_os = "macos") {
        return var("HOME").map(|home| home.join("Library").join("Application Support"));
    }
    var("XDG_DATA_HOME").or_else(|| var("HOME").map(|home| home.join(".local").join("share")))
}

/// What is recorded about an imported material.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// The name the user gave it.
    pub name: String,
    /// The source id, e.g. `refractiveindex.info`.
    pub source: String,
    /// The path it was imported from.
    pub original_path: String,
    /// SHA-256 of the file (hexadecimal).
    pub sha256: String,
    /// The file's own licence notice (its leading comment lines).
    pub notice: Vec<String>,
    /// The file's references.
    pub references: String,
    /// Whether the user allowed its values to be embedded in shared designs.
    pub redistribute: bool,
    /// Where the copy is stored.
    pub directory: PathBuf,
}

impl Record {
    /// As JSON, the form written to `provenance.json` and returned by commands.
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "source": self.source,
            "original_path": self.original_path,
            "sha256": self.sha256,
            "notice": self.notice,
            "references": self.references,
            "redistribute": self.redistribute,
            "directory": self.directory.to_string_lossy(),
        })
    }

    fn from_json(value: &Value, directory: &Path) -> Option<Self> {
        let text = |key: &str| value.get(key)?.as_str().map(str::to_owned);
        Some(Self {
            name: text("name")?,
            source: text("source")?,
            original_path: text("original_path")?,
            sha256: text("sha256")?,
            notice: value
                .get("notice")?
                .as_array()?
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
            references: text("references")?,
            redistribute: value.get("redistribute")?.as_bool()?,
            directory: directory.to_path_buf(),
        })
    }
}

/// Why an import, listing or load failed.
#[derive(Debug)]
pub enum StoreError {
    /// The name is not lowercase ASCII letters, digits, `-`, `_` and `.`, 1 to 64 long,
    /// starting with a letter or digit.
    InvalidName(String),
    /// Another file is already stored under this name.
    NameTaken {
        /// The name.
        name: String,
        /// Where the other file is.
        existing: PathBuf,
    },
    /// No imported material has this name.
    NotFound(String),
    /// The file is not a valid material file.
    Read(ReadError),
    /// The stored file no longer matches its recorded checksum.
    ChecksumMismatch(PathBuf),
    /// A file system error.
    Io(std::io::Error),
}

impl core::fmt::Display for StoreError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            StoreError::InvalidName(name) => write!(
                f,
                "invalid name `{name}`: use 1 to 64 lowercase letters, digits, '-', '_' or '.', starting with a letter or digit"
            ),
            StoreError::NameTaken { name, existing } => write!(
                f,
                "a different file is already imported as `{name}` ({})",
                existing.display()
            ),
            StoreError::NotFound(name) => write!(f, "no imported material named `{name}`"),
            StoreError::Read(error) => write!(f, "not a valid material file: {error}"),
            StoreError::ChecksumMismatch(path) => {
                write!(f, "{} changed since it was imported", path.display())
            }
            StoreError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        StoreError::Io(error)
    }
}

/// Checks a material name (generated file names are lowercase ASCII; docs/design.md).
pub fn check_name(name: &str) -> Result<(), StoreError> {
    let valid = (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b))
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(StoreError::InvalidName(name.to_owned()))
    }
}

fn sources(root: &Path) -> PathBuf {
    root.join("optobench").join("sources")
}

/// What a file would be imported as, without storing anything: parsed, checked, with its
/// notice and references, so they can be shown before the user accepts.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    /// SHA-256 of the file.
    pub sha256: String,
    /// The file's own licence notice.
    pub notice: Vec<String>,
    /// The file's references.
    pub references: String,
    /// The file's comments.
    pub comments: String,
}

/// Reads and checks a material file without storing it.
pub fn preview(path: &Path) -> Result<(Preview, Vec<u8>), StoreError> {
    let bytes = fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let file = parse(&text).map_err(StoreError::Read)?;
    file.material().map_err(StoreError::Read)?;
    Ok((
        Preview {
            sha256: sha256_hex(&bytes),
            notice: file.notice,
            references: file.references,
            comments: file.comments,
        },
        bytes,
    ))
}

/// Imports a refractiveindex.info material file under `name` into `root`.
pub fn import(
    root: &Path,
    path: &Path,
    name: &str,
    redistribute: bool,
) -> Result<Record, StoreError> {
    check_name(name)?;
    let (preview, bytes) = preview(path)?;
    if let Ok(existing) = find(root, name) {
        if existing.sha256 == preview.sha256 {
            return Ok(existing);
        }
        return Err(StoreError::NameTaken {
            name: name.to_owned(),
            existing: existing.directory,
        });
    }
    let directory = sources(root)
        .join(REFRACTIVEINDEX)
        .join(&preview.sha256[..16]);
    fs::create_dir_all(&directory)?;
    fs::write(directory.join(format!("{name}.yml")), &bytes)?;
    let record = Record {
        name: name.to_owned(),
        source: REFRACTIVEINDEX.to_owned(),
        original_path: path.to_string_lossy().into_owned(),
        sha256: preview.sha256,
        notice: preview.notice,
        references: preview.references,
        redistribute,
        directory: directory.clone(),
    };
    let json = serde_json::to_string_pretty(&record.to_json())
        .map_err(|e| StoreError::Io(std::io::Error::other(e)))?;
    fs::write(directory.join("provenance.json"), json)?;
    Ok(record)
}

/// Every imported material, sorted by name.
pub fn list(root: &Path) -> Result<Vec<Record>, StoreError> {
    let mut records = Vec::new();
    let Ok(sources) = fs::read_dir(sources(root)) else {
        return Ok(records);
    };
    for source in sources.flatten() {
        let Ok(versions) = fs::read_dir(source.path()) else {
            continue;
        };
        for version in versions.flatten() {
            let directory = version.path();
            let Ok(text) = fs::read_to_string(directory.join("provenance.json")) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if let Some(record) = Record::from_json(&value, &directory) {
                records.push(record);
            }
        }
    }
    records.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(records)
}

fn find(root: &Path, name: &str) -> Result<Record, StoreError> {
    list(root)?
        .into_iter()
        .find(|r| r.name == name)
        .ok_or_else(|| StoreError::NotFound(name.to_owned()))
}

/// Loads an imported material, checking the stored file against its checksum.
pub fn load(root: &Path, name: &str) -> Result<(Medium, Record), StoreError> {
    let record = find(root, name)?;
    let path = record.directory.join(format!("{}.yml", record.name));
    let bytes = fs::read(&path)?;
    if sha256_hex(&bytes) != record.sha256 {
        return Err(StoreError::ChecksumMismatch(path));
    }
    let medium = parse(&String::from_utf8_lossy(&bytes))
        .and_then(|file| file.material())
        .map_err(StoreError::Read)?;
    Ok((medium, record))
}

/// The bundled material names, as listed by the `materials` command.
pub const BUNDLED: &[&str] = &[
    "fused-silica",
    "standard-air",
    "water",
    "water-hale-querry-1973",
];

/// A material by name: a bundled material, or one imported into `root`. `water` is
/// water at 20 °C; `water-<t>` (for example `water-22.5`) is water at t °C, 19 to 24.
pub fn material(root: Option<&Path>, name: &str) -> Result<Medium, String> {
    use optobench_materials::bundled;
    match name {
        "fused-silica" => return Ok(bundled::fused_silica().into()),
        "standard-air" => return Ok(Medium::new(bundled::StandardAir)),
        "water-hale-querry-1973" => return Ok(bundled::water_hale_querry().into()),
        _ => {}
    }
    if let Some(temperature) = name
        .strip_prefix("water")
        .map(|t| t.trim_start_matches('-'))
    {
        let celsius = if temperature.is_empty() {
            Some(20.0)
        } else {
            temperature.parse::<f64>().ok()
        };
        if let Some(celsius) = celsius {
            return bundled::Water::new(celsius)
                .map(Medium::new)
                .ok_or_else(|| {
                    format!(
                        "water is available from {} to {} °C",
                        bundled::WATER_TEMPERATURES.0,
                        bundled::WATER_TEMPERATURES.1
                    )
                });
        }
    }
    let root = root.ok_or_else(|| {
        format!("`{name}` is not a bundled material, and no data directory is set to look for imported ones")
    })?;
    load(root, name).map(|(medium, _)| medium).map_err(|e| match e {
        StoreError::NotFound(_) => format!(
            "no material `{name}`: not bundled ({}) and not imported (see the `materials` command)",
            BUNDLED.join(", ")
        ),
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the system temporary directory, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "optobench-store-test-{label}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("temporary directory");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const FILE: &str = "\
# copyright and related rights waived via CC0 1.0
REFERENCES: |
    Test reference.
DATA:
  - type: formula 2
    wavelength_range: 0.4 0.8
    coefficients: 0 1.25 0
";

    #[test]
    fn import_list_and_load_round_trip() {
        let scratch = Scratch::new("round-trip");
        let source = scratch.0.join("download.yml");
        fs::write(&source, FILE).expect("write");
        let data = scratch.0.join("data");

        let record = import(&data, &source, "test-glass", false).expect("imports");
        assert!(!record.redistribute);
        assert_eq!(
            record.notice,
            vec!["copyright and related rights waived via CC0 1.0"]
        );
        assert!(
            record
                .directory
                .starts_with(data.join("optobench/sources/refractiveindex.info"))
        );
        // Importing the same file again under the same name is harmless.
        assert_eq!(
            import(&data, &source, "test-glass", false).expect("again"),
            record
        );

        let names: Vec<String> = list(&data)
            .expect("lists")
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, vec!["test-glass"]);
        let (medium, _) = load(&data, "test-glass").expect("loads");
        assert!((medium.material().refractive_index(0.6).expect("in range") - 1.5).abs() < 1e-15);
    }

    #[test]
    fn rejects_bad_names_taken_names_and_tampering() {
        let scratch = Scratch::new("rejects");
        let source = scratch.0.join("a.yml");
        fs::write(&source, FILE).expect("write");
        let data = scratch.0.join("data");
        for bad in ["", "Upper", "-dash", "with space", "slash/name"] {
            assert!(matches!(
                import(&data, &source, bad, false),
                Err(StoreError::InvalidName(_))
            ));
        }
        let record = import(&data, &source, "glass", false).expect("imports");
        let other = scratch.0.join("b.yml");
        fs::write(&other, FILE.replace("1.25", "1.0")).expect("write");
        assert!(matches!(
            import(&data, &other, "glass", false),
            Err(StoreError::NameTaken { .. })
        ));
        assert!(matches!(
            load(&data, "missing"),
            Err(StoreError::NotFound(_))
        ));

        fs::write(
            record.directory.join("glass.yml"),
            FILE.replace("1.25", "2.0"),
        )
        .expect("tamper");
        assert!(matches!(
            load(&data, "glass"),
            Err(StoreError::ChecksumMismatch(_))
        ));
    }

    #[test]
    fn invalid_files_are_not_stored() {
        let scratch = Scratch::new("invalid");
        let source = scratch.0.join("bad.yml");
        fs::write(&source, "NOT_A_KEY: 1\n").expect("write");
        let data = scratch.0.join("data");
        assert!(matches!(
            import(&data, &source, "bad", false),
            Err(StoreError::Read(_))
        ));
        assert!(list(&data).expect("lists").is_empty());
    }
}
