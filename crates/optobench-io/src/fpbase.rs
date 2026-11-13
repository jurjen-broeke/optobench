//! Fluorescent proteins from FPbase (fpbase.org; T. J. Lambert, Nat. Methods 16, 277
//! (2019)), imported from JSON files the user downloaded from its documented REST API
//! (docs/licence-review.md, FPbase). Never bundled, never fetched by the engine.
//!
//! Two responses are read, written from the API documentation (<https://www.fpbase.org/api/>)
//! and the API's own output, not from FPbase's source code:
//!
//! ```text
//! /api/proteins/spectra/?name__iexact=<name>&format=json      (required)
//! [ { "name": "…", "slug": "…",
//!     "spectra": [ { "state": "default_ex", "ec": 55900, "max": 488, "data": [[300, 0.09], …] },
//!                  { "state": "default_em", "qy": 0.6, "max": 507, "data": [[463, 0.003], …] } ] } ]
//!
//! /api/proteins/?name__iexact=<name>&format=json              (optional: lifetime, reference)
//! [ { "name": "…", "slug": "…", "doi": "…",
//!     "states": [ { "slug": "…_default", "name": "default", "ext_coeff": 55900, "qy": 0.6,
//!                   "lifetime": 2.6, … } ] } ]
//! ```
//!
//! Spectra are `[wavelength nm, value]`, normalised to a peak of 1 (observed); a state's
//! spectra are named `<state>_ex` and `<state>_em`. The API "is still a work in
//! development and is subject to change", so every field used is checked.
//!
//! **Two-photon excitation** is not in the API: for EGFP and tdTomato its responses hold
//! only the one-photon spectra and no two-photon field (checked 2026-11-13), while FPbase
//! has two-photon excitation spectra for some proteins (avGFP, EGFP, tdTomato) on its
//! website. Such a spectrum, exported by the user as a table, is read separately
//! ([`read_two_photon`]): delimited text with a header (the column named like `2P`, or
//! given) or JSON `[[nm, value], …]`. It is the excitation only; emission after two-photon
//! excitation follows the one-photon emission spectrum (as C. Xu, W. Zipfel, J. B. Shear,
//! R. M. Williams and W. W. Webb, Proc. Natl. Acad. Sci. USA 93, 10763 (1996), found). Its
//! values are relative unless the user says they are in GM; a relative spectrum needs a
//! peak cross-section with its source before the entry uses it.
//!
//! The import ([`import`]) stores the files unchanged with their provenance, like imported
//! materials, and gives a `[[fluorophore]]` entry for a design ([`Protein::entry`]) whose
//! `source` carries FPbase's citation and the protein's own reference, as FPbase asks.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::sha256::sha256_hex;

/// FPbase's terms, quoted (https://www.fpbase.org/terms/ and /cite/, read 2026-11-13), shown
/// before anything is imported.
pub const NOTICE: &[&str] = &[
    "FPbase terms (https://www.fpbase.org/terms/): \"The data contained in the FPbase are free of all copyright restrictions and made fully and freely available for both non-commercial and commercial use.\"",
    "\"Users of the data should attribute the original authors of the corresponding data (referenced on the corresponding protein page).\"",
    "\"We cannot provide unrestricted permission regarding the use of the data, as some data may be covered by patents or other rights.\"",
    "The site itself is under CC BY-SA 4.0; whether that covers the data is not stated (docs/licence-review.md).",
    "Cite FPbase as: T. J. Lambert, \"FPbase: a community-editable fluorescent protein database\", Nat. Methods 16, 277-278 (2019), doi:10.1038/s41592-019-0352-8.",
];

/// FPbase's citation, carried in every imported entry's `source`.
pub const CITATION: &str =
    "FPbase (T. J. Lambert, Nat. Methods 16, 277 (2019), doi:10.1038/s41592-019-0352-8)";

