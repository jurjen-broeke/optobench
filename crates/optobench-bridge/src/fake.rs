//! A stand-in for SolidCraft's control channel, for tests: CI has no SolidCraft. It
//! speaks the protocol as `docs/control-protocol.md` describes it (JSON lines, the token
//! on a connection's first request, replies by id, `design.changed` events after
//! `control.subscribe`, `engine.batch`) and answers the commands M12 and M13 send with the
//! shapes SolidCraft 0.45.0's schemas give, `engine.describe` among the engine commands
//! (through `engine.execute`) as in the app; an unknown method is refused.
//!
//! Its design: the golden cage relay's occurrences, each with a placement in the design;
//! user parameters and joints, each moving one part along an axis (a parameter by a
//! ratio: a placement given as an expression, as SolidCraft keeps it); its geometry is a
//! STEP file the test names, which `file.export_step` copies where it is asked to, inside
//! the request's `allow_paths`. Placements are written as SolidCraft writes them: the
//! columns of the matrix, the translation last.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::read::{IDENTITY, Matrix, inverse, multiply};

/// A parameter moving a part: `ratio` times its value along `axis`.
#[derive(Debug, Clone)]
struct ParameterLink {
    parameter: String,
    occurrence: usize,
    axis: [f64; 3],
    ratio: f64,
}

/// A slider joint moving one part along `axis` by its value less `zero`.
#[derive(Debug, Clone)]
struct Joint {
    name: String,
    occurrence: usize,
    axis: [f64; 3],
    value: f64,
    zero: f64,
    limits: (f64, f64),
}

/// The design: what a checkpoint keeps.
#[derive(Debug, Clone)]
struct Design {
    /// Name and placement (row-major) before parameters and joints move it.
    placements: Vec<(String, Matrix)>,
    parameters: Vec<(String, f64)>,
    links: Vec<ParameterLink>,
    joints: Vec<Joint>,
    /// Features added since the start: a change of geometry, which a move is not.
    features: u64,
}

impl Design {
    /// The offset parameters and joints give a part.
    fn offset(&self, k: usize) -> [f64; 3] {
        let mut d = [0.0; 3];
        for l in self.links.iter().filter(|l| l.occurrence == k) {
            let v = self
                .parameters
                .iter()
                .find(|(n, _)| *n == l.parameter)
                .map_or(0.0, |p| p.1);
            for i in 0..3 {
                d[i] += l.ratio * v * l.axis[i];
            }
        }
        for j in self.joints.iter().filter(|j| j.occurrence == k) {
            for i in 0..3 {
                d[i] += (j.value - j.zero) * j.axis[i];
            }
        }
        d
    }

    fn shift(d: [f64; 3]) -> Matrix {
        let mut m = IDENTITY;
        for i in 0..3 {
            m[i][3] = d[i];
        }
        m
    }

    /// A part's placement in the design.
    fn world(&self, k: usize) -> Matrix {
        multiply(&Self::shift(self.offset(k)), &self.placements[k].1)
    }
}

/// What the stand-in app holds.
#[derive(Debug)]
struct State {
    document_id: String,
    revision: u64,
    step: PathBuf,
    /// Connections that subscribed, to send events to.
    subscribers: Vec<TcpStream>,
    /// Properties left out of the schemas, to stand for a changed contract:
    /// (command, property).
    removed: Vec<(String, String)>,
    /// Requests served, by method and command.
    served: Vec<String>,
    design: Design,
    checkpoints: Vec<(u64, Design)>,
    next_checkpoint: u64,
}

/// A stand-in app listening on 127.0.0.1.
#[derive(Debug, Clone)]
pub struct FakeApp {
    /// Its port.
    pub port: u16,
    token: String,
    state: Arc<Mutex<State>>,
}

fn number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| {
        // An expression such as "3 mm" or "1.5 deg": its number.
        v.as_str()?.split_whitespace().next()?.parse().ok()
    })
}

