//! The 3D view: the scene's wireframe and the traced rays, drawn with egui's painter
//! (the shaded wgpu view comes in M8b). Left drag orbits, right or middle drag pans, the
//! wheel zooms about the pointer, a click selects; a view gizmo at the top right and a
//! floating view toolbar at the bottom, as in SolidCraft.

use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};

use crate::camera::{Camera, V3};
use crate::document::{Document, Scene};
use crate::theme::{Accent, DARK};

/// What the view asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Select this item (index into the scene's items), or nothing.
    Select(Option<usize>),
    /// Fit the view to the scene.
    Fit,
}

/// The colour of an item by its role.
pub fn role_colour(role: &str) -> Color32 {
    match role {
        "lens" => Color32::from_rgb(110, 200, 235),
        "mirror" => Color32::from_rgb(214, 220, 230),
        "detector" => Color32::from_rgb(236, 200, 80),
        _ => Color32::from_rgb(112, 120, 136),
    }
}

/// The colour of a ray by how it ended.
pub fn ray_colour(end: &str, generation: u64) -> Color32 {
    let c = match end {
        "detected" => Color32::from_rgb(255, 92, 72),
        "clipped" => Color32::from_rgb(200, 120, 255),
        "escaped" => Color32::from_rgb(255, 170, 90),
        _ => Color32::from_rgb(160, 160, 160),
    };
    if generation == 0 {
        c
    } else {
        c.gamma_multiply(0.35)
    }
}

fn background(painter: &egui::Painter, rect: Rect) {
    let mut mesh = egui::Mesh::default();
    let (top, bottom) = (DARK.viewport_top, DARK.viewport_bottom);
    for (p, c) in [
        (rect.left_top(), top),
        (rect.right_top(), top),
        (rect.right_bottom(), bottom),
        (rect.left_bottom(), bottom),
    ] {
        mesh.colored_vertex(p, c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// Distance from a point to a segment on the screen.
fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 {
        ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * t).distance(p)
}

/// Draws the view in `ui`'s remaining space and handles the pointer.
pub fn show(
    ui: &mut egui::Ui,
    camera: &mut Camera,
    doc: Option<&Document>,
    selected: Option<usize>,
    hidden: &[bool],
    show_rays: bool,
    accent: Accent,
) -> Vec<Request> {
    let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let rect = response.rect;
    background(&painter, rect);
    let mut requests = Vec::new();
    let centre = (rect.center().x as f64, rect.center().y as f64);
    let high = rect.height() as f64;

    // Pointer: orbit, pan, zoom.
    if response.dragged_by(egui::PointerButton::Primary) {
        let d = response.drag_delta();
        camera.orbit(d.x as f64, d.y as f64);
    }
    if response.dragged_by(egui::PointerButton::Secondary)
        || response.dragged_by(egui::PointerButton::Middle)
    {
        let d = response.drag_delta();
        camera.pan(d.x as f64, d.y as f64, high);
    }
    if let Some(pointer) = response.hover_pos() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y) as f64;
        if scroll != 0.0 {
            let mm = camera.height / high;
            let at = (
                (pointer.x as f64 - centre.0) * mm,
                (centre.1 - pointer.y as f64) * mm,
            );
            camera.zoom((scroll * 0.002).exp(), at);
        }
    }

    let screen = |p: V3| {
        let (x, y) = camera.to_screen(p, centre, high);
        Pos2::new(x as f32, y as f32)
    };
    let painter = painter.with_clip_rect(rect);

    let scene: Option<&Scene> = doc.and_then(|d| d.scene.as_ref());
    if let Some(scene) = scene {
        // Absorbers first, so optical items draw on top.
        let mut order: Vec<usize> = (0..scene.items.len()).collect();
        order.sort_by_key(|&i| scene.items[i].role != "absorber");
        for i in order {
            if hidden.get(i).copied().unwrap_or(false) {
                continue;
            }
            let item = &scene.items[i];
            let (colour, width) = if Some(i) == selected {
                (accent.ink, 2.0)
            } else if item.role == "absorber" {
                (role_colour(&item.role).gamma_multiply(0.7), 0.8)
            } else {
                (role_colour(&item.role), 1.3)
            };
            for line in &item.lines {
                if line.len() > 1 {
                    painter.add(Shape::line(
                        line.iter().map(|&p| screen(p)).collect(),
                        Stroke::new(width, colour),
                    ));
                }
            }
        }
        for source in &scene.sources {
            let a = screen(source.origin);
            let tip = [0, 1, 2].map(|k| source.origin[k] + source.axis[k] * camera.height * 0.06);
            painter.arrow(
                a,
                screen(tip) - a,
                Stroke::new(2.0, Color32::from_rgb(255, 92, 72)),
            );
            painter.circle_filled(a, 3.5, Color32::from_rgb(255, 92, 72));
        }
    }
    if show_rays {
        if let Some(trace) = doc.and_then(|d| d.trace.as_ref()) {
            for ray in &trace.rays {
                let mut points: Vec<Pos2> = ray.points.iter().map(|&p| screen(p)).collect();
                if ray.end == "escaped" {
                    if let Some(&last) = ray.points.last() {
                        let far =
                            [0, 1, 2].map(|k| last[k] + ray.direction[k] * camera.height * 0.3);
                        points.push(screen(far));
                    }
                }
                if points.len() > 1 {
                    painter.add(Shape::line(
                        points,
                        Stroke::new(0.8, ray_colour(&ray.end, ray.generation)),
                    ));
                }
            }
        }
    }

    // Click: select the nearest item within 6 px.
    if response.clicked() {
        let picked = response.interact_pointer_pos().and_then(|p| {
            let scene = scene?;
            let mut best: Option<(usize, f32)> = None;
            for (i, item) in scene.items.iter().enumerate() {
                if hidden.get(i).copied().unwrap_or(false) {
                    continue;
                }
                for line in &item.lines {
                    for w in line.windows(2) {
                        let d = segment_distance(p, screen(w[0]), screen(w[1]));
                        if d < 6.0 && best.is_none_or(|b| d < b.1) {
                            best = Some((i, d));
                        }
                    }
                }
            }
            best.map(|b| b.0)
        });
        requests.push(Request::Select(picked));
    }

    gizmo(&painter, rect, camera);
    if toolbar(ui, rect, camera, doc) {
        requests.push(Request::Fit);
    }
    if doc.is_none() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Drop a .optobench layout or sidecar here, or open one from the ribbon",
            egui::FontId::proportional(14.0),
            DARK.text_dim,
        );
    }
    requests
}