/// One state of a protein, as read.
#[derive(Debug, Clone, PartialEq)]
pub struct Protein {
    /// Its name.
    pub name: String,
    /// Its slug (its page: https://www.fpbase.org/protein/<slug>/).
    pub slug: String,
    /// The state.
    pub state: String,
    /// Excitation spectrum, (µm, relative).
    pub excitation: Option<Vec<(f64, f64)>>,
    /// Emission spectrum, (µm, relative).
    pub emission: Option<Vec<(f64, f64)>>,
    /// Two-photon excitation, from a separate file.
    pub two_photon: Option<TwoPhoton>,
    /// Extinction coefficient at the excitation peak (L mol⁻¹ cm⁻¹).
    pub extinction: Option<f64>,
    /// Quantum yield.
    pub quantum_yield: Option<f64>,
    /// Lifetime (ns).
    pub lifetime: Option<f64>,
    /// The primary reference's DOI.
    pub doi: Option<String>,
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|v| v.is_finite())
}

/// The entry for protein `name` (name or slug, any case) in a list response, or the only one.
fn pick<'a>(list: &'a Value, name: Option<&str>, what: &str) -> Result<&'a Value, String> {
    let items: Vec<&Value> = match list {
        Value::Array(items) => items.iter().collect(),
        Value::Object(map) if map.contains_key("results") => map["results"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default(),
        Value::Object(_) => vec![list],
        _ => return Err(format!("the {what} response is not a JSON list or object")),
    };
    let label = |v: &Value| {
        format!(
            "{} ({})",
            v["name"].as_str().unwrap_or("?"),
            v["slug"].as_str().unwrap_or("?")
        )
    };
    match name {
        Some(name) => items
            .into_iter()
            .find(|v| {
                ["name", "slug"]
                    .iter()
                    .any(|k| v[*k].as_str().is_some_and(|s| s.eq_ignore_ascii_case(name)))
            })
            .ok_or_else(|| format!("no protein `{name}` in the {what} response")),
        None => match items.as_slice() {
            [one] => Ok(one),
            [] => Err(format!("the {what} response holds no protein")),
            many => Err(format!(
                "the {what} response holds {} proteins ({}); name one",
                many.len(),
                many.iter().map(|v| label(v)).collect::<Vec<_>>().join(", ")
            )),
        },
    }
}

/// A spectrum's `data`: rows of [nm, value] to (µm, value), increasing, values not negative.
fn spectrum(entry: &Value, label: &str) -> Result<Vec<(f64, f64)>, String> {
    let rows = entry["data"]
        .as_array()
        .ok_or_else(|| format!("spectrum `{label}` has no `data` list"))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let pair = row
            .as_array()
            .filter(|r| r.len() == 2)
            .and_then(|r| Some((number(&r[0])?, number(&r[1])?)))
            .ok_or_else(|| format!("spectrum `{label}`: each row is [wavelength nm, value]"))?;
        if pair.0 <= 0.0 || pair.1 < 0.0 {
            return Err(format!(
                "spectrum `{label}`: a wavelength not positive or a value below 0"
            ));
        }
        if out
            .last()
            .is_some_and(|l: &(f64, f64)| pair.0 * 1e-3 <= l.0)
        {
            return Err(format!(
                "spectrum `{label}`: wavelengths not increasing at {} nm",
                pair.0
            ));
        }
        out.push((pair.0 * 1e-3, pair.1));
    }
    if out.len() < 2 {
        return Err(format!("spectrum `{label}` has fewer than two rows"));
    }
    Ok(out)
}

