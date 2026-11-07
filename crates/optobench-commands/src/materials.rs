//! Commands for materials: `import-material` and `materials`.

use std::path::{Path, PathBuf};

use optobench_io::store::{self, StoreError, default_data_directory};
use optobench_materials::bundled::{
    FUSED_SILICA, Provenance, STANDARD_AIR, WATER_ABSORPTION, WATER_INDEX,
};
use serde_json::{Value, json};

use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn text<'a>(parameters: &'a Parameters, key: &str) -> Result<Option<&'a str>, CommandError> {
    match parameters.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(CommandError::new(
            ErrorCode::InvalidParameters,
            format!("`{key}` must be a string"),
        )),
    }
}

fn required<'a>(parameters: &'a Parameters, key: &str) -> Result<&'a str, CommandError> {
    text(parameters, key)?.ok_or_else(|| {
        CommandError::new(
            ErrorCode::InvalidParameters,
            format!("missing parameter `{key}`"),
        )
    })
}

fn flag(parameters: &Parameters, key: &str) -> Result<bool, CommandError> {
    match parameters.get(key) {
        None => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(CommandError::new(
            ErrorCode::InvalidParameters,
            format!("`{key}` must be true or false"),
        )),
    }
}

/// The data directory: the `data_directory` parameter, or the platform default.
fn data_directory(parameters: &Parameters) -> Result<PathBuf, CommandError> {
    match text(parameters, "data_directory")? {
        Some(path) => Ok(PathBuf::from(path)),
        None => default_data_directory().ok_or_else(|| {
            CommandError::new(
                ErrorCode::Failed,
                "no user data directory could be found; pass `data_directory`",
            )
        }),
    }
}

fn failed(error: StoreError) -> CommandError {
    let code = match error {
        StoreError::InvalidName(_) | StoreError::NameTaken { .. } => ErrorCode::InvalidParameters,
        _ => ErrorCode::Failed,
    };
    CommandError::new(code, error.to_string())
}

/// `import-material`: read a refractiveindex.info material file the user downloaded and
/// store it in the data directory. Without `accept_licence: true` nothing is stored; the
/// result shows the file's notice and references so they can be read first.
pub(crate) fn import_material(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(
        parameters,
        &[
            "path",
            "name",
            "accept_licence",
            "redistribute",
            "data_directory",
        ],
    )?;
    let path = Path::new(required(parameters, "path")?);
    let name = required(parameters, "name")?;
    let redistribute = flag(parameters, "redistribute")?;
    store::check_name(name).map_err(failed)?;
    if !flag(parameters, "accept_licence")? {
        let (preview, _) = store::preview(path).map_err(failed)?;
        return Ok(json!({
            "stored": false,
            "notice": preview.notice,
            "references": preview.references,
            "comments": preview.comments,
            "sha256": preview.sha256,
            "redistribute": redistribute,
            "next": "Read the notice and references. To import, run again with \"accept_licence\": true. \
                     Values are embedded in shared designs only with \"redistribute\": true.",
        }));
    }
    let root = data_directory(parameters)?;
    let record = store::import(&root, path, name, redistribute).map_err(failed)?;
    Ok(json!({ "stored": true, "material": record.to_json() }))
}

fn bundled(name: &str, provenance: &Provenance) -> Value {
    json!({
        "name": name,
        "reference": provenance.reference,
        "transcribed_from": provenance.transcribed_from,
        "licence": provenance.licence,
        "verified": provenance.verified,
    })
}

/// `materials`: the bundled materials and those imported into the data directory.
pub(crate) fn materials(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(parameters, &["data_directory"])?;
    let root = data_directory(parameters)?;
    let imported: Vec<Value> = store::list(&root)
        .map_err(failed)?
        .iter()
        .map(store::Record::to_json)
        .collect();
    Ok(json!({
        "bundled": [
            bundled("fused-silica", &FUSED_SILICA),
            bundled("standard-air", &STANDARD_AIR),
            bundled("water", &WATER_INDEX),
            bundled("water-hale-querry-1973", &WATER_ABSORPTION),
        ],
        "imported": imported,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Registry;

    #[test]
    fn import_needs_licence_acceptance_then_lists_the_material() {
        let scratch =
            std::env::temp_dir().join(format!("optobench-commands-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("temporary directory");
        let file = scratch.join("glass.yml");
        std::fs::write(
            &file,
            "# copyright and related rights waived via CC0 1.0\nREFERENCES: |\n    A test.\nDATA:\n  - type: formula 2\n    wavelength_range: 0.4 0.8\n    coefficients: 0 1.25 0\n",
        )
        .expect("write");
        let data = scratch.join("data");
        let registry = Registry::standard();
        let parameters = |accept: bool| {
            json!({
                "path": file.to_string_lossy(),
                "name": "test-glass",
                "accept_licence": accept,
                "data_directory": data.to_string_lossy(),
            })
        };

        let preview = registry
            .run("import-material", &parameters(false))
            .expect("previews");
        assert_eq!(preview["stored"], false);
        assert_eq!(
            preview["notice"][0],
            "copyright and related rights waived via CC0 1.0"
        );
        let listed = registry
            .run(
                "materials",
                &json!({ "data_directory": data.to_string_lossy() }),
            )
            .expect("lists");
        assert_eq!(listed["imported"].as_array().map(Vec::len), Some(0));

        let stored = registry
            .run("import-material", &parameters(true))
            .expect("imports");
        assert_eq!(stored["stored"], true);
        assert_eq!(stored["material"]["redistribute"], false);
        let listed = registry
            .run(
                "materials",
                &json!({ "data_directory": data.to_string_lossy() }),
            )
            .expect("lists");
        assert_eq!(listed["imported"][0]["name"], "test-glass");
        assert_eq!(listed["bundled"][0]["verified"], false);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn bad_parameters_are_reported() {
        let registry = Registry::standard();
        let error = registry
            .run("import-material", &json!({ "name": "x" }))
            .err()
            .map(|e| e.code);
        assert_eq!(error, Some(ErrorCode::InvalidParameters));
        let error = registry
            .run(
                "import-material",
                &json!({ "path": "a.yml", "name": "Bad Name" }),
            )
            .err()
            .map(|e| e.code);
        assert_eq!(error, Some(ErrorCode::InvalidParameters));
    }
}
