//! The open design: its path, the scene as drawn (`view`), the last trace (`trace`), and
//! commands running in the background. Everything goes through the command registry, as
//! for the command line; the interface never reaches into the engine.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use serde_json::{Value, json};

use crate::camera::V3;

/// One drawable item of the scene.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// `element`, `solid`, `proxy` or `detector`.
    pub kind: String,
    /// Its id.
    pub id: String,
    /// For a CAD solid, its occurrence.
    pub occurrence: Option<String>,
    /// `lens`, `mirror`, `absorber` or `detector`.
    pub role: String,
    /// Polylines in the scene frame (mm).
    pub lines: Vec<Vec<V3>>,
    /// Triangles for the shaded view, if `view` gave them.
    pub mesh: Option<Mesh>,
    /// For a CAD body, its own x, y and z axes in the scene frame.
    pub body_axes: Option<[V3; 3]>,
}

/// Where a line of sight met an item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// Distance along the line (mm).
    pub distance: f64,
    /// The unit normal of the triangle met, facing the viewer.
    pub normal: V3,
}

/// An item's triangles, ready for the GPU.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    /// Vertex positions (mm, scene frame).
    pub positions: Vec<[f32; 3]>,
    /// Unit vertex normals.
    pub normals: Vec<[f32; 3]>,
    /// Three vertex indices per triangle.
    pub triangles: Vec<u32>,
    /// The bounding box (low, high).
    pub bounds: ([f32; 3], [f32; 3]),
}

impl Mesh {
    fn from_json(value: &Value) -> Option<Self> {
        let floats = |key: &str| -> Option<Vec<[f32; 3]>> {
            let list = value[key].as_array()?;
            Some(
                list.chunks_exact(3)
                    .map(|c| c.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32))
                    .map(|mut c| {
                        [
                            c.next().unwrap_or(0.0),
                            c.next().unwrap_or(0.0),
                            c.next().unwrap_or(0.0),
                        ]
                    })
                    .collect(),
            )
        };
        let positions = floats("positions")?;
        let normals = floats("normals")?;
        let count = positions.len() as u64;
        let triangles: Vec<u32> = value["triangles"]
            .as_array()?
            .iter()
            .map(|i| i.as_u64().filter(|i| *i < count).map(|i| i as u32))
            .collect::<Option<_>>()?;
        if normals.len() != positions.len() || triangles.len() % 3 != 0 {
            return None;
        }
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for p in &positions {
            for k in 0..3 {
                low[k] = low[k].min(p[k]);
                high[k] = high[k].max(p[k]);
            }
        }
        Some(Self {
            positions,
            normals,
            triangles,
            bounds: (low, high),
        })
    }

    /// The nearest hit of a ray (origin, unit direction) on a triangle at a point `keep`
    /// accepts (Möller–Trumbore).
    pub fn hit(&self, origin: V3, direction: V3, keep: &dyn Fn(V3) -> bool) -> Option<Hit> {
        let (low, high) = self.bounds;
        // Slab test on the box first.
        let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
        for k in 0..3 {
            let (lo, hi) = (low[k] as f64, high[k] as f64);
            if direction[k].abs() < 1e-300 {
                if origin[k] < lo || origin[k] > hi {
                    return None;
                }
                continue;
            }
            let (a, b) = (
                (lo - origin[k]) / direction[k],
                (hi - origin[k]) / direction[k],
            );
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        if t0 > t1 + 1e-9 {
            return None;
        }
        let at = |i: u32| self.positions[i as usize].map(f64::from);
        let sub = |a: V3, b: V3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let cross = |a: V3, b: V3| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let mut best: Option<Hit> = None;
        for t in self.triangles.chunks_exact(3) {
            let (a, b, c) = (at(t[0]), at(t[1]), at(t[2]));
            let (e1, e2) = (sub(b, a), sub(c, a));
            let p = cross(direction, e2);
            let det = dot(e1, p);
            if det.abs() < 1e-14 {
                continue;
            }
            let s = sub(origin, a);
            let u = dot(s, p) / det;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = cross(s, e1);
            let v = dot(direction, q) / det;
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            let distance = dot(e2, q) / det;
            let point = [0, 1, 2].map(|k| origin[k] + direction[k] * distance);
            if distance > 0.0 && best.is_none_or(|b| distance < b.distance) && keep(point) {
                let n = cross(e1, e2);
                let length = dot(n, n).sqrt().max(1e-300);
                let facing = if dot(n, direction) > 0.0 { -1.0 } else { 1.0 };
                best = Some(Hit {
                    distance,
                    normal: n.map(|c| facing * c / length),
                });
            }
        }
        best
    }
}