/// Reads a protein's state from the spectra response and, optionally, the proteins
/// response. `name` picks the protein when a response holds several; `state` is the state's
/// name (`default` for most proteins).
pub fn read(
    spectra: &[u8],
    protein: Option<&[u8]>,
    name: Option<&str>,
    state: &str,
) -> Result<Protein, String> {
    let parse = |bytes: &[u8], what: &str| -> Result<Value, String> {
        serde_json::from_slice(bytes).map_err(|e| format!("the {what} response is not JSON: {e}"))
    };
    let spectra = parse(spectra, "spectra")?;
    let entry = pick(&spectra, name, "spectra")?;
    let protein_name = entry["name"]
        .as_str()
        .ok_or("the spectra response has no protein `name`")?
        .to_owned();
    let slug = entry["slug"].as_str().unwrap_or("").to_owned();
    let list = entry["spectra"]
        .as_array()
        .ok_or("the spectra response has no `spectra` list")?;
    let named = |suffix: &str| {
        let key = format!("{state}_{suffix}");
        list.iter()
            .find(|s| s["state"].as_str() == Some(key.as_str()))
    };
    let states: Vec<&str> = list
        .iter()
        .filter_map(|s| s["state"].as_str())
        .filter_map(|s| s.rsplit_once('_').map(|p| p.0))
        .collect();
    let (ex, em) = (named("ex"), named("em"));
    if ex.is_none() && em.is_none() {
        let mut known = states.clone();
        known.sort_unstable();
        known.dedup();
        return Err(format!(
            "`{protein_name}` has no spectra for state `{state}`; its states: {}",
            known.join(", ")
        ));
    }
    let mut read = Protein {
        name: protein_name.clone(),
        slug,
        state: state.to_owned(),
        excitation: ex
            .map(|s| spectrum(s, &format!("{state}_ex")))
            .transpose()?,
        emission: em
            .map(|s| spectrum(s, &format!("{state}_em")))
            .transpose()?,
        two_photon: None,
        extinction: ex.and_then(|s| number(&s["ec"])),
        quantum_yield: em.and_then(|s| number(&s["qy"])),
        lifetime: None,
        doi: None,
    };
    if let Some(bytes) = protein {
        let proteins = parse(bytes, "proteins")?;
        let entry = pick(&proteins, Some(name.unwrap_or(&protein_name)), "proteins")?;
        read.doi = entry["doi"]
            .as_str()
            .filter(|d| !d.trim().is_empty())
            .map(str::to_owned);
        let found = entry["states"].as_array().and_then(|states| {
            states.iter().find(|s| {
                s["name"].as_str() == Some(state)
                    || s["slug"]
                        .as_str()
                        .is_some_and(|slug| slug.ends_with(&format!("_{state}")))
            })
        });
        match found {
            Some(s) => {
                read.lifetime = number(&s["lifetime"]);
                read.extinction = read.extinction.or_else(|| number(&s["ext_coeff"]));
                read.quantum_yield = read.quantum_yield.or_else(|| number(&s["qy"]));
            }
            None => {
                return Err(format!(
                    "the proteins response has no state `{state}` for `{protein_name}`"
                ));
            }
        }
    }
    if let Some(q) = read.quantum_yield {
        if !(0.0..=1.0).contains(&q) {
            return Err(format!(
                "`{protein_name}`: quantum yield {q} outside 0 to 1"
            ));
        }
    }
    if read.extinction.is_some_and(|e| e <= 0.0) {
        read.extinction = None;
    }
    Ok(read)
}

/// A two-photon excitation spectrum, from its own file.
#[derive(Debug, Clone, PartialEq)]
pub struct TwoPhoton {
    /// (µm, value) as in the file: relative, or in GM.
    pub spectrum: Vec<(f64, f64)>,
    /// What the values are.
    pub values: TwoPhotonValues,
    /// For a relative spectrum, the cross-section that scales it (GM); `None` until given.
    pub peak: Option<Peak>,
    /// Whether the cross-sections (the file's in GM, or `peak`) are action cross-sections.
    pub action: bool,
    /// The file's name and column.
    pub file: String,
    /// Its SHA-256.
    pub sha256: String,
}

/// A published two-photon cross-section that scales a relative spectrum.
#[derive(Debug, Clone, PartialEq)]
pub struct Peak {
    /// GM.
    pub value: f64,
    /// The wavelength it was measured at (µm); `None`: at the spectrum's greatest value.
    /// A relative spectrum need not be 1 there (tdTomato's FPbase spectrum exceeds 1 below
    /// 700 nm), so a value quoted at a wavelength is applied at that wavelength.
    pub wavelength: Option<f64>,
    /// Where it comes from.
    pub source: String,
}

