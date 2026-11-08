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
    let shaded = scene.is_some_and(|s| s.items.iter().any(|i| i.mesh.is_some()))
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