impl FakeApp {
    /// Starts listening; the design is the STEP file `step`.
    pub fn start(step: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let port = listener.local_addr().expect("an address").port();
        let names = [
            "pc-lens:1",
            "pc-lens:2",
            "cage-plate:1",
            "cage-plate:2",
            "rod:1",
            "rod:2",
            "rod:3",
            "rod:4",
        ];
        let app = Self {
            port,
            token: format!("{:064x}", u128::from(port) * 0x9e37_79b9_7f4a_7c15),
            state: Arc::new(Mutex::new(State {
                document_id: "design-1".into(),
                revision: 1,
                step: step.to_path_buf(),
                subscribers: Vec::new(),
                removed: Vec::new(),
                served: Vec::new(),
                design: Design {
                    placements: names.iter().map(|n| ((*n).to_owned(), IDENTITY)).collect(),
                    parameters: vec![("relay_length".into(), 150.0)],
                    links: Vec::new(),
                    joints: vec![Joint {
                        name: "L2 slide".into(),
                        occurrence: 1,
                        axis: [0.0, 0.0, 1.0],
                        value: 50.0,
                        zero: 50.0,
                        limits: (40.0, 60.0),
                    }],
                    features: 0,
                },
                checkpoints: Vec::new(),
                next_checkpoint: 1,
            })),
        };
        let serving = app.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let app = serving.clone();
                std::thread::spawn(move || app.serve(stream));
            }
        });
        app
    }

    /// Its token.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Writes a control folder for it: `<port>.token` and `current.port`.
    pub fn write_control_folder(&self, folder: &Path) {
        std::fs::create_dir_all(folder).expect("folder");
        std::fs::write(folder.join(format!("{}.token", self.port)), &self.token).expect("token");
        std::fs::write(folder.join("current.port"), self.port.to_string()).expect("port");
    }

    /// A change of geometry (a feature added), after which the design exports as `step`.
    pub fn change(&self, step: &Path) {
        let mut state = self.state.lock().expect("state");
        state.step = step.to_path_buf();
        state.design.features += 1;
        Self::announce(&mut state);
    }

    /// An occurrence moved by `translate` (mm, in the design), as Move or a joint does:
    /// its placement changes, its geometry not; an export would now write `step`.
    pub fn move_occurrence(&self, name: &str, translate: [f64; 3], step: &Path) {
        let mut state = self.state.lock().expect("state");
        state.step = step.to_path_buf();
        for (n, m) in &mut state.design.placements {
            if n == name {
                for k in 0..3 {
                    m[k][3] += translate[k];
                }
            }
        }
        Self::announce(&mut state);
    }

    /// A user parameter (made at 0 when new) that moves `occurrence` `ratio` times its
    /// value along `axis`, as a placement given as an expression does.
    pub fn bind_parameter(&self, occurrence: &str, parameter: &str, axis: [f64; 3], ratio: f64) {
        let mut state = self.state.lock().expect("state");
        let k = Self::index(&state, occurrence);
        if !state.design.parameters.iter().any(|(n, _)| n == parameter) {
            state.design.parameters.push((parameter.into(), 0.0));
        }
        state.design.links.push(ParameterLink {
            parameter: parameter.into(),
            occurrence: k,
            axis,
            ratio,
        });
    }

    /// A slider joint holding `occurrence`, at value 0, within `limits` (mm).
    pub fn add_joint(&self, name: &str, occurrence: &str, axis: [f64; 3], limits: (f64, f64)) {
        let mut state = self.state.lock().expect("state");
        let k = Self::index(&state, occurrence);
        state.design.joints.push(Joint {
            name: name.into(),
            occurrence: k,
            axis,
            value: 0.0,
            zero: 0.0,
            limits,
        });
    }

    /// An occurrence's placement in the design (row-major).
    pub fn world(&self, occurrence: &str) -> Matrix {
        let state = self.state.lock().expect("state");
        let k = Self::index(&state, occurrence);
        state.design.world(k)
    }

    /// A parameter's value.
    pub fn parameter(&self, name: &str) -> Option<f64> {
        let state = self.state.lock().expect("state");
        state
            .design
            .parameters
            .iter()
            .find(|(n, _)| n == name)
            .map(|p| p.1)
    }

    /// How many checkpoints are kept.
    pub fn checkpoints(&self) -> usize {
        self.state.lock().expect("state").checkpoints.len()
    }

    fn index(state: &State, occurrence: &str) -> usize {
        state
            .design
            .placements
            .iter()
            .position(|(n, _)| n == occurrence)
            .unwrap_or_else(|| panic!("no occurrence {occurrence}"))
    }

    fn announce(state: &mut State) {
        state.revision += 1;
        let line = format!(
            "{}\n",
            json!({ "event": "design.changed",
                    "params": { "document_id": state.document_id, "revision": state.revision } })
        );
        state
            .subscribers
            .retain_mut(|s| s.write_all(line.as_bytes()).is_ok());
    }

    /// Leaves `property` out of `command`'s schemas, as a changed contract would.
    pub fn remove_from_contract(&self, command: &str, property: &str) {
        let mut state = self.state.lock().expect("state");
        state.removed.push((command.into(), property.into()));
    }

    /// The requests served so far (`method` or `engine.execute command`, and each command
    /// of a batch as `engine.batch command`).
    pub fn served(&self) -> Vec<String> {
        self.state.lock().expect("state").served.clone()
    }

    fn serve(&self, stream: TcpStream) {
        let Ok(mut writer) = stream.try_clone() else {
            return;
        };
        let mut authorised = false;
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { return };
            let Ok(request) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let id = request["id"].clone();
            if !authorised {
                if request["token"] != json!(self.token) {
                    let _ = writeln!(
                        writer,
                        "{}",
                        json!({ "id": id, "ok": false, "error": "unauthorized: wrong token" })
                    );
                    return;
                }
                authorised = true;
            }
            let reply = match self.answer(&request, &writer) {
                Ok(result) => json!({ "id": id, "ok": true, "result": result }),
                Err(error) => json!({ "id": id, "ok": false, "error": error }),
            };
            // Written under the lock, so an event never cuts into a reply.
            let held = self.state.lock().expect("state");
            let written = writeln!(writer, "{reply}");
            drop(held);
            if written.is_err() {
                return;
            }
        }
    }

    fn answer(&self, request: &Value, writer: &TcpStream) -> Result<Value, String> {
        let method = request["method"].as_str().unwrap_or_default();
        let params = &request["params"];
        let mut state = self.state.lock().expect("state");
        let served = match method {
            "engine.execute" => format!(
                "engine.execute {}",
                params["command"].as_str().unwrap_or("?")
            ),
            other => other.to_owned(),
        };
        state.served.push(served);
        match method {
            "control.capabilities" => Ok(json!({ "param_check": ["off", "warn", "strict"] })),
            "control.subscribe" => {
                let stream = writer.try_clone().map_err(|e| e.to_string())?;
                state.subscribers.push(stream);
                Ok(json!({ "document_id": state.document_id, "revision": state.revision }))
            }
            "engine.execute" => {
                let command = params["command"].as_str().unwrap_or_default();
                let (result, wrote) = execute(&mut state, command, &params["params"], request)?;
                if wrote {
                    Self::announce(&mut state);
                }
                Ok(result)
            }
            "engine.batch" => {
                let commands = params["commands"].as_array().cloned().unwrap_or_default();
                let rollback = params["rollback"] == true;
                let before = state.design.clone();
                let mut results = Vec::new();
                let mut wrote = false;
                for (index, step) in commands.iter().enumerate() {
                    let command = step["command"].as_str().unwrap_or_default();
                    state.served.push(format!("engine.batch {command}"));
                    match execute(&mut state, command, &step["params"], request) {
                        Ok((result, w)) => {
                            results.push(result);
                            wrote |= w;
                        }
                        Err(error) => {
                            if rollback {
                                state.design = before;
                            }
                            return Ok(json!({
                                "completed": index, "results": results,
                                "undone": if rollback { index } else { 0 },
                                "rolled_back": rollback,
                                "failed": { "index": index, "command": command, "error": error },
                            }));
                        }
                    }
                }
                if wrote {
                    Self::announce(&mut state);
                }
                Ok(json!({ "completed": commands.len(), "results": results, "undone": 0 }))
            }
            other => Err(format!("unknown method `{other}`")),
        }
    }
}

