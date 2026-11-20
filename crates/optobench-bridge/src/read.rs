//! What M12 reads from the design open in SolidCraft: its identity and revision, the
//! occurrences with their placements, the joints, the parameters, and a STEP export of
//! the whole design for tracing. Nothing here changes the design.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::BridgeError;
use crate::connection::Connection;

/// A row-major 4×4 rigid transform (mm), as SolidCraft writes placements.
pub type Matrix = [[f64; 4]; 4];

/// The identity.
pub const IDENTITY: Matrix = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// The inverse of a rigid transform: the rotation transposed, the translation taken back.
pub fn inverse(m: &Matrix) -> Matrix {
    let mut out = IDENTITY;
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = m[j][i];
        }
        out[i][3] = -(0..3).map(|k| m[k][i] * m[k][3]).sum::<f64>();
    }
    out
}

/// `a · b`.
pub fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..4).map(|k| a[i][k] * b[k][j]).sum()))
}

/// Which design is open, and the revision of what it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    /// Opaque; another for every design opened or started.
    pub document_id: String,
    /// One counter for the app's whole run: it moves on every change and never repeats.
    pub revision: u64,
}

/// One placement of a component, reached by `path` from the design.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    /// The occurrence ids from the design down to this one.
    pub path: Vec<u64>,
    /// Its name.
    pub name: String,
    /// The component it places, by id.
    pub component: u64,
    /// Its placement in its parent.
    pub transform: Matrix,
    /// Its placement in the design (world): its parents' placements and its own.
    pub world: Matrix,
    /// Whether it is grounded.
    pub grounded: bool,
}

/// One value of a joint (an angle in degrees, a distance in mm), with its limits.
#[derive(Debug, Clone, PartialEq)]
pub struct JointValue {
    /// Its name (angle, distance, slide, …).
    pub name: String,
    /// Its value in `unit`.
    pub value: f64,
    /// `deg` or `mm`.
    pub unit: String,
    /// Its limits, if any.
    pub limits: Option<(f64, f64)>,
}

/// A joint between two occurrences.
#[derive(Debug, Clone, PartialEq)]
pub struct Joint {
    /// Its id.
    pub id: u64,
    /// Its name.
    pub name: String,
    /// Its type (rigid, revolute, slider, …).
    pub kind: String,
    /// Its two ends, by occurrence id.
    pub ends: (u64, u64),
    /// Its values as solved.
    pub values: Vec<JointValue>,
    /// Whether it is suppressed (places nothing).
    pub suppressed: bool,
}

/// A parameter of the design.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    /// Its name.
    pub name: String,
    /// Its expression as typed.
    pub expression: String,
    /// Its value (mm for a length, degrees for an angle); `None` for text or an error.
    pub value: Option<f64>,
    /// Its unit.
    pub unit: String,
    /// `user`, `sketch` or `feature`.
    pub source: String,
}

/// Everything M12 reads, at one revision.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// The design and its revision when the reading started.
    pub revision: Revision,
    /// Every placement, by path: parents before their children.
    pub occurrences: Vec<Occurrence>,
    /// Every joint.
    pub joints: Vec<Joint>,
    /// Every parameter.
    pub parameters: Vec<Parameter>,
    /// Everything read that a change of geometry changes and a move does not: the
    /// component tree without its placements, the bodies (names, features, visibility),
    /// the timeline and the parameters. Equal in two snapshots, the design differs only in
    /// where its parts are, and a STEP export of the first, its occurrences moved, is the
    /// second (the live link's fast path).
    pub geometry: Value,
}

/// The [`Snapshot::geometry`] of the replies read: `component.list` with every
/// `transform` and the pending moves left out, the parameters' names, expressions and
/// values, and `document.inspect`'s bodies and timeline.
pub fn geometry(components: &Value, parameters: &Value, inspected: &Value) -> Value {
    fn without_placements(value: &Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .filter(|(k, _)| !matches!(k.as_str(), "transform" | "pending_moves"))
                    .map(|(k, v)| (k.clone(), without_placements(v)))
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(without_placements).collect()),
            other => other.clone(),
        }
    }
    json!({
        "components": without_placements(&components["components"]),
        "parameters": parameters["parameters"].as_array().into_iter().flatten()
            .map(|p| json!([p["name"], p["expression"], p["value"]]))
            .collect::<Vec<_>>(),
        "bodies": inspected["bodies"],
        "timeline": inspected["timeline"],
    })
}

fn protocol(what: &str) -> BridgeError {
    BridgeError::Protocol(what.to_owned())
}

fn number(value: &Value, what: &str) -> Result<f64, BridgeError> {
    value
        .as_f64()
        .ok_or_else(|| protocol(&format!("{what} is not a number")))
}

fn id(value: &Value, what: &str) -> Result<u64, BridgeError> {
    value
        .as_u64()
        .ok_or_else(|| protocol(&format!("{what} is not an id")))
}

