//! Editors for a design's entries (M8c): every key of an `[[element]]`, `[[source]]`,
//! `[[detector]]`, `[[proxy]]` or `[[merit]]` as written in the file, its degrees of
//! freedom, and the defaults for new entries. Editors return `edit` operations; the
//! document sends them through the command registry, which writes the file only if the
//! design still reads.

use std::collections::HashMap;

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::camera::V3;
use crate::theme::DARK;

/// Which entry an editor changes: `{ "table": …, "id": … }`, or `"index"` for merit terms.
pub fn selector(table: &str, entry: &Value, index: usize) -> Value {
    match entry["id"].as_str() {
        Some(id) => json!({ "table": table, "id": id }),
        None => json!({ "table": table, "index": index }),
    }
}

/// Values being typed or dragged, until they are committed.
#[derive(Default)]
pub struct Drafts {
    numbers: HashMap<String, f64>,
    texts: HashMap<String, String>,
}

impl Drafts {
    /// Forgets what was being edited (after the file changed).
    pub fn clear(&mut self) {
        self.numbers.clear();
        self.texts.clear();
    }
}

/// The choices for keys that take one of a few words.
fn choices(table: &str, key: &str) -> Option<&'static [&'static str]> {
    match (table, key) {
        ("element", "kind") => Some(&["lens", "mirror", "filter"]),
        ("source", "kind") => Some(&["collimated", "point", "gaussian"]),
        ("merit", "kind") => Some(&["focus", "centring", "pointing", "collimation", "power"]),
        ("proxy", "kind") => Some(&["tube"]),
        _ => None,
    }
}

/// The keys of an entry in the order to show them: id and kind first, then by name; the
/// degrees of freedom are edited apart.
fn keys(entry: &Value) -> Vec<String> {
    let mut keys: Vec<String> = entry
        .as_object()
        .map(|m| m.keys().filter(|k| *k != "dof").cloned().collect())
        .unwrap_or_default();
    let rank = |k: &str| match k {
        "id" => 0,
        "kind" => 1,
        _ => 2,
    };
    keys.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.cmp(b)));
    keys
}

/// A step for dragging a number: about a hundredth of its size, at least 0.01.
fn speed(x: f64) -> f64 {
    (x.abs() * 0.01).max(0.01)
}

/// A number field; returns the new value when the change is finished (drag released,
/// typing done).
fn number(ui: &mut Ui, drafts: &mut Drafts, path: &str, current: f64) -> Option<f64> {
    let mut value = *drafts.numbers.get(path).unwrap_or(&current);
    let response = ui.add(
        egui::DragValue::new(&mut value)
            .speed(speed(current))
            .max_decimals(6),
    );
    if response.changed() {
        drafts.numbers.insert(path.to_owned(), value);
    }
    let done = response.drag_stopped()
        || response.lost_focus()
        || (response.changed() && !response.dragged() && !response.has_focus());
    if done {
        if let Some(v) = drafts.numbers.remove(path) {
            if v.to_bits() != current.to_bits() {
                return Some(v);
            }
        }
    }
    None
}

/// A text field committed when typing is done; `None` while unchanged.
fn text(ui: &mut Ui, drafts: &mut Drafts, path: &str, current: &str, wide: f32) -> Option<String> {
    let draft = drafts
        .texts
        .entry(path.to_owned())
        .or_insert_with(|| current.to_owned());
    let response = ui.add(egui::TextEdit::singleline(draft).desired_width(wide));
    if response.lost_focus() {
        let typed = drafts.texts.remove(path).unwrap_or_default();
        if typed != current {
            return Some(typed);
        }
    } else if !response.has_focus() && draft != current {
        // The file changed under a field nobody is typing in.
        *draft = current.to_owned();
    }
    None
}

