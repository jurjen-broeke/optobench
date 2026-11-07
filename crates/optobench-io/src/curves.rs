//! Measured R/T curves in the user's parts library: import of vendor tables (CSV as a
//! spreadsheet exports it) and lookup by name (docs/design.md, "Parts library").
//!
//! ```text
//! <data directory>/optobench/library/curves/<name>.csv    wavelength_um,reflectance,transmittance
//! <data directory>/optobench/library/curves/<name>.json   where it came from and how it was read
//! ```
//!
//! A curve is used as `coating = { curve = "<name>" }` in part, layout and sidecar files.
//! Imports are marked `redistribute = false` unless the user says otherwise: vendor data
//! stays on the user's machine.

use std::fs;
use std::path::{Path, PathBuf};

use optobench_trace::coating::CoatingCurve;
use serde_json::{Value, json};

use crate::sha256::sha256_hex;
use crate::store::check_name;

/// `<root>/optobench/library/curves`.
pub fn curves_directory(root: &Path) -> PathBuf {
    root.join("optobench").join("library").join("curves")
}

/// What a column of the table holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    /// Wavelength.
    Wavelength,
    /// Transmittance.
    Transmittance,
    /// Reflectance.
    Reflectance,
    /// Anything else, ignored.
    Skip,
}

impl Column {
    /// From its name in the import parameters.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "wavelength" => Column::Wavelength,
            "transmittance" => Column::Transmittance,
            "reflectance" => Column::Reflectance,
            "skip" => Column::Skip,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Column::Wavelength => "wavelength",
            Column::Transmittance => "transmittance",
            Column::Reflectance => "reflectance",
            Column::Skip => "skip",
        }
    }
}

/// How to read a table.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// The columns, in order (more columns in the file are ignored).
    pub columns: Vec<Column>,
    /// Whether wavelengths are in nm (else µm).
    pub nanometres: bool,
    /// Whether R and T are percentages; `None` decides from the values (any above 1.5).
    pub percent: Option<bool>,
    /// Without a reflectance column: R = 1 − T (true, for low-loss dielectric coatings)
    /// or R = 0 (false, for absorbing filters).
    pub reflectance_is_complement: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            columns: vec![Column::Wavelength, Column::Transmittance],
            nanometres: true,
            percent: None,
            reflectance_is_complement: true,
        }
    }
}

/// A table as read, with what had to be adjusted.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// (wavelength µm, R, T), increasing in wavelength.
    pub points: Vec<(f64, f64, f64)>,
    /// Whether R and T were read as percentages.
    pub percent: bool,
    /// Lines that were not data (headers, notes).
    pub skipped_lines: usize,
    /// Values outside 0..1 (measurement noise), clipped into it.
    pub clipped_values: usize,
    /// Rows whose R + T exceeded 1, where R was lowered to 1 − T.
    pub lowered_reflectance: usize,
    /// Rows dropped because their wavelength repeated an earlier one.
    pub repeated_wavelengths: usize,
}

fn cells(line: &str) -> Vec<&str> {
    let separators: &[char] = if line.contains([',', ';', '\t']) {
        &[',', ';', '\t']
    } else {
        &[' ']
    };
    line.split(separators)
        .map(str::trim)
        .filter(|c| !c.is_empty() || separators != [' '])
        .collect()
}

