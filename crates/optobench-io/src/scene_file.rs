//! The `.optobench` layout file: a standalone scene in TOML.
//!
//! ```toml
//! format = "optobench/1"
//! kind = "layout"
//! layout = "planar"                 # or "table"
//! surrounding = "standard-air"      # optional: a material name or an index; default 1.0
//!
//! [planar]                          # optional, planar layouts
//! height = 0.0                      # height of the axis above the table (mm)
//! direction = [1, 0, 0]             # starting direction of the axis
//!
//! [table]                           # required for table layouts
//! pitch = 25.0                      # distance between holes (mm)
//!
//! [[source]]
//! id = "laser"
//! kind = "collimated"               # or "point" with half_angle_deg, or "gaussian",
//!                                   # "fiber", "led" (below)
//! wavelength_um = 0.633             # optional when spectrum is given
//! spectrum = [[0.488, 1.0], [0.561, 3.0]]  # optional lines [µm, weight]: power shared by weight
//! power_w = 0.001
//! diameter = 2.0
//! rings = 6                         # optional sampling, default 6
//! offset = [0.0, 2.0]               # optional lateral offset in the source frame (mm)
//! at = 0.0                          # planar; table: hole = [0, 0], height = 75, axis = [1, 0, 0]
//!
//! # Other kinds (angle_rings: optional angular sampling, default 3; led 4):
//! #   gaussian: waist_diameter (1/e² diameter at the source, mm), m_squared (≥ 1, default 1)
//! #   fiber:    core_diameter (mm), na (< 1): uniform over the core, within the NA
//! #   led:      size (square side, mm): Lambertian over the hemisphere
//!
//! [[element]]
//! id = "L1"
//! kind = "lens"
//! at = 100.0
//! surfaces = [ { radius = 51.5 }, { radius = "inf" } ]   # optional conic per surface
//! thickness = [3.6]
//! material = ["fused-silica"]       # names (bundled or imported) or indices
//! diameter = 25.4
//! clear_aperture = 22.9
//! coating = "uncoated"              # or "ideal"; default "uncoated"
//!
//! [[element]]
//! id = "L2"
//! at = 150.0
//! library = "thorlabs/LA1951-ML"    # prescription, glasses, diameter, clear aperture and
//!                                   # coating from the parts library; kind follows the part
//! reversed = true                   # optional: the part turned around
//!                                   # (coating and clear_aperture may be given here too)
//!
//! [[element]]
//! id = "M1"
//! kind = "mirror"
//! at = 180.0
//! fold = 90.0                       # planar fold mirrors: degrees, counter-clockwise from above
//! diameter = 25.4
//! clear_aperture = 22.9
//!
//! [[proxy]]
//! id = "tube"
//! kind = "tube"                     # inner_diameter; planar: from, to; table: hole, height, axis, length
//! inner_diameter = 25.4
//! from = 60.0
//! to = 140.0
//!
//! [[proxy]]
//! id = "iris"
//! kind = "plate"                    # hole_diameter, size, position
//! hole_diameter = 5.0
//! size = 50.0
//! at = 50.0
//!
//! [trace]                           # optional: ray splitting at partly reflecting faces
//! split = true                      # default true
//! split_cutoff = 1e-6               # fraction of the total source power; default 1e-6
//!
//! [[detector]]
//! id = "screen"
//! at = 300.0
//! size = [10, 10]
//! pixels = [256, 256]
//! ```
//!
//! Parsing is strict (docs/design.md, Format tag and versions): the first two lines are
//! the format tag and the kind, a newer format is refused, and an unknown key is an error
//! naming the closest valid key. Lengths are in mm, angles in degrees.

use optobench_library::{Library, Outline, Part};
use optobench_materials::Medium;
use optobench_math::{Frame, Vec3, radians_from_degrees};
use optobench_scene::layout::{PlanarLayout, TableLayout};
use optobench_scene::{
    Detector, Element, ElementKind, LensSurface, Proxy, ProxyKind, Scene, Source, SourceKind,
};
use optobench_trace::Coating;
use toml::{Table, Value};

use crate::{FORMAT_PREFIX, FORMAT_VERSION};

/// Why a layout file could not be read. The message names the item and key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileError(pub String);

impl core::fmt::Display for FileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FileError {}

pub(crate) fn error(message: impl Into<String>) -> FileError {
    FileError(message.into())
}

/// Edit distance, for suggesting the closest valid key.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous + usize::from(ca != *cb);
            previous = row[j + 1];
            row[j + 1] = substitution.min(row[j] + 1).min(row[j + 1] + 1);
        }
    }
    row[b.len()]
}

/// Keys that belong to later milestones, with the milestone.
const LATER: &[(&str, &str)] = &[("dof", "degrees of freedom arrive in M6")];

