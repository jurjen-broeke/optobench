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

    /// Sets a text being typed or chosen, as a test would by hand.
    #[cfg(test)]
    pub fn set_text(&mut self, path: &str, text: &str) {
        self.texts.insert(path.to_owned(), text.to_owned());
    }
}

/// The choices for keys that take one of a few words.
fn choices(table: &str, key: &str) -> Option<&'static [&'static str]> {
    match (table, key) {
        ("element", "kind") => Some(&["lens", "mirror", "filter"]),
        ("source", "kind") => Some(&["collimated", "point", "gaussian"]),
        ("merit", "kind") => Some(&[
            "focus",
            "centring",
            "pointing",
            "collimation",
            "diameter",
            "power",
        ]),
        ("proxy", "kind") => Some(&["plate", "tube"]),
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
    /// The groups items are in, and those the file declares.
    pub groups: Vec<String>,
    /// Where each item and source with an id stands (mm, scene frame), for aiming
    /// mirrors.
    pub places: Vec<(String, V3)>,
    /// The mounts an item can be given: the file's `[[mount]]` tables, then the parts
    /// library's.
    pub mounts: Vec<Mount>,
}

/// A mount an item can name: its id (the file's) or key (the library's), where its
/// ranges come from, and its adjusters (`dof` entries without an item).
#[derive(Debug, Clone, PartialEq)]
pub struct Mount {
    /// What `mount = "…"` names.
    pub name: String,
    /// Its `source`.
    pub source: String,
    /// Its adjusters as written.
    pub adjusters: Vec<Value>,
    /// Whether it comes from the parts library.
    pub library: bool,
}

/// The mounts of a design: its own `[[mount]]` tables first, then the library's (`parts`
/// entries of type `mount`) except those a table of the same id (any case) stands in
/// for.
pub fn mounts_of(design: &Value, library: &[Value]) -> Vec<Mount> {
    let mut own: Vec<Mount> = design["mount"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(Mount {
                name: m["id"].as_str()?.to_owned(),
                source: m["source"].as_str().unwrap_or_default().to_owned(),
                adjusters: m["adjusters"].as_array().cloned().unwrap_or_default(),
                library: false,
            })
        })
        .collect();
    let theirs: Vec<Mount> = library
        .iter()
        .filter_map(|p| {
            let key = p["key"].as_str()?;
            if own.iter().any(|m| m.name.eq_ignore_ascii_case(key)) {
                return None;
            }
            Some(Mount {
                name: key.to_owned(),
                source: p["source"].as_str().unwrap_or_default().to_owned(),
                adjusters: p["adjusters"].as_array().cloned().unwrap_or_default(),
                library: true,
            })
        })
        .collect();
    own.extend(theirs);
    own
}

/// An adjuster in words: `slide along x, −1 to 1 mm`, `tilt about z, unlimited`.
fn adjuster_text(adjuster: &Value) -> String {
    let tilt = adjuster["kind"] == "tilt";
    let axis = match &adjuster["axis"] {
        Value::String(a) => a.clone(),
        Value::Array(v) => {
            let names = ["x", "y", "z"];
            let numbers: Vec<f64> = v.iter().filter_map(Value::as_f64).collect();
            match numbers
                .iter()
                .enumerate()
                .filter(|(_, x)| **x != 0.0)
                .collect::<Vec<_>>()
                .as_slice()
            {
                [(k, x)] => {
                    format!("{}{}", if **x < 0.0 { "−" } else { "" }, names[*k])
                }
                _ => format!("{numbers:?}"),
            }
        }
        _ => "?".into(),
    };
    let unit = if tilt { "°" } else { " mm" };
    let reach = match (&adjuster["range"], adjuster["travel"].as_f64()) {
        (_, Some(t)) => format!("{t}{unit} of travel"),
        (Value::Array(r), _) if r.len() == 2 => format!(
            "{} to {}{unit}",
            r[0].as_f64().unwrap_or(0.0),
            r[1].as_f64().unwrap_or(0.0)
        ),
        _ => "unlimited".into(),
    };
    format!(
        "{} {axis}, {reach}",
        if tilt { "tilt about" } else { "slide along" }
    )
}

/// The mount an item stands in: none, or one of the design's or the library's, and the
/// degrees of freedom it gives (`<id>_<name>`). A stack of mounts (a list) is shown and
/// kept until one mount is chosen in its place.
fn mount_field(
    ui: &mut Ui,
    base: &str,
    target: &Value,
    entry: &Value,
    context: &Context,
) -> Option<Value> {
    let names: Vec<String> = match &entry["mount"] {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a
            .iter()
            .filter_map(|m| m.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    if context.mounts.is_empty() && names.is_empty() {
        return None;
    }
    let shown = if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(" on ")
    };
    let mut chosen: Option<Option<String>> = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new("mount").color(DARK.text_dim))
            .on_hover_text("What it stands in: a mount's adjusters become its degrees of freedom");
        egui::ComboBox::from_id_salt(format!("{base}/mount"))
            .selected_text(&shown)
            .show_ui(ui, |ui| {
                if ui.selectable_label(names.is_empty(), "none").clicked() {
                    chosen = Some(None);
                }
                for m in &context.mounts {
                    let on = names.len() == 1 && names[0].eq_ignore_ascii_case(&m.name);
                    let mut response = ui.selectable_label(on, &m.name);
                    if !m.source.is_empty() {
                        response = response.on_hover_text(&m.source);
                    }
                    if response.clicked() {
                        chosen = Some(Some(m.name.clone()));
                    }
                }
            });
    });
    let id = entry["id"].as_str().unwrap_or_default();
    for name in &names {
        match context
            .mounts
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
        {
            Some(m) => {
                for a in &m.adjusters {
                    ui.label(
                        RichText::new(format!(
                            "    {id}_{}: {}",
                            a["name"].as_str().unwrap_or("?"),
                            adjuster_text(a)
                        ))
                        .color(DARK.text_dim)
                        .size(11.0),
                    );
                }
            }
            None => {
                ui.label(
                    RichText::new(format!("    {name}: not in the file or the parts library"))
                        .color(DARK.warning)
                        .size(11.0),
                );
            }
        }
    }
    let mut op = target.clone();
    match chosen? {
        None if !names.is_empty() => {
            op["op"] = json!("unset");
            op["key"] = json!("mount");
            Some(op)
        }
        Some(name) if !(names.len() == 1 && names[0] == name) => {
            op["op"] = json!("set");
            op["key"] = json!("mount");
            op["value"] = json!(name);
            Some(op)
        }
        _ => None,
    }
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
            groups: groups_of(design),
            places: Vec::new(),
            mounts: Vec::new(),
        }
    }

    /// As [`Context::of`], with the mounts items can be given ([`mounts_of`]).
    pub fn with_mounts(mut self, design: &Value, library: &[Value]) -> Self {
        self.mounts = mounts_of(design, library);
        self
    }

    /// As [`Context::of`], with where the scene placed each item and source.
    pub fn with_places(mut self, scene: &crate::document::Scene) -> Self {
        let mut places: Vec<(String, V3)> = scene
            .items
            .iter()
            .filter(|i| !i.id.is_empty())
            .filter_map(|i| Some((i.id.clone(), i.origin?)))
            .chain(scene.sources.iter().map(|s| (s.id.clone(), s.origin)))
            .collect();
        places.dedup_by(|a, b| a.0 == b.0);
        self.places = places;
        self
    }
}