/// The editor for one value; returns the new value when it was changed.
fn field(
    ui: &mut Ui,
    drafts: &mut Drafts,
    path: &str,
    table: &str,
    key: &str,
    value: &Value,
    context: &Context,
) -> Option<Value> {
    if let (Some(options), Some(current)) = (choices(table, key), value.as_str()) {
        let mut chosen = None;
        egui::ComboBox::from_id_salt(path)
            .selected_text(current)
            .show_ui(ui, |ui| {
                for option in options {
                    if ui.selectable_label(*option == current, *option).clicked() {
                        chosen = Some(json!(option));
                    }
                }
            });
        return chosen.filter(|c| c != value);
    }
    if let (true, Some(current)) = (key == "detector", value.as_str()) {
        let mut chosen = None;
        egui::ComboBox::from_id_salt(path)
            .selected_text(current)
            .show_ui(ui, |ui| {
                for id in &context.detectors {
                    if ui.selectable_label(id == current, id).clicked() {
                        chosen = Some(json!(id));
                    }
                }
            });
        return chosen.filter(|c| c != value);
    }
    match value {
        Value::Number(n) => {
            let current = n.as_f64().unwrap_or(0.0);
            number(ui, drafts, path, current).map(|v| {
                // Whole numbers stay whole where they were (pixels, rings, holes).
                if n.is_i64() && v.fract() == 0.0 {
                    json!(v as i64)
                } else {
                    json!(v)
                }
            })
        }
        Value::Bool(b) => {
            let mut on = *b;
            ui.checkbox(&mut on, "").changed().then(|| json!(on))
        }
        Value::String(s) => text(ui, drafts, path, s, 140.0).map(Value::String),
        Value::Array(items) if items.iter().all(Value::is_number) => {
            let mut changed = None;
            let names = component_names(key, items.len());
            ui.horizontal(|ui| {
                for (k, item) in items.iter().enumerate() {
                    if let Some(name) = names.get(k) {
                        ui.label(RichText::new(*name).color(DARK.text_dim).size(11.0));
                    }
                    let component = format!("{path}/{k}");
                    if let Some(v) = field(ui, drafts, &component, table, "", item, context) {
                        let mut new = items.clone();
                        new[k] = v;
                        changed = Some(Value::Array(new));
                    }
                }
            });
            changed
        }
        // Anything else (surfaces, references): its JSON, edited as text.
        other => {
            let shown = serde_json::to_string(other).unwrap_or_default();
            text(ui, drafts, path, &shown, 200.0)
                .and_then(|typed| serde_json::from_str::<Value>(&typed).ok())
        }
    }
}

/// What the editors need to know about the design.
#[derive(Default)]
pub struct Context {
    /// The detectors' ids, for merit terms.
    pub detectors: Vec<String>,
    /// Whether the file is a sidecar (CAD).
    pub sidecar: bool,
    /// A table layout's hole pitch (mm).
    pub pitch: Option<f64>,
}