/// The axes at the top right: x red, y green, z blue, as SolidCraft's view gizmo.
fn gizmo(painter: &egui::Painter, rect: Rect, camera: &Camera) {
    let centre = Pos2::new(rect.right() - 56.0, rect.top() + 56.0);
    painter.circle_filled(centre, 40.0, Color32::from_black_alpha(60));
    for (axis, colour, label) in [
        ([1.0, 0.0, 0.0], Color32::from_rgb(242, 122, 122), "X"),
        ([0.0, 1.0, 0.0], Color32::from_rgb(98, 212, 142), "Y"),
        ([0.0, 0.0, 1.0], Color32::from_rgb(108, 168, 247), "Z"),
    ] {
        let (x, y, _) = Camera {
            target: [0.0; 3],
            ..*camera
        }
        .view(axis);
        let tip = centre + Vec2::new(x as f32, -y as f32) * 30.0;
        painter.line_segment([centre, tip], Stroke::new(2.0, colour));
        painter.circle_filled(tip, 8.0, colour);
        painter.text(
            tip,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(10.0),
            Color32::BLACK,
        );
    }
}

/// The floating view toolbar; true when Fit was pressed.
fn toolbar(ui: &mut egui::Ui, rect: Rect, camera: &mut Camera, doc: Option<&Document>) -> bool {
    let size = Vec2::new(250.0, 30.0);
    let bar = Rect::from_center_size(Pos2::new(rect.center().x, rect.bottom() - 28.0), size);
    ui.painter().rect_filled(
        bar,
        egui::CornerRadius::same(DARK.radius_lg),
        DARK.panel.gamma_multiply(0.92),
    );
    ui.painter().rect_stroke(
        bar,
        egui::CornerRadius::same(DARK.radius_lg),
        Stroke::new(1.0, DARK.border),
        StrokeKind::Inside,
    );
    let mut fit = false;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar.shrink2(Vec2::new(8.0, 3.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    if child
        .small_button("Fit")
        .on_hover_text("Fit the scene to the view")
        .clicked()
    {
        fit = true;
    }
    if child.small_button("Iso").clicked() {
        camera.isometric();
        fit = doc.is_some();
    }
    if child.small_button("Top").clicked() {
        camera.top();
        fit = doc.is_some();
    }
    if child.small_button("Front").clicked() {
        camera.front();
        fit = doc.is_some();
    }
    if child.small_button("Right").clicked() {
        camera.right();
        fit = doc.is_some();
    }
    fit
}