/// What a two-photon file's values are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoPhotonValues {
    /// Relative (any scale): a peak must be given.
    Relative,
    /// Absorption cross-sections σ₂ in GM.
    Absorption,
    /// Action cross-sections η σ₂ in GM.
    Action,
}

/// A table of a two-photon spectrum: delimited text (comma, tab, semicolon or spaces)
/// with a header row, the column named `column` or else the one whose name holds `2p`
/// (any case; among several, the one that also names `protein`, as FPbase's spectra
/// viewer exports `<protein> 2P` columns side by side), or the only value column; or JSON, a list of `[wavelength, value]` or an
/// object with such a `data` list. Wavelengths above 100 are taken as nm. Rows without a
/// value in the column are skipped; small negative values (noise) are set to 0. Returns
/// (µm, value) rows and the column's name.
pub fn read_two_photon(
    bytes: &[u8],
    column: Option<&str>,
    protein: Option<&str>,
) -> Result<(Vec<(f64, f64)>, String), String> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}');
    let trimmed = text.trim_start();
    let mut rows: Vec<(f64, f64)> = Vec::new();
    let label;
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        let value: Value =
            serde_json::from_str(trimmed).map_err(|e| format!("not valid JSON: {e}"))?;
        let data = if value.is_object() {
            &value["data"]
        } else {
            &value
        };
        for row in data
            .as_array()
            .ok_or("the JSON holds no list of [wavelength, value]")?
        {
            let pair = row
                .as_array()
                .filter(|r| r.len() == 2)
                .and_then(|r| Some((number(&r[0])?, number(&r[1])?)))
                .ok_or("each JSON row is [wavelength, value]")?;
            rows.push(pair);
        }
        label = "data".to_owned();
    } else {
        let split = |line: &str| -> Vec<String> {
            let separator = [',', '\t', ';'].into_iter().find(|c| line.contains(*c));
            match separator {
                Some(c) => line
                    .split(c)
                    .map(|f| f.trim().trim_matches('"').to_owned())
                    .collect(),
                None => line.split_whitespace().map(str::to_owned).collect(),
            }
        };
        let mut lines = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'));
        let header = lines.next().ok_or("the file is empty")?;
        let names = split(header);
        if names.iter().all(|n| n.parse::<f64>().is_ok()) {
            return Err("the table needs a header row naming its columns".into());
        }
        let index = match column {
            Some(wanted) => names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(wanted))
                .or_else(|| {
                    names
                        .iter()
                        .position(|n| n.to_lowercase().contains(&wanted.to_lowercase()))
                }),
            None => {
                let two: Vec<usize> = (1..names.len())
                    .filter(|&i| names[i].to_lowercase().contains("2p"))
                    .collect();
                let named: Vec<usize> = match protein {
                    Some(p) => two
                        .iter()
                        .copied()
                        .filter(|&i| {
                            let n = names[i].to_lowercase();
                            let p = p.to_lowercase();
                            n.split_whitespace().any(|w| w == p) || n.starts_with(&format!("{p} "))
                        })
                        .collect(),
                    None => Vec::new(),
                };
                match (two.as_slice(), named.as_slice()) {
                    (_, [one]) => Some(*one),
                    ([one], _) => Some(*one),
                    ([], _) => (names.len() == 2).then_some(1),
                    _ => None,
                }
            }
        }
        .filter(|&i| i > 0)
        .ok_or_else(|| {
            format!(
                "which column is the two-photon spectrum? The columns: {}",
                names.join(", ")
            )
        })?;
        label = names[index].clone();
        for line in lines {
            let fields = split(line);
            let (Some(w), Some(v)) = (fields.first(), fields.get(index)) else {
                continue;
            };
            let (Ok(w), Ok(v)) = (w.parse::<f64>(), v.parse::<f64>()) else {
                continue;
            };
            if w.is_finite() && v.is_finite() {
                rows.push((w, v));
            }
        }
    }
    let top = rows.iter().map(|r| r.1).fold(0.0, f64::max);
    if rows.len() < 2 || top <= 0.0 {
        return Err(format!(
            "column `{label}` holds fewer than two positive rows"
        ));
    }
    let mut out = Vec::with_capacity(rows.len());
    for (w, v) in rows {
        if v < -0.05 * top {
            return Err(format!("column `{label}`: {v} at {w} is well below 0"));
        }
        let w = if w > 100.0 { w / 1000.0 } else { w };
        if out.last().is_some_and(|l: &(f64, f64)| w <= l.0) {
            return Err(format!(
                "column `{label}`: wavelengths not increasing at {w} µm"
            ));
        }
        out.push((w, v.max(0.0)));
    }
    Ok((out, label))
}

