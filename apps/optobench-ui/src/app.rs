//! The window: SolidCraft's frame (title bar, workspace tile and ribbon, browser on the
//! left, properties on the right, results along the bottom, the 3D view in the middle)
//! around the workspaces declared in `workspaces`.

use std::path::PathBuf;

use egui::{
    Align, Color32, CornerRadius, Frame, Layout as EguiLayout, Margin, RichText, Sense, Stroke,
    Vec2,
};
use serde_json::{Value, json};

use crate::camera::Camera;
use crate::document::Document;
use crate::editors::{self, New};
use crate::theme::{Accent, DARK};
use crate::viewport::{self, Request};
use crate::workspaces::{Action, Control, Item, Layout, Section, Stage, WORKSPACES};

/// What the file dialog is asked for.
pub(crate) enum Choice {
    /// A design to open, starting in this folder.
    Open { folder: Option<PathBuf> },
    /// Where to save a new design, starting in this folder with this name.
    Save {
        folder: Option<PathBuf>,
        name: String,
    },
    /// A folder (the data directory), starting in this one.
    Folder { folder: Option<PathBuf> },
}

/// Open's shortcut: ⌘O, or Ctrl+O off macOS.
const OPEN: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);

/// Asks where: the system's file dialogs, or in tests a fixed answer.
pub(crate) type Chooser = Box<dyn FnMut(Choice) -> Option<PathBuf>>;

/// The system's file dialogs (`rfd`).
fn system_chooser() -> Chooser {
    Box::new(|choice| {
        let start = |dialog: rfd::FileDialog, folder: Option<PathBuf>| match folder {
            Some(f) if f.is_dir() => dialog.set_directory(f),
            _ => dialog,
        };
        match choice {
            Choice::Open { folder } => start(rfd::FileDialog::new(), folder)
                .set_title("Open a design")
                .add_filter("optobench design", &["optobench"])
                .pick_file(),
            Choice::Save { folder, name } => start(rfd::FileDialog::new(), folder)
                .set_title("Save the new layout as")
                .add_filter("optobench design", &["optobench"])
                .set_file_name(name)
                .save_file(),
            Choice::Folder { folder } => start(rfd::FileDialog::new(), folder)
                .set_title("The data directory")
                .pick_folder(),
        }
    })
}

/// The application.
pub struct App {
    ctx: egui::Context,
    workspace: usize,
    tab: usize,
    stage: usize,
    doc: Option<Document>,
    camera: Camera,
    view: viewport::Extras,
    selected: Option<usize>,
    hidden: Vec<bool>,
    show_rays: bool,
    /// Whether a table layout's breadboard is drawn.
    show_board: bool,
    /// The Move tool, when it is on (its steps are kept while it is off).
    move_on: bool,
    move_tool: crate::moving::MoveTool,
    fit_pending: bool,
    /// Asks where to open or save a design.
    pub(crate) chooser: Chooser,
    /// Where imported materials, parts and curves are; empty for the platform's default.
    data_directory: String,
    confirm: Option<&'static str>,
    /// Values being typed into the editors.
    drafts: editors::Drafts,
    /// An entry chosen in the browser that is not drawn (a source, a merit term):
    /// its table and id, or index for a merit term.
    focus: Option<(String, String)>,
    /// The scene version the drafts belong to.
    drafts_version: u64,
    /// The last import's outcome (error, text), shown in the results strip.
    notice: Option<(bool, String)>,
    /// A material file waiting for its licence notice to be accepted: path, name, notice.
    licence: Option<(PathBuf, String, Value)>,
    /// See `screenshot`.
    shot: Option<Shot>,
    /// The graph cursor of the last frame (its x quantity and value), shown on graphs
    /// whose x measures the same.
    graph_cursor: Option<(optobench_analysis::graph::Unit, f64)>,
    /// The Graph button's chooser is open.
    graph_dialog: bool,
    /// The kinds ticked in it (kept for next time).
    graph_choice: Vec<&'static str>,
    /// New's chooser is open: planar, or a table with this pitch (mm).
    new_dialog: Option<f64>,
    /// A new item waiting for a hole to be clicked, with the library part it is.
    placing: Option<(New, Option<String>)>,
    /// The Lens chooser is open: its search text and the library's lenses.
    lens_chooser: Option<(String, Vec<Value>)>,
    /// A lens's STEP file dropped on the window: its prescription and what is typed to
    /// make it a library part.
    lens_import: Option<LensImport>,
    /// The parts library's mounts, read again after each change to the design.
    library_mounts: Option<Vec<Value>>,
    /// The live link to SolidCraft, while it runs.
    live: Option<crate::live::Link>,
    /// SolidCraft's control folder, when not its usual place (tests).
    control_directory: Option<PathBuf>,
    /// DOF values waiting to be confirmed for writing into SolidCraft.
    confirm_write: Option<Value>,
}

/// A lens's STEP file being made a library part.
struct LensImport {
    path: PathBuf,
    found: Value,
    vendor: String,
    part: String,
    source: String,
    glasses: Vec<String>,
    /// Clear aperture (mm): from the spec sheet; a STEP file does not give it.
    clear_aperture: f64,
}

/// Where in the design file an item of the view is written: its table and the entry.
fn entry_of(design: &Value, id: &str) -> Option<(String, Value, usize)> {
    for table in ["element", "detector", "proxy", "source"] {
        if let Some(list) = design[table].as_array() {
            if let Some((i, e)) = list.iter().enumerate().find(|(_, e)| e["id"] == id) {
                return Some((table.to_owned(), e.clone(), i));
            }
        }
    }
    None
}

/// Where the scene placed an item or source of this id (mm).
fn origin_of(scene: &crate::document::Scene, id: &str) -> Option<crate::camera::V3> {
    scene
        .items
        .iter()
        .find(|i| i.id == id)
        .and_then(|i| i.origin)
        .or_else(|| scene.sources.iter().find(|s| s.id == id).map(|s| s.origin))
}

/// What the Move tool moves: the selected entry (an item, or a source chosen in the
/// browser) or, when it is in a group, every member of its group.
struct Movable {
    /// Each member's place in the file and where it stands.
    members: Vec<editors::Member>,
    /// Where the selected one stands: the handles' place, turns' centre.
    anchor: crate::camera::V3,
    /// The members' outlines.
    outline: Vec<Vec<crate::camera::V3>>,
    /// Members that are scene items, and sources, by index.
    items: Vec<usize>,
    sources: Vec<usize>,
    /// The group, if a group is moved.
    group: Option<String>,
}

fn movable(
    scene: &crate::document::Scene,
    selected: Option<usize>,
    focus: Option<&(String, String)>,
) -> Option<Movable> {
    if scene.design["layout"] != "table" {
        return None;
    }
    let id = match (selected.and_then(|i| scene.items.get(i)), focus) {
        (Some(item), _) => item.id.clone(),
        (None, Some((t, id))) if t == "source" => id.clone(),
        _ => return None,
    };
    let (_, entry, _) = entry_of(&scene.design, &id)?;
    let group = entry["group"].as_str().map(str::to_owned);
    let ids: Vec<String> = match &group {
        None => vec![id.clone()],
        Some(g) => ["element", "source", "detector", "proxy"]
            .iter()
            .filter_map(|t| scene.design[*t].as_array())
            .flatten()
            .filter(|e| e["group"] == g.as_str())
            .filter_map(|e| e["id"].as_str().map(str::to_owned))
            .collect(),
    };
    let mut moving = Movable {
        members: Vec::new(),
        anchor: [0.0; 3],
        outline: Vec::new(),
        items: Vec::new(),
        sources: Vec::new(),
        group,
    };
    for member in &ids {
        let (table, entry, index) = entry_of(&scene.design, member)?;
        let origin = if let Some(i) = scene.items.iter().position(|it| &it.id == member) {
            moving.items.push(i);
            moving.outline.extend(scene.items[i].lines.iter().cloned());
            scene.items[i].origin?
        } else {
            let j = scene.sources.iter().position(|s| &s.id == member)?;
            let s = &scene.sources[j];
            moving.sources.push(j);
            let tip = [0, 1, 2].map(|k| s.origin[k] + s.axis[k] * 20.0);
            moving.outline.push(vec![s.origin, tip]);
            s.origin
        };
        if *member == id {
            moving.anchor = origin;
        }
        moving.members.push((table, entry, index, origin));
    }
    Some(moving)
}

/// A few words on what operations change, for the runs strip.
fn describe(operations: &[Value]) -> String {
    let Some(first) = operations.first() else {
        return String::new();
    };
    let what = first["id"]
        .as_str()
        .or(first["dof"].as_str())
        .or(first["table"].as_str())
        .unwrap_or("design");
    let key = first["key"]
        .as_str()
        .unwrap_or(first["op"].as_str().unwrap_or(""));
    let more = if operations.len() > 1 {
        format!(" (+{})", operations.len() - 1)
    } else {
        String::new()
    };
    format!("{what} {key}{more}")
}