/// A mirror's axis (its normal, towards the light) that reflects light coming from
/// `from` at `at` towards `to`: the outgoing direction minus the incoming one (the law of
/// reflection: the normal halves the turn), scaled so its largest number is 1 and rounded
/// to 1e-9. `None` when the light would go straight on (no mirror does that), or a place
/// is the mirror's own.
pub fn aim(from: V3, at: V3, to: V3) -> Option<V3> {
    let unit = |a: V3, b: V3| {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        (length > 1e-9).then(|| d.map(|x| x / length))
    };
    let incoming = unit(from, at)?;
    let outgoing = unit(at, to)?;
    let normal: V3 = std::array::from_fn(|k| outgoing[k] - incoming[k]);
    let largest = normal.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    if largest < 1e-6 {
        return None;
    }
    Some(normal.map(|x| ((x / largest) * 1e9).round() / 1e9 + 0.0))
}

/// The axis of an item that faces light coming from `from` at `at` (a lens, an iris, a
/// screen): along the light, scaled so its largest number is 1 and rounded to 1e-9.
/// `None` when `from` is at its own place.
pub fn facing(from: V3, at: V3) -> Option<V3> {
    let d: V3 = std::array::from_fn(|k| at[k] - from[k]);
    let largest = d.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    (largest > 1e-9).then(|| d.map(|x| ((x / largest) * 1e9).round() / 1e9 + 0.0))
}

/// A proxy turned into another kind: its kind set and the keys of the old kind replaced
/// by the new one's (a plate's hole and size, a tube's bore and length; on a planar
/// layout a plate's `at` and a tube's `from` and `to`), so the design still reads.
pub fn proxy_kind(target: &Value, entry: &Value, kind: &str) -> Vec<Value> {
    let op = |op: &str, key: &str, value: Option<Value>| {
        let mut o = target.clone();
        o["op"] = json!(op);
        o["key"] = json!(key);
        if let Some(v) = value {
            o["value"] = v;
        }
        o
    };
    let planar = entry["at"].is_number() || entry["from"].is_number();
    let mut ops = vec![op("set", "kind", Some(json!(kind)))];
    let unset = |ops: &mut Vec<Value>, keys: &[&str]| {
        for k in keys {
            if !entry[*k].is_null() {
                ops.push(op("unset", k, None));
            }
        }
    };
    match kind {
        "plate" => {
            unset(&mut ops, &["inner_diameter", "length", "from", "to"]);
            ops.push(op("set", "hole_diameter", Some(json!(5.0))));
            ops.push(op("set", "size", Some(json!(30.0))));
            if planar {
                ops.push(op("set", "at", Some(entry["from"].clone())));
            }
        }
        "tube" => {
            unset(&mut ops, &["hole_diameter", "size", "at"]);
            ops.push(op("set", "inner_diameter", Some(json!(25.4))));
            if planar {
                let at = entry["at"].as_f64().unwrap_or(0.0);
                ops.push(op("set", "from", Some(tidy(at))));
                ops.push(op("set", "to", Some(tidy(at + 50.0))));
            } else {
                ops.push(op("set", "length", Some(json!(50.0))));
            }
        }
        _ => return Vec::new(),
    }
    ops
}

/// Aim, for an item with an axis: where the light comes from (any other item) and, for
/// a mirror, where it should go; returns the `set` of its axis. A mirror's axis halves
/// the turn ([`aim`]), anything else faces along the light ([`facing`]).
#[allow(clippy::too_many_arguments)]
fn aim_editor(
    ui: &mut Ui,
    drafts: &mut Drafts,
    base: &str,
    target: &Value,
    entry: &Value,
    origin: V3,
    mirror: bool,
    context: &Context,
) -> Option<Value> {
    let own = entry["id"].as_str().unwrap_or_default();
    let others: Vec<&(String, V3)> = context.places.iter().filter(|(id, _)| id != own).collect();
    if others.is_empty() {
        return None;
    }
    let mut op = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("aim").color(DARK.text_dim))
            .on_hover_text(if mirror {
                "Set its axis from where the light comes from and where it should go"
            } else {
                "Turn it to face the light from an item"
            });
        let picks: &[(&str, &str)] = if mirror {
            &[("light from", "from"), ("to", "to")]
        } else {
            &[("light from", "from")]
        };
        for &(label, key) in picks {
            ui.label(RichText::new(label).color(DARK.text_dim).size(11.0));
            let path = format!("{base}/aim/{key}");
            let current = drafts.texts.get(&path).cloned().unwrap_or_default();
            egui::ComboBox::from_id_salt(&path)
                .selected_text(if current.is_empty() { "…" } else { &current })
                .width(70.0)
                .show_ui(ui, |ui| {
                    for (id, _) in &others {
                        if ui.selectable_label(*id == current, id.as_str()).clicked() {
                            drafts.texts.insert(path.clone(), id.clone());
                        }
                    }
                });
        }
        let place = |key: &str| {
            let id = drafts.texts.get(&format!("{base}/aim/{key}"))?;
            others.iter().find(|(o, _)| o == id).map(|(_, p)| *p)
        };
        let (from, to) = (place("from"), place("to"));
        let ready = from.is_some() && (to.is_some() || !mirror);
        if ui
            .add_enabled(ready, egui::Button::new("Aim"))
            .on_hover_text(if mirror {
                "Turn it so the light from the first goes to the second"
            } else {
                "Turn it to face the light from that item"
            })
            .clicked()
        {
            let from = from.unwrap_or_default();
            let axis = if mirror {
                aim(from, origin, to.unwrap_or_default())
            } else {
                facing(from, origin)
            };
            match axis {
                Some(axis) => {
                    let mut set = target.clone();
                    set["op"] = json!("set");
                    set["key"] = json!("axis");
                    set["value"] = json!(axis.map(tidy));
                    op = Some(set);
                    drafts.texts.remove(&format!("{base}/aim/note"));
                }
                None => {
                    drafts.texts.insert(
                        format!("{base}/aim/note"),
                        if mirror {
                            "No mirror lets light go straight on, or aims at its own place"
                        } else {
                            "That item stands at its own place"
                        }
                        .into(),
                    );
                }
            }
        }
    });
    if let Some(note) = drafts.texts.get(&format!("{base}/aim/note")) {
        ui.label(RichText::new(note).color(DARK.warning).size(11.0));
    }
    op
}

/// Every group of a design: those items are in and those `[[group]]` declares, sorted.
pub fn groups_of(design: &Value) -> Vec<String> {
    let mut groups: Vec<String> = ["element", "source", "detector", "proxy"]
        .iter()
        .filter_map(|t| design[*t].as_array())
        .flatten()
        .filter_map(|e| e["group"].as_str())
        .chain(
            design["group"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|g| g["id"].as_str()),
        )
        .map(str::to_owned)
        .collect();
    groups.sort();
    groups.dedup();
    groups
}