impl TwoPhoton {
    /// From a table read by [`read_two_photon`]: values in GM as they are; relative values
    /// with `peak` to scale them (or none yet), `action` saying whether the cross-sections
    /// are action cross-sections η σ₂.
    pub fn new(
        rows: Vec<(f64, f64)>,
        values: TwoPhotonValues,
        peak: Option<Peak>,
        action: bool,
        file: &str,
        sha256: &str,
    ) -> Result<Self, String> {
        if let Some(p) = &peak {
            if values != TwoPhotonValues::Relative {
                return Err("the file's values are in GM already; give no separate peak".into());
            }
            if !(p.value > 0.0 && p.value.is_finite()) {
                return Err("the peak cross-section must be positive".into());
            }
            if p.source.trim().is_empty() {
                return Err("say where the peak cross-section comes from".into());
            }
            if let Some(w) = p.wavelength {
                let at = optobench_materials::Spectrum::Table(rows.clone())
                    .at(w)
                    .map_err(|e| format!("the peak's wavelength: {e}"))?;
                if at <= 0.0 {
                    return Err(format!(
                        "the spectrum is zero at the peak's wavelength {w} µm"
                    ));
                }
            }
        }
        Ok(Self {
            spectrum: rows,
            values,
            peak,
            action: action || values == TwoPhotonValues::Action,
            file: file.to_owned(),
            sha256: sha256.to_owned(),
        })
    }
}

fn table(rows: &[(f64, f64)]) -> String {
    let cells: Vec<String> = rows.iter().map(|(w, v)| format!("[{w}, {v}]")).collect();
    let mut out = String::from("[\n");
    for chunk in cells.chunks(6) {
        out.push_str("    ");
        out.push_str(&chunk.join(", "));
        out.push_str(",\n");
    }
    out.push(']');
    out
}

impl Protein {
    /// Where the values come from, for the entry's `source`.
    pub fn source(&self, sha256: &str) -> String {
        let reference = match &self.doi {
            Some(doi) => format!("original data: doi:{doi}"),
            None => "original data: see its FPbase page".to_owned(),
        };
        let mut text = format!(
            "{CITATION}, protein {} (https://www.fpbase.org/protein/{}/), state {}; {reference}; imported file sha256 {}",
            self.name,
            self.slug,
            self.state,
            &sha256[..16.min(sha256.len())]
        );
        if let Some(two) = &self.two_photon {
            text.push_str(&format!(
                "; two-photon excitation spectrum from {} (sha256 {})",
                two.file,
                &two.sha256[..16.min(two.sha256.len())]
            ));
            let kind = if two.action { "action " } else { "" };
            match (&two.peak, two.values) {
                (Some(p), _) => {
                    let at = p
                        .wavelength
                        .map_or("at its greatest value".to_owned(), |w| {
                            format!("at {} nm", w * 1e3)
                        });
                    text.push_str(&format!(
                        ", scaled by the {kind}cross-section {} GM {at} from {}",
                        p.value, p.source
                    ));
                }
                (None, TwoPhotonValues::Relative) => {}
                (None, _) => text.push_str(&format!(", values in GM ({kind}cross-sections)")),
            }
        }
        text
    }

