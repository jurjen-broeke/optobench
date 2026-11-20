//! A singlet lens as the lens editor designs it (M16): its type, focal length, radius,
//! centre thickness, glass index and diameter kept consistent with each other.
//!
//! Thick-lens relations (E. Hecht, Optics, 5th ed., §6.2), light along +z, a radius
//! positive when its centre lies further along, curvatures c = 1/R:
//!
//! - 1/f = (n − 1) (c₁ − c₂ + (n − 1) t c₁ c₂ / n)
//! - a plano lens: f = R/(n − 1) for its curved surface, whatever its thickness
//! - an equi-convex or equi-concave lens (R₁ = −R₂ = R):
//!   R = (n − 1) f (1 + √(1 − t/(n f)))
//! - otherwise, with R₁ kept: c₂ = (c₁ − 1/((n − 1) f)) / (1 − (n − 1) t c₁ / n)
//!
//! A surface's sag at height h is z(h) = R − sign(R) √(R² − h²) (along +z from its
//! vertex); the edge thickness at the rim is t + z₂(h) − z₁(h). The diameter is kept
//! where the edge stays at least [`MIN_EDGE`] thick and both surfaces reach the rim.

/// The thinnest edge a lens keeps (mm).
pub const MIN_EDGE: f64 = 0.5;

/// What a singlet looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Both sides convex, equal radii.
    Biconvex,
    /// One side convex, the other flat.
    PlanoConvex,
    /// One side concave, the other flat.
    PlanoConcave,
    /// Both sides concave, equal radii.
    Biconcave,
    /// Both radii on the same side: one convex, one concave.
    Meniscus,
}

impl Shape {
    /// Every shape, for the editor's list.
    pub const ALL: [Shape; 5] = [
        Shape::Biconvex,
        Shape::PlanoConvex,
        Shape::PlanoConcave,
        Shape::Biconcave,
        Shape::Meniscus,
    ];

    /// Its name.
    pub fn name(self) -> &'static str {
        match self {
            Shape::Biconvex => "biconvex",
            Shape::PlanoConvex => "plano-convex",
            Shape::PlanoConcave => "plano-concave",
            Shape::Biconcave => "biconcave",
            Shape::Meniscus => "meniscus",
        }
    }

    /// Whether it gathers light (a positive focal length) as made by default.
    fn positive(self) -> bool {
        matches!(self, Shape::Biconvex | Shape::PlanoConvex | Shape::Meniscus)
    }
}

/// A singlet: its two radii (infinite for flat), centre thickness and glass index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Singlet {
    /// First radius (mm), the surface light meets first.
    pub r1: f64,
    /// Second radius (mm).
    pub r2: f64,
    /// Centre thickness (mm).
    pub t: f64,
    /// Glass index.
    pub n: f64,
}

fn curvature(r: f64) -> f64 {
    if r.is_finite() && r != 0.0 {
        1.0 / r
    } else {
        0.0
    }
}

fn radius(c: f64) -> f64 {
    if c.abs() < 1e-12 {
        f64::INFINITY
    } else {
        1.0 / c
    }
}

impl Singlet {
    /// Its shape, and whether it is turned round from the way [`Singlet::design`] makes
    /// it (a plano lens with its flat side first, a meniscus concave side first).
    pub fn shape(&self) -> (Shape, bool) {
        let (c1, c2) = (curvature(self.r1), curvature(self.r2));
        match (c1 == 0.0, c2 == 0.0) {
            (true, true) => (Shape::PlanoConvex, false),
            (false, true) => (
                if c1 > 0.0 {
                    Shape::PlanoConvex
                } else {
                    Shape::PlanoConcave
                },
                false,
            ),
            (true, false) => (
                if c2 < 0.0 {
                    Shape::PlanoConvex
                } else {
                    Shape::PlanoConcave
                },
                true,
            ),
            (false, false) => match (c1 > 0.0, c2 < 0.0) {
                (true, true) => (Shape::Biconvex, false),
                (false, false) => (Shape::Biconcave, false),
                // Both centres on one side: convex first (c₁ > 0, c₂ > 0) or concave
                // first (both negative), which is the meniscus turned round.
                (true, false) => (Shape::Meniscus, false),
                (false, true) => (Shape::Meniscus, true),
            },
        }
    }

