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
use crate::theme::{Accent, DARK};
use crate::viewport::{self, Request};
use crate::workspaces::{Action, Control, Item, Layout, Section, Stage, WORKSPACES};

/// The application.
pub struct App {
    ctx: egui::Context,
    workspace: usize,
    tab: usize,
    stage: usize,
    doc: Option<Document>,
    camera: Camera,
    selected: Option<usize>,
    hidden: Vec<bool>,
    show_rays: bool,
    fit_pending: bool,
    open_path: String,
    /// Where imported materials, parts and curves are; empty for the platform's default.
    data_directory: String,
    confirm: Option<&'static str>,
    /// See `screenshot`.
    shot: Option<Shot>,
}

/// A screenshot to take once the design is ready (see `screenshot`).
struct Shot {
    path: PathBuf,
    trace: bool,
    traced: bool,
    select: Option<String>,
    frames: u32,
    requested: bool,
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl App {
    /// A window, opening `path` if given.
    pub fn new(path: Option<PathBuf>, data_directory: Option<PathBuf>, ctx: egui::Context) -> Self {
        let mut app = Self {
            ctx,
            workspace: 0,
            tab: 0,
            stage: 0,
            doc: None,
            camera: Camera::default(),
            selected: None,
            hidden: Vec::new(),
            show_rays: true,
            fit_pending: false,
            open_path: String::new(),
            data_directory: data_directory
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_default(),
            confirm: None,
            shot: std::env::var_os("OPTOBENCH_UI_SCREENSHOT").map(|p| Shot {
                path: PathBuf::from(p),
                trace: std::env::var_os("OPTOBENCH_UI_TRACE").is_some(),
                traced: false,
                select: std::env::var("OPTOBENCH_UI_SELECT").ok(),
                frames: 0,
                requested: false,
                done: Default::default(),
            }),
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
        app
    }

    fn repaint(&self) -> impl Fn() + Send + Clone + 'static {
        let ctx = self.ctx.clone();
        move || ctx.request_repaint()
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
        self.open_path = path.to_string_lossy().into_owned();
        let data = Some(self.data_directory.trim())
            .filter(|d| !d.is_empty())
            .map(PathBuf::from);
        self.doc = Some(Document::open(path, data, self.repaint()));
        self.selected = None;
        self.hidden.clear();
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
                    let path = PathBuf::from(self.open_path.trim());
                    if path.is_file() {
                        self.open(path);
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
            },
            Control::Command(id) => {
                let writes = optobench_commands::Registry::standard()
                    .get(id)
                    .is_some_and(|c| c.effect == optobench_commands::Effect::Write);
                if writes && id != "optimize" {
                    self.confirm = Some(id);
                } else {
                    self.run(id);
                }
            }
        }
    }

    fn run(&mut self, id: &str) {
        let (repaint, again) = (self.repaint(), self.repaint());
        if let Some(doc) = &mut self.doc {
            doc.run(id, json!({}), repaint);
            // An optimisation changes what the view shows.
            if id == "optimize" {
                doc.run("trace", json!({}), again);
            }
        }
    }

