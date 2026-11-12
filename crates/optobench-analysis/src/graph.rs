//! Graphs as data (M10a): what is plotted, against what, in which units, with which
//! colour scale. Renderers (SVG and CSV in `optobench-io`, the interface) draw them; this
//! module draws nothing, so it builds for wasm32 like the rest of the engine.
//!
//! Values are kept in the engine's units (mm, radians, W, W/mm²); a renderer shows them
//! in a readable unit chosen from the range on screen ([`Axis::display`]), with ticks at
//! 1, 2 and 5 times a power of ten ([`ticks`]).
//!
//! Results can be compared: on shared axes ([`combine`]), against a reference such as a
//! closed form ([`Graph::with_reference`]), and as differences ([`difference`]).

use crate::colour::ColourScale;
use crate::{Focus, Psf};
use optobench_scene::{Scene, SceneTrace};
use optobench_trace::nonsequential::End;

/// What an axis measures, by its unit in the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Lengths (mm).
    Length,
    /// Angles (radians).
    Angle,
    /// Phase (radians, −π to π).
    Phase,
    /// Power (W).
    Power,
    /// Irradiance or n |E|² (W/mm²).
    Irradiance,
    /// Optical path in waves.
    Wave,
    /// Power per volume (W/mm³): absorbed power density, heat sources.
    PowerDensity,
    /// A plain number (a ratio, a count, a Strehl ratio).
    Number,
}

impl Unit {
    /// Its name in files.
    pub fn name(self) -> &'static str {
        match self {
            Unit::Length => "length",
            Unit::Angle => "angle",
            Unit::Phase => "phase",
            Unit::Power => "power",
            Unit::Irradiance => "irradiance",
            Unit::Wave => "wave",
            Unit::PowerDensity => "power-density",
            Unit::Number => "number",
        }
    }

    /// From its name.
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Unit::Length,
            Unit::Angle,
            Unit::Phase,
            Unit::Power,
            Unit::Irradiance,
            Unit::Wave,
            Unit::PowerDensity,
            Unit::Number,
        ]
        .into_iter()
        .find(|u| u.name() == name)
    }

    /// The engine's unit symbol (as in CSV headers).
    pub fn base_symbol(self) -> &'static str {
        match self {
            Unit::Length => "mm",
            Unit::Angle | Unit::Phase => "rad",
            Unit::Power => "W",
            Unit::Irradiance => "W/mm²",
            Unit::Wave => "waves",
            Unit::PowerDensity => "W/mm³",
            Unit::Number => "",
        }
    }
}

/// Linear or logarithmic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// Linear.
    Linear,
    /// Logarithmic (decades); only positive values are shown.
    Logarithmic,
}

/// An axis: what it measures and how it is scaled.
#[derive(Debug, Clone, PartialEq)]
pub struct Axis {
    /// The quantity, in words or a symbol (`z`, `height`, `n |E|²`).
    pub label: String,
    /// Its unit.
    pub unit: Unit,
    /// Its scale.
    pub scale: Scale,
}

/// How values on an axis are shown: `value × factor` in `symbol`, with a further
/// `× 10^exponent` written once in the axis title when the numbers would be long.
#[derive(Debug, Clone, PartialEq)]
pub struct Display {
    /// The unit shown (`µm`, `°`, `W/mm²`).
    pub symbol: String,
    /// Multiplies an engine value into the shown unit (before the exponent).
    pub factor: f64,
    /// Shown values are further divided by 10^exponent.
    pub exponent: i32,
}

impl Display {
    /// An engine value as shown.
    pub fn shown(&self, value: f64) -> f64 {
        value * self.factor / 10f64.powi(self.exponent)
    }

    /// A shown value back in the engine's unit.
    pub fn engine(&self, shown: f64) -> f64 {
        shown * 10f64.powi(self.exponent) / self.factor
    }

    /// The unit as written in an axis title: `µm`, `× 10⁶ W/mm²`.
    pub fn title_unit(&self) -> String {
        if self.exponent == 0 {
            self.symbol.clone()
        } else {
            format!("× 10{} {}", superscript(self.exponent), self.symbol)
                .trim_end()
                .to_owned()
        }
    }
}

fn superscript(n: i32) -> String {
    n.to_string()
        .chars()
        .map(|c| match c {
            '-' => '⁻',
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            _ => '⁹',
        })
        .collect()
}

impl Axis {
    /// An axis on a linear scale.
    pub fn new(label: impl Into<String>, unit: Unit) -> Self {
        Self {
            label: label.into(),
            unit,
            scale: Scale::Linear,
        }
    }

