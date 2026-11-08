//! `edit`: change a layout or sidecar file in place, keeping its comments
//! (`optobench_io::edit`), and only when the result still reads as a scene with the
//! user's data: an edit that would break the design is refused and the file is left as
//! it was. The interface's editors use it; the previous text comes back for undoing.

use std::path::Path;

use optobench_io::edit;
use serde_json::{Value, json};

use crate::trace::{load_text, text};
use crate::{CommandError, ErrorCode, Parameters, check_keys};

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidParameters, message)
}

fn failed(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::Failed, message)
}

/// Writes a file through a temporary file beside it, so a failed write leaves the old
/// file whole.
fn write_whole(path: &Path, contents: &str) -> Result<(), CommandError> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = path.with_file_name(format!(".{name}.writing"));
    std::fs::write(&temporary, contents)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&temporary);
            failed(format!("{}: {e}", path.display()))
        })
}

/// `edit`: parameters `path` and `operations` (a list, applied in order; see
/// `optobench_io::edit`: `set`, `unset`, `add`, `remove`, `add_dof`, `remove_dof`, or one
/// `replace` with the whole `text`); optionally `create` (`{ "layout": "planar" |
/// "table", "pitch": mm }`: start a new layout file at `path`, which must not exist),
/// `check` (default true: read the result as a scene first; a new layout without
/// sources is not read), `dry_run` (return the new text, write nothing) and
/// `data_directory`. Returns the path, whether the text changed, the previous text
/// (`null` for a new file) and, for a dry run, the new text.
pub(crate) fn edit(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(
        parameters,
        &[
            "path",
            "operations",
            "create",
            "check",
            "dry_run",
            "data_directory",
        ],
    )?;
    let path = text(parameters, "path")?.ok_or_else(|| invalid("missing parameter `path`"))?;
    let operations = match parameters.get("operations") {
        None => Vec::new(),
        Some(Value::Array(list)) => list.clone(),
        Some(_) => return Err(invalid("`operations` must be a list")),
    };
    let flag = |key: &str, default: bool| match parameters.get(key) {
        None => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(invalid(format!("`{key}` must be true or false"))),
    };
    let check = flag("check", true)?;
    let dry_run = flag("dry_run", false)?;
    let file = Path::new(path);

    let (before, created) = match parameters.get("create") {
        Some(create) => {
            if file.exists() {
                return Err(invalid(format!(
                    "{path} exists already; `create` starts a new file"
                )));
            }
            let layout = create["layout"]
                .as_str()
                .ok_or_else(|| invalid("`create` needs `layout`: \"planar\" or \"table\""))?;
            let pitch = create.get("pitch").and_then(Value::as_f64);
            (
                edit::new_layout(layout, pitch).map_err(|e| invalid(e.0))?,
                true,
            )
        }
        None => (
            std::fs::read_to_string(file).map_err(|e| failed(format!("{path}: {e}")))?,
            false,
        ),
    };
    let after = edit::edit(&before, &operations).map_err(|e| invalid(e.0))?;

    // Read the result as the scene it describes; a new layout is empty until it has a
    // source, so it is read once it has one.
    let empty_new = created && !after.contains("[[source]]");
    if check && !empty_new {
        load_text(path, after.clone(), parameters).map_err(|e| {
            CommandError::new(e.code, format!("the edit was not written: {}", e.message))
        })?;
    }
    let changed = created || after != before;
    if !dry_run && changed {
        write_whole(file, &after)?;
    }
    let mut out = json!({
        "path": path,
        "changed": changed,
        "previous": if created { Value::Null } else { Value::String(before) },
    });
    if dry_run {
        out["text"] = Value::String(after);
    }
    Ok(out)
}