    /// A `[[fluorophore]]` entry for a design (layout or sidecar) with id `id`. Without a
    /// quantum yield it cannot be read until one is given; concentrations are left for
    /// the user; a two-photon spectrum is written commented out, as it needs a published
    /// peak cross-section.
    pub fn entry(&self, id: &str, sha256: &str) -> String {
        let mut out = format!(
            "[[fluorophore]]\nid = \"{id}\"\nsource = \"{}\"\n",
            self.source(sha256).replace('"', "'")
        );
        match self.quantum_yield {
            Some(q) => out.push_str(&format!("quantum_yield = {q}\n")),
            None => out.push_str("# quantum_yield = …      # FPbase gives none for this state\n"),
        }
        if let Some(t) = self.lifetime {
            out.push_str(&format!("lifetime_ns = {t}\n"));
        }
        match (&self.excitation, self.extinction) {
            (Some(ex), Some(e)) => {
                out.push_str(&format!("extinction_coefficient = {e}\n"));
                out.push_str(&format!("excitation = {}\n", table(ex)));
            }
            (Some(ex), None) => {
                out.push_str("# extinction_coefficient = …   # FPbase gives none for this state\n");
                out.push_str("# excitation (relative) is kept here for reference:\n");
                for line in format!("excitation = {}", table(ex)).lines() {
                    out.push_str(&format!("# {line}\n"));
                }
            }
            _ => {}
        }
        if let Some(em) = &self.emission {
            out.push_str(&format!("emission = {}\n", table(em)));
        }
        if let Some(two) = &self.two_photon {
            let key = if two.action {
                "two_photon_action_cross_section_gm"
            } else {
                "two_photon_cross_section_gm"
            };
            match (&two.peak, two.values) {
                (_, TwoPhotonValues::Absorption | TwoPhotonValues::Action) => {
                    out.push_str(&format!("{key} = {}\n", table(&two.spectrum)));
                }
                (Some(peak), TwoPhotonValues::Relative) => {
                    out.push_str(&format!("{key} = {}\n", peak.value));
                    if let Some(w) = peak.wavelength {
                        out.push_str(&format!("two_photon_reference_um = {w}\n"));
                    }
                    out.push_str(&format!("two_photon_spectrum = {}\n", table(&two.spectrum)));
                }
                (None, TwoPhotonValues::Relative) => {
                    out.push_str(
                        "# The two-photon spectrum is relative: give a published cross-section, the\n\
                         # wavelength it was measured at (two_photon_reference_um; else it applies\n\
                         # at the spectrum's greatest value), add its source to `source`, and\n\
                         # remove the comment marks.\n\
                         # two_photon_cross_section_gm = …\n\
                         # two_photon_reference_um = …\n",
                    );
                    for line in format!("two_photon_spectrum = {}", table(&two.spectrum)).lines() {
                        out.push_str(&format!("# {line}\n"));
                    }
                }
            }
        }
        out.push_str(
            "concentrations = [ ]      # e.g. { sample = \"S\", layer = 1, micromolar = 1.0 }\n",
        );
        out
    }
}

/// `<root>/optobench/sources/fpbase`.
pub fn fpbase_directory(root: &Path) -> PathBuf {
    root.join("optobench").join("sources").join("fpbase")
}