    /// How to show values spanning `low..high` (engine units): lengths in nm, µm, mm or
    /// m, angles in mrad or degrees, other quantities with a power of ten taken out when
    /// the numbers would need more than four digits.
    pub fn display(&self, low: f64, high: f64) -> Display {
        let size = low.abs().max(high.abs());
        let (symbol, factor) = match self.unit {
            Unit::Length => {
                if size < 1e-3 {
                    ("nm", 1e6)
                } else if size < 1.0 {
                    ("µm", 1e3)
                } else if size < 1e4 {
                    ("mm", 1.0)
                } else {
                    ("m", 1e-3)
                }
            }
            Unit::Angle => {
                // Under a degree in mrad (pointing), else degrees (scan angles).
                if size < core::f64::consts::PI / 180.0 {
                    ("mrad", 1e3)
                } else {
                    ("°", 180.0 / core::f64::consts::PI)
                }
            }
            Unit::Phase => ("rad", 1.0),
            Unit::Power => ("W", 1.0),
            Unit::Irradiance => ("W/mm²", 1.0),
            Unit::Wave => ("waves", 1.0),
            Unit::PowerDensity => ("W/mm³", 1.0),
            Unit::Number => ("", 1.0),
        };
        let shown = size * factor;
        let exponent = if self.scale == Scale::Logarithmic
            || matches!(
                self.unit,
                Unit::Length | Unit::Angle | Unit::Phase | Unit::Wave
            )
            || shown == 0.0
            || !shown.is_finite()
            || (1e-2..1e4).contains(&shown)
        {
            0
        } else {
            (shown.log10().floor() as i32 / 3) * 3
        };
        Display {
            symbol: symbol.to_owned(),
            factor,
            exponent,
        }
    }
}

/// Tick positions (shown units) for a shown range: 1, 2 or 5 times a power of ten, about
/// `wanted` of them, inside the range.
pub fn ticks(low: f64, high: f64, wanted: usize) -> Vec<f64> {
    if !(low.is_finite() && high.is_finite()) || high <= low {
        return vec![low];
    }
    let rough = (high - low) / wanted.max(2) as f64;
    let power = rough.log10().floor() as i32;
    let unit = 10f64.powi(power);
    let multiple = [1i64, 2, 5, 10]
        .into_iter()
        .find(|&m| m as f64 * unit >= rough)
        .unwrap_or(10);
    let step = multiple as f64 * unit;
    // k · multiple · 10^power, written as an integer over (or times) a power of ten so
    // the ticks are the decimal numbers they look like.
    let at = |k: i64| {
        let n = (k * multiple) as f64;
        if power < 0 {
            n / 10f64.powi(-power)
        } else {
            n * 10f64.powi(power)
        }
    };
    let first = (low / step).ceil() as i64;
    let last = (high / step).floor() as i64;
    (first..=last).map(at).collect()
}

/// A tick label: as few digits as the step needs.
pub fn tick_label(value: f64, step: f64) -> String {
    let decimals = if step > 0.0 {
        (-step.log10().floor()).max(0.0) as usize
    } else {
        0
    };
    format!("{value:.decimals$}")
}

/// How a series is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// A line through the points in order.
    Line,
    /// A dot at each point (spot diagrams, crossings).
    Dots,
}

/// One series of points.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    /// Its name in the legend.
    pub label: String,
    /// (x, y) in the axes' engine units.
    pub points: Vec<(f64, f64)>,
    /// How it is drawn.
    pub mark: Mark,
    /// A reference (a closed form, a published value): drawn dashed in neutral ink, not
    /// given a series colour.
    pub reference: bool,
}

/// A map: values on a grid of cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Map {
    /// Cells across (along x) and up (along y).
    pub columns: usize,
    /// Rows.
    pub rows: usize,
    /// The x range the cells cover, edge to edge (engine units).
    pub x: (f64, f64),
    /// The y range.
    pub y: (f64, f64),
    /// Row by row from low y, each from low x; non-finite values are blank.
    pub values: Vec<f64>,
    /// What the values are.
    pub value: Axis,
    /// How they are coloured.
    pub colours: ColourScale,
}

impl Map {
    /// The range the colour scale spans: as [`Map::range`], or on a logarithmic value
    /// axis six whole decades up to the decade above the peak.
    pub fn colour_range(&self) -> (f64, f64) {
        let (low, high) = self.range();
        if self.value.scale == Scale::Logarithmic && high > 0.0 {
            let top = 10f64.powf(high.log10().ceil());
            (top * 1e-6, top)
        } else {
            (low, high)
        }
    }

