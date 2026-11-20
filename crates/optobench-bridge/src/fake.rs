//! A stand-in for SolidCraft's control channel, for tests: CI has no SolidCraft. It
//! speaks the protocol as `docs/control-protocol.md` describes it (JSON lines, the token
//! on a connection's first request, replies by id, `design.changed` events after
//! `control.subscribe`) and answers the commands M12 reads with the shapes SolidCraft
//! 0.45.0's schemas give, `engine.describe` among the engine commands (through
//! `engine.execute`) as in the app; an unknown method is refused. Its "design" is a STEP file the test names; `file.export_step`
//! copies it where it is asked to, inside the request's `allow_paths`.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

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
    /// The design's placements, as the golden cage relay has its occurrences: name and
    /// translation (mm) in the design.
    placements: Vec<(String, [f64; 3])>,
    /// Features added since the start: a change of geometry, which a move is not.
    features: u64,
}

/// A stand-in app listening on 127.0.0.1.
#[derive(Debug, Clone)]
pub struct FakeApp {
    /// Its port.
    pub port: u16,
    token: String,
    state: Arc<Mutex<State>>,
}

impl FakeApp {
    /// Starts listening; the design is the STEP file `step`.
    pub fn start(step: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let port = listener.local_addr().expect("an address").port();
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
                placements: [
                    "pc-lens:1",
                    "pc-lens:2",
                    "cage-plate:1",
                    "cage-plate:2",
                    "rod:1",
                    "rod:2",
                    "rod:3",
                    "rod:4",
                ]
                .iter()
                .map(|n| ((*n).to_owned(), [0.0; 3]))
                .collect(),
                features: 0,
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

    /// The design changes to `step`: a new revision, announced to every subscriber.
    /// A change of geometry (a feature added), after which the design exports as `step`.
    pub fn change(&self, step: &Path) {
        let mut state = self.state.lock().expect("state");
        state.step = step.to_path_buf();
        state.features += 1;
        Self::announce(&mut state);
    }

    /// An occurrence moved by `translate` (mm, in the design), as Move or a joint does:
    /// its placement changes, its geometry not; an export would now write `step`.
    pub fn move_occurrence(&self, name: &str, translate: [f64; 3], step: &Path) {
        let mut state = self.state.lock().expect("state");
        state.step = step.to_path_buf();
        for (n, t) in &mut state.placements {
            if n == name {
                for k in 0..3 {
                    t[k] += translate[k];
                }
            }
        }
        Self::announce(&mut state);
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

    /// The requests served so far (`method` or `engine.execute command`).
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
                let inner = &params["params"];
                match command {
                    "engine.describe" => {
                        let described = inner["command"].as_str().unwrap_or_default();
                        let mut schema =
                            schema(described).ok_or(format!("unknown command `{described}`"))?;
                        for (c, p) in &state.removed {
                            if c == described {
                                remove_property(&mut schema, p);
                            }
                        }
                        Ok(schema)
                    }
                    "document.revision" => {
                        Ok(json!({ "document_id": state.document_id, "revision": state.revision }))
                    }
                    "component.list" => Ok(design_tree(&state.placements)),
                    "document.inspect" => Ok(json!({
                        "bodies": state.placements.iter().map(|(n, _)| json!({ "name": n, "feature": 1, "visible": true })).collect::<Vec<_>>(),
                        "timeline": (0..=state.features).map(|k| json!({ "id": k + 1, "name": format!("feature {k}"), "type": "BaseFeature" })).collect::<Vec<_>>(),
                    })),
                    "joint.list" => Ok(joints()),
                    "parameters.list" => Ok(json!({ "parameters": [
                        { "name": "relay_length", "expression": "150 mm", "unit": "mm", "value": 150.0,
                          "display_value": 150.0, "error": null, "comment": "", "source": "user",
                          "favorite": false, "uses": [], "used_by": [] } ], "count": 1, "cycles": [] })),
                    "file.export_step" => {
                        let path =
                            PathBuf::from(inner["path"].as_str().ok_or("`path` is required")?);
                        let allowed: Vec<PathBuf> = request["allow_paths"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|p| p.as_str().map(PathBuf::from))
                            .collect();
                        if !allowed.is_empty() && !allowed.iter().any(|a| path.starts_with(a)) {
                            return Err(format!(
                                "{} is outside the allowed folders",
                                path.display()
                            ));
                        }
                        let bytes = std::fs::copy(&state.step, &path).map_err(|e| e.to_string())?;
                        Ok(
                            json!({ "path": path.to_string_lossy(), "bytes": bytes, "format": "Step" }),
                        )
                    }
                    other => Err(format!("unknown command `{other}`")),
                }
            }
            other => Err(format!("unknown method `{other}`")),
        }
    }
}

/// The design: one component per placement, placed in the design (component 0).
fn design_tree(placements: &[(String, [f64; 3])]) -> Value {
    let mut components = vec![
        json!({ "id": 0, "name": "relay", "parent": null, "bodies": [],
        "features": [], "sketches": [], "construction": [], "occurrences": [],
        "children": placements.iter().enumerate().map(|(k, (name, t))| json!({
            "id": k + 1, "component": k + 1, "parent": 0, "name": name, "grounded": k == 0,
            // As SolidCraft writes placements: the columns, the translation last.
            "transform": [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [t[0], t[1], t[2], 1]],
        })).collect::<Vec<_>>() }),
    ];
    for (k, (name, _)) in placements.iter().enumerate() {
        components.push(
            json!({ "id": k + 1, "name": name.split(':').next(), "parent": 0,
            "bodies": [name], "features": [], "sketches": [], "construction": [],
            "occurrences": [], "children": [] }),
        );
    }
    json!({ "active": 0, "active_path": [], "pending_moves": [], "components": components })
}

fn joints() -> Value {
    json!({ "conflicts": [], "errors": [], "links": [], "origins": [], "floating": [], "joints": [
        { "id": 1, "name": "L2 slide", "type": "slider", "a": 4, "b": 2, "flip": false,
          "offset": 0.0, "offset_xy": [0.0, 0.0], "suppressed": false, "visible": true,
          "frame_a": null, "frame_b": null,
          "values": [ { "name": "slide", "value": 50.0, "unit": "mm", "limits": [40.0, 60.0] } ] } ] })
}

/// The schemas SolidCraft 0.45.0 gives for the commands M12 reads, cut down to what they
/// say of the fields optobench reads.
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
