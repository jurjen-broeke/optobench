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
//! Spectra are `[wavelength nm, value]`, normalised to a peak of 1; a state's spectra are
//! named `<state>_ex`, `<state>_em` and, where FPbase has one, `<state>_2p` (the schema's
//! `2p` subtype; its naming here follows the other two and has not been seen in a
//! response). The API has no two-photon cross-section: a two-photon spectrum comes out
//! relative, to be scaled by a published peak value. The API "is still a work in
//! development and is subject to change", so every field used is checked.
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
    /// Two-photon spectrum, (µm, relative).
    pub two_photon: Option<Vec<(f64, f64)>>,
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
    let (ex, em, two) = (named("ex"), named("em"), named("2p"));
    if ex.is_none() && em.is_none() && two.is_none() {
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
        two_photon: two
            .map(|s| spectrum(s, &format!("{state}_2p")))
            .transpose()?,
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
        format!(
            "{CITATION}, protein {} (https://www.fpbase.org/protein/{}/), state {}; {reference}; imported file sha256 {}",
            self.name,
            self.slug,
            self.state,
            &sha256[..16.min(sha256.len())]
        )
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
            out.push_str(
                "# FPbase gives the two-photon spectrum relative only: give its peak cross-section\n\
                 # from a publication (absorption, or an action cross-section η σ₂ as\n\
                 # two_photon_action_cross_section_gm) and remove the comment marks.\n\
                 # two_photon_cross_section_gm = …\n",
            );
            for line in format!("two_photon_spectrum = {}", table(two)).lines() {
                out.push_str(&format!("# {line}\n"));
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
        {"state": "default_2p", "data": [[900, 0.6], [920, 1.0], [960, 0.4]]},
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
        assert_eq!(p.two_photon.as_ref().expect("2p")[0], (0.9, 0.6));
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
}