/// Reads a table of wavelength against R and/or T.
pub fn read_table(text: &str, options: &Options) -> Result<Table, String> {
    let index = |column: Column| options.columns.iter().position(|c| *c == column);
    let wavelength = index(Column::Wavelength).ok_or("the columns need a `wavelength`")?;
    let transmittance = index(Column::Transmittance);
    let reflectance = index(Column::Reflectance);
    if transmittance.is_none() && reflectance.is_none() {
        return Err("the columns need a `transmittance` or a `reflectance`".into());
    }
    let mut rows: Vec<(f64, Option<f64>, Option<f64>)> = Vec::new();
    let mut skipped_lines = 0;
    for line in text.lines() {
        let cells = cells(line);
        let number = |i: Option<usize>| -> Option<Option<f64>> {
            match i {
                None => Some(None),
                Some(i) => cells
                    .get(i)
                    .and_then(|c| c.parse::<f64>().ok())
                    .filter(|v| v.is_finite())
                    .map(Some),
            }
        };
        match (
            number(Some(wavelength)),
            number(transmittance),
            number(reflectance),
        ) {
            (Some(Some(w)), Some(t), Some(r)) if w > 0.0 => rows.push((w, t, r)),
            _ => skipped_lines += 1,
        }
    }
    if rows.len() < 2 {
        return Err("fewer than two rows of numbers in the columns given".into());
    }
    let percent = options.percent.unwrap_or_else(|| {
        rows.iter()
            .flat_map(|&(_, t, r)| [t, r])
            .flatten()
            .any(|v| v > 1.5)
    });
    let scale = if percent { 0.01 } else { 1.0 };
    let unit = if options.nanometres { 1e-3 } else { 1.0 };
    let mut clipped_values = 0;
    let mut clip = |v: f64| {
        let c = v.clamp(0.0, 1.0);
        if !(0.0..=1.0).contains(&v) {
            clipped_values += 1;
        }
        c
    };
    let mut points: Vec<(f64, f64, f64)> = rows
        .into_iter()
        .map(|(w, t, r)| {
            let t = t.map(|t| clip(t * scale));
            let r = r.map(|r| clip(r * scale));
            let (r, t) = match (r, t) {
                (Some(r), Some(t)) => (r, t),
                (None, Some(t)) if options.reflectance_is_complement => (1.0 - t, t),
                (None, Some(t)) => (0.0, t),
                (Some(r), None) => (r, 1.0 - r),
                (None, None) => (0.0, 0.0),
            };
            (w * unit, r, t)
        })
        .collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    let before = points.len();
    points.dedup_by(|b, a| (a.0 - b.0).abs() <= 1e-12 * a.0);
    let repeated_wavelengths = before - points.len();
    let mut lowered_reflectance = 0;
    for p in &mut points {
        if p.1 + p.2 > 1.0 {
            p.1 = 1.0 - p.2;
            lowered_reflectance += 1;
        }
    }
    Ok(Table {
        points,
        percent,
        skipped_lines,
        clipped_values,
        lowered_reflectance,
        repeated_wavelengths,
    })
}

/// Imports a table under `name` into the library under `root`: the curve as
/// `<name>.csv` and what was read as `<name>.json`. A different file under a name already
/// taken needs `replace`. `source` says where it came from (a spec sheet's address).
pub fn import(
    root: &Path,
    path: &Path,
    name: &str,
    options: &Options,
    source: &str,
    redistribute: bool,
    replace: bool,
) -> Result<Value, String> {
    check_name(name).map_err(|e| e.to_string())?;
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let table = read_table(&text, options).map_err(|e| format!("{}: {e}", path.display()))?;
    CoatingCurve::new(table.points.clone()).map_err(|e| format!("{}: {e:?}", path.display()))?;
    let directory = curves_directory(root);
    let csv_path = directory.join(format!("{name}.csv"));
    let json_path = directory.join(format!("{name}.json"));
    let sha256 = sha256_hex(&bytes);
    if let Ok(existing) = fs::read_to_string(&json_path) {
        let same = serde_json::from_str::<Value>(&existing)
            .ok()
            .is_some_and(|v| v["sha256"] == sha256);
        if !same && !replace {
            return Err(format!(
                "a curve `{name}` is already in the library ({}); pass `replace` to overwrite it",
                csv_path.display()
            ));
        }
    }
    fs::create_dir_all(&directory).map_err(|e| format!("{}: {e}", directory.display()))?;
    let mut csv = String::from("wavelength_um,reflectance,transmittance\n");
    for (w, r, t) in &table.points {
        csv.push_str(&format!("{w},{r},{t}\n"));
    }
    fs::write(&csv_path, csv).map_err(|e| format!("{}: {e}", csv_path.display()))?;
    let (first, last) = (table.points[0].0, table.points[table.points.len() - 1].0);
    let record = json!({
        "name": name,
        "source": source,
        "original_path": path.to_string_lossy(),
        "sha256": sha256,
        "columns": options.columns.iter().map(|c| c.name()).collect::<Vec<_>>(),
        "wavelength_unit": if options.nanometres { "nm" } else { "um" },
        "percent": table.percent,
        "reflectance": if options.columns.contains(&Column::Reflectance) {
            "measured"
        } else if options.reflectance_is_complement {
            "1 - transmittance"
        } else {
            "0"
        },
        "rows": table.points.len(),
        "range_um": [first, last],
        "skipped_lines": table.skipped_lines,
        "clipped_values": table.clipped_values,
        "lowered_reflectance": table.lowered_reflectance,
        "repeated_wavelengths": table.repeated_wavelengths,
        "redistribute": redistribute,
    });
    let pretty = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    fs::write(&json_path, pretty).map_err(|e| format!("{}: {e}", json_path.display()))?;
    Ok(record)
}

