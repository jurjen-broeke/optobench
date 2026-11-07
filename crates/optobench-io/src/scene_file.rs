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
//! kind = "collimated"               # or "point" with half_angle_deg
//! wavelength_um = 0.633
//! power_w = 0.001
//! diameter = 2.0
//! rings = 6                         # optional sampling, default 6
//! offset = [0.0, 2.0]               # optional lateral offset in the source frame (mm)
//! at = 0.0                          # planar; table: hole = [0, 0], height = 75, axis = [1, 0, 0]
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

fn error(message: impl Into<String>) -> FileError {
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
const LATER: &[(&str, &str)] = &[
    (
        "library",
        "parts libraries arrive in M5; give the surfaces inline",
    ),
    ("dof", "degrees of freedom arrive in M6"),
];

/// Rejects any key of `table` not in `allowed`, naming the closest valid one.
fn check_keys(table: &Table, allowed: &[&str], context: &str) -> Result<(), FileError> {
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
struct Item<'a> {
    table: &'a Table,
    context: String,
}

impl<'a> Item<'a> {
    fn get(&self, key: &str) -> Result<&'a Value, FileError> {
        self.table
            .get(key)
            .ok_or_else(|| error(format!("{}: missing `{key}`", self.context)))
    }

    fn number_of(&self, value: &Value, key: &str) -> Result<f64, FileError> {
        match value {
            Value::Float(f) => Ok(*f),
            Value::Integer(i) => Ok(*i as f64),
            _ => Err(error(format!("{}: `{key}` must be a number", self.context))),
        }
    }

    fn number(&self, key: &str) -> Result<f64, FileError> {
        self.number_of(self.get(key)?, key)
    }

    fn positive(&self, key: &str) -> Result<f64, FileError> {
        let value = self.number(key)?;
        if value > 0.0 && value.is_finite() {
            Ok(value)
        } else {
            Err(error(format!("{}: `{key}` must be positive", self.context)))
        }
    }

    fn optional_number(&self, key: &str, default: f64) -> Result<f64, FileError> {
        match self.table.get(key) {
            None => Ok(default),
            Some(v) => self.number_of(v, key),
        }
    }

    fn text(&self, key: &str) -> Result<&'a str, FileError> {
        self.get(key)?
            .as_str()
            .ok_or_else(|| error(format!("{}: `{key}` must be text", self.context)))
    }

    fn numbers(&self, key: &str, count: Option<usize>) -> Result<Vec<f64>, FileError> {
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

    fn vector(&self, key: &str) -> Result<Vec3, FileError> {
        let v = self.numbers(key, Some(3))?;
        Ok(Vec3::new(v[0], v[1], v[2]))
    }

    fn pair(&self, key: &str) -> Result<(f64, f64), FileError> {
        let v = self.numbers(key, Some(2))?;
        Ok((v[0], v[1]))
    }

    fn count_pair(&self, key: &str) -> Result<(usize, usize), FileError> {
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

    fn radius(&self, value: &Value) -> Result<f64, FileError> {
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

fn with(base: &[&'static str], extra: &[&'static str]) -> Vec<&'static str> {
    base.iter().chain(extra).copied().collect()
}

fn items<'a>(root: &'a Table, key: &str) -> Result<Vec<Item<'a>>, FileError> {
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

fn medium(value: &Value, lookup: &MaterialLookup, context: &str) -> Result<Medium, FileError> {
    match value {
        Value::Float(f) => Ok((*f).into()),
        Value::Integer(i) => Ok((*i as f64).into()),
        Value::String(name) => lookup(name).map_err(|e| error(format!("{context}: {e}"))),
        _ => Err(error(format!(
            "{context}: a material is a name or an index"
        ))),
    }
}

/// Checks the first two lines: the format tag and the kind.
fn check_header(text: &str) -> Result<(), FileError> {
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
    };

    for item in items(&root, "source")? {
        let kind = item.text("kind")?;
        let source_kind = match kind {
            "collimated" => {
                check_keys(
                    item.table,
                    &with(
                        position,
                        &[
                            "id",
                            "kind",
                            "wavelength_um",
                            "power_w",
                            "diameter",
                            "rings",
                            "offset",
                        ],
                    ),
                    &item.context,
                )?;
                SourceKind::Collimated {
                    diameter: item.positive("diameter")?,
                }
            }
            "point" => {
                check_keys(
                    item.table,
                    &with(
                        position,
                        &[
                            "id",
                            "kind",
                            "wavelength_um",
                            "power_w",
                            "half_angle_deg",
                            "rings",
                            "offset",
                        ],
                    ),
                    &item.context,
                )?;
                SourceKind::Point {
                    half_angle: radians_from_degrees(item.positive("half_angle_deg")?),
                }
            }
            other => {
                return Err(error(format!(
                    "{}: source kind `{other}` is not known; use \"collimated\" or \"point\"",
                    item.context
                )));
            }
        };
        let mut frame = placement.frame(&item)?;
        if item.table.contains_key("offset") {
            let (dx, dy) = item.pair("offset")?;
            let origin = frame.to_parent_point(Vec3::new(dx, dy, 0.0));
            let [x, _, z] = frame.axes();
            frame = Frame::new(origin, z, x)
                .ok_or_else(|| error(format!("{}: degenerate frame", item.context)))?;
        }
        let rings = item.optional_number("rings", 6.0)?;
        if rings < 0.0 || rings.fract() != 0.0 || rings > 200.0 {
            return Err(error(format!(
                "{}: `rings` must be a whole number from 0 to 200",
                item.context
            )));
        }
        scene.sources.push(Source {
            id: item.text("id")?.to_owned(),
            frame,
            kind: source_kind,
            wavelength: item.positive("wavelength_um")?,
            power: item.positive("power_w")?,
            rings: rings as usize,
        });
    }

    for item in &elements {
        let id = item.text("id")?.to_owned();
        match item.text("kind")? {
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
                let coating = match item.table.get("coating").and_then(Value::as_str) {
                    None | Some("uncoated") => Coating::Uncoated,
                    Some("ideal") => Coating::Ideal,
                    Some(other) => {
                        return Err(error(format!(
                            "{}: coating `{other}` is not known; use \"uncoated\" or \"ideal\"",
                            item.context
                        )));
                    }
                };
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
                    &["id", "kind", "radius", "diameter", "clear_aperture"],
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
                let radius = match item.table.get("radius") {
                    None => f64::INFINITY,
                    Some(v) => item.radius(v)?,
                };
                scene.elements.push(Element {
                    id,
                    frame,
                    kind: ElementKind::Mirror {
                        radius,
                        diameter: item.positive("diameter")?,
                        clear_aperture: item.positive("clear_aperture")?,
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
            "{HEADER}[[element]]\nid = \"L1\"\nkind = \"lens\"\nat = 0\nlibrary = \"x\"\n"
        ))
        .expect_err("rejected");
        assert!(error.0.contains("M5"), "{}", error.0);
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
