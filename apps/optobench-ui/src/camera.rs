//! The 3D view's camera: orthographic, orbiting a target in the scene frame (Z up), with
//! SolidCraft's standard views. Pure geometry, so it is tested without a window.

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

/// An orthographic camera looking at `target` from the direction given by yaw (about z,
/// from +x) and pitch (above the x–y plane); `height` is the scene height the view shows
/// (mm).
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
}

impl Default for Camera {
    fn default() -> Self {
        let mut c = Self {
            yaw: 0.0,
            pitch: 0.0,
            target: [0.0; 3],
            height: 200.0,
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

    /// A scene point on a screen rectangle (`centre`, height in pixels): pixel x to the
    /// right, y down.
    pub fn to_screen(self, p: V3, centre: (f64, f64), pixels_high: f64) -> (f64, f64) {
        let (x, y, _) = self.view(p);
        let scale = pixels_high / self.height;
        (centre.0 + x * scale, centre.1 - y * scale)
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
}