impl Context {
    /// From `view`'s `design`.
    pub fn of(design: &Value) -> Self {
        Self {
            detectors: design["detector"]
                .as_array()
                .map(|l| {
                    l.iter()
                        .filter_map(|d| d["id"].as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            sidecar: design["kind"] == "sidecar",
            pitch: (design["layout"] == "table")
                .then(|| design["table"]["pitch"].as_f64())
                .flatten(),
        }
    }
}

/// The names of a list's numbers, by its key: `hole` [column, row], a size [width,
/// height], a vector [x, y, z].
fn component_names(key: &str, count: usize) -> &'static [&'static str] {
    match (key, count) {
        ("hole" | "corner", 2) => &["column", "row"],
        ("size", 2) => &["width", "height"],
        ("pixels", 2) => &["across", "up"],
        ("offset", 2) => &["x", "y"],
        (_, 3) => &["x", "y", "z"],
        _ => &[],
    }
}

/// The unit of a key's numbers, shown after its field.
fn unit(key: &str) -> Option<&'static str> {
    Some(match key {
        "at" | "height" | "diameter" | "clear_aperture" | "focal_length" | "working_distance"
        | "pupil_distance" | "inner_diameter" | "hole_diameter" | "length" | "waist_diameter"
        | "core_diameter" | "thickness" | "size" | "pitch" => "mm",
        "wavelength_um" => "µm",
        "power_w" => "W",
        "fold" | "half_angle_deg" => "°",
        _ => return None,
    })
}

/// A number rounded to 1e-9, and whole when it is whole (holes stay `[4, 0]`).
pub fn tidy(v: f64) -> Value {
    let r = (v * 1e9).round() / 1e9;
    if r.fract() == 0.0 && r.abs() < 1e15 {
        json!(r as i64)
    } else {
        json!(r)
    }
}

/// The position of an entry (mm, scene frame): on a table layout X, Y and Z to type,
/// written back as its hole and height; otherwise as the scene placed it, to read.
fn position(
    ui: &mut Ui,
    drafts: &mut Drafts,
    base: &str,
    target: &Value,
    entry: &Value,
    origin: Option<V3>,
    context: &Context,
) -> Vec<Value> {
    let mut operations = Vec::new();
    let table = context
        .pitch
        .filter(|_| entry["hole"].is_array() && entry["height"].is_number());
    if table.is_none() && origin.is_none() {
        return operations;
    }
    ui.label(RichText::new("Position").strong());
    egui::Grid::new(format!("position-{base}"))
        .num_columns(3)
        .spacing([10.0, 4.0])
        .show(ui, |ui| match table {
            Some(pitch) => {
                let hole = [
                    entry["hole"][0].as_f64().unwrap_or(0.0),
                    entry["hole"][1].as_f64().unwrap_or(0.0),
                ];
                let height = entry["height"].as_f64().unwrap_or(0.0);
                for (k, label) in ["X", "Y", "Z"].into_iter().enumerate() {
                    ui.label(RichText::new(label).color(DARK.text_dim));
                    let current = if k < 2 { hole[k] * pitch } else { height };
                    let path = format!("{base}/position/{k}");
                    if let Some(v) = number(ui, drafts, &path, current) {
                        let mut op = target.clone();
                        op["op"] = json!("set");
                        if k < 2 {
                            let mut new = hole;
                            new[k] = v / pitch;
                            op["key"] = json!("hole");
                            op["value"] = json!([tidy(new[0]), tidy(new[1])]);
                        } else {
                            op["key"] = json!("height");
                            op["value"] = tidy(v);
                        }
                        operations.push(op);
                    }
                    ui.label(RichText::new("mm").color(DARK.text_dim).size(11.0));
                    ui.end_row();
                }
            }
            None => {
                let p = origin.unwrap_or([0.0; 3]);
                for (k, label) in ["X", "Y", "Z"].into_iter().enumerate() {
                    ui.label(RichText::new(label).color(DARK.text_dim));
                    ui.label(format!("{:.3}", p[k] + 0.0));
                    ui.label(RichText::new("mm").color(DARK.text_dim).size(11.0));
                    ui.end_row();
                }
            }
        });
    if table.is_some() {
        ui.label(
            RichText::new("X and Y move it by holes (fractions allowed), Z is its height")
                .color(DARK.text_dim)
                .size(11.0),
        );
    }
    ui.add_space(4.0);
    operations
}

/// The editor of one entry and its degrees of freedom; returns the operations to apply.
///
/// `origin` is where the scene placed it (mm), shown as its position.
pub fn entry(
    ui: &mut Ui,
    drafts: &mut Drafts,
    table: &str,
    entry: &Value,
    index: usize,
    context: &Context,
    origin: Option<V3>,
) -> Vec<Value> {
    let target = selector(table, entry, index);
    let base = format!(
        "{table}/{}",
        entry["id"].as_str().unwrap_or(&index.to_string())
    );
    let mut operations = position(ui, drafts, &base, &target, entry, origin, context);
    // On a table layout the height is the position's Z.
    let shown_as_z = context.pitch.is_some() && entry["hole"].is_array();
    egui::Grid::new(format!("entry-{base}"))
        .num_columns(3)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            for key in keys(entry) {
                if shown_as_z && key == "height" {
                    continue;
                }
                ui.label(RichText::new(&key).color(DARK.text_dim));
                let path = format!("{base}/{key}");
                if let Some(value) = field(ui, drafts, &path, table, &key, &entry[&key], context) {
                    let mut op = target.clone();
                    op["op"] = json!("set");
                    op["key"] = json!(key);
                    op["value"] = value;
                    operations.push(op);
                }
                match unit(&key) {
                    Some(u) => ui.label(RichText::new(u).color(DARK.text_dim).size(11.0)),
                    None => ui.label(""),
                };
                ui.end_row();
            }
        });
    if WITH_DOFS.contains(&table) && entry["id"].is_string() {
        ui.add_space(6.0);
        operations.extend(dofs(ui, drafts, table, entry, &base, context.sidecar));
    }
    ui.add_space(4.0);
    let what = if context.sidecar && table == "element" {
        "Untag"
    } else {
        "Remove"
    };
    if ui
        .small_button(format!("{what} {}", entry["id"].as_str().unwrap_or(table)))
        .on_hover_text("Take it out of the design (Undo puts it back)")
        .clicked()
    {
        let mut op = target;
        op["op"] = json!("remove");
        operations.push(op);
    }
    operations
}

