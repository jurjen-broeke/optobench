//! The breadboard of a table layout, drawn under the items (M16): a plate with its holes
//! as filled circles. From the file's `[table]`: the pitch, and the board's `columns`,
//! `rows` and `corner` hole when given; otherwise a board around the items with two holes
//! to spare, so a new layout shows a board at once.

use serde_json::Value;

use crate::camera::V3;
use crate::document::Scene;

/// Holes to spare around the items on a board the file does not size.
const MARGIN: i64 = 2;

/// Hole radius as a share of the pitch (an M6 hole on a 25 mm grid).
const HOLE: f64 = 0.12;

/// The plate's thickness (mm), drawn below the holes.
const THICKNESS: f64 = 12.7;

/// A breadboard: holes `columns` × `rows` from hole `corner`, `pitch` apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Board {
    /// Distance between holes (mm).
    pub pitch: f64,
    /// Holes along x.
    pub columns: i64,
    /// Holes along y.
    pub rows: i64,
    /// The hole (column, row) at the corner nearest the origin.
    pub corner: (i64, i64),
    /// Whether the file gives the size (else it is fitted around the items).
    pub given: bool,
}

impl Board {
    /// The board of a table layout; `None` for other designs.
    pub fn of(design: &Value, scene: &Scene) -> Option<Board> {
        if design["layout"] != "table" {
            return None;
        }
        let table = &design["table"];
        let pitch = table["pitch"].as_f64().filter(|p| *p > 0.0)?;
        if let (Some(columns), Some(rows)) = (table["columns"].as_i64(), table["rows"].as_i64()) {
            let corner = (
                table["corner"][0].as_i64().unwrap_or(0),
                table["corner"][1].as_i64().unwrap_or(0),
            );
            return Some(Board {
                pitch,
                columns: columns.max(1),
                rows: rows.max(1),
                corner,
                given: true,
            });
        }
        // Around the items and sources, the origin included.
        let mut low = (0_i64, 0_i64);
        let mut high = (0_i64, 0_i64);
        let points = scene
            .items
            .iter()
            .filter_map(|i| i.origin)
            .chain(scene.sources.iter().map(|s| s.origin));
        for p in points {
            let (c, r) = (p[0] / pitch, p[1] / pitch);
            low = (low.0.min(c.floor() as i64), low.1.min(r.floor() as i64));
            high = (high.0.max(c.ceil() as i64), high.1.max(r.ceil() as i64));
        }
        Some(Board {
            pitch,
            columns: high.0 - low.0 + 1 + 2 * MARGIN,
            rows: high.1 - low.1 + 1 + 2 * MARGIN,
            corner: (low.0 - MARGIN, low.1 - MARGIN),
            given: false,
        })
    }

    /// The board's extent in x and y (mm): half a pitch beyond the outer holes.
    pub fn extent(&self) -> ((f64, f64), (f64, f64)) {
        let p = self.pitch;
        let x = (
            (self.corner.0 as f64 - 0.5) * p,
            (self.corner.0 + self.columns) as f64 * p - 0.5 * p,
        );
        let y = (
            (self.corner.1 as f64 - 0.5) * p,
            (self.corner.1 + self.rows) as f64 * p - 0.5 * p,
        );
        (x, y)
    }

    /// `bounds` (low, high) widened to take in the board.
    pub fn around(&self, bounds: Option<(V3, V3)>) -> (V3, V3) {
        let ((x0, x1), (y0, y1)) = self.extent();
        let (mut low, mut high) = ([x0, y0, -THICKNESS], [x1, y1, 0.0]);
        if let Some((lo, hi)) = bounds {
            for k in 0..3 {
                low[k] = low[k].min(lo[k]);
                high[k] = high[k].max(hi[k]);
            }
        }
        (low, high)
    }

    /// Triangles for the view: the plate (top a hair below the table's surface, its
    /// sides) and the holes, as (positions, normals, indices) each.
    #[allow(clippy::type_complexity)]
    pub fn triangles(&self) -> [(Vec<V3>, Vec<V3>, Vec<u32>); 2] {
        let ((x0, x1), (y0, y1)) = self.extent();
        let top = -0.3;
        let bottom = -THICKNESS;
        let mut plate = (Vec::new(), Vec::new(), Vec::new());
        let mut quad = |corners: [V3; 4], normal: V3| {
            let (p, n, t): &mut (Vec<V3>, Vec<V3>, Vec<u32>) = &mut plate;
            let base = p.len() as u32;
            p.extend(corners);
            n.extend([normal; 4]);
            t.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        };
        quad(
            [[x0, y0, top], [x1, y0, top], [x1, y1, top], [x0, y1, top]],
            [0.0, 0.0, 1.0],
        );
        quad(
            [
                [x0, y0, bottom],
                [x1, y0, bottom],
                [x1, y0, top],
                [x0, y0, top],
            ],
            [0.0, -1.0, 0.0],
        );
        quad(
            [
                [x1, y1, bottom],
                [x0, y1, bottom],
                [x0, y1, top],
                [x1, y1, top],
            ],
            [0.0, 1.0, 0.0],
        );
        quad(
            [
                [x0, y1, bottom],
                [x0, y0, bottom],
                [x0, y0, top],
                [x0, y1, top],
            ],
            [-1.0, 0.0, 0.0],
        );
        quad(
            [
                [x1, y0, bottom],
                [x1, y1, bottom],
                [x1, y1, top],
                [x1, y0, top],
            ],
            [1.0, 0.0, 0.0],
        );
        let mut holes = (Vec::new(), Vec::new(), Vec::new());
        let r = HOLE * self.pitch;
        const SIDES: u32 = 16;
        for column in self.corner.0..self.corner.0 + self.columns {
            for row in self.corner.1..self.corner.1 + self.rows {
                let (cx, cy) = (column as f64 * self.pitch, row as f64 * self.pitch);
                let (p, n, t) = &mut holes;
                let centre = p.len() as u32;
                p.push([cx, cy, 0.0]);
                n.push([0.0, 0.0, 1.0]);
                for k in 0..SIDES {
                    let a = std::f64::consts::TAU * f64::from(k) / f64::from(SIDES);
                    p.push([cx + r * a.cos(), cy + r * a.sin(), 0.0]);
                    n.push([0.0, 0.0, 1.0]);
                    t.extend([centre, centre + 1 + k, centre + 1 + (k + 1) % SIDES]);
                }
            }
        }
        [plate, holes]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Source;
    use serde_json::json;

    #[test]
    fn a_board_is_given_or_fitted_around_the_items() {
        let scene = Scene {
            sources: vec![Source {
                id: "S".into(),
                origin: [0.0, 0.0, 75.0],
                axis: [1.0, 0.0, 0.0],
            }],
            ..Scene::default()
        };
        let mut design = json!({ "layout": "table", "table": { "pitch": 25.0 } });
        let fitted = Board::of(&design, &scene).expect("a table");
        assert_eq!(
            (fitted.columns, fitted.rows, fitted.corner),
            (5, 5, (-2, -2))
        );
        assert!(!fitted.given);
        design["table"] = json!({ "pitch": 25.0, "columns": 12, "rows": 18, "corner": [-1, 0] });
        let given = Board::of(&design, &scene).expect("a table");
        assert_eq!(given.extent(), ((-37.5, 262.5), (-12.5, 437.5)));
        let [plate, holes] = given.triangles();
        assert_eq!(plate.2.len(), 5 * 6);
        assert_eq!(holes.0.len(), 12 * 18 * 17);
        assert!(Board::of(&json!({ "layout": "planar" }), &scene).is_none());
    }
}
