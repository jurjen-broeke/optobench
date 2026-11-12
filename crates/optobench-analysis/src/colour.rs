//! Colours for graphs: the series palette and the colour scales of maps, shared by every
//! renderer (SVG files, the interface) so a series or a value has the same colour
//! everywhere.
//!
//! Every colour has one job (docs/design.md, Graphs):
//!
//! - **Series** (identity): eight hues in a fixed order, assigned to series in order and
//!   never cycled. The order keeps neighbours apart for colour-blind readers (protanopia
//!   and deuteranopia); scatter graphs, where any two series can sit side by side, stay
//!   clear only with their first three.
//! - **Sequential** (magnitude, e.g. irradiance): one hue, light (near zero) to dark.
//! - **Diverging** (a difference): two hues of equal lightness at the ends and a neutral
//!   grey at zero.
//! - **Cyclic** (phase): constant lightness, the hue turning once round, so −π and π
//!   meet.
//!
//! Scales are interpolated in OKLab (B. Ottosson, "A perceptual color space for image
//! processing", 2020), where equal steps look equal. Each colour has a light-surface
//! value (SVG files, reports) and a dark-surface value (the interface).

/// A colour, 8-bit sRGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// From `0xRRGGBB`.
    pub const fn hex(v: u32) -> Self {
        Self((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    /// `#rrggbb`.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// OKLab (L, a, b), L from 0 (black) to 1 (white).
    pub fn oklab(self) -> [f64; 3] {
        let linear = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (linear(self.0), linear(self.1), linear(self.2));
        let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
        let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
        let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
        [
            0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
            1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
            0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
        ]
    }

    /// From OKLab, clipped to the sRGB gamut.
    pub fn from_oklab(lab: [f64; 3]) -> Self {
        let [lightness, a, b] = lab;
        let l = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
        let m = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
        let s = (lightness - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
        let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
        let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
        let bl = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s;
        let encode = |c: f64| {
            let c = c.clamp(0.0, 1.0);
            let c = if c <= 0.003_130_8 {
                12.92 * c
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            };
            (c * 255.0).round() as u8
        };
        Self(encode(r), encode(g), encode(bl))
    }
}

/// Which surface a graph is drawn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Light: files and reports.
    Light,
    /// Dark: the interface.
    Dark,
}

/// The ink of a graph's chrome on a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ink {
    /// The plot's background.
    pub surface: Rgb,
    /// Titles and values.
    pub primary: Rgb,
    /// Axis titles, legend text, reference series.
    pub secondary: Rgb,
    /// Tick labels.
    pub muted: Rgb,
    /// Grid lines (hairlines).
    pub grid: Rgb,
    /// Axes and baselines.
    pub axis: Rgb,
}

/// The chrome's ink.
pub fn ink(surface: Surface) -> Ink {
    match surface {
        Surface::Light => Ink {
            surface: Rgb::hex(0xfcfcfb),
            primary: Rgb::hex(0x0b0b0b),
            secondary: Rgb::hex(0x52514e),
            muted: Rgb::hex(0x898781),
            grid: Rgb::hex(0xe1e0d9),
            axis: Rgb::hex(0xc3c2b7),
        },
        Surface::Dark => Ink {
            surface: Rgb::hex(0x1a1a19),
            primary: Rgb::hex(0xf0efec),
            secondary: Rgb::hex(0xc3c2b7),
            muted: Rgb::hex(0x898781),
            grid: Rgb::hex(0x2c2c2a),
            axis: Rgb::hex(0x383835),
        },
    }
}

const SERIES_LIGHT: [u32; 8] = [
    0x2a78d6, 0xeb6834, 0x1baf7a, 0xeda100, 0xe87ba4, 0x008300, 0x6250d6, 0xe34948,
];
const SERIES_DARK: [u32; 8] = [
    0x3987e5, 0xd95926, 0x199e70, 0xc98500, 0xd55181, 0x008300, 0x9085e9, 0xe66767,
];

/// Series in a graph: at most this many get their own colour.
pub const SERIES_COLOURS: usize = 8;

/// Scatter graphs keep their series apart for colour-blind readers up to this many.
pub const SCATTER_SERIES: usize = 3;

/// The colour of the `index`-th series (0-based), `None` past the eighth: a ninth series
/// is never given a made-up colour.
pub fn series(index: usize, surface: Surface) -> Option<Rgb> {
    let table = match surface {
        Surface::Light => &SERIES_LIGHT,
        Surface::Dark => &SERIES_DARK,
    };
    table.get(index).map(|&v| Rgb::hex(v))
}

/// How a map's values are coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColourScale {
    /// Magnitude: light (low) to dark (high), one hue.
    Sequential,
    /// A signed difference: blue below zero, red above, grey at zero.
    Diverging,
    /// Phase: the hue turns once round from −π to π at constant lightness.
    Cyclic,
}

impl ColourScale {
    /// Its name in files.
    pub fn name(self) -> &'static str {
        match self {
            ColourScale::Sequential => "sequential",
            ColourScale::Diverging => "diverging",
            ColourScale::Cyclic => "cyclic",
        }
    }

    /// From its name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "sequential" => Some(ColourScale::Sequential),
            "diverging" => Some(ColourScale::Diverging),
            "cyclic" => Some(ColourScale::Cyclic),
            _ => None,
        }
    }
}