/// Rejects any key of `table` not in `allowed`, naming the closest valid one.
pub(crate) fn check_keys(table: &Table, allowed: &[&str], context: &str) -> Result<(), FileError> {
    for key in table.keys() {
        if allowed.contains(&key.as_str()) {
            continue;
        }
        if let Some((_, why)) = LATER.iter().find(|(k, _)| k == key) {
            return Err(error(format!(
                "{context}: `{key}` is not available yet: {why}"
            )));
        }
        let closest = allowed
            .iter()
            .min_by_key(|candidate| distance(key, candidate))
            .copied()
            .unwrap_or("");
        return Err(error(format!(
            "{context}: unknown key `{key}`; did you mean `{closest}`? Valid keys: {}",
            allowed.join(", ")
        )));
    }
    Ok(())
}

/// Looks up materials by name: bundled materials first, then imported ones.
pub type MaterialLookup<'a> = dyn Fn(&str) -> Result<Medium, String> + 'a;

/// One `[[...]]` item, with helpers that name the item in errors.
pub(crate) struct Item<'a> {
    pub(crate) table: &'a Table,
    pub(crate) context: String,
}

impl<'a> Item<'a> {
    pub(crate) fn get(&self, key: &str) -> Result<&'a Value, FileError> {
        self.table
            .get(key)
            .ok_or_else(|| error(format!("{}: missing `{key}`", self.context)))
    }

    pub(crate) fn number_of(&self, value: &Value, key: &str) -> Result<f64, FileError> {
        match value {
            Value::Float(f) => Ok(*f),
            Value::Integer(i) => Ok(*i as f64),
            _ => Err(error(format!("{}: `{key}` must be a number", self.context))),
        }
    }

    pub(crate) fn number(&self, key: &str) -> Result<f64, FileError> {
        self.number_of(self.get(key)?, key)
    }

    pub(crate) fn positive(&self, key: &str) -> Result<f64, FileError> {
        let value = self.number(key)?;
        if value > 0.0 && value.is_finite() {
            Ok(value)
        } else {
            Err(error(format!("{}: `{key}` must be positive", self.context)))
        }
    }

    pub(crate) fn optional_number(&self, key: &str, default: f64) -> Result<f64, FileError> {
        match self.table.get(key) {
            None => Ok(default),
            Some(v) => self.number_of(v, key),
        }
    }

    pub(crate) fn text(&self, key: &str) -> Result<&'a str, FileError> {
        self.get(key)?
            .as_str()
            .ok_or_else(|| error(format!("{}: `{key}` must be text", self.context)))
    }

    pub(crate) fn numbers(&self, key: &str, count: Option<usize>) -> Result<Vec<f64>, FileError> {
        let array = self
            .get(key)?
            .as_array()
            .ok_or_else(|| error(format!("{}: `{key}` must be a list", self.context)))?;
        let values = array
            .iter()
            .map(|v| self.number_of(v, key))
            .collect::<Result<Vec<f64>, _>>()?;
        if let Some(n) = count {
            if values.len() != n {
                return Err(error(format!(
                    "{}: `{key}` must have {n} values",
                    self.context
                )));
            }
        }
        Ok(values)
    }

    pub(crate) fn vector(&self, key: &str) -> Result<Vec3, FileError> {
        let v = self.numbers(key, Some(3))?;
        Ok(Vec3::new(v[0], v[1], v[2]))
    }

    pub(crate) fn pair(&self, key: &str) -> Result<(f64, f64), FileError> {
        let v = self.numbers(key, Some(2))?;
        Ok((v[0], v[1]))
    }

    pub(crate) fn count_pair(&self, key: &str) -> Result<(usize, usize), FileError> {
        let (a, b) = self.pair(key)?;
        let whole = |v: f64| (v >= 1.0 && v.fract() == 0.0).then_some(v as usize);
        match (whole(a), whole(b)) {
            (Some(a), Some(b)) => Ok((a, b)),
            _ => Err(error(format!(
                "{}: `{key}` must be two whole numbers ≥ 1",
                self.context
            ))),
        }
    }

    pub(crate) fn radius(&self, value: &Value) -> Result<f64, FileError> {
        match value {
            Value::String(s) if s == "inf" || s == "-inf" => Ok(f64::INFINITY),
            other => self.number_of(other, "radius"),
        }
    }
}

/// Where items go: the layout and how an item's position is read.
enum Placement {
    Planar(PlanarLayout),
    Table(TableLayout),
}

/// Position keys, by layout.
const PLANAR_POSITION: &[&str] = &["at"];
const TABLE_POSITION: &[&str] = &["hole", "height", "axis"];