/// Runs one engine command on the stand-in's design; with whether it wrote.
fn execute(
    state: &mut State,
    command: &str,
    inner: &Value,
    request: &Value,
) -> Result<(Value, bool), String> {
    let read = |v: Value| Ok((v, false));
    match command {
        "engine.describe" => {
            let described = inner["command"].as_str().unwrap_or_default();
            let mut schema = schema(described).ok_or(format!("unknown command `{described}`"))?;
            for (c, p) in &state.removed {
                if c == described {
                    remove_property(&mut schema, p);
                }
            }
            read(schema)
        }
        "document.revision" => {
            read(json!({ "document_id": state.document_id, "revision": state.revision }))
        }
        "component.list" => read(design_tree(&state.design)),
        "document.inspect" => read(json!({
            "bodies": state.design.placements.iter().map(|(n, _)| json!({ "name": n, "feature": 1, "visible": true })).collect::<Vec<_>>(),
            "timeline": (0..=state.design.features).map(|k| json!({ "id": k + 1, "name": format!("feature {k}"), "type": "BaseFeature" })).collect::<Vec<_>>(),
        })),
        "joint.list" => read(joints(&state.design)),
        "parameters.list" => read(json!({
            "parameters": state.design.parameters.iter().map(|(n, v)| json!({
                "name": n, "expression": format!("{v} mm"), "unit": "mm", "value": v,
                "display_value": v, "error": null, "comment": "", "source": "user",
                "favorite": false, "uses": [], "used_by": [] })).collect::<Vec<_>>(),
            "count": state.design.parameters.len(), "cycles": [] })),
        "file.export_step" => {
            let path = PathBuf::from(inner["path"].as_str().ok_or("`path` is required")?);
            let allowed: Vec<PathBuf> = request["allow_paths"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p.as_str().map(PathBuf::from))
                .collect();
            if !allowed.is_empty() && !allowed.iter().any(|a| path.starts_with(a)) {
                return Err(format!("{} is outside the allowed folders", path.display()));
            }
            let bytes = std::fs::copy(&state.step, &path).map_err(|e| e.to_string())?;
            read(json!({ "path": path.to_string_lossy(), "bytes": bytes, "format": "Step" }))
        }
        "parameters.change" => {
            let changes = inner["changes"]
                .as_object()
                .ok_or("parameters.change: give `changes`")?;
            let mut changed = serde_json::Map::new();
            let mut design = state.design.clone();
            for (name, expression) in changes {
                let value = number(expression).ok_or(format!("`{name}`: not a value"))?;
                let p = design
                    .parameters
                    .iter_mut()
                    .find(|(n, _)| n == name)
                    .ok_or(format!("no parameter `{name}`"))?;
                p.1 = value;
                changed.insert(name.clone(), json!(value));
            }
            state.design = design;
            Ok((
                json!({ "changed": changed, "errors": [], "recomputed": 1, "restored": 0 }),
                true,
            ))
        }
        "joint.drive" => {
            // By id (from 1, as `joint.list` numbers them) or by name, as SolidCraft takes it.
            let at = match &inner["joint"] {
                Value::Number(n) => n
                    .as_u64()
                    .and_then(|i| usize::try_from(i).ok())
                    .and_then(|i| i.checked_sub(1))
                    .filter(|i| *i < state.design.joints.len()),
                Value::String(name) => state.design.joints.iter().position(|j| &j.name == name),
                _ => return Err("joint.drive: give `joint`".into()),
            }
            .ok_or_else(|| format!("no joint `{}`", inner["joint"]))?;
            let wanted = number(&inner["value"]).ok_or("joint.drive: give `value`")?;
            let joint = &mut state.design.joints[at];
            joint.value = wanted.clamp(joint.limits.0, joint.limits.1);
            let values = json!([{ "name": "distance", "value": joint.value, "unit": "mm",
                                  "limits": [joint.limits.0, joint.limits.1] }]);
            Ok((json!({ "values": values, "moved": true, "held": [] }), true))
        }
        "occurrence.move" => {
            let id = inner["occurrence"]
                .as_u64()
                .ok_or("occurrence.move: give the occurrence's id")?;
            let k = usize::try_from(id)
                .ok()
                .and_then(|i| i.checked_sub(1))
                .filter(|k| *k < state.design.placements.len())
                .ok_or(format!("no occurrence {id}"))?;
            if inner["space"] != "world" {
                return Err("the stand-in moves in world space only".into());
            }
            let rows = inner["matrix"]
                .as_array()
                .ok_or("occurrence.move: give `matrix`")?;
            let mut m = IDENTITY;
            for i in 0..3 {
                for j in 0..4 {
                    m[i][j] =
                        number(&rows[i][j]).ok_or("occurrence.move: `matrix` is 3 rows of 4")?;
                }
            }
            // The part moves from where it is: its placement before parameters and joints
            // becomes shift⁻¹ · m · shift · placement.
            let shift = Design::shift(state.design.offset(k));
            let base = &mut state.design.placements[k].1;
            *base = multiply(&inverse(&shift), &multiply(&m, &multiply(&shift, base)));
            let world = state.design.world(k);
            Ok((
                json!({ "occurrence": id, "pending": false, "transform": columns(&world) }),
                true,
            ))
        }
        "edit.checkpoint" => {
            let id = state.next_checkpoint;
            state.next_checkpoint += 1;
            let design = state.design.clone();
            state.checkpoints.push((id, design));
            read(json!({ "checkpoint": id }))
        }
        "edit.restore_checkpoint" => {
            let id = inner["checkpoint"].as_u64().ok_or("give `checkpoint`")?;
            let at = state
                .checkpoints
                .iter()
                .position(|(c, _)| *c == id)
                .ok_or(format!("no checkpoint {id}"))?;
            if inner["forget"] == true {
                state.checkpoints.remove(at);
                return read(json!({ "checkpoint": id, "restored": false }));
            }
            state.design = state.checkpoints[at].1.clone();
            state.checkpoints.truncate(at);
            Ok((json!({ "checkpoint": id, "restored": true }), true))
        }
        other => Err(format!("unknown command `{other}`")),
    }
}