/// The breadboard of a table layout (`[table]`): its pitch and size in holes, and the
/// hole at its corner. A board the file does not size is fitted around the items, and
/// can be kept as it is.
pub fn board(ui: &mut Ui, drafts: &mut Drafts, board: &crate::board::Board) -> Vec<Value> {
    let mut operations = Vec::new();
    let set = |key: &str, value: Value| json!({ "op": "set", "section": "table", "key": key, "value": value });
    ui.label(RichText::new("Breadboard").strong().size(14.0));
    egui::Grid::new("breadboard")
        .num_columns(3)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            ui.label(RichText::new("pitch").color(DARK.text_dim));
            if let Some(v) = number(ui, drafts, "table/pitch", board.pitch) {
                if v > 0.0 {
                    operations.push(set("pitch", tidy(v)));
                }
            }
            ui.label(RichText::new("mm").color(DARK.text_dim).size(11.0));
            ui.end_row();
            for (key, current, unit) in [
                ("columns", board.columns, "holes along x"),
                ("rows", board.rows, "holes along y"),
            ] {
                ui.label(RichText::new(key).color(DARK.text_dim));
                if let Some(v) = number(ui, drafts, &format!("table/{key}"), current as f64) {
                    let v = v.round().max(1.0) as i64;
                    if board.given {
                        operations.push(set(key, json!(v)));
                    } else {
                        // Sizing one side keeps the fitted size of the others.
                        operations.push(set("columns", json!(board.columns)));
                        operations.push(set("rows", json!(board.rows)));
                        operations.push(set("corner", json!([board.corner.0, board.corner.1])));
                        operations.push(set(key, json!(v)));
                    }
                }
                ui.label(RichText::new(unit).color(DARK.text_dim).size(11.0));
                ui.end_row();
            }
            ui.label(RichText::new("corner").color(DARK.text_dim));
            ui.horizontal(|ui| {
                for (k, name) in ["column", "row"].into_iter().enumerate() {
                    ui.label(RichText::new(name).color(DARK.text_dim).size(11.0));
                    let current = if k == 0 {
                        board.corner.0
                    } else {
                        board.corner.1
                    };
                    if let Some(v) =
                        number(ui, drafts, &format!("table/corner/{k}"), current as f64)
                    {
                        let mut corner = [board.corner.0, board.corner.1];
                        corner[k] = v.round() as i64;
                        if !board.given {
                            operations.push(set("columns", json!(board.columns)));
                            operations.push(set("rows", json!(board.rows)));
                        }
                        operations.push(set("corner", json!(corner)));
                    }
                }
            });
            ui.label(
                RichText::new("the hole at its corner")
                    .color(DARK.text_dim)
                    .size(11.0),
            );
            ui.end_row();
        });
    let ((x0, x1), (y0, y1)) = board.extent();
    ui.label(
        RichText::new(format!(
            "{:.0} × {:.0} mm, from ({x0:.1}, {y0:.1}) to ({x1:.1}, {y1:.1}) mm",
            x1 - x0,
            y1 - y0
        ))
        .color(DARK.text_dim),
    );
    if !board.given {
        ui.label(
            RichText::new("Not sized in the file: drawn around the items with two holes to spare")
                .color(DARK.text_dim)
                .size(11.0),
        );
        if ui
            .button("Keep this size")
            .on_hover_text("Write these columns, rows and corner into the file's [table]")
            .clicked()
        {
            operations.push(set("columns", json!(board.columns)));
            operations.push(set("rows", json!(board.rows)));
            operations.push(set("corner", json!([board.corner.0, board.corner.1])));
        }
    }
    operations
}

