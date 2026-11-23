//! Turning a picture or GIF into 1-bit frames for the nice!view.
//!
//! Everything here works in the orientation the user sees on the keyboard
//! ("upright": 68 pixels wide, 140 or 160 tall). [`crate::lvgl`] rotates into the
//! display's own orientation when encoding.

use std::io::Cursor;

use image::codecs::gif::GifDecoder;
use image::imageops::FilterType;
use image::{AnimationDecoder, DynamicImage, GrayImage, ImageFormat, ImageReader, Luma, RgbaImage};
use serde::{Deserialize, Serialize};

/// Where the picture goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    /// The picture area of the right half's stock screen (below the battery strip).
    Art,
    /// The whole screen (the status is hidden).
    Full,
}

impl Area {
    /// Upright size (width, height) as seen on the keyboard.
    pub fn upright_size(self) -> (u32, u32) {
        match self {
            Area::Art => (68, 140),
            Area::Full => (68, 160),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Fill the area, cropping what sticks out.
    Cover,
    /// Show the whole picture, with bars.
    Contain,
    /// Stretch to the area.
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dither {
    /// Plain cut-off at the threshold (best for logos and line art).
    Threshold,
    FloydSteinberg,
    /// Lighter, higher-contrast error diffusion (classic Mac look).
    Atkinson,
    /// Ordered 4×4 pattern (stable between animation frames).
    Bayer4,
    Bayer8,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Options {
    pub fit: Fit,
    pub dither: Dither,
    /// 0–255: brightness above which a pixel is white (threshold and the
    /// diffusion/ordered methods' mid point).
    pub threshold: u8,
    /// −100…100.
    pub brightness: i32,
    /// −100…100.
    pub contrast: i32,
    pub invert: bool,
    /// Bars/transparent areas are white instead of black.
    pub white_background: bool,
    /// Shortest time a frame is shown (ms); faster GIFs are slowed down.
    pub min_frame_ms: u32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            fit: Fit::Cover,
            dither: Dither::Atkinson,
            threshold: 128,
            brightness: 0,
            contrast: 0,
            invert: false,
            white_background: true,
            min_frame_ms: 100,
        }
    }
}

/// A decoded source picture (one frame for still images).
#[derive(Debug, Clone)]
pub struct Source {
    pub frames: Vec<(RgbaImage, u32)>,
    pub width: u32,
    pub height: u32,
}

/// A 1-bit frame, upright. `true` = white (lit) pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<bool>,
    pub delay_ms: u32,
}

impl Frame {
    pub fn get(&self, x: u32, y: u32) -> bool {
        self.pixels[(y * self.width + x) as usize]
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("this file isn't a picture mxKeys can read (PNG, GIF, JPEG, BMP or WebP)")]
    Unsupported,
    #[error("couldn't read the picture: {0}")]
    Decode(String),
    #[error("the picture is empty")]
    Empty,
}

/// Longest edge we keep from the source; bigger pictures are scaled down on load
/// (the screen is tiny, and GIFs with many big frames would use a lot of memory).
const MAX_SOURCE_EDGE: u32 = 640;
/// At most this many frames are decoded from an animation.
pub const MAX_SOURCE_FRAMES: usize = 500;

fn shrink(img: RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    if w.max(h) <= MAX_SOURCE_EDGE {
        return img;
    }
    let scale = MAX_SOURCE_EDGE as f32 / w.max(h) as f32;
    let (nw, nh) = (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    image::imageops::resize(&img, nw, nh, FilterType::Triangle)
}

/// Decode a picture file (all frames of a GIF).
pub fn load(bytes: &[u8]) -> Result<Source, RenderError> {
    let format = image::guess_format(bytes).map_err(|_| RenderError::Unsupported)?;
    let frames: Vec<(RgbaImage, u32)> = if format == ImageFormat::Gif {
        let dec = GifDecoder::new(Cursor::new(bytes)).map_err(|e| RenderError::Decode(e.to_string()))?;
        let mut out = Vec::new();
        for f in dec.into_frames().take(MAX_SOURCE_FRAMES) {
            let f = f.map_err(|e| RenderError::Decode(e.to_string()))?;
            let (num, den) = f.delay().numer_denom_ms();
            let ms = num.checked_div(den).unwrap_or(100);
            // Browsers treat 0–10 ms GIF delays as 100 ms; do the same.
            let ms = if ms <= 10 { 100 } else { ms };
            out.push((shrink(f.into_buffer()), ms));
        }
        out
    } else {
        let img = ImageReader::with_format(Cursor::new(bytes), format)
            .decode()
            .map_err(|e| RenderError::Decode(e.to_string()))?;
        vec![(shrink(img.to_rgba8()), 0)]
    };
    let (width, height) = frames
        .first()
        .map(|(f, _)| f.dimensions())
        .ok_or(RenderError::Empty)?;
    if width == 0 || height == 0 {
        return Err(RenderError::Empty);
    }
    Ok(Source {
        frames,
        width,
        height,
    })
}

/// Brightness of each pixel after fitting into `w`×`h`, with transparency and bars
/// filled with the background.
fn fit_gray(img: &RgbaImage, w: u32, h: u32, o: &Options) -> GrayImage {
    let bg = if o.white_background { 255.0 } else { 0.0 };
    let (sw, sh) = img.dimensions();
    let (rw, rh, ox, oy) = match o.fit {
        Fit::Stretch => (w, h, 0i64, 0i64),
        Fit::Cover | Fit::Contain => {
            let sx = w as f32 / sw as f32;
            let sy = h as f32 / sh as f32;
            let s = if o.fit == Fit::Cover {
                sx.max(sy)
            } else {
                sx.min(sy)
            };
            let rw = ((sw as f32 * s).round() as u32).max(1);
            let rh = ((sh as f32 * s).round() as u32).max(1);
            (rw, rh, (w as i64 - rw as i64) / 2, (h as i64 - rh as i64) / 2)
        }
    };
    let filter = if rw < sw || rh < sh {
        FilterType::Lanczos3
    } else {
        FilterType::Nearest
    };
    let scaled = image::imageops::resize(img, rw, rh, filter);
    let mut out = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = (x as i64 - ox, y as i64 - oy);
            let v = if sx >= 0 && sy >= 0 && (sx as u32) < rw && (sy as u32) < rh {
                let p = scaled.get_pixel(sx as u32, sy as u32);
                // Rec. 709 luma, blended over the background by alpha.
                let l = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
                let a = p[3] as f32 / 255.0;
                l * a + bg * (1.0 - a)
            } else {
                bg
            };
            out.put_pixel(x, y, Luma([v.round().clamp(0.0, 255.0) as u8]));
        }
    }
    out
}

