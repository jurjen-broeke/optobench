//! The live link (M12, read only): a sidecar traced against the design open in
//! SolidCraft, again after each change to it.
//!
//! A [`Live`] session finds the running app (`optobench_bridge::discovery`), checks its
//! contract, reads the design (occurrences with their placements, joints, parameters) and
//! subscribes to its changes. Each trace exports the whole design as STEP into the
//! session's own scratch folder (the only folder SolidCraft may write in) and traces the
//! sidecar against that export (`trace`'s `step`), so the user's own STEP file is never
//! touched. `optobench live` and the window's Live switch keep a session going.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use optobench_commands::{CommandError, ErrorCode, Registry};
use serde_json::{Map, Value, json};

use crate::connection::Connection;
use crate::read::{self, Revision, Snapshot};
use crate::{BridgeError, contract, discovery};

/// The parameters of a session: a JSON object.
pub type Parameters = Map<String, Value>;

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidParameters, message)
}

/// How long to wait for SolidCraft to accept a connection.
const CONNECT: Duration = Duration::from_secs(2);

/// How long the design must stay still before it is traced: SolidCraft sends up to ten
/// changes a second while something is dragged.
const SETTLE: Duration = Duration::from_millis(150);

/// The parameters a session takes.
pub const KEYS: &[&str] = &[
    "path",
    "data_directory",
    "control_directory",
    "rays_drawn",
    "ray_paths",
];

fn failed(error: BridgeError) -> CommandError {
    CommandError::new(ErrorCode::Failed, error.to_string())
}

/// A live session.
#[derive(Debug)]
pub struct Live {
    link: Connection,
    folder: PathBuf,
    parameters: Parameters,
    /// The revision traced last.
    pub traced: Revision,
    /// The exports still kept: the latest (the one the last update names, which a
    /// window may still be reading) and the one before it.
    exports: Vec<PathBuf>,
    /// The design as the latest export has it: where its occurrences were then, and its
    /// geometry fingerprint.
    exported: Option<Snapshot>,
    /// The moves the last trace applied to that export.
    moved: Vec<Value>,
}

/// The first place two JSON values differ, as a path (`timeline[3].name`), with both
/// values shortened; `None` when they are equal.
fn first_difference(a: &Value, b: &Value, at: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys()) {
                let (p, q) = (
                    x.get(k).unwrap_or(&Value::Null),
                    y.get(k).unwrap_or(&Value::Null),
                );
                if let Some(d) = first_difference(p, q, &format!("{at}.{k}")) {
                    return Some(d);
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x
            .iter()
            .zip(y)
            .enumerate()
            .find_map(|(i, (p, q))| first_difference(p, q, &format!("{at}[{i}]"))),
        _ if a == b => None,
        _ => {
            let short = |v: &Value| v.to_string().chars().take(80).collect::<String>();
            Some(format!(
                "{}: {} → {}",
                at.trim_start_matches('.'),
                short(a),
                short(b)
            ))
        }
    }
}

