//! The command registry: every command takes JSON parameters and returns a JSON result.
//!
//! This is the single entry point the CLI, the desktop UI, a later MCP server and a
//! SolidCraft plugin all call, the same pattern as SolidCraft's `run(command, args)`.
//! See `docs/design.md`, section Architecture.
//!
//! Layer L5: commands may read and write files through `optobench-io`.
//!
//! Conventions:
//! - Command ids are lowercase ASCII words joined by `-`, written in full
//!   (`export-placements`, never `exp-place`). They pass `cargo xtask naming`.
//! - Parameters are a JSON object (or `null` for none). Parsing is strict: an unknown key
//!   is an error, as for `.optobench` files (docs/design.md, Format tag and versions).
//! - A failure is a [`CommandError`], written as `{"error": {"code": ..., "message": ...}}`.

#![forbid(unsafe_code)]

mod embed;
mod materials;
mod moves;
mod optimize;
mod parts;
mod trace;

use serde_json::{Map, Value, json};

/// The parameters a command receives: a JSON object.
pub type Parameters = Map<String, Value>;

/// The function behind a command.
pub type Handler = fn(&Parameters) -> Result<Value, CommandError>;

/// One command in the registry.
#[derive(Debug, Clone, Copy)]
pub struct Command {
    /// The id callers use, e.g. `version`.
    pub id: &'static str,
    /// One line for help texts and `commands`.
    pub summary: &'static str,
    /// Runs the command.
    pub handler: Handler,
}

/// Why a command failed. The code is stable for callers; the message is for people.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// No command has the requested id.
    UnknownCommand,
    /// The parameters are not an object, have an unknown key, or a value is invalid.
    InvalidParameters,
    /// The command ran and could not complete.
    Failed,
}

impl ErrorCode {
    /// The code as written in JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::UnknownCommand => "unknown_command",
            ErrorCode::InvalidParameters => "invalid_parameters",
            ErrorCode::Failed => "failed",
        }
    }
}

/// A failed command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    /// What kind of failure.
    pub code: ErrorCode,
    /// What went wrong, for people.
    pub message: String,
}

impl CommandError {
    /// Creates an error.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// The error as JSON: `{"error": {"code": ..., "message": ...}}`.
    pub fn to_json(&self) -> Value {
        json!({ "error": { "code": self.code.as_str(), "message": self.message } })
    }
}

impl core::fmt::Display for CommandError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for CommandError {}

/// The commands of this release. New commands arrive with their milestones.
const STANDARD: &[Command] = &[
    Command {
        id: "version",
        summary: "the application and file-format versions",
        handler: version,
    },
    Command {
        id: "commands",
        summary: "every available command with its summary",
        handler: commands,
    },
    Command {
        id: "materials",
        summary: "bundled materials and those imported into the data directory",
        handler: materials::materials,
    },
    Command {
        id: "trace",
        summary: "trace a .optobench layout: detectors, clipping, power ledger, layout view",
        handler: trace::trace,
    },
    Command {
        id: "export-placements",
        summary: "from a layout: element poses for CAD and a matching sidecar skeleton",
        handler: trace::export_placements,
    },
    Command {
        id: "bodies",
        summary: "every placed body of a STEP file: path, product, name, fingerprint",
        handler: trace::bodies,
    },
    Command {
        id: "import-material",
        summary: "import a downloaded refractiveindex.info material file, licence shown first",
        handler: materials::import_material,
    },
    Command {
        id: "parts",
        summary: "the parts in the user's parts library",
        handler: parts::parts,
    },
    Command {
        id: "import-part",
        summary: "check a part file and copy it into the parts library",
        handler: parts::import_part,
    },
    Command {
        id: "curves",
        summary: "the measured R/T curves in the user's parts library",
        handler: parts::curves,
    },
    Command {
        id: "import-curve",
        summary: "import a measured R/T table (CSV) into the parts library",
        handler: parts::import_curve,
    },
    Command {
        id: "embed",
        summary: "embed the library parts, curves and materials a design uses, so it traces without them",
        handler: embed::embed,
    },
    Command {
        id: "optimize",
        summary: "the DOF values that best meet a design's merit (focus, centring, pointing, collimation, power)",
        handler: optimize::optimize,
    },
    Command {
        id: "check-moves",
        summary: "after a move report's moves are made in CAD and exported again: is every part where it should be",
        handler: moves::check_moves,
    },
];

/// The set of commands callers can run.
#[derive(Debug, Clone)]
pub struct Registry {
    commands: &'static [Command],
}