    /// Where a value falls on the colour scale, 0 to 1 (renderers share this, so files and
    /// the interface colour alike).
    pub fn level(&self, value: f64) -> f64 {
        let (low, high) = self.colour_range();
        let t = if self.value.scale == Scale::Logarithmic && high > 0.0 {
            (value.max(low).log10() - low.log10()) / (high.log10() - low.log10())
        } else {
            (value - low) / (high - low)
        };
        t.clamp(0.0, 1.0)
    }

    /// The least and greatest finite value; for a diverging scale, symmetric about zero;
    /// for a cyclic one, −π to π.
    pub fn range(&self) -> (f64, f64) {
        if self.colours == ColourScale::Cyclic {
            return (-core::f64::consts::PI, core::f64::consts::PI);
        }
        let (low, high) = self
            .values
            .iter()
            .filter(|v| v.is_finite())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| {
                (a.min(v), b.max(v))
            });
        if !low.is_finite() {
            return (0.0, 1.0);
        }
        match self.colours {
            ColourScale::Diverging => {
                let m = low.abs().max(high.abs()).max(f64::MIN_POSITIVE);
                (-m, m)
            }
            _ if high > low => (low, high),
            _ => (low, low + 1.0),
        }
    }
}

/// What a graph shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Plot {
    /// Lines and dots.
    Series(Vec<Series>),
    /// A colour map.
    Map(Map),
}

/// A named place along x, drawn as a hairline across the plot.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// Where (engine units).
    pub at: f64,
    /// What is there.
    pub label: String,
}

/// A graph.
#[derive(Debug, Clone, PartialEq)]
pub struct Graph {
    /// Its title: what it shows, of what.
    pub title: String,
    /// The horizontal axis.
    pub x: Axis,
    /// The vertical axis.
    pub y: Axis,
    /// What is drawn.
    pub plot: Plot,
    /// One unit on x as long as one on y (spot diagrams, maps of a plane).
    pub equal_aspect: bool,
    /// Places along x worth naming (the elements along a beam's path).
    pub markers: Vec<Marker>,
    /// Lines of text shown with it (key numbers).
    pub notes: Vec<String>,
}

impl Graph {
    /// The graph with a reference series added (a closed form, a published curve).
    pub fn with_reference(mut self, label: impl Into<String>, points: Vec<(f64, f64)>) -> Self {
        if let Plot::Series(list) = &mut self.plot {
            list.push(Series {
                label: label.into(),
                points,
                mark: Mark::Line,
                reference: true,
            });
        }
        self
    }

    /// The x and y ranges of its data (engine units).
    pub fn bounds(&self) -> ((f64, f64), (f64, f64)) {
        match &self.plot {
            Plot::Map(m) => (m.x, m.y),
            Plot::Series(list) => {
                let mut x = (f64::INFINITY, f64::NEG_INFINITY);
                let mut y = (f64::INFINITY, f64::NEG_INFINITY);
                for s in list {
                    for &(a, b) in &s.points {
                        if !(a.is_finite() && b.is_finite()) {
                            continue;
                        }
                        if self.y.scale == Scale::Logarithmic && b <= 0.0 {
                            continue;
                        }
                        x = (x.0.min(a), x.1.max(a));
                        y = (y.0.min(b), y.1.max(b));
                    }
                }
                // At least a readable span (1 nm, 1 µrad, or a millionth of the values),
                // and a little room round the data so edge points are not cut.
                let widen = |r: (f64, f64), unit: Unit, log: bool| {
                    if !r.0.is_finite() {
                        return (0.0, 1.0);
                    }
                    if log {
                        return if r.1 > r.0 {
                            r
                        } else {
                            (r.0 / 10.0, r.1 * 10.0)
                        };
                    }
                    let middle = 0.5 * (r.0 + r.1);
                    let least = match unit {
                        Unit::Length => 1e-6,
                        Unit::Angle | Unit::Phase => 1e-6,
                        _ => (middle.abs() * 1e-6).max(1e-300),
                    };
                    let span = (r.1 - r.0).max(least);
                    let pad = 0.04 * span;
                    (middle - span / 2.0 - pad, middle + span / 2.0 + pad)
                };
                (
                    widen(x, self.x.unit, self.x.scale == Scale::Logarithmic),
                    widen(y, self.y.unit, self.y.scale == Scale::Logarithmic),
                )
            }
        }
    }
}