/// A screenshot to take once the design is ready (see `screenshot`).
struct Shot {
    path: PathBuf,
    trace: bool,
    traced: bool,
    select: Option<String>,
    /// Graphs to draw first (the `graph` command's `of`), the last drawn first.
    graphs: Vec<String>,
    frames: u32,
    viewed: bool,
    requested: bool,
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl App {
    /// A window, opening `path` if given.
    pub fn new(
        path: Option<PathBuf>,
        data_directory: Option<PathBuf>,
        renderer: Option<crate::render::Renderer>,
        ctx: egui::Context,
    ) -> Self {
        let mut app = Self {
            ctx,
            workspace: 0,
            tab: 0,
            stage: 0,
            doc: None,
            camera: Camera::default(),
            view: viewport::Extras {
                renderer,
                ..Default::default()
            },
            selected: None,
            hidden: Vec::new(),
            show_rays: true,
            show_board: true,
            move_on: false,
            move_tool: crate::moving::MoveTool::default(),
            fit_pending: false,
            chooser: system_chooser(),
            data_directory: data_directory
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_default(),
            confirm: None,
            drafts: editors::Drafts::default(),
            focus: None,
            drafts_version: 0,
            notice: None,
            licence: None,
            shot: std::env::var_os("OPTOBENCH_UI_SCREENSHOT").map(|p| Shot {
                path: PathBuf::from(p),
                trace: std::env::var_os("OPTOBENCH_UI_TRACE").is_some(),
                traced: false,
                select: std::env::var("OPTOBENCH_UI_SELECT").ok(),
                graphs: std::env::var("OPTOBENCH_UI_GRAPHS")
                    .map(|g| {
                        g.split(',')
                            .map(|k| k.trim().to_owned())
                            .filter(|k| !k.is_empty())
                            .collect()
                    })
                    .unwrap_or_default(),
                frames: 0,
                viewed: false,
                requested: false,
                done: Default::default(),
            }),
            graph_cursor: None,
            graph_dialog: false,
            graph_choice: vec!["spot"],
            new_dialog: None,
            placing: None,
            lens_chooser: None,
            lens_import: None,
            library_mounts: None,
            live: None,
            control_directory: None,
            confirm_write: None,
        };
        if let Some(path) = path {
            app.open(path);
        }
        if let Some(w) = std::env::var("OPTOBENCH_UI_WORKSPACE")
            .ok()
            .and_then(|w| w.parse::<usize>().ok())
        {
            app.workspace = w.min(WORKSPACES.len() - 1);
        }
        if std::env::var_os("OPTOBENCH_UI_MOVE").is_some() {
            app.move_on = true;
        }
        if let Some(stage) = std::env::var("OPTOBENCH_UI_STAGE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
        {
            app.stage = stage;
        }
        app
    }

    fn open(&mut self, path: PathBuf) {
        // A layout opens in Layout, a sidecar in CAD.
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        self.workspace = if text.contains("kind = \"layout\"") {
            1
        } else {
            0
        };
        self.tab = 0;
        let data = Some(self.data_directory.trim())
            .filter(|d| !d.is_empty())
            .map(PathBuf::from);
        let ctx = self.ctx.clone();
        self.doc = Some(Document::open(
            path,
            data,
            std::sync::Arc::new(move || ctx.request_repaint()),
        ));
        self.selected = None;
        self.focus = None;
        self.drafts.clear();
        self.hidden.clear();
        self.view.hovered = None;
        self.view.section = None;
        self.fit_pending = true;
    }

    fn accent(&self) -> Accent {
        WORKSPACES[self.workspace].accent
    }

    /// Runs a ribbon control.
    fn activate(&mut self, control: Control) {
        match control {
            Control::Action(action) => match action {
                Action::Open => {
                    let folder = self.folder();
                    if let Some(path) = (self.chooser)(Choice::Open { folder }) {
                        self.dropped(path);
                    }
                }
                Action::Reload => {
                    if let Some(doc) = &self.doc {
                        let path = doc.path.clone();
                        let workspace = self.workspace;
                        self.open(path);
                        self.workspace = workspace;
                    }
                }
                Action::Fit => self.fit_pending = true,
                Action::Top => self.camera.top(),
                Action::Front => self.camera.front(),
                Action::Right => self.camera.right(),
                Action::Isometric => self.camera.isometric(),
                Action::Rays => self.show_rays = !self.show_rays,
                Action::Live => self.toggle_live(),
                Action::ApplyLive => {
                    // The optimised values, asked about first.
                    self.confirm_write = self
                        .doc
                        .as_ref()
                        .and_then(|d| d.optimised.as_ref())
                        .and_then(|o| o["dofs"].as_array())
                        .map(|dofs| {
                            Value::Object(
                                dofs.iter()
                                    .filter_map(|d| {
                                        Some((
                                            d["name"].as_str()?.to_owned(),
                                            d["suggested"].clone(),
                                        ))
                                    })
                                    .collect(),
                            )
                        });
                }
                Action::New => self.new_dialog = Some(25.0),
                Action::Undo => {
                    if let Some(doc) = &mut self.doc {
                        doc.undo();
                    }
                }
                Action::Remove => {
                    if let Some(op) = self.removal() {
                        self.selected = None;
                        self.focus = None;
                        self.apply(vec![op]);
                    }
                }
                Action::AddLens => {
                    let lenses = self
                        .quick("parts", json!({}))
                        .and_then(|v| v["parts"].as_array().cloned())
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|p| p["type"] == "lens")
                        .collect();
                    self.lens_chooser = Some((String::new(), lenses));
                }
                Action::AddMirror => self.start_adding(New::Mirror),
                Action::AddSource => self.start_adding(New::Source),
                Action::AddDetector => self.start_adding(New::Detector),
                Action::AddIris => self.start_adding(New::Iris),
                Action::AddTube => self.start_adding(New::Tube),
                Action::TagLens | Action::TagMirror => {
                    if let Ok(op) = self.tagging(action == Action::TagMirror) {
                        self.apply(vec![op]);
                    }
                }
                Action::Apply => {
                    if let Some(doc) = &mut self.doc {
                        doc.apply_optimised();
                    }
                }
                Action::Move => {
                    // Pressed again, Move ends as Escape does: the pending move is dropped.
                    self.move_on = !self.move_on;
                    self.move_tool.drag = None;
                    self.move_tool.pending.clear();
                    // The pitch is the step to start with on a table.
                    if let Some(pitch) = self.design().and_then(|d| d["table"]["pitch"].as_f64()) {
                        if self.move_tool.pitch != Some(pitch) {
                            self.move_tool.step = pitch;
                        }
                    }
                }
            },
            Control::Command("graph") => self.graph_dialog = true,
            Control::Command(id) => {
                let writes = optobench_commands::Registry::standard()
                    .get(id)
                    .is_some_and(|c| c.effect == optobench_commands::Effect::Write);
                // The window's `optimize` writes only the move report and `graph` no
                // file at all, so neither asks first.
                if writes && id != "optimize" && id != "graph" {
                    self.confirm = Some(id);
                } else {
                    self.run(id);
                }
            }
        }
    }

    fn run(&mut self, id: &str) {
        if let Some(doc) = &mut self.doc {
            doc.start(id, json!({}));
        }
    }

    /// Sends edit operations for the open design.
    fn apply(&mut self, operations: Vec<Value>) {
        // Requests that are no edits: the space around a free DOF (M19).
        let (searches, operations): (Vec<Value>, Vec<Value>) =
            operations.into_iter().partition(|o| o["op"] == "clearance");
        if let Some(doc) = &mut self.doc {
            for s in searches {
                doc.start("clearance", json!({ "dof": s["dof"] }));
            }
        }
        if operations.is_empty() {
            return;
        }
        let what = describe(&operations);
        if let Some(doc) = &mut self.doc {
            doc.edit(operations, what);
        }
    }

    /// Live on: a link to SolidCraft traces the sidecar against the design open there,
    /// again after each change. Live off: the sidecar's own STEP file again.
    fn toggle_live(&mut self) {
        if self.live.take().is_some() {
            self.end_live(
                "Live: off; reading the sidecar's own STEP file again".into(),
                false,
            );
            return;
        }
        let Some(doc) = &self.doc else { return };
        let mut parameters = json!({
            "path": doc.path.to_string_lossy(),
            "ray_paths": crate::document::RAYS_DRAWN,
        });
        if let Some(data) = &doc.data_directory {
            parameters["data_directory"] = json!(data.to_string_lossy());
        }
        if let Some(control) = &self.control_directory {
            parameters["control_directory"] = json!(control.to_string_lossy());
        }
        let ctx = self.ctx.clone();
        self.live = Some(crate::live::Link::start(
            parameters,
            std::sync::Arc::new(move || ctx.request_repaint()),
        ));
        self.notice = Some((false, "Live: connecting to SolidCraft…".into()));
    }

    /// Back to the sidecar's own STEP file: the scene read again, traced again if it was.
    fn end_live(&mut self, why: String, error: bool) {
        self.live = None;
        self.notice = Some((error, why));
        if let Some(doc) = &mut self.doc {
            doc.live_moves = Value::Null;
            if doc.live_step.take().is_some() {
                doc.start("view", json!({}));
                if doc.trace.is_some() {
                    doc.start("trace", json!({}));
                }
            }
        }
    }

    /// Takes in what the live link sent.
    fn poll_live(&mut self) {
        let Some(link) = &mut self.live else { return };
        let mut ended = None;
        for message in link.poll() {
            match message {
                crate::live::Message::Update(update) => {
                    link.revision = update["revision"].as_u64();
                    let seconds = update["seconds"].as_f64().unwrap_or(0.0);
                    if let Some(doc) = &mut self.doc {
                        doc.accept_live(&update);
                    }
                    self.notice = Some((
                        false,
                        format!(
                            "Live: SolidCraft revision {}, traced in {seconds:.2} s",
                            link.revision.unwrap_or_default()
                        ),
                    ));
                }
                crate::live::Message::Ended(why) => ended = Some(why),
                crate::live::Message::Written(done) => {
                    let n = done["written"]["written"].as_u64().unwrap_or(0);
                    let reset = done["sidecar"].as_array().map_or(0, Vec::len);
                    let text = format!(
                        "Written into SolidCraft: {n} change{}, read back as asked; {reset} value{} in the sidecar set",
                        if n == 1 { "" } else { "s" },
                        if reset == 1 { "" } else { "s" },
                    );
                    self.notice = Some((false, text.clone()));
                    if let Some(doc) = &mut self.doc {
                        doc.messages
                            .push(crate::document::Message { error: false, text });
                        // The sidecar was changed: read it again (the trace follows the
                        // design's change).
                        doc.optimised = None;
                        doc.start("view", json!({}));
                    }
                }
                crate::live::Message::NotWritten(why) => {
                    let text = format!("Not written into SolidCraft: {why}");
                    self.notice = Some((true, text.clone()));
                    if let Some(doc) = &mut self.doc {
                        doc.messages
                            .push(crate::document::Message { error: true, text });
                    }
                }
            }
        }
        if let Some(why) = ended {
            self.end_live(format!("Live: {why}"), true);
        }
    }

    fn design(&self) -> Option<&Value> {
        self.doc
            .as_ref()
            .and_then(|d| d.scene.as_ref())
            .map(|s| &s.design)
    }

    /// Adds a new item: on a table layout it waits for a hole to be clicked (or Enter),
    /// on a planar layout it goes after the last item at once.
    fn start_adding(&mut self, new: New) {
        self.start_adding_part(new, None);
    }

    /// As `start_adding`, a lens from the parts library when `library` names it.
    fn start_adding_part(&mut self, new: New, library: Option<String>) {
        if self.design().is_some_and(|d| d["layout"] == "table") {
            self.end_move();
            self.placing = Some((new, library));
        } else if let Some(op) = self
            .design()
            .and_then(|d| editors::add_at(d, new, None, library.as_deref()))
        {
            self.apply(vec![op]);
        }
    }

    /// Apply in SolidCraft, asked first: the values to write, and what writing does.
    fn confirm_write_window(&mut self, ctx: &egui::Context) {
        let Some(dofs) = self.confirm_write.clone() else {
            return;
        };
        let mut choice = None;
        egui::Window::new("Write into SolidCraft")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("These values are made in the design open in SolidCraft:");
                egui::Grid::new("write-values").num_columns(2).show(ui, |ui| {
                    for (name, value) in dofs.as_object().into_iter().flatten() {
                        ui.label(RichText::new(name).color(DARK.text_dim));
                        ui.label(format!("{:.4}", value.as_f64().unwrap_or(f64::NAN)));
                        ui.end_row();
                    }
                });
                ui.label(
                    RichText::new(
                        "Parts move (or their bound parameters and joints are set), each change one undo step in SolidCraft. Everything is read back; if anything is not as asked, the design is put back as it was. The sidecar's values for those parts are set back to 0.",
                    )
                    .color(DARK.text_dim)
                    .size(11.0),
                );
                ui.horizontal(|ui| {
                    if ui.button("Write").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(false);
                    }
                });
            });
        match choice {
            Some(true) => {
                self.confirm_write = None;
                if let Some(link) = &self.live {
                    link.write(dofs);
                    self.notice = Some((false, "Writing into SolidCraft…".into()));
                }
            }
            Some(false) => self.confirm_write = None,
            None => {}
        }
    }

    /// Lens: a singlet (its shape and focal length set in Properties), or a lens of the
    /// parts library (singlets, cemented and air-spaced doublets, triplets).
    fn lens_chooser(&mut self, ctx: &egui::Context) {
        let Some((mut search, lenses)) = self.lens_chooser.take() else {
            return;
        };
        let mut open = true;
        let mut chosen: Option<Option<String>> = None;
        egui::Window::new("Lens")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                if ui
                    .button("Singlet")
                    .on_hover_text("A biconvex singlet: set its shape and focal length in Properties")
                    .clicked()
                {
                    chosen = Some(None);
                }
                ui.separator();
                ui.label(RichText::new("From the parts library").strong());
                if lenses.is_empty() {
                    ui.label(
                        RichText::new("No lenses yet: drop a lens's STEP file (or a part file) on the window to add one")
                            .color(DARK.text_dim),
                    );
                } else {
                    ui.add(egui::TextEdit::singleline(&mut search).hint_text("search"));
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for p in &lenses {
                            let key = p["key"].as_str().unwrap_or("?");
                            if !search.is_empty()
                                && !key.to_lowercase().contains(&search.to_lowercase())
                            {
                                continue;
                            }
                            let glass = p["glass"].as_array().cloned().unwrap_or_default();
                            let air = glass.iter().filter(|g| *g == "standard-air").count();
                            let what = match (glass.len() - air, air > 0) {
                                (1, _) => "singlet".to_owned(),
                                (2, false) => "cemented doublet".to_owned(),
                                (2, true) => "air-spaced doublet".to_owned(),
                                (3, false) => "cemented triplet".to_owned(),
                                (n, _) => format!("{n} glasses"),
                            };
                            let size = p["diameter_mm"]
                                .as_f64()
                                .map_or(String::new(), |d| format!(" · Ø{d}"));
                            if ui
                                .button(key)
                                .on_hover_text(p["source"].as_str().unwrap_or(""))
                                .clicked()
                            {
                                chosen = Some(Some(key.to_owned()));
                            }
                            ui.label(
                                RichText::new(format!("{what}{size}"))
                                    .color(DARK.text_dim)
                                    .size(11.0),
                            );
                        }
                    });
                }
            });
        match chosen {
            Some(library) => self.start_adding_part(New::Lens, library),
            None if open => self.lens_chooser = Some((search, lenses)),
            None => {}
        }
    }

    /// A lens's STEP file dropped: what was found, and the vendor, part number, spec
    /// sheet and glasses that make it a library part.
    fn lens_import_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut import) = self.lens_import.take() else {
            return;
        };
        let mut open = true;
        let mut done = false;
        egui::Window::new("Lens from STEP")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                let f = &import.found;
                ui.label(
                    RichText::new(format!(
                        "{}, Ø{} mm{}",
                        f["kind"].as_str().unwrap_or("?"),
                        f["diameter_mm"].as_f64().unwrap_or(0.0),
                        match f["left_out"].as_u64() {
                            Some(0) | None => String::new(),
                            Some(n) => format!(" ({n} mount bodies left out)"),
                        }
                    ))
                    .strong(),
                );
                let show = |v: &Value| match v {
                    Value::String(s) => s.clone(),
                    v => v.to_string(),
                };
                let radii: Vec<String> = f["radii_mm"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(show)
                    .collect();
                let thickness: Vec<String> = f["thickness_mm"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(show)
                    .collect();
                let gaps: Vec<String> = f["gaps"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(show)
                    .collect();
                egui::Grid::new("lens-import")
                    .num_columns(2)
                    .show(ui, |ui| {
                        for (k, r) in radii.iter().enumerate() {
                            ui.label(RichText::new(format!("R{}", k + 1)).color(DARK.text_dim));
                            ui.label(format!("{r} mm"));
                            ui.end_row();
                            if let (Some(t), Some(g)) = (thickness.get(k), gaps.get(k)) {
                                ui.label(
                                    RichText::new(format!("tc{}", k + 1)).color(DARK.text_dim),
                                );
                                ui.label(format!("{t} mm, {g}"));
                                ui.end_row();
                            }
                        }
                    });
                ui.separator();
                egui::Grid::new("lens-import-fields")
                    .num_columns(2)
                    .show(ui, |ui| {
                        for (label, value) in [
                            ("vendor", &mut import.vendor),
                            ("part number", &mut import.part),
                            ("spec sheet", &mut import.source),
                        ] {
                            ui.label(RichText::new(label).color(DARK.text_dim));
                            ui.text_edit_singleline(value);
                            ui.end_row();
                        }
                        ui.label(RichText::new("clear aperture").color(DARK.text_dim))
                            .on_hover_text("from the spec sheet; a STEP file does not give it");
                        ui.add(
                            egui::DragValue::new(&mut import.clear_aperture)
                                .range(0.1..=1000.0)
                                .speed(0.1)
                                .suffix(" mm"),
                        );
                        ui.end_row();
                        for (k, glass) in import.glasses.iter_mut().enumerate() {
                            ui.label(
                                RichText::new(format!("glass {}", k + 1)).color(DARK.text_dim),
                            )
                            .on_hover_text(
                                "a material name (bundled or imported, e.g. N-BK7) or an index",
                            );
                            ui.text_edit_singleline(glass);
                            ui.end_row();
                        }
                    });
                let ready = !import.vendor.trim().is_empty()
                    && !import.part.trim().is_empty()
                    && !import.source.trim().is_empty()
                    && import.glasses.iter().all(|g| !g.trim().is_empty());
                if ui
                    .add_enabled(ready, egui::Button::new("Import"))
                    .on_hover_text("Write the part file into the parts library")
                    .clicked()
                {
                    done = true;
                }
            });
        if done {
            let glass: Vec<Value> = import
                .glasses
                .iter()
                .map(|g| {
                    g.trim()
                        .parse::<f64>()
                        .map_or_else(|_| json!(g.trim()), |n| json!(n))
                })
                .collect();
            let made = self.quick(
                "part-from-step",
                json!({
                    "path": import.path.to_string_lossy(),
                    "vendor": import.vendor.trim(),
                    "part": import.part.trim(),
                    "source": import.source.trim(),
                    "glass": glass,
                    "clear_aperture": import.clear_aperture,
                    "import": true,
                }),
            );
            if let Some(made) = made {
                let key = made["part"]["key"].as_str().unwrap_or("?").to_owned();
                self.notice = Some((false, format!("Lens imported: {key}; add it with Lens")));
                return;
            }
        }
        if open {
            self.lens_import = Some(import);
        }
    }

    /// The operation that removes the selected entry, if it is in the file.
    fn removal(&self) -> Option<Value> {
        let design = self.design()?;
        if let Some((table, key)) = &self.focus {
            return Some(match table.as_str() {
                "merit" => {
                    json!({ "op": "remove", "table": "merit", "index": key.parse::<usize>().ok()? })
                }
                _ => json!({ "op": "remove", "table": table, "id": key }),
            });
        }
        let item = self
            .doc
            .as_ref()?
            .scene
            .as_ref()?
            .items
            .get(self.selected?)?;
        let (table, _, _) = entry_of(design, &item.id)?;
        Some(json!({ "op": "remove", "table": table, "id": item.id }))
    }

    /// The operation that tags the selected CAD body, or why it cannot.
    fn tagging(&self, mirror: bool) -> Result<Value, String> {
        let design = self.design().ok_or("open a sidecar first")?;
        if design["kind"] != "sidecar" {
            return Err("tagging is for CAD sidecars".into());
        }
        let item = self
            .selected
            .and_then(|i| self.doc.as_ref()?.scene.as_ref()?.items.get(i))
            .ok_or("select a body in the view or under Mechanics")?;
        if item.kind != "solid" || item.role != "absorber" {
            return Err("select an untagged body (under Mechanics)".into());
        }
        let face = if mirror {
            let normal = self
                .view
                .picked_normal
                .ok_or("click the mirror's reflecting face in the view")?;
            let axes = item.body_axes.ok_or("the body's frame is not known")?;
            Some((normal, axes))
        } else {
            None
        };
        editors::tag(design, &item.id, face).ok_or_else(|| "this body has no occurrence".into())
    }

    /// The data directory parameter, when one is typed.
    fn data_parameter(&self) -> Value {
        match self.data_directory.trim() {
            "" => json!({}),
            d => json!({ "data_directory": d }),
        }
    }

    /// Runs a quick command on the spot (imports of small files) and notes the outcome.
    fn quick(&mut self, command: &str, mut parameters: Value) -> Option<Value> {
        if let (Some(p), Some(d)) = (
            parameters.as_object_mut(),
            self.data_parameter().as_object(),
        ) {
            p.extend(d.clone());
        }
        match optobench_commands::Registry::standard().run(command, &parameters) {
            Ok(v) => Some(v),
            Err(e) => {
                self.notice = Some((true, format!("{command}: {}", e.message)));
                None
            }
        }
    }

    /// A file dropped on the window: a design opens; a part, a curve table or a material
    /// is imported into the data directory.
    fn dropped(&mut self, path: PathBuf) {
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let shown = path.to_string_lossy().into_owned();
        match extension.as_str() {
            "optobench" => {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                if text.contains("kind = \"part\"") {
                    if self
                        .quick("import-part", json!({ "path": shown }))
                        .is_some()
                    {
                        self.notice = Some((false, format!("Part imported: {name}")));
                    }
                } else {
                    self.open(path);
                }
            }
            "step" | "stp" => {
                if let Some(found) = self.quick("part-from-step", json!({ "path": shown })) {
                    let glasses = found["glasses"].as_u64().unwrap_or(1) as usize;
                    // A start: 90 % of the diameter, as spec sheets often give it.
                    let clear_aperture =
                        (found["diameter_mm"].as_f64().unwrap_or(25.4) * 9.0).round() / 10.0;
                    self.lens_import = Some(LensImport {
                        path,
                        found,
                        clear_aperture,
                        vendor: String::new(),
                        part: name.to_uppercase(),
                        source: format!(
                            "{name}: radii from its STEP file, glasses from its spec sheet"
                        ),
                        glasses: vec![String::new(); glasses],
                    });
                }
            }
            "yml" | "yaml" => {
                if let Some(preview) =
                    self.quick("import-material", json!({ "path": shown, "name": name }))
                {
                    self.licence = Some((path, name, preview));
                }
            }
            "xlsx" | "csv" | "txt" | "tsv" | "zip" => {
                if let Some(out) = self.quick("import-curve", json!({ "path": shown })) {
                    let count = out["curves"].as_array().map_or(1, Vec::len);
                    self.notice = Some((false, format!("{count} curve(s) imported from {name}")));
                }
            }
            other => {
                self.notice = Some((
                    true,
                    format!(
                        "{name}.{other}: drop a design (.optobench), a part, a lens's STEP file, a curve table (.xlsx, .csv, .zip) or a material (.yml)"
                    ),
                ));
            }
        }
    }

    /// The licence notice of a material file being imported, to accept or cancel.
    fn licence_dialog(&mut self, ctx: &egui::Context) {
        let Some((path, name, preview)) = self.licence.clone() else {
            return;
        };
        let response = egui::Modal::new(egui::Id::unique("licence")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.label(
                RichText::new(format!("Import material {name}"))
                    .strong()
                    .size(14.0),
            );
            for (title, key) in [("Notice", "notice"), ("References", "references")] {
                if let Some(text) = preview[key].as_str().filter(|t| !t.is_empty()) {
                    ui.label(RichText::new(title).color(DARK.text_dim));
                    ui.label(text);
                }
            }
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    return Some(false);
                }
                let accept =
                    egui::Button::new(RichText::new("Accept and import").color(Color32::WHITE))
                        .fill(self.accent().fill);
                if ui.add(accept).clicked() {
                    return Some(true);
                }
                None
            })
            .inner
        });
        match response.inner {
            Some(true) => {
                self.licence = None;
                let parameters = json!({
                    "path": path.to_string_lossy(), "name": name, "accept_licence": true,
                });
                if self.quick("import-material", parameters).is_some() {
                    self.notice = Some((false, format!("Material imported: {name}")));
                }
            }
            Some(false) => self.licence = None,
            None if response.should_close() => self.licence = None,
            None => {}
        }
    }

    /// New's chooser: a planar layout, or a table layout on a breadboard of a pitch.
    fn new_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut pitch) = self.new_dialog else {
            return;
        };
        let mut open = true;
        let mut chosen: Option<Option<f64>> = None;
        egui::Window::new("New layout")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                if ui
                    .button("Planar")
                    .on_hover_text("Items placed along a folded optical axis")
                    .clicked()
                {
                    chosen = Some(None);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .button("Table")
                        .on_hover_text("Items placed on the holes of a breadboard, at a height")
                        .clicked()
                    {
                        chosen = Some(Some(pitch));
                    }
                    ui.label(RichText::new("pitch").color(DARK.text_dim));
                    ui.add(
                        egui::DragValue::new(&mut pitch)
                            .range(1.0..=200.0)
                            .speed(0.1)
                            .suffix(" mm"),
                    );
                    if ui
                        .small_button("25 mm")
                        .on_hover_text("Metric breadboard")
                        .clicked()
                    {
                        pitch = 25.0;
                    }
                    if ui
                        .small_button("1 in")
                        .on_hover_text("Imperial breadboard, 25.4 mm")
                        .clicked()
                    {
                        pitch = 25.4;
                    }
                });
            });
        self.new_dialog = open.then_some(pitch);
        if let Some(table) = chosen {
            self.new_dialog = None;
            self.new_layout(table);
        }
    }

    /// Ends the Move tool, dropping what is pending (written already when applied).
    fn end_move(&mut self) {
        self.move_on = false;
        self.move_tool.drag = None;
        self.move_tool.pending.clear();
    }

    /// The open design's folder, where the file dialogs start.
    fn folder(&self) -> Option<PathBuf> {
        self.doc
            .as_ref()
            .and_then(|d| d.path.parent().map(PathBuf::from))
            .filter(|f| !f.as_os_str().is_empty())
    }

    /// Starts a new layout: planar, or a table with this pitch (mm), saved where asked.
    fn new_layout(&mut self, table: Option<f64>) {
        // Where: asked, starting next to the open design; the dialog has asked already
        // whether to replace a file that is there.
        let folder = self.folder();
        let Some(mut path) = (self.chooser)(Choice::Save {
            folder,
            name: "layout.optobench".into(),
        }) else {
            return;
        };
        if path.extension().is_none_or(|e| e != "optobench") {
            path.as_mut_os_string().push(".optobench");
        }
        if path.is_file() && std::fs::remove_file(&path).is_err() {
            self.notice = Some((
                true,
                format!("New: {} could not be replaced", path.display()),
            ));
            return;
        }
        let result = optobench_commands::Registry::standard().run(
            "edit",
            &json!({
                "path": path.to_string_lossy(),
                "create": match table {
                    Some(pitch) => json!({ "layout": "table", "pitch": pitch }),
                    None => json!({ "layout": "planar" }),
                },
            }),
        );
        match result {
            Ok(_) => {
                self.open(path);
                self.workspace = 1;
            }
            Err(e) => {
                if let Some(doc) = &mut self.doc {
                    doc.messages.push(crate::document::Message {
                        error: true,
                        text: format!("New: {}", e.message),
                    });
                }
            }
        }
    }

    /// Whether a control can run now, or why not.
    fn available(&self, control: Control) -> Result<(), String> {
        let layout = || -> Result<(), String> {
            match self.design().and_then(|d| d["layout"].as_str()) {
                Some(_) => Ok(()),
                None => Err("for layouts (planar or table)".into()),
            }
        };
        match control {
            Control::Action(Action::Open | Action::New) => Ok(()),
            Control::Action(Action::Undo) => match &self.doc {
                Some(doc) if !doc.undo.is_empty() => Ok(()),
                Some(_) => Err("nothing to undo".into()),
                None => Err("open a design first".into()),
            },
            Control::Action(Action::Apply) => match &self.doc {
                Some(doc) if doc.optimised.is_some() => Ok(()),
                _ => Err("optimise first".into()),
            },
            Control::Action(Action::Remove) => self
                .removal()
                .map(|_| ())
                .ok_or_else(|| "select an item of the design".into()),
            Control::Action(
                Action::AddLens
                | Action::AddMirror
                | Action::AddSource
                | Action::AddDetector
                | Action::AddIris
                | Action::AddTube,
            ) => layout(),
            Control::Action(Action::ApplyLive) => match (&self.live, &self.doc) {
                (None, _) => {
                    Err("turn Live on first: it writes into the design open in SolidCraft".into())
                }
                (Some(_), Some(doc)) if doc.optimised.is_some() => Ok(()),
                _ => Err("optimise first".into()),
            },
            Control::Action(Action::Live) => match self.design() {
                Some(d) if d["kind"] == "sidecar" => Ok(()),
                Some(_) => Err("the live link traces a CAD design: open its sidecar".into()),
                None => Err("open a sidecar first".into()),
            },
            Control::Action(Action::TagLens) => self.tagging(false).map(|_| ()),
            Control::Action(Action::TagMirror) => self.tagging(true).map(|_| ()),
            Control::Action(_) => self
                .doc
                .as_ref()
                .map(|_| ())
                .ok_or_else(|| "open a design first".into()),
            Control::Command(id) => {
                let doc = self.doc.as_ref().ok_or("open a design first")?;
                if doc.running.iter().any(|c| c == id) {
                    return Err("running".into());
                }
                doc.parameters(id).map(|_| ())
            }
        }
    }

    fn label(control: Control) -> String {
        match control {
            Control::Action(a) => a.label().to_owned(),
            Control::Command(id) => optobench_commands::Registry::standard()
                .get(id)
                .map_or(id.to_owned(), |c| c.label.to_owned()),
        }
    }

    /// The icon id of a control: the command's from the registry, or the action's.
    fn icon(control: Control) -> String {
        match control {
            Control::Action(a) => a.icon().to_owned(),
            Control::Command(id) => optobench_commands::Registry::standard()
                .get(id)
                .map_or(id.to_owned(), |c| c.icon.to_owned()),
        }
    }

    fn summary(control: Control) -> String {
        match control {
            Control::Action(a) => a.summary().to_owned(),
            Control::Command(id) => optobench_commands::Registry::standard()
                .get(id)
                .map_or(String::new(), |c| c.summary.to_owned()),
        }
    }

    // ----- The frame -----

    fn app_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(8.0);
            mark(ui, 14.0, self.accent().fill);
            ui.label(RichText::new("optobench").strong().size(14.0));
            ui.separator();
            if self.doc.is_none() {
                let keys = ui.ctx().format_shortcut(&OPEN);
                ui.label(
                    RichText::new(format!(
                        "Open a design ({keys}), or drop a .optobench file on the window"
                    ))
                    .color(DARK.text_dim),
                );
            }
            let data = egui::TextEdit::singleline(&mut self.data_directory)
                .hint_text("data directory (default: the platform's)")
                .desired_width(220.0);
            let response = ui.add(data).on_hover_text(
                "Where imported materials, parts and curves are. Enter reloads the design.",
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.activate(Control::Action(Action::Reload));
            }
            if ui
                .small_button("…")
                .on_hover_text("Choose the data directory")
                .clicked()
            {
                let folder = Some(PathBuf::from(self.data_directory.trim()))
                    .filter(|f| !f.as_os_str().is_empty());
                if let Some(chosen) = (self.chooser)(Choice::Folder { folder }) {
                    self.data_directory = chosen.to_string_lossy().into_owned();
                    self.activate(Control::Action(Action::Reload));
                }
            }
            if let Some(doc) = &self.doc {
                ui.separator();
                Frame::new()
                    .fill(DARK.panel)
                    .corner_radius(CornerRadius::same(DARK.radius_md))
                    .inner_margin(Margin::symmetric(10, 3))
                    .show(ui, |ui| {
                        ui.label(RichText::new(doc.name()).strong())
                            .on_hover_text(doc.path.to_string_lossy());
                    });
                if !doc.running.is_empty() {
                    ui.spinner();
                    ui.label(RichText::new(doc.running.join(", ")).color(DARK.text_dim));
                }
            }
            ui.with_layout(EguiLayout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).color(DARK.text_dim),
                );
            });
        });
    }

    fn workspace_tile(&mut self, ui: &mut egui::Ui) {
        let accent = self.accent();
        let (rect, response) = ui.allocate_exact_size(Vec2::new(104.0, 78.0), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Workspace"));
        let fill = if response.hovered() {
            accent.soft.gamma_multiply(1.3)
        } else {
            accent.soft
        };
        ui.painter()
            .rect_filled(rect, CornerRadius::same(DARK.radius_lg), fill);
        ui.painter().line_segment(
            [
                rect.left_top() + Vec2::new(10.0, 1.0),
                rect.right_top() + Vec2::new(-10.0, 1.0),
            ],
            Stroke::new(3.0, accent.fill),
        );
        // The workspace's emblem.
        crate::icons::draw(
            ui.painter(),
            egui::Rect::from_center_size(rect.center() - Vec2::new(0.0, 15.0), Vec2::splat(30.0)),
            &format!("workspace-{}", WORKSPACES[self.workspace].id),
            true,
        );
        ui.painter().text(
            rect.center() + Vec2::new(0.0, 10.0),
            egui::Align2::CENTER_CENTER,
            WORKSPACES[self.workspace].name,
            egui::FontId::proportional(14.0),
            DARK.text,
        );
        ui.painter().text(
            rect.center() + Vec2::new(0.0, 26.0),
            egui::Align2::CENTER_CENTER,
            "Workspace",
            egui::FontId::proportional(10.5),
            DARK.text_dim,
        );
        chevron(
            ui.painter(),
            rect.center() + Vec2::new(30.0, 26.0),
            DARK.text_dim,
        );
        egui::Popup::menu(&response).show(|ui| {
            for (i, w) in WORKSPACES.iter().enumerate() {
                let text = RichText::new(w.name).color(if i == self.workspace {
                    w.accent.ink
                } else {
                    DARK.text
                });
                if ui.selectable_label(i == self.workspace, text).clicked() {
                    self.workspace = i;
                    self.tab = 0;
                    ui.close();
                }
            }
        });
    }

    /// A ribbon button: an icon tile (the command's initial until the icon set lands)
    /// over its label.
    fn ribbon_button(&mut self, ui: &mut egui::Ui, item: Item) {
        let available = self.available(item.control);
        // As wide as the label needs, at least 64 points.
        let label = Self::label(item.control);
        let width = ui
            .painter()
            .layout_no_wrap(label.clone(), egui::FontId::proportional(11.0), DARK.text)
            .size()
            .x;
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new((width + 12.0).max(64.0), 58.0), Sense::click());
        let enabled = available.is_ok();
        if response.hovered() && enabled {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(DARK.radius_md), DARK.hover);
        }
        let ink = if enabled {
            DARK.text
        } else {
            DARK.text_dim.gamma_multiply(0.6)
        };
        let icon = Vec2::new(26.0, 26.0);
        let icon_rect =
            egui::Rect::from_center_size(rect.center_top() + Vec2::new(0.0, 18.0), icon);
        let active = match item.control {
            Control::Action(Action::Rays) => self.show_rays,
            Control::Action(Action::Live) => self.live.is_some(),
            _ => false,
        };
        if active {
            ui.painter().rect_filled(
                icon_rect.expand(3.0),
                CornerRadius::same(DARK.radius_sm),
                self.accent().soft,
            );
        }
        crate::icons::draw(ui.painter(), icon_rect, &Self::icon(item.control), enabled);
        ui.painter().text(
            rect.center_bottom() - Vec2::new(0.0, 9.0),
            egui::Align2::CENTER_CENTER,
            &label,
            egui::FontId::proportional(11.0),
            ink,
        );
        // Named for screen readers (and the interface tests).
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label.clone()));
        let response = match &available {
            Ok(()) => response.on_hover_text(Self::summary(item.control)),
            Err(why) => response.on_hover_text(format!("{}: {why}", Self::summary(item.control))),
        };
        if response.clicked() && enabled {
            self.activate(item.control);
        }
    }

    fn section(&mut self, ui: &mut egui::Ui, section: &Section) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                for item in section.items.iter().filter(|i| i.promoted) {
                    self.ribbon_button(ui, *item);
                }
            });
            ui.menu_button(
                RichText::new(section.name).size(10.5).color(DARK.text_dim),
                |ui| {
                    for item in section.items {
                        let enabled = self.available(item.control).is_ok();
                        if ui
                            .add_enabled(enabled, egui::Button::new(Self::label(item.control)))
                            .clicked()
                        {
                            self.activate(item.control);
                            ui.close();
                        }
                    }
                },
            );
        });
        ui.separator();
    }

    fn ribbon(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_top(|ui| {
            ui.add_space(6.0);
            self.workspace_tile(ui);
            ui.add_space(6.0);
            let accent = self.accent();
            match WORKSPACES[self.workspace].layout {
                Layout::Ribbon(tabs) => {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            for (i, tab) in tabs.iter().enumerate() {
                                let on = i == self.tab;
                                let text = RichText::new(tab.name).color(if on {
                                    accent.ink
                                } else {
                                    DARK.text
                                });
                                let button = egui::Button::new(text).fill(if on {
                                    accent.soft
                                } else {
                                    Color32::TRANSPARENT
                                });
                                if ui.add(button).clicked() {
                                    self.tab = i;
                                }
                            }
                        });
                        let tab = tabs[self.tab.min(tabs.len() - 1)];
                        ui.horizontal(|ui| {
                            for section in tab.sections {
                                self.section(ui, section);
                            }
                        });
                    });
                }
                Layout::Stages {
                    stages,
                    primary,
                    tools,
                } => {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            for (i, stage) in stages.iter().enumerate() {
                                if i > 0 {
                                    ui.label(RichText::new("—").color(DARK.text_dim));
                                }
                                self.stage_chip(ui, i, stage, accent);
                            }
                            ui.with_layout(EguiLayout::right_to_left(Align::Center), |ui| {
                                ui.add_space(8.0);
                                let enabled = self.available(primary).is_ok();
                                let button = egui::Button::new(
                                    RichText::new(Self::label(primary))
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(accent.fill)
                                .min_size(Vec2::new(96.0, 28.0));
                                if ui.add_enabled(enabled, button).clicked() {
                                    self.activate(primary);
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            for item in tools {
                                self.ribbon_button(ui, *item);
                            }
                        });
                    });
                }
            }
        });
    }

    fn stage_chip(&mut self, ui: &mut egui::Ui, i: usize, stage: &Stage, accent: Accent) {
        let on = i == self.stage;
        let text = RichText::new(format!("{}  {}", i + 1, stage.name)).color(if on {
            accent.ink
        } else {
            DARK.text
        });
        let button = egui::Button::new(text)
            .fill(if on {
                accent.soft
            } else {
                Color32::TRANSPARENT
            })
            .corner_radius(CornerRadius::same(14));
        if ui.add(button).clicked() {
            self.stage = i;
        }
    }

    fn browser(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Browser").strong().size(14.0));
            if let Some(scene) = self.doc.as_ref().and_then(|d| d.scene.as_ref()) {
                ui.with_layout(EguiLayout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{} items", scene.items.len())).color(DARK.text_dim),
                    );
                });
            }
        });
        ui.separator();
        let Some(scene) = self.doc.as_ref().and_then(|d| d.scene.clone()) else {
            ui.label(RichText::new("No design open").color(DARK.text_dim));
            return;
        };
        if self.hidden.len() != scene.items.len() {
            self.hidden = vec![false; scene.items.len()];
        }
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let row = |ui: &mut egui::Ui, app: &mut Self, index: usize, label: String| {
                    ui.horizontal(|ui| {
                        let mut visible = !app.hidden[index];
                        if ui
                            .checkbox(&mut visible, "")
                            .on_hover_text("Show or hide")
                            .changed()
                        {
                            app.hidden[index] = !visible;
                        }
                        let on = app.selected == Some(index);
                        let colour = viewport::role_colour(&scene.items[index].role);
                        swatch(ui, colour);
                        if ui.selectable_label(on, label).clicked() {
                            app.selected = if on { None } else { Some(index) };
                            app.focus = None;
                        }
                    });
                };
                if crate::board::Board::of(&scene.design, &scene).is_some() {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.show_board, "")
                            .on_hover_text("Show or hide the breadboard");
                        swatch(ui, Color32::from_rgb(56, 61, 71));
                        let key = ("table".to_owned(), "board".to_owned());
                        let on = self.focus.as_ref() == Some(&key);
                        if ui.selectable_label(on, "Breadboard").clicked() {
                            self.focus = (!on).then_some(key);
                            self.selected = None;
                        }
                    });
                }
                let groups = editors::groups_of(&scene.design);
                if !groups.is_empty() {
                    egui::CollapsingHeader::new(format!("Groups ({})", groups.len()))
                        .default_open(true)
                        .show(ui, |ui| {
                            for g in &groups {
                                let members: Vec<String> =
                                    ["element", "source", "detector", "proxy"]
                                        .iter()
                                        .filter_map(|t| scene.design[*t].as_array())
                                        .flatten()
                                        .filter(|e| e["group"] == g.as_str())
                                        .filter_map(|e| e["id"].as_str().map(str::to_owned))
                                        .collect();
                                // Its CAD parts too (a sidecar's `occurrences`, M18).
                                let parts: Vec<String> = scene.design["group"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter(|x| x["id"] == g.as_str())
                                    .flat_map(|x| x["occurrences"].as_array().cloned().unwrap_or_default())
                                    .filter_map(|o| o.as_str().map(str::to_owned))
                                    .collect();
                                let label = format!(
                                    "{g}  ·  {}",
                                    members.iter().chain(&parts).cloned().collect::<Vec<_>>().join(", ")
                                );
                                let key = ("group".to_owned(), g.clone());
                                let on = self.focus.as_ref() == Some(&key);
                                if ui
                                    .selectable_label(on, label)
                                    .on_hover_text(
                                        "Its Properties (its degrees of freedom); its first item selected, so Move moves the whole group",
                                    )
                                    .clicked()
                                {
                                    let focus = Some(key);
                                    if let Some(first) = members.first() {
                                        match scene.items.iter().position(|i| &i.id == first) {
                                            Some(i) => self.selected = Some(i),
                                            None => self.selected = None,
                                        }
                                    }
                                    self.focus = focus;
                                }
                            }
                        });
                }
                egui::CollapsingHeader::new(format!("Sources ({})", scene.sources.len()))
                    .default_open(true)
                    .show(ui, |ui| {
                        for s in &scene.sources {
                            ui.horizontal(|ui| {
                                swatch(ui, crate::viewport::ray_colour("detected", 0));
                                let on = self.focus.as_ref()
                                    == Some(&("source".to_owned(), s.id.clone()));
                                if ui.selectable_label(on, &s.id).clicked() {
                                    self.focus = (!on).then(|| ("source".to_owned(), s.id.clone()));
                                    self.selected = None;
                                }
                            });
                        }
                    });
                let optics: Vec<usize> = (0..scene.items.len())
                    .filter(|&i| {
                        matches!(
                            scene.items[i].role.as_str(),
                            "lens" | "mirror" | "objective" | "sample"
                        )
                    })
                    .collect();
                egui::CollapsingHeader::new(format!("Optics ({})", optics.len()))
                    .default_open(true)
                    .show(ui, |ui| {
                        for i in optics {
                            let item = &scene.items[i];
                            row(ui, self, i, format!("{}  ·  {}", item.id, item.role));
                        }
                    });
                // Proxies: an iris (a plate with a hole) or a tube, by the file's kind.
                let proxies: Vec<(usize, String)> = scene
                    .design["proxy"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| {
                        let id = p["id"].as_str()?;
                        let i = scene.items.iter().position(|it| it.kind == "proxy" && it.id == id)?;
                        let kind = match p["kind"].as_str() {
                            Some("plate") => "iris",
                            Some(other) => other,
                            None => "proxy",
                        };
                        Some((i, format!("{id}  ·  {kind}")))
                    })
                    .collect();
                if !proxies.is_empty() {
                    egui::CollapsingHeader::new(format!("Irises and tubes ({})", proxies.len()))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (i, label) in proxies {
                                row(ui, self, i, label);
                            }
                        });
                }
                let detectors: Vec<usize> = (0..scene.items.len())
                    .filter(|&i| scene.items[i].role == "detector")
                    .collect();
                egui::CollapsingHeader::new(format!("Detectors ({})", detectors.len()))
                    .default_open(true)
                    .show(ui, |ui| {
                        for i in detectors {
                            row(ui, self, i, scene.items[i].id.clone());
                        }
                    });
                // What the optimiser aligns for: always listed, terms added here.
                let merit = scene.design["merit"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let first_detector = editors::first_detector(&scene.design);
                let backscatter = editors::backscatter_term(&scene.design);
                egui::CollapsingHeader::new(format!("Merit ({})", merit.len()))
                    .default_open(true)
                    .show(ui, |ui| {
                        for (i, term) in merit.iter().enumerate() {
                            let key = ("merit".to_owned(), i.to_string());
                            let on = self.focus.as_ref() == Some(&key);
                            let target = match &term["target"] {
                                Value::Null => String::new(),
                                t => format!(" → {t}"),
                            };
                            let label = format!(
                                "{} · {}{target}",
                                term["kind"].as_str().unwrap_or("?"),
                                term["detector"].as_str().unwrap_or("")
                            );
                            if ui.selectable_label(on, label).clicked() {
                                self.focus = (!on).then_some(key);
                                self.selected = None;
                            }
                        }
                        ui.menu_button("Add a term", |ui| {
                            if let Some(ops) = &backscatter {
                                if ui
                                    .button("backscatter")
                                    .on_hover_text("as little light as possible coming back into the laser (%): measured on a returning detector at the laser, added there when there is none")
                                    .clicked()
                                {
                                    self.apply(ops.clone());
                                    self.focus = Some(("merit".into(), merit.len().to_string()));
                                    self.selected = None;
                                    ui.close();
                                }
                            }
                            let Some(detector) = &first_detector else {
                                ui.label(
                                    RichText::new("Add a detector first: terms are measured on one")
                                        .color(DARK.text_dim),
                                );
                                return;
                            };
                            for (kind, help) in [
                                ("centring", "the beam on the detector's centre (mm)"),
                                ("pointing", "the beam along the detector's axis (mrad)"),
                                ("collimation", "the beam collimated: no spread of directions (mrad)"),
                                ("diameter", "the beam this wide (mm; target 5 to start)"),
                                ("focus", "the smallest spot (mm)"),
                                ("power", "as much light as possible arriving"),
                                ("headroom", "adjusters near the middle of their ranges (small weight)"),
                            ] {
                                if ui.button(kind).on_hover_text(help).clicked() {
                                    let mut entry = vec![json!(["kind", kind])];
                                    if kind != "headroom" {
                                        entry.push(json!(["detector", detector]));
                                    }
                                    if kind == "diameter" {
                                        entry.push(json!(["target", 5.0]));
                                    }
                                    self.apply(vec![json!({
                                        "op": "add", "table": "merit", "entry": entry,
                                    })]);
                                    self.focus = Some(("merit".into(), merit.len().to_string()));
                                    self.selected = None;
                                    ui.close();
                                }
                            }
                        });
                        if !merit.is_empty() && scene.dofs.is_empty() {
                            ui.label(
                                RichText::new("Optimise needs degrees of freedom: add a slide or tilt to an item")
                                    .color(DARK.text_dim)
                                    .size(11.0),
                            );
                        }
                    });
                if !scene.dofs.is_empty() {
                    egui::CollapsingHeader::new(format!(
                        "Degrees of freedom ({})",
                        scene.dofs.len()
                    ))
                    .show(ui, |ui| {
                        for d in &scene.dofs {
                            ui.label(format!(
                                "{}  {:.4} {}",
                                d["name"].as_str().unwrap_or("?"),
                                d["value"].as_f64().unwrap_or(0.0),
                                d["unit"].as_str().unwrap_or("")
                            ));
                        }
                    });
                }
                let occurrences = scene.occurrences();
                egui::CollapsingHeader::new(format!("Mechanics ({})", occurrences.len())).show(
                    ui,
                    |ui| {
                        for (name, members) in occurrences {
                            egui::CollapsingHeader::new(name)
                                .id_salt(members[0])
                                .show(ui, |ui| {
                                    for i in members {
                                        row(
                                            ui,
                                            self,
                                            i,
                                            scene.items[i]
                                                .id
                                                .rsplit('/')
                                                .next()
                                                .unwrap_or("")
                                                .to_owned(),
                                        );
                                    }
                                });
                        }
                    },
                );
            });
    }

    fn properties(&mut self, ui: &mut egui::Ui) {
        // The drafts belong to the scene they were typed against.
        if let Some(doc) = &self.doc {
            if doc.scene_version != self.drafts_version {
                self.drafts.clear();
                self.drafts_version = doc.scene_version;
                self.library_mounts = None;
            }
        }
        if self.library_mounts.is_none() {
            let mut parameters = json!({});
            if let (Some(p), Some(d)) = (
                parameters.as_object_mut(),
                self.data_parameter().as_object(),
            ) {
                p.extend(d.clone());
            }
            let parts = optobench_commands::Registry::standard()
                .run("parts", &parameters)
                .ok()
                .and_then(|v| v["parts"].as_array().cloned())
                .unwrap_or_default();
            self.library_mounts =
                Some(parts.into_iter().filter(|p| p["type"] == "mount").collect());
        }
        let mut operations = Vec::new();
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                if let Layout::Stages { stages, .. } = WORKSPACES[self.workspace].layout {
                    let stage = self.stage.min(stages.len() - 1);
                    ui.label(RichText::new(stages[stage].name).strong().size(14.0));
                    ui.label(RichText::new(stages[stage].help).color(DARK.text_dim));
                    ui.separator();
                    self.ready_card(ui);
                    ui.separator();
                    operations = self.stage_editor(ui, stage);
                } else {
                    operations = self.selection_editor(ui);
                }
            });
        self.apply(operations);
    }

    /// Entries of one table, each in a collapsing section with its editor.
    fn table_editor(&mut self, ui: &mut egui::Ui, table: &str, title: &str) -> Vec<Value> {
        let Some(design) = self.design().cloned() else {
            return Vec::new();
        };
        let scene = self.doc.as_ref().and_then(|d| d.scene.clone());
        let library = self.library_mounts.clone().unwrap_or_default();
        let context = match &scene {
            Some(scene) => editors::Context::of(&design).with_places(scene),
            None => editors::Context::of(&design),
        }
        .with_mounts(&design, &library);
        let context = match &self.doc {
            Some(doc) => context.with_clearances(doc),
            None => context,
        };
        let mut operations = Vec::new();
        let list = design[table].as_array().cloned().unwrap_or_default();
        ui.label(RichText::new(format!("{title} ({})", list.len())).strong());
        for (i, entry) in list.iter().enumerate() {
            let name = entry["id"].as_str().map_or_else(
                || {
                    format!(
                        "{} {}",
                        entry["kind"].as_str().unwrap_or("term"),
                        entry["detector"].as_str().unwrap_or("")
                    )
                },
                str::to_owned,
            );
            egui::CollapsingHeader::new(name)
                .id_salt(format!("{table}-{i}"))
                .show(ui, |ui| {
                    let origin = scene
                        .as_ref()
                        .zip(entry["id"].as_str())
                        .and_then(|(scene, id)| origin_of(scene, id));
                    let lens = scene
                        .as_ref()
                        .zip(entry["id"].as_str())
                        .and_then(|(s, id)| {
                            s.items
                                .iter()
                                .find(|i| i.id == id)
                                .and_then(|i| i.lens.clone())
                        });
                    operations.extend(editors::entry(
                        ui,
                        &mut self.drafts,
                        table,
                        entry,
                        i,
                        &context,
                        origin,
                        lens.as_ref(),
                    ));
                });
        }
        operations
    }

    /// The Simulate workspace's editor for a stage.
    fn stage_editor(&mut self, ui: &mut egui::Ui, stage: usize) -> Vec<Value> {
        let Some(design) = self.design().cloned() else {
            return Vec::new();
        };
        let layout = design["layout"].is_string();
        let mut operations = Vec::new();
        let mut add = |ui: &mut egui::Ui, label: &str, new: New| {
            if layout && ui.button(label).clicked() {
                operations.extend(editors::add(&design, new));
            }
        };
        match stage {
            0 => {
                add(ui, "Add source", New::Source);
                let more = self.table_editor(ui, "source", "Sources");
                operations.extend(more);
            }
            1 => {
                ui.horizontal(|ui| {
                    add(ui, "Add lens", New::Lens);
                    add(ui, "Add mirror", New::Mirror);
                });
                if !layout {
                    ui.label(
                        RichText::new("Tag CAD bodies as lenses or mirrors in the CAD workspace")
                            .color(DARK.text_dim),
                    );
                }
                let more = self.table_editor(ui, "element", "Optics");
                operations.extend(more);
            }
            2 => {
                add(ui, "Add detector", New::Detector);
                let more = self.table_editor(ui, "detector", "Detectors");
                operations.extend(more);
                ui.separator();
                let first = editors::first_detector(&design);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Add merit").color(DARK.text_dim));
                    if let Some(ops) = editors::backscatter_term(&design) {
                        if ui.small_button("backscatter").clicked() {
                            operations.extend(ops);
                        }
                    }
                    for kind in [
                        "focus",
                        "centring",
                        "pointing",
                        "collimation",
                        "diameter",
                        "power",
                    ] {
                        if let Some(detector) = &first {
                            if ui.small_button(kind).clicked() {
                                // A diameter needs its target: 5 mm to start, set in its
                                // editor.
                                let mut entry =
                                    vec![json!(["kind", kind]), json!(["detector", detector])];
                                if kind == "diameter" {
                                    entry.push(json!(["target", 5.0]));
                                }
                                operations.push(json!({
                                    "op": "add", "table": "merit", "entry": entry,
                                }));
                            }
                        }
                    }
                });
                let more = self.table_editor(ui, "merit", "Merit");
                operations.extend(more);
            }
            _ => {
                ui.label(RichText::new("Runs").strong());
                self.runs_table(ui, 12);
            }
        }
        operations
    }

    /// The editor of what is selected in the view or the browser.
    fn selection_editor(&mut self, ui: &mut egui::Ui) -> Vec<Value> {
        ui.label(RichText::new("Properties").strong().size(14.0));
        ui.separator();
        let Some(doc) = &self.doc else {
            return Vec::new();
        };
        let Some(scene) = &doc.scene else {
            return Vec::new();
        };
        let design = scene.design.clone();
        let context = editors::Context::of(&design)
            .with_places(scene)
            .with_mounts(&design, self.library_mounts.as_deref().unwrap_or_default())
            .with_clearances(doc);
        if self.focus.as_ref().is_some_and(|(t, _)| t == "table") {
            if let Some(board) = crate::board::Board::of(&design, scene) {
                return editors::board(ui, &mut self.drafts, &board);
            }
        }
        // A source or merit term chosen in the browser.
        if let Some((table, key)) = self.focus.clone() {
            let found = if table == "merit" {
                key.parse::<usize>()
                    .ok()
                    .and_then(|i| Some((design["merit"].get(i)?.clone(), i)))
            } else {
                design[&table].as_array().and_then(|l| {
                    l.iter()
                        .enumerate()
                        .find(|(_, e)| e["id"] == key.as_str())
                        .map(|(i, e)| (e.clone(), i))
                })
            };
            if let Some((entry, i)) = found {
                ui.label(RichText::new(format!("{table} {key}")).color(DARK.text_dim));
                let origin = origin_of(scene, &key);
                return editors::entry(
                    ui,
                    &mut self.drafts,
                    &table,
                    &entry,
                    i,
                    &context,
                    origin,
                    None,
                );
            }
        }
        let Some(item) = self.selected.and_then(|i| scene.items.get(i)).cloned() else {
            ui.label(
                RichText::new("Select an item in the view or the browser").color(DARK.text_dim),
            );
            return Vec::new();
        };
        let mut operations = Vec::new();
        match entry_of(&design, &item.id) {
            Some((table, entry, i)) => {
                ui.label(RichText::new(format!("{table} · {}", item.role)).color(DARK.text_dim));
                operations = editors::entry(
                    ui,
                    &mut self.drafts,
                    &table,
                    &entry,
                    i,
                    &context,
                    item.origin,
                    item.lens.as_ref(),
                );
            }
            None => {
                egui::Grid::new("properties")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        for (k, v) in [
                            ("Id", item.id.clone()),
                            ("Kind", item.kind.clone()),
                            ("Role", item.role.clone()),
                            (
                                "Occurrence",
                                item.occurrence.clone().unwrap_or_else(|| "—".into()),
                            ),
                        ] {
                            ui.label(RichText::new(k).color(DARK.text_dim));
                            ui.label(v);
                            ui.end_row();
                        }
                    });
                if context.sidecar && item.role == "absorber" {
                    ui.label(
                        RichText::new(
                            "Tag Lens makes this body a lens; for Tag Mirror, click its reflecting face first",
                        )
                        .color(DARK.text_dim),
                    );
                    // A mount moving with its lens (M19): the part in the lens's group.
                    if let Some(occurrence) = item.occurrence.clone() {
                        let now = editors::group_of_part(&design, &occurrence);
                        let element_of = |g: &str| {
                            design["element"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .find(|e| e["group"] == g)
                                .and_then(|e| e["id"].as_str())
                                .map(str::to_owned)
                        };
                        let shown = now
                            .as_deref()
                            .and_then(element_of)
                            .unwrap_or_else(|| "none".into());
                        let elements: Vec<String> = design["element"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|e| e["id"].as_str().map(str::to_owned))
                            .collect();
                        let mut chosen: Option<Option<String>> = None;
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("moves with").color(DARK.text_dim))
                                .on_hover_text(format!(
                                    "The CAD part `{occurrence}` moves with an element: in its group, it follows the group's degrees of freedom (a lens's mount sliding along the rods with it)"
                                ));
                            egui::ComboBox::from_id_salt("moves-with")
                                .selected_text(&shown)
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(shown == "none", "none").clicked() {
                                        chosen = Some(None);
                                    }
                                    for e in &elements {
                                        if ui.selectable_label(&shown == e, e).clicked() {
                                            chosen = Some(Some(e.clone()));
                                        }
                                    }
                                });
                        });
                        if let Some(choice) = chosen {
                            if choice.as_deref() != Some(shown.as_str()) {
                                operations.extend(editors::move_with(
                                    &design,
                                    &occurrence,
                                    choice.as_deref(),
                                ));
                            }
                        }
                        if let Some(g) = &now {
                            ui.label(
                                RichText::new(format!(
                                    "In the group `{g}`: give the group a slide to move them together"
                                ))
                                .color(DARK.text_dim)
                                .size(11.0),
                            );
                        }
                    }
                }
            }
        }
        if item.role == "detector" {
            if let Some(d) = self.doc.as_ref().and_then(|doc| {
                doc.trace.as_ref()?.report["detectors"]
                    .as_array()?
                    .iter()
                    .find(|d| d["id"] == item.id.as_str())
                    .cloned()
            }) {
                ui.separator();
                ui.label(RichText::new("Last trace").strong());
                detector_grid(ui, &d);
            }
        }
        operations
    }

    /// The last runs, newest first: what changed, detected power, and per detector the
    /// centroid, RMS radius and spread.
    fn runs_table(&self, ui: &mut egui::Ui, rows: usize) {
        let Some(doc) = &self.doc else { return };
        if doc.runs.is_empty() {
            ui.label(RichText::new("Trace to start comparing runs").color(DARK.text_dim));
            return;
        }
        egui::Grid::new("runs")
            .num_columns(6)
            .spacing([10.0, 2.0])
            .striped(true)
            .show(ui, |ui| {
                for h in [
                    "#",
                    "after",
                    "detected",
                    "centroid (mm)",
                    "rms (mm)",
                    "spread (mrad)",
                ] {
                    ui.label(RichText::new(h).color(DARK.text_dim).size(11.0));
                }
                ui.end_row();
                for run in doc.runs.iter().rev().take(rows) {
                    ui.label(run.number.to_string());
                    ui.label(&run.after);
                    ui.label(format!("{:.2} %", 100.0 * run.detected));
                    match run.detectors.first() {
                        Some((_, c, rms, spread)) => {
                            ui.label(format!("{:.4}, {:.4}", c[0], c[1]));
                            ui.label(format!("{rms:.5}"));
                            ui.label(format!("{spread:.4}"));
                        }
                        None => {
                            ui.label("—");
                            ui.label("—");
                            ui.label("—");
                        }
                    }
                    ui.end_row();
                }
            });
    }

    /// The Ready-to-trace card of the Simulate workspace.
    fn ready_card(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("READY TO TRACE")
                .size(10.5)
                .color(DARK.text_dim),
        );
        let Some(doc) = &self.doc else {
            ui.label("Open a design first");
            return;
        };
        let mut notes: Vec<(bool, String)> = Vec::new();
        match &doc.scene {
            None => notes.push((false, "the scene is still being read".into())),
            Some(scene) => {
                notes.push((
                    !scene.sources.is_empty(),
                    format!("{} source(s)", scene.sources.len()),
                ));
                let detectors = scene.items.iter().filter(|i| i.role == "detector").count();
                notes.push((detectors > 0, format!("{detectors} detector(s)")));
                let optics = scene
                    .items
                    .iter()
                    .filter(|i| {
                        matches!(i.role.as_str(), "lens" | "mirror" | "objective" | "sample")
                    })
                    .count();
                notes.push((true, format!("{optics} optical item(s)")));
            }
        }
        let errors = doc.messages.iter().filter(|m| m.error).count();
        if errors > 0 {
            notes.push((false, format!("{errors} error message(s) below")));
        }
        for (ok, text) in notes {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                let colour = if ok {
                    Color32::from_rgb(127, 209, 174)
                } else {
                    DARK.warning
                };
                ui.painter().circle_filled(r.center(), 4.0, colour);
                ui.label(RichText::new(text).color(if ok { DARK.text } else { DARK.warning }));
            });
        }
    }

    fn results(&mut self, ui: &mut egui::Ui) {
        if let Some((error, text)) = &self.notice {
            ui.label(RichText::new(text).color(if *error { DARK.error } else { DARK.text }));
        }
        if self.doc.is_none() {
            ui.label(RichText::new("Results appear here after a trace").color(DARK.text_dim));
            return;
        }
        ui.horizontal_top(|ui| {
            self.graphs_panel(ui);
            ui.separator();
            self.result_columns(ui);
        });
    }

    /// The graphs: one shown at a time, a switcher between those open, a mark on graphs
    /// drawn before the design last changed, Redraw and Close.
    fn graphs_panel(&mut self, ui: &mut egui::Ui) {
        let App {
            doc, graph_cursor, ..
        } = self;
        let Some(doc) = doc else { return };
        let mut hovered = None;
        let mut redraw: Option<String> = None;
        let mut close = None;
        // Half the strip for a graph; little until one is open.
        let width = if doc.graphs.is_empty() {
            240.0
        } else {
            (ui.available_width() * 0.45).max(380.0)
        };
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Graphs").strong());
                if doc.graphs.is_empty() {
                    return;
                }
                let shown = doc.shown_graph.min(doc.graphs.len() - 1);
                egui::ComboBox::new("open graphs", "")
                    .selected_text(doc.graphs[shown].graph.title.clone())
                    .show_ui(ui, |ui| {
                        for i in 0..doc.graphs.len() {
                            let title = doc.graphs[i].graph.title.clone();
                            let stale = doc.out_of_date(&doc.graphs[i]);
                            let label = if stale {
                                format!("{title} · out of date")
                            } else {
                                title
                            };
                            ui.selectable_value(&mut doc.shown_graph, i, label);
                        }
                    });
                let graph = &doc.graphs[shown];
                if doc.out_of_date(graph) {
                    // The reload icon with a warning dot: drawn before the last change.
                    let (rect, response) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                    crate::icons::draw(ui.painter(), rect, "reload", true);
                    ui.painter()
                        .circle_filled(rect.right_top() + Vec2::new(-2.0, 2.0), 3.0, DARK.warning);
                    response.on_hover_text("Out of date: the design changed after this graph was drawn");
                }
                if ui
                    .small_button("Redraw")
                    .on_hover_text("Draw this graph again from the design as it is now")
                    .clicked()
                {
                    redraw = Some(graph.kind.clone());
                }
                if ui.small_button("×").on_hover_text("Close this graph").clicked() {
                    close = Some(shown);
                }
            });
            if doc.graphs.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new("Graph draws the graphs you choose; scroll to zoom, drag to pan, double-click to fit")
                            .color(DARK.text_dim),
                    )
                    .wrap(),
                );
                return;
            }
            let shown = doc.shown_graph.min(doc.graphs.len() - 1);
            let height = ui.available_height().max(150.0);
            let open = &mut doc.graphs[shown];
            if let Some(c) = crate::graphs::show(
                ui,
                &open.graph,
                &mut open.view,
                *graph_cursor,
                Vec2::new(width, height),
            ) {
                hovered = Some(c);
            }
        });
        *graph_cursor = hovered;
        if let Some(kind) = redraw {
            doc.draw_graphs(&[kind]);
        }
        if let Some(i) = close {
            doc.graphs.remove(i);
            doc.shown_graph = doc.shown_graph.min(doc.graphs.len().saturating_sub(1));
        }
    }

    /// The Graph button's chooser: tick the graphs to draw, all at once.
    fn graph_dialog(&mut self, ctx: &egui::Context) {
        if !self.graph_dialog {
            return;
        }
        let mut open = true;
        let mut draw = false;
        egui::Window::new("Graphs to draw")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                for (id, name) in crate::graphs::KINDS {
                    let mut on = self.graph_choice.contains(id);
                    if ui.checkbox(&mut on, *name).changed() {
                        if on {
                            self.graph_choice.push(id);
                        } else {
                            self.graph_choice.retain(|k| k != id);
                        }
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let some = !self.graph_choice.is_empty();
                    if ui.add_enabled(some, egui::Button::new("Draw")).clicked() {
                        draw = true;
                    }
                    if ui.button("Cancel").clicked() {
                        self.graph_dialog = false;
                    }
                });
            });
        if draw {
            // In the list's order, so the first ticked is shown.
            let kinds: Vec<String> = crate::graphs::KINDS
                .iter()
                .filter(|(id, _)| self.graph_choice.contains(id))
                .map(|(id, _)| (*id).to_owned())
                .collect();
            if let Some(doc) = &mut self.doc {
                doc.draw_graphs(&kinds);
            }
            self.graph_dialog = false;
        }
        if !open {
            self.graph_dialog = false;
        }
    }

    /// Messages, the power ledger, detectors, runs and the last command's output.
    fn result_columns(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = &self.doc else { return };
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(300.0);
                ui.label(RichText::new("Messages").strong());
                for m in doc.messages.iter().rev().take(6) {
                    ui.label(RichText::new(&m.text).color(if m.error {
                        DARK.error
                    } else {
                        DARK.text
                    }));
                }
            });
            ui.separator();
            if let Some(trace) = &doc.trace {
                ui.vertical(|ui| {
                    ui.set_width(210.0);
                    ui.label(RichText::new("Power ledger (W)").strong());
                    egui::Grid::new("ledger").num_columns(2).show(ui, |ui| {
                        if let Some(ledger) = trace.report["ledger"].as_object() {
                            for (k, v) in ledger {
                                ui.label(
                                    RichText::new(k.trim_end_matches("_w")).color(DARK.text_dim),
                                );
                                ui.label(format!("{:.4e}", v.as_f64().unwrap_or(0.0)));
                                ui.end_row();
                            }
                        }
                    });
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_max_width(300.0);
                    ui.label(RichText::new("Detectors").strong());
                    for d in trace.report["detectors"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                    {
                        ui.label(
                            RichText::new(d["id"].as_str().unwrap_or("?")).color(DARK.text_dim),
                        );
                        detector_grid(ui, &d);
                    }
                    let irises = &trace.report["alignment_irises"];
                    if let Some(list) = irises["irises"].as_array() {
                        ui.add_space(4.0);
                        ui.label(RichText::new("Alignment irises").strong())
                            .on_hover_text("Where to put irises on the bench to align this run; not part of the design");
                        for (k, i) in list.iter().enumerate() {
                            let hole = &i["hole"];
                            ui.label(format!(
                                "I{}  hole ({}, {}) · {} mm high{}",
                                k + 1,
                                hole[0],
                                hole[1],
                                i["height"].as_f64().unwrap_or(0.0),
                                if i["on_grid"] == true { "" } else { " · off the grid" }
                            ));
                        }
                        ui.add(
                            egui::Label::new(
                                RichText::new(irises["advice"].as_str().unwrap_or(""))
                                    .color(DARK.text_dim)
                                    .size(11.0),
                            )
                            .wrap(),
                        );
                    }
                });
            }
            if !doc.runs.is_empty() {
                ui.separator();
                ui.vertical(|ui| {
                    ui.label(RichText::new("Runs").strong());
                    egui::ScrollArea::vertical()
                        .id_salt("runs")
                        .max_height(140.0)
                        .show(ui, |ui| self.runs_table(ui, 20));
                });
            }
            if let Some((command, value)) = &doc.output {
                ui.separator();
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("Output of {command}")).strong());
                    egui::ScrollArea::vertical()
                        .id_salt("output")
                        .max_height(140.0)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(
                                    serde_json::to_string_pretty(value).unwrap_or_default(),
                                )
                                .monospace()
                                .size(10.5),
                            );
                        });
                });
            }
        });
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm else { return };
        let target = self.doc.as_ref().map(|d| d.name()).unwrap_or_default();
        let response = egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
            ui.label(
                RichText::new(Self::label(Control::Command(id)))
                    .strong()
                    .size(14.0),
            );
            ui.label(format!(
                "{}. It writes files next to {target}.",
                Self::summary(Control::Command(id))
            ));
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    return Some(false);
                }
                if ui
                    .add(
                        egui::Button::new(RichText::new("Continue").color(Color32::WHITE))
                            .fill(self.accent().fill),
                    )
                    .clicked()
                {
                    return Some(true);
                }
                None
            })
            .inner
        });
        match response.inner {
            Some(true) => {
                self.confirm = None;
                self.run(id);
            }
            Some(false) => self.confirm = None,
            None if response.should_close() => self.confirm = None,
            None => {}
        }
    }
}