/// Stores the files read for `protein` unchanged, with a `provenance.json`, in
/// `<root>/optobench/sources/fpbase/<version>/`, `<version>` the first 16 hexadecimal
/// digits of the spectra file's SHA-256. Returns the provenance record.
pub fn import(
    root: &Path,
    protein: &Protein,
    spectra: (&Path, &[u8]),
    proteins: Option<(&Path, &[u8])>,
    two_photon: Option<(&Path, &[u8])>,
) -> Result<Value, String> {
    let sha256 = sha256_hex(spectra.1);
    let directory = fpbase_directory(root).join(&sha256[..16]);
    fs::create_dir_all(&directory).map_err(|e| format!("{}: {e}", directory.display()))?;
    let write = |file: &str, bytes: &[u8]| {
        let path = directory.join(file);
        fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    };
    write(
        &format!("{}-spectra.json", protein.slug_or_name()),
        spectra.1,
    )?;
    if let Some((_, bytes)) = proteins {
        write(&format!("{}-protein.json", protein.slug_or_name()), bytes)?;
    }
    if let Some((path, bytes)) = two_photon {
        let extension = path
            .extension()
            .map_or("txt".to_owned(), |e| e.to_string_lossy().to_lowercase());
        write(
            &format!("{}-two-photon.{extension}", protein.slug_or_name()),
            bytes,
        )?;
    }
    let record = json!({
        "source": "fpbase",
        "protein": protein.name,
        "slug": protein.slug,
        "state": protein.state,
        "doi": protein.doi,
        "citation": CITATION,
        "notice": NOTICE,
        "spectra_file": spectra.0.to_string_lossy(),
        "spectra_sha256": sha256,
        "protein_file": proteins.map(|p| p.0.to_string_lossy().into_owned()),
        "protein_sha256": proteins.map(|p| sha256_hex(p.1)),
        "two_photon_file": two_photon.map(|p| p.0.to_string_lossy().into_owned()),
        "two_photon_sha256": two_photon.map(|p| sha256_hex(p.1)),
        "two_photon_peak_gm": protein.two_photon.as_ref().and_then(|t| t.peak.as_ref().map(|p| p.value)),
        "two_photon_peak_um": protein.two_photon.as_ref().and_then(|t| t.peak.as_ref().and_then(|p| p.wavelength)),
        "two_photon_peak_source": protein.two_photon.as_ref().and_then(|t| t.peak.as_ref().map(|p| p.source.clone())),
        "directory": directory.to_string_lossy(),
    });
    let pretty = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    write("provenance.json", pretty.as_bytes())?;
    Ok(record)
}