/// A placement as SolidCraft writes it: four rows of four numbers that are the columns of
/// the matrix (the translation in the last row, `[tx, ty, tz, 1]`, as SolidCraft 0.45.0's
/// `component.list` gives it), turned into the row-major [`Matrix`] this crate uses.
fn matrix(value: &Value) -> Result<Matrix, BridgeError> {
    let rows = value
        .as_array()
        .filter(|r| r.len() == 4)
        .ok_or_else(|| protocol("a transform is not 4 rows"))?;
    let mut m = IDENTITY;
    for (i, row) in rows.iter().enumerate() {
        let row = row
            .as_array()
            .filter(|r| r.len() == 4)
            .ok_or_else(|| protocol("a transform row is not 4 numbers"))?;
        for (j, v) in row.iter().enumerate() {
            m[j][i] = number(v, "a transform entry")?;
        }
    }
    Ok(m)
}

/// The design's identity and revision (`document.revision`).
pub fn revision(connection: &mut Connection) -> Result<Revision, BridgeError> {
    let reply = connection.execute("document.revision", json!({}))?;
    Ok(Revision {
        document_id: reply["document_id"]
            .as_str()
            .ok_or_else(|| protocol("document_id is not text"))?
            .to_owned(),
        revision: id(&reply["revision"], "revision")?,
    })
}