/// A source, drawn as an arrow.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    /// Its id.
    pub id: String,
    /// Where it starts.
    pub origin: V3,
    /// Its axis.
    pub axis: V3,
}

/// The scene as `view` returns it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    /// Items, in the order the scene lists them.
    pub items: Vec<Item>,
    /// Sources.
    pub sources: Vec<Source>,
    /// Bounds (low, high), if anything is drawn.
    pub bounds: Option<(V3, V3)>,
    /// The degrees of freedom, as `view` and `trace` report them.
    pub dofs: Vec<Value>,
    /// The file's entries as written (`view`'s `design`), for the editors.
    pub design: Value,
}

/// A traced ray's path.
#[derive(Debug, Clone, PartialEq)]
pub struct RayPath {
    /// Points (mm).
    pub points: Vec<V3>,
    /// How it ended: `detected`, `clipped`, `escaped`, …
    pub end: String,
    /// Its last direction, for a ray that left the scene.
    pub direction: V3,
    /// 0 for a launched ray, more for rays split off.
    pub generation: u64,
}

/// The last trace.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Trace {
    /// The whole report.
    pub report: Value,
    /// The ray paths it carried.
    pub rays: Vec<RayPath>,
}

fn v3(value: &Value) -> Option<V3> {
    let a = value.as_array()?;
    Some([
        a.first()?.as_f64()?,
        a.get(1)?.as_f64()?,
        a.get(2)?.as_f64()?,
    ])
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

impl Scene {
    /// From a `view` result.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let items = value["items"]
            .as_array()
            .ok_or("the view has no items")?
            .iter()
            .map(|item| Item {
                kind: text(item, "kind"),
                id: text(item, "id"),
                occurrence: item["occurrence"].as_str().map(str::to_owned),
                role: text(item, "role"),
                lines: item["lines"]
                    .as_array()
                    .map(|lines| {
                        lines
                            .iter()
                            .filter_map(|l| {
                                l.as_array().map(|ps| ps.iter().filter_map(v3).collect())
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                mesh: item.get("mesh").and_then(Mesh::from_json),
                body_axes: item["body_axes"]
                    .as_array()
                    .and_then(|a| Some([v3(a.first()?)?, v3(a.get(1)?)?, v3(a.get(2)?)?])),
            })
            .collect();
        let sources = value["sources"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|s| {
                        Some(Source {
                            id: text(s, "id"),
                            origin: v3(&s["origin"])?,
                            axis: v3(&s["axis"])?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let bounds = value["bounds"]
            .as_array()
            .and_then(|b| Some((v3(b.first()?)?, v3(b.get(1)?)?)));
        Ok(Self {
            items,
            sources,
            bounds,
            dofs: value["dofs"].as_array().cloned().unwrap_or_default(),
            design: value["design"].clone(),
        })
    }

    /// The CAD occurrences of untagged solids (mounts, rods), each with its items'
    /// indices, in first-seen order: the browser groups them.
    pub fn occurrences(&self) -> Vec<(String, Vec<usize>)> {
        let mut out: Vec<(String, Vec<usize>)> = Vec::new();
        for (i, item) in self.items.iter().enumerate() {
            if item.role != "absorber" || item.kind != "solid" {
                continue;
            }
            let name = item.occurrence.clone().unwrap_or_else(|| item.id.clone());
            match out.iter_mut().find(|(n, _)| *n == name) {
                Some((_, list)) => list.push(i),
                None => out.push((name, vec![i])),
            }
        }
        out
    }
}

impl Trace {
    /// From a `trace` result.
    pub fn from_json(value: Value) -> Self {
        let rays = value["ray_paths"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|r| RayPath {
                        points: r["points"]
                            .as_array()
                            .map(|ps| ps.iter().filter_map(v3).collect())
                            .unwrap_or_default(),
                        end: text(r, "end"),
                        direction: v3(&r["direction"]).unwrap_or([0.0; 3]),
                        generation: r["generation"].as_u64().unwrap_or(0),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            report: value,
            rays,
        }
    }
}

/// A command's result arriving from the background.
#[derive(Debug)]
pub struct Finished {
    /// The command.
    pub command: String,
    /// Its result, or the error message.
    pub result: Result<Value, String>,
    /// For `view`, the scene read from the result (on the background thread: meshes are
    /// large).
    pub scene: Option<Result<Scene, String>>,
}

/// A message for the results strip.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// Shown in the error colour.
    pub error: bool,
    /// The text.
    pub text: String,
}

/// One trace, for comparing runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// 1, 2, …
    pub number: usize,
    /// What changed before it, in words (the last edit or optimisation).
    pub after: String,
    /// Detected over launched power.
    pub detected: f64,
    /// Per detector: id, direct centroid (mm), direct RMS radius (mm), direct RMS slope
    /// spread (mrad).
    pub detectors: Vec<(String, [f64; 2], f64, f64)>,
}

/// A function that asks for a repaint.
pub type Repaint = std::sync::Arc<dyn Fn() + Send + Sync>;

/// The open design.
pub struct Document {
    /// Its file.
    pub path: PathBuf,
    /// Where imported materials, parts and curves are (the platform's default if none).
    pub data_directory: Option<PathBuf>,
    /// The scene as drawn.
    pub scene: Option<Scene>,
    /// Counts the scenes taken in, so views know when to upload a new one.
    pub scene_version: u64,
    /// Counts the traces taken in.
    pub trace_version: u64,
    /// The last trace.
    pub trace: Option<Trace>,
    /// The last result of any other command (shown in the results strip).
    pub output: Option<(String, Value)>,
    /// Commands still running.
    pub running: Vec<String>,
    /// Messages, newest last.
    pub messages: Vec<Message>,
    /// The file's earlier texts, newest last, for undoing edits.
    pub undo: Vec<String>,
    /// The traces so far.
    pub runs: Vec<Run>,
    /// The last optimisation's result, until applied or replaced.
    pub optimised: Option<Value>,
    /// The graphs drawn so far, newest first (at most three), with their views.
    pub graphs: Vec<(optobench_analysis::graph::Graph, crate::graphs::GraphView)>,
    /// The graph the Graph button draws (the `graph` command's `of`).
    pub graph_kind: &'static str,
    /// What the next run follows (for the runs strip).
    changed: String,
    undoing: bool,
    repaint: Repaint,
    sender: Sender<Finished>,
    receiver: Receiver<Finished>,
}

/// Rays drawn in the view.
pub const RAYS_DRAWN: u64 = 400;

impl Document {
    /// Opens a design: reads its scene in the background.
    pub fn open(path: PathBuf, data_directory: Option<PathBuf>, repaint: Repaint) -> Self {
        let (sender, receiver) = channel();
        let mut doc = Self {
            path,
            data_directory,
            scene: None,
            scene_version: 0,
            trace_version: 0,
            trace: None,
            output: None,
            graphs: Vec::new(),
            graph_kind: "spot",
            running: Vec::new(),
            messages: Vec::new(),
            undo: Vec::new(),
            runs: Vec::new(),
            optimised: None,
            changed: "opened".into(),
            undoing: false,
            repaint,
            sender,
            receiver,
        };
        doc.start("view", json!({}));
        doc
    }

    /// The file name, for the document tab.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.to_string_lossy().into_owned())
    }

    /// The parameters a command takes from the open design, or why it cannot run from a
    /// button yet.
    pub fn parameters(&self, command: &str) -> Result<Value, String> {
        let mut parameters = self.design_parameters(command)?;
        let uses_data = matches!(
            command,
            "view"
                | "edit"
                | "trace"
                | "optimize"
                | "embed"
                | "export-placements"
                | "parts"
                | "curves"
                | "materials"
                | "graph"
        );
        if let (true, Some(data), Some(p)) =
            (uses_data, &self.data_directory, parameters.as_object_mut())
        {
            p.insert("data_directory".into(), json!(data.to_string_lossy()));
        }
        Ok(parameters)
    }

    fn design_parameters(&self, command: &str) -> Result<Value, String> {
        let path = json!(self.path.to_string_lossy());
        match command {
            "view" => Ok(json!({ "path": path, "max_points": 24, "mesh": true })),
            "trace" => Ok(json!({ "path": path, "ray_paths": RAYS_DRAWN })),
            "optimize" => Ok(json!({ "path": path, "report": true })),
            "bodies" | "embed" | "export-placements" => {
                if command == "bodies" {
                    let step = self.path.with_extension("step");
                    return if step.is_file() {
                        Ok(json!({ "path": step.to_string_lossy() }))
                    } else {
                        Err("needs the STEP file next to the sidecar".into())
                    };
                }
                if command == "export-placements" {
                    let out = self.path.parent().unwrap_or(Path::new("."));
                    return Ok(json!({ "path": path, "out_directory": out.to_string_lossy() }));
                }
                Ok(json!({ "path": path }))
            }
            "check-moves" => {
                let report = moves_report(&self.path);
                if report.is_file() {
                    Ok(json!({ "path": path, "report": report.to_string_lossy() }))
                } else {
                    Err("needs a move report: run Optimise first".into())
                }
            }
            "parts" | "curves" | "materials" => Ok(json!({})),
            "graph" => Ok(json!({ "path": path, "of": self.graph_kind })),
            "edit" => Ok(json!({ "path": path })),
            "import-part" | "import-curve" | "import-material" => {
                Err("drop the file on the window to import it (coming with M8c)".into())
            }
            other => Err(format!("`{other}` has no button yet")),
        }
    }

    /// Runs a command in the background with `extra` parameters added to the design's.
    pub fn start(&mut self, command: &str, extra: Value) {
        let repaint = self.repaint.clone();
        self.run(command, extra, move || repaint());
    }

    /// Changes the file (`edit` operations); the scene is read again when it is written,
    /// and traced again if it was traced. `what` says what changed, for the runs strip.
    pub fn edit(&mut self, operations: Vec<Value>, what: impl Into<String>) {
        if operations.is_empty() {
            return;
        }
        self.changed = what.into();
        self.start("edit", json!({ "operations": operations }));
    }

    /// Puts back the file as it was before the last edit.
    pub fn undo(&mut self) {
        if let Some(text) = self.undo.pop() {
            self.undoing = true;
            self.changed = "undo".into();
            self.start(
                "edit",
                json!({ "operations": [{ "op": "replace", "text": text }] }),
            );
        }
    }

    /// Writes the last optimisation's suggested values into the file's DOFs.
    pub fn apply_optimised(&mut self) {
        let Some(result) = self.optimised.take() else {
            return;
        };
        let operations: Vec<Value> = result["dofs"]
            .as_array()
            .map(|dofs| {
                dofs.iter()
                    .map(|d| json!({ "op": "set", "dof": d["name"], "key": "value", "value": d["suggested"] }))
                    .collect()
            })
            .unwrap_or_default();
        self.edit(operations, "optimised values applied");
    }

    /// Runs a command in the background with `extra` parameters added to the design's;
    /// `repaint` is called when it is done.
    pub fn run(&mut self, command: &str, extra: Value, repaint: impl Fn() + Send + 'static) {
        let parameters = match self.parameters(command) {
            Ok(mut p) => {
                if let (Some(p), Some(e)) = (p.as_object_mut(), extra.as_object()) {
                    p.extend(e.clone());
                }
                p
            }
            Err(why) => {
                self.messages.push(Message {
                    error: true,
                    text: format!("{command}: {why}"),
                });
                return;
            }
        };
        self.running.push(command.to_owned());
        let sender = self.sender.clone();
        let name = command.to_owned();
        std::thread::spawn(move || {
            let result = optobench_commands::Registry::standard()
                .run(&name, &parameters)
                .map_err(|e| e.message);
            let scene = match (&result, name.as_str()) {
                (Ok(value), "view") => Some(Scene::from_json(value)),
                _ => None,
            };
            let _ = sender.send(Finished {
                command: name,
                result,
                scene,
            });
            repaint();
        });
    }

    /// Takes in the results that arrived.
    pub fn poll(&mut self) {
        while let Ok(done) = self.receiver.try_recv() {
            if let Some(i) = self.running.iter().position(|c| *c == done.command) {
                self.running.remove(i);
            }
            self.accept(done);
        }
    }

    /// Takes in one result.
    pub fn accept(&mut self, done: Finished) {
        match (done.command.as_str(), done.result) {
            (_, Err(message)) => self.messages.push(Message {
                error: true,
                text: format!("{}: {message}", done.command),
            }),
            ("view", Ok(value)) => match done.scene.unwrap_or_else(|| Scene::from_json(&value)) {
                Ok(scene) => {
                    self.scene = Some(scene);
                    self.scene_version += 1;
                }
                Err(e) => self.messages.push(Message {
                    error: true,
                    text: e,
                }),
            },
            ("edit", Ok(value)) => {
                if let (false, Some(previous)) = (self.undoing, value["previous"].as_str()) {
                    self.undo.push(previous.to_owned());
                }
                self.undoing = false;
                self.messages.push(Message {
                    error: false,
                    text: format!("Saved: {}", self.changed),
                });
                self.start("view", json!({}));
                if self.trace.is_some() {
                    self.start("trace", json!({}));
                }
            }
            ("optimize", Ok(value)) => {
                let cost = |k: &str| value["cost"][k].as_f64().unwrap_or(f64::NAN);
                self.messages.push(Message {
                    error: false,
                    text: format!(
                        "Optimised: merit {:.3e} to {:.3e}; Apply writes the values",
                        cost("start"),
                        cost("end")
                    ),
                });
                self.output = Some(("optimize".into(), value.clone()));
                self.optimised = Some(value);
            }
            ("graph", Ok(value)) => match optobench_io::graph::from_json(&value["graph"]) {
                Ok(graph) => {
                    self.messages.push(Message {
                        error: false,
                        text: format!("Graph: {}", graph.title),
                    });
                    self.graphs
                        .insert(0, (graph, crate::graphs::GraphView::default()));
                    self.graphs.truncate(3);
                }
                Err(e) => self.messages.push(Message {
                    error: true,
                    text: format!("graph: {e}"),
                }),
            },
            ("trace", Ok(value)) => {
                let detected = value["ledger"]["detected_w"].as_f64().unwrap_or(0.0);
                let launched = value["ledger"]["launched_w"].as_f64().unwrap_or(0.0);
                self.messages.push(Message {
                    error: false,
                    text: format!(
                        "Traced: {:.1} % of {:.3e} W detected",
                        if launched > 0.0 {
                            100.0 * detected / launched
                        } else {
                            0.0
                        },
                        launched
                    ),
                });
                if let Some(scene) = &mut self.scene {
                    scene.dofs = value["dofs"].as_array().cloned().unwrap_or_default();
                }
                self.runs.push(Run {
                    number: self.runs.len() + 1,
                    after: std::mem::take(&mut self.changed),
                    detected: if launched > 0.0 {
                        detected / launched
                    } else {
                        0.0
                    },
                    detectors: value["detectors"]
                        .as_array()
                        .map(|list| {
                            list.iter()
                                .map(|d| {
                                    let n = |k: &str| d[k].as_f64().unwrap_or(f64::NAN);
                                    let c = &d["direct_centroid_mm"];
                                    (
                                        d["id"].as_str().unwrap_or("?").to_owned(),
                                        [
                                            c[0].as_f64().unwrap_or(f64::NAN),
                                            c[1].as_f64().unwrap_or(f64::NAN),
                                        ],
                                        n("direct_rms_radius_mm"),
                                        n("direct_rms_slope_mrad"),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                });
                self.trace = Some(Trace::from_json(value));
                self.trace_version += 1;
            }
            (command, Ok(value)) => {
                self.messages.push(Message {
                    error: false,
                    text: format!("{command}: done"),
                });
                self.output = Some((command.to_owned(), value));
            }
        }
    }
}

/// Where `optimize` writes the move report of a design.
pub fn moves_report(design: &Path) -> PathBuf {
    let stem = design.with_extension("");
    let name = stem
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    stem.with_file_name(format!("{name}.moves.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/optobench-io/tests/step")
    }

    fn run(command: &str, parameters: Value) -> Value {
        optobench_commands::Registry::standard()
            .run(command, &parameters)
            .expect("runs")
    }

    #[test]
    fn the_golden_periscope_reads_into_the_model() {
        let path = fixtures().join("periscope.optobench");
        let empty = std::env::temp_dir().join("optobench-ui-empty");
        let view = run(
            "view",
            json!({ "path": path.to_string_lossy(), "data_directory": empty.to_string_lossy(), "mesh": true }),
        );
        let scene = Scene::from_json(&view).expect("a scene");
        // Every item has triangles, and a line through the middle of a lens meets it.
        assert!(
            scene
                .items
                .iter()
                .all(|i| i.mesh.as_ref().is_some_and(|m| !m.triangles.is_empty()))
        );
        let lens = scene
            .items
            .iter()
            .find(|i| i.id == "L2")
            .and_then(|i| i.mesh.as_ref())
            .expect("L2");
        let (lo, hi) = lens.bounds;
        let centre = [0, 1, 2].map(|k| 0.5 * (lo[k] + hi[k]) as f64);
        let above = [centre[0], centre[1], hi[2] as f64 + 100.0];
        let hit = lens
            .hit(above, [0.0, 0.0, -1.0], &|_| true)
            .expect("meets L2")
            .distance;
        assert!(
            hit > 100.0 - 1e-6 && hit < 100.0 + (hi[2] - lo[2]) as f64 + 1e-6,
            "{hit}"
        );
        assert!(lens.hit(above, [0.0, 0.0, 1.0], &|_| true).is_none());
        let roles: Vec<(&str, &str)> = scene
            .items
            .iter()
            .map(|i| (i.id.as_str(), i.role.as_str()))
            .collect();
        for expected in [
            ("M1", "mirror"),
            ("L1", "lens"),
            ("L2", "lens"),
            ("M2", "mirror"),
            ("screen", "detector"),
        ] {
            assert!(roles.contains(&expected), "{expected:?} in {roles:?}");
        }
        assert_eq!(scene.sources.len(), 1);
        assert_eq!(scene.dofs.len(), 5);
        let (low, high) = scene.bounds.expect("bounds");
        assert!(low[2] < 50.0 && high[2] > 300.0, "{low:?} {high:?}");
        let trace = Trace::from_json(run(
            "trace",
            json!({ "path": path.to_string_lossy(), "data_directory": empty.to_string_lossy(), "ray_paths": 5 }),
        ));
        assert_eq!(trace.rays.len(), 5);
        assert!(
            trace
                .rays
                .iter()
                .all(|r| r.end == "detected" && r.points.len() == 8)
        );
    }

    /// Waits until the document's background commands are done.
    fn settle(doc: &mut Document) {
        for _ in 0..600 {
            doc.poll();
            if doc.running.is_empty() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("commands still running: {:?}", doc.running);
    }

    #[test]
    fn an_edit_is_written_read_again_and_undone() {
        let dir = std::env::temp_dir().join(format!("optobench-ui-edit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        let path = dir.join("periscope.optobench");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/table-periscope.optobench"),
            &path,
        )
        .expect("copied");
        let original = std::fs::read_to_string(&path).expect("read");
        let mut doc = Document::open(path.clone(), Some(dir.clone()), std::sync::Arc::new(|| {}));
        settle(&mut doc);
        let version = doc.scene_version;
        doc.edit(
            vec![json!({ "op": "set", "table": "element", "id": "M2", "key": "height", "value": 160.0 })],
            "M2 height",
        );
        settle(&mut doc);
        assert!(
            std::fs::read_to_string(&path)
                .expect("read")
                .contains("height = 160.0")
        );
        assert!(doc.scene_version > version, "the scene was read again");
        let m2 = doc.scene.as_ref().expect("scene").design["element"][1].clone();
        assert_eq!(m2["height"], 160.0);
        assert_eq!(doc.undo.len(), 1);
        // A refused edit leaves the file and says why.
        doc.edit(
            vec![json!({ "op": "set", "table": "element", "id": "M2", "key": "kind", "value": "banana" })],
            "bad",
        );
        settle(&mut doc);
        assert!(doc.messages.last().expect("a message").error);
        assert_eq!(doc.undo.len(), 1);
        doc.undo();
        settle(&mut doc);
        assert_eq!(std::fs::read_to_string(&path).expect("read"), original);
        assert!(doc.undo.is_empty());
    }

    #[test]
    fn a_cad_body_is_untagged_and_tagged_again_as_a_mirror() {
        let dir = std::env::temp_dir().join(format!("optobench-ui-tag-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        for name in ["periscope.optobench", "periscope.step"] {
            std::fs::copy(fixtures().join(name), dir.join(name)).expect("copied");
        }
        let path = dir.join("periscope.optobench");
        let mut doc = Document::open(path.clone(), Some(dir.clone()), std::sync::Arc::new(|| {}));
        settle(&mut doc);
        doc.edit(
            vec![json!({ "op": "remove", "table": "element", "id": "M2" })],
            "untag",
        );
        settle(&mut doc);
        doc.start("trace", json!({}));
        settle(&mut doc);
        let detected = |doc: &Document| {
            let r = &doc.trace.as_ref().expect("traced").report["ledger"];
            r["detected_w"].as_f64().unwrap_or(0.0) / r["launched_w"].as_f64().unwrap_or(1.0)
        };
        assert!(
            detected(&doc) < 0.5,
            "without M2 the light misses the screen"
        );
        // M2's body is now an untagged body; click its reflecting face (its own +z).
        let scene = doc.scene.as_ref().expect("scene");
        let body = scene
            .items
            .iter()
            .find(|i| i.occurrence.as_deref() == Some("mirror:2"))
            .expect("the body");
        assert_eq!(body.role, "absorber");
        let axes = body.body_axes.expect("its frame");
        let op =
            crate::editors::tag(&scene.design, &body.id, Some((axes[2], axes))).expect("a tag");
        doc.edit(vec![op], "tag mirror");
        settle(&mut doc);
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("reflect = [0.0, 0.0, 1.0]"), "{text}");
        assert!((detected(&doc) - 1.0).abs() < 1e-9, "{}", detected(&doc));
    }

    #[test]
    fn buttons_say_why_they_cannot_run() {
        let (sender, receiver) = channel();
        let doc = Document {
            path: fixtures().join("periscope.optobench"),
            data_directory: Some(PathBuf::from("/data")),
            scene: None,
            scene_version: 0,
            trace_version: 0,
            trace: None,
            output: None,
            graphs: Vec::new(),
            graph_kind: "spot",
            running: Vec::new(),
            messages: Vec::new(),
            undo: Vec::new(),
            runs: Vec::new(),
            optimised: None,
            changed: String::new(),
            undoing: false,
            repaint: std::sync::Arc::new(|| {}),
            sender,
            receiver,
        };
        let trace = doc.parameters("trace").expect("trace can run");
        assert_eq!(trace["data_directory"], "/data");
        assert!(
            doc.parameters("bodies")
                .expect("periscope.step is next to it")
                .get("data_directory")
                .is_none()
        );
        assert!(
            doc.parameters("check-moves")
                .expect_err("no move report yet")
                .contains("Optimise")
        );
        assert!(doc.parameters("import-part").is_err());
        assert_eq!(
            moves_report(Path::new("/a/b/design.optobench")),
            Path::new("/a/b/design.moves.json")
        );
    }
}
