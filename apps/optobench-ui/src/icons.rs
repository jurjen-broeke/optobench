//! optobench's own icons, drawn with egui's painter in SolidCraft's icon language
//! (its `assets/icons/ui/STYLE_GUIDE.md` and `design/palette.toml`, MIT OR Apache-2.0;
//! ATTRIBUTION.md): a 64-unit canvas, the isometric cube projection with 30° slopes,
//! outlines 2.55, action strokes 4.25 and navigation strokes 5.6 with round ends, grey
//! reference geometry with one blue result or action cue, colours by role (the dark
//! theme's values). Optics adds one role of its own: the beam, in the view's ray red.
//! The drawings are optobench's; none is copied from SolidCraft's set.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2};

/// Colour roles (dark theme).
#[derive(Clone, Copy)]
struct Roles {
    ink: Color32,
    line: Color32,
    p1: Color32,
    p2: Color32,
    p3: Color32,
    guide: Color32,
    a1: Color32,
    a2: Color32,
    a3: Color32,
    beam: Color32,
}

const DARK_ROLES: Roles = Roles {
    ink: Color32::from_rgb(0x1f, 0x20, 0x21),
    line: Color32::from_rgb(0xdb, 0xe4, 0xee),
    p1: Color32::from_rgb(0x8d, 0xa3, 0xc3),
    p2: Color32::from_rgb(0x6d, 0x81, 0xa2),
    p3: Color32::from_rgb(0x54, 0x67, 0x82),
    guide: Color32::from_rgb(0x80, 0x93, 0xa4),
    a1: Color32::from_rgb(0x4e, 0xa1, 0xdc),
    a2: Color32::from_rgb(0x32, 0x87, 0xd1),
    a3: Color32::from_rgb(0x29, 0x76, 0xb8),
    beam: Color32::from_rgb(255, 92, 72),
};

const OUTLINE: f32 = 2.55;
const ACTION: f32 = 4.25;
const NAVIGATION: f32 = 5.6;

/// A drawing on the 64-unit canvas, mapped into a rectangle.
struct Canvas<'a> {
    painter: &'a Painter,
    origin: Pos2,
    scale: f32,
    roles: Roles,
}

/// The isometric projection of the cube base: x right-down, y left-down, z up; `s` the
/// scale, (`cx`, `cy`) where the physical origin at z = 12 lands.
#[derive(Clone, Copy)]
struct Iso {
    s: f32,
    cx: f32,
    cy: f32,
}

impl Iso {
    const BASE: Iso = Iso {
        s: 1.125,
        cx: 32.0,
        cy: 32.0,
    };

    fn at(s: f32, cx: f32, cy: f32) -> Self {
        Self { s, cx, cy }
    }

    fn p(self, x: f32, y: f32, z: f32) -> Pos2 {
        let k = 3f32.sqrt() / 2.0;
        pos2(
            self.cx + (x - y) * k * self.s,
            self.cy + (x + y) / 2.0 * self.s - (z - 12.0) * self.s,
        )
    }

    /// A circle in 3D (centre, two unit axes of its plane, radius), projected.
    fn circle(self, c: [f32; 3], u: [f32; 3], v: [f32; 3], r: f32) -> Vec<Pos2> {
        (0..48)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / 48.0;
                let (s, co) = a.sin_cos();
                self.p(
                    c[0] + r * (co * u[0] + s * v[0]),
                    c[1] + r * (co * u[1] + s * v[1]),
                    c[2] + r * (co * u[2] + s * v[2]),
                )
            })
            .collect()
    }
}