fn detector_grid(ui: &mut egui::Ui, d: &Value) {
    egui::Grid::new(format!("detector-{}", d["id"]))
        .num_columns(2)
        .show(ui, |ui| {
            let n = |k: &str| d[k].as_f64().unwrap_or(f64::NAN);
            let pair = |k: &str| {
                let a = &d[k];
                format!(
                    "{:.4}, {:.4}",
                    a[0].as_f64().unwrap_or(f64::NAN),
                    a[1].as_f64().unwrap_or(f64::NAN)
                )
            };
            for (k, v) in [
                ("power", format!("{:.4e} W", n("power_w"))),
                ("direct", format!("{:.4e} W", n("direct_power_w"))),
                ("centroid", format!("{} mm", pair("direct_centroid_mm"))),
                ("rms radius", format!("{:.5} mm", n("direct_rms_radius_mm"))),
                (
                    "pointing",
                    format!("{} mrad", pair("direct_mean_slope_mrad")),
                ),
                ("spread", format!("{:.4} mrad", n("direct_rms_slope_mrad"))),
            ] {
                ui.label(RichText::new(k).color(DARK.text_dim));
                ui.label(v);
                ui.end_row();
            }
        });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(doc) = &mut self.doc {
            doc.poll();
        }
        self.poll_live();
        // A design dropped on the window opens.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        for path in dropped {
            self.dropped(path);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&OPEN)) {
            self.activate(Control::Action(Action::Open));
        }

        egui::Panel::top("app_bar")
            .exact_size(36.0)
            .frame(Frame::new().fill(DARK.app_bar))
            .show(ui, |ui| self.app_bar(ui));
        egui::Panel::top("ribbon")
            .exact_size(100.0)
            .frame(
                Frame::new()
                    .fill(DARK.panel)
                    .inner_margin(Margin::symmetric(4, 6)),
            )
            .show(ui, |ui| self.ribbon(ui));
        egui::Panel::bottom("results")
            .resizable(true)
            .default_size(240.0)
            .frame(Frame::new().fill(DARK.panel).inner_margin(Margin::same(8)))
            .show(ui, |ui| self.results(ui));
        egui::Panel::left("browser")
            .resizable(true)
            .default_size(270.0)
            .frame(Frame::new().fill(DARK.panel).inner_margin(Margin::same(8)))
            .show(ui, |ui| self.browser(ui));
        egui::Panel::right("properties")
            .resizable(true)
            .default_size(280.0)
            .frame(Frame::new().fill(DARK.panel).inner_margin(Margin::same(8)))
            .show(ui, |ui| self.properties(ui));
        let accent = self.accent();
        // The icon review sheet (OPTOBENCH_UI_ICON_SHEET): every icon at 64, 32, 24 and 16.
        if std::env::var_os("OPTOBENCH_UI_ICON_SHEET").is_some() {
            egui::CentralPanel::default().show(ui, |ui| {
                egui::Grid::new("icon-sheet")
                    .spacing([14.0, 10.0])
                    .show(ui, |ui| {
                        for (k, id) in crate::icons::ALL.iter().enumerate() {
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    for size in [64.0, 32.0, 24.0, 16.0] {
                                        let (rect, _) = ui
                                            .allocate_exact_size(Vec2::splat(size), Sense::hover());
                                        crate::icons::draw(ui.painter(), rect, id, true);
                                    }
                                });
                                ui.label(RichText::new(*id).size(10.0).color(DARK.text_dim));
                            });
                            if k % 5 == 4 {
                                ui.end_row();
                            }
                        }
                    });
            });
            self.screenshot(&ctx);
            return;
        }
        let requests = egui::CentralPanel::no_frame()
            .show(ui, |ui| {
                // Fit once the scene has arrived, with the view's own aspect.
                if self.fit_pending {
                    let scene = self.doc.as_ref().and_then(|d| d.scene.as_ref());
                    let board = scene
                        .and_then(|s| crate::board::Board::of(&s.design, s))
                        .filter(|_| self.show_board);
                    let bounds = match board {
                        Some(board) => Some(board.around(scene.and_then(|s| s.bounds))),
                        None => scene.and_then(|s| s.bounds),
                    };
                    if let Some((low, high)) = bounds {
                        let size = ui.available_size();
                        self.camera
                            .fit(low, high, (size.x / size.y.max(1.0)) as f64);
                        self.fit_pending = false;
                    }
                }
                self.view.board = self
                    .doc
                    .as_ref()
                    .and_then(|d| d.scene.as_ref())
                    .and_then(|s| crate::board::Board::of(&s.design, s));
                self.view.show_board = self.show_board;
                self.view.placing = self.placing.clone().and_then(|(new, part)| {
                    let design = self.design()?;
                    Some(viewport::Placing {
                        what: match (new, part) {
                            (New::Lens, Some(key)) => key,
                            (New::Lens, None) => "Lens".into(),
                            (New::Mirror, _) => "Mirror".into(),
                            (New::Source, _) => "Source".into(),
                            (New::Detector, _) => "Detector".into(),
                            (New::Iris, _) => "Iris".into(),
                            (New::Tube, _) => "Tube".into(),
                        },
                        height: editors::new_height(design),
                        pitch: design["table"]["pitch"].as_f64()?,
                    })
                });
                self.view.moving = self.move_on.then(|| {
                    let mut tool = self.move_tool.clone();
                    let scene = self.doc.as_ref().and_then(|d| d.scene.as_ref());
                    tool.pitch = scene.and_then(|s| s.design["table"]["pitch"].as_f64());
                    let moving = scene.and_then(|s| movable(s, self.selected, self.focus.as_ref()));
                    tool.target = moving.as_ref().map(|m| (m.anchor, m.outline.clone()));
                    // The posts that follow the move: every member's.
                    tool.items = moving.as_ref().map_or_else(Vec::new, |m| m.items.clone());
                    tool.sources = moving.as_ref().map_or_else(Vec::new, |m| m.sources.clone());
                    tool.posts = moving
                        .as_ref()
                        .map_or_else(Vec::new, |m| m.members.iter().map(|x| x.3).collect());
                    tool.label = moving.as_ref().and_then(|m| {
                        m.group
                            .as_ref()
                            .map(|g| format!("group {g}: {} items", m.members.len()))
                    });
                    if tool.target.is_none() {
                        tool.pending.clear();
                    }
                    tool
                });
                viewport::show(
                    ui,
                    &mut self.camera,
                    &mut self.view,
                    self.doc.as_ref(),
                    self.selected,
                    &self.hidden,
                    self.show_rays,
                    accent,
                )
            })
            .inner;
        // The Move tool's steps and drag live on in the app.
        if let Some(tool) = self.view.moving.take() {
            self.move_tool = tool;
        }
        for request in requests {
            match request {
                Request::Select(index) => {
                    // While moving, a click beside the handles keeps what is selected, and
                    // nothing else is selected while a move is pending.
                    let pending = self.move_on && !self.move_tool.pending.is_empty();
                    if !(self.move_on && index.is_none()) && !pending {
                        self.selected = index;
                        self.focus = None;
                    }
                }
                Request::Fit => self.fit_pending = true,
                Request::MoveApply(steps) => {
                    let operations = self
                        .doc
                        .as_ref()
                        .and_then(|d| d.scene.as_ref())
                        .and_then(|s| {
                            let pitch = s.design["table"]["pitch"].as_f64()?;
                            let m = movable(s, self.selected, self.focus.as_ref())?;
                            Some(editors::moved_group(&m.members, pitch, m.anchor, &steps))
                        })
                        .unwrap_or_default();
                    self.apply(operations);
                    self.end_move();
                }
                Request::MoveCancel => self.end_move(),
                Request::Place(hole) => {
                    if let Some((new, part)) = self.placing.take() {
                        if let Some(op) = self
                            .design()
                            .and_then(|d| editors::add_at(d, new, hole, part.as_deref()))
                        {
                            self.apply(vec![op]);
                        }
                    }
                }
                Request::PlaceCancel => self.placing = None,
            }
        }
        self.confirm_dialog(&ctx);
        self.licence_dialog(&ctx);
        self.graph_dialog(&ctx);
        self.new_dialog(&ctx);
        self.lens_chooser(&ctx);
        self.confirm_write_window(&ctx);
        self.lens_import_dialog(&ctx);
        self.screenshot(&ctx);
    }
}

