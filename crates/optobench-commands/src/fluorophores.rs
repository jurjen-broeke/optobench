//! `import-fluorophore`: a fluorescent protein from FPbase, imported from the JSON the user
//! downloaded from its API (`optobench_io::fpbase`), into a design as a `[[fluorophore]]`.

use std::path::Path;

use optobench_io::fpbase;
use serde_json::{Value, json};

use crate::materials::{data_directory, flag, required, text};
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidParameters, message)
}

fn failed(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::Failed, message)
}

fn read_file(path: &str) -> Result<Vec<u8>, CommandError> {
    std::fs::read(path).map_err(|e| failed(format!("{path}: {e}")))
}

/// `import-fluorophore`: parameters `spectra` (the file saved from
/// `https://www.fpbase.org/api/proteins/spectra/?name__iexact=<name>&format=json`), and
/// optionally `protein` (the file from `/api/proteins/?name__iexact=<name>&format=json`,
/// for the lifetime and the primary reference), `name` (which protein, when a file holds
/// several), `state` (default `default`), `id` (the fluorophore's id in the design;
/// default the protein's slug), `append_to` (a layout or sidecar to add the entry to),
/// `two_photon` (a two-photon excitation spectrum the user exported from FPbase's website,
/// which its API does not give: a table with a header, or JSON pairs) with
/// `two_photon_column` (default the column named like `2P`), `two_photon_values`
/// (`relative`, default; `gm`; `action-gm`) and, for a relative spectrum,
/// `two_photon_peak_gm`, `two_photon_peak_action` and `two_photon_peak_source`;
/// `accept_licence` and `data_directory`. Without `accept_licence: true` nothing is stored
/// or written: the result shows FPbase's terms, the protein's reference and the entry.
pub(crate) fn import_fluorophore(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(
        parameters,
        &[
            "spectra",
            "protein",
            "name",
            "state",
            "id",
            "append_to",
            "two_photon",
            "two_photon_column",
            "two_photon_values",
            "two_photon_peak_gm",
            "two_photon_peak_action",
            "two_photon_peak_source",
            "accept_licence",
            "data_directory",
        ],
    )?;
    let spectra_path = required(parameters, "spectra")?;
    let spectra = read_file(spectra_path)?;
    let protein_path = text(parameters, "protein")?;
    let protein_bytes = protein_path.map(read_file).transpose()?;
    let state = text(parameters, "state")?.unwrap_or("default");
    let mut protein = fpbase::read(
        &spectra,
        protein_bytes.as_deref(),
        text(parameters, "name")?,
        state,
    )
    .map_err(|e| failed(format!("{spectra_path}: {e}")))?;
    let two_photon_path = text(parameters, "two_photon")?;
    let two_photon_bytes = two_photon_path.map(read_file).transpose()?;
    if let (Some(path), Some(bytes)) = (two_photon_path, &two_photon_bytes) {
        let (rows, column) = fpbase::read_two_photon(bytes, text(parameters, "two_photon_column")?)
            .map_err(|e| failed(format!("{path}: {e}")))?;
        let values = match text(parameters, "two_photon_values")? {
            None | Some("relative") => fpbase::TwoPhotonValues::Relative,
            Some("gm") => fpbase::TwoPhotonValues::Absorption,
            Some("action-gm") => fpbase::TwoPhotonValues::Action,
            Some(other) => {
                return Err(invalid(format!(
                    "`two_photon_values` `{other}` is not known; use \"relative\", \"gm\" or \"action-gm\""
                )));
            }
        };
        let peak = match parameters.get("two_photon_peak_gm") {
            None => None,
            Some(v) => Some((
                v.as_f64()
                    .ok_or_else(|| invalid("`two_photon_peak_gm` must be a number"))?,
                text(parameters, "two_photon_peak_source")?
                    .ok_or_else(|| {
                        invalid("say where the peak comes from (`two_photon_peak_source`)")
                    })?
                    .to_owned(),
                flag(parameters, "two_photon_peak_action")?,
            )),
        };
        let file = std::path::Path::new(path)
            .file_name()
            .map_or(path.to_owned(), |f| f.to_string_lossy().into_owned());
        let file = format!("{file}, column {column}");
        protein.two_photon = Some(
            fpbase::TwoPhoton::new(rows, values, peak, &file, &optobench_io::sha256_hex(bytes))
                .map_err(invalid)?,
        );
    } else if [
        "two_photon_column",
        "two_photon_values",
        "two_photon_peak_gm",
        "two_photon_peak_action",
        "two_photon_peak_source",
    ]
    .iter()
    .any(|k| parameters.contains_key(*k))
    {
        return Err(invalid(
            "the two-photon parameters need the `two_photon` file",
        ));
    }
    let id = match text(parameters, "id")? {
        Some(id) => id.to_owned(),
        None if !protein.slug.is_empty() => protein.slug.clone(),
        None => protein.name.to_lowercase().replace(' ', "-"),
    };
    let sha256 = optobench_io::sha256_hex(&spectra);
    let entry = protein.entry(&id, &sha256);
    let mut missing = Vec::new();
    for (what, absent) in [
        ("quantum yield", protein.quantum_yield.is_none()),
        ("extinction coefficient", protein.extinction.is_none()),
        ("excitation spectrum", protein.excitation.is_none()),
        ("emission spectrum", protein.emission.is_none()),
        (
            "lifetime (give the `protein` file)",
            protein.lifetime.is_none(),
        ),
        (
            "primary reference (give the `protein` file)",
            protein.doi.is_none(),
        ),
    ] {
        if absent {
            missing.push(what);
        }
    }
    let summary = json!({
        "protein": protein.name,
        "slug": protein.slug,
        "state": protein.state,
        "doi": protein.doi,
        "quantum_yield": protein.quantum_yield,
        "extinction_coefficient": protein.extinction,
        "lifetime_ns": protein.lifetime,
        "two_photon_spectrum": protein.two_photon.as_ref().map(|t| json!({
            "file": t.file,
            "rows": t.spectrum.len(),
            "range_um": [t.spectrum.first().map(|r| r.0), t.spectrum.last().map(|r| r.0)],
            "peak_gm": t.peak,
            "action": t.action,
            "peak_source": t.peak_source,
        })),
        "missing": missing,
        "two_photon_note": "FPbase's API gives no two-photon data: export the protein's two-photon excitation spectrum from the website and pass it as `two_photon`; a relative spectrum needs `two_photon_peak_gm` with `two_photon_peak_source`. Emission after two-photon excitation follows the one-photon emission spectrum.",
    });
    if !flag(parameters, "accept_licence")? {
        return Ok(json!({
            "stored": false,
            "notice": fpbase::NOTICE,
            "fluorophore": summary,
            "entry": entry,
            "next": "Read FPbase's terms and the protein's reference. To import, run again with \"accept_licence\": true \
                     (and \"append_to\" a design to add the entry to it).",
        }));
    }
    let root = data_directory(parameters)?;
    let record = fpbase::import(
        &root,
        &protein,
        (Path::new(spectra_path), &spectra),
        protein_path
            .zip(protein_bytes.as_deref())
            .map(|(p, b)| (Path::new(p), b)),
        two_photon_path
            .zip(two_photon_bytes.as_deref())
            .map(|(p, b)| (Path::new(p), b)),
    )
    .map_err(failed)?;
    let mut appended = None;
    if let Some(design) = text(parameters, "append_to")? {
        if protein.quantum_yield.is_none() {
            return Err(invalid(format!(
                "`{}` has no quantum yield on FPbase for state `{state}`; add the entry by hand with one",
                protein.name
            )));
        }
        let original =
            std::fs::read_to_string(design).map_err(|e| failed(format!("{design}: {e}")))?;
        let root_table: toml::Table = original
            .parse()
            .map_err(|e| failed(format!("{design}: not valid TOML: {e}")))?;
        let taken = root_table
            .get("fluorophore")
            .and_then(|v| v.as_array())
            .is_some_and(|list| {
                list.iter()
                    .any(|f| f.get("id").and_then(|i| i.as_str()) == Some(id.as_str()))
            });
        if taken {
            return Err(invalid(format!(
                "{design} already has a fluorophore `{id}`; pass another `id`"
            )));
        }
        let (body, resolved) = optobench_io::resolved::split(&original)
            .map_err(|e| failed(format!("{design}: {e}")))?;
        let mut text = body.trim_end().to_owned();
        text.push_str("\n\n");
        text.push_str(&entry);
        if let Some(resolved) = resolved {
            text.push('\n');
            text.push_str(resolved);
        }
        std::fs::write(design, text).map_err(|e| failed(format!("{design}: {e}")))?;
        appended = Some(design.to_owned());
    }
    Ok(json!({
        "stored": true,
        "provenance": record,
        "fluorophore": summary,
        "entry": entry,
        "appended_to": appended,
    }))
}