    /// Whether a control can run now, or why not.
    fn available(&self, control: Control) -> Result<(), String> {
        match control {
            Control::Action(Action::Open) => Ok(()),
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

    fn summary(control: Control) -> String {
        match control {
            Control::Action(a) => a.label().to_owned(),
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
            let field = egui::TextEdit::singleline(&mut self.open_path)
                .hint_text("path to a .optobench layout or sidecar")
                .desired_width(360.0);
            let response = ui.add(field);
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.activate(Control::Action(Action::Open));
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
            if let Some(doc) = &self.doc {
                ui.separator();
                Frame::new()
                    .fill(DARK.panel)
                    .corner_radius(CornerRadius::same(DARK.radius_md))
                    .inner_margin(Margin::symmetric(10, 3))
                    .show(ui, |ui| {
                        ui.label(RichText::new(doc.name()).strong());
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
        // The workspace's emblem: a lens between two rays (the icon set comes in M8c).
        let c = rect.center() - Vec2::new(0.0, 16.0);
        ui.painter()
            .circle_stroke(c, 9.0, Stroke::new(1.6, accent.ink));
        ui.painter().line_segment(
            [c - Vec2::new(18.0, 4.0), c + Vec2::new(18.0, 4.0)],
            Stroke::new(1.4, accent.ink),
        );
        ui.painter().line_segment(
            [c - Vec2::new(18.0, -4.0), c + Vec2::new(18.0, -4.0)],
            Stroke::new(1.4, accent.ink),
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
        let (rect, response) = ui.allocate_exact_size(Vec2::new(64.0, 58.0), Sense::click());
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
        let label = Self::label(item.control);
        let icon = Vec2::new(26.0, 26.0);
        let icon_rect =
            egui::Rect::from_center_size(rect.center_top() + Vec2::new(0.0, 18.0), icon);
        let active = matches!(item.control, Control::Action(Action::Rays)) && self.show_rays;
        ui.painter().rect_stroke(
            icon_rect,
            CornerRadius::same(DARK.radius_sm),
            Stroke::new(1.4, if active { self.accent().ink } else { ink }),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            icon_rect.center(),
            egui::Align2::CENTER_CENTER,
            label.chars().next().unwrap_or('?').to_string(),
            egui::FontId::proportional(13.0),
            ink,
        );
        ui.painter().text(
            rect.center_bottom() - Vec2::new(0.0, 9.0),
            egui::Align2::CENTER_CENTER,
            &label,
            egui::FontId::proportional(11.0),
            ink,
        );
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
                        }
                    });
                };
                egui::CollapsingHeader::new(format!("Sources ({})", scene.sources.len()))
                    .default_open(true)
                    .show(ui, |ui| {
                        for s in &scene.sources {
                            ui.horizontal(|ui| {
                                swatch(ui, crate::viewport::ray_colour("detected", 0));
                                ui.label(&s.id);
                            });
                        }
                    });
                let optics: Vec<usize> = (0..scene.items.len())
                    .filter(|&i| matches!(scene.items[i].role.as_str(), "lens" | "mirror"))
                    .collect();
                egui::CollapsingHeader::new(format!("Optics ({})", optics.len()))
                    .default_open(true)
                    .show(ui, |ui| {
                        for i in optics {
                            let item = &scene.items[i];
                            row(ui, self, i, format!("{}  ·  {}", item.id, item.role));
                        }
                    });
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
        if let Layout::Stages { stages, .. } = WORKSPACES[self.workspace].layout {
            let stage = stages[self.stage.min(stages.len() - 1)];
            ui.label(RichText::new(stage.name).strong().size(14.0));
            ui.label(RichText::new(stage.help).color(DARK.text_dim));
            ui.separator();
            self.ready_card(ui);
            ui.separator();
        }
        ui.label(RichText::new("Properties").strong().size(14.0));
        ui.separator();
        let Some(doc) = &self.doc else { return };
        let Some(scene) = &doc.scene else { return };
        let Some(item) = self.selected.and_then(|i| scene.items.get(i)) else {
            ui.label(
                RichText::new("Select an item in the view or the browser").color(DARK.text_dim),
            );
            return;
        };
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
                    ("Outlines", item.lines.len().to_string()),
                ] {
                    ui.label(RichText::new(k).color(DARK.text_dim));
                    ui.label(v);
                    ui.end_row();
                }
            });
        if item.role == "detector" {
            if let Some(d) = doc.trace.as_ref().and_then(|t| {
                t.report["detectors"]
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
                    .filter(|i| matches!(i.role.as_str(), "lens" | "mirror"))
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
        let Some(doc) = &self.doc else {
            ui.label(RichText::new("Results appear here after a trace").color(DARK.text_dim));
            return;
        };
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(340.0);
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
                    ui.set_width(260.0);
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
        // A design dropped on the window opens.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.open(path);
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
            .default_size(170.0)
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
        let requests = egui::CentralPanel::no_frame()
            .show(ui, |ui| {
                // Fit once the scene has arrived, with the view's own aspect.
                if self.fit_pending {
                    if let Some((low, high)) = self
                        .doc
                        .as_ref()
                        .and_then(|d| d.scene.as_ref())
                        .and_then(|s| s.bounds)
                    {
                        let size = ui.available_size();
                        self.camera
                            .fit(low, high, (size.x / size.y.max(1.0)) as f64);
                        self.fit_pending = false;
                    }
                }
                viewport::show(
                    ui,
                    &mut self.camera,
                    self.doc.as_ref(),
                    self.selected,
                    &self.hidden,
                    self.show_rays,
                    accent,
                )
            })
            .inner;
        for request in requests {
            match request {
                Request::Select(index) => self.selected = index,
                Request::Fit => self.fit_pending = true,
            }
        }
        self.confirm_dialog(&ctx);
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
        if let Some(id) = shot.select.take() {
            self.selected = doc
                .scene
                .as_ref()
                .and_then(|s| s.items.iter().position(|i| i.id == id));
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