/// The blue ramp, steps 100 to 700 (light to dark).
const BLUE_RAMP: [u32; 13] = [
    0xcde2fb, 0xb7d3f6, 0x9ec5f4, 0x86b6ef, 0x6da7ec, 0x5598e7, 0x3987e5, 0x2a78d6, 0x256abf,
    0x1c5cab, 0x184f95, 0x104281, 0x0d366b,
];

fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// The colour of `t` in [0, 1] on a scale (for `Diverging`, 0.5 is zero; for `Cyclic`,
/// 0 and 1 are the same phase). On a dark surface the sequential scale runs the other
/// way, dark (low) to light (high), so low values recede into the surface there too.
pub fn scale(scale: ColourScale, t: f64, surface: Surface) -> Rgb {
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    match scale {
        ColourScale::Sequential => {
            let t = match surface {
                Surface::Light => t,
                Surface::Dark => 1.0 - t,
            };
            let x = t * (BLUE_RAMP.len() - 1) as f64;
            let i = (x.floor() as usize).min(BLUE_RAMP.len() - 2);
            let a = Rgb::hex(BLUE_RAMP[i]).oklab();
            let b = Rgb::hex(BLUE_RAMP[i + 1]).oklab();
            Rgb::from_oklab(mix(a, b, x - i as f64))
        }
        ColourScale::Diverging => {
            let middle = match surface {
                Surface::Light => Rgb::hex(0xf0efec),
                Surface::Dark => Rgb::hex(0x383835),
            }
            .oklab();
            let (low, high) = match surface {
                Surface::Light => (Rgb::hex(0x2a78d6), Rgb::hex(0xe34948)),
                Surface::Dark => (Rgb::hex(0x3987e5), Rgb::hex(0xe66767)),
            };
            if t < 0.5 {
                Rgb::from_oklab(mix(low.oklab(), middle, t / 0.5))
            } else {
                Rgb::from_oklab(mix(middle, high.oklab(), (t - 0.5) / 0.5))
            }
        }
        ColourScale::Cyclic => {
            // OKLCH at constant lightness and chroma, inside the sRGB gamut at every hue.
            let (lightness, chroma) = match surface {
                Surface::Light => (0.62, 0.10),
                Surface::Dark => (0.66, 0.10),
            };
            let hue = 2.0 * core::f64::consts::PI * t;
            Rgb::from_oklab([lightness, chroma * hue.cos(), chroma * hue.sin()])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oklab_round_trips_and_knows_white() {
        let white = Rgb::hex(0xffffff).oklab();
        assert!((white[0] - 1.0).abs() < 1e-6 && white[1].abs() < 1e-6 && white[2].abs() < 1e-6);
        for v in [0x2a78d6, 0xeb6834, 0x0d366b, 0xf0efec, 0x000000] {
            let c = Rgb::hex(v);
            assert_eq!(Rgb::from_oklab(c.oklab()), c, "{}", c.to_hex());
        }
    }

    #[test]
    fn the_sequential_scale_darkens_steadily() {
        for surface in [Surface::Light, Surface::Dark] {
            let lightness: Vec<f64> = (0..=50)
                .map(|i| scale(ColourScale::Sequential, i as f64 / 50.0, surface).oklab()[0])
                .collect();
            for pair in lightness.windows(2) {
                match surface {
                    Surface::Light => assert!(pair[1] <= pair[0] + 1e-3, "{pair:?}"),
                    Surface::Dark => assert!(pair[1] >= pair[0] - 1e-3, "{pair:?}"),
                }
            }
            assert!((lightness[0] - lightness[50]).abs() > 0.5);
        }
    }

    #[test]
    fn the_diverging_scale_is_grey_at_zero_and_balanced() {
        for surface in [Surface::Light, Surface::Dark] {
            let at = |t: f64| scale(ColourScale::Diverging, t, surface).oklab();
            let middle = at(0.5);
            assert!(
                middle[1].abs() < 0.01 && middle[2].abs() < 0.01,
                "{middle:?}"
            );
            // The two ends: blue and red, equally far from grey in lightness (±0.05).
            let (low, high) = (at(0.0), at(1.0));
            assert!(low[2] < 0.0 && high[1] > 0.0);
            assert!(((middle[0] - low[0]) - (middle[0] - high[0])).abs() < 0.05);
        }
    }

    #[test]
    fn the_cyclic_scale_keeps_its_lightness_and_closes() {
        for surface in [Surface::Light, Surface::Dark] {
            let first = scale(ColourScale::Cyclic, 0.0, surface);
            assert_eq!(first, scale(ColourScale::Cyclic, 1.0, surface));
            let lightness: Vec<f64> = (0..=72)
                .map(|i| scale(ColourScale::Cyclic, i as f64 / 72.0, surface).oklab()[0])
                .collect();
            let (low, high) = lightness
                .iter()
                .fold((1.0f64, 0.0f64), |(a, b), &l| (a.min(l), b.max(l)));
            // Within the sRGB gamut at every hue: no clipping changes the lightness.
            assert!(high - low < 0.01, "{low} {high}");
        }
    }
}
