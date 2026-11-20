//! The commands M12 reads, checked against the app's own schemas (the engine command
//! `engine.describe`, run through `engine.execute`)
//! before anything is read: each must exist with a typed contract, and its schema must
//! still have every field optobench reads. A SolidCraft whose contract changed is refused
//! with the changes named, rather than read wrongly.

use serde_json::{Value, json};

use crate::BridgeError;
use crate::connection::Connection;

/// One field optobench reads: in `side` (`input` or `output`) of `command`'s schema, the
/// properties of `definition` (one of the schema's `$defs`, or `None` for the top level).
struct Needed {
    command: &'static str,
    side: &'static str,
    definition: Option<&'static str>,
    properties: &'static [&'static str],
}

/// What M12 reads, as SolidCraft 0.45.0 describes it.
const NEEDED: &[Needed] = &[
    Needed {
        command: "document.revision",
        side: "output",
        definition: None,
        properties: &["document_id", "revision"],
    },
    Needed {
        command: "component.list",
        side: "output",
        definition: None,
        properties: &["components"],
    },
    Needed {
        command: "component.list",
        side: "output",
        definition: Some("ComponentNode"),
        properties: &["id", "name", "children"],
    },
    Needed {
        command: "component.list",
        side: "output",
        definition: Some("PlacementInfo"),
        properties: &["id", "component", "parent", "name", "transform", "grounded"],
    },
    Needed {
        command: "joint.list",
        side: "output",
        definition: None,
        properties: &["joints"],
    },
    Needed {
        command: "joint.list",
        side: "output",
        definition: Some("ListedJoint"),
        properties: &["id", "name", "type", "a", "b", "values", "suppressed"],
    },
    Needed {
        command: "joint.list",
        side: "output",
        definition: Some("NamedValue"),
        properties: &["name", "value", "unit", "limits"],
    },
    Needed {
        command: "parameters.list",
        side: "output",
        definition: None,
        properties: &["parameters"],
    },
    Needed {
        command: "parameters.list",
        side: "output",
        definition: Some("ParameterRow"),
        properties: &["name", "expression", "value", "unit", "source"],
    },
    Needed {
        command: "file.export_step",
        side: "input",
        definition: None,
        properties: &["path"],
    },
    Needed {
        command: "file.export_step",
        side: "output",
        definition: None,
        properties: &["path", "bytes"],
    },
];

/// The commands M12 uses, by id.
pub fn commands() -> Vec<&'static str> {
    let mut ids: Vec<&str> = NEEDED.iter().map(|n| n.command).collect();
    ids.dedup();
    ids
}

/// What is missing from `schema` (an `engine.describe` reply) for `needed`.
fn missing(schema: &Value, needed: &Needed) -> Vec<String> {
    let side = &schema[needed.side];
    if side.is_null() {
        return vec![format!(
            "`{}` has no typed {} contract",
            needed.command, needed.side
        )];
    }
    let object = match needed.definition {
        None => side,
        Some(d) => &side["$defs"][d],
    };
    needed
        .properties
        .iter()
        .filter(|p| object["properties"].get(**p).is_none())
        .map(|p| {
            format!(
                "`{}` {}{} has no `{p}`",
                needed.command,
                needed.side,
                needed.definition.map_or(String::new(), |d| format!(" {d}"))
            )
        })
        .collect()
}

/// Checks the app's contracts for every command M12 reads; [`BridgeError::Contract`]
/// lists every difference.
pub fn check(connection: &mut Connection) -> Result<(), BridgeError> {
    let mut changes = Vec::new();
    for command in commands() {
        // `engine.describe` is an engine command (through `engine.execute`), not a method.
        match connection.execute("engine.describe", json!({ "command": command })) {
            Ok(schema) => {
                for needed in NEEDED.iter().filter(|n| n.command == command) {
                    changes.extend(missing(&schema, needed));
                }
            }
            Err(BridgeError::Refused(why)) => {
                changes.push(format!("`{command}` is not known ({why})"));
            }
            Err(other) => return Err(other),
        }
    }
    if changes.is_empty() {
        Ok(())
    } else {
        Err(BridgeError::Contract(changes))
    }
}