/// The group an entry is in: none, one of the design's, or a new one typed.
fn group_field(
    ui: &mut Ui,
    drafts: &mut Drafts,
    base: &str,
    target: &Value,
    entry: &Value,
    context: &Context,
) -> Option<Value> {
    let current = entry["group"].as_str();
    let mut op = target.clone();
    let mut chosen: Option<Option<String>> = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new("group").color(DARK.text_dim))
            .on_hover_text("Items in a group move together: Move on any of them moves all");
        egui::ComboBox::from_id_salt(format!("{base}/group"))
            .selected_text(current.unwrap_or("none"))
            .show_ui(ui, |ui| {
                if ui.selectable_label(current.is_none(), "none").clicked() {
                    chosen = Some(None);
                }
                for g in &context.groups {
                    if ui
                        .selectable_label(current == Some(g.as_str()), g)
                        .clicked()
                    {
                        chosen = Some(Some(g.clone()));
                    }
                }
            });
        let path = format!("{base}/group/new");
        if let Some(typed) = text(ui, drafts, &path, "", 90.0) {
            let typed = typed.trim().to_owned();
            if !typed.is_empty() {
                chosen = Some(Some(typed));
            }
        }
        ui.label(
            RichText::new("or a new one")
                .color(DARK.text_dim)
                .size(11.0),
        );
    });
    match chosen? {
        None if current.is_some() => {
            op["op"] = json!("unset");
            op["key"] = json!("group");
            Some(op)
        }
        Some(g) if current != Some(g.as_str()) => {
            op["op"] = json!("set");
            op["key"] = json!("group");
            op["value"] = json!(g);
            Some(op)
        }
        _ => None,
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
/// `origin` is where the scene placed it (mm), shown as its position; `lens` a lens's
/// properties from `view`.
#[allow(clippy::too_many_arguments)]
pub fn entry(
    ui: &mut Ui,
    drafts: &mut Drafts,
    table: &str,
    entry: &Value,
    index: usize,
    context: &Context,
    origin: Option<V3>,
    lens: Option<&Value>,
) -> Vec<Value> {
    let target = selector(table, entry, index);
    let base = format!(
        "{table}/{}",
        entry["id"].as_str().unwrap_or(&index.to_string())
    );
    let mut operations = Vec::new();
    // Its name first.
    if let Some(id) = entry["id"].as_str() {
        ui.horizontal(|ui| {
            ui.label(RichText::new("name").color(DARK.text_dim));
            if let Some(typed) = text(ui, drafts, &format!("{base}/id"), id, 160.0) {
                let mut op = target.clone();
                op["op"] = json!("set");
                op["key"] = json!("id");
                op["value"] = json!(typed.trim());
                operations.push(op);
            }
        });
    }
    // A lens's own editor next: its type, focal length, radii, thickness, glass, size.
    let prescribed = table == "element" && entry["surfaces"].is_array();
    if table == "element" && (prescribed || lens.is_some()) {
        operations.extend(lens_editor(ui, drafts, &base, &target, entry, lens));
    }
    operations.extend(position(ui, drafts, &base, &target, entry, origin, context));
    if ["element", "detector", "proxy"].contains(&table) && entry["axis"].is_array() {
        if let Some(at) = origin {
            let mirror = table == "element" && entry["kind"] == "mirror";
            operations.extend(aim_editor(
                ui, drafts, &base, &target, entry, at, mirror, context,
            ));
        }
    }
    if ["element", "source", "detector", "proxy"].contains(&table) && entry["id"].is_string() {
        operations.extend(group_field(ui, drafts, &base, &target, entry, context));
    }
    if ["element", "source", "detector"].contains(&table) && entry["id"].is_string() {
        operations.extend(mount_field(ui, &base, &target, entry, context));
    }
    // On a table layout the height is the position's Z.
    let shown_as_z = context.pitch.is_some() && entry["hole"].is_array();
    egui::Grid::new(format!("entry-{base}"))
        .num_columns(3)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            for key in keys(entry) {
                if (shown_as_z && key == "height")
                    || ["group", "id", "mount"].contains(&key.as_str())
                {
                    continue;
                }
                let in_lens_editor = matches!(
                    key.as_str(),
                    "surfaces" | "thickness" | "material" | "diameter" | "clear_aperture"
                );
                if prescribed && in_lens_editor {
                    continue;
                }
                if table == "element" && entry["library"].is_string() && key == "reversed" {
                    continue;
                }
                ui.label(RichText::new(&key).color(DARK.text_dim));
                let path = format!("{base}/{key}");
                if let Some(value) = field(ui, drafts, &path, table, &key, &entry[&key], context) {
                    if table == "proxy" && key == "kind" {
                        operations.extend(proxy_kind(&target, entry, value.as_str().unwrap_or("")));
                        ui.label("");
                        ui.end_row();
                        continue;
                    }
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
        operations.extend(dofs(ui, drafts, table, entry, &base, context));
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

/// A radius as a file writes it: a number, or "inf" for flat.
fn radius_value(r: f64) -> Value {
    if r.is_finite() {
        tidy((r * 1e4).round() / 1e4)
    } else {
        json!("inf")
    }
}

/// A radius as written: a number, or "inf" / "-inf".
fn radius_of(value: &Value) -> f64 {
    match value {
        Value::String(s) if s.starts_with('-') => f64::NEG_INFINITY,
        Value::String(_) => f64::INFINITY,
        v => v.as_f64().unwrap_or(f64::INFINITY),
    }
}

/// A number field with a label and unit in a grid row; the new value when changed.
fn row(
    ui: &mut Ui,
    drafts: &mut Drafts,
    path: &str,
    label: &str,
    help: &str,
    unit: &str,
    current: f64,
) -> Option<f64> {
    ui.label(RichText::new(label).color(DARK.text_dim))
        .on_hover_text(help);
    let changed = number(ui, drafts, path, current);
    ui.label(RichText::new(unit).color(DARK.text_dim).size(11.0));
    ui.end_row();
    changed
}

/// A lens's own editor, in the order a lens is chosen: its type; its focal length,
/// radius, centre thickness and glass, kept consistent (a change to one redesigns the
/// others: the radius follows the focal length, thickness and index; the focal length
/// follows the radius); its diameter and clear aperture, made smaller when the lens's
/// curvature leaves no edge; turned round by Flip. A lens of more glasses (a doublet) is
/// edited radius by radius; a library lens shows its properties and is flipped by
/// `reversed`.
fn lens_editor(
    ui: &mut Ui,
    drafts: &mut Drafts,
    base: &str,
    target: &Value,
    entry: &Value,
    lens: Option<&Value>,
) -> Vec<Value> {
    use crate::lens::{Shape, Singlet};
    let mut operations = Vec::new();
    let set = |key: &str, value: Value| {
        let mut op = target.clone();
        op["op"] = json!("set");
        op["key"] = json!(key);
        op["value"] = value;
        op
    };
    let wavelength = lens.and_then(|l| l["wavelength_um"].as_f64());
    let shown = |key: &str| lens.and_then(|l| l[key].as_f64());
    ui.label(RichText::new("Lens").strong());

    // A library lens: what it is, turned round or not.
    if !entry["surfaces"].is_array() {
        egui::Grid::new(format!("lens-{base}"))
            .num_columns(3)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                ui.label(RichText::new("type").color(DARK.text_dim));
                ui.label(lens.and_then(|l| l["shape"].as_str()).unwrap_or("—"));
                ui.label("");
                ui.end_row();
                for (label, key) in [("f", "focal_length_mm"), ("fb", "back_focal_length_mm")] {
                    ui.label(RichText::new(label).color(DARK.text_dim));
                    ui.label(shown(key).map_or("—".into(), |v| format!("{v:.3}")));
                    ui.label(RichText::new("mm").color(DARK.text_dim).size(11.0));
                    ui.end_row();
                }
            });
        let reversed = entry["reversed"].as_bool().unwrap_or(false);
        if ui
            .button("Flip")
            .on_hover_text("Turn the lens round")
            .clicked()
        {
            operations.push(set("reversed", json!(!reversed)));
        }
        ui.label(
            RichText::new("Its prescription, size and coating come from the parts library")
                .color(DARK.text_dim)
                .size(11.0),
        );
        ui.add_space(4.0);
        return operations;
    }

    let surfaces: Vec<Value> = entry["surfaces"].as_array().cloned().unwrap_or_default();
    let thickness: Vec<Value> = entry["thickness"].as_array().cloned().unwrap_or_default();
    let material: Vec<Value> = entry["material"].as_array().cloned().unwrap_or_default();
    let diameter = entry["diameter"].as_f64().unwrap_or(25.4);
    let clear = entry["clear_aperture"].as_f64().unwrap_or(0.9 * diameter);
    let singlet_now = surfaces.len() == 2 && material.len() == 1;
    // The index: the glass's own number, else what the scene found at the wavelength.
    let n = material
        .first()
        .and_then(Value::as_f64)
        .or_else(|| lens.and_then(|l| l["indices"][0].as_f64()));
    let custom_path = format!("{base}/custom");
    let custom = !singlet_now || drafts.texts.get(&custom_path).is_some_and(|c| c == "yes");

    // Writes a singlet, keeping its diameter where its curvature leaves an edge.
    let write = |operations: &mut Vec<Value>, lens: Singlet, glass: Value| {
        operations.push(set(
            "surfaces",
            json!([{ "radius": radius_value(lens.r1) }, { "radius": radius_value(lens.r2) }]),
        ));
        operations.push(set("thickness", json!([tidy(lens.t)])));
        operations.push(set("material", json!([glass])));
        let largest = lens.max_diameter();
        if diameter > largest && largest > 0.0 {
            operations.push(set("diameter", tidy(largest)));
            operations.push(set(
                "clear_aperture",
                tidy((clear.min(0.9 * largest) * 10.0).floor() / 10.0),
            ));
        }
    };

    // The type.
    let now = singlet_now.then(|| Singlet {
        r1: radius_of(&surfaces[0]["radius"]),
        r2: radius_of(&surfaces[1]["radius"]),
        t: thickness.first().and_then(Value::as_f64).unwrap_or(4.0),
        n: n.unwrap_or(1.5),
    });
    let (shape, flipped) = now.map_or((Shape::Biconvex, false), |l| l.shape());
    let glass = material.first().cloned().unwrap_or(json!(1.5));
    let mut chosen: Option<Option<Shape>> = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new("type").color(DARK.text_dim));
        let shown = if custom {
            "custom (radius by radius)"
        } else {
            shape.name()
        };
        egui::ComboBox::from_id_salt(format!("{base}/type"))
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for s in Shape::ALL {
                    if ui
                        .selectable_label(!custom && s == shape, s.name())
                        .clicked()
                    {
                        chosen = Some(Some(s));
                    }
                }
                if ui
                    .selectable_label(custom, "custom (radius by radius)")
                    .clicked()
                {
                    chosen = Some(None);
                }
            });
        if ui
            .button("Flip")
            .on_hover_text("Turn the lens round: its surfaces in the other order")
            .clicked()
        {
            if let Some(l) = now {
                write(&mut operations, l.flipped(), glass.clone());
            } else {
                let back = |v: &Value| {
                    let r = radius_of(&v["radius"]);
                    json!({ "radius": radius_value(if r.is_finite() { -r } else { r }) })
                };
                operations.push(set(
                    "surfaces",
                    json!(surfaces.iter().rev().map(back).collect::<Vec<_>>()),
                ));
                operations.push(set(
                    "thickness",
                    json!(thickness.iter().rev().cloned().collect::<Vec<_>>()),
                ));
                operations.push(set(
                    "material",
                    json!(material.iter().rev().cloned().collect::<Vec<_>>()),
                ));
            }
        }
    });
    match chosen {
        Some(None) => {
            drafts.texts.insert(custom_path.clone(), "yes".into());
        }
        Some(Some(new)) => {
            drafts.texts.remove(&custom_path);
            // Keep the focal length (100 mm for a lens without one), the thickness, glass.
            let base_lens = now.unwrap_or(Singlet {
                r1: 50.0,
                r2: -50.0,
                t: 4.0,
                n: n.unwrap_or(1.5),
            });
            let f = Some(base_lens.focal_length())
                .filter(|f| f.is_finite())
                .unwrap_or(100.0);
            let f = if new == Shape::Meniscus { f.abs() } else { f };
            match base_lens.design(
                new,
                f,
                flipped && !matches!(new, Shape::Biconvex | Shape::Biconcave),
            ) {
                Some(l) => write(&mut operations, l, glass.clone()),
                None => {
                    drafts.texts.insert(
                        format!("{base}/note"),
                        format!("No {} of f {f:.1} mm with this thickness", new.name()),
                    );
                }
            }
        }
        None => {}
    }

    if custom {
        operations.extend(prescription_editor(
            ui, drafts, base, &set, &surfaces, &thickness, &material,
        ));
    } else if let (Some(l), Some(n)) = (now, n) {
        let f = l.focal_length();
        egui::Grid::new(format!("lens-{base}")).num_columns(3).spacing([10.0, 4.0]).show(ui, |ui| {
            // Focal length: the radius follows.
            if let Some(v) = row(ui, drafts, &format!("{base}/f"), "f", "focal length, from the rear principal plane", "mm", (f * 1e4).round() / 1e4) {
                match l.design(shape, v, flipped) {
                    Some(new) => write(&mut operations, new, glass.clone()),
                    None => {
                        drafts.texts.insert(format!("{base}/note"), format!("No {} of f {v:.1} mm with this thickness", shape.name()));
                    }
                }
            }
            // The radius: one for plano and equi lenses (the focal length follows), two
            // for a meniscus.
            let curved = if l.r1.is_finite() { l.r1 } else { l.r2 };
            match shape {
                Shape::Meniscus => {
                    for (k, r) in [(1, l.r1), (2, l.r2)] {
                        if let Some(v) = row(ui, drafts, &format!("{base}/R{k}"), &format!("R{k}"), "radius of curvature, positive when its centre lies further along the light", "mm", r) {
                            if v != 0.0 {
                                let mut new = l;
                                if k == 1 { new.r1 = v } else { new.r2 = v }
                                write(&mut operations, new, glass.clone());
                            }
                        }
                    }
                }
                _ => {
                    if let Some(v) = row(ui, drafts, &format!("{base}/R"), "R", "radius of its curved surface (both, for an equi-convex or -concave lens)", "mm", curved.abs()) {
                        if v > 0.0 {
                            let sign1 = if l.r1.is_finite() { l.r1.signum() } else { 0.0 };
                            let sign2 = if l.r2.is_finite() { l.r2.signum() } else { 0.0 };
                            let new = Singlet {
                                r1: if sign1 == 0.0 { f64::INFINITY } else { sign1 * v },
                                r2: if sign2 == 0.0 { f64::INFINITY } else { sign2 * v },
                                ..l
                            };
                            write(&mut operations, new, glass.clone());
                        }
                    }
                }
            }
            // Thickness: the focal length is kept.
            if let Some(v) = row(ui, drafts, &format!("{base}/t"), "tc", "centre thickness", "mm", l.t) {
                if v > 0.0 {
                    let thicker = Singlet { t: v, ..l };
                    let new = thicker.design(shape, f, flipped).unwrap_or(thicker);
                    write(&mut operations, new, glass.clone());
                }
            }
            // The glass: a name (its index from the scene) or an index; with an index
            // typed, the focal length is kept.
            ui.label(RichText::new("glass").color(DARK.text_dim))
                .on_hover_text("a material name (bundled or imported, e.g. N-BK7) or a refractive index");
            let named = match &glass {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            if let Some(typed) = text(ui, drafts, &format!("{base}/glass"), &named, 90.0) {
                let typed = typed.trim().to_owned();
                match typed.parse::<f64>() {
                    Ok(index) if index > 1.0 => {
                        let other = Singlet { n: index, ..l };
                        let new = other.design(shape, f, flipped).unwrap_or(other);
                        write(&mut operations, new, json!(index));
                    }
                    Ok(_) => {}
                    Err(_) => operations.push(set("material", json!([typed]))),
                }
            }
            ui.label(
                RichText::new(match wavelength {
                    Some(w) => format!("n {n:.4} at {w} µm"),
                    None => format!("n {n:.4}"),
                })
                .color(DARK.text_dim)
                .size(11.0),
            );
            ui.end_row();
            ui.label(RichText::new("fb").color(DARK.text_dim)).on_hover_text("back focal length, from the last surface");
            ui.label(shown("back_focal_length_mm").map_or("—".into(), |v| format!("{v:.3}")));
            ui.label(RichText::new("mm").color(DARK.text_dim).size(11.0));
            ui.end_row();
        });
        ui.label(
            RichText::new(match (shape, flipped) {
                (Shape::PlanoConvex | Shape::PlanoConcave, false) => "curved side first",
                (Shape::PlanoConvex | Shape::PlanoConcave, true) => "flat side first",
                (Shape::Meniscus, false) => "convex side first",
                (Shape::Meniscus, true) => "concave side first",
                _ => "",
            })
            .color(DARK.text_dim)
            .size(11.0),
        );
    } else {
        ui.label(
            RichText::new("The glass's index is not known at this wavelength: type an index, or import the material")
                .color(DARK.warning)
                .size(11.0),
        );
    }
    if let Some(note) = drafts.texts.get(&format!("{base}/note")).cloned() {
        ui.label(RichText::new(note).color(DARK.warning).size(11.0));
    }

    // Its size: no larger than its curvature allows.
    let largest = now.map(|l| l.max_diameter());
    egui::Grid::new(format!("lens-size-{base}"))
        .num_columns(3)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            if let Some(v) = row(
                ui,
                drafts,
                &format!("{base}/diameter"),
                "diameter",
                "mechanical diameter",
                "mm",
                diameter,
            ) {
                let v = match largest {
                    Some(m) if v > m && m > 0.0 => {
                        drafts.texts.insert(
                            format!("{base}/note"),
                            format!(
                                "Ø kept to {m} mm: beyond it the edge would be thinner than {} mm",
                                crate::lens::MIN_EDGE
                            ),
                        );
                        m
                    }
                    _ => v,
                };
                if v > 0.0 {
                    operations.push(set("diameter", tidy(v)));
                    if clear > v {
                        operations.push(set("clear_aperture", tidy(v)));
                    }
                }
            }
            if let Some(v) = row(
                ui,
                drafts,
                &format!("{base}/clear"),
                "clear aperture",
                "the part light may pass, at most the diameter",
                "mm",
                clear,
            ) {
                if v > 0.0 {
                    operations.push(set("clear_aperture", tidy(v.min(diameter))));
                }
            }
        });
    if let Some(m) = largest.filter(|m| *m < 1000.0) {
        ui.label(
            RichText::new(format!(
                "at most Ø{m} mm for an edge of {} mm",
                crate::lens::MIN_EDGE
            ))
            .color(DARK.text_dim)
            .size(11.0),
        );
    }
    if !operations.is_empty() {
        drafts.texts.remove(&format!("{base}/note"));
    }
    ui.add_space(4.0);
    operations
}

