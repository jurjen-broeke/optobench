//! Measured R/T curves in the user's parts library: import of vendor tables (CSV as a
//! spreadsheet exports it) and lookup by name (docs/design.md, "Parts library").
//!
//! ```text
//! <data directory>/optobench/library/curves/<name>.csv    wavelength_um,reflectance,transmittance
//! <data directory>/optobench/library/curves/<name>.json   where it came from and how it was read
//! ```
//!
//! Tables are delimited text (CSV, tab- or space-separated) or spreadsheets (`.xlsx`, as
//! vendors publish them; see `xlsx`). Columns and units come from a header row when one
//! names the wavelength. A curve is used as `coating = { curve = "<name>" }` in part,
//! layout and sidecar files.
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

/// How to read a table. Left as `None`, the columns and units come from a header row
/// such as `Wavelength (nm) | Optical Density | % Transmission`, as vendors write them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Options {
    /// The columns, in order (more columns in the file are ignored); `None`: from the
    /// header, else wavelength then transmittance.
    pub columns: Option<Vec<Column>>,
    /// Whether wavelengths are in nm (else µm); `None`: from the header, else nm.
    pub nanometres: Option<bool>,
    /// Whether R and T are percentages; `None`: from the header (`%`), else from the
    /// values (any above 1.5).
    pub percent: Option<bool>,
    /// Without a reflectance column, R = 0 (for absorbing filters) instead of R = 1 − T.
    pub reflectance_zero: bool,
    /// The sheet of a spreadsheet; `None`: the first.
    pub sheet: Option<String>,
}

/// A table as read, with what had to be adjusted.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// (wavelength µm, R, T), increasing in wavelength.
    pub points: Vec<(f64, f64, f64)>,
    /// The columns as used.
    pub columns: Vec<Column>,
    /// Whether wavelengths were read as nm.
    pub nanometres: bool,
    /// Whether R and T were read as percentages.
    pub percent: bool,
    /// The header row the columns came from, if they did.
    pub header: Option<Vec<String>>,
    /// The spreadsheet's sheet, for spreadsheets.
    pub sheet: Option<String>,
    /// Other text in the table (a vendor's description and disclaimer), up to 20 cells.
    pub notes: Vec<String>,
    /// Lines that were not data (headers, notes).
    pub skipped_lines: usize,
    /// Values outside 0..1 (measurement noise), clipped into it.
    pub clipped_values: usize,
    /// Rows whose R + T exceeded 1, where R was lowered to 1 − T.
    pub lowered_reflectance: usize,
    /// Rows dropped because their wavelength repeated an earlier one.
    pub repeated_wavelengths: usize,
}

/// The cells of one line of delimited text: split on commas, semicolons or tabs (outside
/// double quotes), else on spaces.
fn cells(line: &str) -> Vec<String> {
    if !line.contains([',', ';', '\t']) {
        return line.split_whitespace().map(str::to_owned).collect();
    }
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' | ';' | '\t' if !quoted => out.push(core::mem::take(&mut cell).trim().to_owned()),
            c => cell.push(c),
        }
    }
    out.push(cell.trim().to_owned());
    out
}

/// Columns and units from a header row naming a wavelength.
fn header_columns(row: &[String]) -> Option<(Vec<Column>, Option<bool>, Option<bool>)> {
    let lower: Vec<String> = row.iter().map(|c| c.to_lowercase()).collect();
    let wavelength = lower.iter().position(|c| c.contains("wavelength"))?;
    let columns: Vec<Column> = lower
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i == wavelength {
                Column::Wavelength
            } else if c.contains("transmi") {
                Column::Transmittance
            } else if c.contains("reflect") {
                Column::Reflectance
            } else {
                Column::Skip
            }
        })
        .collect();
    if !columns
        .iter()
        .any(|c| matches!(c, Column::Transmittance | Column::Reflectance))
    {
        return None;
    }
    let w = &lower[wavelength];
    let nanometres = if w.contains("nm") {
        Some(true)
    } else if w.contains("µm") || w.contains("um)") || w.contains("micron") {
        Some(false)
    } else {
        None
    };
    let percent = columns
        .iter()
        .zip(&lower)
        .filter(|(c, _)| matches!(c, Column::Transmittance | Column::Reflectance))
        .any(|(_, text)| text.contains('%'))
        .then_some(true);
    Some((columns, nanometres, percent))
}

/// Reads a table of wavelength against R and/or T from delimited text.
pub fn read_table(text: &str, options: &Options) -> Result<Table, String> {
    let rows: Vec<Vec<String>> = text.lines().map(cells).collect();
    read_rows(&rows, options)
}