/// The edit a Move makes to a table layout's entry: a slide along x or y moves its hole
/// by `amount / pitch` (fractions kept), along z its height; a turn about a scene axis
/// turns its `axis` (degrees). `None` when the entry is not placed on the table.
pub fn moved(
    table: &str,
    entry: &Value,
    index: usize,
    pitch: f64,
    handle: crate::moving::Handle,
    amount: f64,
) -> Option<Value> {
    let hole = [entry["hole"][0].as_f64()?, entry["hole"][1].as_f64()?];
    let mut op = selector(table, entry, index);
    op["op"] = json!("set");
    match handle {
        crate::moving::Handle::Slide(k @ (0 | 1)) => {
            let mut new = hole;
            new[k] += amount / pitch;
            op["key"] = json!("hole");
            op["value"] = json!([tidy(new[0]), tidy(new[1])]);
        }
        crate::moving::Handle::Slide(_) => {
            op["key"] = json!("height");
            op["value"] = tidy(entry["height"].as_f64()? + amount);
        }
        crate::moving::Handle::Turn(k) => {
            let a = &entry["axis"];
            let axis = [a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?];
            let turned = crate::moving::turned(axis, [0.0; 3], k, amount);
            op["key"] = json!("axis");
            op["value"] = json!(turned.map(tidy));
        }
    }
    Some(op)
}

/// The edits a whole Move makes, its steps in order: the entry's hole, height and axis
/// where they changed. `None` when the entry is not placed on the table.
pub fn moved_all(
    table: &str,
    entry: &Value,
    index: usize,
    pitch: f64,
    steps: &[(crate::moving::Handle, f64)],
) -> Option<Vec<Value>> {
    let mut now = entry.clone();
    for &(handle, amount) in steps {
        let op = moved(table, &now, index, pitch, handle, amount)?;
        let key = op["key"].as_str()?.to_owned();
        now[key] = op["value"].clone();
    }
    let mut operations = Vec::new();
    for key in ["hole", "height", "axis"] {
        if now[key] != entry[key] {
            let mut op = selector(table, entry, index);
            op["op"] = json!("set");
            op["key"] = json!(key);
            op["value"] = now[key].clone();
            operations.push(op);
        }
    }
    Some(operations)
}

/// The tables whose entries carry degrees of freedom.
const WITH_DOFS: &[&str] = &["element", "source", "detector"];