/// The occurrences of a `component.list` reply, each with its world placement, walking
/// the tree from the design (component 0) down; at most 64 deep, as SolidCraft's paths.
pub fn occurrences(list: &Value) -> Result<Vec<Occurrence>, BridgeError> {
    let components = list["components"]
        .as_array()
        .ok_or_else(|| protocol("component.list has no components"))?;
    let find = |component: u64| components.iter().find(|c| c["id"] == json!(component));
    let mut out = Vec::new();
    let mut stack: Vec<(u64, Vec<u64>, Matrix)> = vec![(0, Vec::new(), IDENTITY)];
    while let Some((component, path, world)) = stack.pop() {
        if path.len() > 64 {
            return Err(protocol("occurrences nest more than 64 deep"));
        }
        let Some(node) = find(component) else {
            return Err(protocol(&format!("component {component} is not listed")));
        };
        let children = node["children"]
            .as_array()
            .ok_or_else(|| protocol("a component has no children list"))?;
        for child in children {
            let occurrence = id(&child["id"], "an occurrence id")?;
            let transform = matrix(&child["transform"])?;
            let mut child_path = path.clone();
            child_path.push(occurrence);
            let child_world = multiply(&world, &transform);
            stack.push((
                id(&child["component"], "a placed component")?,
                child_path.clone(),
                child_world,
            ));
            out.push(Occurrence {
                path: child_path,
                name: child["name"].as_str().unwrap_or_default().to_owned(),
                component: id(&child["component"], "a placed component")?,
                transform,
                world: child_world,
                grounded: child["grounded"] == true,
            });
        }
    }
    // By path: parents before their children.
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The joints of a `joint.list` reply.
pub fn joints(list: &Value) -> Result<Vec<Joint>, BridgeError> {
    list["joints"]
        .as_array()
        .ok_or_else(|| protocol("joint.list has no joints"))?
        .iter()
        .map(|j| {
            let values = j["values"]
                .as_array()
                .ok_or_else(|| protocol("a joint has no values"))?
                .iter()
                .map(|v| {
                    Ok(JointValue {
                        name: v["name"].as_str().unwrap_or_default().to_owned(),
                        value: number(&v["value"], "a joint value")?,
                        unit: v["unit"].as_str().unwrap_or_default().to_owned(),
                        limits: v["limits"]
                            .as_array()
                            .and_then(|l| Some((l.first()?.as_f64()?, l.get(1)?.as_f64()?))),
                    })
                })
                .collect::<Result<_, BridgeError>>()?;
            Ok(Joint {
                id: id(&j["id"], "a joint id")?,
                name: j["name"].as_str().unwrap_or_default().to_owned(),
                kind: j["type"].as_str().unwrap_or_default().to_owned(),
                ends: (id(&j["a"], "a joint end")?, id(&j["b"], "a joint end")?),
                values,
                suppressed: j["suppressed"] == true,
            })
        })
        .collect()
}

/// The parameters of a `parameters.list` reply.
pub fn parameters(list: &Value) -> Result<Vec<Parameter>, BridgeError> {
    Ok(list["parameters"]
        .as_array()
        .ok_or_else(|| protocol("parameters.list has no parameters"))?
        .iter()
        .map(|p| Parameter {
            name: p["name"].as_str().unwrap_or_default().to_owned(),
            expression: p["expression"].as_str().unwrap_or_default().to_owned(),
            value: p["value"].as_f64(),
            unit: p["unit"].as_str().unwrap_or_default().to_owned(),
            source: p["source"].as_str().unwrap_or_default().to_owned(),
        })
        .collect())
}

/// Reads the design at its current revision. When it changed while being read (the
/// revision moved), it is read again, at most three times.
pub fn snapshot(connection: &mut Connection) -> Result<Snapshot, BridgeError> {
    for _ in 0..3 {
        let before = revision(connection)?;
        let components = connection.execute("component.list", json!({}))?;
        let joint_list = connection.execute("joint.list", json!({ "quiet": true }))?;
        let parameter_list = connection.execute("parameters.list", json!({}))?;
        // No measurements: on a vendor assembly they take half a minute.
        let inspected = connection.execute(
            "document.inspect",
            json!({ "measure": false, "timeline": "summary", "bodies": true, "sketch_counts": false }),
        )?;
        if revision(connection)? == before {
            return Ok(Snapshot {
                revision: before,
                occurrences: occurrences(&components)?,
                joints: joints(&joint_list)?,
                parameters: parameters(&parameter_list)?,
                geometry: geometry(&components, &parameter_list, &inspected),
            });
        }
    }
    Err(BridgeError::Busy(
        "the design kept changing while it was read".into(),
    ))
}

/// Exports the whole design as STEP into `folder` (as `<name>.step`), the only folder
/// the request lets SolidCraft write in; returns the file written.
pub fn export_step(
    connection: &mut Connection,
    folder: &Path,
    name: &str,
) -> Result<PathBuf, BridgeError> {
    let path = folder.join(format!("{name}.step"));
    connection.allow_paths(vec![folder.to_path_buf()]);
    let reply = connection.execute(
        "file.export_step",
        json!({ "path": path.to_string_lossy() }),
    );
    connection.allow_paths(Vec::new());
    let reply = reply?;
    let written = reply["path"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| protocol("file.export_step gave no path"))?;
    Ok(written)
}

/// Starts the design's change events on this connection (`control.subscribe`); returns
/// the revision it starts from.
pub fn subscribe(connection: &mut Connection) -> Result<Revision, BridgeError> {
    let reply = connection.call("control.subscribe", json!({}))?;
    Ok(Revision {
        document_id: reply["document_id"]
            .as_str()
            .ok_or_else(|| protocol("document_id is not text"))?
            .to_owned(),
        revision: id(&reply["revision"], "revision")?,
    })
}

/// Waits up to `timeout` for the design to change, then for `settle` without another
/// change (SolidCraft sends at most about ten a second while something is dragged), and
/// returns the last revision named; `None` when nothing changed in time.
pub fn wait_for_change(
    connection: &mut Connection,
    timeout: Duration,
    settle: Duration,
) -> Result<Option<Revision>, BridgeError> {
    let mut last = None;
    let mut wait = timeout;
    while let Some(event) = connection.next_event(wait)? {
        if event.name != "design.changed" {
            continue;
        }
        last = Some(Revision {
            document_id: event.params["document_id"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            revision: event.params["revision"].as_u64().unwrap_or_default(),
        });
        wait = settle;
    }
    Ok(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placements_compose_down_the_tree() {
        // The design places a cage (component 1) at x 100 turned a quarter about z; the
        // cage places a lens (component 2) at its own y 10: the lens stands at
        // (100 − 10, 0, 0) in the world, turned with the cage.
        // As SolidCraft writes placements: the columns of the matrix, translation last.
        let quarter = json!([[0, 1, 0, 0], [-1, 0, 0, 0], [0, 0, 1, 0], [100, 0, 0, 1]]);
        let up = json!([[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 10, 0, 1]]);
        let list = json!({ "components": [
            { "id": 0, "name": "design", "children": [
                { "id": 4, "component": 1, "parent": 0, "name": "cage:1", "transform": quarter, "grounded": true } ] },
            { "id": 1, "name": "cage", "children": [
                { "id": 7, "component": 2, "parent": 1, "name": "lens:1", "transform": up, "grounded": false } ] },
            { "id": 2, "name": "lens", "children": [] } ] });
        let found = occurrences(&list).expect("read");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].path, [4]);
        assert!(found[0].grounded);
        assert_eq!(found[1].path, [4, 7]);
        assert_eq!(found[1].name, "lens:1");
        let w = found[1].world;
        assert_eq!([w[0][3], w[1][3], w[2][3]], [90.0, 0.0, 0.0]);
        assert_eq!([w[0][1], w[1][0]], [-1.0, 1.0]);
        // A component placed that is not listed is a protocol error.
        let broken = json!({ "components": [ { "id": 0, "name": "design", "children": [
            { "id": 4, "component": 9, "parent": 0, "name": "x", "transform": up, "grounded": false } ] } ] });
        assert!(matches!(
            occurrences(&broken),
            Err(BridgeError::Protocol(_))
        ));
    }
}