/// A lens's prescription radius by radius (R1…Rn, flat or not), with each glass's centre
/// thickness and material between them; a cemented glass added or taken away.
fn prescription_editor(
    ui: &mut Ui,
    drafts: &mut Drafts,
    base: &str,
    set: &dyn Fn(&str, Value) -> Value,
    surfaces: &[Value],
    thickness: &[Value],
    material: &[Value],
) -> Vec<Value> {
    let mut operations = Vec::new();
    egui::Grid::new(format!("prescription-{base}"))
        .num_columns(4)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for (k, surface) in surfaces.iter().enumerate() {
                let r = radius_of(&surface["radius"]);
                ui.label(RichText::new(format!("R{}", k + 1)).color(DARK.text_dim))
                    .on_hover_text("radius of curvature, positive when its centre lies further along the light");
                let mut flat = !r.is_finite();
                if flat {
                    ui.label("flat");
                } else if let Some(v) = number(ui, drafts, &format!("{base}/R/{k}"), r) {
                    if v != 0.0 {
                        let mut new = surfaces.to_vec();
                        new[k]["radius"] = radius_value(v);
                        operations.push(set("surfaces", json!(new)));
                    }
                }
                if ui.checkbox(&mut flat, "flat").changed() {
                    let mut new = surfaces.to_vec();
                    new[k]["radius"] = if flat { json!("inf") } else { json!(100.0) };
                    operations.push(set("surfaces", json!(new)));
                }
                ui.label(RichText::new("mm").color(DARK.text_dim).size(11.0));
                ui.end_row();
                if k + 1 < surfaces.len() {
                    ui.label(RichText::new(format!("tc{}", k + 1)).color(DARK.text_dim))
                        .on_hover_text("centre thickness of this glass (or air gap)");
                    let tk = thickness.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                    if let Some(v) = number(ui, drafts, &format!("{base}/t/{k}"), tk) {
                        if v > 0.0 {
                            let mut new = thickness.to_vec();
                            new.resize(surfaces.len() - 1, json!(1.0));
                            new[k] = tidy(v);
                            operations.push(set("thickness", json!(new)));
                        }
                    }
                    let glass = material.get(k).cloned().unwrap_or(json!(1.5));
                    let shown = match &glass {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    if let Some(typed) = text(ui, drafts, &format!("{base}/glass/{k}"), &shown, 90.0) {
                        let value = typed
                            .trim()
                            .parse::<f64>()
                            .map_or_else(|_| json!(typed.trim()), |n| json!(n));
                        let mut new = material.to_vec();
                        new.resize(surfaces.len() - 1, json!(1.5));
                        new[k] = value;
                        operations.push(set("material", json!(new)));
                    }
                    ui.label(RichText::new("mm, glass").color(DARK.text_dim).size(11.0))
                        .on_hover_text("a material name or an index; 1 (or standard-air) for an air gap");
                    ui.end_row();
                }
            }
        });
    ui.horizontal(|ui| {
        if ui
            .small_button("Add a cemented glass")
            .on_hover_text("One more surface after the last: a singlet becomes a cemented doublet (three radii)")
            .clicked()
        {
            let mut s = surfaces.to_vec();
            s.push(json!({ "radius": "inf" }));
            let mut t = thickness.to_vec();
            t.push(json!(2.0));
            let mut m = material.to_vec();
            m.push(material.last().cloned().unwrap_or(json!(1.5)));
            operations.push(set("surfaces", json!(s)));
            operations.push(set("thickness", json!(t)));
            operations.push(set("material", json!(m)));
        }
        if surfaces.len() > 2
            && ui
                .small_button("Remove the last glass")
                .on_hover_text("Take away the last surface and the glass before it")
                .clicked()
        {
            let keep = surfaces.len() - 1;
            operations.push(set("surfaces", json!(surfaces[..keep])));
            operations.push(set("thickness", json!(thickness[..keep - 1])));
            operations.push(set("material", json!(material[..keep.saturating_sub(1).min(material.len())])));
        }
    });
    operations
}