impl App {
    /// Takes the screenshot asked for in the environment, once the design is ready.
    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.shot else { return };
        if shot.done.load(std::sync::atomic::Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let Some(doc) = &mut self.doc else { return };
        if doc.scene.is_none() || !doc.running.is_empty() {
            ctx.request_repaint();
            return;
        }
        if shot.trace && !shot.traced {
            shot.traced = true;
            let ctx2 = ctx.clone();
            doc.run("trace", json!({}), move || ctx2.request_repaint());
            return;
        }
        // One graph at a time, so they arrive in order.
        if !shot.graphs.is_empty() {
            let of = shot.graphs.remove(0);
            let ctx2 = ctx.clone();
            doc.run("graph", json!({ "of": of }), move || ctx2.request_repaint());
            return;
        }
        if let Some(id) = shot.select.take() {
            self.selected = doc
                .scene
                .as_ref()
                .and_then(|s| s.items.iter().position(|i| i.id == id));
        }
        if !shot.viewed {
            shot.viewed = true;
            let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
            match env("OPTOBENCH_UI_VIEW").as_deref() {
                Some("top") => self.camera.top(),
                Some("front") => self.camera.front(),
                Some("right") => self.camera.right(),
                Some("iso") => self.camera.isometric(),
                _ => {}
            }
            self.camera.perspective = env("OPTOBENCH_UI_PERSPECTIVE").is_some();
            // The selected item's box, else the scene's.
            let scene = doc.scene.as_ref();
            let item_box = self
                .selected
                .and_then(|i| scene?.items.get(i)?.mesh.as_ref())
                .map(|m| (m.bounds.0.map(f64::from), m.bounds.1.map(f64::from)));
            if let Some((low, high)) = item_box.or(scene.and_then(|s| s.bounds)) {
                let centre = [0, 1, 2].map(|k| 0.5 * (low[k] + high[k]));
                if let Some(zoom) = env("OPTOBENCH_UI_ZOOM").and_then(|z| z.parse::<f64>().ok()) {
                    let size = (0..3).map(|k| high[k] - low[k]).fold(0.0, f64::max);
                    self.camera.target = centre;
                    self.camera.height = (size * zoom).max(1e-3);
                }
                if let Some(axis) = env("OPTOBENCH_UI_SECTION").and_then(|a| match a.as_str() {
                    "x" => Some(0),
                    "y" => Some(1),
                    "z" => Some(2),
                    _ => None,
                }) {
                    self.view.section = Some((axis, centre[axis]));
                }
            }
        }
        shot.frames += 1;
        ctx.request_repaint();
        if shot.frames == 10 && !shot.requested {
            shot.requested = true;
            let (path, done, ctx2) = (shot.path.clone(), shot.done.clone(), ctx.clone());
            ctx.request_screenshot(move |image| {
                if let Err(e) = crate::screenshot::write_bmp(&image, &path) {
                    eprintln!("{}: {e}", path.display());
                }
                done.store(true, std::sync::atomic::Ordering::SeqCst);
                ctx2.request_repaint();
            });
        }
    }
}

