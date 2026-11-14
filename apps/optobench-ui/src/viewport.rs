//! The 3D view: the scene shaded on wgpu with its edges and the traced rays (`render`), or,
//! without wgpu or meshes, as a wireframe drawn with egui's painter. Left drag orbits,
//! right or middle drag pans, the wheel zooms about the pointer, a click selects what is
//! under it; a view gizmo at the top right and a floating view toolbar at the bottom
//! (fit, standard views, perspective, section), as in SolidCraft.

use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};

use crate::camera::{Camera, V3};
use crate::document::{Document, Hit, Scene};
use crate::render::{self, Renderer};
use crate::theme::{Accent, DARK};

/// The view's own state besides the camera.
#[derive(Default)]
pub struct Extras {
    /// The shaded renderer, when eframe runs on wgpu.
    pub renderer: Option<Renderer>,
    /// The item under the pointer.
    pub hovered: Option<usize>,
    /// The section: the axis (0 x, 1 y, 2 z) whose plane cuts the scene, and where.
    pub section: Option<(usize, f64)>,
    /// The normal of the face last clicked (scene frame), for tagging a mirror by its
    /// reflecting face.
    pub picked_normal: Option<V3>,
    /// Where the view was last drawn.
    pub rect: Option<Rect>,
    /// A table layout's breadboard, drawn under the items.
    pub board: Option<crate::board::Board>,
    /// Whether the breadboard is shown.
    pub show_board: bool,
    /// The Move tool, when it is on.
    pub moving: Option<crate::moving::MoveTool>,
}

impl Extras {
    /// The section plane as (unit normal, offset): parts with p·n > offset, on the
    /// viewer's side of the plane, are cut away, so the cut faces the viewer.
    pub fn plane(&self, camera: &Camera) -> Option<(V3, f64)> {
        let (back, _, _) = camera.basis();
        self.section.map(|(axis, at)| {
            let towards_viewer = if back[axis] >= 0.0 { 1.0 } else { -1.0 };
            let mut n = [0.0; 3];
            n[axis] = towards_viewer;
            (n, at * towards_viewer)
        })
    }
}

/// The item a line of sight meets first (shaded scenes), skipping hidden items and parts
/// cut away by the section.
pub fn pick_mesh(
    scene: &Scene,
    hidden: &[bool],
    plane: Option<(V3, f64)>,
    origin: V3,
    direction: V3,
) -> Option<(usize, Hit)> {
    let keep = |p: V3| plane.is_none_or(|(n, at)| n[0] * p[0] + n[1] * p[1] + n[2] * p[2] <= at);
    let mut best: Option<(usize, Hit)> = None;
    for (i, item) in scene.items.iter().enumerate() {
        if hidden.get(i).copied().unwrap_or(false) {
            continue;
        }
        if let Some(hit) = item
            .mesh
            .as_ref()
            .and_then(|m| m.hit(origin, direction, &keep))
        {
            if best.is_none_or(|b| hit.distance < b.1.distance) {
                best = Some((i, hit));
            }
        }
    }
    best
}

/// What the view asks the app to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Select this item (index into the scene's items), or nothing.
    Select(Option<usize>),
    /// Fit the view to the scene.
    Fit,
    /// Write the Move tool's pending move (slides in mm along, turns in degrees about,
    /// scene axes, in order) and end the tool.
    MoveApply(Vec<(crate::moving::Handle, f64)>),
    /// Drop the pending move and end the tool.
    MoveCancel,
}