/// Several graphs of the same quantities on shared axes, each named (a design, a run, a
/// method). Series keep their kind; labels become `name: label`, or just the name for a
/// graph with one series. Maps cannot be combined (compare them by difference).
pub fn combine(graphs: &[(String, Graph)]) -> Result<Graph, String> {
    let (_, first) = graphs.first().ok_or("nothing to combine")?;
    let mut series = Vec::new();
    for (name, g) in graphs {
        if g.x.unit != first.x.unit || g.y.unit != first.y.unit {
            return Err(format!(
                "`{name}` plots {} against {}, not {} against {}",
                g.y.unit.name(),
                g.x.unit.name(),
                first.y.unit.name(),
                first.x.unit.name()
            ));
        }
        let Plot::Series(list) = &g.plot else {
            return Err(format!(
                "`{name}` is a map; maps are compared by difference"
            ));
        };
        let own: usize = list.iter().filter(|s| !s.reference).count();
        for s in list {
            let label = if s.reference || own > 1 {
                format!("{name}: {}", s.label)
            } else {
                name.clone()
            };
            series.push(Series { label, ..s.clone() });
        }
    }
    let names: Vec<&str> = graphs.iter().map(|(n, _)| n.as_str()).collect();
    Ok(Graph {
        title: format!("{} — {}", first.title, names.join(", ")),
        plot: Plot::Series(series),
        notes: Vec::new(),
        ..first.clone()
    })
}

/// y at `x` on a series sorted by x, by linear interpolation; `None` outside it.
fn interpolate(points: &[(f64, f64)], x: f64) -> Option<f64> {
    let i = points.partition_point(|p| p.0 < x);
    if i < points.len() && points[i].0.total_cmp(&x).is_eq() {
        return Some(points[i].1);
    }
    if i == 0 || i == points.len() {
        return None;
    }
    let (a, b) = (points[i - 1], points[i]);
    Some(a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0))
}

/// `a − b`: for series, each line of `a` minus the line of `b` with the same label (or
/// the only one), at `a`'s x where `b` covers it; for maps on the same grid, cell by
/// cell, on a diverging scale.
pub fn difference(a: &Graph, b: &Graph) -> Result<Graph, String> {
    if a.x.unit != b.x.unit || a.y.unit != b.y.unit {
        return Err("the graphs plot different quantities".into());
    }
    match (&a.plot, &b.plot) {
        (Plot::Map(ma), Plot::Map(mb)) => {
            let close = |p: (f64, f64), q: (f64, f64)| {
                let tolerance = 1e-9 * (p.1 - p.0).abs().max(1e-12);
                (p.0 - q.0).abs() < tolerance && (p.1 - q.1).abs() < tolerance
            };
            if ma.columns != mb.columns
                || ma.rows != mb.rows
                || !close(ma.x, mb.x)
                || !close(ma.y, mb.y)
                || ma.value.unit != mb.value.unit
            {
                return Err("the maps are not on the same grid".into());
            }
            let values = ma
                .values
                .iter()
                .zip(&mb.values)
                .map(|(p, q)| p - q)
                .collect();
            Ok(Graph {
                title: format!("Difference: {}", a.title),
                plot: Plot::Map(Map {
                    values,
                    value: Axis {
                        label: format!("Δ {}", ma.value.label),
                        ..ma.value.clone()
                    },
                    colours: ColourScale::Diverging,
                    ..ma.clone()
                }),
                notes: Vec::new(),
                ..a.clone()
            })
        }
        (Plot::Series(sa), Plot::Series(sb)) => {
            let own = |list: &[Series]| -> Vec<Series> {
                list.iter().filter(|s| !s.reference).cloned().collect()
            };
            let (sa, sb) = (own(sa), own(sb));
            let mut out = Vec::new();
            for s in &sa {
                let partner = sb
                    .iter()
                    .find(|t| t.label == s.label)
                    .or_else(|| (sa.len() == 1 && sb.len() == 1).then(|| &sb[0]));
                let Some(t) = partner else {
                    continue;
                };
                let mut sorted = t.points.clone();
                sorted.sort_by(|p, q| p.0.total_cmp(&q.0));
                let points: Vec<(f64, f64)> = s
                    .points
                    .iter()
                    .filter_map(|&(x, y)| interpolate(&sorted, x).map(|v| (x, y - v)))
                    .collect();
                out.push(Series {
                    label: s.label.clone(),
                    points,
                    mark: s.mark,
                    reference: false,
                });
            }
            if out.is_empty() {
                return Err("no series of the one graph matches a series of the other".into());
            }
            Ok(Graph {
                title: format!("Difference: {}", a.title),
                y: Axis {
                    label: format!("Δ {}", a.y.label),
                    scale: Scale::Linear,
                    ..a.y.clone()
                },
                plot: Plot::Series(out),
                notes: Vec::new(),
                ..a.clone()
            })
        }
        _ => Err("a map and a line graph cannot be subtracted".into()),
    }
}