/// optobench's mark in the title bar: a lens outline with the axis through it.
fn mark(ui: &mut egui::Ui, size: f32, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size + 4.0), Sense::hover());
    let c = rect.center();
    ui.painter()
        .circle_stroke(c, size * 0.42, Stroke::new(1.8, colour));
    ui.painter().line_segment(
        [
            c - Vec2::new(size * 0.6, 0.0),
            c + Vec2::new(size * 0.6, 0.0),
        ],
        Stroke::new(1.8, colour),
    );
}

/// A small colour square, for an item's role in the browser.
fn swatch(ui: &mut egui::Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter()
        .rect_filled(rect.shrink(1.0), CornerRadius::same(2), colour);
}

/// A downward chevron, after a label that opens a menu.
fn chevron(painter: &egui::Painter, at: egui::Pos2, colour: Color32) {
    let stroke = Stroke::new(1.2, colour);
    painter.line_segment(
        [at + Vec2::new(-3.0, -1.5), at + Vec2::new(0.0, 1.5)],
        stroke,
    );
    painter.line_segment(
        [at + Vec2::new(0.0, 1.5), at + Vec2::new(3.0, -1.5)],
        stroke,
    );
}

/// The M8 exit test (docs/milestones/M8-interface.md): the window driven through its own
/// buttons and fields, as a person would, with `egui_kittest` (no screen, no GPU).
#[cfg(test)]
mod exit {
    use std::path::{Path, PathBuf};

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::{Value, json};