/// Whether two values are the same, numbers by value (`75.0` and `75` are).
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        _ => a == b,
    }
}

/// One item of a group as the Move tool moves it: its table, entry, index in the table
/// and where the scene placed it (mm).
pub type Member = (String, Value, usize, V3);

/// The edits a Move of a whole group makes on a table layout, its steps in order: every
/// member slides alike; turns turn each member's place about `anchor` (where the item
/// the handles sit on was) and its axis. Holes and heights follow from the places (holes
/// may come out as fractions).
pub fn moved_group(
    members: &[Member],
    pitch: f64,
    anchor: V3,
    steps: &[(crate::moving::Handle, f64)],
) -> Vec<Value> {
    use crate::moving::{Handle, placed, turned};
    let mut operations = Vec::new();
    for (table, entry, index, origin) in members {
        if !entry["hole"].is_array() {
            continue;
        }
        // Where its frame goes: the anchor's path, with the member's offset turned along.
        let start = [
            origin[0] - anchor[0],
            origin[1] - anchor[1],
            origin[2] - anchor[2],
        ];
        let mut offset = start;
        let mut at = anchor;
        let mut axis = [
            entry["axis"][0].as_f64().unwrap_or(1.0),
            entry["axis"][1].as_f64().unwrap_or(0.0),
            entry["axis"][2].as_f64().unwrap_or(0.0),
        ];
        for &(handle, amount) in steps {
            match handle {
                Handle::Slide(_) => at = placed(at, at, &[(handle, amount)]),
                Handle::Turn(k) => {
                    offset = turned(offset, [0.0; 3], k, amount);
                    axis = turned(axis, [0.0; 3], k, amount);
                }
            }
        }
        let now = [at[0] + offset[0], at[1] + offset[1], at[2] + offset[2]];
        let hole = json!([tidy(now[0] / pitch), tidy(now[1] / pitch)]);
        let mut set = |key: &str, value: Value| {
            if !same(&entry[key], &value) {
                let mut op = selector(table, entry, *index);
                op["op"] = json!("set");
                op["key"] = json!(key);
                op["value"] = value;
                operations.push(op);
            }
        };
        set("hole", hole);
        set("height", tidy(now[2]));
        if entry["axis"].is_array() {
            set("axis", json!(axis.map(tidy)));
        }
    }
    operations
}

