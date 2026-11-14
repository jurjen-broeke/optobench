//! The Move tool (M16), as SolidCraft's: with an item selected, three arrows along the
//! scene's x, y and z and three rings about them, drawn at the item. Dragging an arrow
//! slides the item along it, dragging a ring turns it about it; the amount snaps to a
//! step (a hole pitch, or what is typed). Drags add up into a pending move, shown as the
//! item's outline, handles and post where it will go; Enter writes it and ends the tool
//! (`Request::MoveApply`), Escape drops it (`Request::MoveCancel`).

use egui::{Color32, Pos2, Rect, Stroke, Vec2};

use crate::camera::{Camera, V3};

/// The scene axes' colours, as the view gizmo's.
pub const AXIS_COLOURS: [Color32; 3] = [
    Color32::from_rgb(242, 122, 122),
    Color32::from_rgb(98, 212, 142),
    Color32::from_rgb(108, 168, 247),
];

/// Arrow length and ring radius on the screen (points).
const ARROW: f32 = 80.0;
const RING: f32 = 56.0;
/// How near the pointer must be to take a handle (points).
const REACH: f32 = 8.0;

/// What is dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    /// Along a scene axis (0 x, 1 y, 2 z).
    Slide(usize),
    /// About a scene axis.
    Turn(usize),
}

/// A drag under way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    /// The handle taken.
    pub handle: Handle,
    /// Where the pointer took it.
    pub start: Pos2,
    /// The amount so far, snapped: mm for a slide, degrees for a turn.
    pub amount: f64,
}

/// The Move tool's state, kept by the view.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveTool {
    /// The step slides snap to (mm); 0 for none.
    pub step: f64,
    /// The step turns snap to (degrees); 0 for none.
    pub turn_step: f64,
    /// The item to move this frame: where it sits and its outline (scene frame).
    pub target: Option<(V3, Vec<Vec<V3>>)>,
    /// A table layout's pitch, offered as the step.
    pub pitch: Option<f64>,
    /// The drag under way.
    pub drag: Option<Drag>,
    /// The drags done, in order, not yet written.
    pub pending: Vec<(Handle, f64)>,
    /// The scene item being moved (its post follows the move), if an item.
    pub item: Option<usize>,
    /// The source being moved (its post follows the move), if a source.
    pub source: Option<usize>,
}

impl Default for MoveTool {
    fn default() -> Self {
        Self {
            step: 25.0,
            turn_step: 15.0,
            target: None,
            pitch: None,
            drag: None,
            pending: Vec::new(),
            item: None,
            source: None,
        }
    }
}

/// `p` moved by `steps` in order, the item starting at `origin`: slides move both, turns
/// turn `p` about where the item is then.
pub fn placed(p: V3, origin: V3, steps: &[(Handle, f64)]) -> V3 {
    let (mut p, mut o) = (p, origin);
    for &(handle, amount) in steps {
        match handle {
            Handle::Slide(k) => {
                p = add(p, unit(k), amount);
                o = add(o, unit(k), amount);
            }
            Handle::Turn(k) => p = turned(p, o, k, amount),
        }
    }
    p
}

