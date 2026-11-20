//! optobench's application icon: the title bar's mark (a lens outline with the axis
//! through it) on a dark rounded square, the axis drawn as the beam in the view's ray
//! red. Drawn here at any size, anti-aliased by distance to each shape's edge, so the
//! window and the platforms' icon sizes need no image files.

/// Straight RGBA, `size` × `size`.
pub fn rgba(size: u32) -> Vec<u8> {
    let s = size as f64;
    // In units of the icon's side, from its top-left corner.
    let background = |x: f64, y: f64| rounded_square(x, y, 0.5, 0.5, 0.42, 0.19);
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for row in 0..size {
        for column in 0..size {
            // The pixel's centre, in units of the side; distances come back in pixels.
            let (x, y) = ((column as f64 + 0.5) / s, (row as f64 + 0.5) / s);
            let coverage = |distance: f64| (0.5 - distance * s).clamp(0.0, 1.0);
            let mut pixel = [0.0_f64; 4];
            // The plate: dark, a little lighter at the top.
            let shade = 1.0 - 0.35 * y;
            over(
                &mut pixel,
                [0.17 * shade, 0.18 * shade, 0.20 * shade],
                coverage(background(x, y)),
            );
            // The lens: a faint glass fill inside its ring, then the ring.
            let r = ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt();
            over(&mut pixel, [0.31, 0.63, 0.86], 0.16 * coverage(r - 0.25));
            over(
                &mut pixel,
                [0.31, 0.63, 0.86],
                coverage((r - 0.25).abs() - 0.03),
            );
            // The beam along the axis, past the lens on both sides, round ends.
            let along = (x - 0.5).abs() - 0.32;
            let beam = if along > 0.0 {
                (along.powi(2) + (y - 0.5).powi(2)).sqrt()
            } else {
                (y - 0.5).abs()
            } - 0.028;
            over(&mut pixel, [1.0, 0.36, 0.28], coverage(beam));
            out.extend(pixel.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
    }
    out
}

/// Signed distance from (x, y) to a square centred on (cx, cy), half-side `half`, with
/// corners of radius `radius` (negative inside).
fn rounded_square(x: f64, y: f64, cx: f64, cy: f64, half: f64, radius: f64) -> f64 {
    let qx = (x - cx).abs() - (half - radius);
    let qy = (y - cy).abs() - (half - radius);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

/// Paints `colour` over `pixel` (straight RGBA) with coverage `alpha`.
fn over(pixel: &mut [f64; 4], colour: [f64; 3], alpha: f64) {
    if alpha <= 0.0 {
        return;
    }
    let below = pixel[3];
    let total = alpha + below * (1.0 - alpha);
    for k in 0..3 {
        pixel[k] = (colour[k] * alpha + pixel[k] * below * (1.0 - alpha)) / total;
    }
    pixel[3] = total;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_a_plate_with_a_lens_and_a_beam() {
        let size = 256;
        let image = rgba(size);
        assert_eq!(image.len(), (size * size * 4) as usize);
        let at = |x: u32, y: u32| {
            let k = ((y * size + x) * 4) as usize;
            [image[k], image[k + 1], image[k + 2], image[k + 3]]
        };
        // Outside the plate, clear; its corner rounded off.
        assert_eq!(at(2, 128)[3], 0);
        assert_eq!(at(24, 24)[3], 0);
        // On the plate, opaque and dark.
        let plate = at(128, 40);
        assert!(plate[3] == 255 && plate[0] < 60, "{plate:?}");
        // The ring, blue, at a quarter of the side from the centre.
        let ring = at(128, 64);
        assert!(ring[2] > 180 && ring[0] < 110, "{ring:?}");
        // The beam through the middle, red, beyond the ring as well.
        for x in [60, 128, 196] {
            let beam = at(x, 128);
            assert!(beam[0] > 240 && beam[2] < 100, "{x}: {beam:?}");
        }
        // Debug aid: OPTOBENCH_ICON=<path> writes the raw image for viewing.
        if let Some(path) = std::env::var_os("OPTOBENCH_ICON") {
            std::fs::write(path, &image).expect("written");
        }
    }
}
