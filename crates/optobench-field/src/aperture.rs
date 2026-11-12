//! Apertures as amplitude masks with exact edges: a grid cell wholly inside passes, one
//! wholly outside blocks, and one an edge crosses passes the fraction of its area that
//! is open (found by subdividing that cell), so a circle of radius a passes π a² to the
//! sampling of its rim, not to the staircase of whole cells.

/// An opening (or a stop) in the plane of a field, in its local x, y (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Aperture {
    /// Open inside a circle.
    Circle {
        /// Radius (mm).
        radius: f64,
        /// Centre (mm).
        centre: (f64, f64),
    },
    /// Open between two circles about the same centre (a central obscuration).
    Annulus {
        /// Inner radius (mm): blocked inside.
        inner: f64,
        /// Outer radius (mm).
        outer: f64,
        /// Centre (mm).
        centre: (f64, f64),
    },
    /// Open inside a rectangle.
    Rectangle {
        /// Half-widths along x and y (mm).
        half: (f64, f64),
        /// Centre (mm).
        centre: (f64, f64),
    },
    /// A square plate with a round hole (an iris on its mount): open in the hole and
    /// outside the plate.
    Plate {
        /// Hole radius (mm).
        hole: f64,
        /// Half the plate's side (mm).
        half: f64,
        /// Centre (mm).
        centre: (f64, f64),
    },
    /// Open outside a circle (a stop on the axis, a mirror's hole seen from behind).
    Stop {
        /// Radius (mm).
        radius: f64,
        /// Centre (mm).
        centre: (f64, f64),
    },
}

/// Inside (true), outside (false), or crossed by an edge (None), for a square cell of
/// half-width `h` about (x, y).
fn classify(aperture: &Aperture, x: f64, y: f64, h: f64) -> Option<bool> {
    let circle = |radius: f64, centre: (f64, f64)| -> Option<bool> {
        let (dx, dy) = ((x - centre.0).abs(), (y - centre.1).abs());
        let near = (dx - h).max(0.0).hypot((dy - h).max(0.0));
        let far = (dx + h).hypot(dy + h);
        if far <= radius {
            Some(true)
        } else if near >= radius {
            Some(false)
        } else {
            None
        }
    };
    match *aperture {
        Aperture::Circle { radius, centre } => circle(radius, centre),
        Aperture::Stop { radius, centre } => circle(radius, centre).map(|inside| !inside),
        Aperture::Annulus {
            inner,
            outer,
            centre,
        } => match (circle(outer, centre), circle(inner, centre)) {
            (Some(false), _) | (_, Some(true)) => Some(false),
            (Some(true), Some(false)) => Some(true),
            _ => None,
        },
        Aperture::Plate { hole, half, centre } => {
            let (dx, dy) = ((x - centre.0).abs(), (y - centre.1).abs());
            // Wholly outside the plate's square, or wholly in the hole.
            if dx - h >= half || dy - h >= half {
                return Some(true);
            }
            let inside_square = dx + h <= half && dy + h <= half;
            match circle(hole, centre) {
                Some(true) => Some(true),
                Some(false) if inside_square => Some(false),
                _ => None,
            }
        }
        Aperture::Rectangle { half, centre } => {
            let (dx, dy) = ((x - centre.0).abs(), (y - centre.1).abs());
            if dx + h <= half.0 && dy + h <= half.1 {
                Some(true)
            } else if dx - h >= half.0 || dy - h >= half.1 {
                Some(false)
            } else {
                None
            }
        }
    }
}

fn open_at(aperture: &Aperture, x: f64, y: f64) -> bool {
    classify(aperture, x, y, 0.0).unwrap_or(true)
}

/// Subdivisions per side of a cell an edge crosses.
const SUBDIVISIONS: usize = 32;

impl Aperture {
    /// The open fraction of the square cell of side `size` centred on (x, y).
    pub fn transmission(&self, x: f64, y: f64, size: f64) -> f64 {
        match classify(self, x, y, size / 2.0) {
            Some(true) => 1.0,
            Some(false) => 0.0,
            None => {
                let s = size / SUBDIVISIONS as f64;
                let mut open = 0;
                for i in 0..SUBDIVISIONS {
                    for j in 0..SUBDIVISIONS {
                        let px = x - size / 2.0 + (i as f64 + 0.5) * s;
                        let py = y - size / 2.0 + (j as f64 + 0.5) * s;
                        if open_at(self, px, py) {
                            open += 1;
                        }
                    }
                }
                f64::from(open) / (SUBDIVISIONS * SUBDIVISIONS) as f64
            }
        }
    }

    /// The open area (mm²), from the shape.
    pub fn area(&self) -> f64 {
        let pi = core::f64::consts::PI;
        match *self {
            Aperture::Circle { radius, .. } => pi * radius * radius,
            Aperture::Annulus { inner, outer, .. } => pi * (outer * outer - inner * inner),
            Aperture::Rectangle { half, .. } => 4.0 * half.0 * half.1,
            Aperture::Stop { .. } | Aperture::Plate { .. } => f64::INFINITY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_coarse_grid_passes_the_exact_area_of_a_circle() {
        // Cells of 0.1 mm over a circle of radius 0.537 mm: whole cells alone would be
        // off by several per cent; open fractions get the area to the subdivision.
        let aperture = Aperture::Circle {
            radius: 0.537,
            centre: (0.013, -0.021),
        };
        let size = 0.1;
        let mut area = 0.0;
        for i in -10..=10 {
            for j in -10..=10 {
                area += aperture.transmission(i as f64 * size, j as f64 * size, size) * size * size;
            }
        }
        let exact = aperture.area();
        assert!((area / exact - 1.0).abs() < 2e-4, "{area} vs {exact}");
    }

    #[test]
    fn annuli_rectangles_and_stops() {
        let annulus = Aperture::Annulus {
            inner: 0.2,
            outer: 0.5,
            centre: (0.0, 0.0),
        };
        assert_eq!(annulus.transmission(0.0, 0.0, 0.01), 0.0);
        assert_eq!(annulus.transmission(0.35, 0.0, 0.01), 1.0);
        assert_eq!(annulus.transmission(0.7, 0.0, 0.01), 0.0);
        let rectangle = Aperture::Rectangle {
            half: (0.3, 0.1),
            centre: (0.0, 0.0),
        };
        // A cell straddling the x edge half in.
        assert!((rectangle.transmission(0.3, 0.0, 0.02) - 0.5).abs() < 1e-12);
        let stop = Aperture::Stop {
            radius: 0.2,
            centre: (0.0, 0.0),
        };
        assert_eq!(stop.transmission(0.0, 0.0, 0.01), 0.0);
        assert_eq!(stop.transmission(1.0, 0.0, 0.01), 1.0);
    }
}