    use super::App;

    fn scratch(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("optobench-exit-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn window(path: Option<PathBuf>, data: &Path) -> Harness<'static, App> {
        let data = data.to_path_buf();
        Harness::builder()
            .with_size(egui::Vec2::new(1440.0, 900.0))
            .build_eframe(move |cc| App::new(path, Some(data), None, cc.egui_ctx.clone()))
    }

    /// A file dialog that always answers `path`.
    fn answer(path: &Path) -> super::Chooser {
        let path = path.to_path_buf();
        Box::new(move |_| Some(path.clone()))
    }

    /// Runs frames until the background commands are done.
    fn settle(h: &mut Harness<'static, App>) {
        let busy = |h: &Harness<'static, App>| {
            h.state()
                .doc
                .as_ref()
                .is_some_and(|d| !d.running.is_empty())
        };
        for _ in 0..6000 {
            h.step();
            if !busy(h) {
                h.step();
                h.step();
                if !busy(h) {
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!(
            "still running: {:?}",
            h.state().doc.as_ref().map(|d| &d.running)
        );
    }

    /// Clicks the first widget with this label and waits for what it started.
    fn click(h: &mut Harness<'static, App>, label: &str) {
        h.step();
        h.query_all_by_label(label)
            .next()
            .unwrap_or_else(|| panic!("no `{label}` on the screen"))
            .click();
        h.step();
        h.step();
        settle(h);
    }

    fn errors(h: &Harness<'static, App>) -> Vec<String> {
        h.state()
            .doc
            .as_ref()
            .map(|d| {
                d.messages
                    .iter()
                    .filter(|m| m.error)
                    .map(|m| m.text.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn trace(path: &Path, data: &Path) -> Value {
        optobench_commands::Registry::standard()
            .run(
                "trace",
                &json!({ "path": path.to_string_lossy(), "data_directory": data.to_string_lossy() }),
            )
            .expect("traces")
    }

    #[test]
    fn open_and_new_ask_where_in_a_file_dialog() {
        // Open (the button, and ⌘O / Ctrl+O) opens what the dialog chose; a cancelled
        // dialog changes nothing; New saves where the dialog says, adding `.optobench` to
        // a name without it, and replaces a file there (the dialog asked already).
        let dir = scratch("dialogs");
        let first = dir.join("first");
        let mut h = window(None, &dir);
        click(&mut h, "Workspace");
        click(&mut h, "Layout");
        h.state_mut().chooser = Box::new(|_| None);
        click(&mut h, "New");
        click(&mut h, "Planar");
        assert!(h.state().doc.is_none());
        h.state_mut().chooser = answer(&first);
        click(&mut h, "New");
        click(&mut h, "Planar");
        let made = dir.join("first.optobench");
        assert!(made.is_file(), "New added the extension");
        assert_eq!(
            h.state().doc.as_ref().map(|d| d.path.clone()),
            Some(made.clone())
        );
        let second = dir.join("second.optobench");
        std::fs::write(&second, "old").expect("written");
        h.state_mut().chooser = answer(&second);
        click(&mut h, "New");
        click(&mut h, "Planar");
        let text = std::fs::read_to_string(&second).expect("read");
        assert!(text.contains("layout = \"planar\""), "replaced: {text}");
        // Open, by the button and by the shortcut.
        h.state_mut().chooser = answer(&made);
        click(&mut h, "Open");
        assert_eq!(
            h.state().doc.as_ref().map(|d| d.path.clone()),
            Some(made.clone())
        );
        h.state_mut().chooser = answer(&second);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::O);
        h.step();
        assert_eq!(h.state().doc.as_ref().map(|d| d.path.clone()), Some(second));
        h.state_mut().chooser = Box::new(|_| None);
        click(&mut h, "Open");
        assert!(h.state().doc.is_some(), "a cancelled Open keeps the design");
    }

    #[test]
    fn new_starts_a_table_layout_on_a_breadboard() {
        let dir = scratch("new-table");
        let path = dir.join("bench.optobench");
        let mut h = window(None, &dir);
        click(&mut h, "Workspace");
        click(&mut h, "Layout");
        h.state_mut().chooser = answer(&path);
        click(&mut h, "New");
        click(&mut h, "1 in");
        click(&mut h, "Table");
        let text = std::fs::read_to_string(&path).expect("New made the file");
        assert!(
            text.contains("layout = \"table\"") && text.contains("pitch = 25.4"),
            "{text}"
        );
        // Source waits for a hole; Enter puts it where Add always did (hole (0, 0)).
        click(&mut h, "Source");
        h.key_press(egui::Key::Enter);
        h.step();
        settle(&mut h);
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(text.contains("hole = [0, 0]"), "{text}");
        // Mirror: a click on the board over hole (3, 2) puts it there.
        click(&mut h, "Mirror");
        let at = {
            let app = h.state();
            let rect = app.view.rect.expect("drawn");
            let (x, y) = app.camera.to_screen(
                [3.0 * 25.4, 2.0 * 25.4, 0.0],
                (rect.center().x as f64, rect.center().y as f64),
                rect.height() as f64,
            );
            egui::pos2(x as f32, y as f32)
        };
        h.event(egui::Event::PointerMoved(at));
        h.step();
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
        }
        settle(&mut h);
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(text.contains("hole = [3, 2]"), "{text}");
        assert!(h.state().placing.is_none());
        // A lens's STEP file dropped on the window becomes a library lens (the golden
        // plano-convex lens), placed from Lens's chooser.
        let step = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/optobench-io/tests/step/lens.step");
        h.state_mut().dropped(step);
        h.step();
        {
            let import = h.state_mut().lens_import.as_mut().expect("the dialog");
            assert_eq!(import.found["radii_mm"], json!([30.0, "inf"]));
            import.vendor = "Example".into();
            import.part = "PC-30".into();
            import.glasses = vec!["1.5".into()];
        }
        click(&mut h, "Import");
        assert!(h.state().lens_import.is_none(), "{:?}", h.state().notice);
        click(&mut h, "Lens");
        click(&mut h, "example/PC-30");
        h.key_press(egui::Key::Enter);
        h.step();
        settle(&mut h);
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(text.contains("library = \"example/PC-30\""), "{text}");
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
        // Escape places nothing.
        click(&mut h, "Lens");
        click(&mut h, "Singlet");
        h.key_press(egui::Key::Escape);
        h.step();
        settle(&mut h);
        assert_eq!(std::fs::read_to_string(&path).expect("read"), text);
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
        // The board is listed, and its Properties give the pitch.
        click(&mut h, "Breadboard");
        assert!(
            h.query_by_label("Keep this size").is_some(),
            "the board's Properties"
        );
    }

    #[test]
    fn merit_and_lens_types_are_set_in_the_layout_workspace() {
        // A table layout: a source, a singlet, a detector. A collimation term added from
        // the browser; the singlet (biconvex R ±50, n 1.5, 4 mm: f 50.67 mm) made
        // plano-concave keeps |f| and becomes R −25.3 then flat; Flip puts the flat first.
        let dir = scratch("merit-lens");
        let path = dir.join("bench.optobench");
        let mut h = window(None, &dir);
        click(&mut h, "Workspace");
        click(&mut h, "Layout");
        h.state_mut().chooser = answer(&path);
        click(&mut h, "New");
        click(&mut h, "Table");
        for add in ["Source", "Lens", "Detector"] {
            click(&mut h, add);
            if add == "Lens" {
                click(&mut h, "Singlet");
            }
            h.key_press(egui::Key::Enter);
            h.step();
            settle(&mut h);
        }
        click(&mut h, "Add a term");
        click(&mut h, "collimation");
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(
            text.contains("kind = \"collimation\"\ndetector = \"D1\""),
            "{text}"
        );
        click(&mut h, "L1  ·  lens");
        // The lens's type: the first list in its Properties.
        h.query_all_by_role(egui::accesskit::Role::ComboBox)
            .next()
            .expect("the type list")
            .click();
        h.step();
        h.step();
        click(&mut h, "plano-concave");
        let text = std::fs::read_to_string(&path).expect("written");
        let f = 1.0 / (0.5 * (2.0 / 50.0 - 0.5 * 4.0 / (1.5 * 2500.0)));
        let r = format!("{:.4}", -0.5 * f);
        assert!(
            text.contains(&format!(
                "surfaces = [{{ radius = {r} }}, {{ radius = \"inf\" }}]"
            )),
            "{r}: {text}"
        );
        click(&mut h, "Flip");
        let text = std::fs::read_to_string(&path).expect("written");
        let flipped = format!("{:.4}", 0.5 * f);
        assert!(
            text.contains(&format!(
                "surfaces = [{{ radius = \"inf\" }}, {{ radius = {flipped} }}]"
            )),
            "{text}"
        );
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
    }

    #[test]
    fn a_group_moves_as_one() {
        // Two lenses in the group `expander`, a detector outside it: Move on L1, its x
        // arrow dragged two holes, Enter: both lenses move two holes, the detector not.
        let dir = scratch("group");
        let path = dir.join("expander.optobench");
        std::fs::write(
            &path,
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"table\"\n\n[table]\npitch = 25.0\n\n\
             [[source]]\nid = \"laser\"\nkind = \"collimated\"\nwavelength_um = 0.633\npower_w = 0.001\ndiameter = 1.0\nhole = [0, 0]\nheight = 75.0\naxis = [1, 0, 0]\n\n\
             [[element]]\nid = \"L1\"\nkind = \"ideal_lens\"\nfocal_length = -12\ndiameter = 10\nhole = [2, 0]\nheight = 75.0\naxis = [1, 0, 0]\ngroup = \"expander\"\n\n\
             [[element]]\nid = \"L2\"\nkind = \"ideal_lens\"\nfocal_length = 60\ndiameter = 25\nhole = [4, 0]\nheight = 75.0\naxis = [1, 0, 0]\ngroup = \"expander\"\n\n\
             [[detector]]\nid = \"out\"\nhole = [10, 0]\nheight = 75.0\naxis = [1, 0, 0]\nsize = [20, 20]\npixels = [20, 20]\n",
        )
        .expect("written");
        let mut h = window(Some(path.clone()), &dir);
        settle(&mut h);
        assert!(
            h.query_by_label("expander  ·  L1, L2").is_some(),
            "the browser lists it"
        );
        click(&mut h, "Move");
        click(&mut h, "expander  ·  L1, L2");
        h.state_mut().camera.top();
        h.step();
        h.step();
        let (centre, pitch) = {
            let app = h.state();
            let rect = app.view.rect.expect("drawn");
            let scene = app
                .doc
                .as_ref()
                .and_then(|d| d.scene.as_ref())
                .expect("a scene");
            let i = app.selected.expect("L1 selected");
            assert_eq!(scene.items[i].id, "L1");
            let origin = scene.items[i].origin.expect("placed");
            let (x, y) = app.camera.to_screen(
                origin,
                (rect.center().x as f64, rect.center().y as f64),
                rect.height() as f64,
            );
            (
                egui::pos2(x as f32, y as f32),
                25.0 * rect.height() / app.camera.height as f32,
            )
        };
        let from = centre + egui::vec2(40.0, 0.0);
        let to = from + egui::vec2(2.1 * pitch, 0.0);
        h.event(egui::Event::PointerMoved(from));
        h.step();
        h.event(egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
        for k in 1..=10 {
            h.event(egui::Event::PointerMoved(
                from + (to - from) * (k as f32 / 10.0),
            ));
            h.step();
        }
        h.event(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
        assert_eq!(
            h.state().move_tool.label.as_deref(),
            Some("group expander: 2 items")
        );
        h.key_press(egui::Key::Enter);
        h.step();
        settle(&mut h);
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(
            text.contains("hole = [4, 0]") && text.contains("hole = [6, 0]"),
            "{text}"
        );
        assert!(
            text.contains("hole = [10, 0]"),
            "the detector stays: {text}"
        );
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
    }

    #[test]
    fn an_item_is_moved_and_turned_by_dragging_its_handles() {
        // The table periscope's screen at hole (8, 0): Move on, its x arrow dragged
        // about two pitches to the right (seen from above) puts it at hole (10, 0); its
        // ring about z dragged a quarter turn turns its axis from +x to +y.
        let dir = scratch("move");
        let path = dir.join("periscope.optobench");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/table-periscope.optobench"),
            &path,
        )
        .expect("copied");
        let mut h = window(Some(path.clone()), &dir);
        settle(&mut h);
        click(&mut h, "Move");
        let index = h
            .state()
            .doc
            .as_ref()
            .and_then(|d| d.scene.as_ref())
            .and_then(|s| s.items.iter().position(|i| i.id == "screen"))
            .expect("the screen");
        h.state_mut().selected = Some(index);
        h.state_mut().camera.top();
        h.step();
        h.step();
        let drag = |h: &mut Harness<'static, App>, from: egui::Pos2, to: egui::Pos2| {
            h.event(egui::Event::PointerMoved(from));
            h.step();
            h.event(egui::Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
            for k in 1..=10 {
                h.event(egui::Event::PointerMoved(
                    from + (to - from) * (k as f32 / 10.0),
                ));
                h.step();
            }
            h.event(egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
            settle(h);
        };
        let at = |h: &Harness<'static, App>| {
            let app = h.state();
            let rect = app.view.rect.expect("drawn");
            let scene = app
                .doc
                .as_ref()
                .and_then(|d| d.scene.as_ref())
                .expect("a scene");
            let origin = scene.items[index].origin.expect("placed");
            let (x, y) = app.camera.to_screen(
                origin,
                (rect.center().x as f64, rect.center().y as f64),
                rect.height() as f64,
            );
            (
                egui::pos2(x as f32, y as f32),
                25.0 * rect.height() / app.camera.height as f32,
            )
        };
        let (centre, pitch) = at(&h);
        let before = std::fs::read_to_string(&path).expect("read");
        let start = centre + egui::vec2(40.0, 0.0);
        drag(&mut h, start, start + egui::vec2(2.1 * pitch, 0.0));
        // The drag is pending: the handles moved with it, the file did not.
        assert_eq!(std::fs::read_to_string(&path).expect("read"), before);
        assert_eq!(h.state().move_tool.pending.len(), 1);
        // On the ring at 45° about where the screen now is, clear of the arrows, a quarter
        // turn counter-clockwise.
        let moved = centre + egui::vec2(2.0 * pitch, 0.0);
        let r = 56.0 * std::f32::consts::FRAC_1_SQRT_2;
        drag(
            &mut h,
            moved + egui::vec2(r, -r),
            moved + egui::vec2(-r, -r),
        );
        assert_eq!(std::fs::read_to_string(&path).expect("read"), before);
        // Enter writes both and ends Move.
        h.key_press(egui::Key::Enter);
        h.step();
        settle(&mut h);
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(text.contains("hole = [10, 0]"), "{text}");
        assert!(text.contains("axis = [0, 1, 0]"), "{text}");
        assert!(!h.state().move_on, "Enter ends Move");
        // Escape drops a move: the file stays as it was, and Move ends.
        click(&mut h, "Move");
        let (centre, pitch) = at(&h);
        let start = centre + egui::vec2(40.0, 0.0);
        drag(&mut h, start, start + egui::vec2(3.1 * pitch, 0.0));
        h.key_press(egui::Key::Escape);
        h.step();
        settle(&mut h);
        assert_eq!(std::fs::read_to_string(&path).expect("read"), text);
        assert!(!h.state().move_on && h.state().move_tool.pending.is_empty());
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
    }

    #[test]
    fn graphs_are_drawn_in_the_window_and_read_out_under_the_pointer() {
        // A dry NA 0.1 objective focusing onto a screen: the Graph button draws the
        // chosen graph in the results strip, newest first; the pointer on a graph sets
        // the cursor other graphs with the same x quantity show.
        let dir = scratch("graphs");
        let path = dir.join("focus.optobench");
        std::fs::write(
            &path,
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n\n\
             [[source]]\nid = \"beam\"\nkind = \"collimated\"\nwavelength_um = 0.55\npower_w = 1.0\ndiameter = 2.4\nrings = 6\nat = 0\n\n\
             [[element]]\nid = \"OBJ\"\nkind = \"objective\"\nat = 50\nfocal_length = 10.0\nna = 0.1\npupil_distance = 30.0\nworking_distance = 5.0\n\n\
             [[detector]]\nid = \"focus\"\nat = 85.0\nsize = [0.05, 0.05]\npixels = [40, 40]\n",
        )
        .expect("written");
        let mut h = window(Some(path), &dir);
        settle(&mut h);
        click(&mut h, "Workspace");
        click(&mut h, "Simulate");
        // Graph asks what to draw: Spot is ticked to start with.
        click(&mut h, "Graph");
        click(&mut h, "Draw");
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
        let titles = |h: &Harness<'static, App>| -> Vec<String> {
            h.state()
                .doc
                .as_ref()
                .map(|d| d.graphs.iter().map(|g| g.graph.title.clone()).collect())
                .unwrap_or_default()
        };
        assert_eq!(titles(&h), ["Spot diagram on `focus`"]);
        // The PSF ticked and the spot unticked: the PSF is added and shown, the spot
        // stays open to switch back to.
        click(&mut h, "Graph");
        click(&mut h, "PSF across the peak");
        click(&mut h, "Spot diagram");
        click(&mut h, "Draw");
        assert_eq!(
            titles(&h),
            ["Spot diagram on `focus`", "PSF across the peak"]
        );
        let shown = |h: &Harness<'static, App>| h.state().doc.as_ref().map_or(9, |d| d.shown_graph);
        assert_eq!(shown(&h), 1, "only the PSF was left ticked");
        // An edit leaves the open graphs out of date until redrawn.
        let doc = h.state().doc.as_ref().expect("open");
        assert!(!doc.out_of_date(&doc.graphs[1]));
        if let Some(doc) = h.state_mut().doc.as_mut() {
            doc.edit(
                vec![serde_json::json!({ "op": "set", "table": "detector", "id": "focus", "key": "at", "value": 85.01 })],
                "detector",
            );
        }
        settle(&mut h);
        let doc = h.state().doc.as_ref().expect("open");
        assert!(doc.out_of_date(&doc.graphs[1]) && doc.out_of_date(&doc.graphs[0]));
        click(&mut h, "Redraw");
        let doc = h.state().doc.as_ref().expect("open");
        assert!(
            !doc.out_of_date(&doc.graphs[1]),
            "the shown graph is redrawn"
        );
        assert!(doc.out_of_date(&doc.graphs[0]), "the other one is not");
        // The pointer on the shown graph's plot reads a length along x.
        let strip = h
            .query_all_by_label("Graphs")
            .next()
            .expect("the Graphs heading")
            .rect();
        h.hover_at(egui::pos2(strip.left() + 200.0, strip.bottom() + 80.0));
        h.step();
        h.step();
        let cursor = h.state().graph_cursor;
        assert!(
            matches!(cursor, Some((optobench_analysis::graph::Unit::Length, _))),
            "{cursor:?}"
        );
    }

    #[test]
    fn a_layout_is_built_traced_optimised_and_saved_in_the_window() {
        // A collimated beam, a biconvex singlet (R ±50 mm, 4 mm, n = 1.5: f ≈ 50.7 mm)
        // and a screen 50 mm behind it, about 1 mm short of the focus; the optimiser
        // slides the screen onto the focus.
        let dir = scratch("layout");
        let path = dir.join("bench.optobench");
        let mut h = window(None, &dir);
        click(&mut h, "Workspace");
        click(&mut h, "Layout");
        h.state_mut().chooser = answer(&path); // chosen in the save dialog
        click(&mut h, "New");
        click(&mut h, "Planar");
        assert!(path.is_file(), "New made the file");
        for add in ["Source", "Lens", "Detector"] {
            click(&mut h, add);
            if add == "Lens" {
                click(&mut h, "Singlet");
            }
        }
        click(&mut h, "D1");
        click(&mut h, "along its axis");
        click(&mut h, "Workspace");
        click(&mut h, "Simulate");
        click(&mut h, "3  Detectors & merit");
        click(&mut h, "focus");
        click(&mut h, "Trace");
        click(&mut h, "Optimise");
        assert!(
            h.state()
                .doc
                .as_ref()
                .is_some_and(|d| d.optimised.is_some()),
            "{:?}",
            errors(&h)
        );
        click(&mut h, "Apply");
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));

        let text = std::fs::read_to_string(&path).expect("saved");
        for expected in [
            "[[source]]",
            "[[element]]",
            "[[detector]]",
            "kind = \"focus\"",
            "name = \"D1_along\"",
            "value = ",
        ] {
            assert!(text.contains(expected), "`{expected}` in\n{text}");
        }
        // The runs strip: before and after the optimised values.
        let doc = h.state().doc.as_ref().expect("open");
        assert!(doc.runs.len() >= 2, "{} runs", doc.runs.len());
        let first = doc.runs[0].detectors[0].2;
        let last = doc.runs[doc.runs.len() - 1].detectors[0].2;
        assert!(last < 0.5 * first, "rms radius {first} → {last}");
        // What the window saved traces the same on its own.
        let again = trace(&path, &dir);
        let rms = again["detectors"][0]["direct_rms_radius_mm"]
            .as_f64()
            .expect("rms");
        assert!((rms - last).abs() < 1e-9 * (1.0 + rms), "{rms} vs {last}");
        // The uncoated singlet reflects about 4 % at each face (Fresnel); the window's
        // last run and the saved file agree on what arrives.
        let saved = again["ledger"]["detected_w"].as_f64().expect("detected")
            / again["ledger"]["launched_w"].as_f64().expect("launched");
        assert!((doc.runs[doc.runs.len() - 1].detected - saved).abs() < 1e-12);
        assert!(saved > 0.9 && saved < 0.95, "{saved}");
    }

    #[test]
    fn the_golden_periscopes_cad_workflow_runs_in_the_window() {
        // The misaligned golden periscope with M2 not yet tagged: tag its body as a
        // mirror by clicking its reflecting face, give it two tilts, optimise, write the
        // move report and apply the values.
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/optobench-io/tests/step");
        let dir = scratch("cad");
        for name in [
            "periscope-misaligned.optobench",
            "periscope-misaligned.step",
        ] {
            std::fs::copy(fixtures.join(name), dir.join(name)).expect("copied");
        }
        let path = dir.join("periscope-misaligned.optobench");
        optobench_commands::Registry::standard()
            .run(
                "edit",
                &json!({ "path": path.to_string_lossy(), "data_directory": dir.to_string_lossy(),
                         "operations": [{ "op": "remove", "table": "element", "id": "M2" }] }),
            )
            .expect("M2 untagged");
        let before = trace(&path, &dir);
        assert!(
            before["ledger"]["detected_w"].as_f64().unwrap_or(1.0)
                < 0.5 * before["ledger"]["launched_w"].as_f64().unwrap_or(0.0),
            "without M2 the light misses the screen"
        );

        let mut h = window(Some(path.clone()), &dir);
        settle(&mut h);
        // Turn the view to face M2's reflecting face (its own +z), as by orbiting, and
        // click its middle.
        let (centre, normal) = {
            let scene = h
                .state()
                .doc
                .as_ref()
                .and_then(|d| d.scene.as_ref())
                .expect("scene");
            let body = scene
                .items
                .iter()
                .find(|i| i.occurrence.as_deref() == Some("mirror:2"))
                .expect("M2's body");
            let axes = body.body_axes.expect("its frame");
            let (lo, hi) = body.mesh.as_ref().expect("triangles").bounds;
            let middle = [0, 1, 2].map(|k| 0.5 * (lo[k] + hi[k]) as f64);
            let n = axes[2];
            ([0, 1, 2].map(|k| middle[k] + 3.0 * n[k]), n)
        };
        {
            let app = h.state_mut();
            app.camera.target = centre;
            app.camera.height = 120.0;
            app.camera.pitch = normal[2].asin();
            app.camera.yaw = normal[1].atan2(normal[0]);
        }
        h.step();
        let rect = h.state().view.rect.expect("drawn");
        let at = rect.center();
        h.event(egui::Event::PointerMoved(at));
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
        }
        settle(&mut h);
        assert!(
            h.state().view.picked_normal.is_some(),
            "the face was clicked"
        );
        click(&mut h, "Tag Mirror");
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("reflect = [0.0, 0.0, 1.0]"), "{text}");

        // Select the new mirror (click it again) and give it two tilts.
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
        }
        settle(&mut h);
        click(&mut h, "tilt about X");
        click(&mut h, "tilt about Y");
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
        click(&mut h, "Align");
        click(&mut h, "Optimise");
        let optimised = h
            .state()
            .doc
            .as_ref()
            .and_then(|d| d.optimised.clone())
            .expect("optimised");
        let names: Vec<&str> = optimised["dofs"]
            .as_array()
            .expect("dofs")
            .iter()
            .filter_map(|d| d["name"].as_str())
            .collect();
        assert!(
            names.contains(&"M2_tilt_X") && names.contains(&"M2_tilt_Y"),
            "{names:?}"
        );
        assert!(
            dir.join("periscope-misaligned.moves.md").is_file(),
            "the move report"
        );
        let (start, end) = (
            optimised["cost"]["start"].as_f64().expect("cost"),
            optimised["cost"]["end"].as_f64().expect("cost"),
        );
        assert!(end < 1e-3 * start, "merit {start} → {end}");
        click(&mut h, "Apply");
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
        let after = trace(&path, &dir);
        let detected = after["ledger"]["detected_w"].as_f64().expect("detected")
            / after["ledger"]["launched_w"].as_f64().expect("launched");
        assert!((detected - 1.0).abs() < 1e-6, "{detected}");
    }