fn intensity_axis() -> Axis {
    Axis::new("n |E|²", Unit::Irradiance)
}

fn psf_notes(psf: &Psf) -> Vec<String> {
    let um = |v: f64| format!("{:.4} µm", v * 1e3);
    let mut notes = vec![
        format!(
            "peak {:.4e} W/mm² at z = {}",
            psf.peak_intensity,
            um(psf.peak.z)
        ),
        format!("Strehl ratio {:.4}", psf.strehl),
    ];
    let widths: Vec<String> = [("x", psf.fwhm.0), ("y", psf.fwhm.1), ("axial", psf.fwhm.2)]
        .iter()
        .filter_map(|(k, w)| w.map(|w| format!("{k} {}", um(w))))
        .collect();
    if !widths.is_empty() {
        notes.push(format!("FWHM {}", widths.join(", ")));
    }
    notes
}

/// The PSF along its focus line.
pub fn psf_axial(psf: &Psf) -> Graph {
    Graph {
        title: "PSF along the focus line".into(),
        x: Axis::new("z", Unit::Length),
        y: intensity_axis(),
        plot: Plot::Series(vec![Series {
            label: "PSF".into(),
            points: psf.axial.clone(),
            mark: Mark::Line,
            reference: false,
        }]),
        equal_aspect: false,
        markers: Vec::new(),
        notes: psf_notes(psf),
    }
}

/// The PSF across its peak, along the detector's x and y.
pub fn psf_lateral(psf: &Psf) -> Graph {
    let series = |label: &str, points: &[(f64, f64)]| Series {
        label: label.into(),
        points: points.to_vec(),
        mark: Mark::Line,
        reference: false,
    };
    Graph {
        title: "PSF across the peak".into(),
        x: Axis::new("offset", Unit::Length),
        y: intensity_axis(),
        plot: Plot::Series(vec![
            series("along x", &psf.lateral.0),
            series("along y", &psf.lateral.1),
        ]),
        equal_aspect: false,
        markers: Vec::new(),
        notes: psf_notes(psf),
    }
}

/// The PSF's x–z section (when it was computed).
pub fn psf_section(psf: &Psf) -> Option<Graph> {
    let s = psf.section.as_ref()?;
    Some(Graph {
        title: "PSF section through the peak (x–z)".into(),
        x: Axis::new("x", Unit::Length),
        y: Axis::new("z", Unit::Length),
        plot: Plot::Map(Map {
            columns: s.columns,
            rows: s.rows,
            x: s.x,
            y: s.z,
            values: s.values.clone(),
            value: intensity_axis(),
            colours: ColourScale::Sequential,
        }),
        equal_aspect: false,
        markers: Vec::new(),
        notes: psf_notes(psf),
    })
}

/// The wavefront error over the beam, referred to the PSF peak.
pub fn psf_wavefront(psf: &Psf) -> Graph {
    let w = &psf.wavefront;
    Graph {
        title: "Wavefront error at the focus, over the beam".into(),
        x: Axis::new("beam x at the source", Unit::Length),
        y: Axis::new("beam y at the source", Unit::Length),
        plot: Plot::Map(Map {
            columns: w.columns,
            rows: w.rows,
            x: w.x,
            y: w.y,
            values: w.values.clone(),
            value: Axis::new("OPD", Unit::Wave),
            colours: ColourScale::Diverging,
        }),
        equal_aspect: true,
        markers: Vec::new(),
        notes: vec![
            format!(
                "RMS {:.4} waves, peak to valley {:.4} waves",
                w.rms, w.peak_to_valley
            ),
            format!("Strehl ratio {:.4}", psf.strehl),
        ],
    }
}