    /// Its focal length (mm); infinite for no power.
    pub fn focal_length(&self) -> f64 {
        let (c1, c2, n, t) = (curvature(self.r1), curvature(self.r2), self.n, self.t);
        let power = (n - 1.0) * (c1 - c2 + (n - 1.0) * t * c1 * c2 / n);
        if power.abs() < 1e-15 {
            f64::INFINITY
        } else {
            1.0 / power
        }
    }

    /// The same lens turned round.
    pub fn flipped(&self) -> Singlet {
        let back = |r: f64| if r.is_finite() { -r } else { r };
        Singlet {
            r1: back(self.r2),
            r2: back(self.r1),
            ..*self
        }
    }

    /// A lens of `shape` with focal length `f` (its sign taken from the shape: convex
    /// shapes positive, concave ones negative; a meniscus keeps the sign given), this
    /// lens's thickness and index, turned round if `flipped`. A meniscus keeps the
    /// radius of its convex surface and bends the other. `None` when there is none.
    pub fn design(&self, shape: Shape, f: f64, flipped: bool) -> Option<Singlet> {
        let (n, t) = (self.n, self.t);
        if !(n > 1.0 && f.is_finite() && f != 0.0 && t > 0.0) {
            return None;
        }
        let f = match shape {
            Shape::Meniscus => f,
            s if s.positive() => f.abs(),
            _ => -f.abs(),
        };
        let made = match shape {
            Shape::PlanoConvex | Shape::PlanoConcave => Singlet {
                r1: (n - 1.0) * f,
                r2: f64::INFINITY,
                t,
                n,
            },
            Shape::Biconvex | Shape::Biconcave => {
                let root = 1.0 - t / (n * f);
                if root < 0.0 {
                    return None;
                }
                let r = (n - 1.0) * f * (1.0 + root.sqrt());
                Singlet {
                    r1: r,
                    r2: -r,
                    t,
                    n,
                }
            }
            Shape::Meniscus => {
                // Keep this lens's convex first radius when it reaches f; else start from
                // (n − 1)|f|/2 for a positive meniscus (its first surface must bend more
                // than the whole lens), (n − 1)|f| for a negative one.
                let solve = |r1: f64| -> Option<Singlet> {
                    let c1 = 1.0 / r1;
                    let denominator = 1.0 - (n - 1.0) * t * c1 / n;
                    if denominator.abs() < 1e-12 {
                        return None;
                    }
                    let c2 = (c1 - 1.0 / ((n - 1.0) * f)) / denominator;
                    (c2 > 0.0).then(|| Singlet {
                        r1,
                        r2: radius(c2),
                        t,
                        n,
                    })
                };
                let unturned = if self.shape().1 {
                    self.flipped()
                } else {
                    *self
                };
                let kept = (unturned.shape().0 == Shape::Meniscus && unturned.r1.is_finite())
                    .then(|| solve(unturned.r1.abs()))
                    .flatten();
                let start = if f > 0.0 {
                    0.5 * (n - 1.0) * f
                } else {
                    (n - 1.0) * f.abs()
                };
                kept.or_else(|| solve(start))?
            }
        };
        Some(if flipped { made.flipped() } else { made })
    }

    /// The sag of a surface of radius `r` at height `h` (mm, along +z), if it reaches it.
    fn sag(r: f64, h: f64) -> Option<f64> {
        if !r.is_finite() {
            return Some(0.0);
        }
        let inside = r * r - h * h;
        (inside > 0.0).then(|| r - r.signum() * inside.sqrt())
    }

    /// The edge thickness at diameter `d` (mm), if both surfaces reach it.
    pub fn edge(&self, d: f64) -> Option<f64> {
        let h = 0.5 * d;
        Some(self.t + Singlet::sag(self.r2, h)? - Singlet::sag(self.r1, h)?)
    }