/// A row-major matrix as SolidCraft writes placements: its columns, the translation last.
fn columns(m: &Matrix) -> Value {
    json!(
        (0..4)
            .map(|j| (0..4).map(|i| m[i][j]).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    )
}

/// The design: one component per placement, placed in the design (component 0).
fn design_tree(design: &Design) -> Value {
    let children: Vec<Value> = design
        .placements
        .iter()
        .enumerate()
        .map(|(k, (name, _))| {
            json!({ "id": k + 1, "component": k + 1, "parent": 0, "name": name,
                    "grounded": k == 0, "transform": columns(&design.world(k)) })
        })
        .collect();
    let mut components = vec![
        json!({ "id": 0, "name": "relay", "parent": null, "bodies": [],
        "features": [], "sketches": [], "construction": [], "occurrences": [],
        "children": children }),
    ];
    for (k, (name, _)) in design.placements.iter().enumerate() {
        components.push(
            json!({ "id": k + 1, "name": name.split(':').next(), "parent": 0,
            "bodies": [name], "features": [], "sketches": [], "construction": [],
            "occurrences": [], "children": [] }),
        );
    }
    json!({ "active": 0, "active_path": [], "pending_moves": [], "components": components })
}

fn joints(design: &Design) -> Value {
    json!({ "conflicts": [], "errors": [], "links": [], "origins": [], "floating": [],
        "joints": design.joints.iter().enumerate().map(|(i, j)| json!({
            "id": i + 1, "name": j.name, "type": "slider", "a": 1, "b": j.occurrence + 1,
            "flip": false, "offset": 0.0, "offset_xy": [0.0, 0.0], "suppressed": false,
            "visible": true, "frame_a": null, "frame_b": null,
            "values": [ { "name": "distance", "value": j.value, "unit": "mm",
                          "limits": [j.limits.0, j.limits.1] } ] })).collect::<Vec<_>>() })
}

/// The schemas SolidCraft 0.45.0 gives for the commands M12 and M13 send, cut down to what
/// they say of the fields optobench uses.
fn schema(command: &str) -> Option<Value> {
    let properties = |names: &[&str]| -> Value {
        Value::Object(names.iter().map(|n| ((*n).to_owned(), json!({}))).collect())
    };
    let (input, output) = match command {
        "document.revision" => (
            json!({}),
            json!({ "properties": properties(&["document_id", "revision"]) }),
        ),
        "component.list" => (
            json!({}),
            json!({ "properties": properties(&["active", "components", "pending_moves"]),
                    "$defs": {
                        "ComponentNode": { "properties": properties(&["id", "name", "parent", "children", "bodies"]) },
                        "PlacementInfo": { "properties": properties(&["id", "component", "parent", "name", "transform", "grounded"]) } } }),
        ),
        "joint.list" => (
            json!({}),
            json!({ "properties": properties(&["joints", "links", "conflicts"]),
                    "$defs": {
                        "ListedJoint": { "properties": properties(&["id", "name", "type", "a", "b", "values", "suppressed"]) },
                        "NamedValue": { "properties": properties(&["name", "value", "unit", "limits"]) } } }),
        ),
        "parameters.list" => (
            json!({}),
            json!({ "properties": properties(&["parameters", "count"]),
                    "$defs": { "ParameterRow": { "properties": properties(&["name", "expression", "value", "unit", "source"]) } } }),
        ),
        "document.inspect" => (
            json!({}),
            json!({ "properties": properties(&["bodies", "timeline", "params", "revision"]) }),
        ),
        "file.export_step" => (
            json!({ "properties": properties(&["path", "bodies"]) }),
            json!({ "properties": properties(&["path", "bytes", "format"]) }),
        ),
        "parameters.change" => (
            json!({ "properties": properties(&["changes", "name", "expression", "unit"]) }),
            json!({ "properties": properties(&["param_warnings"]) }),
        ),
        "joint.drive" => (
            json!({ "properties": properties(&["joint", "value", "values", "index", "quiet"]) }),
            json!({ "properties": properties(&["values", "held", "moved", "requested"]) }),
        ),
        "occurrence.move" => (
            json!({ "properties": properties(&["occurrence", "path", "space", "matrix", "capture", "translate"]) }),
            json!({ "properties": properties(&["occurrence", "transform", "pending", "reverted"]) }),
        ),
        "edit.checkpoint" => (
            json!({ "properties": {} }),
            json!({ "properties": properties(&["checkpoint", "restored"]) }),
        ),
        "edit.restore_checkpoint" => (
            json!({ "properties": properties(&["checkpoint", "forget"]) }),
            json!({ "properties": properties(&["checkpoint", "restored"]) }),
        ),
        _ => return None,
    };
    Some(json!({ "id": command, "input": input, "output": output }))
}

/// Takes `property` out of every `properties` in `schema`.
fn remove_property(schema: &mut Value, property: &str) {
    match schema {
        Value::Object(map) => {
            if let Some(Value::Object(p)) = map.get_mut("properties") {
                p.remove(property);
            }
            for value in map.values_mut() {
                remove_property(value, property);
            }
        }
        Value::Array(items) => {
            for item in items {
                remove_property(item, property);
            }
        }
        _ => {}
    }
}