/// A Gaussian beam's 1/e² radius along its chief ray, element by element: the free
/// stretch before each plane drawn from the beam there, w(z) = w₀ √(1 + (z / z_R)²) with
/// z_R = π n w₀² / (M² λ) (Siegman, Lasers, §17.1 and §17.6), and the planes marked.
/// `wavelength` in µm. Inside elements (between their faces) nothing is drawn.
pub fn gaussian_path(beams: &[crate::BeamAt], wavelength: f64, m_squared: f64) -> Graph {
    let lambda = wavelength * 1e-3;
    let stretch = |b: &crate::BeamAt, axis: usize| -> Vec<(f64, f64)> {
        let (w0, to_waist) = if axis == 0 {
            (b.waist.0, b.to_waist.0)
        } else {
            (b.waist.1, b.to_waist.1)
        };
        let rayleigh = core::f64::consts::PI * b.index * w0 * w0 / (m_squared * lambda);
        let steps = 400;
        let mut points: Vec<(f64, f64)> = (0..=steps)
            .map(|k| {
                let s = -b.free + b.free * k as f64 / steps as f64;
                // Distance past the waist at s: the waist is `to_waist` beyond the plane.
                let z = s - to_waist;
                let w = if rayleigh > 0.0 {
                    w0 * (1.0 + (z / rayleigh).powi(2)).sqrt()
                } else {
                    f64::NAN
                };
                (b.distance + s, w)
            })
            .collect();
        points.push((f64::NAN, f64::NAN)); // a break before the next stretch
        points
    };
    let round = beams.iter().all(|b| {
        let close = |p: (f64, f64)| (p.0 - p.1).abs() <= 1e-9 * p.0.abs().max(p.1.abs());
        close(b.waist) && close(b.to_waist)
    });
    let series = |label: &str, axis: usize| Series {
        label: label.into(),
        points: beams.iter().flat_map(|b| stretch(b, axis)).collect(),
        mark: Mark::Line,
        reference: false,
    };
    let plot = if round {
        vec![series("1/e² radius", 0)]
    } else {
        vec![series("1/e² radius, x", 0), series("1/e² radius, y", 1)]
    };
    let notes = beams
        .iter()
        .filter_map(|b| {
            b.fill.map(|f| {
                format!(
                    "`{}`: pupil fill {:.3} (w / (f · NA)), w = {:.4} mm",
                    b.id,
                    f,
                    0.5 * (b.radius.0 + b.radius.1)
                )
            })
        })
        .collect();
    Graph {
        title: "Gaussian beam along its path".into(),
        x: Axis::new("distance along the chief ray", Unit::Length),
        y: Axis::new("radius", Unit::Length),
        plot: Plot::Series(plot),
        equal_aspect: false,
        markers: beams
            .iter()
            .map(|b| Marker {
                at: b.distance,
                label: b.id.clone(),
            })
            .collect(),
        notes,
    }
}

/// The fluence rate along a sample's axis (the innermost ring): direct, diffuse and both.
pub fn fluence_depth(light: &crate::sample::SampleLight) -> Graph {
    let g = &light.grid;
    let line = |label: &str, values: &dyn Fn(usize) -> f64| Series {
        label: label.into(),
        points: (0..g.slices)
            .map(|j| (g.z_at(j), values(g.index(0, j))))
            .collect(),
        mark: Mark::Line,
        reference: false,
    };
    let mut notes = vec![format!(
        "scattered {:.4e} W, absorbed from the direct beam {:.4e} W",
        light.scattered, light.direct_absorbed
    )];
    if let Some(d) = &light.diffuse_light {
        notes.push(format!(
            "diffuse: absorbed {:.4e} W, out through the top {:.4e}, bottom {:.4e}, side {:.4e} W",
            d.absorbed, d.top, d.bottom, d.side
        ));
    }
    Graph {
        title: format!("Fluence rate along the axis of `{}`", light.id),
        x: Axis::new("depth", Unit::Length),
        y: Axis::new("fluence rate", Unit::Irradiance),
        plot: Plot::Series(vec![
            line("direct", &|c| light.direct[c]),
            line("diffuse", &|c| light.diffuse[c]),
            line("total", &|c| light.direct[c] + light.diffuse[c]),
        ]),
        equal_aspect: false,
        markers: Vec::new(),
        notes,
    }
}

/// The absorbed power density in a sample as an x–z map (the rings mirrored about the
/// axis).
pub fn absorption_map(light: &crate::sample::SampleLight) -> Graph {
    let g = &light.grid;
    let columns = 2 * g.rings;
    let mut values = Vec::with_capacity(columns * g.slices);
    // Rows from low y: the deepest slice first.
    for j in (0..g.slices).rev() {
        for c in 0..columns {
            let i = if c < g.rings {
                g.rings - 1 - c
            } else {
                c - g.rings
            };
            values.push(light.absorbed[g.index(i, j)]);
        }
    }
    let peak = light.absorbed.iter().copied().fold(0.0, f64::max);
    Graph {
        title: format!("Absorbed power density in `{}`", light.id),
        x: Axis::new("x", Unit::Length),
        // Depth downwards: y is minus the depth, so the beam comes in at the top.
        y: Axis::new("−depth", Unit::Length),
        plot: Plot::Map(Map {
            columns,
            rows: g.slices,
            x: (-g.radius, g.radius),
            y: (-g.z.1, -g.z.0),
            values,
            // Over decades, from the beam's entry to the diffuse glow: a log scale.
            value: Axis {
                label: "absorbed".into(),
                unit: Unit::PowerDensity,
                scale: Scale::Logarithmic,
            },
            colours: ColourScale::Sequential,
        }),
        equal_aspect: true,
        markers: Vec::new(),
        notes: vec![format!(
            "peak {peak:.4e} W/mm³ (a log scale of six decades); depth from {} to {} mm",
            g.z.0, g.z.1
        )],
    }
}