fn adjust(g: &mut GrayImage, o: &Options) {
    if o.brightness == 0 && o.contrast == 0 {
        return;
    }
    let c = o.contrast.clamp(-100, 100) as f32;
    let factor = (259.0 * (c + 255.0)) / (255.0 * (259.0 - c));
    let b = o.brightness.clamp(-100, 100) as f32 * 2.55;
    for p in g.pixels_mut() {
        let v = factor * (p[0] as f32 - 128.0) + 128.0 + b;
        p[0] = v.round().clamp(0.0, 255.0) as u8;
    }
}

const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

fn bayer8(x: u32, y: u32) -> u32 {
    // Recursive construction: M8 = 4·M4 + {0,2,3,1} per quadrant.
    let q = [[0, 2], [3, 1]][((y / 4) % 2) as usize][((x / 4) % 2) as usize];
    4 * BAYER4[(y % 4) as usize][(x % 4) as usize] as u32 + q
}

/// 1-bit dithering of a gray image.
pub fn dither(g: &GrayImage, o: &Options) -> Vec<bool> {
    let (w, h) = g.dimensions();
    let t = o.threshold as f32;
    match o.dither {
        Dither::Threshold => g.pixels().map(|p| p[0] as f32 > t).collect(),
        Dither::Bayer4 | Dither::Bayer8 => {
            let n = if o.dither == Dither::Bayer4 { 16.0 } else { 64.0 };
            g.enumerate_pixels()
                .map(|(x, y, p)| {
                    let rank = if o.dither == Dither::Bayer4 {
                        BAYER4[(y % 4) as usize][(x % 4) as usize] as f32
                    } else {
                        bayer8(x, y) as f32
                    };
                    // Shift the pattern so `threshold` stays the mid point.
                    let bias = (rank + 0.5) / n * 255.0 - 127.5;
                    p[0] as f32 > t + bias
                })
                .collect()
        }
        Dither::FloydSteinberg | Dither::Atkinson => {
            let mut v: Vec<f32> = g.pixels().map(|p| p[0] as f32).collect();
            let mut out = vec![false; v.len()];
            let idx = |x: i64, y: i64| -> Option<usize> {
                (x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h)
                    .then(|| (y as u32 * w + x as u32) as usize)
            };
            // Error-diffusion kernels: (dx, dy, weight).
            let fs: &[(i64, i64, f32)] = &[
                (1, 0, 7.0 / 16.0),
                (-1, 1, 3.0 / 16.0),
                (0, 1, 5.0 / 16.0),
                (1, 1, 1.0 / 16.0),
            ];
            let at: &[(i64, i64, f32)] = &[
                (1, 0, 0.125),
                (2, 0, 0.125),
                (-1, 1, 0.125),
                (0, 1, 0.125),
                (1, 1, 0.125),
                (0, 2, 0.125),
            ];
            let kernel = if o.dither == Dither::Atkinson { at } else { fs };
            for y in 0..h as i64 {
                for x in 0..w as i64 {
                    let i = idx(x, y).unwrap();
                    let old = v[i];
                    let on = old > t;
                    out[i] = on;
                    let err = old - if on { 255.0 } else { 0.0 };
                    for (dx, dy, wt) in kernel {
                        if let Some(j) = idx(x + dx, y + dy) {
                            v[j] += err * wt;
                        }
                    }
                }
            }
            out
        }
    }
}