    #[test]
    fn a_mirror_is_aimed_from_its_properties() {
        // A source along +x, a mirror two holes on facing back along x (wrong), a screen
        // two holes up in y: the light from S1 to D1 turns from +x to +y, so Aim writes
        // the axis (−1, 1, 0), and the beam reaches the screen.
        let dir = scratch("aim");
        let path = dir.join("fold.optobench");
        std::fs::write(
            &path,
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"table\"\n\n[table]\npitch = 25.0\n\n\
             [[source]]\nid = \"S1\"\nkind = \"collimated\"\nwavelength_um = 0.633\npower_w = 0.001\ndiameter = 2.0\nhole = [0, 0]\nheight = 75.0\naxis = [1, 0, 0]\n\n\
             [[element]]\nid = \"M1\"\nkind = \"mirror\"\nhole = [2, 0]\nheight = 75.0\naxis = [-1, 0, 0]\ndiameter = 25.4\nclear_aperture = 22.9\n\n\
             [[detector]]\nid = \"D1\"\nhole = [2, 2]\nheight = 75.0\naxis = [0, 1, 0]\nsize = [20, 20]\npixels = [20, 20]\n",
        )
        .expect("written");
        let mut h = window(Some(path.clone()), &dir);
        settle(&mut h);
        click(&mut h, "Workspace");
        click(&mut h, "Layout");
        click(&mut h, "M1  ·  mirror");
        assert!(h.query_by_label("Aim").is_some(), "Aim in its Properties");
        h.state_mut().drafts.set_text("element/M1/aim/from", "S1");
        h.state_mut().drafts.set_text("element/M1/aim/to", "D1");
        click(&mut h, "Aim");
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(text.contains("axis = [-1, 1, 0]"), "{text}");
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
        let traced = trace(&path, &dir);
        let detected = traced["ledger"]["detected_w"].as_f64().expect("detected");
        assert!(detected > 0.9e-3, "{traced}");
    }

