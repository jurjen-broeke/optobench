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
//! columns = 24                      # optional, the breadboard: holes along x and y
//! rows = 18                         # (together), its edge half a pitch beyond the
//! corner = [0, 0]                   # outer holes; corner: the hole at its corner
//!                                   # nearest the origin (default [0, 0])
//!
//! [[source]]
//! id = "laser"
//! kind = "collimated"               # or "point" with half_angle_deg, or "gaussian",
//!                                   # "fiber", "led", "isotropic" (below)
//! wavelength_um = 0.633             # optional when spectrum is given
//! spectrum = [[0.488, 1.0], [0.561, 3.0]]  # optional lines [µm, weight]: power shared by weight
//! power_w = 0.001
//! diameter = 2.0
//! rings = 6                         # optional sampling, default 6
//! offset = [0.0, 2.0]               # optional lateral offset in the source frame (mm)
//! reversed = false                  # optional: true points it back along the path
//!                                   # (an emitter in a sample, facing the objective)
//! pulse = { repetition_rate_mhz = 80, width_fs = 120, shape = "sech2", prechirp_fs2 = 0 }
//!                                   # optional, a pulsed laser: power_w is the mean;
//!                                   # width the transform-limited FWHM; shape "sech2"
//!                                   # (default) or "gaussian"; prechirp optional
//! at = 0.0                          # planar; table: hole = [0, 0], height = 75, axis = [1, 0, 0]
//!
//! # Other kinds (angle_rings: optional angular sampling, default 3; led 4):
//! #   gaussian: waist_diameter (1/e² diameter at the source, mm), m_squared (≥ 1, default 1)
//! #   fiber:    core_diameter (mm), na (< 1): uniform over the core, within the NA
//! #   led:      size (square side, mm): Lambertian over the hemisphere
//! #   isotropic: a point emitting equally in every direction (a fluorophore); power_w
//! #             is the total, rays only within half_angle_deg (default 180) of the
//! #             axis, so aim it at the objective; rings default 30
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
//! coating = "uncoated"              # default; or "ideal", { curve = "<name>" },
//!                                   # { layers = [...] }: see `coatings`
//! # coatings = ["ideal", "uncoated"]  # instead: one per surface
//!
//! [[element]]
//! id = "SL"
//! kind = "ideal_lens"               # a perfect thin lens: slopes change by −h / f
//! at = 100.0
//! focal_length = 50.0               # negative for a diverging lens
//! diameter = 25.4                   # optional; rays beyond it are clipped
//!
//! [[element]]
//! id = "OBJ"
//! kind = "objective"                # an ideal objective (optobench_trace::objective)
//! at = 200.0                        # its pupil plane; +z (along the path) to the sample
//! focal_length = 9.0                # f: the pupil radius is f · na
//! na = 1.0
//! pupil_distance = 45.0             # pupil plane to the front plane (mm)
//! working_distance = 2.0            # design: front plane to the cover glass (or focus)
//! immersion = "water"               # design immersion; default 1.0 (dry)
//! cover_glass = { thickness = 0.17, material = 1.5255 }   # design; omit for none
//! sample_medium = "water"           # design medium at the focus; default the immersion
//! group_delay_dispersion_fs2 = 0    # optional: its glass's GDD at the laser wavelength,
//!                                   # as its maker states it (the ideal objective has none)
//! dof = [ { name = "OBJ_collar", kind = "collar", range = [-0.04, 0.04] } ]
//!                                   # correction collar: the change in the cover-glass
//!                                   # thickness it corrects for (mm); no axis
//!
//! [[element]]
//! id = "S"
//! kind = "sample"                   # flat layers from `at` along the path
//! at = 247.0
//! above = "water"                   # the immersion above the first face; default the
//!                                   # surroundings (the objective's rays go on in it)
//! layers = [ { thickness = 0.17, material = 1.5255 }, { thickness = 5.0, material = "water" } ]
//!                                   # a turbid layer adds absorption µa (in place of the
//!                                   # material's own), scattering µs (both 1/mm) and
//!                                   # anisotropy g, each a number or [[wavelength_um, value], …],
//!                                   # and says where they come from:
//!                                   # { thickness = 1.0, material = 1.36, absorption = 0.02,
//!                                   #   scattering = 20.0, anisotropy = 0.9, source = "…" }
//!                                   # and thermal properties for heating (M11d):
//!                                   # thermal = { conductivity_w_per_m_k = 0.527,
//!                                   #   density_kg_per_m3 = 1040, heat_capacity_j_per_kg_k = 3650,
//!                                   #   perfusion_per_s = 8.5e-3, blood_density_kg_per_m3 = 1060,
//!                                   #   blood_heat_capacity_j_per_kg_k = 3600, source = "…" }
//!                                   # (perfusion and the blood's values optional)
//! thermal_boundaries = { top = "fixed", bottom = "fixed", side = "insulated" }
//!                                   # optional, each "fixed" (default), "insulated" or a
//!                                   # heat transfer coefficient (W m⁻² K⁻¹)
//! below = 1.0                       # optional; default the surroundings
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
//! id = "L3"
//! # … as L1, plus degrees of freedom (elements, sources and detectors; see
//! # optobench_scene::dof): slides in mm, tilts in degrees, axes and pivots in the item's
//! # own frame unless frame = "scene"
//! dof = [ { name = "L3_z", kind = "slide", axis = "z", range = [-10, 10] },
//!         { name = "L3_tip", kind = "tilt", axis = "x", pivot = [0, 0, 0], range = [-2, 2] },
//!         { name = "L3_x", kind = "slide", axis = "x", locked = true, value = 0.1 } ]
//! # a coupled DOF: { name = "D_z", kind = "slide", axis = "z", follows = "L3_z", ratio = 1 }
//! # a DOF's travel: `range = [low, high]`, or `travel = 12.7` (the whole travel, the
//! # part placed in its middle); `source = "…"` says where it comes from. A mount's
//! # adjusters are written once in a [[mount]] and given by `mount = "<id>"` (see
//! # `mounts`); `trace`, `optimize` and `tolerance` warn about slides that adjust 1 mm or
//! # less either way (`small_range_mm`)
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
//! quantum_efficiency = { value = [[0.45, 0.25], [0.55, 0.40]], source = "…" }
//!                                   # optional: a number or [[wavelength_um, value], …]
//!                                   # from 0 to 1, and where it comes from
//!
//! [[fluorophore]]                   # what a dye or protein absorbs and emits (M11b)
//! id = "dye"
//! source = "…"                      # where the values come from (required)
//! quantum_yield = 0.9
//! lifetime_ns = 4.0                 # optional
//! extinction_coefficient = 56000    # optional, L/(mol cm): a number at the peak, shaped
//! excitation = [[0.45, 0.2], [0.49, 1.0], [0.52, 0.1]]  # by this relative spectrum,
//!                                   # or a table; or one_photon_cross_section_cm2
//! two_photon_cross_section_gm = [[0.9, 30.0], [0.94, 25.0]]  # optional, GM
//!                                   # (10⁻⁵⁰ cm⁴ s), two photons per excitation; or
//!                                   # two_photon_action_cross_section_gm (η σ₂, as most
//!                                   # spectra are published), divided by quantum_yield;
//!                                   # a number with two_photon_spectrum (relative) applies at
//!                                   # two_photon_reference_um, else at its greatest value
//! emission = [[0.5, 0.3], [0.52, 1.0], [0.6, 0.1]]      # optional, relative
//! concentrations = [ { sample = "S", layer = 2, micromolar = 10.0 } ]
//!
//! [[merit]]                         # what `optimize` aligns for: see `merit`
//! kind = "focus"                    # or "centring", "pointing", "collimation", "diameter"
//!                                   # (with target, mm), "power"
//! detector = "screen"
//!
//! # [resolved] ...                  # written by `embed`, always last: see `resolved`
//! ```
//!
//! Parsing is strict (docs/design.md, Format tag and versions): the first two lines are
//! the format tag and the kind, a newer format is refused, and an unknown key is an error
//! naming the closest valid key. Lengths are in mm, angles in degrees.