impl Placement {
    fn keys(&self) -> &'static [&'static str] {
        match self {
            Placement::Planar(_) => PLANAR_POSITION,
            Placement::Table(_) => TABLE_POSITION,
        }
    }

    fn frame(&self, item: &Item) -> Result<Frame, FileError> {
        let scene = |e: optobench_scene::SceneError| error(format!("{}: {e}", item.context));
        match self {
            Placement::Planar(layout) => layout.frame(item.number("at")?).map_err(scene),
            Placement::Table(layout) => layout
                .frame(
                    item.pair("hole")?,
                    item.number("height")?,
                    item.vector("axis")?,
                )
                .map_err(scene),
        }
    }
}

pub(crate) fn with(base: &[&'static str], extra: &[&'static str]) -> Vec<&'static str> {
    base.iter().chain(extra).copied().collect()
}

pub(crate) fn items<'a>(root: &'a Table, key: &str) -> Result<Vec<Item<'a>>, FileError> {
    let Some(value) = root.get(key) else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| error(format!("`{key}` must be a list of [[{key}]] tables")))?;
    array
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let table = v
                .as_table()
                .ok_or_else(|| error(format!("[[{key}]] number {} must be a table", i + 1)))?;
            let id = table.get("id").and_then(Value::as_str).unwrap_or("?");
            Ok(Item {
                table,
                context: format!("[[{key}]] `{id}`"),
            })
        })
        .collect()
}

pub(crate) fn medium(
    value: &Value,
    lookup: &MaterialLookup,
    context: &str,
) -> Result<Medium, FileError> {
    match value {
        Value::Float(f) => Ok((*f).into()),
        Value::Integer(i) => Ok((*i as f64).into()),
        Value::String(name) => lookup(name).map_err(|e| error(format!("{context}: {e}"))),
        _ => Err(error(format!(
            "{context}: a material is a name or an index"
        ))),
    }
}

/// The `[trace]` table: `split = true | false` (default true) and `split_cutoff`, the
/// fraction of the total source power below which a reflected part is not followed
/// (default 1e-6). Returns the cutoff, or `None` with splitting off.
pub(crate) fn trace_options(root: &Table) -> Result<Option<f64>, FileError> {
    let Some(value) = root.get("trace") else {
        return Ok(Some(optobench_scene::DEFAULT_SPLIT_CUTOFF));
    };
    let table = value
        .as_table()
        .ok_or_else(|| error("[trace] must be a table"))?;
    check_keys(table, &["split", "split_cutoff"], "[trace]")?;
    let item = Item {
        table,
        context: "[trace]".into(),
    };
    let split = match table.get("split") {
        None => true,
        Some(v) => v
            .as_bool()
            .ok_or_else(|| error("[trace]: `split` must be true or false"))?,
    };
    if !split {
        return Ok(None);
    }
    let cutoff = item.optional_number("split_cutoff", optobench_scene::DEFAULT_SPLIT_CUTOFF)?;
    if !(cutoff > 0.0 && cutoff < 1.0) {
        return Err(error("[trace]: `split_cutoff` must be between 0 and 1"));
    }
    Ok(Some(cutoff))
}

