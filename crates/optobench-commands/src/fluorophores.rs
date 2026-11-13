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
            "accept_licence",
            "data_directory",
        ],
    )?;
    let spectra_path = required(parameters, "spectra")?;
    let spectra = read_file(spectra_path)?;
    let protein_path = text(parameters, "protein")?;
    let protein_bytes = protein_path.map(read_file).transpose()?;
    let state = text(parameters, "state")?.unwrap_or("default");
    let protein = fpbase::read(
        &spectra,
        protein_bytes.as_deref(),
        text(parameters, "name")?,
        state,
    )
    .map_err(|e| failed(format!("{spectra_path}: {e}")))?;
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
        "two_photon_spectrum": protein.two_photon.is_some(),
        "missing": missing,
        "two_photon_cross_section": "FPbase gives none: add a published peak value to use two-photon excitation",
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
