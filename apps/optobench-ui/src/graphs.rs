//! Graphs in the window (M10a2): the engine's graphs (`optobench_analysis::graph`)
//! drawn with egui's painter on the dark surface, the same data and colours as the SVG
//! files (`optobench_io::graph`), with:
//!
//! - a readout of the nearest point (series) or the cell (maps) under the pointer;
//! - zoom about the pointer with the scroll wheel, pan by dragging, fit by double-click;
//! - on maps, a cursor drawing the row and column profiles through the cell it is on;
//! - a cursor linked across graphs whose x axes measure the same quantity.

use egui::{
    Align2, Color32, ColorImage, FontId, Pos2, Rect, Sense, Shape, Stroke, TextureHandle,
    TextureOptions, Ui, Vec2, pos2, vec2,
};
use optobench_analysis::colour::{self, Rgb, Surface};
use optobench_analysis::graph::{
    Axis, Display, Graph, Map, Mark, Plot, Scale, Unit, tick_label, ticks,
};

fn colour(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// What one graph on screen keeps between frames.
#[derive(Default)]
pub struct GraphView {
    /// The ranges shown, in axis space (log10 of values on logarithmic axes); `None`
    /// fits the data.
    pub range: Option<((f64, f64), (f64, f64))>,
    /// A map's picture, with a key of the map it shows.
    texture: Option<(u64, TextureHandle)>,
}

/// A value in axis space: log10 on logarithmic axes.
fn to_axis(axis: &Axis, v: f64) -> f64 {
    match axis.scale {
        Scale::Linear => v,
        Scale::Logarithmic if v > 0.0 => v.log10(),
        Scale::Logarithmic => f64::NAN,
    }
}

fn from_axis(axis: &Axis, a: f64) -> f64 {
    match axis.scale {
        Scale::Linear => a,
        Scale::Logarithmic => 10f64.powf(a),
    }
}

/// The plot's frame: screen rectangle and the axis-space ranges on it.
struct Frame {
    plot: Rect,
    x: (f64, f64),
    y: (f64, f64),
}

impl Frame {
    fn screen(&self, ax: f64, ay: f64) -> Pos2 {
        let fx = (ax - self.x.0) / (self.x.1 - self.x.0);
        let fy = (ay - self.y.0) / (self.y.1 - self.y.0);
        pos2(
            self.plot.left() + fx as f32 * self.plot.width(),
            self.plot.bottom() - fy as f32 * self.plot.height(),
        )
    }

    fn axis_space(&self, p: Pos2) -> (f64, f64) {
        let fx = f64::from((p.x - self.plot.left()) / self.plot.width());
        let fy = f64::from((self.plot.bottom() - p.y) / self.plot.height());
        (
            self.x.0 + fx * (self.x.1 - self.x.0),
            self.y.0 + fy * (self.y.1 - self.y.0),
        )
    }
}

/// A value as shown on an axis, with its unit.
fn shown(display: &Display, v: f64) -> String {
    let s = display.shown(v);
    let unit = display.title_unit();
    let number = if s != 0.0 && (s.abs() < 1e-3 || s.abs() >= 1e5) {
        format!("{s:.4e}")
    } else {
        format!("{s:.4}")
    };
    if unit.is_empty() {
        number
    } else {
        format!("{number} {unit}")
    }
}

fn key(map: &Map) -> u64 {
    // Enough to tell maps apart: size and a few sampled values.
    let mut k = (map.columns as u64) << 32 | map.rows as u64;
    for (i, v) in map.values.iter().enumerate().step_by(97) {
        k = k.rotate_left(7) ^ v.to_bits() ^ i as u64;
    }
    k
}

fn picture(map: &Map) -> ColorImage {
    let mut pixels = Vec::with_capacity(map.columns * map.rows * 4);
    // Image rows run down the screen; map rows run up from low y.
    for r in (0..map.rows).rev() {
        for c in 0..map.columns {
            let v = map.values[r * map.columns + c];
            if v.is_finite() {
                let rgb = colour::scale(map.colours, map.level(v), Surface::Dark);
                pixels.extend_from_slice(&[rgb.0, rgb.1, rgb.2, 255]);
            } else {
                pixels.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    ColorImage::from_rgba_unmultiplied([map.columns, map.rows], &pixels)
}

/// Draws `graph` in a rectangle of `size`. `linked` is the cursor of another graph (its
/// x quantity and value), drawn here when this graph's x measures the same; returns this
/// graph's cursor when the pointer is on it.
pub fn show(
    ui: &mut Ui,
    graph: &Graph,
    view: &mut GraphView,
    linked: Option<(Unit, f64)>,
    size: Vec2,
) -> Option<(Unit, f64)> {
    let ink = colour::ink(Surface::Dark);
    let map = match &graph.plot {
        Plot::Map(m) => Some(m),
        Plot::Series(_) => None,
    };
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, colour(ink.surface));
    let small = FontId::proportional(10.5);
    painter.text(
        rect.left_top() + vec2(8.0, 6.0),
        Align2::LEFT_TOP,
        &graph.title,
        FontId::proportional(12.5),
        colour(ink.primary),
    );
    let right = if map.is_some() { 64.0 } else { 10.0 };
    let plot = Rect::from_min_max(
        rect.left_top() + vec2(58.0, 42.0),
        rect.right_bottom() - vec2(right, 34.0),
    );
    if plot.width() < 40.0 || plot.height() < 30.0 {
        return None;
    }

    // Ranges: the view's, or the data's.
    let ((bx, by), fit) = (graph.bounds(), view.range.is_none());
    let mut range = view.range.unwrap_or((
        (to_axis(&graph.x, bx.0), to_axis(&graph.x, bx.1)),
        (to_axis(&graph.y, by.0), to_axis(&graph.y, by.1)),
    ));
    let clean = |r: (f64, f64)| {
        if r.0.is_finite() && r.1.is_finite() && r.1 > r.0 {
            r
        } else {
            (0.0, 1.0)
        }
    };
    range = (clean(range.0), clean(range.1));
    if fit && graph.equal_aspect {
        // The same length per pixel both ways, about the data's centre.
        let per_x = (range.0.1 - range.0.0) / f64::from(plot.width());
        let per_y = (range.1.1 - range.1.0) / f64::from(plot.height());
        let per = per_x.max(per_y);
        let grow = |r: (f64, f64), pixels: f32| {
            let middle = 0.5 * (r.0 + r.1);
            let half = 0.5 * per * f64::from(pixels);
            (middle - half, middle + half)
        };
        range = (grow(range.0, plot.width()), grow(range.1, plot.height()));
    }
    let frame = Frame {
        plot,
        x: range.0,
        y: range.1,
    };

    // Zoom, pan, fit.
    if let Some(pointer) = response.hover_pos().filter(|p| plot.contains(*p)) {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let factor = (-f64::from(scroll) * 0.003).exp();
            let (ax, ay) = frame.axis_space(pointer);
            let scale =
                |r: (f64, f64), at: f64| (at + (r.0 - at) * factor, at + (r.1 - at) * factor);
            view.range = Some((scale(range.0, ax), scale(range.1, ay)));
        }
    }
    if response.dragged() {
        let d = response.drag_delta();
        let dx = -f64::from(d.x / plot.width()) * (range.0.1 - range.0.0);
        let dy = f64::from(d.y / plot.height()) * (range.1.1 - range.1.0);
        view.range = Some((
            (range.0.0 + dx, range.0.1 + dx),
            (range.1.0 + dy, range.1.1 + dy),
        ));
    }
    if response.double_clicked() {
        view.range = None;
    }

    // Axes, grid and ticks.
    let axis_ticks = |axis: &Axis, r: (f64, f64), wanted: usize| -> (Display, Vec<(f64, String)>) {
        let display = axis.display(from_axis(axis, r.0), from_axis(axis, r.1));
        if axis.scale == Scale::Logarithmic {
            let (low, high) = (r.0.ceil() as i32, r.1.floor() as i32);
            let every = ((high - low) / wanted.max(1) as i32).max(1);
            let list = (low..=high)
                .filter(|e| (e - low) % every == 0)
                .map(|e| (f64::from(e), format!("1e{e}")))
                .collect();
            return (display, list);
        }
        let (low, high) = (display.shown(r.0), display.shown(r.1));
        let found = ticks(low, high, wanted);
        let step = if found.len() > 1 {
            found[1] - found[0]
        } else {
            1.0
        };
        let list = found
            .into_iter()
            .map(|t| (display.engine(t), tick_label(t, step)))
            .collect();
        (display, list)
    };
    let (x_display, x_ticks) = axis_ticks(&graph.x, frame.x, 6);
    let (y_display, y_ticks) = axis_ticks(&graph.y, frame.y, 5);
    let grid = Stroke::new(1.0, colour(ink.grid));
    for (t, label) in &x_ticks {
        let p = frame.screen(*t, frame.y.0);
        painter.line_segment([pos2(p.x, plot.top()), pos2(p.x, plot.bottom())], grid);
        painter.text(
            pos2(p.x, plot.bottom() + 3.0),
            Align2::CENTER_TOP,
            label,
            small.clone(),
            colour(ink.muted),
        );
    }
    for (t, label) in &y_ticks {
        let p = frame.screen(frame.x.0, *t);
        painter.line_segment([pos2(plot.left(), p.y), pos2(plot.right(), p.y)], grid);
        painter.text(
            pos2(plot.left() - 4.0, p.y),
            Align2::RIGHT_CENTER,
            label,
            small.clone(),
            colour(ink.muted),
        );
    }
    let title = |axis: &Axis, display: &Display| {
        let unit = display.title_unit();
        if unit.is_empty() {
            axis.label.clone()
        } else {
            format!("{} ({unit})", axis.label)
        }
    };
    painter.text(
        pos2(plot.center().x, rect.bottom() - 4.0),
        Align2::CENTER_BOTTOM,
        title(&graph.x, &x_display),
        small.clone(),
        colour(ink.secondary),
    );
    painter.text(
        pos2(rect.left() + 8.0, plot.top() - 5.0),
        Align2::LEFT_BOTTOM,
        title(&graph.y, &y_display),
        small.clone(),
        colour(ink.secondary),
    );

    // Data, clipped to the plot.
    let clipped = painter.with_clip_rect(plot);
    match &graph.plot {
        Plot::Map(m) => {
            let k = key(m);
            if view.texture.as_ref().is_none_or(|(old, _)| *old != k) {
                let handle = ui.ctx().load_texture(
                    format!("graph-{k}"),
                    picture(m),
                    TextureOptions::NEAREST,
                );
                view.texture = Some((k, handle));
            }
            if let Some((_, texture)) = &view.texture {
                let corner =
                    |x: f64, y: f64| frame.screen(to_axis(&graph.x, x), to_axis(&graph.y, y));
                let image = Rect::from_two_pos(corner(m.x.0, m.y.1), corner(m.x.1, m.y.0));
                clipped.image(
                    texture.id(),
                    image,
                    Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
        Plot::Series(list) => {
            let mut k = 0;
            for s in list {
                let c = if s.reference {
                    colour(ink.secondary)
                } else {
                    k += 1;
                    colour::series(k - 1, Surface::Dark).map_or(colour(ink.secondary), colour)
                };
                let screen: Vec<Option<Pos2>> = s
                    .points
                    .iter()
                    .map(|&(x, y)| {
                        let (ax, ay) = (to_axis(&graph.x, x), to_axis(&graph.y, y));
                        (ax.is_finite() && ay.is_finite()).then(|| frame.screen(ax, ay))
                    })
                    .collect();
                match s.mark {
                    Mark::Line => {
                        let width = if s.reference { 1.5 } else { 2.0 };
                        for run in screen.split(Option::is_none) {
                            let points: Vec<Pos2> = run.iter().flatten().copied().collect();
                            if points.len() < 2 {
                                continue;
                            }
                            if s.reference {
                                clipped.extend(Shape::dashed_line(
                                    &points,
                                    Stroke::new(width, c),
                                    6.0,
                                    4.0,
                                ));
                            } else {
                                clipped.add(Shape::line(points, Stroke::new(width, c)));
                            }
                        }
                    }
                    Mark::Dots => {
                        let dense = s.points.len() > 300;
                        let (radius, c) = if dense {
                            (1.6, c.gamma_multiply(0.6))
                        } else {
                            (3.5, c)
                        };
                        for p in screen.iter().flatten() {
                            clipped.circle_filled(*p, radius, c);
                        }
                    }
                }
            }
        }
    }
    for m in &graph.markers {
        let p = frame.screen(to_axis(&graph.x, m.at), frame.y.0);
        if (plot.left()..=plot.right()).contains(&p.x) {
            clipped.line_segment(
                [pos2(p.x, plot.top()), pos2(p.x, plot.bottom())],
                Stroke::new(1.0, colour(ink.axis)),
            );
            clipped.text(
                pos2(p.x + 3.0, plot.top() + 2.0),
                Align2::LEFT_TOP,
                &m.label,
                small.clone(),
                colour(ink.muted),
            );
        }
    }
    painter.line_segment(
        [plot.left_top(), plot.left_bottom()],
        Stroke::new(1.0, colour(ink.axis)),
    );
    painter.line_segment(
        [plot.left_bottom(), plot.right_bottom()],
        Stroke::new(1.0, colour(ink.axis)),
    );

    // A map's colour bar.
    if let Some(m) = map {
        let bar = Rect::from_min_max(
            pos2(plot.right() + 10.0, plot.top()),
            pos2(plot.right() + 22.0, plot.bottom()),
        );
        let steps = 48;
        for i in 0..steps {
            let t = (f64::from(i) + 0.5) / f64::from(steps);
            let y0 = bar.bottom() - bar.height() * (i + 1) as f32 / steps as f32;
            let cell = Rect::from_min_max(
                pos2(bar.left(), y0),
                pos2(bar.right(), y0 + bar.height() / steps as f32 + 0.5),
            );
            painter.rect_filled(
                cell,
                0.0,
                colour(colour::scale(m.colours, t, Surface::Dark)),
            );
        }
        let (low, high) = m.colour_range();
        let display = m.value.display(low, high);
        let log = m.value.scale == Scale::Logarithmic;
        let middle = if log {
            (low * high).sqrt()
        } else {
            0.5 * (low + high)
        };
        for (t, at) in [
            (low, bar.bottom()),
            (middle, bar.center().y),
            (high, bar.top()),
        ] {
            let text = if log {
                format!("{t:.0e}")
            } else {
                format!("{:.3}", display.shown(t))
            };
            painter.text(
                pos2(bar.right() + 3.0, at),
                Align2::LEFT_CENTER,
                text,
                small.clone(),
                colour(ink.muted),
            );
        }
        painter.text(
            pos2(rect.right() - 6.0, plot.top() - 5.0),
            Align2::RIGHT_BOTTOM,
            display.title_unit(),
            small.clone(),
            colour(ink.secondary),
        );
    }

    // A linked cursor from another graph.
    if let Some((_, x)) = linked.filter(|(u, _)| *u == graph.x.unit) {
        let p = frame.screen(to_axis(&graph.x, x), frame.y.0);
        if (plot.left()..=plot.right()).contains(&p.x) {
            painter.line_segment(
                [pos2(p.x, plot.top()), pos2(p.x, plot.bottom())],
                Stroke::new(1.0, colour(ink.secondary)),
            );
        }
    }

    // The readout under the pointer.
    let pointer = response.hover_pos().filter(|p| plot.contains(*p))?;
    let (ax, ay) = frame.axis_space(pointer);
    let (x, y) = (from_axis(&graph.x, ax), from_axis(&graph.y, ay));
    let mut lines: Vec<String> = Vec::new();
    match &graph.plot {
        Plot::Series(list) => {
            // The nearest point on screen, within 18 px.
            let mut best: Option<(f32, usize, (f64, f64))> = None;
            for (i, s) in list.iter().enumerate() {
                for &(px, py) in &s.points {
                    let (qx, qy) = (to_axis(&graph.x, px), to_axis(&graph.y, py));
                    if !(qx.is_finite() && qy.is_finite()) {
                        continue;
                    }
                    let d = frame.screen(qx, qy).distance(pointer);
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, i, (px, py)));
                    }
                }
            }
            if let Some((_, i, (px, py))) = best.filter(|b| b.0 < 18.0) {
                let p = frame.screen(to_axis(&graph.x, px), to_axis(&graph.y, py));
                painter.circle_stroke(p, 5.0, Stroke::new(1.5, colour(ink.primary)));
                lines.push(list[i].label.clone());
                lines.push(format!("{} = {}", graph.x.label, shown(&x_display, px)));
                lines.push(format!("{} = {}", graph.y.label, shown(&y_display, py)));
            }
        }
        Plot::Map(m) => {
            let column = ((x - m.x.0) / (m.x.1 - m.x.0) * m.columns as f64).floor();
            let row = ((y - m.y.0) / (m.y.1 - m.y.0) * m.rows as f64).floor();
            if (0.0..m.columns as f64).contains(&column) && (0.0..m.rows as f64).contains(&row) {
                let (c, r) = (column as usize, row as usize);
                let value = m.values[r * m.columns + c];
                let (low, high) = m.colour_range();
                let cell_x = m.x.0 + (m.x.1 - m.x.0) * (c as f64 + 0.5) / m.columns as f64;
                let cell_y = m.y.0 + (m.y.1 - m.y.0) * (r as f64 + 0.5) / m.rows as f64;
                // Crosshair, and the row and column profiles through the cell: the row's
                // values drawn up from the bottom, the column's in from the right.
                let cross = Stroke::new(1.0, colour(ink.secondary));
                let at = frame.screen(to_axis(&graph.x, cell_x), to_axis(&graph.y, cell_y));
                clipped.line_segment([pos2(plot.left(), at.y), pos2(plot.right(), at.y)], cross);
                clipped.line_segment([pos2(at.x, plot.top()), pos2(at.x, plot.bottom())], cross);
                let level = |v: f64| m.level(v) as f32;
                let profile = Stroke::new(1.5, colour(ink.primary));
                let row_points: Vec<Pos2> = (0..m.columns)
                    .filter_map(|i| {
                        let v = m.values[r * m.columns + i];
                        let px = m.x.0 + (m.x.1 - m.x.0) * (i as f64 + 0.5) / m.columns as f64;
                        v.is_finite().then(|| {
                            let s = frame.screen(to_axis(&graph.x, px), frame.y.0);
                            pos2(s.x, plot.bottom() - 0.25 * plot.height() * level(v))
                        })
                    })
                    .collect();
                let column_points: Vec<Pos2> = (0..m.rows)
                    .filter_map(|j| {
                        let v = m.values[j * m.columns + c];
                        let py = m.y.0 + (m.y.1 - m.y.0) * (j as f64 + 0.5) / m.rows as f64;
                        v.is_finite().then(|| {
                            let s = frame.screen(frame.x.0, to_axis(&graph.y, py));
                            pos2(plot.right() - 0.25 * plot.width() * level(v), s.y)
                        })
                    })
                    .collect();
                clipped.add(Shape::line(row_points, profile));
                clipped.add(Shape::line(column_points, profile));
                let value_display = m.value.display(low, high);
                lines.push(format!("{} = {}", graph.x.label, shown(&x_display, cell_x)));
                lines.push(format!("{} = {}", graph.y.label, shown(&y_display, cell_y)));
                lines.push(format!(
                    "{} = {}",
                    m.value.label,
                    shown(&value_display, value)
                ));
            }
        }
    }
    if !lines.is_empty() {
        let galley = painter.layout_no_wrap(lines.join("\n"), small, colour(ink.primary));
        let mut at = pointer + vec2(12.0, 12.0);
        if at.x + galley.size().x + 8.0 > rect.right() {
            at.x = pointer.x - galley.size().x - 16.0;
        }
        if at.y + galley.size().y + 8.0 > rect.bottom() {
            at.y = pointer.y - galley.size().y - 16.0;
        }
        let back = Rect::from_min_size(at, galley.size() + vec2(8.0, 6.0));
        painter.rect_filled(back, 4.0, colour(ink.surface).gamma_multiply(0.95));
        painter.rect_stroke(
            back,
            4.0,
            Stroke::new(1.0, colour(ink.axis)),
            egui::StrokeKind::Inside,
        );
        painter.galley(at + vec2(4.0, 3.0), galley, colour(ink.primary));
    }
    Some((graph.x.unit, x))
}

/// The graphs the window offers, by the `graph` command's `of`, with their names.
pub const KINDS: &[(&str, &str)] = &[
    ("spot", "Spot diagram"),
    ("irradiance", "Irradiance"),
    ("focus", "Focus by ray height"),
    ("psf-lateral", "PSF across the peak"),
    ("psf-axial", "PSF along the axis"),
    ("psf-section", "PSF section"),
    ("psf-wavefront", "Wavefront error"),
    ("gaussian", "Gaussian beam"),
];