/// The degrees of freedom of an entry: value within the range, remove; and a line to
/// add one.
/// In a layout the axes are the item's own (z along its optical axis); in a sidecar they
/// are the scene's, as the view's gizmo shows them.
fn dofs(
    ui: &mut Ui,
    drafts: &mut Drafts,
    table: &str,
    entry: &Value,
    base: &str,
    sidecar: bool,
) -> Vec<Value> {
    let mut operations = Vec::new();
    let id = entry["id"].as_str().unwrap_or_default();
    ui.label(RichText::new("Degrees of freedom").strong());
    let list = entry["dof"].as_array().cloned().unwrap_or_default();
    for dof in &list {
        let name = dof["name"].as_str().unwrap_or("?");
        let unit = if dof["kind"] == "tilt" { "°" } else { "mm" };
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).color(DARK.text_dim));
            let current = dof["value"].as_f64().unwrap_or(0.0);
            let range = (
                dof["range"][0].as_f64().unwrap_or(f64::NEG_INFINITY),
                dof["range"][1].as_f64().unwrap_or(f64::INFINITY),
            );
            let path = format!("{base}/dof/{name}");
            let mut value = *drafts.numbers.get(&path).unwrap_or(&current);
            let response = ui.add(
                egui::DragValue::new(&mut value)
                    .range(range.0..=range.1)
                    .speed(0.01)
                    .max_decimals(4)
                    .suffix(format!(" {unit}")),
            );
            if response.changed() {
                drafts.numbers.insert(path.clone(), value);
            }
            if response.drag_stopped() || response.lost_focus() {
                if let Some(v) = drafts.numbers.remove(&path) {
                    if v.to_bits() != current.to_bits() {
                        operations
                            .push(json!({ "op": "set", "dof": name, "key": "value", "value": v }));
                    }
                }
            }
            let kind = dof["kind"].as_str().unwrap_or("?");
            let locked = dof["locked"].as_bool().unwrap_or(false);
            ui.label(
                RichText::new(format!(
                    "{kind}, {} to {}{}",
                    range.0,
                    range.1,
                    if locked { ", locked" } else { "" }
                ))
                .color(DARK.text_dim)
                .size(11.0),
            );
            if ui
                .small_button("×")
                .on_hover_text("Remove this degree of freedom")
                .clicked()
            {
                operations.push(json!({ "op": "remove_dof", "dof": name }));
            }
        });
    }
    let frame = if sidecar { "scene" } else { "local" };
    let axes_named = if sidecar {
        "the scene's"
    } else {
        "its own (z along its optical axis)"
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Add").color(DARK.text_dim));
        for (kind, axis, label, range) in [
            ("slide", [1, 0, 0], "slide x", 5.0),
            ("slide", [0, 1, 0], "slide y", 5.0),
            ("slide", [0, 0, 1], "slide z", 5.0),
            ("tilt", [1, 0, 0], "tilt x", 2.0),
            ("tilt", [0, 1, 0], "tilt y", 2.0),
        ] {
            let short = label.replace(' ', "_");
            let name = format!("{id}_{short}");
            if list.iter().any(|d| d["name"] == name.as_str()) {
                continue;
            }
            if ui
                .small_button(label)
                .on_hover_text(format!(
                    "A {kind} along or about {axis:?} in {axes_named} axes, ±{range}"
                ))
                .clicked()
            {
                operations.push(json!({
                    "op": "add_dof", "table": table, "id": id,
                    "dof_entry": [["name", name], ["kind", kind], ["axis", axis], ["frame", frame], ["range", [-range, range]]],
                }));
            }
        }
    });
    operations
}

/// The next free id with a prefix: `L1`, `L2`, …
pub fn next_id(design: &Value, prefix: &str) -> String {
    let used: Vec<&str> = ["element", "source", "detector", "proxy"]
        .iter()
        .filter_map(|t| design[*t].as_array())
        .flatten()
        .filter_map(|e| e["id"].as_str())
        .collect();
    (1..)
        .map(|n| format!("{prefix}{n}"))
        .find(|id| !used.contains(&id.as_str()))
        .unwrap_or_else(|| prefix.to_owned())
}

/// What a layout can have added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum New {
    /// A biconvex singlet.
    Lens,
    /// A flat mirror.
    Mirror,
    /// A collimated beam.
    Source,
    /// A screen.
    Detector,
}

/// The height new items of a table layout get: the first source's, else 50 mm.
pub fn new_height(design: &Value) -> f64 {
    design["source"][0]["height"].as_f64().unwrap_or(50.0)
}

/// An `add` operation for a new entry of a layout, placed after what is there: on a
/// planar layout 50 mm further along the path, on a table layout two holes further
/// along x at the source's height.
pub fn add(design: &Value, new: New) -> Option<Value> {
    add_at(design, new, None)
}