/// Where rays cross the axis near a detector, against their launch height.
pub fn focus_crossings(focus: &Focus) -> Graph {
    Graph {
        title: "Axial crossings by ray height".into(),
        x: Axis::new("launch height", Unit::Length),
        y: Axis::new("crossing z", Unit::Length),
        plot: Plot::Series(vec![Series {
            label: "rays".into(),
            // The axial ray runs along the axis and crosses it nowhere in particular.
            points: focus
                .crossings
                .iter()
                .filter(|c| c.height > 1e-9)
                .map(|c| (c.height, c.z))
                .collect(),
            mark: Mark::Dots,
            reference: false,
        }]),
        equal_aspect: false,
        markers: Vec::new(),
        notes: vec![
            format!("paraxial focus {:.6} mm", focus.paraxial),
            format!("marginal focus {:.6} mm", focus.marginal),
            format!(
                "best focus {:.6} mm, RMS radius {:.4} µm",
                focus.best,
                focus.best_rms * 1e3
            ),
        ],
    }
}

/// Where the direct light lands on detector `detector` (its local x, y): a spot
/// diagram, one series per source.
pub fn spot(scene: &Scene, trace: &SceneTrace, detector: usize) -> Option<Graph> {
    let d = scene.detectors.get(detector)?;
    let mut per_source: Vec<Vec<(f64, f64)>> = vec![Vec::new(); scene.sources.len().max(1)];
    for record in &trace.records {
        if record.generation > 0 {
            continue;
        }
        let End::Detected { detector: hit, .. } = record.end else {
            continue;
        };
        if hit != detector {
            continue;
        }
        let Some(p) = record.detector_point else {
            continue;
        };
        // The source it came from: the nearest source origin to the launch point.
        let source = scene
            .sources
            .iter()
            .enumerate()
            .min_by(|a, b| {
                let da = (a.1.frame.origin() - record.launched.position).length();
                let db = (b.1.frame.origin() - record.launched.position).length();
                da.total_cmp(&db)
            })
            .map_or(0, |(i, _)| i);
        per_source[source].push(p);
    }
    let series: Vec<Series> = per_source
        .into_iter()
        .enumerate()
        .filter(|(_, p)| !p.is_empty())
        .map(|(i, points)| Series {
            label: scene
                .sources
                .get(i)
                .map_or_else(|| "rays".into(), |s| s.id.clone()),
            points,
            mark: Mark::Dots,
            reference: false,
        })
        .collect();
    let result = trace.detectors.get(detector)?;
    Some(Graph {
        title: format!("Spot diagram on `{}`", d.id),
        x: Axis::new("x", Unit::Length),
        y: Axis::new("y", Unit::Length),
        plot: Plot::Series(series),
        equal_aspect: true,
        markers: Vec::new(),
        notes: vec![
            format!("{} rays, {:.6} W", result.rays, result.power),
            format!("RMS radius {:.4} µm", result.direct_rms_radius * 1e3),
        ],
    })
}