    /// The largest diameter (mm, to 0.1 mm) that keeps the edge at least [`MIN_EDGE`]
    /// thick and both surfaces reaching it.
    pub fn max_diameter(&self) -> f64 {
        let fits = |d: f64| self.edge(d).is_some_and(|e| e >= MIN_EDGE);
        let (mut low, mut high) = (0.0_f64, 1000.0_f64);
        if fits(high) {
            return high;
        }
        for _ in 0..60 {
            let mid = 0.5 * (low + high);
            if fits(mid) {
                low = mid;
            } else {
                high = mid;
            }
        }
        (low * 10.0).floor() / 10.0
    }
}

/// A cemented achromatic doublet (M17d): a crown glass first, equi-convex, then a flint,
/// its three radii (mm) and two centre thicknesses.
///
/// Designed from its focal length f and the two glasses' nd and Abbe numbers Vd (each
/// as [`optobench_materials::ModelGlass`]): first as thin lenses (E. Hecht, Optics, 5th
/// ed., ch. 6, achromatic doublets), the powers φ₁ = φ V₁/(V₁ − V₂) and
/// φ₂ = −φ V₂/(V₁ − V₂) so that φ₁ + φ₂ = φ and φ₁/V₁ + φ₂/V₂ = 0; the crown
/// equi-convex, c₁ = −c₂ = φ₁/(2 (n₁ − 1)), the flint on it, c₃ = c₂ − φ₂/(n₂ − 1).
/// Then with the thicknesses: c₁ (= −c₂) and c₃ by Newton's method on the paraxial
/// trace until the power at the d line is φ and the powers at F and C are equal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Achromat {
    /// Radii (mm), light along +z.
    pub radii: [f64; 3],
    /// Centre thicknesses of the crown and the flint (mm).
    pub thickness: [f64; 2],
}

/// A glass's index at a wavelength (µm) from its nd and Vd.
fn model_index(glass: (f64, f64), wavelength: f64) -> Option<f64> {
    use optobench_materials::Material;
    optobench_materials::ModelGlass::new(glass.0, glass.1)?
        .refractive_index(wavelength)
        .ok()
}

/// The paraxial power (1/mm) of curvatures `c` between air, glass 1, glass 2 and air
/// at their indices `n`, the glasses `t` thick: a ray at height 1 parallel to the axis,
/// refracted as n′u′ = nu − y c (n′ − n) and carried on as y += t u′; the power is minus
/// its last slope in air.
fn power(c: [f64; 3], t: [f64; 2], n: [f64; 2]) -> f64 {
    let media = [1.0, n[0], n[1], 1.0];
    let (mut y, mut nu) = (1.0, 0.0);
    for k in 0..3 {
        nu -= y * c[k] * (media[k + 1] - media[k]);
        if k < 2 {
            y += t[k] * nu / media[k + 1];
        }
    }
    -nu
}

impl Achromat {
    /// Thin-lens powers (φ₁, φ₂) of the crown and the flint for a doublet of focal
    /// length `f` from Abbe numbers `v1`, `v2`.
    pub fn thin_powers(f: f64, v1: f64, v2: f64) -> (f64, f64) {
        let phi = 1.0 / f;
        (phi * v1 / (v1 - v2), -phi * v2 / (v1 - v2))
    }