/// The moves that take the occurrences of `then` to where `now` has them: for each
/// occurrence whose world placement changed, its path of names (joined by `/`, as a STEP
/// export names them) and the move, `now · then⁻¹`. `None` when an occurrence of `now`
/// was not in `then` (a part added: export again).
fn moves_between(then: &Snapshot, now: &Snapshot) -> Option<Vec<Value>> {
    let name = |path: &[u64]| -> Option<String> {
        let names: Option<Vec<&str>> = (1..=path.len())
            .map(|k| {
                now.occurrences
                    .iter()
                    .find(|o| o.path == path[..k])
                    .map(|o| o.name.as_str())
            })
            .collect();
        names.map(|n| n.join("/"))
    };
    let mut moves = Vec::new();
    for occurrence in &now.occurrences {
        let before = then
            .occurrences
            .iter()
            .find(|o| o.path == occurrence.path)?;
        let delta = read::multiply(&occurrence.world, &read::inverse(&before.world));
        let still =
            (0..4).all(|i| (0..4).all(|j| (delta[i][j] - read::IDENTITY[i][j]).abs() < 1e-12));
        if !still {
            moves.push(json!({ "occurrence": name(&occurrence.path)?, "matrix": delta }));
        }
    }
    Some(moves)
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

/// The snapshot as JSON.
fn snapshot_json(snapshot: &Snapshot) -> Value {
    json!({
        "document_id": snapshot.revision.document_id,
        "revision": snapshot.revision.revision,
        "occurrences": snapshot.occurrences.iter().map(|o| json!({
            "path": o.path, "name": o.name, "component": o.component,
            "grounded": o.grounded, "world": o.world,
        })).collect::<Vec<_>>(),
        "joints": snapshot.joints.iter().map(|j| json!({
            "id": j.id, "name": j.name, "type": j.kind, "ends": [j.ends.0, j.ends.1],
            "suppressed": j.suppressed,
            "values": j.values.iter().map(|v| json!({
                "name": v.name, "value": v.value, "unit": v.unit,
                "limits": v.limits.map(|(a, b)| json!([a, b])),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "parameters": snapshot.parameters.iter().map(|p| json!({
            "name": p.name, "expression": p.expression, "value": p.value,
            "unit": p.unit, "source": p.source,
        })).collect::<Vec<_>>(),
    })
}

impl Live {
    /// Connects, checks the contract, subscribes, reads the design and traces it once;
    /// returns the session and that first result. Parameters: `path` (the sidecar),
    /// optionally `data_directory`, `control_directory` (SolidCraft's control folder,
    /// else its usual place) and `rays_drawn`.
    pub fn start(parameters: &Parameters) -> Result<(Self, Value), CommandError> {
        if let Some(key) = parameters.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(invalid(format!(
                "unknown parameter `{key}`; the live link takes {}",
                KEYS.join(", ")
            )));
        }
        let text = |key: &str| -> Result<Option<&str>, CommandError> {
            match parameters.get(key) {
                None => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.as_str())),
                Some(_) => Err(invalid(format!("`{key}` must be a string"))),
            }
        };
        let path = text("path")?.ok_or_else(|| invalid("missing parameter `path`"))?;
        let source = std::fs::read_to_string(path)
            .map_err(|e| CommandError::new(ErrorCode::Failed, format!("{path}: {e}")))?;
        if optobench_io::sidecar::kind_of(&source).as_deref() != Some("sidecar") {
            return Err(CommandError::new(
                ErrorCode::InvalidParameters,
                format!(
                    "{path}: the live link traces a sidecar (kind = \"sidecar\"), the optics of a CAD design"
                ),
            ));
        }
        let endpoint = match text("control_directory")? {
            Some(dir) => discovery::find_in(std::path::Path::new(dir), CONNECT),
            None => discovery::find(CONNECT),
        }
        .map_err(failed)?;
        let mut link = Connection::open(&endpoint, CONNECT).map_err(failed)?;
        contract::check(&mut link).map_err(failed)?;
        let subscribed = read::subscribe(&mut link).map_err(failed)?;
        static SESSIONS: AtomicU64 = AtomicU64::new(0);
        let folder = std::env::temp_dir().join(format!(
            "optobench-live-{}-{}",
            std::process::id(),
            SESSIONS.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&folder).map_err(|e| {
            CommandError::new(ErrorCode::Failed, format!("{}: {e}", folder.display()))
        })?;
        let mut live = Self {
            link,
            folder,
            parameters: parameters.clone(),
            traced: subscribed,
            exports: Vec::new(),
            exported: None,
            moved: Vec::new(),
        };
        let first = live.trace_now(Instant::now())?;
        Ok((live, first))
    }

    /// Waits up to `timeout` for the design to change and traces it again; `None` when
    /// it did not change.
    pub fn next(&mut self, timeout: Duration) -> Result<Option<Value>, CommandError> {
        let Some(changed) =
            read::wait_for_change(&mut self.link, timeout, SETTLE).map_err(failed)?
        else {
            return Ok(None);
        };
        let noticed = Instant::now();
        if changed == self.traced {
            return Ok(None);
        }
        self.trace_now(noticed).map(Some)
    }

    /// Reads the design and traces the sidecar against it; the seconds from `since` (the
    /// change noticed) to the trace done. When only placements changed since the latest
    /// export (the same geometry fingerprint), that export is traced with its occurrences
    /// moved where SolidCraft has them now; otherwise the design is exported again.
    fn trace_now(&mut self, since: Instant) -> Result<Value, CommandError> {
        let snapshot = read::snapshot(&mut self.link).map_err(failed)?;
        let fast = self
            .exported
            .as_ref()
            .filter(|then| then.geometry == snapshot.geometry)
            .and_then(|then| moves_between(then, &snapshot))
            .zip(self.exports.last().cloned());
        // Why it is exported again, in words.
        let reason = match &self.exported {
            None => "the first trace".to_owned(),
            Some(then) => first_difference(&then.geometry, &snapshot.geometry, "")
                .map(|d| format!("the geometry changed: {d}"))
                .unwrap_or_else(|| "an occurrence was added".to_owned()),
        };
        let (step, moves, geometry) = match fast {
            Some((moves, step)) => (step, moves, "moved"),
            None => {
                let name = format!("design-{}", snapshot.revision.revision);
                let step =
                    read::export_step(&mut self.link, &self.folder, &name).map_err(failed)?;
                // The folder keeps the last two exports.
                self.exports.push(step.clone());
                while self.exports.len() > 2 {
                    let _ = std::fs::remove_file(self.exports.remove(0));
                }
                self.exported = Some(snapshot.clone());
                (step, Vec::new(), "exported")
            }
        };
        let mut parameters = Parameters::new();
        for key in ["path", "data_directory", "rays_drawn", "ray_paths"] {
            if let Some(v) = self.parameters.get(key) {
                parameters.insert(key.into(), v.clone());
            }
        }
        parameters.insert("step".into(), json!(step.to_string_lossy()));
        if !moves.is_empty() {
            parameters.insert("moves".into(), json!(moves));
        }
        let traced = Registry::standard().run("trace", &Value::Object(parameters))?;
        self.traced = snapshot.revision.clone();
        self.moved = moves.clone();
        Ok(json!({
            "revision": snapshot.revision.revision,
            "document_id": snapshot.revision.document_id,
            "seconds": since.elapsed().as_secs_f64(),
            // `exported`: the design exported again; `moved`: only placements changed, so
            // the latest export traced with its occurrences moved (`moves`).
            "geometry": geometry,
            "reason": if geometry == "moved" { Value::Null } else { json!(reason) },
            // The export traced, kept until two more have come, and the moves applied.
            "step": step.to_string_lossy(),
            "moves": moves,
            "design": snapshot_json(&snapshot),
            "trace": traced,
        }))
    }

    /// Makes DOF values (`{ "<name>": value }`, mm or degrees; others keep the file's) in
    /// the design open in SolidCraft (M13): `cad-moves` against the design as the last
    /// trace had it, where every DOF is 0, written in one batch and read back
    /// (`write::write`; taken back whole when anything differs). Then the sidecar: the
    /// CAD parts' DOFs back to 0, since the design now has their moves, and the DOFs of
    /// sources and detectors, which are not CAD parts, set to the values asked. The
    /// design's change is traced by the next [`Live::next`].
    pub fn write(&mut self, dofs: &Value) -> Result<Value, CommandError> {
        let mut parameters = Parameters::new();
        for key in ["path", "data_directory"] {
            if let Some(v) = self.parameters.get(key) {
                parameters.insert(key.into(), v.clone());
            }
        }
        if let Some(step) = self.exports.last() {
            parameters.insert("step".into(), json!(step.to_string_lossy()));
        }
        if !self.moved.is_empty() {
            parameters.insert("moves".into(), json!(self.moved));
        }
        parameters.insert("dofs".into(), dofs.clone());
        let plan = Registry::standard().run("cad-moves", &Value::Object(parameters))?;
        let written =
            crate::write::write(&mut self.link, &plan, crate::write::Tolerance::default())
                .map_err(failed)?;
        // The sidecar: what the design now has goes back to 0; the rest is set there.
        let mut operations = Vec::new();
        let current = |name: &str| plan["current"][name].as_f64().unwrap_or(0.0);
        for item in plan["items"].as_array().into_iter().flatten() {
            for name in item["dofs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if current(name) != 0.0 {
                    operations
                        .push(json!({ "op": "set", "dof": name, "key": "value", "value": 0 }));
                }
            }
        }
        for item in plan["skipped"].as_array().into_iter().flatten() {
            if item["changed"] != true {
                continue;
            }
            for name in item["dofs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                operations.push(json!({
                    "op": "set", "dof": name, "key": "value", "value": plan["values"][name],
                }));
            }
        }
        if !operations.is_empty() {
            Registry::standard().run(
                "edit",
                &json!({ "path": self.parameters["path"], "operations": operations }),
            )?;
        }
        Ok(json!({ "written": written, "sidecar": operations, "plan": plan }))
    }
}