/// The irradiance on detector `detector`, pixel by pixel.
pub fn irradiance(scene: &Scene, trace: &SceneTrace, detector: usize) -> Option<Graph> {
    let d = scene.detectors.get(detector)?;
    let result = trace.detectors.get(detector)?;
    let (columns, rows) = d.pixels;
    let (width, height) = d.size;
    Some(Graph {
        title: format!("Irradiance on `{}`", d.id),
        x: Axis::new("x", Unit::Length),
        y: Axis::new("y", Unit::Length),
        plot: Plot::Map(Map {
            columns,
            rows,
            x: (-width / 2.0, width / 2.0),
            y: (-height / 2.0, height / 2.0),
            // W/m² to W/mm².
            values: result.irradiance.iter().map(|v| v * 1e-6).collect(),
            value: Axis::new("irradiance", Unit::Irradiance),
            colours: ColourScale::Sequential,
        }),
        equal_aspect: true,
        markers: Vec::new(),
        notes: vec![format!("{:.6} W on {} rays", result.power, result.rays)],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(label: &str, points: Vec<(f64, f64)>) -> Series {
        Series {
            label: label.into(),
            points,
            mark: Mark::Line,
            reference: false,
        }
    }

    fn graph(series: Vec<Series>) -> Graph {
        Graph {
            title: "t".into(),
            x: Axis::new("z", Unit::Length),
            y: Axis::new("I", Unit::Irradiance),
            plot: Plot::Series(series),
            equal_aspect: false,
            markers: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn ticks_fall_on_one_two_five() {
        assert_eq!(ticks(0.0, 1.0, 5), vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
        assert_eq!(ticks(-3.0, 7.0, 5), vec![-2.0, 0.0, 2.0, 4.0, 6.0]);
        assert_eq!(ticks(0.13, 0.61, 4), vec![0.2, 0.4, 0.6]);
        assert_eq!(ticks(0.13, 0.61, 8), vec![0.2, 0.3, 0.4, 0.5, 0.6]);
        assert_eq!(tick_label(0.30000000000000004, 0.1), "0.3");
        assert_eq!(tick_label(250.0, 50.0), "250");
    }

    #[test]
    fn lengths_and_large_numbers_are_shown_readably() {
        let z = Axis::new("z", Unit::Length);
        assert_eq!(z.display(-0.002, 0.003).symbol, "µm");
        assert_eq!(z.display(-2e-4, 3e-4).symbol, "nm");
        assert_eq!(z.display(0.0, 120.0).symbol, "mm");
        let i = Axis::new("I", Unit::Irradiance);
        let d = i.display(0.0, 1.45e7);
        assert_eq!(d.exponent, 6);
        assert_eq!(d.title_unit(), "× 10⁶ W/mm²");
        assert!((d.shown(1.45e7) - 14.5).abs() < 1e-12);
        assert!((d.engine(14.5) - 1.45e7).abs() < 1e-6);
        assert_eq!(i.display(0.0, 25.0).exponent, 0);
    }

    #[test]
    fn differences_subtract_matching_series_where_both_have_values() {
        let a = graph(vec![line("PSF", vec![(0.0, 1.0), (1.0, 3.0), (2.0, 5.0)])]);
        let b = graph(vec![line("PSF", vec![(0.5, 1.0), (2.5, 1.0)])]);
        let d = difference(&a, &b).expect("difference");
        let Plot::Series(s) = &d.plot else {
            panic!("series");
        };
        assert_eq!(s[0].points, vec![(1.0, 2.0), (2.0, 4.0)]);
        assert_eq!(d.y.label, "Δ I");
    }

    #[test]
    fn combining_names_each_graph_and_refuses_other_quantities() {
        let a = graph(vec![line("PSF", vec![(0.0, 1.0)])]);
        let b = graph(vec![line("PSF", vec![(0.0, 2.0)])]).with_reference("Airy", vec![]);
        let c = combine(&[("before".into(), a.clone()), ("after".into(), b)]).expect("combined");
        let Plot::Series(s) = &c.plot else {
            panic!("series");
        };
        let labels: Vec<&str> = s.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["before", "after", "after: Airy"]);
        let mut other = a.clone();
        other.y.unit = Unit::Power;
        assert!(combine(&[("a".into(), a), ("b".into(), other)]).is_err());
    }

    #[test]
    fn maps_subtract_on_the_same_grid_only() {
        let map = |values: Vec<f64>, x: (f64, f64)| Graph {
            title: "m".into(),
            x: Axis::new("x", Unit::Length),
            y: Axis::new("y", Unit::Length),
            plot: Plot::Map(Map {
                columns: 2,
                rows: 1,
                x,
                y: (0.0, 1.0),
                values,
                value: Axis::new("I", Unit::Irradiance),
                colours: ColourScale::Sequential,
            }),
            equal_aspect: true,
            markers: Vec::new(),
            notes: Vec::new(),
        };
        let d = difference(
            &map(vec![3.0, 1.0], (0.0, 2.0)),
            &map(vec![1.0, 2.0], (0.0, 2.0)),
        )
        .expect("same grid");
        let Plot::Map(m) = &d.plot else {
            panic!("map");
        };
        assert_eq!(m.values, vec![2.0, -1.0]);
        assert_eq!(m.colours, ColourScale::Diverging);
        assert_eq!(m.range(), (-2.0, 2.0));
        assert!(
            difference(
                &map(vec![3.0, 1.0], (0.0, 2.0)),
                &map(vec![1.0, 2.0], (0.0, 3.0))
            )
            .is_err()
        );
    }
}