    /// The doublet of focal length `f` (mm, positive) from a crown and a flint, each
    /// (nd, Vd), `t` thick; `None` when the glasses' Abbe numbers are too close (within
    /// 5) or the design does not settle.
    pub fn design(f: f64, crown: (f64, f64), flint: (f64, f64), t: [f64; 2]) -> Option<Self> {
        use optobench_materials::{LINE_C, LINE_D, LINE_F};
        if !(f.is_finite() && f > 0.0 && t[0] > 0.0 && t[1] > 0.0)
            || (crown.1 - flint.1).abs() < 5.0
        {
            return None;
        }
        let at = |l: f64| Some([model_index(crown, l)?, model_index(flint, l)?]);
        let (nd, nf, nc) = (at(LINE_D)?, at(LINE_F)?, at(LINE_C)?);
        let (p1, p2) = Self::thin_powers(f, crown.1, flint.1);
        let c1 = p1 / (2.0 * (crown.0 - 1.0));
        let mut x = [c1, -c1 - p2 / (flint.0 - 1.0)];
        let curvatures = |x: [f64; 2]| [x[0], -x[0], x[1]];
        let residual = |x: [f64; 2]| {
            let c = curvatures(x);
            [
                (power(c, t, nd) - 1.0 / f) * f,
                (power(c, t, nf) - power(c, t, nc)) * f,
            ]
        };
        for _ in 0..50 {
            let r = residual(x);
            if r[0].abs() < 1e-13 && r[1].abs() < 1e-13 {
                let c = curvatures(x);
                return Some(Self {
                    radii: c.map(|c| if c == 0.0 { f64::INFINITY } else { 1.0 / c }),
                    thickness: t,
                });
            }
            // The Jacobian by central differences (the residuals are smooth, and nearly
            // linear in the curvatures).
            let h = 1e-7 * (x[0].abs() + x[1].abs()).max(1e-6);
            let column = |k: usize| {
                let (mut up, mut down) = (x, x);
                up[k] += h;
                down[k] -= h;
                let (a, b) = (residual(up), residual(down));
                [(a[0] - b[0]) / (2.0 * h), (a[1] - b[1]) / (2.0 * h)]
            };
            let (j0, j1) = (column(0), column(1));
            let det = j0[0] * j1[1] - j1[0] * j0[1];
            if det.abs() < 1e-30 {
                return None;
            }
            x[0] -= (r[0] * j1[1] - j1[0] * r[1]) / det;
            x[1] -= (j0[0] * r[1] - r[0] * j0[1]) / det;
        }
        None
    }

    /// Its paraxial focal length (mm) at a wavelength (µm), with these glasses.
    #[cfg(test)]
    pub fn focal_length(
        &self,
        crown: (f64, f64),
        flint: (f64, f64),
        wavelength: f64,
    ) -> Option<f64> {
        let n = [
            model_index(crown, wavelength)?,
            model_index(flint, wavelength)?,
        ];
        let c = self.radii.map(curvature);
        Some(1.0 / power(c, self.thickness, n))
    }