/// A `[[source]]` entry, in either kind of file. `placement_keys` are the keys that
/// place it (checked by the caller); `frame` is where it was placed.
pub(crate) fn source(
    item: &Item,
    placement_keys: &[&'static str],
    frame: Frame,
) -> Result<Source, FileError> {
    let kind = item.text("kind")?;
    let (specific, default_rings): (&[&'static str], f64) = match kind {
        "collimated" => (&["diameter"], 6.0),
        "point" => (&["half_angle_deg"], 6.0),
        "gaussian" => (&["waist_diameter", "m_squared", "angle_rings"], 3.0),
        "fiber" => (&["core_diameter", "na", "angle_rings"], 3.0),
        "led" => (&["size", "angle_rings"], 3.0),
        other => {
            return Err(error(format!(
                "{}: source kind `{other}` is not known; use \"collimated\", \"point\", \"gaussian\", \"fiber\" or \"led\"",
                item.context
            )));
        }
    };
    let common = [
        "id",
        "kind",
        "wavelength_um",
        "power_w",
        "rings",
        "spectrum",
    ];
    let allowed: Vec<&str> = common
        .iter()
        .chain(specific)
        .chain(placement_keys)
        .copied()
        .collect();
    check_keys(item.table, &allowed, &item.context)?;
    let whole = |key: &str, default: f64| -> Result<usize, FileError> {
        let value = item.optional_number(key, default)?;
        if value < 0.0 || value.fract() != 0.0 || value > 200.0 {
            return Err(error(format!(
                "{}: `{key}` must be a whole number from 0 to 200",
                item.context
            )));
        }
        Ok(value as usize)
    };
    let source_kind = match kind {
        "collimated" => SourceKind::Collimated {
            diameter: item.positive("diameter")?,
        },
        "point" => SourceKind::Point {
            half_angle: radians_from_degrees(item.positive("half_angle_deg")?),
        },
        "gaussian" => SourceKind::Gaussian {
            waist_diameter: item.positive("waist_diameter")?,
            m_squared: {
                let m = item.optional_number("m_squared", 1.0)?;
                if m < 1.0 {
                    return Err(error(format!(
                        "{}: `m_squared` is at least 1",
                        item.context
                    )));
                }
                m
            },
            angle_rings: whole("angle_rings", 3.0)?,
        },
        "fiber" => SourceKind::Fiber {
            core_diameter: item.positive("core_diameter")?,
            numerical_aperture: {
                let na = item.positive("na")?;
                if na >= 1.0 {
                    return Err(error(format!("{}: `na` must be below 1", item.context)));
                }
                na
            },
            angle_rings: whole("angle_rings", 3.0)?,
        },
        _ => SourceKind::Led {
            size: item.positive("size")?,
            angle_rings: whole("angle_rings", 4.0)?,
        },
    };
    let spectrum: Vec<(f64, f64)> = match item.table.get("spectrum") {
        None => Vec::new(),
        Some(value) => {
            let lines = value.as_array().ok_or_else(|| {
                error(format!(
                    "{}: `spectrum` is a list of [wavelength_um, weight]",
                    item.context
                ))
            })?;
            lines
                .iter()
                .map(|line| {
                    let pair: Vec<f64> = line
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| {
                                    v.as_float().or_else(|| v.as_integer().map(|i| i as f64))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    match pair[..] {
                        [w, weight] if w > 0.0 && weight >= 0.0 => Ok((w, weight)),
                        _ => Err(error(format!(
                            "{}: each spectrum line is [wavelength_um, weight] with both positive",
                            item.context
                        ))),
                    }
                })
                .collect::<Result<_, _>>()?
        }
    };
    if !spectrum.is_empty() && spectrum.iter().map(|l| l.1).sum::<f64>() <= 0.0 {
        return Err(error(format!(
            "{}: the spectrum's weights add up to zero",
            item.context
        )));
    }
    let wavelength = match (item.table.get("wavelength_um"), spectrum.first()) {
        (Some(_), _) => item.positive("wavelength_um")?,
        (None, Some(&(w, _))) => w,
        (None, None) => {
            return Err(error(format!(
                "{}: missing `wavelength_um` (or a `spectrum`)",
                item.context
            )));
        }
    };
    Ok(Source {
        id: item.text("id")?.to_owned(),
        frame,
        kind: source_kind,
        wavelength,
        power: item.positive("power_w")?,
        rings: whole("rings", default_rings)?,
        spectrum,
    })
}

/// The part an element names with `library = "<vendor>/<part>"`, turned around with
/// `reversed = true`.
fn library_part(item: &Item, library: &Library) -> Result<Option<Part>, FileError> {
    let Some(value) = item.table.get("library") else {
        if item.table.contains_key("reversed") {
            return Err(error(format!(
                "{}: `reversed` turns a library part around; there is no `library`",
                item.context
            )));
        }
        return Ok(None);
    };
    let key = value.as_str().ok_or_else(|| {
        error(format!(
            "{}: `library` is \"<vendor>/<part number>\"",
            item.context
        ))
    })?;
    let part = library.get(key).ok_or_else(|| {
        error(format!(
            "{}: part `{key}` is not in the parts library (see the `parts` command)",
            item.context
        ))
    })?;
    let reversed = match item.table.get("reversed") {
        None => false,
        Some(Value::Boolean(b)) => *b,
        Some(_) => {
            return Err(error(format!(
                "{}: `reversed` is true or false",
                item.context
            )));
        }
    };
    Ok(Some(if reversed {
        part.reversed()
    } else {
        part.clone()
    }))
}

/// A round part's diameter; layouts place round parts only.
fn round_diameter(part: &Part, context: &str) -> Result<f64, FileError> {
    match part.outline {
        Outline::Round { diameter } => Ok(diameter),
        Outline::Rectangular { .. } => Err(error(format!(
            "{context}: part `{}` is rectangular; layouts place round parts only",
            part.key()
        ))),
    }
}

/// The element's `clear_aperture`, else the part's.
fn clear_aperture(item: &Item, part: &Part) -> Result<f64, FileError> {
    match (item.table.get("clear_aperture"), part.clear_aperture) {
        (Some(_), _) => item.positive("clear_aperture"),
        (None, Some(ca)) => Ok(ca),
        (None, None) => Err(error(format!(
            "{}: part `{}` gives no clear aperture; add `clear_aperture` here",
            item.context,
            part.key()
        ))),
    }
}

/// An element's `coating`, if given.
pub(crate) fn coating_of(item: &Item) -> Result<Option<Coating>, FileError> {
    match item.table.get("coating") {
        None => Ok(None),
        Some(value) => match value.as_str() {
            Some("uncoated") => Ok(Some(Coating::Uncoated)),
            Some("ideal") => Ok(Some(Coating::Ideal)),
            _ => Err(error(format!(
                "{}: coating `{}` is not known; use \"uncoated\" or \"ideal\"",
                item.context,
                value.as_str().unwrap_or("?")
            ))),
        },
    }
}

/// Checks the first two lines: the format tag and the kind.
pub(crate) fn check_header(text: &str) -> Result<(), FileError> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    let first = lines.next().unwrap_or("");
    let second = lines.next().unwrap_or("");
    if !first.starts_with("format") || !second.starts_with("kind") {
        return Err(error(
            "the first two lines must be `format = \"optobench/<N>\"` and `kind = \"<kind>\"`",
        ));
    }
    Ok(())
}

/// Reads a layout file into a scene. `lookup` resolves material names.
pub fn parse(text: &str, lookup: &MaterialLookup) -> Result<Scene, FileError> {
    parse_with(text, lookup, &Library::default())
}

/// A layout file, with parts from `library` for elements that name one.
pub fn parse_with(
    text: &str,
    lookup: &MaterialLookup,
    library: &Library,
) -> Result<Scene, FileError> {
    check_header(text)?;
    let root: Table = text
        .parse()
        .map_err(|e: toml::de::Error| error(format!("not valid TOML: {e}")))?;
    let top = Item {
        table: &root,
        context: "top level".into(),
    };

    let format = top.text("format")?;
    let version: u32 = format
        .strip_prefix(FORMAT_PREFIX)
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| error(format!("format `{format}` is not `optobench/<N>`")))?;
    if version > FORMAT_VERSION {
        return Err(error(format!(
            "format {format} is newer than this release reads (optobench/{FORMAT_VERSION}); \
             a newer optobench is needed"
        )));
    }
    let kind = top.text("kind")?;
    if kind != "layout" {
        return Err(error(format!(
            "kind `{kind}` cannot be traced on its own; this reader takes kind = \"layout\""
        )));
    }
    check_keys(
        &root,
        &[
            "format",
            "kind",
            "layout",
            "surrounding",
            "planar",
            "table",
            "source",
            "element",
            "proxy",
            "detector",
            "trace",
        ],
        "top level",
    )?;

    let surrounding = match root.get("surrounding") {
        None => 1.0.into(),
        Some(v) => medium(v, lookup, "surrounding")?,
    };

    let placement = match top.text("layout")? {
        "planar" => {
            let mut layout = PlanarLayout::default();
            if let Some(planar) = root.get("planar") {
                let table = planar
                    .as_table()
                    .ok_or_else(|| error("[planar] must be a table"))?;
                check_keys(table, &["height", "direction"], "[planar]")?;
                let item = Item {
                    table,
                    context: "[planar]".into(),
                };
                layout.origin = Vec3::new(0.0, 0.0, item.optional_number("height", 0.0)?);
                if table.contains_key("direction") {
                    layout.direction = item.vector("direction")?;
                }
            }
            if root.contains_key("table") {
                return Err(error(
                    "[table] belongs to table layouts; this layout is planar",
                ));
            }
            Placement::Planar(layout)
        }
        "table" => {
            let table = root
                .get("table")
                .and_then(Value::as_table)
                .ok_or_else(|| error("a table layout needs a [table] with `pitch`"))?;
            check_keys(table, &["pitch"], "[table]")?;
            let pitch = Item {
                table,
                context: "[table]".into(),
            }
            .positive("pitch")?;
            Placement::Table(TableLayout { pitch })
        }
        other => {
            return Err(error(format!(
                "layout `{other}` is not known; use \"planar\" or \"table\""
            )));
        }
    };
    // Folds first: in a planar layout every frame depends on them.
    let elements = items(&root, "element")?;
    let mut placement = placement;
    if let Placement::Planar(layout) = &mut placement {
        for item in &elements {
            if item.table.contains_key("fold") {
                layout
                    .folds
                    .push((item.number("at")?, item.number("fold")?));
            }
        }
    }
    let position = placement.keys();

    let mut scene = Scene {
        surrounding,
        elements: Vec::new(),
        sources: Vec::new(),
        proxies: Vec::new(),
        detectors: Vec::new(),
        solids: Vec::new(),
        split_cutoff: trace_options(&root)?,
    };

    for item in items(&root, "source")? {
        let mut frame = placement.frame(&item)?;
        if item.table.contains_key("offset") {
            let (dx, dy) = item.pair("offset")?;
            let origin = frame.to_parent_point(Vec3::new(dx, dy, 0.0));
            let [x, _, z] = frame.axes();
            frame = Frame::new(origin, z, x)
                .ok_or_else(|| error(format!("{}: degenerate frame", item.context)))?;
        }
        scene
            .sources
            .push(source(&item, &with(position, &["offset"]), frame)?);
    }

    for item in &elements {
        let id = item.text("id")?.to_owned();
        let part = library_part(item, library)?;
        let kind = match (item.table.get("kind"), &part) {
            (None, Some(part)) if part.part_type.transmits() => "lens",
            (None, Some(_)) => "mirror",
            _ => item.text("kind")?,
        };
        if let Some(part) = &part {
            if part.part_type.transmits() != (kind == "lens") {
                return Err(error(format!(
                    "{}: part `{}` is a {}, not a {kind}",
                    item.context,
                    part.key(),
                    part.part_type.as_str()
                )));
            }
        }
        match kind {
            "lens" if part.is_some() => {
                let part = part.as_ref().expect("checked");
                check_keys(
                    item.table,
                    &with(
                        position,
                        &[
                            "id",
                            "kind",
                            "library",
                            "reversed",
                            "coating",
                            "clear_aperture",
                        ],
                    ),
                    &item.context,
                )?;
                let diameter = round_diameter(part, &item.context)?;
                let materials = part
                    .glass
                    .iter()
                    .map(|g| {
                        crate::library::glass(g, lookup).map_err(|e| {
                            error(format!("{} (part `{}`): {e}", item.context, part.key()))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                scene.elements.push(Element {
                    id,
                    frame: placement.frame(item)?,
                    kind: ElementKind::Lens {
                        surfaces: part
                            .surfaces
                            .iter()
                            .map(|s| LensSurface {
                                radius: s.radius,
                                conic: s.conic,
                            })
                            .collect(),
                        thicknesses: part.thickness.clone(),
                        materials,
                        diameter,
                        clear_aperture: clear_aperture(item, part)?,
                        coating: coating_of(item)?.unwrap_or_else(|| crate::library::coating(part)),
                    },
                });
            }
            "lens" => {
                check_keys(
                    item.table,
                    &with(
                        position,
                        &[
                            "id",
                            "kind",
                            "surfaces",
                            "thickness",
                            "material",
                            "diameter",
                            "clear_aperture",
                            "coating",
                        ],
                    ),
                    &item.context,
                )?;
                let surfaces = item
                    .get("surfaces")?
                    .as_array()
                    .ok_or_else(|| error(format!("{}: `surfaces` must be a list", item.context)))?
                    .iter()
                    .map(|s| {
                        let table = s.as_table().ok_or_else(|| {
                            error(format!(
                                "{}: each surface is {{ radius = …, conic = … }}",
                                item.context
                            ))
                        })?;
                        let context = format!("{} surface", item.context);
                        check_keys(table, &["radius", "conic"], &context)?;
                        let surface = Item { table, context };
                        Ok(LensSurface {
                            radius: surface.radius(surface.get("radius")?)?,
                            conic: surface.optional_number("conic", 0.0)?,
                        })
                    })
                    .collect::<Result<Vec<_>, FileError>>()?;
                let materials = item
                    .get("material")?
                    .as_array()
                    .ok_or_else(|| error(format!("{}: `material` must be a list", item.context)))?
                    .iter()
                    .map(|m| medium(m, lookup, &item.context))
                    .collect::<Result<Vec<_>, _>>()?;
                let coating = coating_of(item)?.unwrap_or(Coating::Uncoated);
                scene.elements.push(Element {
                    id,
                    frame: placement.frame(item)?,
                    kind: ElementKind::Lens {
                        surfaces,
                        thicknesses: item.numbers("thickness", None)?,
                        materials,
                        diameter: item.positive("diameter")?,
                        clear_aperture: item.positive("clear_aperture")?,
                        coating,
                    },
                });
            }
            "mirror" => {
                let mut keys = with(
                    position,
                    if part.is_some() {
                        &["id", "kind", "library", "reversed", "clear_aperture"]
                    } else {
                        &["id", "kind", "radius", "diameter", "clear_aperture"]
                    },
                );
                if matches!(placement, Placement::Planar(_)) {
                    keys.push("fold");
                }
                check_keys(item.table, &keys, &item.context)?;
                let frame = match &placement {
                    Placement::Planar(layout) if item.table.contains_key("fold") => layout
                        .fold_frame(item.number("at")?)
                        .map_err(|e| error(format!("{}: {e}", item.context)))?,
                    _ => placement.frame(item)?,
                };
                let (radius, diameter, clear_aperture) = match &part {
                    Some(part) => (
                        part.surfaces[0].radius,
                        round_diameter(part, &item.context)?,
                        clear_aperture(item, part)?,
                    ),
                    None => (
                        match item.table.get("radius") {
                            None => f64::INFINITY,
                            Some(v) => item.radius(v)?,
                        },
                        item.positive("diameter")?,
                        item.positive("clear_aperture")?,
                    ),
                };
                scene.elements.push(Element {
                    id,
                    frame,
                    kind: ElementKind::Mirror {
                        radius,
                        diameter,
                        clear_aperture,
                    },
                });
            }
            other => {
                return Err(error(format!(
                    "{}: element kind `{other}` is not known; use \"lens\" or \"mirror\"",
                    item.context
                )));
            }
        }
    }

    for item in items(&root, "proxy")? {
        let id = item.text("id")?.to_owned();
        let (frame, kind) = match item.text("kind")? {
            "tube" => {
                let inner_diameter = item.positive("inner_diameter")?;
                match &placement {
                    Placement::Planar(layout) => {
                        check_keys(
                            item.table,
                            &["id", "kind", "inner_diameter", "from", "to"],
                            &item.context,
                        )?;
                        let (from, to) = (item.number("from")?, item.number("to")?);
                        if to <= from || layout.fold_between(from, to) {
                            return Err(error(format!(
                                "{}: a tube runs from `from` to a larger `to` with no fold between",
                                item.context
                            )));
                        }
                        let frame = layout
                            .frame(from)
                            .map_err(|e| error(format!("{}: {e}", item.context)))?;
                        (
                            frame,
                            ProxyKind::Tube {
                                inner_diameter,
                                length: to - from,
                            },
                        )
                    }
                    Placement::Table(_) => {
                        check_keys(
                            item.table,
                            &with(TABLE_POSITION, &["id", "kind", "inner_diameter", "length"]),
                            &item.context,
                        )?;
                        let length = item.positive("length")?;
                        (
                            placement.frame(&item)?,
                            ProxyKind::Tube {
                                inner_diameter,
                                length,
                            },
                        )
                    }
                }
            }
            "plate" => {
                check_keys(
                    item.table,
                    &with(position, &["id", "kind", "hole_diameter", "size"]),
                    &item.context,
                )?;
                (
                    placement.frame(&item)?,
                    ProxyKind::Plate {
                        hole_diameter: item.positive("hole_diameter")?,
                        size: item.positive("size")?,
                    },
                )
            }
            other => {
                return Err(error(format!(
                    "{}: proxy kind `{other}` is not known; use \"tube\" or \"plate\"",
                    item.context
                )));
            }
        };
        scene.proxies.push(Proxy { id, frame, kind });
    }

    for item in items(&root, "detector")? {
        check_keys(
            item.table,
            &with(position, &["id", "size", "pixels"]),
            &item.context,
        )?;
        let size = item.pair("size")?;
        if !(size.0 > 0.0 && size.1 > 0.0) {
            return Err(error(format!("{}: `size` must be positive", item.context)));
        }
        scene.detectors.push(Detector {
            id: item.text("id")?.to_owned(),
            frame: placement.frame(&item)?,
            size,
            pixels: item.count_pair("pixels")?,
        });
    }
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(name: &str) -> Result<Medium, String> {
        match name {
            "glass" => Ok(1.5.into()),
            other => Err(format!("no material `{other}`")),
        }
    }

    fn read(text: &str) -> Result<Scene, FileError> {
        parse(text, &lookup)
    }

    const HEADER: &str = "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\n";

    #[test]
    fn minimal_planar_scene() {
        let scene = read(&format!(
            "{HEADER}[[detector]]\nid = \"d\"\nat = 10\nsize = [5, 5]\npixels = [2, 2]\n"
        ))
        .expect("valid");
        assert_eq!(scene.detectors.len(), 1);
        assert!((scene.detectors[0].frame.origin() - Vec3::new(10.0, 0.0, 0.0)).length() < 1e-12);
    }

    #[test]
    fn unknown_keys_name_the_closest_valid_key() {
        let error = read(&format!(
            "{HEADER}[[element]]\nid = \"L1\"\nkind = \"lens\"\nat = 0\ndiamter = 25.4\n"
        ))
        .expect_err("rejected");
        assert!(
            error.0.contains("`L1`") && error.0.contains("did you mean `diameter`"),
            "{}",
            error.0
        );
        let error = read(&format!(
            "{HEADER}[[element]]\nid = \"L1\"\nkind = \"lens\"\nat = 0\ndof = []\n"
        ))
        .expect_err("rejected");
        assert!(error.0.contains("M6"), "{}", error.0);
        let error = read(&format!(
            "{HEADER}[[element]]\nid = \"L1\"\nat = 0\nlibrary = \"example/none\"\n"
        ))
        .expect_err("rejected");
        assert!(error.0.contains("not in the parts library"), "{}", error.0);
    }

    #[test]
    fn header_format_and_kind_are_checked() {
        let error = read("kind = \"layout\"\nformat = \"optobench/1\"\n").expect_err("rejected");
        assert!(error.0.contains("first two lines"));
        let error = read("format = \"optobench/9\"\nkind = \"layout\"\nlayout = \"planar\"\n")
            .expect_err("rejected");
        assert!(error.0.contains("newer optobench is needed"));
        let error = read("format = \"optobench/1\"\nkind = \"sidecar\"\nlayout = \"planar\"\n")
            .expect_err("rejected");
        assert!(error.0.contains("kind = \"layout\""));
    }

    #[test]
    fn trace_options_set_or_switch_off_splitting() {
        let scene = read(HEADER).expect("valid");
        assert_eq!(
            scene.split_cutoff,
            Some(optobench_scene::DEFAULT_SPLIT_CUTOFF)
        );
        let off = read(&format!("{HEADER}[trace]\nsplit = false\n")).expect("valid");
        assert_eq!(off.split_cutoff, None);
        let finer = read(&format!("{HEADER}[trace]\nsplit_cutoff = 1e-9\n")).expect("valid");
        assert_eq!(finer.split_cutoff, Some(1e-9));
        let bad = read(&format!("{HEADER}[trace]\nsplit_cutoff = 2\n")).expect_err("rejected");
        assert!(bad.0.contains("between 0 and 1"));
    }

    #[test]
    #[allow(clippy::float_cmp, reason = "values parsed exactly from the file text")]
    fn gaussian_fiber_and_led_sources_with_spectra() {
        let source = |body: &str| {
            read(&format!(
                "{HEADER}[[source]]\nid = \"s\"\npower_w = 0.002\nat = 0\n{body}"
            ))
            .map(|scene| scene.sources[0].clone())
        };
        let beam = source(
            "kind = \"gaussian\"\nwavelength_um = 1.064\nwaist_diameter = 1.0\nm_squared = 1.3\n",
        )
        .expect("valid");
        assert!(matches!(
            beam.kind,
            SourceKind::Gaussian { waist_diameter, m_squared, angle_rings: 3 }
                if waist_diameter == 1.0 && m_squared == 1.3
        ));
        let fiber = source(
            "kind = \"fiber\"\ncore_diameter = 0.05\nna = 0.22\nangle_rings = 6\nspectrum = [[0.488, 1], [0.561, 3.0]]\n",
        )
        .expect("valid");
        assert!(matches!(
            fiber.kind,
            SourceKind::Fiber { numerical_aperture, angle_rings: 6, .. } if numerical_aperture == 0.22
        ));
        // Without wavelength_um, the first spectral line is the source's wavelength.
        assert_eq!(fiber.spectrum, vec![(0.488, 1.0), (0.561, 3.0)]);
        assert_eq!(fiber.wavelength, 0.488);
        let led = source("kind = \"led\"\nwavelength_um = 0.625\nsize = 1.0\n").expect("valid");
        assert!(matches!(led.kind, SourceKind::Led { size, angle_rings: 4 } if size == 1.0));
        for (body, message) in [
            (
                "kind = \"gaussian\"\nwavelength_um = 1\nwaist_diameter = 1\nm_squared = 0.9\n",
                "at least 1",
            ),
            (
                "kind = \"fiber\"\nwavelength_um = 1\ncore_diameter = 0.05\nna = 1.2\n",
                "below 1",
            ),
            ("kind = \"led\"\nsize = 1\n", "missing `wavelength_um`"),
            (
                "kind = \"led\"\nsize = 1\nspectrum = [[0.5, 0]]\n",
                "add up to zero",
            ),
            (
                "kind = \"led\"\nsize = 1\nspectrum = [[0.5]]\n",
                "[wavelength_um, weight]",
            ),
            (
                "kind = \"led\"\nwavelength_um = 1\nsize = 1\nna = 0.1\n",
                "`na`",
            ),
            ("kind = \"laser\"\nwavelength_um = 1\n", "not known"),
        ] {
            let error = source(body).expect_err("rejected");
            assert!(error.0.contains(message), "{body}: {}", error.0);
        }
    }

    #[test]
    fn materials_and_layouts_are_checked() {
        let lens = "[[element]]\nid = \"L1\"\nkind = \"lens\"\nat = 0\nsurfaces = [{ radius = 50 }, { radius = \"inf\" }]\nthickness = [4]\ndiameter = 20\nclear_aperture = 18\n";
        let scene = read(&format!("{HEADER}{lens}material = [\"glass\"]\n")).expect("valid");
        assert_eq!(scene.elements.len(), 1);
        let error =
            read(&format!("{HEADER}{lens}material = [\"unobtainium\"]\n")).expect_err("rejected");
        assert!(error.0.contains("unobtainium"));
        let error = read("format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"table\"\n")
            .expect_err("rejected");
        assert!(error.0.contains("[table]"));
        let error = read(&format!(
            "{HEADER}[[element]]\nid = \"M\"\nkind = \"mirror\"\nat = 50\nfold = 90\ndiameter = 25\nclear_aperture = 22\n[[proxy]]\nid = \"t\"\nkind = \"tube\"\ninner_diameter = 10\nfrom = 0\nto = 100\n"
        ))
        .expect_err("rejected");
        assert!(error.0.contains("no fold between"), "{}", error.0);
    }
}
