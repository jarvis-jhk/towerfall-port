//! A minimal RGBA8 image with PNG load/save and CPU blitting (used by the
//! tower sheet and by the font texture decoder; the renderer uploads these).

use crate::error::{Error, Result};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }
}

/// Straight (non-premultiplied) RGBA8, row-major, no padding.
#[derive(Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for RgbaImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RgbaImage({}x{})", self.width, self.height)
    }
}

impl RgbaImage {
    pub fn new(width: u32, height: u32) -> Self {
        RgbaImage {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    pub fn filled(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let mut img = Self::new(width, height);
        img.fill_rect(Rect::new(0, 0, width as i32, height as i32), rgba);
        img
    }

    /// Loads a PNG of any colour type/bit depth and converts it to RGBA8.
    pub fn load_png(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path).map_err(|e| Error::io(path, e))?;
        let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let png_err = |source| Error::Png {
            path: path.to_owned(),
            source,
        };
        let mut reader = decoder.read_info().map_err(png_err)?;
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| Error::format(path, "PNG too large"))?;
        let mut buf = vec![0; size];
        let info = reader.next_frame(&mut buf).map_err(png_err)?;
        buf.truncate(info.buffer_size());
        let (w, h) = (info.width, info.height);
        let pixels = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => buf
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            png::ColorType::GrayscaleAlpha => buf
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            png::ColorType::Indexed => {
                return Err(Error::format(path, "indexed PNG not expanded"));
            }
        };
        Ok(RgbaImage {
            width: w,
            height: h,
            pixels,
        })
    }

    pub fn save_png(&self, path: &Path) -> Result<()> {
        let file = std::fs::File::create(path).map_err(|e| Error::io(path, e))?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let to_err = |e: png::EncodingError| Error::format(path, format!("PNG encode: {e}"));
        let mut w = enc.write_header().map_err(to_err)?;
        w.write_image_data(&self.pixels).map_err(to_err)?;
        w.finish().map_err(to_err)?;
        Ok(())
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    #[inline]
    pub fn put(&mut self, x: u32, y: u32, p: [u8; 4]) {
        let i = ((y * self.width + x) * 4) as usize;
        self.pixels[i..i + 4].copy_from_slice(&p);
    }

    /// Copies a sub-image out (clipped to the source).
    pub fn crop(&self, r: Rect) -> RgbaImage {
        let mut out = RgbaImage::new(r.w.max(0) as u32, r.h.max(0) as u32);
        out.blit(self, r, 0, 0, Blend::Replace);
        out
    }

    pub fn fill_rect(&mut self, r: Rect, rgba: [u8; 4]) {
        let x0 = r.x.max(0);
        let y0 = r.y.max(0);
        let x1 = (r.x + r.w).min(self.width as i32);
        let y1 = (r.y + r.h).min(self.height as i32);
        for y in y0..y1 {
            for x in x0..x1 {
                self.put(x as u32, y as u32, rgba);
            }
        }
    }

    /// Draws `src_rect` of `src` with its top-left at (`dx`, `dy`), clipped.
    pub fn blit(&mut self, src: &RgbaImage, src_rect: Rect, dx: i32, dy: i32, blend: Blend) {
        for sy in 0..src_rect.h {
            let ty = dy + sy;
            let py = src_rect.y + sy;
            if ty < 0 || ty >= self.height as i32 || py < 0 || py >= src.height as i32 {
                continue;
            }
            for sx in 0..src_rect.w {
                let tx = dx + sx;
                let px = src_rect.x + sx;
                if tx < 0 || tx >= self.width as i32 || px < 0 || px >= src.width as i32 {
                    continue;
                }
                let s = src.get(px as u32, py as u32);
                match blend {
                    Blend::Replace => self.put(tx as u32, ty as u32, s),
                    Blend::Alpha => {
                        if s[3] == 255 {
                            self.put(tx as u32, ty as u32, s);
                        } else if s[3] > 0 {
                            let d = self.get(tx as u32, ty as u32);
                            self.put(tx as u32, ty as u32, over(s, d));
                        }
                    }
                }
            }
        }
    }

    /// Sprite-batch style draw: `src_rect` of `src` placed so that its
    /// `origin` lands on `pos`, scaled, rotated (radians, clockwise in screen
    /// space), optionally mirrored, tinted by `tint` (RGBA multipliers) and
    /// alpha-blended. Nearest-neighbour sampling at pixel centres.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        src: &RgbaImage,
        src_rect: Rect,
        pos: (f32, f32),
        origin: (f32, f32),
        scale: (f32, f32),
        rotation: f32,
        flip_x: bool,
        tint: [f32; 4],
    ) {
        if src_rect.w <= 0 || src_rect.h <= 0 || scale.0 == 0.0 || scale.1 == 0.0 || tint[3] <= 0.0
        {
            return;
        }
        let (sn, cs) = rotation.sin_cos();
        // Bounding box of the transformed rectangle.
        let corners = [
            (0.0, 0.0),
            (src_rect.w as f32, 0.0),
            (0.0, src_rect.h as f32),
            (src_rect.w as f32, src_rect.h as f32),
        ];
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (cx, cy) in corners {
            let lx = (cx - origin.0) * scale.0;
            let ly = (cy - origin.1) * scale.1;
            let px = pos.0 + lx * cs - ly * sn;
            let py = pos.1 + lx * sn + ly * cs;
            x0 = x0.min(px);
            y0 = y0.min(py);
            x1 = x1.max(px);
            y1 = y1.max(py);
        }
        let xa = (x0.floor() as i32).max(0);
        let ya = (y0.floor() as i32).max(0);
        let xb = (x1.ceil() as i32).min(self.width as i32);
        let yb = (y1.ceil() as i32).min(self.height as i32);
        for y in ya..yb {
            for x in xa..xb {
                let dx = x as f32 + 0.5 - pos.0;
                let dy = y as f32 + 0.5 - pos.1;
                let lx = (dx * cs + dy * sn) / scale.0 + origin.0;
                let ly = (-dx * sn + dy * cs) / scale.1 + origin.1;
                if lx < 0.0 || ly < 0.0 || lx >= src_rect.w as f32 || ly >= src_rect.h as f32 {
                    continue;
                }
                let mut ix = lx as i32;
                if flip_x {
                    ix = src_rect.w - 1 - ix;
                }
                let (sx, sy) = (src_rect.x + ix, src_rect.y + ly as i32);
                if sx < 0 || sy < 0 || sx >= src.width as i32 || sy >= src.height as i32 {
                    continue;
                }
                let p = src.get(sx as u32, sy as u32);
                let q = [
                    (p[0] as f32 * tint[0]).min(255.0) as u8,
                    (p[1] as f32 * tint[1]).min(255.0) as u8,
                    (p[2] as f32 * tint[2]).min(255.0) as u8,
                    (p[3] as f32 * tint[3]).min(255.0) as u8,
                ];
                if q[3] == 0 {
                    continue;
                }
                let d = self.get(x as u32, y as u32);
                self.put(x as u32, y as u32, if q[3] == 255 { q } else { over(q, d) });
            }
        }
    }

    /// Nearest-neighbour downscale by an integer factor (for previews).
    pub fn downscale(&self, factor: u32) -> RgbaImage {
        let mut out = RgbaImage::new(self.width / factor, self.height / factor);
        for y in 0..out.height {
            for x in 0..out.width {
                out.put(x, y, self.get(x * factor, y * factor));
            }
        }
        out
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blend {
    /// Copy source pixels as they are (alpha included).
    Replace,
    /// Straight-alpha "over".
    Alpha,
}

fn over(s: [u8; 4], d: [u8; 4]) -> [u8; 4] {
    let sa = s[3] as u32;
    let da = d[3] as u32;
    let out_a = sa * 255 + da * (255 - sa); // scaled by 255
    if out_a == 0 {
        return [0; 4];
    }
    let mut o = [0u8; 4];
    for c in 0..3 {
        let v = (s[c] as u32 * sa * 255 + d[c] as u32 * da * (255 - sa)) / out_a;
        o[c] = v as u8;
    }
    o[3] = (out_a / 255) as u8;
    o
}