    /// The largest diameter (mm, to 0.1 mm) at which both glasses keep an edge of at
    /// least [`MIN_EDGE`] and every surface reaches.
    pub fn max_diameter(&self) -> f64 {
        let [r1, r2, r3] = self.radii;
        let glass = |r1, r2, t| Singlet { r1, r2, t, n: 1.5 }.max_diameter();
        glass(r1, r2, self.thickness[0]).min(glass(r2, r3, self.thickness[1]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GLASS: Singlet = Singlet {
        r1: 50.0,
        r2: -50.0,
        t: 4.0,
        n: 1.5168,
    };

    #[test]
    fn an_achromat_is_designed_from_its_focal_length() {
        use optobench_materials::{LINE_C, LINE_D, LINE_F};
        // A crown (nd 1.5168, Vd 64.17) and a dense flint (nd 1.6727, Vd 32.25), typical
        // catalogue values, for f 100 mm.
        let (crown, flint) = ((1.5168, 64.17), (1.6727, 32.25));
        // Thin lenses (Hecht, ch. 6): the powers add up to 1/f and their chromatic
        // changes, φ/V, cancel.
        let (p1, p2) = Achromat::thin_powers(100.0, crown.1, flint.1);
        assert!((p1 + p2 - 0.01).abs() < 1e-15);
        assert!((p1 / crown.1 + p2 / flint.1).abs() < 1e-15);
        assert!(p1 > 0.0 && p2 < 0.0);
        // Thick: f at d is 100 mm and the same at F and C, from the paraxial trace.
        let doublet = Achromat::design(100.0, crown, flint, [6.0, 2.5]).expect("a design");
        let f = |l| doublet.focal_length(crown, flint, l).expect("in range");
        assert!((f(LINE_D) - 100.0).abs() < 1e-9, "{}", f(LINE_D));
        assert!((f(LINE_F) - f(LINE_C)).abs() < 1e-9);
        // A singlet of the crown alone with the same f spreads F to C by f/V: the doublet
        // has none of that left; its secondary spectrum (d against F and C) stays small.
        assert!(
            (f(LINE_D) - f(LINE_F)).abs() < 0.1,
            "{}",
            f(LINE_D) - f(LINE_F)
        );
        // An equi-convex crown, cemented to the flint, which curves less after it.
        let [r1, r2, r3] = doublet.radii;
        assert!((r1 + r2).abs() < 1e-9 && r1 > 0.0 && r3 < r2);
        // The thin-lens radius is where it starts; the thicknesses move it a little.
        let thin_r1 = 2.0 * (crown.0 - 1.0) / p1;
        assert!(
            (r1 - thin_r1).abs() / thin_r1 < 0.05,
            "{r1} against {thin_r1}"
        );
        // One inch fits with the edges kept.
        assert!(doublet.max_diameter() >= 25.4, "{}", doublet.max_diameter());
        // Glasses of nearly one Abbe number make no achromat.
        assert!(Achromat::design(100.0, crown, (1.6, 62.0), [6.0, 2.5]).is_none());
    }

    #[test]
    fn each_shape_has_the_focal_length_asked_for_either_way_round() {
        for (shape, f) in [
            (Shape::Biconvex, 100.0),
            (Shape::PlanoConvex, 60.0),
            (Shape::PlanoConcave, -12.0),
            (Shape::Biconcave, -40.0),
            (Shape::Meniscus, 150.0),
        ] {
            for flipped in [false, true] {
                let lens = GLASS
                    .design(shape, f, flipped)
                    .unwrap_or_else(|| panic!("{}", shape.name()));
                assert!(
                    (lens.focal_length() - f).abs() < 1e-9,
                    "{} {flipped}: {lens:?}",
                    shape.name()
                );
                assert_eq!(
                    lens.shape(),
                    (
                        shape,
                        flipped && shape != Shape::Biconvex && shape != Shape::Biconcave
                    ),
                    "{lens:?}"
                );
            }
        }
        // Concave shapes take −|f|.
        let concave = GLASS
            .design(Shape::PlanoConcave, 12.0, false)
            .expect("lens");
        assert!((concave.focal_length() + 12.0).abs() < 1e-9);
        // A plano lens: R = (n − 1) f, curved side first; turned round, flat side first.
        let plano = GLASS.design(Shape::PlanoConvex, 60.0, false).expect("lens");
        assert!((plano.r1 - 0.5168 * 60.0).abs() < 1e-9 && plano.r2.is_infinite());
        let turned = GLASS.design(Shape::PlanoConvex, 60.0, true).expect("lens");
        assert!(turned.r1.is_infinite() && (turned.r2 + 0.5168 * 60.0).abs() < 1e-9);
    }

    #[test]
    fn the_relay_lens_has_its_focal_length() {
        // R ±100 mm, n 1.5, 4 mm: 1/f = 0.5 (2/100 − 0.5 · 4 / (1.5 · 100²)), f 100.6711 mm;
        // designed from that f it has its radii back.
        let lens = Singlet {
            r1: 100.0,
            r2: -100.0,
            t: 4.0,
            n: 1.5,
        };
        assert!((lens.focal_length() - 100.671_140_939_597_3).abs() < 1e-9);
        let back = lens
            .design(Shape::Biconvex, lens.focal_length(), false)
            .expect("lens");
        assert!((back.r1 - 100.0).abs() < 1e-9 && (back.r2 + 100.0).abs() < 1e-9);
        // Too thick for its focal length: none.
        assert!(lens.design(Shape::Biconvex, 2.0, false).is_none());
    }

    #[test]
    fn the_diameter_keeps_an_edge() {
        // A plano-convex R 31.008, 4 mm thick: the edge is 0.5 mm where the sag is 3.5 mm,
        // h = √(R² − (R − 3.5)²).
        let lens = GLASS.design(Shape::PlanoConvex, 60.0, false).expect("lens");
        let r = lens.r1;
        let h = (r * r - (r - 3.5) * (r - 3.5)).sqrt();
        let d = lens.max_diameter();
        assert!(d <= 2.0 * h && 2.0 * h - d < 0.1, "{d} vs {}", 2.0 * h);
        assert!(lens.edge(d).expect("reaches") >= MIN_EDGE);
        // A plano-concave lens is limited by its radius: R −6.2 cannot be 12.7 across.
        let concave = GLASS
            .design(Shape::PlanoConcave, -12.0, false)
            .expect("lens");
        assert!(concave.max_diameter() < 2.0 * concave.r1.abs());
        assert!(concave.edge(25.4).is_none());
    }
}