/// The colour of an item by its role.
pub fn role_colour(role: &str) -> Color32 {
    match role {
        "lens" => Color32::from_rgb(110, 200, 235),
        "mirror" => Color32::from_rgb(214, 220, 230),
        "detector" => Color32::from_rgb(236, 200, 80),
        "sample" => Color32::from_rgb(110, 160, 220),
        "objective" => Color32::from_rgb(214, 128, 36),
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

/// Two unit vectors across `direction` (and across each other), for a ring facing it.
fn across(direction: V3) -> (V3, V3) {
    let cross = |a: V3, b: V3| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let unit = |a: V3| {
        let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-12);
        a.map(|c| c / l)
    };
    let hint = if direction[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let u = unit(cross(direction, hint));
    (u, unit(cross(direction, u)))
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
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    camera: &mut Camera,
    extras: &mut Extras,
    doc: Option<&Document>,
    selected: Option<usize>,
    hidden: &[bool],
    show_rays: bool,
    accent: Accent,
) -> Vec<Request> {
    let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let rect = response.rect;
    extras.rect = Some(rect);
    background(&painter, rect);
    let mut requests = Vec::new();
    let centre = (rect.center().x as f64, rect.center().y as f64);
    let high = rect.height() as f64;

    // The Move tool's handles take a drag that starts on them.
    if let Some(tool) = extras.moving.as_mut() {
        if response.drag_started_by(egui::PointerButton::Primary) {
            let start = ui.input(|i| i.pointer.press_origin());
            if let Some((start, handle)) =
                start.and_then(|p| Some((p, tool.handle_at(p, camera, rect)?)))
            {
                tool.drag = Some(crate::moving::Drag {
                    handle,
                    start,
                    amount: 0.0,
                });
            }
        }
        if let (Some(drag), Some(pointer)) = (tool.drag, response.interact_pointer_pos()) {
            let amount = tool.amount(drag.handle, drag.start, pointer, camera, rect);
            tool.drag = Some(crate::moving::Drag { amount, ..drag });
        }
        if response.drag_stopped() {
            if let Some(drag) = tool.drag.take() {
                if drag.amount != 0.0 {
                    tool.pending.push((drag.handle, drag.amount));
                }
            }
        }
        // Enter writes the move and ends the tool, Escape drops it (not while typing).
        if tool.drag.is_none() && !ui.ctx().egui_wants_keyboard_input() {
            let (enter, escape) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Enter),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            if enter {
                requests.push(Request::MoveApply(tool.pending.clone()));
            } else if escape {
                requests.push(Request::MoveCancel);
            }
        }
        if tool.drag.is_none() {
            if let Some(p) = response.hover_pos() {
                if tool.handle_at(p, camera, rect).is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
            }
        }
    }
    let handle_dragged = extras.moving.as_ref().is_some_and(|t| t.drag.is_some());

    // Pointer: orbit, pan, zoom.
    if response.dragged_by(egui::PointerButton::Primary) && !handle_dragged {
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
    let shaded = (scene.is_some_and(|s| s.items.iter().any(|i| i.mesh.is_some()))
        || extras.board.is_some())
        && extras.renderer.is_some();
    let plane = extras.plane(camera);

    // The scene's extent, for rays that leave it.
    let reach = scene.and_then(|s| s.bounds).map_or(100.0, |(lo, hi)| {
        ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt()
    });
    let ray_paths = |doc: &Document| -> Vec<(Vec<V3>, Color32)> {
        let Some(trace) = &doc.trace else {
            return Vec::new();
        };
        trace
            .rays
            .iter()
            .map(|ray| {
                let mut points = ray.points.clone();
                if ray.end == "escaped" {
                    if let Some(&last) = ray.points.last() {
                        points.push([0, 1, 2].map(|k| last[k] + ray.direction[k] * 0.3 * reach));
                    }
                }
                (points, ray_colour(&ray.end, ray.generation))
            })
            .collect()
    };

    // Hover and pick on the triangles, whenever the scene has them.
    let meshes = scene.is_some_and(|s| s.items.iter().any(|i| i.mesh.is_some()));
    if let (true, Some(scene)) = (meshes, scene) {
        let pick = |p: Pos2| {
            let (o, d) = camera.ray((p.x as f64, p.y as f64), centre, high);
            pick_mesh(scene, hidden, plane, o, d)
        };
        extras.hovered = response.hover_pos().and_then(pick).map(|p| p.0);
        if response.clicked() {
            let picked = response.interact_pointer_pos().and_then(pick);
            extras.picked_normal = picked.map(|p| p.1.normal);
            requests.push(Request::Select(picked.map(|p| p.0)));
        }
    }
    if let (true, Some(scene), Some(doc)) = (shaded, scene, doc) {
        let segments: Vec<(V3, V3, Color32)> = if show_rays {
            ray_paths(doc)
                .into_iter()
                .flat_map(|(points, colour)| {
                    points
                        .windows(2)
                        .map(|w| (w[0], w[1], colour))
                        .collect::<Vec<_>>()
                })
                .collect()
        } else {
            Vec::new()
        };
        let pixels = ui.ctx().pixels_per_point();
        let size = (
            (rect.width() * pixels).round() as u32,
            (rect.height() * pixels).round() as u32,
        );
        let (board, show_board) = (extras.board, extras.show_board);
        if let Some(renderer) = extras.renderer.as_mut() {
            let texture = renderer.draw(
                scene,
                doc.scene_version,
                &render::Frame {
                    camera,
                    hidden,
                    selected,
                    hovered: extras.hovered,
                    section: plane,
                    accent: accent.fill,
                    rays: &segments,
                    rays_version: doc.trace_version * 2 + u64::from(show_rays),
                    board,
                    show_board,
                },
                size,
            );
            painter.image(
                texture,
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    } else {
        if !meshes {
            extras.hovered = None;
        }
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
        }
        if let (true, Some(doc)) = (show_rays, doc) {
            for (points, colour) in ray_paths(doc) {
                if points.len() > 1 {
                    painter.add(Shape::line(
                        points.iter().map(|&p| screen(p)).collect(),
                        Stroke::new(0.8, colour),
                    ));
                }
            }
        }
        // Click without triangles: select the nearest outline within 6 px.
        if response.clicked() && !meshes {
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
    }

    // On a breadboard, a post from each item down to the board, a ring where it stands:
    // where it is on the grid and how high.
    if let (Some(scene), true) = (scene, extras.board.is_some() && extras.show_board) {
        let posts = scene
            .items
            .iter()
            .enumerate()
            .filter(|(i, _)| !hidden.get(*i).copied().unwrap_or(false))
            .filter_map(|(i, item)| Some(((Some(i), None), item.origin?)))
            .chain(
                scene
                    .sources
                    .iter()
                    .enumerate()
                    .map(|(j, s)| ((None, Some(j)), s.origin)),
            );
        // The item being moved has its post drawn where the move takes it.
        let moved = extras
            .moving
            .as_ref()
            .filter(|t| t.target.is_some())
            .map(|t| (t.item, t.source));
        for ((index, source), top) in posts {
            if moved.is_some_and(|m| m == (index, source) && (index, source) != (None, None)) {
                continue;
            }
            let foot = [top[0], top[1], 0.0];
            let colour = if index.is_some() && index == selected {
                accent.ink
            } else {
                Color32::from_gray(150).gamma_multiply(0.7)
            };
            painter.line_segment([screen(top), screen(foot)], Stroke::new(1.0, colour));
            painter.circle_stroke(screen(foot), 4.0, Stroke::new(1.2, colour));
        }
    }

    // Where alignment irises go (the trace's advice, not part of the design): a ring
    // across the beam on a post, named as the results list them.
    if let Some(report) = doc.and_then(|d| d.trace.as_ref()).map(|t| &t.report) {
        let a = &report["alignment_irises"];
        let d = &a["segment"]["direction"];
        let direction = [0, 1, 2].map(|k| d[k].as_f64().unwrap_or(0.0));
        let (u, v) = across(direction);
        let colour = Color32::from_rgb(140, 230, 170);
        for (k, iris) in a["irises"].as_array().into_iter().flatten().enumerate() {
            let p = &iris["position_mm"];
            let Some(centre) = (|| Some([p[0].as_f64()?, p[1].as_f64()?, p[2].as_f64()?]))() else {
                continue;
            };
            let radius = 12.7;
            let ring: Vec<Pos2> = (0..=32)
                .map(|n| {
                    let t = std::f64::consts::TAU * f64::from(n) / 32.0;
                    screen(
                        [0, 1, 2].map(|i| centre[i] + radius * (u[i] * t.cos() + v[i] * t.sin())),
                    )
                })
                .collect();
            painter.add(Shape::line(ring, Stroke::new(2.0, colour)));
            let foot = [centre[0], centre[1], 0.0];
            let below = [centre[0], centre[1], centre[2] - radius];
            painter.line_segment([screen(below), screen(foot)], Stroke::new(1.0, colour));
            painter.circle_stroke(screen(foot), 4.0, Stroke::new(1.2, colour));
            painter.text(
                screen([centre[0], centre[1], centre[2] + radius]) + Vec2::new(0.0, -6.0),
                egui::Align2::CENTER_BOTTOM,
                format!("I{}", k + 1),
                egui::FontId::proportional(12.0),
                colour,
            );
        }
    }

    if let Some(tool) = &extras.moving {
        let posts = extras.board.is_some() && extras.show_board;
        tool.paint(&painter, camera, rect, accent.ink, posts);
    }

    // Sources on top: where the light starts and where it goes.
    if let Some(scene) = scene {
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

    gizmo(&painter, rect, camera);
    if let Some(tool) = extras.moving.as_mut() {
        if let Some(request) = move_panel(ui, rect, tool) {
            requests.push(request);
        }
    }
    if toolbar(ui, rect, camera, extras, scene, doc.is_some()) {
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
    } else if scene.is_none() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Reading the design…",
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

/// The Move tool's settings, floating at the view's top left: the steps moves and turns
/// snap to.
fn move_panel(
    ui: &mut egui::Ui,
    rect: Rect,
    tool: &mut crate::moving::MoveTool,
) -> Option<Request> {
    let panel = Rect::from_min_size(
        rect.left_top() + Vec2::new(12.0, 12.0),
        Vec2::new(270.0, 150.0),
    );
    ui.painter().rect_filled(
        panel,
        egui::CornerRadius::same(DARK.radius_lg),
        DARK.panel.gamma_multiply(0.92),
    );
    ui.painter().rect_stroke(
        panel,
        egui::CornerRadius::same(DARK.radius_lg),
        Stroke::new(1.0, DARK.border),
        StrokeKind::Inside,
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(panel.shrink(8.0))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.label(egui::RichText::new("Move").strong());
    child.horizontal(|ui| {
        ui.label(egui::RichText::new("step").color(DARK.text_dim));
        ui.add(
            egui::DragValue::new(&mut tool.step)
                .range(0.0..=1000.0)
                .speed(0.1)
                .suffix(" mm"),
        );
        if let Some(pitch) = tool.pitch {
            if ui
                .small_button("pitch")
                .on_hover_text(format!("One hole: {pitch} mm"))
                .clicked()
            {
                tool.step = pitch;
            }
        }
    });
    child.horizontal(|ui| {
        ui.label(egui::RichText::new("turn").color(DARK.text_dim));
        ui.add(
            egui::DragValue::new(&mut tool.turn_step)
                .range(0.0..=180.0)
                .speed(0.1)
                .suffix(" °"),
        );
        ui.label(
            egui::RichText::new("0: no snapping")
                .color(DARK.text_dim)
                .size(11.0),
        );
    });
    if tool.target.is_none() {
        child.label(
            egui::RichText::new("Select an item of the layout to move it")
                .color(DARK.text_dim)
                .size(11.0),
        );
        return None;
    }
    let mut request = None;
    let text = if tool.pending.is_empty() {
        egui::RichText::new("Drag an arrow to slide, a ring to turn").color(DARK.text_dim)
    } else {
        egui::RichText::new(crate::moving::describe(&tool.pending))
    };
    child.add(egui::Label::new(text.size(11.0)).truncate());
    child.horizontal(|ui| {
        if ui
            .add_enabled(!tool.pending.is_empty(), egui::Button::new("Apply"))
            .on_hover_text("Write the move into the design and end Move (Enter)")
            .clicked()
        {
            request = Some(Request::MoveApply(tool.pending.clone()));
        }
        if ui
            .button("Cancel")
            .on_hover_text("Drop the move and end Move (Escape)")
            .clicked()
        {
            request = Some(Request::MoveCancel);
        }
    });
    request
}

/// The floating view toolbar; true when Fit was pressed.
fn toolbar(
    ui: &mut egui::Ui,
    rect: Rect,
    camera: &mut Camera,
    extras: &mut Extras,
    scene: Option<&Scene>,
    open: bool,
) -> bool {
    let wide = if extras.section.is_some() {
        560.0
    } else {
        400.0
    };
    let bar = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.bottom() - 28.0),
        Vec2::new(wide, 30.0),
    );
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
    for (label, view) in [
        ("Iso", Camera::isometric as fn(&mut Camera)),
        ("Top", Camera::top),
        ("Front", Camera::front),
        ("Right", Camera::right),
    ] {
        if child.small_button(label).clicked() {
            view(camera);
            fit = open;
        }
    }
    child.separator();
    if child
        .selectable_label(camera.perspective, "Perspective")
        .on_hover_text("Perspective or orthographic projection")
        .clicked()
    {
        camera.perspective = !camera.perspective;
    }
    let label = match extras.section {
        None => "Section",
        Some((0, _)) => "Section X",
        Some((1, _)) => "Section Y",
        Some(_) => "Section Z",
    };
    let bounds = scene.and_then(|s| s.bounds);
    if child
        .selectable_label(extras.section.is_some(), label)
        .on_hover_text("Cut the scene by a plane across x, then y, then z, then off")
        .clicked()
    {
        extras.section = match extras.section {
            None => Some(0),
            Some((axis, _)) if axis < 2 => Some(axis + 1),
            Some(_) => None,
        }
        .map(|axis| {
            let middle = bounds.map_or(0.0, |(lo, hi)| 0.5 * (lo[axis] + hi[axis]));
            (axis, middle)
        });
    }
    if let (Some((axis, at)), Some((lo, hi))) = (extras.section.as_mut(), bounds) {
        child.add(
            egui::Slider::new(at, lo[*axis]..=hi[*axis])
                .suffix(" mm")
                .max_decimals(1),
        );
    }
    fit
}