/// Render one source frame to a 1-bit upright frame for `area`.
pub fn render_frame(img: &RgbaImage, delay_ms: u32, area: Area, o: &Options) -> Frame {
    let (w, h) = area.upright_size();
    let mut g = fit_gray(img, w, h, o);
    adjust(&mut g, o);
    let mut pixels = dither(&g, o);
    if o.invert {
        pixels.iter_mut().for_each(|p| *p = !*p);
    }
    Frame {
        width: w,
        height: h,
        pixels,
        delay_ms,
    }
}

/// Render all frames, then apply the limits:
/// - frames shorter than `min_frame_ms` are merged with the next ones;
/// - consecutive identical frames are merged (their times added);
/// - more than `max_frames` frames are reduced by sampling evenly (times kept, so the
///   animation runs at the original speed, just less smoothly).
pub fn render(src: &Source, area: Area, o: &Options, max_frames: usize) -> Vec<Frame> {
    let max_frames = max_frames.max(1);
    let min = o.min_frame_ms.max(20);
    let mut frames: Vec<Frame> = Vec::new();
    let mut pending = 0u32;
    for (img, delay) in &src.frames {
        if src.frames.len() > 1 && pending > 0 && pending < min {
            // The previous frame was too short: absorb this one's time and skip it.
            if let Some(last) = frames.last_mut() {
                last.delay_ms += delay;
                pending += delay;
                continue;
            }
        }
        let f = render_frame(img, *delay, area, o);
        match frames.last_mut() {
            Some(last) if last.pixels == f.pixels => {
                last.delay_ms += f.delay_ms;
                pending = last.delay_ms;
            }
            _ => {
                pending = f.delay_ms;
                frames.push(f);
            }
        }
    }
    if frames.len() > 1 {
        for f in &mut frames {
            f.delay_ms = f.delay_ms.max(min);
        }
    }
    if frames.len() > max_frames {
        let n = frames.len();
        let total: u32 = frames.iter().map(|f| f.delay_ms).sum();
        let mut picked: Vec<Frame> = (0..max_frames)
            .map(|i| frames[i * n / max_frames].clone())
            .collect();
        let each = (total / max_frames as u32).max(min);
        picked.iter_mut().for_each(|f| f.delay_ms = each);
        frames = picked;
    }
    if frames.len() == 1 {
        frames[0].delay_ms = 0;
    }
    frames
}