/// As [`add`], on a table layout on the hole given (column, row) when there is one.
pub fn add_at(design: &Value, new: New, hole: Option<(i64, i64)>) -> Option<Value> {
    let planar = match design["layout"].as_str()? {
        "planar" => true,
        "table" => false,
        _ => return None,
    };
    let all = || {
        ["element", "source", "detector"]
            .iter()
            .filter_map(|t| design[*t].as_array())
            .flatten()
    };
    let place: Vec<(String, Value)> = if planar {
        let furthest = all()
            .filter_map(|e| e["at"].as_f64())
            .fold(f64::NEG_INFINITY, f64::max);
        let at = if furthest.is_finite() {
            furthest + 50.0
        } else {
            0.0
        };
        vec![("at".into(), json!(at))]
    } else {
        let furthest = all().filter_map(|e| e["hole"][0].as_i64()).max();
        let hole = hole.unwrap_or((furthest.map_or(0, |x| x + 2), 0));
        vec![
            ("hole".into(), json!([hole.0, hole.1])),
            ("height".into(), json!(new_height(design))),
        ]
    };
    let axis = |a: [i64; 3]| -> Vec<(String, Value)> {
        if planar {
            Vec::new()
        } else {
            vec![("axis".into(), json!(a))]
        }
    };
    let (table, prefix, fields): (&str, &str, Vec<(String, Value)>) = match new {
        New::Lens => (
            "element",
            "L",
            [
                vec![("kind".into(), json!("lens"))],
                place,
                axis([1, 0, 0]),
                vec![
                    (
                        "surfaces".into(),
                        json!([{ "radius": 50.0 }, { "radius": -50.0 }]),
                    ),
                    ("thickness".into(), json!([4.0])),
                    ("material".into(), json!([1.5])),
                    ("diameter".into(), json!(25.4)),
                    ("clear_aperture".into(), json!(22.9)),
                ],
            ]
            .concat(),
        ),
        New::Mirror => (
            "element",
            "M",
            [
                vec![("kind".into(), json!("mirror"))],
                place,
                if planar {
                    vec![("fold".into(), json!(90.0))]
                } else {
                    axis([-1, 1, 0])
                },
                vec![
                    ("diameter".into(), json!(25.4)),
                    ("clear_aperture".into(), json!(22.9)),
                ],
            ]
            .concat(),
        ),
        New::Source => (
            "source",
            "S",
            [
                vec![
                    ("kind".into(), json!("collimated")),
                    ("wavelength_um".into(), json!(0.55)),
                    ("power_w".into(), json!(0.001)),
                    ("diameter".into(), json!(4.0)),
                ],
                place,
                axis([1, 0, 0]),
            ]
            .concat(),
        ),
        New::Detector => (
            "detector",
            "D",
            [
                place,
                axis([1, 0, 0]),
                vec![
                    ("size".into(), json!([20, 20])),
                    ("pixels".into(), json!([64, 64])),
                ],
            ]
            .concat(),
        ),
    };
    let id = next_id(design, prefix);
    let mut entry = vec![json!(["id", id])];
    entry.extend(fields.into_iter().map(|(k, v)| json!([k, v])));
    Some(json!({ "op": "add", "table": table, "entry": entry }))
}

