//! The 3D view's camera: orthographic or perspective, orbiting a target in the scene
//! frame (Z up), with SolidCraft's standard views. Pure geometry, so it is tested without
//! a window.

/// A point or direction in the scene (mm).
pub type V3 = [f64; 3];

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// A camera looking at `target` from the direction given by yaw (about z, from +x) and
/// pitch (above the x–y plane); `height` is the scene height the view shows at the
/// target (mm). In perspective the eye is as far away as a 30° field of view needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Turn about z (radians).
    pub yaw: f64,
    /// Elevation (radians), −90° to 90°.
    pub pitch: f64,
    /// The point looked at.
    pub target: V3,
    /// The scene height shown (mm).
    pub height: f64,
    /// Perspective (else orthographic).
    pub perspective: bool,
}

/// The perspective field of view, top to bottom (radians).
pub const FIELD_OF_VIEW: f64 = 30.0 * core::f64::consts::PI / 180.0;

impl Default for Camera {
    fn default() -> Self {
        let mut c = Self {
            yaw: 0.0,
            pitch: 0.0,
            target: [0.0; 3],
            height: 200.0,
            perspective: false,
        };
        c.isometric();
        c
    }
}

impl Camera {
    /// Unit vectors: towards the viewer, to the right, and up, on the screen.
    pub fn basis(&self) -> (V3, V3, V3) {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let back = [cp * cy, cp * sy, sp];
        let right = [-sy, cy, 0.0];
        let up = cross(back, right);
        (back, right, up)
    }

    /// A scene point in view coordinates: (right, up) in mm from the target, and depth
    /// towards the viewer.
    pub fn view(&self, p: V3) -> (f64, f64, f64) {
        let (back, right, up) = self.basis();
        let d = sub(p, self.target);
        (dot(d, right), dot(d, up), dot(d, back))
    }

    /// The eye's distance from the target in perspective (mm).
    pub fn distance(&self) -> f64 {
        0.5 * self.height / (0.5 * FIELD_OF_VIEW).tan()
    }

    /// Pixels per mm for a point at view depth `z` (towards the viewer, from the
    /// target), or `None` behind the eye.
    fn scale_at(&self, z: f64, pixels_high: f64) -> Option<f64> {
        if self.perspective {
            let d = self.distance();
            (d - z > 1e-9 * d).then(|| pixels_high / self.height * d / (d - z))
        } else {
            Some(pixels_high / self.height)
        }
    }

    /// A scene point on a screen rectangle (`centre`, height in pixels): pixel x to the
    /// right, y down. A point behind the eye goes far off the screen.
    pub fn to_screen(self, p: V3, centre: (f64, f64), pixels_high: f64) -> (f64, f64) {
        let (x, y, z) = self.view(p);
        let scale = self.scale_at(z, pixels_high).unwrap_or(1e9);
        (centre.0 + x * scale, centre.1 - y * scale)
    }

    /// The line of sight through a pixel: origin and unit direction, into the scene.
    pub fn ray(&self, pixel: (f64, f64), centre: (f64, f64), pixels_high: f64) -> (V3, V3) {
        let (back, right, up) = self.basis();
        let mm = self.height / pixels_high;
        let (x, y) = ((pixel.0 - centre.0) * mm, (centre.1 - pixel.1) * mm);
        let on_plane = [0, 1, 2].map(|i| self.target[i] + right[i] * x + up[i] * y);
        if self.perspective {
            let d = self.distance();
            let eye = [0, 1, 2].map(|i| self.target[i] + back[i] * d);
            let dir = sub(on_plane, eye);
            let length = dot(dir, dir).sqrt();
            (eye, dir.map(|c| c / length))
        } else {
            (on_plane, back.map(|c| -c))
        }
    }