/// A PNG of the frame at `scale`× (for previews).
pub fn preview_png(f: &Frame, scale: u32) -> Vec<u8> {
    let scale = scale.max(1);
    let img = GrayImage::from_fn(f.width * scale, f.height * scale, |x, y| {
        Luma([if f.get(x / scale, y / scale) { 255 } else { 0 }])
    });
    let mut out = Vec::new();
    DynamicImage::ImageLuma8(img)
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .expect("encoding a PNG in memory can't fail");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Delay, Frame as GifFrame, Rgba};

    fn png(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let img = RgbaImage::from_fn(w, h, |x, y| Rgba(f(x, y)));
        let mut out = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    fn gif(frames: &[(u8, u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut out);
            for (v, ms) in frames {
                let img = RgbaImage::from_pixel(20, 20, Rgba([*v, *v, *v, 255]));
                enc.encode_frame(GifFrame::from_parts(
                    img,
                    0,
                    0,
                    Delay::from_numer_denom_ms(*ms, 1),
                ))
                .unwrap();
            }
        }
        out
    }

    fn lit(f: &Frame) -> usize {
        f.pixels.iter().filter(|p| **p).count()
    }

    #[test]
    fn loads_and_fits() {
        // Left half black, right half white, 200×100 (wide) into 68×160 (tall).
        let src = load(&png(
            200,
            100,
            |x, _| if x < 100 { [0, 0, 0, 255] } else { [255; 4] },
        ))
        .unwrap();
        assert_eq!((src.width, src.height, src.frames.len()), (200, 100, 1));
        let o = Options {
            dither: Dither::Threshold,
            ..Options::default()
        };
        let cover = render_frame(&src.frames[0].0, 0, Area::Full, &o);
        assert_eq!((cover.width, cover.height), (68, 160));
        // Cover crops the middle: left side black, right side white.
        assert!(!cover.get(0, 80) && cover.get(67, 80));
        // Contain letterboxes with the (white) background above and below.
        let contain = render_frame(
            &src.frames[0].0,
            0,
            Area::Full,
            &Options {
                fit: Fit::Contain,
                ..o
            },
        );
        assert!(contain.get(0, 0) && !contain.get(0, 80));
        let black_bg = render_frame(
            &src.frames[0].0,
            0,
            Area::Full,
            &Options {
                fit: Fit::Contain,
                white_background: false,
                ..o
            },
        );
        assert!(!black_bg.get(67, 0));
        // Transparent pixels take the background colour.
        let clear = load(&png(10, 10, |_, _| [0, 0, 0, 0])).unwrap();
        assert_eq!(lit(&render_frame(&clear.frames[0].0, 0, Area::Art, &o)), 68 * 140);
        assert!(matches!(load(b"not a picture"), Err(RenderError::Unsupported)));
    }

    #[test]
    fn dithering_keeps_average_brightness() {
        let src = load(&png(68, 160, |_, _| [128, 128, 128, 255])).unwrap();
        for d in [
            Dither::FloydSteinberg,
            Dither::Atkinson,
            Dither::Bayer4,
            Dither::Bayer8,
        ] {
            let f = render_frame(
                &src.frames[0].0,
                0,
                Area::Full,
                &Options {
                    dither: d,
                    ..Options::default()
                },
            );
            let share = lit(&f) as f32 / (68.0 * 160.0);
            assert!((0.35..0.65).contains(&share), "{d:?}: {share}");
        }
        // Threshold is all-or-nothing; invert flips it.
        let t = Options {
            dither: Dither::Threshold,
            threshold: 100,
            ..Options::default()
        };
        assert_eq!(lit(&render_frame(&src.frames[0].0, 0, Area::Full, &t)), 68 * 160);
        assert_eq!(
            lit(&render_frame(
                &src.frames[0].0,
                0,
                Area::Full,
                &Options { invert: true, ..t }
            )),
            0
        );
        let darker = Options { brightness: -60, ..t };
        assert_eq!(lit(&render_frame(&src.frames[0].0, 0, Area::Full, &darker)), 0);
    }

    #[test]
    fn animation_limits() {
        let o = Options {
            dither: Dither::Threshold,
            ..Options::default()
        };
        // Six frames: 40 ms each (too fast), alternating black/white.
        let src = load(&gif(&[
            (0, 40),
            (255, 40),
            (0, 40),
            (255, 40),
            (0, 40),
            (255, 40),
        ]))
        .unwrap();
        assert_eq!(src.frames.len(), 6);
        let frames = render(&src, Area::Full, &o, 100);
        assert!(
            frames.iter().all(|f| f.delay_ms >= 100),
            "{:?}",
            frames.iter().map(|f| f.delay_ms).collect::<Vec<_>>()
        );
        // Identical neighbours merge; a still picture has no delay.
        let same = load(&gif(&[(0, 200), (0, 200), (255, 200)])).unwrap();
        let f = render(&same, Area::Art, &o, 100);
        assert_eq!(f.iter().map(|f| f.delay_ms).collect::<Vec<_>>(), [400, 200]);
        // The frame budget samples evenly and keeps the total time.
        let many: Vec<(u8, u32)> = (0..20).map(|i| (if i % 2 == 0 { 0 } else { 255 }, 100)).collect();
        let f = render(&load(&gif(&many)).unwrap(), Area::Full, &o, 5);
        assert_eq!(f.len(), 5);
        assert_eq!(f.iter().map(|f| f.delay_ms).sum::<u32>(), 2000);
        let still = render(&load(&png(5, 5, |_, _| [255; 4])).unwrap(), Area::Full, &o, 5);
        assert_eq!((still.len(), still[0].delay_ms), (1, 0));
        // Preview PNG decodes back at the requested scale.
        let p = preview_png(&still[0], 3);
        let back = image::load_from_memory(&p).unwrap();
        assert_eq!((back.width(), back.height()), (204, 480));
    }
}
