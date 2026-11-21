//! Degrees of freedom made in the design open in SolidCraft (M13).
//!
//! The changes come as `cad-moves` gives them (`optobench-commands`): for each CAD
//! occurrence a DOF moves, its move in the assembly frame, or for bound DOFs the change of
//! a user parameter or a joint value. Writing them:
//!
//! 1. the design is read, and each change made absolute: a parameter's or joint's value
//!    now plus the change, a part's placement now times its move;
//! 2. `edit.checkpoint`;
//! 3. one `engine.batch` (with `rollback`, and `coalesce_parameters` so all parameters
//!    change in one rebuild): `parameters.change` of every bound parameter,
//!    `joint.drive` of every bound joint value, `occurrence.move` (in world space, by the
//!    occurrence's id and path, captured) of every part without bindings;
//! 4. the design read back: every parameter and joint value as asked, and every changed
//!    part where its move puts it (whatever moved it: a binding that does not move the
//!    part as its DOF says is caught here);
//! 5. anything different, or a command that failed, and the checkpoint is restored: the
//!    design, its undo history included, is as it was. Otherwise the checkpoint is
//!    dropped and the changes stay, one undo step each.

use serde_json::{Value, json};

use crate::BridgeError;
use crate::connection::Connection;
use crate::read::{self, Matrix, Snapshot, multiply};

/// How close a read-back must be to what was asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    /// A placement's translation (mm) and a parameter (mm or degrees).
    pub mm: f64,
    /// Each number of a placement's rotation.
    pub rotation: f64,
    /// A joint value (mm or degrees; `joint.list` rounds them for reading).
    pub joint: f64,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            mm: 1e-6,
            rotation: 1e-9,
            joint: 1e-3,
        }
    }
}

fn protocol(what: String) -> BridgeError {
    BridgeError::Protocol(what)
}

/// Each occurrence's path of names (joined by `/`, as a STEP export names them).
fn name_path(snapshot: &Snapshot, path: &[u64]) -> Option<String> {
    let names: Option<Vec<&str>> = (1..=path.len())
        .map(|k| {
            snapshot
                .occurrences
                .iter()
                .find(|o| o.path == path[..k])
                .map(|o| o.name.as_str())
        })
        .collect();
    names.map(|n| n.join("/"))
}

fn matrix(value: &Value) -> Result<Matrix, BridgeError> {
    let mut m = read::IDENTITY;
    for (i, row) in m.iter_mut().enumerate() {
        for (j, entry) in row.iter_mut().enumerate() {
            *entry = value[i][j]
                .as_f64()
                .ok_or_else(|| protocol("a move's `matrix` is not 4 rows of 4 numbers".into()))?;
        }
    }
    Ok(m)
}

/// What to check after writing.
enum Check {
    Parameter {
        name: String,
        value: f64,
    },
    Joint {
        id: u64,
        name: String,
        index: usize,
        value: f64,
    },
    Placement {
        path: Vec<u64>,
        name: String,
        world: Matrix,
    },
}

