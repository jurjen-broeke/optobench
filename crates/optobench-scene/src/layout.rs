//! Placing items: planar layouts along a folded optical axis, and table layouts on a
//! breadboard grid. Both produce a [`Frame`] per item whose local +z is the optical axis
//! at that point (a mirror's normal for mirrors). The scene frame is Z up.

use optobench_math::{Frame, Rotation, Vec3, radians_from_degrees};

use crate::SceneError;

/// A frame at `origin` with +z along `axis`, local x horizontal where possible (so a
/// detector's local y points up when the axis is horizontal).
pub fn frame_along(origin: Vec3, axis: Vec3) -> Option<Frame> {
    let axis = axis.normalized()?;
    let horizontal = Vec3::Z.cross(axis);
    let hint = if horizontal.length() > 1e-9 {
        horizontal
    } else {
        Vec3::X
    };
    Frame::new(origin, axis, hint)
}

/// A planar layout: the optical axis starts at `origin` heading along `direction`
/// (horizontal), and turns about the vertical at each fold.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanarLayout {
    /// Start of the axis (mm, scene frame).
    pub origin: Vec3,
    /// Starting direction of the axis.
    pub direction: Vec3,
    /// Folds as (distance along the axis in mm, angle in degrees; positive turns
    /// counter-clockwise seen from above), in any order.
    pub folds: Vec<(f64, f64)>,
}

impl Default for PlanarLayout {
    fn default() -> Self {
        Self {
            origin: Vec3::ZERO,
            direction: Vec3::X,
            folds: Vec::new(),
        }
    }
}

impl PlanarLayout {
    fn sorted_folds(&self) -> Vec<(f64, f64)> {
        let mut folds = self.folds.clone();
        folds.sort_by(|a, b| a.0.total_cmp(&b.0));
        folds
    }

    fn turn(direction: Vec3, degrees: f64) -> Vec3 {
        Rotation::from_axis_angle(Vec3::Z, radians_from_degrees(degrees))
            .map_or(direction, |r| r.apply(direction))
    }

    /// The point on the axis at distance `at`, and the axis direction there. At a fold
    /// itself, the direction is the incoming one.
    pub fn point(&self, at: f64) -> Result<(Vec3, Vec3), SceneError> {
        let mut position = self.origin;
        let mut direction = self.direction.normalized().ok_or(SceneError::Invalid(
            "planar layout direction has zero length".into(),
        ))?;
        let mut travelled = 0.0;
        for (fold_at, angle) in self.sorted_folds() {
            if fold_at >= at {
                break;
            }
            position = position + direction * (fold_at - travelled);
            travelled = fold_at;
            direction = Self::turn(direction, angle);
        }
        Ok((position + direction * (at - travelled), direction))
    }

    /// The frame of an element at `at` whose axis follows the optical axis.
    pub fn frame(&self, at: f64) -> Result<Frame, SceneError> {
        let (point, direction) = self.point(at)?;
        frame_along(point, direction).ok_or(SceneError::Invalid("degenerate axis".into()))
    }

    /// The frame of the fold mirror at `at` (matched within 1e-9 mm): +z along the
    /// mirror normal, which bisects the reversed incoming and the outgoing directions.
    pub fn fold_frame(&self, at: f64) -> Result<Frame, SceneError> {
        let angle = self
            .folds
            .iter()
            .find(|(fold_at, _)| (*fold_at - at).abs() < 1e-9)
            .map(|(_, angle)| *angle)
            .ok_or(SceneError::Invalid(format!("no fold at {at} mm")))?;
        let (point, incoming) = self.point(at)?;
        let outgoing = Self::turn(incoming, angle);
        let normal = outgoing - incoming;
        frame_along(point, normal).ok_or(SceneError::Invalid(format!(
            "fold at {at} mm has angle {angle}°: a mirror cannot fold by 0°"
        )))
    }

    /// Whether a fold lies strictly between `from` and `to`.
    pub fn fold_between(&self, from: f64, to: f64) -> bool {
        let (low, high) = (from.min(to), from.max(to));
        self.folds.iter().any(|(at, _)| *at > low && *at < high)
    }
}

/// A table layout: positions on a breadboard grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TableLayout {
    /// Distance between holes (mm), e.g. 25 or 25.4.
    pub pitch: f64,
}

impl TableLayout {
    /// The frame of an item at grid hole (`column`, `row`) (fractions allowed), `height`
    /// above the table, with local +z along `axis`.
    pub fn frame(&self, hole: (f64, f64), height: f64, axis: Vec3) -> Result<Frame, SceneError> {
        let origin = Vec3::new(hole.0 * self.pitch, hole.1 * self.pitch, height);
        frame_along(origin, axis).ok_or(SceneError::Invalid("axis has zero length".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-12;

    #[test]
    fn straight_axis_and_one_fold() {
        let layout = PlanarLayout {
            folds: vec![(100.0, 90.0)],
            ..PlanarLayout::default()
        };
        let (p, d) = layout.point(50.0).expect("valid");
        assert!((p - Vec3::new(50.0, 0.0, 0.0)).length() < EPS && (d - Vec3::X).length() < EPS);
        // Past a 90° left turn the axis heads +y.
        let (p, d) = layout.point(130.0).expect("valid");
        assert!((p - Vec3::new(100.0, 30.0, 0.0)).length() < EPS);
        assert!((d - Vec3::Y).length() < EPS);
        // The fold mirror's normal bisects −x and +y.
        let mirror = layout.fold_frame(100.0).expect("valid");
        let normal = mirror.axes()[2];
        assert!((normal - Vec3::new(-1.0, 1.0, 0.0) * (1.0 / 2f64.sqrt())).length() < EPS);
        assert!(layout.fold_between(90.0, 110.0) && !layout.fold_between(0.0, 99.0));
    }

    #[test]
    fn frames_keep_local_x_horizontal() {
        let frame = frame_along(Vec3::ZERO, Vec3::X).expect("valid");
        let [x, y, _] = frame.axes();
        assert!(x.z.abs() < EPS && (y - Vec3::Z).length() < EPS);
        // A vertical axis falls back to x along scene +x.
        let up = frame_along(Vec3::ZERO, Vec3::Z).expect("valid");
        assert!((up.axes()[0] - Vec3::X).length() < EPS);
    }

    #[test]
    fn table_holes_scale_with_pitch() {
        let table = TableLayout { pitch: 25.0 };
        let frame = table.frame((2.0, -1.0), 75.0, Vec3::X).expect("valid");
        assert!((frame.origin() - Vec3::new(50.0, -25.0, 75.0)).length() < EPS);
    }
}