    #[test]
    fn an_iris_is_placed_on_a_hole_and_clips_the_beam() {
        // An 8 mm beam along x to a screen six holes on; Iris clicked on hole (3, 0): a
        // plate with a 5 mm hole, facing along x, passes about (5/8)² of the power. Made
        // a tube in Properties, its keys follow and the file still reads.
        let dir = scratch("iris");
        let path = dir.join("iris.optobench");
        std::fs::write(
            &path,
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"table\"\n\n[table]\npitch = 25.0\n\n\
             [[source]]\nid = \"S1\"\nkind = \"collimated\"\nwavelength_um = 0.633\npower_w = 0.001\ndiameter = 8.0\nhole = [0, 0]\nheight = 75.0\naxis = [1, 0, 0]\n\n\
             [[detector]]\nid = \"D1\"\nhole = [6, 0]\nheight = 75.0\naxis = [1, 0, 0]\nsize = [20, 20]\npixels = [20, 20]\n",
        )
        .expect("written");
        let mut h = window(Some(path.clone()), &dir);
        settle(&mut h);
        click(&mut h, "Workspace");
        click(&mut h, "Layout");
        click(&mut h, "Iris");
        h.state_mut().camera.top();
        h.step();
        let at = {
            let app = h.state();
            let rect = app.view.rect.expect("drawn");
            let (x, y) = app.camera.to_screen(
                [75.0, 0.0, 0.0],
                (rect.center().x as f64, rect.center().y as f64),
                rect.height() as f64,
            );
            egui::pos2(x as f32, y as f32)
        };
        h.event(egui::Event::PointerMoved(at));
        h.step();
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
        }
        settle(&mut h);
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(
            text.contains("[[proxy]]\nid = \"I1\"\nkind = \"plate\"")
                && text.contains("hole = [3, 0]"),
            "{text}"
        );
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
        let traced = trace(&path, &dir);
        let passed = traced["ledger"]["detected_w"].as_f64().expect("detected") / 1e-3;
        let expected = (5.0_f64 / 8.0).powi(2);
        assert!(
            (passed - expected).abs() < 0.1,
            "{passed} against {expected}"
        );
        // Properties: the iris made a tube.
        click(&mut h, "I1  ·  iris");
        h.query_all_by_role(egui::accesskit::Role::ComboBox)
            .find(|c| c.value().as_deref() == Some("plate"))
            .expect("its kind")
            .click();
        h.step();
        h.step();
        click(&mut h, "tube");
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(
            text.contains("kind = \"tube\"")
                && text.contains("length = 50")
                && !text.contains("hole_diameter"),
            "{text}"
        );
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
    }

    #[test]
    fn a_mount_is_chosen_from_the_parts_library() {
        // A kinematic mount (±4° tip and tilt) in the library; M1's Properties list it,
        // and choosing it writes its key: the scene's M1 gets M1_tip and M1_tilt.
        let dir = scratch("mount");
        let part = dir.join("km1.toml");
        std::fs::write(
            &part,
            "format = \"optobench/1\"\nkind = \"part\"\nvendor = \"Example\"\ntype = \"mount\"\npart = \"KM1\"\n\
             source = \"kinematic mirror mount: ±4° tip and tilt\"\n\
             adjusters = [ { name = \"tip\", kind = \"tilt\", axis = \"x\", range = [-4, 4] },\n\
                           { name = \"tilt\", kind = \"tilt\", axis = \"y\", range = [-4, 4] } ]\n",
        )
        .expect("written");
        optobench_commands::Registry::standard()
            .run(
                "import-part",
                &json!({ "path": part.to_string_lossy(), "data_directory": dir.to_string_lossy() }),
            )
            .expect("imported");
        let path = dir.join("mirror.optobench");
        std::fs::write(
            &path,
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"table\"\n\n[table]\npitch = 25.0\n\n\
             [[source]]\nid = \"S1\"\nkind = \"collimated\"\nwavelength_um = 0.633\npower_w = 0.001\ndiameter = 2.0\nhole = [0, 0]\nheight = 75.0\naxis = [1, 0, 0]\n\n\
             [[element]]\nid = \"M1\"\nkind = \"mirror\"\nhole = [2, 0]\nheight = 75.0\naxis = [-1, 1, 0]\ndiameter = 25.4\nclear_aperture = 22.9\n",
        )
        .expect("written");
        let mut h = window(Some(path.clone()), &dir);
        settle(&mut h);
        click(&mut h, "Workspace");
        click(&mut h, "Layout");
        click(&mut h, "M1  ·  mirror");
        h.query_all_by_role(egui::accesskit::Role::ComboBox)
            .filter(|c| c.value().as_deref() == Some("none"))
            .nth(1)
            .expect("the mount list, after the group's")
            .click();
        h.step();
        h.step();
        click(&mut h, "example/KM1");
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(text.contains("mount = \"example/KM1\""), "{text}");
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
        let names: Vec<String> = h
            .state()
            .doc
            .as_ref()
            .and_then(|d| d.scene.as_ref())
            .expect("a scene")
            .dofs
            .iter()
            .filter_map(|d| d["name"].as_str().map(str::to_owned))
            .collect();
        assert_eq!(names, ["M1_tip", "M1_tilt"]);
        assert!(
            h.query_by_label("    M1_tip: tilt about x, -4 to 4°")
                .is_some(),
            "what it gives"
        );
    }

    #[test]
    fn a_singlet_becomes_an_achromat_of_the_same_focal_length() {
        // A biconvex singlet (R ±50, n 1.5, 4 mm: f 50.67 mm at any wavelength) made an
        // achromat from the type list: a crown and a flint by nd and Vd, f 50.67 mm at the
        // d line; the engine's own paraxial trace (`view`) finds that f at d and one f at
        // the F and C lines.
        let dir = scratch("achromat");
        let path = dir.join("achromat.optobench");
        let file = |wavelength: f64| {
            format!(
                "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"table\"\n\n[table]\npitch = 25.0\n\n\
                 [[source]]\nid = \"S1\"\nkind = \"collimated\"\nwavelength_um = {wavelength}\npower_w = 0.001\ndiameter = 2.0\nhole = [0, 0]\nheight = 75.0\naxis = [1, 0, 0]\n\n\
                 [[element]]\nid = \"L1\"\nkind = \"lens\"\nhole = [2, 0]\nheight = 75.0\naxis = [1, 0, 0]\n\
                 surfaces = [{{ radius = 50.0 }}, {{ radius = -50.0 }}]\nthickness = [4.0]\nmaterial = [1.5]\ndiameter = 25.4\nclear_aperture = 22.9\n"
            )
        };
        std::fs::write(&path, file(optobench_materials::LINE_D)).expect("written");
        let mut h = window(Some(path.clone()), &dir);
        settle(&mut h);
        click(&mut h, "Workspace");
        click(&mut h, "Layout");
        click(&mut h, "L1  ·  lens");
        h.query_all_by_role(egui::accesskit::Role::ComboBox)
            .next()
            .expect("the type list")
            .click();
        h.step();
        h.step();
        click(&mut h, "achromat (cemented doublet)");
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(
            text.contains("material = [{ nd = 1.5168, vd = 64.17 }, { nd = 1.6727, vd = 32.25 }]")
                && text.contains("thickness = [6, 2.5]"),
            "{text}"
        );
        assert!(h.query_by_label("Achromat").is_some(), "its own editor");
        let f_at = |wavelength: f64| {
            let changed = text.replace(
                &format!("wavelength_um = {}", optobench_materials::LINE_D),
                &format!("wavelength_um = {wavelength}"),
            );
            std::fs::write(&path, changed).expect("written");
            let view = optobench_commands::Registry::standard()
                .run("view", &json!({ "path": path.to_string_lossy(), "data_directory": dir.to_string_lossy() }))
                .expect("viewed");
            view["items"]
                .as_array()
                .and_then(|items| items.iter().find(|i| i["id"] == "L1"))
                .and_then(|i| i["lens"]["focal_length_mm"].as_f64())
                .expect("its focal length")
        };
        let singlet = 1.0 / (0.5 * (2.0 / 50.0 - 0.5 * 4.0 / (1.5 * 2500.0)));
        let d = f_at(optobench_materials::LINE_D);
        assert!((d - singlet).abs() < 1e-4, "{d} against {singlet}");
        let (blue, red) = (
            f_at(optobench_materials::LINE_F),
            f_at(optobench_materials::LINE_C),
        );
        assert!((blue - red).abs() < 1e-4, "F {blue}, C {red}");
    }

    #[test]
    fn backscatter_is_added_with_a_detector_at_the_laser() {
        // A 2 mm beam through an uncoated window square to it: Add a term, backscatter,
        // puts a returning detector R1 at the laser, 2 mm wide, and the term on it; the
        // trace then weighs the reflections coming back (a few per cent of the beam).
        let dir = scratch("backscatter");
        let path = dir.join("window.optobench");
        std::fs::write(
            &path,
            "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"table\"\n\n[table]\npitch = 25.0\n\n\
             [[source]]\nid = \"S1\"\nkind = \"collimated\"\nwavelength_um = 0.633\npower_w = 0.001\ndiameter = 2.0\nhole = [0, 0]\nheight = 75.0\naxis = [1, 0, 0]\n\n\
             [[element]]\nid = \"W1\"\nkind = \"lens\"\nhole = [4, 0]\nheight = 75.0\naxis = [1, 0, 0]\n\
             surfaces = [{ radius = \"inf\" }, { radius = \"inf\" }]\nthickness = [5.0]\nmaterial = [1.5]\ndiameter = 25.4\nclear_aperture = 22.9\n\n\
             [[detector]]\nid = \"D1\"\nhole = [10, 0]\nheight = 75.0\naxis = [1, 0, 0]\nsize = [20, 20]\npixels = [20, 20]\n",
        )
        .expect("written");
        let mut h = window(Some(path.clone()), &dir);
        settle(&mut h);
        click(&mut h, "Workspace");
        click(&mut h, "Layout");
        click(&mut h, "Add a term");
        click(&mut h, "backscatter");
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(
            text.contains("id = \"R1\"\nhole = [0, 0]") && text.contains("returning = true"),
            "{text}"
        );
        assert!(
            text.contains("kind = \"backscatter\"\ndetector = \"R1\""),
            "{text}"
        );
        let traced = trace(&path, &dir);
        let back = traced["detectors"]
            .as_array()
            .and_then(|l| l.iter().find(|d| d["id"] == "R1"))
            .expect("R1");
        let percent = back["returned_percent"].as_f64().expect("%");
        assert!(percent > 2.0 && percent < 7.7, "{percent} %");
        // R1 stops nothing: the screen gets the beam through both faces.
        let screen = traced["detectors"][0]["direct_power_w"]
            .as_f64()
            .expect("direct");
        assert!((screen / 1e-3 - 0.9216).abs() < 1e-6, "{screen}");
    }

    #[test]
    fn live_traces_the_design_open_in_solidcraft() {
        // Against a stand-in for SolidCraft: Live on the golden cage relay's sidecar traces
        // the design as it is; a lens moved there is traced again and the scene read from
        // the new export; Live off reads the sidecar's own STEP file again.
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/optobench-io/tests/step");
        let app = optobench_bridge::fake::FakeApp::start(&fixtures.join("cage-relay.step"));
        let dir = scratch("live");
        app.write_control_folder(&dir.join("control"));
        let mut h = window(Some(fixtures.join("cage-relay.optobench")), &dir);
        h.state_mut().control_directory = Some(dir.join("control"));
        settle(&mut h);
        let wait = |h: &mut Harness<'static, App>, until: &dyn Fn(&App) -> bool| {
            for _ in 0..500 {
                h.step();
                settle(h);
                if until(h.state()) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            panic!("waited in vain: {:?}", h.state().notice);
        };
        let image = |app: &App| -> Option<Value> {
            app.doc.as_ref()?.trace.as_ref()?.report["detectors"]
                .as_array()?
                .iter()
                .find(|d| d["id"] == "image")
                .map(|d| d["direct_centroid_mm"].clone())
        };
        click(&mut h, "Live");
        wait(&mut h, &|a| {
            a.live.as_ref().is_some_and(|l| l.revision == Some(1)) && image(a).is_some()
        });
        let first = image(h.state()).expect("traced");
        assert!(h.state().doc.as_ref().expect("open").live_step.is_some());
        app.change(&fixtures.join("cage-relay-moved.step"));
        wait(&mut h, &|a| {
            a.live.as_ref().is_some_and(|l| l.revision == Some(2))
        });
        settle(&mut h);
        let moved = image(h.state()).expect("traced again");
        assert_ne!(moved, first, "the move shows");
        // Moved back in SolidCraft: no export, the parts placed by moves, as at first.
        app.move_occurrence(
            "pc-lens:2",
            [0.0, 0.0, -1.0],
            &fixtures.join("cage-relay.step"),
        );
        app.move_occurrence(
            "cage-plate:2",
            [0.0, 0.0, -1.0],
            &fixtures.join("cage-relay.step"),
        );
        wait(&mut h, &|a| {
            a.live.as_ref().is_some_and(|l| l.revision == Some(4))
        });
        settle(&mut h);
        let doc = h.state().doc.as_ref().expect("open");
        assert_eq!(
            doc.live_moves.as_array().map(Vec::len),
            Some(2),
            "{}",
            doc.live_moves
        );
        assert!(
            h.state()
                .notice
                .as_ref()
                .is_some_and(|(e, t)| !e && t.contains("revision 4")),
            "{:?}",
            h.state().notice
        );
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
        // Live off: the sidecar's own STEP file, traced as before.
        click(&mut h, "Live");
        settle(&mut h);
        assert!(h.state().live.is_none());
        assert!(h.state().doc.as_ref().expect("open").live_step.is_none());
        assert_eq!(image(h.state()).expect("traced"), first);
    }

    #[test]
    fn optimised_values_are_written_into_solidcraft() {
        // Against a stand-in for SolidCraft: the golden relay with L2 on a slide, Live on,
        // an optimisation that asks L2_z = 1 mm; Apply in SolidCraft asks first, then
        // moves L2 1 mm there and the design is traced again.
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/optobench-io/tests/step");
        let app = optobench_bridge::fake::FakeApp::start(&fixtures.join("cage-relay.step"));
        let dir = scratch("write-live");
        app.write_control_folder(&dir.join("control"));
        let text = std::fs::read_to_string(fixtures.join("cage-relay.optobench"))
            .expect("read")
            .replace(
                "kind = \"sidecar\"\n",
                &format!("kind = \"sidecar\"\nstep = '{}'\n", fixtures.join("cage-relay.step").to_string_lossy()),
            )
            .replace(
                "body = { product = \"pc-lens\", occurrence = \"pc-lens:2\" }",
                "body = { product = \"pc-lens\", occurrence = \"pc-lens:2\" }\ndof = [ { name = \"L2_z\", kind = \"slide\", axis = [0, 0, 1], range = [-5, 5] } ]",
            );
        let sidecar = dir.join("relay.optobench");
        std::fs::write(&sidecar, text).expect("written");
        let mut h = window(Some(sidecar.clone()), &dir);
        h.state_mut().control_directory = Some(dir.join("control"));
        settle(&mut h);
        let wait = |h: &mut Harness<'static, App>, until: &dyn Fn(&App) -> bool| {
            for _ in 0..500 {
                h.step();
                settle(h);
                if until(h.state()) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            panic!("waited in vain: {:?}", h.state().notice);
        };
        click(&mut h, "Live");
        wait(&mut h, &|a| {
            a.live.as_ref().is_some_and(|l| l.revision == Some(1))
        });
        h.state_mut().doc.as_mut().expect("open").optimised =
            Some(json!({ "dofs": [{ "name": "L2_z", "suggested": 1.0 }] }));
        click(&mut h, "Align");
        click(&mut h, "Apply in SolidCraft");
        assert!(
            h.query_by_label("Write into SolidCraft").is_some(),
            "asked first"
        );
        click(&mut h, "Write");
        wait(&mut h, &|a| {
            a.doc.as_ref().is_some_and(|d| {
                d.messages
                    .iter()
                    .any(|m| m.text.starts_with("Written into SolidCraft"))
            })
        });
        assert!((app.world("pc-lens:2")[2][3] - 1.0).abs() < 1e-12);
        wait(&mut h, &|a| {
            a.live
                .as_ref()
                .is_some_and(|l| l.revision.is_some_and(|r| r > 1))
        });
        assert!(h.state().doc.as_ref().expect("open").optimised.is_none());
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
    }

    #[test]
    fn a_rough_placement_is_optimised_first() {
        // M18 in the window: the golden relay with L2 and its cage plate in a group on a
        // free slide (20 mm out) and L2's adjuster off-centre; Optimise says it went in two
        // stages, and the free range is ticked off and on in Properties.
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/optobench-io/tests/step");
        let dir = scratch("stages");
        let text = std::fs::read_to_string(fixtures.join("cage-relay.optobench"))
            .expect("read")
            .replace(
                "kind = \"sidecar\"\n",
                &format!("kind = \"sidecar\"\nstep = '{}'\n", fixtures.join("cage-relay.step").to_string_lossy()),
            )
            .replace(
                "body = { product = \"pc-lens\", occurrence = \"pc-lens:2\" }",
                "body = { product = \"pc-lens\", occurrence = \"pc-lens:2\" }\ngroup = \"L2 assembly\"\ndof = [ { name = \"L2_x\", kind = \"slide\", axis = [1, 0, 0], frame = \"scene\", range = [-1, 1], value = 0.3 } ]",
            )
            + "\n[[group]]\nid = \"L2 assembly\"\noccurrences = [\"cage-plate:2\"]\ndof = [ { name = \"L2_rail\", kind = \"slide\", axis = [0, 0, 1], range = \"free\", value = 20 } ]\n\n[[merit]]\nkind = \"focus\"\ndetector = \"image\"\n";
        let path = dir.join("relay.optobench");
        std::fs::write(&path, text).expect("written");
        let mut h = window(Some(path.clone()), &dir);
        settle(&mut h);
        click(&mut h, "Workspace");
        click(&mut h, "CAD");
        click(&mut h, "Align");
        click(&mut h, "Optimise");
        let said = h
            .state()
            .doc
            .as_ref()
            .and_then(|d| {
                d.messages
                    .iter()
                    .rev()
                    .find(|m| m.text.starts_with("Optimised"))
                    .cloned()
            })
            .unwrap_or_else(|| {
                panic!(
                    "no message: {:?} {:?}",
                    h.state().doc.as_ref().map(|d| d.messages.clone()),
                    h.state().notice
                )
            });
        assert!(
            said.text.starts_with("Optimised in two stages: rough (1)")
                && said.text.contains("then fine (1)"),
            "{}",
            said.text
        );
        // L2's adjuster made free in Properties, then given its range back.
        click(&mut h, "Scene");
        click(&mut h, "L2  ·  lens");
        click(&mut h, "free");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(
            text.contains("name = \"L2_x\"") && text.contains("range = \"free\", value = 0.3"),
            "{text}"
        );
        click(&mut h, "free");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("range = [-10, 10], value = 0.3"), "{text}");
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
    }

    #[test]
    fn a_mount_moves_with_its_lens_and_the_space_is_found() {
        // M19 in the window, on the golden relay: cage plate 2 (an untagged CAD part) made
        // to move with L2, the group given a free slide along Z, and its space found: down
        // to the other plate (117.5 mm, plus the 0.05 mm they may sink in).
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/optobench-io/tests/step");
        let dir = scratch("move-with");
        let text = std::fs::read_to_string(fixtures.join("cage-relay.optobench"))
            .expect("read")
            .replace(
                "kind = \"sidecar\"\n",
                &format!(
                    "kind = \"sidecar\"\nstep = '{}'\n",
                    fixtures.join("cage-relay.step").to_string_lossy()
                ),
            );
        let path = dir.join("relay.optobench");
        std::fs::write(&path, text).expect("written");
        let mut h = window(Some(path.clone()), &dir);
        settle(&mut h);
        click(&mut h, "Workspace");
        click(&mut h, "CAD");
        let plate = h
            .state()
            .doc
            .as_ref()
            .and_then(|d| d.scene.as_ref())
            .and_then(|s| s.items.iter().position(|i| i.id == "cage-plate:2/Plate"))
            .expect("the plate");
        h.state_mut().selected = Some(plate);
        h.step();
        h.step();
        h.query_all_by_role(egui::accesskit::Role::ComboBox)
            .find(|c| c.value().as_deref() == Some("none"))
            .expect("moves with")
            .click();
        h.step();
        h.step();
        click(&mut h, "L2");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(
            text.contains("group = \"L2 assembly\"")
                && text
                    .contains("[[group]]\nid = \"L2 assembly\"\noccurrences = [\"cage-plate:2\"]"),
            "{text}"
        );
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
        click(&mut h, "L2 assembly  ·  L2, cage-plate:2");
        click(&mut h, "slide Z");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(
            text.contains("name = \"L2_assembly_slide_Z\"") && text.contains("range = \"free\""),
            "{text}"
        );
        click(&mut h, "Find the space");
        for _ in 0..600 {
            h.step();
            settle(&mut h);
            if h.state()
                .doc
                .as_ref()
                .is_some_and(|d| !d.clearances.is_empty())
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        h.step();
        h.step();
        // Up, the plate stops flush with the rods' ends (M20).
        let shown = "space: -117.55 mm (stopped by cage-plate:1/Plate) to +56.50 mm (at the end of rod:1/Rod)";
        assert!(
            h.query_by_label(shown).is_some(),
            "{:?}",
            h.state().doc.as_ref().map(|d| d.clearances.clone())
        );
        assert!(errors(&h).is_empty(), "{:?}", errors(&h));
    }
}