impl Protein {
    fn slug_or_name(&self) -> String {
        let base = if self.slug.is_empty() {
            &self.name
        } else {
            &self.slug
        };
        base.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Responses shaped like the API's, with made-up values (not FPbase data).
    const SPECTRA: &str = r#"[{"name": "TestFP", "slug": "testfp", "spectra": [
        {"state": "default_ex", "ec": 50000, "max": 480, "data": [[460, 0.5], [480, 1.0], [500, 0.25]]},
        {"state": "default_em", "qy": 0.7, "max": 510, "data": [[490, 0.2], [510, 1.0], [540, 0.3]]},
        {"state": "off_ex", "ec": 1000, "data": [[400, 1.0], [420, 0.5]]}
    ]}]"#;
    const PROTEINS: &str = r#"[{"name": "TestFP", "slug": "testfp", "doi": "10.0000/test.1",
        "states": [{"slug": "testfp_default", "name": "default", "ext_coeff": 50000, "qy": 0.7, "lifetime": 3.1}]}]"#;

    #[test]
    fn a_state_is_read_with_its_reference() {
        let p = read(
            SPECTRA.as_bytes(),
            Some(PROTEINS.as_bytes()),
            None,
            "default",
        )
        .expect("read");
        assert_eq!(p.name, "TestFP");
        assert_eq!(p.excitation.as_ref().expect("ex")[1], (0.48, 1.0));
        assert_eq!(p.emission.as_ref().expect("em").len(), 3);
        assert!(p.two_photon.is_none());
        assert_eq!(p.extinction, Some(50000.0));
        assert_eq!(p.quantum_yield, Some(0.7));
        assert_eq!(p.lifetime, Some(3.1));
        assert_eq!(p.doi.as_deref(), Some("10.0000/test.1"));
        let source = p.source("abcdef0123456789ff");
        assert!(source.contains(CITATION) && source.contains("doi:10.0000/test.1"));
        // Another state; an unknown one names the known.
        let off = read(SPECTRA.as_bytes(), None, Some("testfp"), "off").expect("off");
        assert_eq!(off.extinction, Some(1000.0));
        assert!(off.emission.is_none() && off.quantum_yield.is_none());
        let error = read(SPECTRA.as_bytes(), None, None, "on").expect_err("no state");
        assert!(error.contains("default, off"), "{error}");
        assert!(read(SPECTRA.as_bytes(), None, Some("OtherFP"), "default").is_err());
        assert!(read(b"[1, 2]", None, None, "default").is_err());
        let backwards = SPECTRA.replace("[460, 0.5], [480, 1.0]", "[480, 0.5], [460, 1.0]");
        assert!(
            read(backwards.as_bytes(), None, None, "default")
                .expect_err("not increasing")
                .contains("not increasing")
        );
    }

    #[test]
    fn a_two_photon_table_is_read_and_scaled() {
        // Several spectra in one table; the two-photon column found by its name.
        let csv = "wavelength,TestFP EX,TestFP 2P\n700,,10\n800,0.1,40\n900,0.2,20\n";
        let (rows, label) = read_two_photon(csv.as_bytes(), None, None).expect("read");
        assert_eq!(label, "TestFP 2P");
        assert_eq!(rows, vec![(0.7, 10.0), (0.8, 40.0), (0.9, 20.0)]);
        // Given a column; tabs; µm stays µm; JSON pairs.
        let (rows, _) = read_two_photon(
            "nm\tex\tfoo\n0.8\t1\t3\n0.9\t1\t6\n".as_bytes(),
            Some("foo"),
            None,
        )
        .expect("read");
        assert_eq!(rows[1], (0.9, 6.0));
        let (rows, _) = read_two_photon(b"[[800, 0.5], [900, 1.0]]", None, None).expect("json");
        assert_eq!(rows[0], (0.8, 0.5));
        assert!(
            read_two_photon("w,a,b\n800,1,2\n".as_bytes(), None, None)
                .expect_err("which")
                .contains("which column")
        );
        // Two "2P" columns, as FPbase's viewer exports them: the protein's is taken.
        let both = "\u{feff}\"Wavelength\",\"AFP 2P\",\"BFP 2P\"\n800,1,0.5\n900,0.5,1\n";
        let (rows, label) = read_two_photon(both.as_bytes(), None, Some("bfp")).expect("named");
        assert_eq!((label.as_str(), rows[1]), ("BFP 2P", (0.9, 1.0)));
        assert!(read_two_photon(both.as_bytes(), None, None).is_err());
        // In GM as it is; relative with a peak at a wavelength, which must be on the table.
        let gm = TwoPhoton::new(
            vec![(0.7, 10.0), (0.8, 40.0)],
            TwoPhotonValues::Absorption,
            None,
            false,
            "f",
            "00",
        )
        .expect("gm");
        assert_eq!(gm.spectrum[1].1, 40.0);
        let peak = |w: Option<f64>, source: &str| Peak {
            value: 30.0,
            wavelength: w,
            source: source.into(),
        };
        let rows = vec![(0.7, 1.2), (0.8, 1.0), (0.9, 0.5)];
        assert!(
            TwoPhoton::new(
                rows.clone(),
                TwoPhotonValues::Relative,
                Some(peak(Some(0.8), "paper")),
                false,
                "f",
                "00"
            )
            .is_ok()
        );
        assert!(
            TwoPhoton::new(
                rows.clone(),
                TwoPhotonValues::Relative,
                Some(peak(Some(1.2), "paper")),
                false,
                "f",
                "00"
            )
            .is_err()
        );
        assert!(
            TwoPhoton::new(
                rows.clone(),
                TwoPhotonValues::Relative,
                Some(peak(None, " ")),
                false,
                "f",
                "00"
            )
            .is_err()
        );
        assert!(
            TwoPhoton::new(
                rows,
                TwoPhotonValues::Action,
                Some(peak(None, "paper")),
                true,
                "f",
                "00"
            )
            .is_err()
        );
    }
}