    /// The view–projection matrix (column-major, clip depth 0 to 1) for a view `aspect`
    /// (width / height) wide, with depth room for a scene of `radius` (mm) around the
    /// target.
    pub fn matrix(&self, aspect: f64, radius: f64) -> [[f32; 4]; 4] {
        let (back, right, up) = self.basis();
        let radius = radius.max(1e-3);
        // View: (right, up, back) from the target; then projection.
        let view = |row: V3, offset: f64| [row[0], row[1], row[2], -dot(row, self.target) + offset];
        let mut rows: [[f64; 4]; 4];
        if self.perspective {
            let d = self.distance();
            let far = d + 2.0 * radius;
            let near = (d - 2.0 * radius).max(far * 1e-4);
            let f = 1.0 / (0.5 * FIELD_OF_VIEW).tan();
            // Eye space: z_e = back·(p − target) − d (negative in front of the eye).
            let ze = view(back, -d);
            let a = far / (near - far);
            let b = near * far / (near - far);
            rows = [
                view(right, 0.0).map(|c| c * f / aspect),
                view(up, 0.0).map(|c| c * f),
                ze.map(|c| c * a),
                ze.map(|c| -c),
            ];
            rows[2][3] += b;
        } else {
            let half = 0.5 * self.height;
            // Depth: from 2·radius in front of the target (0) to 2·radius behind (1).
            let z = view(back, 0.0);
            rows = [
                view(right, 0.0).map(|c| c / (half * aspect)),
                view(up, 0.0).map(|c| c / half),
                z.map(|c| -c / (4.0 * radius)),
                [0.0, 0.0, 0.0, 1.0],
            ];
            rows[2][3] += 0.5;
        }
        // Column-major for the shader.
        let mut m = [[0.0_f32; 4]; 4];
        for (r, row) in rows.iter().enumerate() {
            for (c, value) in row.iter().enumerate() {
                m[c][r] = *value as f32;
            }
        }
        m
    }

    /// Turns the view: dragging right turns the scene with the pointer.
    pub fn orbit(&mut self, dx_pixels: f64, dy_pixels: f64) {
        self.yaw -= dx_pixels * 0.008;
        self.pitch = (self.pitch + dy_pixels * 0.008).clamp(-1.5607, 1.5607);
    }

    /// Moves the target with the pointer.
    pub fn pan(&mut self, dx_pixels: f64, dy_pixels: f64, pixels_high: f64) {
        let (_, right, up) = self.basis();
        let mm = self.height / pixels_high;
        for i in 0..3 {
            self.target[i] -= (right[i] * dx_pixels - up[i] * dy_pixels) * mm;
        }
    }

    /// Zooms by `factor` (> 1 closer) keeping the scene point under `at` (view mm from
    /// the centre) where it is.
    pub fn zoom(&mut self, factor: f64, at: (f64, f64)) {
        let (_, right, up) = self.basis();
        let keep = 1.0 - 1.0 / factor;
        for i in 0..3 {
            self.target[i] += (right[i] * at.0 + up[i] * at.1) * keep;
        }
        self.height = (self.height / factor).clamp(1e-3, 1e7);
    }

    /// Frames the box (low, high corners) on a view `aspect` (width / height) wide.
    pub fn fit(&mut self, low: V3, high: V3, aspect: f64) {
        self.target = [0, 1, 2].map(|i| 0.5 * (low[i] + high[i]));
        let corners = (0..8).map(|k| {
            [
                if k & 1 == 0 { low[0] } else { high[0] },
                if k & 2 == 0 { low[1] } else { high[1] },
                if k & 4 == 0 { low[2] } else { high[2] },
            ]
        });
        let (mut w, mut h) = (0.0_f64, 0.0_f64);
        for c in corners {
            let (x, y, _) = self.view(c);
            w = w.max(x.abs());
            h = h.max(y.abs());
        }
        self.height = (2.0 * h).max(2.0 * w / aspect.max(1e-6)).max(1e-3) * 1.1;
    }

    /// Looking down: x to the right, y up.
    pub fn top(&mut self) {
        self.yaw = -core::f64::consts::FRAC_PI_2;
        self.pitch = 1.5607;
    }

    /// Looking along +y: x to the right, z up.
    pub fn front(&mut self) {
        self.yaw = -core::f64::consts::FRAC_PI_2;
        self.pitch = 0.0;
    }

    /// Looking along −x: y to the right, z up.
    pub fn right(&mut self) {
        self.yaw = 0.0;
        self.pitch = 0.0;
    }

    /// The isometric view, from (1, −1, 1).
    pub fn isometric(&mut self) {
        self.yaw = -core::f64::consts::FRAC_PI_4;
        self.pitch = (1.0_f64 / 2f64.sqrt()).atan();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
    }

