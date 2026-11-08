//! Screenshots of the window for checking and documenting the interface without a person
//! at the screen: with `OPTOBENCH_UI_SCREENSHOT=<file.bmp>` the app waits until the open
//! design is read (and traced, with `OPTOBENCH_UI_TRACE=1`), saves the window as a 24-bit
//! BMP and exits. `OPTOBENCH_UI_WORKSPACE` (0, 1, 2) and `OPTOBENCH_UI_SELECT` (an item
//! id) set the workspace and the selection first; `OPTOBENCH_UI_VIEW` (top, front, right,
//! iso), `OPTOBENCH_UI_PERSPECTIVE=1`, `OPTOBENCH_UI_ZOOM` (the view's height in sizes of
//! the selected item, or of the scene, centred on it) and `OPTOBENCH_UI_SECTION` (x, y or
//! z: a cut through its centre) set the view.

use std::io::Write;
use std::path::Path;

use egui::ColorImage;

/// Writes an image as an uncompressed 24-bit BMP (bottom-up rows, padded to 4 bytes).
pub fn write_bmp(image: &ColorImage, path: &Path) -> std::io::Result<()> {
    let [w, h] = image.size;
    let row = (w * 3).div_ceil(4) * 4;
    let size = 54 + row * h;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in (0..h).rev() {
        for x in 0..w {
            let c = image.pixels[y * w + x];
            out.extend_from_slice(&[c.b(), c.g(), c.r()]);
        }
        out.extend(std::iter::repeat_n(0u8, row - w * 3));
    }
    std::fs::File::create(path)?.write_all(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmp_header_and_padding() {
        let image = ColorImage::new([3, 2], vec![egui::Color32::from_rgb(10, 20, 30); 6]);
        let path = std::env::temp_dir().join(format!("optobench-ui-{}.bmp", std::process::id()));
        write_bmp(&image, &path).expect("written");
        let bytes = std::fs::read(&path).expect("read");
        let _ = std::fs::remove_file(&path);
        // 3 pixels × 3 bytes = 9, padded to 12 per row; 2 rows; 54-byte header.
        assert_eq!(bytes.len(), 54 + 24);
        assert_eq!(&bytes[..2], b"BM");
        assert_eq!(&bytes[54..57], &[30, 20, 10], "blue, green, red");
    }
}