/// Tags a CAD body as a lens or, with the normal of the face clicked (scene frame), a
/// mirror reflecting on that face: an `add` operation for the sidecar. `solid` is the
/// body's item id, `<occurrence>/<body>`.
pub fn tag(design: &Value, solid: &str, mirror_face: Option<(V3, [V3; 3])>) -> Option<Value> {
    let (occurrence, body) = solid.rsplit_once('/')?;
    let reference = json!({ "occurrence": occurrence, "body": body });
    let entry = match mirror_face {
        None => json!([
            ["id", next_id(design, "L")],
            ["kind", "lens"],
            ["body", reference],
            ["material", 1.5],
        ]),
        Some((normal, axes)) => {
            // The face normal in the body's own frame, made unit again (the axes come
            // rounded) and rounded to 1e-4, the precision of the triangles it was read
            // from; `reflect` picks faces within 1°, so this keeps the file legible.
            let local = axes.map(|a| normal[0] * a[0] + normal[1] * a[1] + normal[2] * a[2]);
            let length = local.iter().map(|c| c * c).sum::<f64>().sqrt().max(1e-300);
            let local = local.map(|c| (c / length * 1e4).round() / 1e4 + 0.0);
            json!([
                ["id", next_id(design, "M")],
                ["kind", "mirror"],
                ["body", reference],
                ["reflect", local],
            ])
        }
    };
    Some(json!({ "op": "add", "table": "element", "entry": entry }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_entries_get_free_ids_and_places_after_the_rest() {
        let planar = json!({ "layout": "planar", "source": [{ "id": "S1", "at": 0.0 }],
                             "element": [{ "id": "L1", "at": 50.0 }, { "id": "M1", "at": 120.0 }] });
        let op = add(&planar, New::Lens).expect("an op");
        assert_eq!(op["table"], "element");
        assert_eq!(op["entry"][0], json!(["id", "L2"]));
        assert!(
            op["entry"]
                .as_array()
                .expect("pairs")
                .contains(&json!(["at", 170.0]))
        );
        let table = json!({ "layout": "table", "source": [{ "id": "S1", "hole": [0, 0], "height": 75.0 }],
                            "element": [{ "id": "M1", "hole": [4, 0] }] });
        let op = add(&table, New::Detector).expect("an op");
        let entry = op["entry"].as_array().expect("pairs");
        assert!(entry.contains(&json!(["hole", [6, 0]])));
        assert!(entry.contains(&json!(["height", 75.0])));
        assert_eq!(entry[0], json!(["id", "D1"]));
        // A sidecar is not a layout.
        assert!(add(&json!({ "kind": "sidecar" }), New::Lens).is_none());
        // Placed on a hole clicked.
        let op = add_at(&table, New::Mirror, Some((3, -2))).expect("an op");
        assert!(
            op["entry"]
                .as_array()
                .expect("pairs")
                .contains(&json!(["hole", [3, -2]]))
        );
    }

    #[test]
    fn moves_write_holes_heights_and_axes() {
        use crate::moving::Handle;
        let entry = json!({ "id": "M1", "hole": [4, 0], "height": 75.0, "axis": [-1, 1, 0] });
        let op = moved("element", &entry, 0, 25.0, Handle::Slide(0), 50.0).expect("an op");
        assert_eq!(op["key"], "hole");
        assert_eq!(op["value"], json!([6, 0]));
        let op = moved("element", &entry, 0, 25.0, Handle::Slide(1), -12.5).expect("an op");
        assert_eq!(op["value"], json!([4, -0.5]));
        let op = moved("element", &entry, 0, 25.0, Handle::Slide(2), 25.0).expect("an op");
        assert_eq!(
            (op["key"].as_str(), op["value"].as_f64()),
            (Some("height"), Some(100.0))
        );
        // A mirror facing (−1, 1, 0) turned 90° about z faces (−1, −1, 0).
        let op = moved("element", &entry, 0, 25.0, Handle::Turn(2), 90.0).expect("an op");
        assert_eq!(op["key"], "axis");
        assert_eq!(op["value"], json!([-1, -1, 0]));
        assert_eq!(op["id"], "M1");
        assert!(
            moved(
                "element",
                &json!({ "id": "L", "at": 5 }),
                0,
                25.0,
                Handle::Slide(0),
                1.0
            )
            .is_none()
        );
    }

    #[test]
    fn a_mirror_is_tagged_by_the_face_clicked_in_the_body_frame() {
        let design = json!({ "element": [{ "id": "M1" }] });
        // The body turned 90° about z: its x axis along scene y.
        let axes = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let op = tag(&design, "mirror:2/Mirror", Some(([0.0, 1.0, 0.0], axes))).expect("an op");
        let entry = op["entry"].as_array().expect("pairs");
        assert_eq!(entry[0], json!(["id", "M2"]));
        assert!(entry.contains(&json!(["body", { "occurrence": "mirror:2", "body": "Mirror" }])));
        assert!(entry.contains(&json!(["reflect", [1.0, 0.0, 0.0]])));
        let lens = tag(&design, "lens:1/Lens", None).expect("an op");
        assert!(
            lens["entry"]
                .as_array()
                .expect("pairs")
                .contains(&json!(["kind", "lens"]))
        );
        assert!(tag(&design, "no-slash", None).is_none());
    }
}