/// What steps come to, in words: `X +50.00 mm, about Z +90.0°`.
pub fn describe(steps: &[(Handle, f64)]) -> String {
    steps
        .iter()
        .map(|&(handle, amount)| match handle {
            Handle::Slide(k) => format!("{} {amount:+.2} mm", ["X", "Y", "Z"][k]),
            Handle::Turn(k) => format!("about {} {amount:+.1}°", ["X", "Y", "Z"][k]),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn unit(k: usize) -> V3 {
    let mut e = [0.0; 3];
    e[k] = 1.0;
    e
}

fn add(a: V3, b: V3, s: f64) -> V3 {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}

/// `p` turned by `degrees` about the scene axis `k` through `centre` (right-handed).
pub fn turned(p: V3, centre: V3, k: usize, degrees: f64) -> V3 {
    let (s, c) = degrees.to_radians().sin_cos();
    let d = [p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]];
    let (i, j) = ((k + 1) % 3, (k + 2) % 3);
    let mut r = d;
    r[i] = c * d[i] - s * d[j];
    r[j] = s * d[i] + c * d[j];
    [r[0] + centre[0], r[1] + centre[1], r[2] + centre[2]]
}

/// Snaps `amount` to `step` (none when the step is 0).
pub fn snapped(amount: f64, step: f64) -> f64 {
    if step > 0.0 {
        (amount / step).round() * step
    } else {
        amount
    }
}

/// The screen geometry of the handles at `origin`.
struct Handles {
    centre: Pos2,
    /// Per axis: arrow tip, and the length (mm) the arrow stands for.
    arrows: [(Pos2, f64); 3],
    /// Per axis: the ring's points.
    rings: [Vec<Pos2>; 3],
    /// Per axis: whether it points towards the viewer.
    towards: [bool; 3],
}

fn handles(origin: V3, camera: &Camera, rect: Rect) -> Handles {
    let centre = (rect.center().x as f64, rect.center().y as f64);
    let high = rect.height() as f64;
    let screen = |p: V3| {
        let (x, y) = camera.to_screen(p, centre, high);
        Pos2::new(x as f32, y as f32)
    };
    let mm_per_point = camera.height / high;
    let length = ARROW as f64 * mm_per_point;
    let radius = RING as f64 * mm_per_point;
    let (back, _, _) = camera.basis();
    let arrows = [0, 1, 2].map(|k| (screen(add(origin, unit(k), length)), length));
    let rings = [0, 1, 2].map(|k| {
        let (i, j) = ((k + 1) % 3, (k + 2) % 3);
        (0..=48)
            .map(|n| {
                let a = std::f64::consts::TAU * f64::from(n) / 48.0;
                let p = add(
                    add(origin, unit(i), radius * a.cos()),
                    unit(j),
                    radius * a.sin(),
                );
                screen(p)
            })
            .collect()
    });
    Handles {
        centre: screen(origin),
        arrows,
        rings,
        towards: [0, 1, 2].map(|k| back[k] >= 0.0),
    }
}

fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 {
        ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * t).distance(p)
}

impl MoveTool {
    /// Where the item is after the pending move (the handles' place while not dragging).
    pub fn moved_origin(&self) -> Option<V3> {
        let (origin, _) = self.target.as_ref()?;
        Some(placed(*origin, *origin, &self.pending))
    }

    /// The pending steps and the drag under way.
    pub fn steps(&self) -> Vec<(Handle, f64)> {
        let mut steps = self.pending.clone();
        if let Some(d) = self.drag.filter(|d| d.amount != 0.0) {
            steps.push((d.handle, d.amount));
        }
        steps
    }

    /// The handle under `pointer`, arrows first.
    pub fn handle_at(&self, pointer: Pos2, camera: &Camera, rect: Rect) -> Option<Handle> {
        let origin = self.moved_origin()?;
        let h = handles(origin, camera, rect);
        let arrow = (0..3)
            .map(|k| (k, segment_distance(pointer, h.centre, h.arrows[k].0)))
            .filter(|(_, d)| *d <= REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((k, _)) = arrow {
            return Some(Handle::Slide(k));
        }
        (0..3)
            .map(|k| {
                let d = h.rings[k]
                    .windows(2)
                    .map(|w| segment_distance(pointer, w[0], w[1]))
                    .fold(f32::INFINITY, f32::min);
                (k, d)
            })
            .filter(|(_, d)| *d <= REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| Handle::Turn(k))
    }

    /// The amount a drag from `start` to `pointer` on `handle` stands for, snapped.
    pub fn amount(
        &self,
        handle: Handle,
        start: Pos2,
        pointer: Pos2,
        camera: &Camera,
        rect: Rect,
    ) -> f64 {
        let Some(origin) = self.moved_origin() else {
            return 0.0;
        };
        let h = handles(origin, camera, rect);
        match handle {
            Handle::Slide(k) => {
                let (tip, length) = h.arrows[k];
                let along = tip - h.centre;
                let pixels = along.length();
                if pixels < 1.0 {
                    return 0.0;
                }
                let moved = (pointer - start).dot(along / pixels) as f64;
                snapped(moved * length / pixels as f64, self.step)
            }
            Handle::Turn(k) => {
                let angle = |p: Pos2| ((p.y - h.centre.y) as f64).atan2((p.x - h.centre.x) as f64);
                let mut swept = angle(pointer) - angle(start);
                if swept > std::f64::consts::PI {
                    swept -= std::f64::consts::TAU;
                } else if swept < -std::f64::consts::PI {
                    swept += std::f64::consts::TAU;
                }
                // The screen's y points down: a turn that looks counter-clockwise is
                // positive about an axis towards the viewer.
                let sign = if h.towards[k] { -1.0 } else { 1.0 };
                snapped(sign * swept.to_degrees(), self.turn_step)
            }
        }
    }

    /// Draws the item where the move takes it (its outline, and with `posts` its post and
    /// the ring where it stands), the handles there, and during a drag the amount.
    pub fn paint(
        &self,
        painter: &egui::Painter,
        camera: &Camera,
        rect: Rect,
        accent: Color32,
        posts: bool,
    ) {
        let Some((origin, outline)) = self.target.as_ref() else {
            return;
        };
        let steps = self.steps();
        let centre = (rect.center().x as f64, rect.center().y as f64);
        let high = rect.height() as f64;
        let screen = |p: V3| {
            let (x, y) = camera.to_screen(p, centre, high);
            Pos2::new(x as f32, y as f32)
        };
        let now = placed(*origin, *origin, &steps);
        if posts {
            let foot = [now[0], now[1], 0.0];
            painter.line_segment([screen(now), screen(foot)], Stroke::new(1.0, accent));
            painter.circle_stroke(screen(foot), 4.0, Stroke::new(1.4, accent));
        }
        if !steps.is_empty() {
            for line in outline {
                if line.len() > 1 {
                    painter.add(egui::Shape::line(
                        line.iter()
                            .map(|&p| screen(placed(p, *origin, &steps)))
                            .collect(),
                        Stroke::new(1.6, accent),
                    ));
                }
            }
        }
        let h = handles(now, camera, rect);
        let taken = self.drag.map(|d| d.handle);
        for k in 0..3 {
            let colour = AXIS_COLOURS[k];
            let width = |handle| if taken == Some(handle) { 3.0 } else { 1.6 };
            painter.add(egui::Shape::line(
                h.rings[k].clone(),
                Stroke::new(width(Handle::Turn(k)), colour.gamma_multiply(0.8)),
            ));
            let (tip, _) = h.arrows[k];
            let along = tip - h.centre;
            if along.length() > 4.0 {
                painter.arrow(
                    h.centre,
                    along,
                    Stroke::new(width(Handle::Slide(k)), colour),
                );
            }
            painter.text(
                tip + along.normalized() * 9.0,
                egui::Align2::CENTER_CENTER,
                ["X", "Y", "Z"][k],
                egui::FontId::proportional(11.0),
                colour,
            );
        }
        painter.circle_filled(h.centre, 3.5, accent);
        if let Some(drag) = self.drag.filter(|d| d.amount != 0.0) {
            painter.text(
                h.centre + Vec2::new(14.0, -14.0),
                egui::Align2::LEFT_BOTTOM,
                describe(&[(drag.handle, drag.amount)]),
                egui::FontId::proportional(13.0),
                Color32::WHITE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "snapped values are whole multiples of the step"
    )]
    fn turns_are_right_handed_and_steps_snap() {
        let p = turned([1.0, 0.0, 5.0], [0.0, 0.0, 5.0], 2, 90.0);
        assert!((p[0]).abs() < 1e-12 && (p[1] - 1.0).abs() < 1e-12 && (p[2] - 5.0).abs() < 1e-12);
        let q = turned([0.0, 1.0, 0.0], [0.0; 3], 0, 90.0);
        assert!((q[2] - 1.0).abs() < 1e-12, "y turns to z about x");
        assert_eq!(snapped(37.0, 25.0), 25.0);
        assert_eq!(snapped(38.0, 25.0), 50.0);
        assert_eq!(snapped(-7.6, 15.0), -15.0);
        assert_eq!(snapped(3.3, 0.0), 3.3);
    }

    #[test]
    fn steps_add_up_slides_moving_the_centre_of_later_turns() {
        // Slid 10 along x, then a quarter turn about z: a point 1 along x from the item
        // ends 1 along y from where the item went.
        let steps = [(Handle::Slide(0), 10.0), (Handle::Turn(2), 90.0)];
        let p = placed([1.0, 0.0, 0.0], [0.0; 3], &steps);
        assert!(
            (p[0] - 10.0).abs() < 1e-12 && (p[1] - 1.0).abs() < 1e-12,
            "{p:?}"
        );
        assert_eq!(describe(&steps), "X +10.00 mm, about Z +90.0°");
    }

    #[test]
    fn dragging_an_arrow_moves_along_it_in_millimetres() {
        // Seen from above, x to the right: dragging the x arrow 40 points to the right
        // moves by 40 points' worth of millimetres, snapped to the step.
        let mut camera = Camera::default();
        camera.top();
        camera.target = [0.0; 3];
        camera.height = 200.0; // mm across the 200-point-high view: 1 mm a point
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(300.0, 200.0));
        let tool = MoveTool {
            step: 10.0,
            target: Some(([0.0; 3], Vec::new())),
            ..MoveTool::default()
        };
        let centre = rect.center();
        let at = tool.handle_at(centre + Vec2::new(60.0, 0.0), &camera, rect);
        assert_eq!(at, Some(Handle::Slide(0)));
        let start = centre + Vec2::new(60.0, 0.0);
        let moved = tool.amount(
            Handle::Slide(0),
            start,
            start + Vec2::new(44.0, 0.0),
            &camera,
            rect,
        );
        assert!((moved - 40.0).abs() < 1e-9, "{moved}");
        // A quarter turn counter-clockwise on the screen about z (towards the viewer).
        let quarter = tool.amount(
            Handle::Turn(2),
            centre + Vec2::new(56.0, 0.0),
            centre + Vec2::new(0.0, -56.0),
            &camera,
            rect,
        );
        assert!((quarter - 90.0).abs() < 1e-9, "{quarter}");
    }
}