/// The convex hull of points (monotone chain), counter-clockwise.
fn hull(mut points: Vec<Pos2>) -> Vec<Pos2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let cross = |o: Pos2, a: Pos2, b: Pos2| (a - o).x * (b - o).y - (a - o).y * (b - o).x;
    let mut lower: Vec<Pos2> = Vec::new();
    for &p in &points {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<Pos2> = Vec::new();
    for &p in points.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

impl Canvas<'_> {
    fn map(&self, p: Pos2) -> Pos2 {
        self.origin + p.to_vec2() * self.scale
    }

    /// A filled convex shape, outlined in ink.
    fn face(&self, points: &[Pos2], fill: Color32) {
        let pts: Vec<Pos2> = points.iter().map(|p| self.map(*p)).collect();
        self.painter.add(Shape::convex_polygon(
            pts,
            fill,
            Stroke::new(OUTLINE * self.scale, self.roles.ink),
        ));
    }

    /// A filled convex shape without outline.
    fn fill(&self, points: &[Pos2], fill: Color32) {
        let pts: Vec<Pos2> = points.iter().map(|p| self.map(*p)).collect();
        self.painter
            .add(Shape::convex_polygon(pts, fill, Stroke::NONE));
    }

    /// A stroke with round ends and joins (egui strokes are round at this width).
    fn line(&self, points: &[Pos2], width: f32, colour: Color32) {
        let pts: Vec<Pos2> = points.iter().map(|p| self.map(*p)).collect();
        let stroke = Stroke::new(width * self.scale, colour);
        self.painter.add(Shape::line(pts.clone(), stroke));
        // Round caps and joins.
        for p in pts {
            self.painter
                .circle_filled(p, width * self.scale / 2.0, colour);
        }
    }

    fn closed(&self, points: &[Pos2], width: f32, colour: Color32) {
        let mut pts = points.to_vec();
        if let Some(&first) = points.first() {
            pts.push(first);
        }
        self.line(&pts, width, colour);
    }

    fn dot(&self, at: Pos2, radius: f32, colour: Color32) {
        self.painter
            .circle_filled(self.map(at), radius * self.scale, colour);
    }

    /// An arrow from `a` to `b` with a head of about 9 units.
    fn arrow(&self, a: Pos2, b: Pos2, width: f32, colour: Color32) {
        let d = (b - a).normalized();
        let n = Vec2::new(-d.y, d.x);
        let base = b - d * 8.0;
        self.line(&[a, base + d * 1.0], width, colour);
        self.fill(&[b + d * 1.0, base + n * 5.0, base - n * 5.0], colour);
    }

    /// A circular arc (screen-space centre and radius, angles in degrees, y down).
    fn arc(&self, c: Pos2, r: f32, from: f32, to: f32) -> Vec<Pos2> {
        (0..=32)
            .map(|i| {
                let a = (from + (to - from) * i as f32 / 32.0).to_radians();
                pos2(c.x + r * a.cos(), c.y + r * a.sin())
            })
            .collect()
    }

    /// The standard badge: a blue disc at the lower right with a plus (or minus)
    /// punched out of it.
    fn badge(&self, plus: bool) {
        let c = pos2(50.0, 50.0);
        self.dot(c, 11.0, self.roles.ink);
        self.dot(c, 9.5, self.roles.a2);
        self.line(&[pos2(44.5, 50.0), pos2(55.5, 50.0)], 3.4, self.roles.line);
        if plus {
            self.line(&[pos2(50.0, 44.5), pos2(50.0, 55.5)], 3.4, self.roles.line);
        }
    }

    /// A box from (x0, y0, z0) to (x1, y1, z1), passive grey unless `active`.
    fn cuboid(&self, iso: Iso, lo: [f32; 3], hi: [f32; 3], active: bool) {
        let (top, left, right) = if active {
            (self.roles.a1, self.roles.a2, self.roles.a3)
        } else {
            (self.roles.p1, self.roles.p2, self.roles.p3)
        };
        let [x0, y0, z0] = lo;
        let [x1, y1, z1] = hi;
        let p = |x, y, z| iso.p(x, y, z);
        // Screen-left face y = y1, screen-right face x = x1, top z = z1.
        self.face(
            &[p(x0, y1, z0), p(x1, y1, z0), p(x1, y1, z1), p(x0, y1, z1)],
            left,
        );
        self.face(
            &[p(x1, y0, z0), p(x1, y1, z0), p(x1, y1, z1), p(x1, y0, z1)],
            right,
        );
        self.face(
            &[p(x0, y0, z1), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1)],
            top,
        );
    }

    /// A short cylinder along the scene x axis (a lens or mirror standing in a beam):
    /// side band, then the near face.
    fn disc(&self, iso: Iso, x0: f32, x1: f32, r: f32, side: Color32, near: Color32) {
        let u = [0.0, 1.0, 0.0];
        let v = [0.0, 0.0, 1.0];
        let back = iso.circle([x0, 0.0, 12.0], u, v, r);
        let front = iso.circle([x1, 0.0, 12.0], u, v, r);
        let mut all = back.clone();
        all.extend(front.iter().copied());
        self.face(&hull(all), side);
        self.face(&front, near);
    }
}