/// The curve stored under `name`.
pub fn load(root: &Path, name: &str) -> Result<CoatingCurve, String> {
    check_name(name).map_err(|e| e.to_string())?;
    let path = curves_directory(root).join(format!("{name}.csv"));
    let text = fs::read_to_string(&path).map_err(|_| {
        format!("no curve `{name}` in the library (see the `curves` command and `import-curve`)")
    })?;
    let options = Options {
        columns: vec![
            Column::Wavelength,
            Column::Reflectance,
            Column::Transmittance,
        ],
        nanometres: false,
        percent: Some(false),
        reflectance_is_complement: false,
    };
    let table = read_table(&text, &options).map_err(|e| format!("{}: {e}", path.display()))?;
    CoatingCurve::new(table.points).map_err(|e| format!("{}: {e:?}", path.display()))
}

/// Every curve's record, sorted by name.
pub fn list(root: &Path) -> Result<Vec<Value>, String> {
    let Ok(entries) = fs::read_dir(curves_directory(root)) else {
        return Ok(Vec::new());
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
        .iter()
        .map(|f| {
            let text = fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreadsheet_tables_are_read() {
        // Header lines, tab separators, nm and percent, descending order, a value above
        // 100 % from noise.
        let text = "Bandpass filter\nWavelength (nm)\tTransmission (%)\n\
                    700\t1.0\n600\t50.0\n500\t100.4\n";
        let table = read_table(text, &Options::default()).expect("reads");
        assert!(table.percent);
        assert_eq!(table.skipped_lines, 2);
        assert_eq!(table.clipped_values, 1);
        assert_eq!(table.points.len(), 3);
        let (w, r, t) = table.points[0];
        assert!((w - 0.5).abs() < 1e-15 && (t - 1.0).abs() < 1e-15 && r.abs() < 1e-15);
        let (w, r, t) = table.points[1];
        assert!((w - 0.6).abs() < 1e-15 && (t - 0.5).abs() < 1e-15 && (r - 0.5).abs() < 1e-15);
        // Comma separated, µm, fractions, with a measured reflectance column first.
        let text = "um,R,T\n0.4,0.9,0.05\n0.8,0.05,0.97\n";
        let options = Options {
            columns: vec![
                Column::Wavelength,
                Column::Reflectance,
                Column::Transmittance,
            ],
            nanometres: false,
            ..Options::default()
        };
        let table = read_table(text, &options).expect("reads");
        assert!(!table.percent);
        // 0.05 + 0.97 > 1: R lowered to 1 − T.
        assert_eq!(table.lowered_reflectance, 1);
        assert!((table.points[1].1 - 0.03).abs() < 1e-12);
        // An absorbing filter: R = 0.
        let options = Options {
            reflectance_is_complement: false,
            ..Options::default()
        };
        let table = read_table("500 0.2\n600 0.8\n", &options).expect("spaces");
        assert_eq!(table.points[0].1, 0.0);
        assert!(read_table("500,0.5\n", &Options::default()).is_err());
    }

    #[test]
    fn import_and_load_round_trip() {
        let root = std::env::temp_dir().join(format!("optobench-curves-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch");
        let file = root.join("filter.csv");
        fs::write(&file, "nm,T\n400,10\n500,90\n").expect("written");
        let record = import(
            &root,
            &file,
            "test-filter",
            &Options::default(),
            "test",
            false,
            false,
        )
        .expect("imported");
        assert_eq!(record["reflectance"], "1 - transmittance");
        assert_eq!(record["redistribute"], false);
        let curve = load(&root, "test-filter").expect("loads");
        let response = optobench_trace::Coating::Curve(std::sync::Arc::new(curve))
            .response(&optobench_trace::coating::Incidence {
                wavelength: 0.45,
                cos_incidence: 1.0,
                index_from: 1.0,
                index_to: 1.5,
                from_outside: true,
            })
            .expect("in range");
        assert!((response.transmittance - 0.5).abs() < 1e-12);
        // A different file under the same name needs `replace`.
        fs::write(&file, "nm,T\n400,20\n500,90\n").expect("written");
        assert!(
            import(
                &root,
                &file,
                "test-filter",
                &Options::default(),
                "test",
                false,
                false
            )
            .is_err()
        );
        import(
            &root,
            &file,
            "test-filter",
            &Options::default(),
            "test",
            false,
            true,
        )
        .expect("replaced");
        assert_eq!(list(&root).expect("lists").len(), 1);
        assert!(load(&root, "other").is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
