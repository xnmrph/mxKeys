//! Encoding frames as LVGL 9 `LV_COLOR_FORMAT_I1` images (what ZMK's nice!view art
//! uses): a 2-colour palette (8 bytes) followed by rows of packed bits, MSB first,
//! each row padded to whole bytes.
//!
//! The nice!view is mounted rotated on the Corne: an image drawn in the display's
//! coordinates (160×68 landscape) appears turned 90° on the keyboard. Checked against
//! ZMK's own art (`balloon`, `mountain`): turning the stored image 90°
//! counter-clockwise gives the upright picture, so we store the upright frame turned
//! 90° clockwise.

use crate::render::Frame;

/// Size in the display's coordinates (width, height).
pub fn display_size(f: &Frame) -> (u32, u32) {
    (f.height, f.width)
}

/// Bytes per row.
pub fn stride(width: u32) -> u32 {
    width.div_ceil(8)
}

/// Packed bits in display orientation (without palette). Bit set = palette index 1
/// = white (as in ZMK's art).
pub fn pack(f: &Frame) -> Vec<u8> {
    let (w, h) = display_size(f);
    let s = stride(w) as usize;
    let mut out = vec![0u8; s * h as usize];
    for y in 0..h {
        for x in 0..w {
            // Display (x, y) ← upright (y, H−1−x)  [90° clockwise]
            let on = f.get(y, f.height - 1 - x);
            if on {
                out[y as usize * s + (x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
    }
    out
}

/// Inverse of [`pack`]: display-orientation bits back to an upright frame.
pub fn unpack(bits: &[u8], display_w: u32, display_h: u32) -> Frame {
    let s = stride(display_w) as usize;
    let (uw, uh) = (display_h, display_w);
    let mut pixels = vec![false; (uw * uh) as usize];
    for y in 0..display_h {
        for x in 0..display_w {
            let on = bits[y as usize * s + (x / 8) as usize] & (0x80 >> (x % 8)) != 0;
            // upright (y, H−1−x) with H = uh
            let (ux, uy) = (y, uh - 1 - x);
            pixels[(uy * uw + ux) as usize] = on;
        }
    }
    Frame {
        width: uw,
        height: uh,
        pixels,
        delay_ms: 0,
    }
}

/// Total bytes of one frame in flash: palette + rows (the `data_size` LVGL expects).
pub fn frame_bytes(f: &Frame) -> usize {
    let (w, h) = display_size(f);
    8 + (stride(w) * h) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ZMK's stock `balloon` art (MIT, © 2023 Collin Hodge / The ZMK Contributors):
    /// 140×68 display pixels, packed rows without the palette.
    const BALLOON: &[u8] = include_bytes!("../tests/fixtures/balloon_140x68.bin");

    #[test]
    fn round_trips_zmk_art_exactly() {
        assert_eq!(BALLOON.len(), 18 * 68);
        let upright = unpack(BALLOON, 140, 68);
        assert_eq!((upright.width, upright.height), (68, 140));
        assert_eq!(pack(&upright), BALLOON);
        assert_eq!(frame_bytes(&upright), 1232); // = ZMK's `.data_size`
    }

    #[test]
    fn upright_top_left_is_display_top_right() {
        // One lit pixel at the upright top-left corner → display (w−1, 0).
        let mut f = Frame {
            width: 68,
            height: 160,
            pixels: vec![false; 68 * 160],
            delay_ms: 0,
        };
        f.pixels[0] = true;
        let bits = pack(&f);
        let s = stride(160) as usize;
        assert_eq!(s, 20);
        assert_eq!(bits[19], 0x01, "last bit of the first row");
        assert_eq!(bits.iter().filter(|b| **b != 0).count(), 1);
    }
}
