//! Graphs (`optobench_analysis::graph`) written out: SVG for reports and the command
//! line, CSV of the numbers behind them, and JSON that reads back, so a result kept from
//! one run can be compared with the next.
//!
//! The SVG is deterministic (the same graph gives the same bytes) and drawn on the light
//! surface: hairline grid, recessive axes, 2 px lines, dots of 8 px (smaller and
//! translucent when there are many), reference series dashed in neutral ink, a legend
//! from two series up, and a colour bar beside a map. Each series and map carries a
//! `<title>`, so a viewer shows its name on hover.

use optobench_analysis::colour::{self, ColourScale, Rgb, Surface};
use optobench_analysis::graph::{
    Axis, Display, Graph, Map, Mark, Plot, Scale, Series, Unit, tick_label, ticks,
};
use serde_json::{Value, json};

const WIDTH: f64 = 720.0;
const PLOT_HEIGHT: f64 = 360.0;
const LEFT: f64 = 76.0;
const RIGHT: f64 = 28.0;
const BAR: f64 = 96.0; // room for a map's colour bar
const FONT: &str = "system-ui, -apple-system, Segoe UI, sans-serif";

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn number(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// An axis as drawn: its display, its shown range and ticks.
struct Drawn {
    display: Display,
    low: f64,
    high: f64,
    ticks: Vec<f64>,
    step: f64,
    log: bool,
}

impl Drawn {
    fn new(axis: &Axis, range: (f64, f64), wanted: usize) -> Self {
        if axis.scale == Scale::Logarithmic {
            let low = range.0.max(f64::MIN_POSITIVE).log10().floor();
            let high = range.1.max(f64::MIN_POSITIVE).log10().ceil().max(low + 1.0);
            return Self {
                display: axis.display(range.0, range.1),
                low,
                high,
                ticks: (low as i32..=high as i32).map(f64::from).collect(),
                step: 1.0,
                log: true,
            };
        }
        let display = axis.display(range.0, range.1);
        let (low, high) = (display.shown(range.0), display.shown(range.1));
        let found = ticks(low, high, wanted);
        let step = if found.len() > 1 {
            found[1] - found[0]
        } else {
            (high - low).abs().max(1e-300)
        };
        Self {
            display,
            low,
            high,
            ticks: found,
            step,
            log: false,
        }
    }

    /// An engine value as a fraction of the axis (0 at low, 1 at high).
    fn fraction(&self, value: f64) -> Option<f64> {
        let v = if self.log {
            if value <= 0.0 {
                return None;
            }
            value.log10()
        } else {
            self.display.shown(value)
        };
        Some((v - self.low) / (self.high - self.low))
    }

    fn label(&self, tick: f64) -> String {
        if self.log {
            let e = tick as i32;
            format!("1e{e}")
        } else {
            tick_label(tick, self.step)
        }
    }

    fn title(&self, axis: &Axis) -> String {
        let unit = if self.log {
            self.display.symbol.clone()
        } else {
            self.display.title_unit()
        };
        if unit.is_empty() {
            axis.label.clone()
        } else {
            format!("{} ({unit})", axis.label)
        }
    }
}

/// The graph as an SVG document.
pub fn svg(graph: &Graph) -> String {
    let ink = colour::ink(Surface::Light);
    let hex = |c: Rgb| c.to_hex();
    let ((xr, yr), map) = (
        graph.bounds(),
        match &graph.plot {
            Plot::Map(m) => Some(m),
            Plot::Series(_) => None,
        },
    );
    let legend: Vec<(String, Option<Rgb>, bool)> = match &graph.plot {
        Plot::Series(list) if list.len() >= 2 => {
            let mut k = 0;
            list.iter()
                .map(|s| {
                    if s.reference {
                        (s.label.clone(), None, true)
                    } else {
                        k += 1;
                        (
                            s.label.clone(),
                            colour::series(k - 1, Surface::Light),
                            false,
                        )
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    };
    let top = 64.0 + if legend.is_empty() { 0.0 } else { 22.0 };
    let right = RIGHT + if map.is_some() { BAR } else { 0.0 };
    let mut plot_width = WIDTH - LEFT - right;
    let mut plot_height = PLOT_HEIGHT;
    let x = Drawn::new(&graph.x, xr, 7);
    let y = Drawn::new(&graph.y, yr, 6);
    if graph.equal_aspect && !x.log && !y.log {
        // The same engine length per pixel on both axes.
        let span_x = (xr.1 - xr.0).abs();
        let span_y = (yr.1 - yr.0).abs();
        if span_x > 0.0 && span_y > 0.0 {
            let scale = (plot_width / span_x).min(plot_height / span_y);
            plot_width = span_x * scale;
            plot_height = span_y * scale;
        }
    }
    let notes_height = 18.0 * graph.notes.len() as f64;
    let height = top + plot_height + 56.0 + notes_height + 12.0;
    let px = |v: f64| x.fraction(v).map(|f| LEFT + f * plot_width);
    let py = |v: f64| y.fraction(v).map(|f| top + (1.0 - f) * plot_height);

    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {} {}' width='{}' height='{}' role='img' aria-label='{}' font-family='{FONT}' font-size='12'>\n",
        number(WIDTH),
        number(height),
        number(WIDTH),
        number(height),
        escape(&graph.title)
    ));
    out.push_str(&format!(
        "<rect width='{}' height='{}' fill='{}'/>\n",
        number(WIDTH),
        number(height),
        hex(ink.surface)
    ));
    out.push_str(&format!(
        "<text x='{}' y='30' font-size='15' font-weight='600' fill='{}'>{}</text>\n",
        number(LEFT),
        hex(ink.primary),
        escape(&graph.title)
    ));
    // Legend.
    let mut lx = LEFT;
    for (label, c, reference) in &legend {
        let ly = 54.0;
        if *reference {
            out.push_str(&format!(
                "<line x1='{}' y1='{}' x2='{}' y2='{}' stroke='{}' stroke-width='1.5' stroke-dasharray='6 4'/>\n",
                number(lx),
                number(ly - 4.0),
                number(lx + 18.0),
                number(ly - 4.0),
                hex(ink.secondary)
            ));
        } else if let Some(c) = c {
            out.push_str(&format!(
                "<rect x='{}' y='{}' width='12' height='12' rx='3' fill='{}'/>\n",
                number(lx + 3.0),
                number(ly - 10.0),
                hex(*c)
            ));
        }
        out.push_str(&format!(
            "<text x='{}' y='{}' fill='{}'>{}</text>\n",
            number(lx + 24.0),
            number(ly),
            hex(ink.secondary),
            escape(label)
        ));
        lx += 24.0 + 7.0 * label.chars().count() as f64 + 20.0;
    }
    // Grid and ticks.
    let bottom = top + plot_height;
    for &t in &x.ticks {
        let f = (t - x.low) / (x.high - x.low);
        let gx = LEFT + f * plot_width;
        out.push_str(&format!(
            "<line x1='{0}' y1='{1}' x2='{0}' y2='{2}' stroke='{3}' stroke-width='1'/>\n",
            number(gx),
            number(top),
            number(bottom),
            hex(ink.grid)
        ));
        out.push_str(&format!(
            "<text x='{}' y='{}' text-anchor='middle' font-size='11' fill='{}' style='font-variant-numeric: tabular-nums'>{}</text>\n",
            number(gx),
            number(bottom + 16.0),
            hex(ink.muted),
            escape(&x.label(t))
        ));
    }
    for &t in &y.ticks {
        let f = (t - y.low) / (y.high - y.low);
        let gy = top + (1.0 - f) * plot_height;
        out.push_str(&format!(
            "<line x1='{1}' y1='{0}' x2='{2}' y2='{0}' stroke='{3}' stroke-width='1'/>\n",
            number(gy),
            number(LEFT),
            number(LEFT + plot_width),
            hex(ink.grid)
        ));
        out.push_str(&format!(
            "<text x='{}' y='{}' text-anchor='end' font-size='11' fill='{}' style='font-variant-numeric: tabular-nums'>{}</text>\n",
            number(LEFT - 8.0),
            number(gy + 4.0),
            hex(ink.muted),
            escape(&y.label(t))
        ));
    }
    out.push_str(&format!(
        "<defs><clipPath id='plot'><rect x='{}' y='{}' width='{}' height='{}'/></clipPath></defs>\n",
        number(LEFT),
        number(top),
        number(plot_width),
        number(plot_height)
    ));
    // Data.
    out.push_str("<g clip-path='url(#plot)'>\n");
    match &graph.plot {
        Plot::Map(m) => out.push_str(&map_cells(m, LEFT, top, plot_width, plot_height)),
        Plot::Series(list) => {
            let mut k = 0;
            for s in list {
                let c = if s.reference {
                    ink.secondary
                } else {
                    k += 1;
                    colour::series(k - 1, Surface::Light).unwrap_or(ink.secondary)
                };
                out.push_str(&format!("<g><title>{}</title>\n", escape(&s.label)));
                match s.mark {
                    Mark::Line => {
                        let mut path = String::new();
                        let mut pen_down = false;
                        for &(a, b) in &s.points {
                            match (px(a), py(b)) {
                                (Some(u), Some(v)) if u.is_finite() && v.is_finite() => {
                                    path.push_str(if pen_down { "L" } else { "M" });
                                    path.push_str(&format!("{} {}", number(u), number(v)));
                                    pen_down = true;
                                }
                                _ => pen_down = false,
                            }
                        }
                        let dash = if s.reference {
                            " stroke-dasharray='6 4' stroke-width='1.5'"
                        } else {
                            " stroke-width='2'"
                        };
                        out.push_str(&format!(
                            "<path d='{path}' fill='none' stroke='{}'{dash} stroke-linejoin='round' stroke-linecap='round'/>\n",
                            hex(c)
                        ));
                    }
                    Mark::Dots => {
                        let dense = s.points.len() > 300;
                        let (radius, opacity) = if dense { (1.6, 0.55) } else { (4.0, 1.0) };
                        for &(a, b) in &s.points {
                            if let (Some(u), Some(v)) = (px(a), py(b)) {
                                out.push_str(&format!(
                                    "<circle cx='{}' cy='{}' r='{}' fill='{}' fill-opacity='{}'/>\n",
                                    number(u),
                                    number(v),
                                    number(radius),
                                    hex(c),
                                    number(opacity)
                                ));
                            }
                        }
                    }
                }
                out.push_str("</g>\n");
            }
        }
    }
    out.push_str("</g>\n");
    // Axes.
    out.push_str(&format!(
        "<path d='M{} {}V{}H{}' fill='none' stroke='{}' stroke-width='1'/>\n",
        number(LEFT),
        number(top),
        number(bottom),
        number(LEFT + plot_width),
        hex(ink.axis)
    ));
    out.push_str(&format!(
        "<text x='{}' y='{}' text-anchor='middle' fill='{}'>{}</text>\n",
        number(LEFT + plot_width / 2.0),
        number(bottom + 38.0),
        hex(ink.secondary),
        escape(&x.title(&graph.x))
    ));
    out.push_str(&format!(
        "<text transform='translate({} {}) rotate(-90)' text-anchor='middle' fill='{}'>{}</text>\n",
        number(18.0),
        number(top + plot_height / 2.0),
        hex(ink.secondary),
        escape(&y.title(&graph.y))
    ));
    if let Some(m) = map {
        out.push_str(&colour_bar(m, LEFT + plot_width + 24.0, top, plot_height));
    }
    for (i, note) in graph.notes.iter().enumerate() {
        out.push_str(&format!(
            "<text x='{}' y='{}' font-size='11.5' fill='{}'>{}</text>\n",
            number(LEFT),
            number(bottom + 62.0 + 18.0 * i as f64),
            hex(ink.secondary),
            escape(note)
        ));
    }
    out.push_str("</svg>\n");
    out
}

/// At most this many cells are drawn along each side of a map.
const MAP_CELLS: usize = 240;

fn map_cells(m: &Map, left: f64, top: f64, width: f64, height: f64) -> String {
    let (low, high) = m.range();
    let columns = m.columns.min(MAP_CELLS);
    let rows = m.rows.min(MAP_CELLS);
    let (w, h) = (width / columns as f64, height / rows as f64);
    let mut out = format!(
        "<g shape-rendering='crispEdges'><title>{}</title>\n",
        escape(&m.value.label)
    );
    for j in 0..rows {
        let source_row = j * m.rows / rows;
        for i in 0..columns {
            let source_column = i * m.columns / columns;
            let v = m.values[source_row * m.columns + source_column];
            if !v.is_finite() {
                continue;
            }
            let t = if m.value.scale == Scale::Logarithmic {
                let floor = high * 1e-6;
                ((v.max(floor)).log10() - floor.log10()) / (high.log10() - floor.log10())
            } else {
                (v - low) / (high - low)
            };
            let c = colour::scale(m.colours, t, Surface::Light);
            // Row j counts from low y, drawn from the bottom up.
            out.push_str(&format!(
                "<rect x='{}' y='{}' width='{}' height='{}' fill='{}'/>\n",
                number(left + i as f64 * w),
                number(top + height - (j + 1) as f64 * h),
                number(w + 0.5),
                number(h + 0.5),
                c.to_hex()
            ));
        }
    }
    out.push_str("</g>\n");
    out
}

fn colour_bar(m: &Map, left: f64, top: f64, height: f64) -> String {
    let ink = colour::ink(Surface::Light);
    let (low, high) = m.range();
    let steps = 64;
    let mut out = String::from("<g shape-rendering='crispEdges'>\n");
    for k in 0..steps {
        let t = (k as f64 + 0.5) / steps as f64;
        let c = colour::scale(m.colours, t, Surface::Light);
        out.push_str(&format!(
            "<rect x='{}' y='{}' width='14' height='{}' fill='{}'/>\n",
            number(left),
            number(top + height * (1.0 - (k + 1) as f64 / steps as f64)),
            number(height / steps as f64 + 0.5),
            c.to_hex()
        ));
    }
    out.push_str("</g>\n");
    let axis = Drawn::new(&m.value, (low, high), 5);
    for &t in &axis.ticks {
        let f = (t - axis.low) / (axis.high - axis.low);
        let ty = top + (1.0 - f) * height;
        out.push_str(&format!(
            "<text x='{}' y='{}' font-size='11' fill='{}' style='font-variant-numeric: tabular-nums'>{}</text>\n",
            number(left + 20.0),
            number(ty + 4.0),
            hex_of(ink.muted),
            escape(&axis.label(t))
        ));
    }
    out.push_str(&format!(
        "<text x='{}' y='{}' font-size='11' fill='{}'>{}</text>\n",
        number(left),
        number(top - 10.0),
        hex_of(ink.secondary),
        escape(&axis.title(&m.value))
    ));
    out
}

fn hex_of(c: Rgb) -> String {
    c.to_hex()
}

fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

fn column(axis: &Axis) -> String {
    let unit = axis.unit.base_symbol();
    if unit.is_empty() {
        axis.label.clone()
    } else {
        format!("{} ({unit})", axis.label)
    }
}

/// The numbers behind a graph, in the engine's units: for series one row per point
/// (`series, x, y`), for a map one row per cell centre (`x, y, value`).
pub fn csv(graph: &Graph) -> String {
    let mut out = String::new();
    match &graph.plot {
        Plot::Series(list) => {
            out.push_str(&format!(
                "series,{},{}\n",
                csv_field(&column(&graph.x)),
                csv_field(&column(&graph.y))
            ));
            for s in list {
                for (x, y) in &s.points {
                    out.push_str(&format!("{},{x},{y}\n", csv_field(&s.label)));
                }
            }
        }
        Plot::Map(m) => {
            out.push_str(&format!(
                "{},{},{}\n",
                csv_field(&column(&graph.x)),
                csv_field(&column(&graph.y)),
                csv_field(&column(&m.value))
            ));
            for j in 0..m.rows {
                let y = m.y.0 + (m.y.1 - m.y.0) * (j as f64 + 0.5) / m.rows as f64;
                for i in 0..m.columns {
                    let x = m.x.0 + (m.x.1 - m.x.0) * (i as f64 + 0.5) / m.columns as f64;
                    out.push_str(&format!("{x},{y},{}\n", m.values[j * m.columns + i]));
                }
            }
        }
    }
    out
}

fn finite(v: f64) -> Value {
    if v.is_finite() { json!(v) } else { Value::Null }
}

fn axis_json(a: &Axis) -> Value {
    json!({
        "label": a.label,
        "unit": a.unit.name(),
        "scale": match a.scale { Scale::Linear => "linear", Scale::Logarithmic => "logarithmic" },
    })
}

/// The graph as JSON (reads back with [`from_json`]).
pub fn to_json(graph: &Graph) -> Value {
    let mut v = json!({
        "title": graph.title,
        "x": axis_json(&graph.x),
        "y": axis_json(&graph.y),
        "equal_aspect": graph.equal_aspect,
        "notes": graph.notes,
    });
    match &graph.plot {
        Plot::Series(list) => {
            v["series"] = json!(list.iter().map(|s| json!({
                "label": s.label,
                "mark": match s.mark { Mark::Line => "line", Mark::Dots => "dots" },
                "reference": s.reference,
                "points": s.points.iter().map(|(a, b)| json!([finite(*a), finite(*b)])).collect::<Vec<_>>(),
            })).collect::<Vec<_>>());
        }
        Plot::Map(m) => {
            v["map"] = json!({
                "columns": m.columns,
                "rows": m.rows,
                "x": [m.x.0, m.x.1],
                "y": [m.y.0, m.y.1],
                "value": axis_json(&m.value),
                "colours": m.colours.name(),
                "values": m.values.iter().map(|x| finite(*x)).collect::<Vec<_>>(),
            });
        }
    }
    v
}

fn read_axis(v: &Value, key: &str) -> Result<Axis, String> {
    let a = &v[key];
    let text = |k: &str| {
        a[k].as_str()
            .ok_or_else(|| format!("`{key}.{k}` must be a string"))
    };
    Ok(Axis {
        label: text("label")?.to_owned(),
        unit: Unit::from_name(text("unit")?)
            .ok_or_else(|| format!("`{key}.unit` is not a known unit"))?,
        scale: match text("scale")? {
            "linear" => Scale::Linear,
            "logarithmic" => Scale::Logarithmic,
            other => return Err(format!("`{key}.scale` `{other}` is not known")),
        },
    })
}

fn read_number(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

fn read_pair(v: &Value, key: &str) -> Result<(f64, f64), String> {
    match v[key].as_array().map(Vec::as_slice) {
        Some([a, b]) => Ok((read_number(a), read_number(b))),
        _ => Err(format!("`{key}` must be [from, to]")),
    }
}

/// A graph read from JSON written by [`to_json`].
pub fn from_json(v: &Value) -> Result<Graph, String> {
    let title = v["title"]
        .as_str()
        .ok_or("`title` must be a string")?
        .to_owned();
    let notes = v["notes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|n| n.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let plot = if let Some(list) = v["series"].as_array() {
        Plot::Series(
            list.iter()
                .map(|s| {
                    Ok(Series {
                        label: s["label"]
                            .as_str()
                            .ok_or("a series needs a `label`")?
                            .to_owned(),
                        mark: match s["mark"].as_str() {
                            Some("dots") => Mark::Dots,
                            _ => Mark::Line,
                        },
                        reference: s["reference"].as_bool().unwrap_or(false),
                        points: s["points"]
                            .as_array()
                            .ok_or("a series needs `points`")?
                            .iter()
                            .map(|p| (read_number(&p[0]), read_number(&p[1])))
                            .collect(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        )
    } else if v["map"].is_object() {
        let m = &v["map"];
        let columns = m["columns"]
            .as_u64()
            .ok_or("`map.columns` must be a count")? as usize;
        let rows = m["rows"].as_u64().ok_or("`map.rows` must be a count")? as usize;
        let values: Vec<f64> = m["values"]
            .as_array()
            .ok_or("`map.values` must be a list")?
            .iter()
            .map(read_number)
            .collect();
        if values.len() != columns * rows {
            return Err("`map.values` must hold columns × rows numbers".into());
        }
        Plot::Map(Map {
            columns,
            rows,
            x: read_pair(m, "x")?,
            y: read_pair(m, "y")?,
            values,
            value: read_axis(m, "value")?,
            colours: m["colours"]
                .as_str()
                .and_then(ColourScale::from_name)
                .ok_or("`map.colours` is not a known colour scale")?,
        })
    } else {
        return Err("a graph has `series` or a `map`".into());
    };
    Ok(Graph {
        title,
        x: read_axis(v, "x")?,
        y: read_axis(v, "y")?,
        plot,
        equal_aspect: v["equal_aspect"].as_bool().unwrap_or(false),
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Graph {
        Graph {
            title: "PSF along the focus line".into(),
            x: Axis::new("z", Unit::Length),
            y: Axis::new("n |E|²", Unit::Irradiance),
            plot: Plot::Series(vec![Series {
                label: "PSF".into(),
                points: vec![(-0.001, 0.25), (0.0, 1.0e7), (0.001, 0.5)],
                mark: Mark::Line,
                reference: false,
            }]),
            equal_aspect: false,
            notes: vec!["peak 1e7 W/mm²".into()],
        }
        .with_reference("Airy, \"closed form\"", vec![(-0.001, 0.0), (0.001, 0.0)])
    }

    #[test]
    fn json_reads_back_exactly() {
        let g = sample();
        assert_eq!(from_json(&to_json(&g)).expect("reads"), g);
        let map = Graph {
            title: "m".into(),
            x: Axis::new("x", Unit::Length),
            y: Axis::new("y", Unit::Length),
            plot: Plot::Map(Map {
                columns: 2,
                rows: 2,
                x: (-1.0, 1.0),
                y: (-1.0, 1.0),
                values: vec![1.0, 2.0, 3.0, 0.1 + 0.2],
                value: Axis::new("I", Unit::Irradiance),
                colours: ColourScale::Sequential,
            }),
            equal_aspect: true,
            notes: Vec::new(),
        };
        assert_eq!(from_json(&to_json(&map)).expect("reads"), map);
    }

    #[test]
    fn csv_holds_every_number_in_full_and_quotes_labels() {
        let text = csv(&sample());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "series,z (mm),n |E|² (W/mm²)");
        assert_eq!(lines[1], "PSF,-0.001,0.25");
        assert_eq!(lines[2], "PSF,0,10000000");
        assert_eq!(lines[4], "\"Airy, \"\"closed form\"\"\",-0.001,0");
    }

    #[test]
    fn svg_is_deterministic_and_shows_units_legend_and_reference() {
        let g = sample();
        let a = svg(&g);
        assert_eq!(a, svg(&g));
        assert!(a.starts_with("<svg xmlns='http://www.w3.org/2000/svg'"));
        assert!(a.contains("z (µm)"), "length in µm for a µm range");
        assert!(a.contains("n |E|² (× 10⁶ W/mm²)"));
        assert!(
            a.contains("stroke-dasharray='6 4'"),
            "the reference is dashed"
        );
        assert!(a.contains("#2a78d6"), "the first series colour");
        assert!(
            a.contains("Airy, &quot;closed form&quot;"),
            "legend with escaped text"
        );
        assert!(a.trim_end().ends_with("</svg>"));
    }
}