/// The tables whose entries carry degrees of freedom.
const WITH_DOFS: &[&str] = &["element", "source", "detector"];

/// The degrees of freedom of an entry: each with its value and its range (both
/// editable), removed by ×; and buttons to add one. On a table layout and in a sidecar
/// they move along and about the scene's X, Y and Z (the board's axes), or along the
/// item's own axis; on a planar layout along its axis and across it. Slides start with
/// a range of two holes either way on a table, 10 mm elsewhere; tilts 5°.
fn dofs(
    ui: &mut Ui,
    drafts: &mut Drafts,
    table: &str,
    entry: &Value,
    base: &str,
    context: &Context,
) -> Vec<Value> {
    let mut operations = Vec::new();
    let id = entry["id"].as_str().unwrap_or_default();
    ui.label(RichText::new("Degrees of freedom").strong())
        .on_hover_text("What the optimiser may move, and how far");
    let list = entry["dof"].as_array().cloned().unwrap_or_default();
    for dof in &list {
        let name = dof["name"].as_str().unwrap_or("?");
        let tilt = dof["kind"] == "tilt";
        let unit = if tilt { "°" } else { "mm" };
        let current = dof["value"].as_f64().unwrap_or(0.0);
        let range = (dof["range"][0].as_f64(), dof["range"][1].as_f64());
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).color(DARK.text_dim));
            let path = format!("{base}/dof/{name}");
            let mut value = *drafts.numbers.get(&path).unwrap_or(&current);
            let response = ui.add(
                egui::DragValue::new(&mut value)
                    .range(range.0.unwrap_or(f64::NEG_INFINITY)..=range.1.unwrap_or(f64::INFINITY))
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
            if ui
                .small_button("×")
                .on_hover_text("Remove this degree of freedom")
                .clicked()
            {
                operations.push(json!({ "op": "remove_dof", "dof": name }));
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let axis = &dof["axis"];
            let along = match axis {
                Value::String(a) => a.clone(),
                Value::Array(v) => format!(
                    "[{}]",
                    v.iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                _ => "?".into(),
            };
            let frame = if dof["frame"] == "scene" {
                "scene"
            } else {
                "its own"
            };
            ui.label(
                RichText::new(format!(
                    "{} {along} ({frame} axes), range",
                    if tilt { "tilt about" } else { "slide along" }
                ))
                .color(DARK.text_dim)
                .size(11.0),
            );
            for (k, end) in [range.0, range.1].into_iter().enumerate() {
                let path = format!("{base}/dof/{name}/range/{k}");
                let shown = end.unwrap_or(if k == 0 { -10.0 } else { 10.0 });
                if let Some(v) = number(ui, drafts, &path, shown) {
                    let mut new = [range.0.unwrap_or(-10.0), range.1.unwrap_or(10.0)];
                    new[k] = v;
                    if new[0] <= new[1] {
                        operations.push(json!({
                            "op": "set", "dof": name, "key": "range",
                            "value": [tidy(new[0]), tidy(new[1])],
                        }));
                    }
                }
            }
            ui.label(RichText::new(unit).color(DARK.text_dim).size(11.0));
        });
    }
    // Buttons to add one.
    let scene_axes = context.sidecar || context.pitch.is_some();
    let slide = context
        .pitch
        .map_or(10.0, |p| tidy(2.0 * p).as_f64().unwrap_or(50.0));
    let options: Vec<(&str, &str, Value, &str, f64, &str)> = if scene_axes {
        vec![
            (
                "slide X",
                "slide_X",
                json!([1, 0, 0]),
                "scene",
                slide,
                "along the board's X",
            ),
            (
                "slide Y",
                "slide_Y",
                json!([0, 1, 0]),
                "scene",
                slide,
                "along the board's Y",
            ),
            (
                "slide Z",
                "slide_Z",
                json!([0, 0, 1]),
                "scene",
                slide,
                "up and down",
            ),
            (
                "along its axis",
                "along",
                json!([0, 0, 1]),
                "local",
                slide,
                "along its own optical axis",
            ),
            (
                "tilt about X",
                "tilt_X",
                json!([1, 0, 0]),
                "scene",
                5.0,
                "about the board's X",
            ),
            (
                "tilt about Y",
                "tilt_Y",
                json!([0, 1, 0]),
                "scene",
                5.0,
                "about the board's Y",
            ),
            (
                "tilt about Z",
                "tilt_Z",
                json!([0, 0, 1]),
                "scene",
                5.0,
                "about the vertical",
            ),
        ]
    } else {
        vec![
            (
                "along its axis",
                "along",
                json!([0, 0, 1]),
                "local",
                slide,
                "along the path",
            ),
            (
                "across x",
                "slide_x",
                json!([1, 0, 0]),
                "local",
                5.0,
                "across the path, sideways",
            ),
            (
                "across y",
                "slide_y",
                json!([0, 1, 0]),
                "local",
                5.0,
                "across the path, up",
            ),
            (
                "tilt x",
                "tilt_x",
                json!([1, 0, 0]),
                "local",
                5.0,
                "about its own x",
            ),
            (
                "tilt y",
                "tilt_y",
                json!([0, 1, 0]),
                "local",
                5.0,
                "about its own y",
            ),
        ]
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Add").color(DARK.text_dim));
        for (label, short, axis, frame, range, help) in options {
            let name = format!("{id}_{short}");
            if list.iter().any(|d| d["name"] == name.as_str()) {
                continue;
            }
            let kind = if short.starts_with("tilt") { "tilt" } else { "slide" };
            let unit = if kind == "tilt" { "°" } else { " mm" };
            if ui
                .small_button(label)
                .on_hover_text(format!("A {kind} {help}, ±{range}{unit} to start (edit its range after)"))
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
    /// An iris: a `plate` proxy with a round hole.
    Iris,
    /// A tube proxy.
    Tube,
}

/// The height new items of a table layout get: the first source's, else 50 mm.
pub fn new_height(design: &Value) -> f64 {
    design["source"][0]["height"].as_f64().unwrap_or(50.0)
}

/// An `add` operation for a new entry of a layout, placed after what is there: on a
/// planar layout 50 mm further along the path, on a table layout two holes further
/// along x at the source's height.
pub fn add(design: &Value, new: New) -> Option<Value> {
    add_at(design, new, None, None)
}

/// As [`add`], on a table layout on the hole given (column, row) when there is one; a
/// lens from the parts library when `library` names it.
pub fn add_at(
    design: &Value,
    new: New,
    hole: Option<(i64, i64)>,
    library: Option<&str>,
) -> Option<Value> {
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
        if new == New::Tube {
            // A tube runs from `from` to `to` along the path.
            vec![("from".into(), json!(at)), ("to".into(), json!(at + 50.0))]
        } else {
            vec![("at".into(), json!(at))]
        }
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
        New::Iris => (
            "proxy",
            "I",
            [
                vec![
                    ("kind".into(), json!("plate")),
                    ("hole_diameter".into(), json!(5.0)),
                    ("size".into(), json!(30.0)),
                ],
                place,
                axis([1, 0, 0]),
            ]
            .concat(),
        ),
        New::Tube => (
            "proxy",
            "T",
            [
                vec![
                    ("kind".into(), json!("tube")),
                    ("inner_diameter".into(), json!(25.4)),
                ],
                place,
                axis([1, 0, 0]),
                if planar {
                    Vec::new()
                } else {
                    vec![("length".into(), json!(50.0))]
                },
            ]
            .concat(),
        ),
    };
    // A library lens takes its kind, prescription, size and coating from its part.
    let fields: Vec<(String, Value)> = match (new, library) {
        (New::Lens, Some(key)) => [
            vec![("library".to_owned(), json!(key))],
            fields
                .into_iter()
                .filter(|(k, _)| matches!(k.as_str(), "at" | "hole" | "height" | "axis"))
                .collect(),
        ]
        .concat(),
        _ => fields,
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
    fn irises_and_tubes_are_added_and_turned_into_each_other() {
        let pairs = |op: &Value| op["entry"].as_array().expect("pairs").clone();
        // On a table: an iris is a plate on the hole clicked, facing along x.
        let table = json!({ "layout": "table", "source": [{ "id": "S1", "hole": [0, 0], "height": 75.0 }] });
        let op = add_at(&table, New::Iris, Some((3, 0)), None).expect("an op");
        assert_eq!(op["table"], "proxy");
        let entry = pairs(&op);
        for pair in [
            json!(["id", "I1"]),
            json!(["kind", "plate"]),
            json!(["hole_diameter", 5.0]),
            json!(["hole", [3, 0]]),
            json!(["height", 75.0]),
            json!(["axis", [1, 0, 0]]),
        ] {
            assert!(entry.contains(&pair), "{pair} in {entry:?}");
        }
        // A tube on a table has a length; on a planar layout it runs from, to.
        let op = add_at(&table, New::Tube, Some((1, 0)), None).expect("an op");
        assert!(pairs(&op).contains(&json!(["length", 50.0])));
        let planar = json!({ "layout": "planar", "source": [{ "id": "S1", "at": 0.0 }],
                             "detector": [{ "id": "D1", "at": 100.0 }] });
        let entry = pairs(&add(&planar, New::Tube).expect("an op"));
        assert!(entry.contains(&json!(["from", 150.0])) && entry.contains(&json!(["to", 200.0])));
        assert!(!entry.iter().any(|p| p[0] == "axis" || p[0] == "at"));
        // A planar tube made a plate: its bore and run replaced by a hole, size and `at`.
        let target = json!({ "table": "proxy", "id": "T1" });
        let tube = json!({ "id": "T1", "kind": "tube", "inner_diameter": 25.4, "from": 150.0, "to": 200.0 });
        let ops = proxy_kind(&target, &tube, "plate");
        let said = |op: &str, key: &str| ops.iter().any(|o| o["op"] == op && o["key"] == key);
        assert!(said("set", "kind") && said("unset", "inner_diameter") && said("unset", "from"));
        assert!(said("unset", "to") && said("set", "hole_diameter") && said("set", "size"));
        assert!(ops.contains(
            &json!({ "table": "proxy", "id": "T1", "op": "set", "key": "at", "value": 150.0 })
        ));
        // An iris that faces the light: along it, whatever its length.
        assert_eq!(
            facing([0.0, 0.0, 75.0], [0.0, 75.0, 75.0]),
            Some([0.0, 1.0, 0.0])
        );
        assert_eq!(facing([0.0; 3], [-30.0, 30.0, 0.0]), Some([-1.0, 1.0, 0.0]));
        assert_eq!(facing([1.0; 3], [1.0; 3]), None);
    }

    #[test]
    fn mounts_come_from_the_file_then_the_library() {
        let design = json!({ "mount": [{ "id": "Example/XY1", "source": "own",
            "adjusters": [{ "name": "x", "kind": "slide", "axis": "x", "range": [-1, 1] }] }] });
        let library = [
            json!({ "key": "example/XY1", "type": "mount", "source": "library", "adjusters": [] }),
            json!({ "key": "example/KM1", "type": "mount", "source": "kinematic",
                    "adjusters": [{ "name": "tip", "kind": "tilt", "axis": [1.0, 0.0, 0.0], "range": [-4.0, 4.0] },
                                  { "name": "coarse", "kind": "tilt", "axis": [0.0, 0.0, -1.0], "range": null },
                                  { "name": "z", "kind": "slide", "axis": [0.0, 0.0, 1.0], "range": [-6.35, 6.35], "travel": 12.7 }] }),
        ];
        let mounts = mounts_of(&design, &library);
        // The file's XY1 stands in for the library's (keys ignore case).
        let names: Vec<&str> = mounts.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Example/XY1", "example/KM1"]);
        assert_eq!(mounts[0].source, "own");
        assert!(!mounts[0].library && mounts[1].library);
        let words: Vec<String> = mounts[1].adjusters.iter().map(adjuster_text).collect();
        assert_eq!(
            words,
            [
                "tilt about x, -4 to 4°",
                "tilt about −z, unlimited",
                "slide along z, 12.7 mm of travel"
            ]
        );
        assert_eq!(
            adjuster_text(&mounts[0].adjusters[0]),
            "slide along x, -1 to 1 mm"
        );
    }

    #[test]
    fn a_mirror_is_aimed_by_the_law_of_reflection() {
        // A fold from +y to −x (a hand-built layout's first mirror, 25 mm pitch).
        let axis = aim(
            [175.0, -25.0, 50.0],
            [175.0, 300.0, 50.0],
            [-50.0, 300.0, 50.0],
        );
        assert_eq!(axis, Some([-1.0, -1.0, 0.0]));
        // From −y up to +z: a periscope's lower mirror.
        let axis = aim(
            [-50.0, 125.0, 50.0],
            [-50.0, 25.0, 50.0],
            [-50.0, 25.0, 200.0],
        );
        assert_eq!(axis, Some([0.0, 1.0, 1.0]));
        // A 60° turn in the plane: the normal halves it, so the light leaves at the same
        // angle to it as it came in.
        let to = [100.0 + 50.0 * 0.5, 50.0 * 3f64.sqrt() / 2.0, 0.0];
        let n = aim([0.0, 0.0, 0.0], [100.0, 0.0, 0.0], to).expect("a normal");
        let length = n.iter().map(|x| x * x).sum::<f64>().sqrt();
        let cos_in = -n[0] / length;
        let cos_out = (n[0] * 0.5 + n[1] * 3f64.sqrt() / 2.0) / length;
        assert!((cos_in - cos_out).abs() < 1e-9, "{cos_in} {cos_out}");
        // Straight back: the mirror faces the light.
        assert_eq!(
            aim([0.0; 3], [100.0, 0.0, 0.0], [0.0; 3]),
            Some([-1.0, 0.0, 0.0])
        );
        // Straight on, and a place at the mirror's own: refused.
        assert_eq!(aim([0.0; 3], [100.0, 0.0, 0.0], [200.0, 0.0, 0.0]), None);
        assert_eq!(aim([0.0; 3], [100.0, 0.0, 0.0], [100.0, 0.0, 0.0]), None);
    }

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
        let op = add_at(&table, New::Mirror, Some((3, -2)), None).expect("an op");
        assert!(
            op["entry"]
                .as_array()
                .expect("pairs")
                .contains(&json!(["hole", [3, -2]]))
        );
        // A library lens: its key, its place, nothing of its prescription.
        let op = add_at(&table, New::Lens, Some((5, 0)), Some("example/AC-30")).expect("an op");
        let entry = op["entry"].as_array().expect("pairs");
        assert!(entry.contains(&json!(["library", "example/AC-30"])));
        assert!(entry.contains(&json!(["hole", [5, 0]])));
        assert!(!entry.iter().any(|p| p[0] == "surfaces" || p[0] == "kind"));
    }

    #[test]
    fn moves_write_holes_heights_and_axes() {
        use crate::moving::Handle;
        let entry = json!({ "id": "M1", "hole": [4, 0], "height": 75.0, "axis": [-1, 1, 0] });
        let one = [("element".to_owned(), entry, 0, [100.0, 0.0, 75.0])];
        let ops = |steps: &[(Handle, f64)]| moved_group(&one, 25.0, one[0].3, steps);
        let value = |ops: &[Value], key: &str| {
            ops.iter()
                .find(|o| o["key"] == key)
                .map(|o| o["value"].clone())
        };
        let o = ops(&[(Handle::Slide(0), 50.0)]);
        assert_eq!(value(&o, "hole"), Some(json!([6, 0])));
        assert_eq!(o.len(), 1, "only the hole changed");
        assert_eq!(o[0]["id"], "M1");
        let o = ops(&[(Handle::Slide(1), -12.5)]);
        assert_eq!(value(&o, "hole"), Some(json!([4, -0.5])), "fractions kept");
        let o = ops(&[(Handle::Slide(2), 25.0)]);
        assert_eq!(value(&o, "height"), Some(json!(100)));
        // A mirror facing (−1, 1, 0) turned 90° about z faces (−1, −1, 0).
        let o = ops(&[(Handle::Turn(2), 90.0)]);
        assert_eq!(value(&o, "axis"), Some(json!([-1, -1, 0])));
        // A whole move, its steps in order: two holes along x, back one, a quarter turn.
        let o = ops(&[
            (Handle::Slide(0), 50.0),
            (Handle::Slide(0), -25.0),
            (Handle::Turn(2), 90.0),
        ]);
        let keys: Vec<&str> = o.iter().filter_map(|x| x["key"].as_str()).collect();
        assert_eq!(keys, ["hole", "axis"], "the height did not change");
        assert_eq!(value(&o, "hole"), Some(json!([5, 0])));
        // An entry not on the table is left alone.
        let planar = [(
            "element".to_owned(),
            json!({ "id": "L", "at": 5 }),
            0,
            [5.0, 0.0, 0.0],
        )];
        assert!(moved_group(&planar, 25.0, planar[0].3, &[(Handle::Slide(0), 1.0)]).is_empty());
    }

    #[test]
    fn a_group_moves_and_turns_together() {
        use crate::moving::Handle;
        let m = |id: &str, hole: [i64; 2]| -> Member {
            (
                "element".into(),
                json!({ "id": id, "hole": hole, "height": 75.0, "axis": [1, 0, 0] }),
                0,
                [hole[0] as f64 * 25.0, hole[1] as f64 * 25.0, 75.0],
            )
        };
        let members = [m("L1", [2, 0]), m("L2", [4, 0])];
        // Two holes along y and 25 mm up: both alike.
        let ops = moved_group(
            &members,
            25.0,
            members[0].3,
            &[(Handle::Slide(1), 50.0), (Handle::Slide(2), 25.0)],
        );
        let value = |id: &str, key: &str| {
            ops.iter()
                .find(|o| o["id"] == id && o["key"] == key)
                .map(|o| o["value"].clone())
        };
        assert_eq!(value("L1", "hole"), Some(json!([2, 2])));
        assert_eq!(value("L2", "hole"), Some(json!([4, 2])));
        assert_eq!(value("L2", "height"), Some(json!(100)));
        assert_eq!(value("L1", "axis"), None, "not turned");
        // A quarter turn about z at L1: L2 two holes along +y from it, both facing +y.
        let ops = moved_group(&members, 25.0, members[0].3, &[(Handle::Turn(2), 90.0)]);
        let value = |id: &str, key: &str| {
            ops.iter()
                .find(|o| o["id"] == id && o["key"] == key)
                .map(|o| o["value"].clone())
        };
        assert_eq!(value("L1", "hole"), None, "the anchor stays");
        assert_eq!(value("L2", "hole"), Some(json!([2, 2])));
        assert_eq!(value("L2", "axis"), Some(json!([0, 1, 0])));
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