/// Draws an icon by id in `rect`; dimmed when not `enabled`. Unknown ids draw a neutral
/// square with the first letter, so a missing icon shows.
pub fn draw(painter: &Painter, rect: Rect, id: &str, enabled: bool) {
    let mut roles = DARK_ROLES;
    if !enabled {
        for c in [
            &mut roles.ink,
            &mut roles.line,
            &mut roles.p1,
            &mut roles.p2,
            &mut roles.p3,
            &mut roles.guide,
            &mut roles.a1,
            &mut roles.a2,
            &mut roles.a3,
            &mut roles.beam,
        ] {
            *c = c.gamma_multiply(0.45);
        }
    }
    let side = rect.width().min(rect.height());
    let origin = rect.center() - Vec2::splat(side / 2.0);
    let c = Canvas {
        painter,
        origin,
        scale: side / 64.0,
        roles,
    };
    let r = roles;
    let small = Iso::at(0.8, 30.0, 34.0);
    match id {
        // ----- Navigation and files: line art on the background -----
        "open" => {
            let folder = [
                pos2(8.0, 18.0),
                pos2(24.0, 18.0),
                pos2(29.0, 23.0),
                pos2(56.0, 23.0),
                pos2(56.0, 50.0),
                pos2(8.0, 50.0),
            ];
            c.closed(&folder, NAVIGATION, r.line);
            c.line(&[pos2(8.0, 30.0), pos2(56.0, 30.0)], NAVIGATION, r.line);
        }
        "reload" => {
            let arc = c.arc(pos2(32.0, 33.0), 19.0, -60.0, 230.0);
            c.line(&arc, NAVIGATION, r.line);
            let end = pos2(
                32.0 + 19.0 * (-60f32).to_radians().cos(),
                33.0 + 19.0 * (-60f32).to_radians().sin(),
            );
            c.fill(
                &[
                    end + Vec2::new(4.0, -9.0),
                    end + Vec2::new(8.0, 5.0),
                    end + Vec2::new(-6.0, 3.0),
                ],
                r.line,
            );
        }
        "undo" => {
            let arc = c.arc(pos2(34.0, 36.0), 16.0, -90.0, 110.0);
            c.line(&arc, NAVIGATION, r.line);
            c.line(&[pos2(34.0, 20.0), pos2(20.0, 20.0)], NAVIGATION, r.line);
            c.fill(
                &[pos2(10.0, 20.0), pos2(20.0, 11.0), pos2(20.0, 29.0)],
                r.line,
            );
        }
        "fit" => {
            for (a, b, d) in [
                (pos2(8.0, 22.0), pos2(8.0, 8.0), pos2(22.0, 8.0)),
                (pos2(42.0, 8.0), pos2(56.0, 8.0), pos2(56.0, 22.0)),
                (pos2(56.0, 42.0), pos2(56.0, 56.0), pos2(42.0, 56.0)),
                (pos2(22.0, 56.0), pos2(8.0, 56.0), pos2(8.0, 42.0)),
            ] {
                c.line(&[a, b, d], NAVIGATION, r.line);
            }
            let iso = Iso::at(0.55, 32.0, 32.0);
            c.cuboid(iso, [-12.0, -12.0, 0.0], [12.0, 12.0, 24.0], false);
        }
        "isometric" | "top" | "front" | "right" => {
            let iso = Iso::BASE;
            let p = |x, y, z| iso.p(x, y, z);
            let top = [
                p(-12., -12., 24.),
                p(12., -12., 24.),
                p(12., 12., 24.),
                p(-12., 12., 24.),
            ];
            let left = [
                p(-12., 12., 0.),
                p(12., 12., 0.),
                p(12., 12., 24.),
                p(-12., 12., 24.),
            ];
            let right = [
                p(12., -12., 0.),
                p(12., 12., 0.),
                p(12., 12., 24.),
                p(12., -12., 24.),
            ];
            match id {
                "top" => c.fill(&top, r.a2),
                "front" => c.fill(&left, r.a2),
                "right" => c.fill(&right, r.a2),
                _ => {
                    c.fill(&top, r.a1);
                    c.fill(&left, r.a2);
                    c.fill(&right, r.a3);
                }
            }
            for f in [top, left, right] {
                c.closed(&f, ACTION, r.line);
            }
        }
        "rays" => {
            let start = pos2(10.0, 32.0);
            for end in [pos2(54.0, 14.0), pos2(56.0, 32.0), pos2(54.0, 50.0)] {
                c.line(&[start, end], ACTION, r.beam);
            }
            c.dot(start, 6.0, r.beam);
        }
        "view" => {
            let upper = c.arc(pos2(32.0, 52.0), 30.0, 215.0, 325.0);
            let lower = c.arc(pos2(32.0, 12.0), 30.0, 35.0, 145.0);
            c.line(&upper, NAVIGATION, r.line);
            c.line(&lower, NAVIGATION, r.line);
            c.dot(pos2(32.0, 32.0), 8.0, r.a2);
        }
        "list" => {
            for y in [16.0, 32.0, 48.0] {
                c.dot(pos2(12.0, y), 4.0, r.line);
                c.line(&[pos2(24.0, y), pos2(54.0, y)], NAVIGATION, r.line);
            }
        }
        "info" => {
            let ring = c.arc(pos2(32.0, 32.0), 22.0, 0.0, 360.0);
            c.line(&ring, NAVIGATION, r.line);
            c.dot(pos2(32.0, 21.0), 3.8, r.line);
            c.line(&[pos2(32.0, 29.0), pos2(32.0, 43.0)], NAVIGATION, r.line);
        }
        // ----- Documents and edits -----
        "new" => {
            let page = [
                pos2(14.0, 8.0),
                pos2(38.0, 8.0),
                pos2(48.0, 18.0),
                pos2(48.0, 56.0),
                pos2(14.0, 56.0),
            ];
            c.face(&page, r.p1);
            c.line(
                &[pos2(38.0, 8.0), pos2(38.0, 18.0), pos2(48.0, 18.0)],
                OUTLINE,
                r.ink,
            );
            c.badge(true);
        }
        "remove" => {
            c.cuboid(small, [-12.0, -12.0, 0.0], [12.0, 12.0, 24.0], false);
            c.badge(false);
        }
        "edit" => {
            c.cuboid(
                Iso::at(0.8, 26.0, 40.0),
                [-12.0, -12.0, 0.0],
                [12.0, 12.0, 8.0],
                false,
            );
            // A pencil laid across, its tip on the slab.
            let body = [
                pos2(28.0, 40.0),
                pos2(52.0, 12.0),
                pos2(58.0, 17.0),
                pos2(34.0, 45.0),
            ];
            c.face(&body, r.a2);
            c.face(
                &[pos2(28.0, 40.0), pos2(34.0, 45.0), pos2(25.0, 48.0)],
                r.p1,
            );
        }
        "apply" => {
            c.cuboid(
                Iso::at(0.9, 32.0, 40.0),
                [-12.0, -12.0, 0.0],
                [12.0, 12.0, 8.0],
                false,
            );
            c.line(
                &[pos2(16.0, 24.0), pos2(27.0, 34.0), pos2(50.0, 10.0)],
                ACTION + 1.0,
                r.ink,
            );
            c.line(
                &[pos2(16.0, 24.0), pos2(27.0, 34.0), pos2(50.0, 10.0)],
                ACTION,
                r.a3,
            );
        }
        // ----- Optics -----
        "lens" | "add-lens" => {
            let big = Iso::at(1.05, 32.0, 31.0);
            c.disc(big, -3.0, 3.0, 22.0, r.a2, r.a1);
            // The bulge of the near face: a highlight arc.
            let h = big.circle([3.5, 0.0, 12.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 14.0);
            c.line(&h[30..42], OUTLINE, r.line);
            if id == "add-lens" {
                c.badge(true);
            }
        }
        "mirror" | "add-mirror" => {
            let big = Iso::at(1.05, 34.0, 31.0);
            c.disc(big, -6.0, 1.0, 22.0, r.p3, r.p1);
            // A beam in and out of the face.
            let hit = big.p(1.0, 0.0, 12.0);
            c.line(&[pos2(2.0, 8.0), hit], ACTION, r.beam);
            c.line(&[hit, pos2(6.0, 52.0)], ACTION, r.beam);
            if id == "add-mirror" {
                c.badge(true);
            }
        }
        "source" | "add-source" => {
            let iso = Iso::at(0.7, 22.0, 34.0);
            c.cuboid(iso, [-12.0, -8.0, 4.0], [12.0, 8.0, 20.0], false);
            let out = iso.p(12.0, 0.0, 12.0);
            c.arrow(out, pos2(58.0, out.y + 12.0), ACTION, r.beam);
            if id == "add-source" {
                c.badge(true);
            }
        }
        "detector" | "add-detector" => {
            let iso = Iso::at(0.85, 32.0, 32.0);
            c.cuboid(iso, [-2.0, -14.0, 0.0], [2.0, 14.0, 26.0], false);
            // A pixel grid on the near face (x = 2).
            for k in [-7.0, 0.0, 7.0] {
                c.line(&[iso.p(2.0, k, 0.0), iso.p(2.0, k, 26.0)], 1.6, r.ink);
            }
            for z in [6.5, 13.0, 19.5] {
                c.line(&[iso.p(2.0, -14.0, z), iso.p(2.0, 14.0, z)], 1.6, r.ink);
            }
            c.fill(
                &[
                    iso.p(2.0, 0.0, 6.5),
                    iso.p(2.0, 0.0, 13.0),
                    iso.p(2.0, 7.0, 13.0),
                    iso.p(2.0, 7.0, 6.5),
                ],
                r.beam,
            );
            if id == "add-detector" {
                c.badge(true);
            }
        }
        "trace" => {
            let iso = Iso::at(0.75, 32.0, 34.0);
            let rays = [
                (pos2(4.0, 22.0), iso.p(0.0, 0.0, 22.0)),
                (pos2(4.0, 30.0), iso.p(0.0, 0.0, 12.0)),
                (pos2(4.0, 38.0), iso.p(0.0, 0.0, 2.0)),
            ];
            for (a, b) in rays {
                c.line(&[a, b], ACTION, r.beam);
            }
            c.disc(iso, -1.5, 1.5, 16.0, r.a2, r.a1);
            let focus = pos2(58.0, 44.0);
            for (_, b) in rays {
                c.line(&[b, focus], ACTION, r.beam);
            }
        }
        "optimise" => {
            for radius in [22.0, 13.0] {
                let ring = c.arc(pos2(32.0, 32.0), radius, 0.0, 360.0);
                c.line(&ring, ACTION, r.a3);
            }
            for (a, b) in [
                (pos2(32.0, 4.0), pos2(32.0, 14.0)),
                (pos2(32.0, 50.0), pos2(32.0, 60.0)),
                (pos2(4.0, 32.0), pos2(14.0, 32.0)),
                (pos2(50.0, 32.0), pos2(60.0, 32.0)),
            ] {
                c.line(&[a, b], ACTION, r.a3);
            }
            c.dot(pos2(32.0, 32.0), 5.5, r.beam);
        }
        "check-moves" => {
            let iso = Iso::at(0.75, 26.0, 36.0);
            // Where the part was (guide outline) and where it is (grey).
            let g = Iso::at(0.75, 34.0, 30.0);
            let p = |x, y, z| g.p(x, y, z);
            c.closed(
                &[
                    p(-12., -12., 24.),
                    p(12., -12., 24.),
                    p(12., 12., 24.),
                    p(12., 12., 0.),
                    p(-12., 12., 0.),
                    p(-12., 12., 24.),
                ],
                2.0,
                r.guide,
            );
            c.cuboid(iso, [-12.0, -12.0, 0.0], [12.0, 12.0, 24.0], false);
            c.line(
                &[pos2(36.0, 46.0), pos2(43.0, 53.0), pos2(58.0, 36.0)],
                ACTION,
                r.a3,
            );
        }
        "bodies" => {
            c.cuboid(
                Iso::at(0.6, 22.0, 24.0),
                [-12.0, -12.0, 0.0],
                [12.0, 12.0, 24.0],
                false,
            );
            c.cuboid(
                Iso::at(0.6, 42.0, 40.0),
                [-12.0, -12.0, 0.0],
                [12.0, 12.0, 24.0],
                false,
            );
            c.cuboid(
                Iso::at(0.6, 20.0, 46.0),
                [-12.0, -12.0, 0.0],
                [12.0, 12.0, 24.0],
                true,
            );
        }
        "tag-lens" | "tag-mirror" => {
            c.cuboid(
                Iso::at(0.75, 26.0, 34.0),
                [-12.0, -12.0, 0.0],
                [12.0, 12.0, 24.0],
                id == "tag-lens",
            );
            // A tag hanging from the body.
            let tag = [
                pos2(40.0, 40.0),
                pos2(48.0, 32.0),
                pos2(60.0, 44.0),
                pos2(52.0, 56.0),
                pos2(40.0, 56.0),
            ];
            c.face(&tag, if id == "tag-lens" { r.a1 } else { r.p1 });
            c.dot(pos2(47.0, 41.0), 2.4, r.ink);
            if id == "tag-mirror" {
                let hit = pos2(26.0, 14.0);
                c.line(&[pos2(4.0, 2.0), hit], ACTION, r.beam);
                c.line(&[hit, pos2(10.0, 30.0)], ACTION, r.beam);
            }
        }
        "glass" | "import-glass" => {
            // A prism bending the beam.
            let prism = [pos2(32.0, 8.0), pos2(54.0, 50.0), pos2(10.0, 50.0)];
            c.face(&prism, r.p2);
            c.line(&[pos2(2.0, 36.0), pos2(22.0, 30.0)], ACTION, r.beam);
            c.line(&[pos2(22.0, 30.0), pos2(42.0, 34.0)], ACTION, r.beam);
            c.line(&[pos2(42.0, 34.0), pos2(62.0, 46.0)], ACTION, r.beam);
            if id == "import-glass" {
                c.badge(true);
            }
        }
        "curve" | "import-curve" => {
            c.line(
                &[pos2(10.0, 8.0), pos2(10.0, 54.0), pos2(56.0, 54.0)],
                NAVIGATION,
                r.line,
            );
            let curve = [
                pos2(14.0, 48.0),
                pos2(22.0, 47.0),
                pos2(27.0, 40.0),
                pos2(30.0, 20.0),
                pos2(34.0, 15.0),
                pos2(44.0, 15.0),
                pos2(48.0, 20.0),
                pos2(52.0, 46.0),
            ];
            c.line(&curve, ACTION, r.a3);
            if id == "import-curve" {
                c.badge(true);
            }
        }
        "part" | "import-part" => {
            let iso = Iso::at(0.6, 32.0, 36.0);
            for (dy, active) in [(-20.0, false), (0.0, true), (20.0, false)] {
                c.cuboid(iso, [-12.0, dy - 4.0, 0.0], [12.0, dy + 4.0, 30.0], active);
            }
            if id == "import-part" {
                c.badge(true);
            }
        }
        "embed" => {
            let iso = Iso::at(0.85, 32.0, 38.0);
            c.cuboid(iso, [-12.0, -12.0, 0.0], [12.0, 12.0, 18.0], false);
            c.arrow(pos2(32.0, 2.0), iso.p(0.0, 0.0, 18.0), ACTION, r.a3);
        }
        "export" => {
            let iso = Iso::at(0.75, 24.0, 34.0);
            c.cuboid(iso, [-12.0, -12.0, 0.0], [12.0, 12.0, 24.0], false);
            c.arrow(pos2(36.0, 30.0), pos2(60.0, 30.0), ACTION, r.a3);
        }
        // ----- Workspace emblems -----
        "workspace-cad" => {
            c.cuboid(Iso::BASE, [-12.0, -12.0, 0.0], [12.0, 12.0, 24.0], false);
            let p = |x, y, z| Iso::BASE.p(x, y, z);
            c.line(
                &[p(12., -12., 24.), p(12., 12., 24.), p(-12., 12., 24.)],
                ACTION,
                r.a3,
            );
        }
        "workspace-layout" => {
            let iso = Iso::at(1.1, 32.0, 38.0);
            c.cuboid(iso, [-16.0, -16.0, 0.0], [16.0, 16.0, 4.0], false);
            for i in [-10.0, 0.0, 10.0] {
                for j in [-10.0, 0.0, 10.0] {
                    c.dot(iso.p(i, j, 4.0), 1.6, r.ink);
                }
            }
            let post = Iso::at(1.1, 32.0, 38.0);
            c.disc(post, -1.0, 1.0, 7.0, r.a2, r.a1);
        }
        "workspace-simulate" => {
            let iso = Iso::at(0.95, 32.0, 32.0);
            for (a, z) in [(pos2(2.0, 22.0), 22.0), (pos2(2.0, 34.0), 12.0)] {
                c.line(&[a, iso.p(0.0, 0.0, z)], ACTION, r.beam);
            }
            c.disc(iso, -1.5, 1.5, 15.0, r.a2, r.a1);
            for z in [22.0, 12.0] {
                c.line(&[iso.p(0.0, 0.0, z), pos2(60.0, 40.0)], ACTION, r.beam);
            }
        }
        _ => {
            let square = [
                pos2(14.0, 14.0),
                pos2(50.0, 14.0),
                pos2(50.0, 50.0),
                pos2(14.0, 50.0),
            ];
            c.closed(&square, OUTLINE, r.guide);
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                id.chars().next().unwrap_or('?').to_ascii_uppercase(),
                egui::FontId::proportional(side * 0.4),
                r.line,
            );
        }
    }
}

/// Every icon id, for the review sheet and the test that each is drawn.
pub const ALL: &[&str] = &[
    "open",
    "reload",
    "undo",
    "fit",
    "isometric",
    "top",
    "front",
    "right",
    "rays",
    "view",
    "info",
    "list",
    "new",
    "remove",
    "edit",
    "apply",
    "lens",
    "add-lens",
    "mirror",
    "add-mirror",
    "source",
    "add-source",
    "detector",
    "add-detector",
    "trace",
    "optimise",
    "check-moves",
    "bodies",
    "tag-lens",
    "tag-mirror",
    "glass",
    "import-glass",
    "curve",
    "import-curve",
    "part",
    "import-part",
    "embed",
    "export",
    "workspace-cad",
    "workspace-layout",
    "workspace-simulate",
];