    #[test]
    fn standard_views_put_the_axes_where_cad_puts_them() {
        let mut c = Camera::default();
        c.front();
        let at = |c: &Camera, p: V3| {
            let (x, y, _) = c.view(p);
            (x, y)
        };
        // Front: x right, z up.
        assert!(close(at(&c, [1.0, 0.0, 0.0]), (1.0, 0.0)));
        assert!(close(at(&c, [0.0, 0.0, 1.0]), (0.0, 1.0)));
        // Right: y right, z up.
        c.right();
        assert!(close(at(&c, [0.0, 1.0, 0.0]), (1.0, 0.0)));
        // Top: x right, y up (nearly: the pitch stops just short of 90°).
        c.top();
        let (x, y) = at(&c, [0.0, 1.0, 0.0]);
        assert!(x.abs() < 1e-6 && (y - 1.0).abs() < 1e-4);
        // Isometric: the three axes at equal length on the screen.
        c.isometric();
        let lengths: Vec<f64> = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
            .iter()
            .map(|&p| {
                let (x, y) = at(&c, p);
                x.hypot(y)
            })
            .collect();
        assert!((lengths[0] - lengths[1]).abs() < 1e-9 && (lengths[1] - lengths[2]).abs() < 1e-9);
    }

    #[test]
    fn fit_frames_the_box_and_zoom_keeps_the_point_under_the_pointer() {
        let mut c = Camera::default();
        c.front();
        c.fit([0.0, 0.0, 0.0], [100.0, 10.0, 50.0], 2.0);
        assert_eq!(c.target, [50.0, 5.0, 25.0]);
        // 100 mm wide on a 2:1 view needs 50 mm of height; 50 mm high needs 50: × 1.1.
        assert!((c.height - 55.0).abs() < 1e-9);
        // The point under the pointer stays on the same pixel.
        let p = [80.0, 0.0, 40.0];
        let (x, y, _) = c.view(p);
        let before = c.to_screen(p, (400.0, 200.0), 400.0);
        c.zoom(2.0, (x, y));
        let after = c.to_screen(p, (400.0, 200.0), 400.0);
        assert!((before.0 - after.0).abs() < 1e-9 && (before.1 - after.1).abs() < 1e-9);
        assert!((c.height - 27.5).abs() < 1e-9);
        // Panning by the view's height in pixels moves the target by its height in mm.
        let t = c.target;
        c.pan(100.0, 0.0, 100.0);
        assert!((c.target[0] - (t[0] - c.height)).abs() < 1e-9);
    }

    /// A scene point through the matrix, to pixels and depth.
    fn through(c: &Camera, p: V3, centre: (f64, f64), high: f64, aspect: f64) -> (f64, f64, f64) {
        let m = c.matrix(aspect, 100.0);
        let v = [p[0], p[1], p[2], 1.0];
        let row = |r: usize| (0..4).map(|k| m[k][r] as f64 * v[k]).sum::<f64>();
        let (x, y, z, w) = (row(0), row(1), row(2), row(3));
        (
            centre.0 + x / w * 0.5 * high * aspect,
            centre.1 - y / w * 0.5 * high,
            z / w,
        )
    }

    #[test]
    fn the_matrix_agrees_with_the_screen_mapping_and_rays() {
        let centre = (300.0, 200.0);
        let (high, aspect) = (400.0, 1.5);
        for perspective in [false, true] {
            let c = Camera {
                perspective,
                target: [10.0, -5.0, 3.0],
                height: 120.0,
                ..Camera::default()
            };
            let near_point = [30.0, 0.0, 20.0];
            let far_point = [-20.0, 10.0, -30.0];
            for p in [near_point, far_point, c.target] {
                let (x, y, depth) = through(&c, p, centre, high, aspect);
                let (sx, sy) = c.to_screen(p, centre, high);
                assert!(
                    (x - sx).abs() < 1e-3 && (y - sy).abs() < 1e-3,
                    "{perspective}"
                );
                assert!((0.0..=1.0).contains(&depth), "{perspective}: depth {depth}");
                // The ray through that pixel passes through the point.
                let (o, d) = c.ray((sx, sy), centre, high);
                let to = sub(p, o);
                let along = dot(to, d);
                let miss = sub(to, d.map(|k| k * along));
                assert!(dot(miss, miss).sqrt() < 1e-6, "{perspective}");
            }
            // Nearer the viewer, smaller depth.
            let (back, _, _) = c.basis();
            let nearer = [0, 1, 2].map(|i| c.target[i] + back[i] * 10.0);
            assert!(
                through(&c, nearer, centre, high, aspect).2
                    < through(&c, c.target, centre, high, aspect).2
            );
        }
    }
}