/// Reads a table from a file's contents: a spreadsheet (`.xlsx`, a ZIP archive) or
/// delimited text.
pub fn read_file(bytes: &[u8], options: &Options) -> Result<Table, String> {
    if bytes.starts_with(b"PK\x03\x04") {
        let (sheet, rows) = crate::xlsx::read_sheet(bytes, options.sheet.as_deref())?;
        let mut table = read_rows(&rows, options)?;
        table.sheet = Some(sheet);
        Ok(table)
    } else {
        read_table(&String::from_utf8_lossy(bytes), options)
    }
}

/// Reads a table from rows of cells.
pub fn read_rows(rows: &[Vec<String>], options: &Options) -> Result<Table, String> {
    let found_header = if options.columns.is_none() {
        rows.iter()
            .take(100)
            .enumerate()
            .find_map(|(i, r)| header_columns(r).map(|h| (i, h)))
    } else {
        None
    };
    let columns = match (&options.columns, &found_header) {
        (Some(c), _) => c.clone(),
        (None, Some((_, (c, _, _)))) => c.clone(),
        (None, None) => vec![Column::Wavelength, Column::Transmittance],
    };
    let header_row = found_header.as_ref().map(|(i, _)| *i);
    let nanometres = options
        .nanometres
        .or(found_header.as_ref().and_then(|(_, (_, n, _))| *n))
        .unwrap_or(true);
    let index = |column: Column| columns.iter().position(|c| *c == column);
    let wavelength = index(Column::Wavelength).ok_or("the columns need a `wavelength`")?;
    let transmittance = index(Column::Transmittance);
    let reflectance = index(Column::Reflectance);
    if transmittance.is_none() && reflectance.is_none() {
        return Err("the columns need a `transmittance` or a `reflectance`".into());
    }
    let parse = |c: &str| c.trim().parse::<f64>().ok().filter(|v| v.is_finite());
    let mut data: Vec<(f64, Option<f64>, Option<f64>)> = Vec::new();
    let mut skipped_lines = 0;
    let mut notes: Vec<String> = Vec::new();
    for (row, cells) in rows.iter().enumerate() {
        let number = |i: Option<usize>| -> Option<Option<f64>> {
            match i {
                None => Some(None),
                Some(i) => cells.get(i).and_then(|c| parse(c)).map(Some),
            }
        };
        match (
            number(Some(wavelength)),
            number(transmittance),
            number(reflectance),
        ) {
            (Some(Some(w)), Some(t), Some(r)) if w > 0.0 => data.push((w, t, r)),
            _ => skipped_lines += 1,
        }
        if Some(row) != header_row {
            for c in cells {
                let c = c.trim();
                if !c.is_empty()
                    && parse(c).is_none()
                    && notes.len() < 20
                    && !notes.iter().any(|n| n == c)
                {
                    notes.push(c.to_owned());
                }
            }
        }
    }
    if data.len() < 2 {
        return Err("fewer than two rows of numbers in the columns given".into());
    }
    let percent = options
        .percent
        .or(found_header.as_ref().and_then(|(_, (_, _, p))| *p))
        .unwrap_or_else(|| {
            data.iter()
                .flat_map(|&(_, t, r)| [t, r])
                .flatten()
                .any(|v| v > 1.5)
        });
    let scale = if percent { 0.01 } else { 1.0 };
    let unit = if nanometres { 1e-3 } else { 1.0 };
    let mut clipped_values = 0;
    let mut clip = |v: f64| {
        if !(0.0..=1.0).contains(&v) {
            clipped_values += 1;
        }
        v.clamp(0.0, 1.0)
    };
    let mut points: Vec<(f64, f64, f64)> = data
        .into_iter()
        .map(|(w, t, r)| {
            let t = t.map(|t| clip(t * scale));
            let r = r.map(|r| clip(r * scale));
            let (r, t) = match (r, t) {
                (Some(r), Some(t)) => (r, t),
                (None, Some(t)) if options.reflectance_zero => (0.0, t),
                (None, Some(t)) => (1.0 - t, t),
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
        header: header_row.map(|i| rows[i].clone()),
        columns,
        nanometres,
        percent,
        sheet: None,
        notes,
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
    let table = read_file(&bytes, options).map_err(|e| format!("{}: {e}", path.display()))?;
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
        "sheet": table.sheet,
        "header": table.header,
        "columns": table.columns.iter().map(|c| c.name()).collect::<Vec<_>>(),
        "wavelength_unit": if table.nanometres { "nm" } else { "um" },
        "percent": table.percent,
        "reflectance": if table.columns.contains(&Column::Reflectance) {
            "measured"
        } else if options.reflectance_zero {
            "0"
        } else {
            "1 - transmittance"
        },
        "notes": table.notes,
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
        columns: Some(vec![
            Column::Wavelength,
            Column::Reflectance,
            Column::Transmittance,
        ]),
        nanometres: Some(false),
        percent: Some(false),
        ..Options::default()
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
            columns: Some(vec![
                Column::Wavelength,
                Column::Reflectance,
                Column::Transmittance,
            ]),
            nanometres: Some(false),
            ..Options::default()
        };
        let table = read_table(text, &options).expect("reads");
        assert!(!table.percent);
        // 0.05 + 0.97 > 1: R lowered to 1 − T.
        assert_eq!(table.lowered_reflectance, 1);
        assert!((table.points[1].1 - 0.03).abs() < 1e-12);
        // An absorbing filter: R = 0.
        let options = Options {
            reflectance_zero: true,
            ..Options::default()
        };
        let table = read_table("500 0.2\n600 0.8\n", &options).expect("spaces");
        assert_eq!(table.points[0].1, 0.0);
        assert!(read_table("500,0.5\n", &Options::default()).is_err());
    }

    /// A ZIP archive of stored (uncompressed) entries, laid out as APPNOTE.TXT describes.
    fn stored_zip(entries: &[(&str, String)]) -> Vec<u8> {
        let (mut out, mut directory) = (Vec::new(), Vec::new());
        for (name, text) in entries {
            let (data, crc) = (text.as_bytes(), crate::zip::crc32(text.as_bytes()));
            let offset = out.len() as u32;
            let sizes = [crc, data.len() as u32, data.len() as u32];
            out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            sizes
                .iter()
                .for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            directory.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
            directory.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            sizes
                .iter()
                .for_each(|v| directory.extend_from_slice(&v.to_le_bytes()));
            directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
            directory.extend_from_slice(&[0; 12]);
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
        }
        let start = out.len() as u32;
        out.extend_from_slice(&directory);
        out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        let count = (entries.len() as u16).to_le_bytes();
        out.extend_from_slice(&count);
        out.extend_from_slice(&count);
        out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn vendor_spreadsheets_are_read_from_their_header() {
        // The layout vendors use: notes in columns A and B, a header row naming the
        // columns, wavelength (nm) descending, optical density, % transmission; text in
        // the shared string table.
        let strings = [
            "Wavelength (nm)",
            "Optical Density",
            "% Transmission",
            "Item #",
            "F1 &amp; co",
        ];
        let shared = format!(
            "<?xml version=\"1.0\"?><sst xmlns=\"x\">{}</sst>",
            strings
                .iter()
                .map(|s| format!("<si><t>{s}</t></si>"))
                .collect::<String>()
        );
        let row = |r: usize, w: f64, t: f64| {
            format!(
                "<row r=\"{r}\"><c r=\"C{r}\"><v>{w}</v></c><c r=\"D{r}\"><v>1</v></c><c r=\"E{r}\"><v>{t}</v></c></row>"
            )
        };
        let sheet = format!(
            "<worksheet><sheetData><row r=\"1\"><c r=\"C1\" t=\"s\"><v>0</v></c><c r=\"D1\" t=\"s\"><v>1</v></c><c r=\"E1\" t=\"s\"><v>2</v></c></row>\
             <row r=\"2\"><c r=\"A2\" t=\"s\"><v>3</v></c><c r=\"B2\" t=\"s\"><v>4</v></c></row>{}{}{}</sheetData></worksheet>",
            row(3, 600.0, 2.0),
            row(4, 550.0, 90.0),
            row(5, 500.0, 2.0)
        );
        let workbook = "<workbook xmlns:r=\"r\"><sheets><sheet name=\"Data\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>".to_owned();
        let relations = "<Relationships><Relationship Id=\"rId1\" Target=\"worksheets/sheet1.xml\"/></Relationships>".to_owned();
        let file = stored_zip(&[
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", relations),
            ("xl/sharedStrings.xml", shared),
            ("xl/worksheets/sheet1.xml", sheet),
        ]);
        let table = read_file(&file, &Options::default()).expect("reads");
        assert_eq!(table.sheet.as_deref(), Some("Data"));
        assert_eq!(
            table.columns,
            vec![
                Column::Skip,
                Column::Skip,
                Column::Wavelength,
                Column::Skip,
                Column::Transmittance
            ]
        );
        assert!(table.nanometres && table.percent);
        assert_eq!(table.notes, vec!["Item #".to_owned(), "F1 & co".to_owned()]);
        assert_eq!(table.points.len(), 3);
        let (w, r, t) = table.points[1];
        assert!((w - 0.55).abs() < 1e-15 && (t - 0.9).abs() < 1e-15 && (r - 0.1).abs() < 1e-15);
        let error = read_file(
            &file,
            &Options {
                sheet: Some("Other".into()),
                ..Options::default()
            },
        )
        .expect_err("no such sheet");
        assert!(error.contains("`Data`"), "{error}");
        // Delimited text with the same header, the description quoted with commas in it.
        let csv = "\"Filter, mounted\"\nWavelength (µm),% Transmission\n0.5,2\n0.55,90\n";
        let table = read_table(csv, &Options::default()).expect("reads");
        assert!(!table.nanometres && table.percent);
        assert_eq!(table.notes, vec!["Filter, mounted".to_owned()]);
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
