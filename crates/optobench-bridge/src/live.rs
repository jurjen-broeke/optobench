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

    /// Reads the design, exports it and traces the sidecar against the export; the
    /// seconds from `since` (the change noticed) to the trace done.
    fn trace_now(&mut self, since: Instant) -> Result<Value, CommandError> {
        let snapshot = read::snapshot(&mut self.link).map_err(failed)?;
        let name = format!("design-{}", snapshot.revision.revision);
        let step = read::export_step(&mut self.link, &self.folder, &name).map_err(failed)?;
        let mut parameters = Parameters::new();
        for key in ["path", "data_directory", "rays_drawn", "ray_paths"] {
            if let Some(v) = self.parameters.get(key) {
                parameters.insert(key.into(), v.clone());
            }
        }
        parameters.insert("step".into(), json!(step.to_string_lossy()));
        let traced = Registry::standard().run("trace", &Value::Object(parameters));
        // The folder keeps the last two exports.
        self.exports.push(step.clone());
        while self.exports.len() > 2 {
            let _ = std::fs::remove_file(self.exports.remove(0));
        }
        let traced = traced?;
        self.traced = snapshot.revision.clone();
        Ok(json!({
            "revision": snapshot.revision.revision,
            "document_id": snapshot.revision.document_id,
            "seconds": since.elapsed().as_secs_f64(),
            // The export traced, kept until two more have come.
            "step": step.to_string_lossy(),
            "design": snapshot_json(&snapshot),
            "trace": traced,
        }))
    }
}