impl Registry {
    /// The commands of this release.
    pub fn standard() -> Self {
        Self { commands: STANDARD }
    }

    /// Every command, in a stable order.
    pub fn commands(&self) -> &[Command] {
        self.commands
    }

    /// The command with this id, if any.
    pub fn get(&self, id: &str) -> Option<&Command> {
        self.commands.iter().find(|c| c.id == id)
    }

    /// Runs a command. `parameters` must be a JSON object, or `null` for none.
    pub fn run(&self, id: &str, parameters: &Value) -> Result<Value, CommandError> {
        let command = self.get(id).ok_or_else(|| {
            CommandError::new(
                ErrorCode::UnknownCommand,
                format!("unknown command `{id}`; run `commands` for the list"),
            )
        })?;
        let empty = Parameters::new();
        let parameters = match parameters {
            Value::Null => &empty,
            Value::Object(map) => map,
            _ => {
                return Err(CommandError::new(
                    ErrorCode::InvalidParameters,
                    "parameters must be a JSON object",
                ));
            }
        };
        (command.handler)(parameters)
    }
}

/// Rejects any key not in `allowed`, so a typo is never silently ignored.
pub fn check_keys(parameters: &Parameters, allowed: &[&str]) -> Result<(), CommandError> {
    match parameters.keys().find(|k| !allowed.contains(&k.as_str())) {
        None => Ok(()),
        Some(key) if allowed.is_empty() => Err(CommandError::new(
            ErrorCode::InvalidParameters,
            format!("unknown parameter `{key}`; this command takes no parameters"),
        )),
        Some(key) => Err(CommandError::new(
            ErrorCode::InvalidParameters,
            format!(
                "unknown parameter `{key}`; valid parameters: {}",
                allowed.join(", ")
            ),
        )),
    }
}

fn version(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(parameters, &[])?;
    Ok(json!({
        "application": "optobench",
        "version": env!("CARGO_PKG_VERSION"),
        "file_format": optobench_io::format_tag(),
    }))
}

fn commands(parameters: &Parameters) -> Result<Value, CommandError> {
    check_keys(parameters, &[])?;
    let list: Vec<Value> = STANDARD
        .iter()
        .map(|c| json!({ "id": c.id, "summary": c.summary }))
        .collect();
    Ok(json!({ "commands": list }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_reports_the_file_format() {
        let result = Registry::standard().run("version", &Value::Null);
        assert_eq!(
            result
                .ok()
                .and_then(|v| v["file_format"].as_str().map(str::to_owned)),
            Some("optobench/1".to_owned())
        );
    }

    #[test]
    fn commands_lists_every_registered_id() {
        let registry = Registry::standard();
        let result = registry.run("commands", &json!({})).unwrap_or_default();
        let listed: Vec<&str> = result["commands"]
            .as_array()
            .map(|a| a.iter().filter_map(|c| c["id"].as_str()).collect())
            .unwrap_or_default();
        let ids: Vec<&str> = registry.commands().iter().map(|c| c.id).collect();
        assert_eq!(listed, ids);
    }

    #[test]
    fn unknown_command_is_an_error() {
        let error = Registry::standard()
            .run("no-such-command", &Value::Null)
            .err();
        assert_eq!(error.map(|e| e.code), Some(ErrorCode::UnknownCommand));
    }

    #[test]
    fn unknown_parameter_is_an_error() {
        let error = Registry::standard()
            .run("version", &json!({ "verbose": true }))
            .err();
        assert_eq!(error.map(|e| e.code), Some(ErrorCode::InvalidParameters));
    }

    #[test]
    fn parameters_must_be_an_object() {
        let error = Registry::standard().run("version", &json!([1, 2])).err();
        assert_eq!(error.map(|e| e.code), Some(ErrorCode::InvalidParameters));
    }

    #[test]
    fn errors_serialize_with_code_and_message() {
        let error = CommandError::new(ErrorCode::Failed, "no detector");
        assert_eq!(
            error.to_json(),
            json!({ "error": { "code": "failed", "message": "no detector" } })
        );
    }

    #[test]
    fn ids_are_unique_lowercase_words_joined_by_hyphens() {
        let registry = Registry::standard();
        let ids: Vec<&str> = registry.commands().iter().map(|c| c.id).collect();
        for (i, id) in ids.iter().enumerate() {
            assert!(!ids[..i].contains(id), "duplicate command id `{id}`");
            assert!(
                id.split('-')
                    .all(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase())),
                "command id `{id}` is not lowercase words joined by `-`"
            );
        }
    }
}