use optobench_library::{CoatingSpec, Library, Outline, Part};
use optobench_scene::dof::{Dof, Motion, Target};

use crate::coatings::CurveLookup;
use optobench_materials::{Medium, Spectrum, Turbid};
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
    // Edit distance with adjacent transpositions counted as one edit (optimal string
    // alignment), so `rnage` is one edit from `range`.
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[a.len()][b.len()]
}

/// Keys that belong to later milestones, with the milestone.
const LATER: &[(&str, &str)] = &[];

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

/// Checks the breadboard of a table layout (`[table]` `columns`, `rows`, `corner`):
/// the number of holes along x and y, and the hole (column, row) at the board's corner
/// nearest the origin (default [0, 0]); the board's edge lies half a pitch beyond its
/// outer holes. Only the interface draws it; the trace does not need it.
fn board(table: &Table) -> Result<(), FileError> {
    let whole = |key: &str, least: i64| -> Result<Option<i64>, FileError> {
        match table.get(key) {
            None => Ok(None),
            Some(v) => v
                .as_integer()
                .filter(|n| *n >= least)
                .map(Some)
                .ok_or_else(|| error(format!("[table]: `{key}` must be a whole number ≥ {least}"))),
        }
    };
    match (whole("columns", 1)?, whole("rows", 1)?) {
        (Some(_), Some(_)) => {}
        (None, None) => {
            if table.contains_key("corner") {
                return Err(error("[table]: `corner` goes with `columns` and `rows`"));
            }
            return Ok(());
        }
        _ => return Err(error("[table]: give `columns` and `rows` together")),
    }
    match table.get("corner") {
        None => Ok(()),
        Some(v) => {
            let pair = v.as_array().and_then(|a| match a.as_slice() {
                [c, r] => Some((c.as_integer()?, r.as_integer()?)),
                _ => None,
            });
            pair.map(|_| ()).ok_or_else(|| {
                error("[table]: `corner` is the hole [column, row] at the board's corner")
            })
        }
    }
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
        "isotropic" => (&["half_angle_deg"], 30.0),
        other => {
            return Err(error(format!(
                "{}: source kind `{other}` is not known; use \"collimated\", \"point\", \"gaussian\", \"fiber\", \"led\" or \"isotropic\"",
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
        "pulse",
        "dof",
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
        "isotropic" => SourceKind::Isotropic {
            half_angle: {
                let degrees = item.optional_number("half_angle_deg", 180.0)?;
                if !(degrees > 0.0 && degrees <= 180.0) {
                    return Err(error(format!(
                        "{}: `half_angle_deg` lies above 0 and up to 180",
                        item.context
                    )));
                }
                radians_from_degrees(degrees)
            },
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
        pulse: pulse(item)?,
    })
}

/// A source's `pulse = { repetition_rate_mhz, width_fs, shape, prechirp_fs2 }`.
fn pulse(item: &Item) -> Result<Option<optobench_scene::Pulse>, FileError> {
    let Some(value) = item.table.get("pulse") else {
        return Ok(None);
    };
    let context = format!("{} `pulse`", item.context);
    let table = value.as_table().ok_or_else(|| {
        error(format!(
            "{context}: write {{ repetition_rate_mhz = …, width_fs = …, shape = \"sech2\" }}"
        ))
    })?;
    check_keys(
        table,
        &["repetition_rate_mhz", "width_fs", "shape", "prechirp_fs2"],
        &context,
    )?;
    let inner = Item { table, context };
    let shape = match table.get("shape").map(|v| v.as_str()) {
        None | Some(Some("sech2")) => optobench_scene::PulseShape::SechSquared,
        Some(Some("gaussian")) => optobench_scene::PulseShape::Gaussian,
        _ => {
            return Err(error(format!(
                "{}: `shape` is \"sech2\" (default) or \"gaussian\"",
                inner.context
            )));
        }
    };
    Ok(Some(optobench_scene::Pulse {
        repetition_rate: inner.positive("repetition_rate_mhz")? * 1e6,
        width: inner.positive("width_fs")? * 1e-15,
        shape,
        prechirp: inner.optional_number("prechirp_fs2", 0.0)? * 1e-30,
    }))
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

/// An element's `coating` (on every one of `surfaces`) or `coatings` (one per surface),
/// if given.
pub(crate) fn element_coatings(
    item: &Item,
    surfaces: usize,
) -> Result<Option<Vec<CoatingSpec>>, FileError> {
    match (item.table.get("coating"), item.table.get("coatings")) {
        (None, None) => Ok(None),
        (Some(c), None) => Ok(Some(vec![
            crate::coatings::parse(c, &item.context)?;
            surfaces
        ])),
        (None, Some(list)) => crate::coatings::parse_list(list, surfaces, &item.context).map(Some),
        (Some(_), Some(_)) => Err(error(format!(
            "{}: give `coating` or `coatings`, not both",
            item.context
        ))),
    }
}

/// The part with the element's `coating` or `coatings` in place of its own; a filter's
/// curve still covers the whole part (see `Part::surface_coatings`).
pub(crate) fn with_element_coatings(item: &Item, part: &Part) -> Result<Part, FileError> {
    let mut part = part.clone();
    match (item.table.get("coating"), item.table.get("coatings")) {
        (None, None) => {}
        (Some(c), None) => {
            part.coating = Some(crate::coatings::parse(c, &item.context)?);
            part.coatings.clear();
        }
        (None, Some(list)) => {
            part.coatings = crate::coatings::parse_list(list, part.surfaces.len(), &item.context)?;
            part.coating = None;
        }
        (Some(_), Some(_)) => {
            return Err(error(format!(
                "{}: give `coating` or `coatings`, not both",
                item.context
            )));
        }
    }
    Ok(part)
}

/// Resolves coatings, naming the item in errors.
pub(crate) fn resolve_coatings(
    specs: &[CoatingSpec],
    lookups: &Lookups,
    context: &str,
) -> Result<Vec<Coating>, FileError> {
    specs
        .iter()
        .map(|spec| {
            crate::coatings::resolve(spec, lookups.materials, lookups.curves)
                .map_err(|e| error(format!("{context}: {e}")))
        })
        .collect()
}

/// An item's `dof` list (see `optobench_scene::dof`). `local` is the item's nominal
/// frame; axes and pivots are in it unless a DOF says `frame = "scene"` (the default
/// with `scene_default`). `default_pivot` (scene frame) is where tilts turn without a
/// `pivot`. Lengths in mm, angles in degrees.
pub(crate) fn dofs(
    item: &Item,
    target: &Target,
    local: &Frame,
    scene_default: bool,
    default_pivot: Vec3,
) -> Result<Vec<Dof>, FileError> {
    let Some(value) = item.table.get("dof") else {
        return Ok(Vec::new());
    };
    let list = value.as_array().ok_or_else(|| {
        error(format!(
            "{}: `dof` is a list of {{ name, kind, axis, … }}",
            item.context
        ))
    })?;
    list.iter()
        .map(|entry| {
            let table = entry.as_table().ok_or_else(|| {
                error(format!(
                    "{}: each DOF is {{ name = …, kind = \"slide\" | \"tilt\", axis = … }}",
                    item.context
                ))
            })?;
            let name = table.get("name").and_then(Value::as_str).unwrap_or("?");
            let context = format!("{} DOF `{name}`", item.context);
            check_keys(
                table,
                &[
                    "name", "kind", "axis", "frame", "pivot", "range", "travel", "value",
                    "locked", "follows", "ratio", "source",
                ],
                &context,
            )?;
            let dof = Item {
                table,
                context: context.clone(),
            };
            let name = dof.text("name")?.to_owned();
            let kind = dof.text("kind")?;
            if kind == "collar" {
                if let Some(key) = ["axis", "pivot", "frame"]
                    .iter()
                    .find(|k| table.contains_key(**k))
                {
                    return Err(error(format!(
                        "{context}: a collar changes the cover glass the objective corrects for; it has no `{key}`"
                    )));
                }
            }
            let in_scene = match table.get("frame").map(|f| f.as_str()) {
                None => scene_default,
                Some(Some("scene")) => true,
                Some(Some("local")) => false,
                Some(_) => {
                    return Err(error(format!(
                        "{context}: `frame` is \"local\" (the item's own) or \"scene\""
                    )));
                }
            };
            let to_scene_vector = |v: Vec3| {
                if in_scene {
                    v
                } else {
                    local.to_parent_vector(v)
                }
            };
            let axis = match table.get("axis") {
                _ if kind == "collar" => Vec3::Z,
                Some(Value::String(a)) => match a.as_str() {
                    "x" => Vec3::X,
                    "y" => Vec3::Y,
                    "z" => Vec3::Z,
                    _ => {
                        return Err(error(format!(
                            "{context}: `axis` is \"x\", \"y\", \"z\" or [x, y, z]"
                        )));
                    }
                },
                Some(_) => dof.vector("axis")?,
                None => return Err(error(format!("{context}: missing `axis`"))),
            };
            let axis = to_scene_vector(axis)
                .normalized()
                .ok_or_else(|| error(format!("{context}: `axis` has zero length")))?;
            let scale = match kind {
                "slide" | "collar" => 1.0,
                "tilt" => core::f64::consts::PI / 180.0,
                other => {
                    return Err(error(format!(
                        "{context}: kind `{other}` is not known; use \"slide\", \"tilt\" or \"collar\""
                    )));
                }
            };
            let motion = if kind == "collar" {
                Motion::Collar
            } else if kind == "slide" {
                if table.contains_key("pivot") {
                    return Err(error(format!("{context}: a slide has no `pivot`")));
                }
                Motion::Slide { axis }
            } else {
                let pivot = if table.contains_key("pivot") {
                    let p = dof.vector("pivot")?;
                    if in_scene {
                        p
                    } else {
                        local.to_parent_point(p)
                    }
                } else {
                    default_pivot
                };
                Motion::Tilt { axis, pivot }
            };
            let range = match (table.get("range"), table.get("travel")) {
                (Some(_), Some(_)) => {
                    return Err(error(format!(
                        "{context}: give `range` or `travel`, not both"
                    )));
                }
                (None, None) => None,
                // The whole travel, the part placed in its middle.
                (None, Some(_)) => {
                    let travel = dof.positive("travel")?;
                    Some((-0.5 * travel * scale, 0.5 * travel * scale))
                }
                (Some(_), None) => {
                    let (low, high) = dof.pair("range")?;
                    if low > high {
                        return Err(error(format!("{context}: `range` runs from low to high")));
                    }
                    Some((low * scale, high * scale))
                }
            };
            let locked = match table.get("locked") {
                None => false,
                Some(Value::Boolean(b)) => *b,
                Some(_) => return Err(error(format!("{context}: `locked` is true or false"))),
            };
            let follows = match table.get("follows") {
                None => {
                    if table.contains_key("ratio") {
                        return Err(error(format!("{context}: `ratio` goes with `follows`")));
                    }
                    None
                }
                Some(Value::String(leader)) => {
                    Some((leader.clone(), dof.optional_number("ratio", 1.0)?))
                }
                Some(_) => return Err(error(format!("{context}: `follows` names another DOF"))),
            };
            if follows.is_some() && (locked || table.contains_key("value")) {
                return Err(error(format!(
                    "{context}: a DOF that follows another takes its value from it"
                )));
            }
            Ok(Dof {
                name,
                target: target.clone(),
                motion,
                range,
                locked,
                follows,
                value: dof.optional_number("value", 0.0)? * scale,
                source: match table.get("source") {
                    None => None,
                    Some(Value::String(text)) if !text.trim().is_empty() => Some(text.clone()),
                    Some(_) => {
                        return Err(error(format!(
                            "{context}: `source` says where the range comes from (text)"
                        )));
                    }
                },
            })
        })
        .collect()
}

/// Checks the scene's DOFs: unique names, couplings that resolve, values in range.
pub(crate) fn check_dofs(scene: &Scene) -> Result<(), FileError> {
    for (i, dof) in scene.dofs.iter().enumerate() {
        if scene.dofs[..i].iter().any(|d| d.name == dof.name) {
            return Err(error(format!(
                "two degrees of freedom are named `{}`",
                dof.name
            )));
        }
    }
    scene
        .dof_values(&std::collections::BTreeMap::new())
        .map(|_| ())
        .map_err(|e| error(e.to_string()))
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

/// The keys of an objective besides its placement.
pub(crate) const OBJECTIVE_KEYS: &[&str] = &[
    "id",
    "kind",
    "focal_length",
    "na",
    "pupil_distance",
    "working_distance",
    "immersion",
    "cover_glass",
    "sample_medium",
    "diameter",
    "group_delay_dispersion_fs2",
    "dof",
];

/// The keys of a sample besides its placement.
pub(crate) const SAMPLE_KEYS: &[&str] = &[
    "id",
    "kind",
    "layers",
    "above",
    "below",
    "diameter",
    "thermal_boundaries",
    "dof",
];

/// An `objective` entry's optics (layouts and sidecars alike).
pub(crate) fn objective_kind(
    item: &Item,
    lookup: &MaterialLookup,
) -> Result<ElementKind, FileError> {
    let material = |key: &str| -> Result<Option<Medium>, FileError> {
        item.table
            .get(key)
            .map(|v| medium(v, lookup, &format!("{} `{key}`", item.context)))
            .transpose()
    };
    let immersion = material("immersion")?.unwrap_or_else(|| 1.0.into());
    let working_distance = item.positive("working_distance")?;
    let mut design_layers = vec![optobench_trace::objective::DesignLayer {
        thickness: working_distance,
        medium: immersion.clone(),
    }];
    if let Some(cover) = item.table.get("cover_glass") {
        let table = cover.as_table().ok_or_else(|| {
            error(format!(
                "{}: `cover_glass` is {{ thickness = …, material = … }}",
                item.context
            ))
        })?;
        let context = format!("{} `cover_glass`", item.context);
        check_keys(table, &["thickness", "material"], &context)?;
        let cover = Item { table, context };
        design_layers.push(optobench_trace::objective::DesignLayer {
            thickness: cover.positive("thickness")?,
            medium: medium(cover.get("material")?, lookup, &cover.context)?,
        });
    }
    let design_sample = material("sample_medium")?.unwrap_or_else(|| immersion.clone());
    let objective = optobench_trace::objective::IdealObjective {
        focal_length: item.positive("focal_length")?,
        na: item.positive("na")?,
        pupil_distance: item.positive("pupil_distance")?,
        design_layers,
        design_sample,
    };
    let diameter =
        item.optional_number("diameter", (2.0 * objective.pupil_radius() * 1.5).max(20.0))?;
    let group_delay_dispersion = item.optional_number("group_delay_dispersion_fs2", 0.0)?;
    if !(group_delay_dispersion.is_finite() && group_delay_dispersion >= 0.0) {
        return Err(error(format!(
            "{}: `group_delay_dispersion_fs2` is a number, not negative",
            item.context
        )));
    }
    Ok(ElementKind::Objective {
        objective,
        diameter,
        group_delay_dispersion: group_delay_dispersion * 1e-30,
    })
}

/// A layer's optical quantity: a number (1/mm, or none for g), or a table
/// `[[wavelength_um, value], …]` in increasing wavelength; values not negative (g may be).
fn spectrum(layer: &Item, key: &str) -> Result<Option<Spectrum>, FileError> {
    let context = &layer.context;
    let Some(value) = layer.table.get(key) else {
        return Ok(None);
    };
    let signed = key == "anisotropy";
    let check = |v: f64| -> Result<f64, FileError> {
        if v.is_finite() && (signed || v >= 0.0) {
            Ok(v)
        } else {
            Err(error(format!(
                "{context}: `{key}` must be a finite number{}",
                if signed { "" } else { ", not negative" }
            )))
        }
    };
    match value {
        Value::Float(_) | Value::Integer(_) => {
            Ok(Some(Spectrum::Constant(check(layer.number(key)?)?)))
        }
        Value::Array(rows) => {
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                let pair = row
                    .as_array()
                    .filter(|r| r.len() == 2)
                    .and_then(|r| {
                        let n =
                            |v: &Value| v.as_float().or_else(|| v.as_integer().map(|i| i as f64));
                        Some((n(&r[0])?, n(&r[1])?))
                    })
                    .ok_or_else(|| {
                        error(format!(
                            "{context}: `{key}` is a number or [[wavelength_um, value], …]"
                        ))
                    })?;
                out.push((pair.0, check(pair.1)?));
            }
            if out.len() < 2 || out.windows(2).any(|w| w[1].0 <= w[0].0) {
                return Err(error(format!(
                    "{context}: `{key}` needs at least two rows in increasing wavelength"
                )));
            }
            Ok(Some(Spectrum::Table(out)))
        }
        _ => Err(error(format!(
            "{context}: `{key}` is a number or [[wavelength_um, value], …]"
        ))),
    }
}

/// A detector's `quantum_efficiency = { value = …, source = "…" }` (layouts and sidecars
/// alike): a number or a table over wavelength, every value from 0 to 1.
pub(crate) fn quantum_efficiency(
    item: &Item,
) -> Result<Option<optobench_scene::QuantumEfficiency>, FileError> {
    let Some(value) = item.table.get("quantum_efficiency") else {
        return Ok(None);
    };
    let context = format!("{} `quantum_efficiency`", item.context);
    let table = value
        .as_table()
        .ok_or_else(|| error(format!("{context}: write {{ value = …, source = \"…\" }}")))?;
    check_keys(table, &["value", "source"], &context)?;
    let inner = Item {
        table,
        context: context.clone(),
    };
    let curve = spectrum(&inner, "value")?
        .ok_or_else(|| error(format!("{context}: `value` is required")))?;
    let values: Vec<f64> = match &curve {
        Spectrum::Constant(v) => vec![*v],
        Spectrum::Table(rows) => rows.iter().map(|r| r.1).collect(),
    };
    if values.iter().any(|v| *v > 1.0) {
        return Err(error(format!("{context}: values lie from 0 to 1")));
    }
    let source = match table.get("source") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        _ => {
            return Err(error(format!(
                "{context}: say where the values come from (`source`, text)"
            )));
        }
    };
    Ok(Some(optobench_scene::QuantumEfficiency { curve, source }))
}

/// A `sample` entry's layers (layouts and sidecars alike).
pub(crate) fn sample_kind(item: &Item, lookup: &MaterialLookup) -> Result<ElementKind, FileError> {
    let layers = item
        .get("layers")?
        .as_array()
        .ok_or_else(|| error(format!("{}: `layers` must be a list", item.context)))?
        .iter()
        .map(|l| {
            let table = l.as_table().ok_or_else(|| {
                error(format!(
                    "{}: each layer is {{ thickness = …, material = … }}",
                    item.context
                ))
            })?;
            let context = format!("{} layer", item.context);
            check_keys(
                table,
                &[
                    "thickness",
                    "material",
                    "absorption",
                    "scattering",
                    "anisotropy",
                    "source",
                    "thermal",
                ],
                &context,
            )?;
            let layer = Item { table, context };
            let base = medium(layer.get("material")?, lookup, &layer.context)?;
            let optics = ["absorption", "scattering", "anisotropy"]
                .iter()
                .any(|k| table.contains_key(*k));
            let medium = if optics {
                let source = match table.get("source") {
                    Some(Value::String(s)) => s.clone(),
                    Some(_) => {
                        return Err(error(format!("{}: `source` is text", layer.context)));
                    }
                    None => {
                        return Err(error(format!(
                            "{}: say where µa, µs and g come from (`source`)",
                            layer.context
                        )));
                    }
                };
                let anisotropy = spectrum(&layer, "anisotropy")?.unwrap_or(Spectrum::Constant(0.0));
                if let Spectrum::Constant(g) = anisotropy {
                    if !(-1.0..=1.0).contains(&g) {
                        return Err(error(format!(
                            "{}: `anisotropy` lies from −1 to 1",
                            layer.context
                        )));
                    }
                }
                Medium::from(Turbid {
                    base,
                    absorption: spectrum(&layer, "absorption")?,
                    scattering: spectrum(&layer, "scattering")?.unwrap_or(Spectrum::Constant(0.0)),
                    anisotropy,
                    source,
                })
            } else {
                if table.contains_key("source") {
                    return Err(error(format!(
                        "{}: `source` goes with `absorption`, `scattering` or `anisotropy`",
                        layer.context
                    )));
                }
                base
            };
            Ok((layer.positive("thickness")?, medium))
        })
        .collect::<Result<Vec<_>, FileError>>()?;
    let material = |key: &str| -> Result<Option<Medium>, FileError> {
        item.table
            .get(key)
            .map(|v| medium(v, lookup, &format!("{} `{key}`", item.context)))
            .transpose()
    };
    Ok(ElementKind::Sample {
        layers,
        above: material("above")?,
        below: material("below")?,
        diameter: item.optional_number("diameter", 50.0)?,
    })
}

/// A sample's thermal description (layouts and sidecars alike): each layer's
/// `thermal = { conductivity_w_per_m_k, density_kg_per_m3, heat_capacity_j_per_kg_k,
/// perfusion_per_s, blood_density_kg_per_m3, blood_heat_capacity_j_per_kg_k, source }`
/// (perfusion and the blood's values optional, together) and the sample's
/// `thermal_boundaries = { top, bottom, side }`, each `"fixed"` (default), `"insulated"`
/// or a heat transfer coefficient in W m⁻² K⁻¹. `None` if no layer has `thermal`.
pub(crate) fn sample_thermal(
    item: &Item,
    id: &str,
) -> Result<Option<optobench_scene::SampleThermal>, FileError> {
    use optobench_scene::{HeatBoundary, SampleThermal, ThermalProperties};
    let Some(layers) = item.table.get("layers").and_then(Value::as_array) else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(layers.len());
    for (n, layer) in layers.iter().enumerate() {
        let Some(value) = layer.get("thermal") else {
            out.push(None);
            continue;
        };
        let context = format!("{} layer {} `thermal`", item.context, n + 1);
        let table = value.as_table().ok_or_else(|| {
            error(format!(
                "{context}: write {{ conductivity_w_per_m_k = …, … }}"
            ))
        })?;
        check_keys(
            table,
            &[
                "conductivity_w_per_m_k",
                "density_kg_per_m3",
                "heat_capacity_j_per_kg_k",
                "perfusion_per_s",
                "blood_density_kg_per_m3",
                "blood_heat_capacity_j_per_kg_k",
                "source",
            ],
            &context,
        )?;
        let t = Item { table, context };
        let source = match table.get("source") {
            Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
            _ => {
                return Err(error(format!(
                    "{}: say where the values come from (`source`, text)",
                    t.context
                )));
            }
        };
        let blood = ["blood_density_kg_per_m3", "blood_heat_capacity_j_per_kg_k"];
        let perfusion = match table.get("perfusion_per_s") {
            None => {
                if blood.iter().any(|k| table.contains_key(*k)) {
                    return Err(error(format!(
                        "{}: the blood's values go with `perfusion_per_s`",
                        t.context
                    )));
                }
                0.0
            }
            Some(_) => {
                let w = t.number("perfusion_per_s")?;
                if !(w >= 0.0 && w.is_finite()) {
                    return Err(error(format!(
                        "{}: `perfusion_per_s` is a number, not negative",
                        t.context
                    )));
                }
                // ρ_b c_b w_b in W m⁻³ K⁻¹, to W mm⁻³ K⁻¹.
                t.positive(blood[0])? * t.positive(blood[1])? * w * 1e-9
            }
        };
        out.push(Some(ThermalProperties {
            conductivity: t.positive("conductivity_w_per_m_k")? * 1e-3,
            capacity: t.positive("density_kg_per_m3")?
                * t.positive("heat_capacity_j_per_kg_k")?
                * 1e-9,
            perfusion,
            source,
        }));
    }
    let boundaries = item.table.get("thermal_boundaries");
    if out.iter().all(Option::is_none) {
        if boundaries.is_some() {
            return Err(error(format!(
                "{}: `thermal_boundaries` goes with the layers' `thermal`",
                item.context
            )));
        }
        return Ok(None);
    }
    let mut sides = [HeatBoundary::Fixed; 3];
    if let Some(value) = boundaries {
        let context = format!("{} `thermal_boundaries`", item.context);
        let table = value.as_table().ok_or_else(|| {
            error(format!(
                "{context}: write {{ top = …, bottom = …, side = … }}"
            ))
        })?;
        check_keys(table, &["top", "bottom", "side"], &context)?;
        for (slot, key) in sides.iter_mut().zip(["top", "bottom", "side"]) {
            *slot = match table.get(key) {
                None => HeatBoundary::Fixed,
                Some(Value::String(s)) if s == "fixed" => HeatBoundary::Fixed,
                Some(Value::String(s)) if s == "insulated" => HeatBoundary::Insulated,
                Some(Value::Float(_) | Value::Integer(_)) => {
                    let h = Item {
                        table,
                        context: context.clone(),
                    }
                    .positive(key)?;
                    HeatBoundary::Transfer(h * 1e-6) // W m⁻² K⁻¹ to W mm⁻² K⁻¹
                }
                _ => {
                    return Err(error(format!(
                        "{context}: `{key}` is \"fixed\", \"insulated\" or a heat transfer coefficient (W m⁻² K⁻¹)"
                    )));
                }
            };
        }
    }
    Ok(Some(SampleThermal {
        sample: id.to_owned(),
        layers: out,
        top: sides[0],
        bottom: sides[1],
        side: sides[2],
    }))
}

/// Reads a layout file into a scene. `lookup` resolves material names.
pub fn parse(text: &str, lookup: &MaterialLookup) -> Result<Scene, FileError> {
    let library = Library::default();
    parse_with(
        text,
        &Lookups {
            materials: lookup,
            curves: &crate::coatings::no_curves,
            library: &library,
        },
    )
}

/// Where files find what they name: materials, R/T curves and library parts.
pub struct Lookups<'a> {
    /// Materials by name.
    pub materials: &'a MaterialLookup<'a>,
    /// Imported R/T curves by name.
    pub curves: &'a CurveLookup<'a>,
    /// The parts library.
    pub library: &'a Library,
}

/// A layout file, with materials, curves and library parts from `lookups`.
pub fn parse_with(text: &str, lookups: &Lookups) -> Result<Scene, FileError> {
    check_header(text)?;
    let mut root: Table = text
        .parse()
        .map_err(|e: toml::de::Error| error(format!("not valid TOML: {e}")))?;
    crate::mounts::expand(&mut root)?;
    crate::groups::expand(&mut root)?;
    // Embedded values (`[resolved]`) first, then the user's own data.
    let embedded = crate::resolved::read(&root)?;
    let merged_library = embedded.library(lookups.library)?;
    let merged_materials = |name: &str| {
        embedded
            .material(name)
            .map_or_else(|| (lookups.materials)(name), Ok)
    };
    let merged_curves = |name: &str| {
        embedded
            .curve(name)
            .map_or_else(|| (lookups.curves)(name), Ok)
    };
    let lookups = &Lookups {
        materials: &merged_materials,
        curves: &merged_curves,
        library: &merged_library,
    };
    let (lookup, library) = (lookups.materials, lookups.library);
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
            "resolved",
            "merit",
            "fluorophore",
            "tolerance",
            "tolerancing",
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
            check_keys(table, &["pitch", "columns", "rows", "corner"], "[table]")?;
            let pitch = Item {
                table,
                context: "[table]".into(),
            }
            .positive("pitch")?;
            board(table)?;
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
        dofs: Vec::new(),
        fluorophores: Vec::new(),
        thermal: Vec::new(),
        tolerances: Vec::new(),
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
        if reversed {
            let [x, _, z] = frame.axes();
            frame = Frame::new(frame.origin(), -z, x)
                .ok_or_else(|| error(format!("{}: degenerate frame", item.context)))?;
        }
        scene.sources.push(source(
            &item,
            &with(position, &["offset", "reversed"]),
            frame,
        )?);
        let target = Target::Source(item.text("id")?.to_owned());
        scene
            .dofs
            .extend(dofs(&item, &target, &frame, false, frame.origin())?);
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
                            "coatings",
                            "clear_aperture",
                            "dof",
                        ],
                    ),
                    &item.context,
                )?;
                let part = &with_element_coatings(item, part)?;
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
                        coatings: resolve_coatings(
                            &part.surface_coatings(),
                            lookups,
                            &item.context,
                        )?,
                    },
                });
            }
            "ideal_lens" => {
                check_keys(
                    item.table,
                    &with(position, &["id", "kind", "focal_length", "diameter", "dof"]),
                    &item.context,
                )?;
                scene.elements.push(Element {
                    id,
                    frame: placement.frame(item)?,
                    kind: ElementKind::IdealLens {
                        focal_length: item.number("focal_length")?,
                        diameter: item.optional_number("diameter", 25.4)?,
                    },
                });
            }
            "objective" => {
                check_keys(item.table, &with(position, OBJECTIVE_KEYS), &item.context)?;
                scene.elements.push(Element {
                    id,
                    frame: placement.frame(item)?,
                    kind: objective_kind(item, lookup)?,
                });
            }
            "sample" => {
                check_keys(item.table, &with(position, SAMPLE_KEYS), &item.context)?;
                if let Some(thermal) = sample_thermal(item, &id)? {
                    scene.thermal.push(thermal);
                }
                scene.elements.push(Element {
                    id,
                    frame: placement.frame(item)?,
                    kind: sample_kind(item, lookup)?,
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
                            "coatings",
                            "dof",
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
                let specs = element_coatings(item, surfaces.len())?
                    .unwrap_or_else(|| vec![CoatingSpec::Uncoated; surfaces.len()]);
                let coatings = resolve_coatings(&specs, lookups, &item.context)?;
                scene.elements.push(Element {
                    id,
                    frame: placement.frame(item)?,
                    kind: ElementKind::Lens {
                        surfaces,
                        thicknesses: item.numbers("thickness", None)?,
                        materials,
                        diameter: item.positive("diameter")?,
                        clear_aperture: item.positive("clear_aperture")?,
                        coatings,
                    },
                });
            }
            "mirror" => {
                let mut keys = with(
                    position,
                    if part.is_some() {
                        &["id", "kind", "library", "reversed", "clear_aperture", "dof"]
                    } else {
                        &["id", "kind", "radius", "diameter", "clear_aperture", "dof"]
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
        let frame = scene.elements[scene.elements.len() - 1].frame;
        let target = Target::Element(item.text("id")?.to_owned());
        scene
            .dofs
            .extend(dofs(item, &target, &frame, false, frame.origin())?);
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
            &with(
                position,
                &["id", "size", "pixels", "quantum_efficiency", "dof"],
            ),
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
            quantum_efficiency: quantum_efficiency(&item)?,
        });
        let frame = scene.detectors[scene.detectors.len() - 1].frame;
        let target = Target::Detector(item.text("id")?.to_owned());
        scene
            .dofs
            .extend(dofs(&item, &target, &frame, false, frame.origin())?);
    }
    scene.fluorophores = fluorophores(&root, &scene)?;
    check_dofs(&scene)?;
    scene.tolerances = crate::tolerance::tolerances(&root, &scene, &[])?;
    scene.placed().map_err(|e| error(e.to_string()))
}

/// Avogadro's constant (1/mol), exact in the SI since 2019.
const AVOGADRO: f64 = 6.022_140_76e23;

/// The `[[fluorophore]]` entries (layouts and sidecars alike), checked against the
/// scene's samples.
pub(crate) fn fluorophores(
    root: &Table,
    scene: &Scene,
) -> Result<Vec<optobench_scene::Fluorophore>, FileError> {
    let mut out: Vec<optobench_scene::Fluorophore> = Vec::new();
    for item in items(root, "fluorophore")? {
        check_keys(
            item.table,
            &[
                "id",
                "source",
                "quantum_yield",
                "lifetime_ns",
                "extinction_coefficient",
                "excitation",
                "one_photon_cross_section_cm2",
                "two_photon_cross_section_gm",
                "two_photon_action_cross_section_gm",
                "two_photon_spectrum",
                "two_photon_reference_um",
                "emission",
                "concentrations",
            ],
            &item.context,
        )?;
        let id = item.text("id")?.to_owned();
        if out.iter().any(|f| f.id == id) {
            return Err(error(format!(
                "{}: a second fluorophore `{id}`",
                item.context
            )));
        }
        let source = match item.table.get("source") {
            Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
            _ => {
                return Err(error(format!(
                    "{}: say where the values come from (`source`, text)",
                    item.context
                )));
            }
        };
        let quantum_yield = item.number("quantum_yield")?;
        if !(0.0..=1.0).contains(&quantum_yield) {
            return Err(error(format!(
                "{}: `quantum_yield` lies from 0 to 1",
                item.context
            )));
        }
        let lifetime = match item.table.get("lifetime_ns") {
            None => None,
            Some(_) => Some(item.positive("lifetime_ns")? * 1e-9),
        };
        // σ₁ = 1000 ln 10 ε / N_A cm² (ε in L mol⁻¹ cm⁻¹; Beer–Lambert in base 10), ×100 to mm².
        let per_extinction = 1000.0 * core::f64::consts::LN_10 / AVOGADRO * 100.0;
        let extinction = spectrum(&item, "extinction_coefficient")?;
        let excitation = spectrum(&item, "excitation")?;
        let cross_section = spectrum(&item, "one_photon_cross_section_cm2")?;
        let scaled = |s: Spectrum, by: f64| match s {
            Spectrum::Constant(v) => Spectrum::Constant(v * by),
            Spectrum::Table(rows) => {
                Spectrum::Table(rows.into_iter().map(|(w, v)| (w, v * by)).collect())
            }
        };
        let one_photon = match (extinction, excitation, cross_section) {
            (Some(_), _, Some(_)) => {
                return Err(error(format!(
                    "{}: give `extinction_coefficient` or `one_photon_cross_section_cm2`, not both",
                    item.context
                )));
            }
            (None, Some(_), _) => {
                return Err(error(format!(
                    "{}: `excitation` shapes `extinction_coefficient` (a number, at the peak), which is missing",
                    item.context
                )));
            }
            // The peak value spread over the relative excitation spectrum.
            (Some(Spectrum::Constant(peak)), Some(Spectrum::Table(rows)), None) => {
                let top = rows.iter().map(|r| r.1).fold(0.0, f64::max);
                if top <= 0.0 {
                    return Err(error(format!(
                        "{}: `excitation` is zero everywhere",
                        item.context
                    )));
                }
                Some(scaled(Spectrum::Table(rows), peak * per_extinction / top))
            }
            (Some(Spectrum::Table(_)), Some(_), None) => {
                return Err(error(format!(
                    "{}: with `excitation`, `extinction_coefficient` is one number (at the peak)",
                    item.context
                )));
            }
            (Some(e), _, None) => Some(scaled(e, per_extinction)),
            (None, None, Some(c)) => Some(scaled(c, 100.0)),
            (None, None, None) => None,
        };
        // 1 GM = 10⁻⁵⁰ cm⁴ s per photon (Xu, Zipfel, Shear, Williams and Webb, Proc. Natl.
        // Acad. Sci. USA 93, 10763 (1996), Fig. 1) = 10⁻⁴⁸ mm⁴ s. Most published spectra are
        // action cross-sections η σ₂ (ibid., Figs. 1 to 4), so σ₂ is that over η.
        let two_photon = match (
            spectrum(&item, "two_photon_cross_section_gm")?,
            spectrum(&item, "two_photon_action_cross_section_gm")?,
        ) {
            (Some(_), Some(_)) => {
                return Err(error(format!(
                    "{}: give `two_photon_cross_section_gm` or `two_photon_action_cross_section_gm`, not both",
                    item.context
                )));
            }
            (Some(absorption), None) => Some(scaled(absorption, 1e-48)),
            (None, Some(action)) => {
                if quantum_yield <= 0.0 {
                    return Err(error(format!(
                        "{}: an action cross-section (η σ₂) needs a quantum yield above 0",
                        item.context
                    )));
                }
                Some(scaled(action, 1e-48 / quantum_yield))
            }
            (None, None) => None,
        };
        // A relative two-photon spectrum (as FPbase gives it) shaped by a peak value.
        let two_photon = match (two_photon, spectrum(&item, "two_photon_spectrum")?) {
            (any, None) => {
                if item.table.contains_key("two_photon_reference_um") {
                    return Err(error(format!(
                        "{}: `two_photon_reference_um` belongs with `two_photon_spectrum`",
                        item.context
                    )));
                }
                any
            }
            (Some(Spectrum::Constant(peak)), Some(Spectrum::Table(rows))) => {
                // The number applies at `two_photon_reference_um`, else at the greatest value.
                let reference = match item.table.get("two_photon_reference_um") {
                    None => rows.iter().map(|r| r.1).fold(0.0, f64::max),
                    Some(_) => {
                        let at = item.positive("two_photon_reference_um")?;
                        Spectrum::Table(rows.clone()).at(at).map_err(|e| {
                            error(format!("{}: `two_photon_reference_um`: {e}", item.context))
                        })?
                    }
                };
                if reference <= 0.0 {
                    return Err(error(format!(
                        "{}: `two_photon_spectrum` is zero where its cross-section applies",
                        item.context
                    )));
                }
                Some(scaled(Spectrum::Table(rows), peak / reference))
            }
            (None, Some(_)) => {
                return Err(error(format!(
                    "{}: `two_photon_spectrum` is relative: give its peak as a number in \
                     `two_photon_cross_section_gm` (or `two_photon_action_cross_section_gm`)",
                    item.context
                )));
            }
            (Some(_), Some(_)) => {
                return Err(error(format!(
                    "{}: with `two_photon_spectrum`, the cross-section is one number (at the peak)",
                    item.context
                )));
            }
        };
        let emission = spectrum(&item, "emission")?;
        let mut concentrations = Vec::new();
        if let Some(value) = item.table.get("concentrations") {
            let list = value.as_array().ok_or_else(|| {
                error(format!(
                    "{}: `concentrations` is a list of {{ sample = …, layer = …, micromolar = … }}",
                    item.context
                ))
            })?;
            for entry in list {
                let context = format!("{} concentration", item.context);
                let table = entry.as_table().ok_or_else(|| {
                    error(format!(
                        "{context}: write {{ sample = …, layer = …, micromolar = … }}"
                    ))
                })?;
                check_keys(table, &["sample", "layer", "micromolar"], &context)?;
                let entry = Item { table, context };
                let sample = entry.text("sample")?.to_owned();
                let layer = entry.number("layer")?;
                let layers =
                    scene
                        .elements
                        .iter()
                        .find(|e| e.id == sample)
                        .map(|e| match &e.kind {
                            ElementKind::Sample { layers, .. } => Some(layers.len()),
                            _ => None,
                        });
                let count = match layers {
                    None => {
                        return Err(error(format!("{}: no element `{sample}`", entry.context)));
                    }
                    Some(None) => {
                        return Err(error(format!(
                            "{}: `{sample}` is not a sample",
                            entry.context
                        )));
                    }
                    Some(Some(n)) => n,
                };
                if !(layer >= 1.0 && layer.fract() == 0.0 && layer as usize <= count) {
                    return Err(error(format!(
                        "{}: `layer` is a whole number from 1 to {count} (the layers of `{sample}`)",
                        entry.context
                    )));
                }
                concentrations.push(optobench_scene::Concentration {
                    sample,
                    layer: layer as usize,
                    molar: entry.positive("micromolar")? * 1e-6,
                });
            }
        }
        out.push(optobench_scene::Fluorophore {
            id,
            one_photon,
            two_photon,
            emission,
            quantum_yield,
            lifetime,
            source,
            concentrations,
        });
    }
    Ok(out)
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
    fn a_breadboard_is_checked() {
        let table = |keys: &str| {
            read(&format!(
                "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"table\"\n\n[table]\npitch = 25.0\n{keys}"
            ))
        };
        table("").expect("no board");
        table("columns = 24\nrows = 18\n").expect("a board");
        table("columns = 24\nrows = 18\ncorner = [-2, 0]\n").expect("a board with its corner");
        for (keys, words) in [
            ("columns = 24\n", "together"),
            ("columns = 0\nrows = 3\n", "whole number ≥ 1"),
            ("columns = 2.5\nrows = 3\n", "whole number"),
            ("corner = [0, 0]\n", "goes with"),
            (
                "columns = 2\nrows = 3\ncorner = [0.5, 0]\n",
                "the hole [column, row]",
            ),
        ] {
            let e = table(keys).expect_err(keys);
            assert!(e.0.contains(words), "{keys}: {}", e.0);
        }
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
    fn degrees_of_freedom_are_read_strictly() {
        let lens = "[[element]]\nid = \"L1\"\nkind = \"lens\"\nat = 10\nsurfaces = [{ radius = 50 }, { radius = \"inf\" }]\nthickness = [4]\nmaterial = [1.5]\ndiameter = 20\nclear_aperture = 18\n";
        let scene = read(&format!(
            "{HEADER}{lens}dof = [{{ name = \"z\", kind = \"slide\", axis = \"z\", range = [-1, 2], value = 0.5 }}, \
             {{ name = \"tip\", kind = \"tilt\", axis = \"x\", pivot = [0, 0, 2], range = [-3, 3] }}]\n"
        ))
        .expect("valid");
        assert_eq!(scene.dofs.len(), 2);
        // Local axes become scene axes: the planar axis runs along +x, so the element's
        // own z is the scene's x.
        let Motion::Slide { axis } = scene.dofs[0].motion else {
            panic!("a slide")
        };
        assert!((axis - Vec3::X).length() < 1e-12);
        assert!((scene.dofs[0].value - 0.5).abs() < 1e-15);
        let Motion::Tilt { pivot, .. } = scene.dofs[1].motion else {
            panic!("a tilt")
        };
        assert!((pivot - Vec3::new(12.0, 0.0, 0.0)).length() < 1e-12);
        assert!((scene.dofs[1].range.expect("range").1 - 3f64.to_radians()).abs() < 1e-15);
        for (dof, message) in [
            (
                "{ name = \"a\", kind = \"spin\", axis = \"z\" }",
                "not known",
            ),
            ("{ name = \"a\", kind = \"slide\", axis = \"w\" }", "`axis`"),
            (
                "{ name = \"a\", kind = \"slide\", axis = \"z\", pivot = [0, 0, 0] }",
                "no `pivot`",
            ),
            (
                "{ name = \"a\", kind = \"slide\", axis = \"z\", range = [2, 1] }",
                "low to high",
            ),
            (
                "{ name = \"a\", kind = \"slide\", axis = \"z\", range = [0, 1], value = 2 }",
                "outside its range",
            ),
            (
                "{ name = \"a\", kind = \"slide\", axis = \"z\", follows = \"b\" }",
                "`b`",
            ),
            (
                "{ name = \"a\", kind = \"slide\", axis = \"z\" }, { name = \"a\", kind = \"slide\", axis = \"x\" }",
                "two degrees of freedom",
            ),
            (
                "{ name = \"a\", kind = \"slide\", axis = \"z\", rnage = [0, 1] }",
                "did you mean `range`",
            ),
        ] {
            let error = read(&format!("{HEADER}{lens}dof = [{dof}]\n")).expect_err("rejected");
            assert!(error.0.contains(message), "{dof}: {}", error.0);
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