/// Writes `cad_moves` (a `cad-moves` result) into the design: see the module. Returns
/// what was written; [`BridgeError::Mismatch`] (the design restored) when the read-back
/// differs from what was asked beyond `tolerance`.
pub fn write(
    connection: &mut Connection,
    cad_moves: &Value,
    tolerance: Tolerance,
) -> Result<Value, BridgeError> {
    crate::contract::check_writing(connection)?;
    let before = read::snapshot(connection)?;
    let mut commands: Vec<Value> = Vec::new();
    let mut parameters = serde_json::Map::new();
    let mut checks: Vec<Check> = Vec::new();
    let items = cad_moves["items"]
        .as_array()
        .ok_or_else(|| protocol("`cad-moves` gave no items".into()))?;
    for item in items.iter().filter(|i| i["changed"] == true) {
        let name = item["occurrence"].as_str().unwrap_or_default();
        let occurrence = before
            .occurrences
            .iter()
            .find(|o| name_path(&before, &o.path).as_deref() == Some(name))
            .ok_or_else(|| {
                BridgeError::Refused(format!(
                    "the design open in SolidCraft has no occurrence `{name}`"
                ))
            })?;
        let movement = matrix(&item["matrix"])?;
        checks.push(Check::Placement {
            path: occurrence.path.clone(),
            name: name.to_owned(),
            world: multiply(&movement, &occurrence.world),
        });
        match item["bindings"].as_array() {
            None => commands.push(json!({
                "command": "occurrence.move",
                "params": {
                    "occurrence": occurrence.path.last(),
                    "path": occurrence.path,
                    "space": "world",
                    "matrix": [movement[0], movement[1], movement[2]],
                    "capture": true,
                },
            })),
            Some(bindings) => {
                for b in bindings {
                    let change = b["change"].as_f64().unwrap_or(0.0);
                    if let Some(p) = b["parameter"].as_str() {
                        let now = before
                            .parameters
                            .iter()
                            .find(|q| q.name == p)
                            .and_then(|q| q.value)
                            .ok_or_else(|| {
                                BridgeError::Refused(format!(
                                    "the design has no parameter `{p}` with a value"
                                ))
                            })?;
                        let value = now + change;
                        let unit = b["unit"].as_str().unwrap_or("mm");
                        parameters.insert(p.to_owned(), json!(format!("{value} {unit}")));
                        checks.push(Check::Parameter {
                            name: p.to_owned(),
                            value,
                        });
                    } else if let Some(j) = b["joint"].as_str() {
                        let index = b["index"].as_u64().unwrap_or(0) as usize;
                        let joint =
                            before.joints.iter().find(|q| q.name == j).ok_or_else(|| {
                                BridgeError::Refused(format!("the design has no joint `{j}`"))
                            })?;
                        let now = joint.values.get(index).map(|v| v.value).ok_or_else(|| {
                            BridgeError::Refused(format!("joint `{j}` has no value {index}"))
                        })?;
                        let value = now + change;
                        commands.push(json!({
                            "command": "joint.drive",
                            "params": { "joint": joint.id, "index": index, "value": value, "quiet": true },
                        }));
                        checks.push(Check::Joint {
                            id: joint.id,
                            name: j.to_owned(),
                            index,
                            value,
                        });
                    }
                }
            }
        }
    }
    if !parameters.is_empty() {
        commands.insert(
            0,
            json!({ "command": "parameters.change", "params": { "changes": parameters } }),
        );
    }
    if commands.is_empty() {
        return Ok(json!({ "written": 0, "revision": before.revision.revision }));
    }

    let checkpoint = connection.execute("edit.checkpoint", json!({}))?["checkpoint"].clone();
    let restore = |connection: &mut Connection, forget: bool| {
        connection.execute(
            "edit.restore_checkpoint",
            json!({ "checkpoint": checkpoint, "forget": forget }),
        )
    };
    let batch = connection.call(
        "engine.batch",
        json!({ "commands": commands, "rollback": true, "coalesce_parameters": true }),
    );
    let failed = match &batch {
        Ok(reply) if reply.get("failed").is_some_and(|f| !f.is_null()) => {
            let f = &reply["failed"];
            Some(format!(
                "`{}` failed: {}",
                f["command"].as_str().unwrap_or("?"),
                f["error"].as_str().unwrap_or("no reason given")
            ))
        }
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
    if let Some(why) = failed {
        restore(connection, false)?;
        return Err(BridgeError::Refused(format!(
            "{why}; the design is as it was"
        )));
    }

    let after = read::snapshot(connection)?;
    let mut differences = Vec::new();
    for check in &checks {
        match check {
            Check::Parameter { name, value } => {
                let got = after
                    .parameters
                    .iter()
                    .find(|p| &p.name == name)
                    .and_then(|p| p.value);
                if got.is_none_or(|g| (g - value).abs() > tolerance.mm) {
                    differences.push(format!("parameter `{name}` is {got:?}, not {value}"));
                }
            }
            Check::Joint {
                id,
                name,
                index,
                value,
            } => {
                let got = after
                    .joints
                    .iter()
                    .find(|j| j.id == *id)
                    .and_then(|j| j.values.get(*index))
                    .map(|v| v.value);
                if got.is_none_or(|g| (g - value).abs() > tolerance.joint) {
                    differences.push(format!(
                        "joint `{name}` is at {got:?}, not {value} (its limits, or what it holds)"
                    ));
                }
            }
            Check::Placement { path, name, world } => {
                let Some(got) = after.occurrences.iter().find(|o| &o.path == path) else {
                    differences.push(format!("`{name}` is gone"));
                    continue;
                };
                let shift = (0..3)
                    .map(|i| (got.world[i][3] - world[i][3]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                let turn = (0..3)
                    .flat_map(|i| (0..3).map(move |j| (i, j)))
                    .map(|(i, j)| (got.world[i][j] - world[i][j]).abs())
                    .fold(0.0, f64::max);
                if shift > tolerance.mm || turn > tolerance.rotation {
                    differences.push(format!(
                        "`{name}` is {shift:.3e} mm and {turn:.1e} (rotation) from where its move puts it"
                    ));
                }
            }
        }
    }
    if !differences.is_empty() {
        restore(connection, false)?;
        return Err(BridgeError::Mismatch(differences));
    }
    restore(connection, true)?;
    Ok(json!({
        "written": commands.len(),
        "revision": after.revision.revision,
        "parameters": parameters,
        "checked": checks.len(),
    }))
}
