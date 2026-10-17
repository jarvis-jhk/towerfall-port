//! XNA content files (`Content/Misc/*.xnb`): sprite fonts (decoded) and
//! compiled effects (only described — we rewrite the shaders in WGSL).
//! See `docs/formats/xnb.md`.
//!
//! The game's XNBs are XNA 4.0 (`version 5`), Windows (`w`), uncompressed;
//! compressed files (LZX `0x80` / LZ4 `0x40` flag) are rejected with a clear
//! error rather than decoded, since none ship.

use crate::audio::Reader;
use crate::error::{Error, Result, read_file};
use crate::image::{Rect, RgbaImage};
use std::path::{Path, PathBuf};

pub const FLAG_HIDEF: u8 = 0x01;
pub const FLAG_LZ4: u8 = 0x40;
pub const FLAG_LZX: u8 = 0x80;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeReader {
    /// Assembly-qualified reader type name.
    pub name: String,
    pub version: i32,
}

impl TypeReader {
    /// `Microsoft.Xna.Framework.Content.SpriteFontReader` → `SpriteFontReader`.
    pub fn short_name(&self) -> &str {
        let base = self.name.split(',').next().unwrap_or(&self.name);
        let base = base.split('[').next().unwrap_or(base);
        base.rsplit('.').next().unwrap_or(base)
    }
}

#[derive(Clone, Debug)]
pub struct XnbHeader {
    pub platform: char,
    pub version: u8,
    pub flags: u8,
    pub file_size: u32,
    pub readers: Vec<TypeReader>,
    pub shared_resources: u32,
    /// Offset of the primary object (its type-reader id).
    pub object_offset: usize,
}

fn read_7bit(r: &mut Reader) -> Option<u32> {
    let mut v = 0u32;
    let mut shift = 0;
    loop {
        let b = r.u8()?;
        v |= ((b & 0x7f) as u32) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 28 {
            return None;
        }
    }
}

fn read_string(r: &mut Reader) -> Option<String> {
    let n = read_7bit(r)? as usize;
    Some(String::from_utf8_lossy(r.bytes(n)?).into_owned())
}

/// A .NET `BinaryReader.ReadChar` (UTF-8).
fn read_char(r: &mut Reader) -> Option<char> {
    let b0 = r.u8()?;
    let (len, init) = match b0 {
        0x00..=0x7f => return Some(b0 as char),
        0xc0..=0xdf => (1, (b0 & 0x1f) as u32),
        0xe0..=0xef => (2, (b0 & 0x0f) as u32),
        0xf0..=0xf7 => (3, (b0 & 0x07) as u32),
        _ => return None,
    };
    let mut v = init;
    for _ in 0..len {
        v = (v << 6) | (r.u8()? & 0x3f) as u32;
    }
    char::from_u32(v)
}

pub fn read_header(path: &Path, d: &[u8]) -> Result<XnbHeader> {
    let short = || Error::format(path, "truncated XNB");
    if d.len() < 10 || &d[0..3] != b"XNB" {
        return Err(Error::format(path, "not an XNB file"));
    }
    let platform = d[3] as char;
    let version = d[4];
    let flags = d[5];
    let mut r = Reader::at(d, 6);
    let file_size = r.u32().ok_or_else(short)?;
    if flags & (FLAG_LZX | FLAG_LZ4) != 0 {
        return Err(Error::format(
            path,
            format!("compressed XNB (flags {flags:#04x}) not supported"),
        ));
    }
    let n = read_7bit(&mut r).ok_or_else(short)?;
    let mut readers = Vec::new();
    for _ in 0..n {
        let name = read_string(&mut r).ok_or_else(short)?;
        let version = r.i32().ok_or_else(short)?;
        readers.push(TypeReader { name, version });
    }
    let shared_resources = read_7bit(&mut r).ok_or_else(short)?;
    Ok(XnbHeader {
        platform,
        version,
        flags,
        file_size,
        readers,
        shared_resources,
        object_offset: r.pos,
    })
}

/// XNA `SurfaceFormat` values seen in font textures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceFormat {
    Color,
    Dxt1,
    Dxt3,
    Dxt5,
    Other(i32),
}

impl SurfaceFormat {
    fn from_i32(v: i32) -> Self {
        match v {
            0 => SurfaceFormat::Color,
            4 => SurfaceFormat::Dxt1,
            5 => SurfaceFormat::Dxt3,
            6 => SurfaceFormat::Dxt5,
            o => SurfaceFormat::Other(o),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SpriteFont {
    pub path: PathBuf,
    pub texture_format: SurfaceFormat,
    /// Decoded glyph sheet (straight alpha).
    pub texture: RgbaImage,
    /// Per glyph: source rectangle in the texture.
    pub glyphs: Vec<Rect>,
    /// Per glyph: placement box (x/y offset from the pen, width, height).
    pub cropping: Vec<Rect>,
    pub characters: Vec<char>,
    pub line_spacing: i32,
    pub spacing: f32,
    /// Per glyph: (left bearing, glyph width, right bearing) — XNA's
    /// "kerning" triple; advance = left + width + right + spacing.
    pub kerning: Vec<[f32; 3]>,
    pub default_character: Option<char>,
}

impl SpriteFont {
    pub fn load(path: &Path) -> Result<SpriteFont> {
        let d = read_file(path)?;
        let h = read_header(path, &d)?;
        let short = || Error::format(path, "truncated sprite font");
        let reader_at = |id: u32| {
            h.readers
                .get(id.checked_sub(1)? as usize)
                .map(|t| t.short_name().to_owned())
        };
        let mut r = Reader::at(&d, h.object_offset);
        let expect = |r: &mut Reader, want: &str| -> Result<()> {
            let id = read_7bit(r).ok_or_else(short)?;
            match reader_at(id) {
                Some(n) if n.starts_with(want) => Ok(()),
                other => Err(Error::format(
                    path,
                    format!("expected {want}, found {other:?}"),
                )),
            }
        };
        expect(&mut r, "SpriteFontReader")?;
        expect(&mut r, "Texture2DReader")?;
        let fmt = SurfaceFormat::from_i32(r.i32().ok_or_else(short)?);
        let w = r.u32().ok_or_else(short)?;
        let hgt = r.u32().ok_or_else(short)?;
        let mips = r.u32().ok_or_else(short)?;
        if mips == 0 {
            return Err(Error::format(path, "texture without levels"));
        }
        let size = r.u32().ok_or_else(short)? as usize;
        let pixels = r.bytes(size).ok_or_else(short)?;
        for _ in 1..mips {
            let s = r.u32().ok_or_else(short)? as usize;
            r.bytes(s).ok_or_else(short)?;
        }
        let texture = decode_texture(fmt, w, hgt, pixels)
            .ok_or_else(|| Error::format(path, format!("cannot decode {fmt:?} texture")))?;
        let rects = |r: &mut Reader| -> Result<Vec<Rect>> {
            expect(r, "ListReader")?;
            let n = r.u32().ok_or_else(short)?;
            (0..n)
                .map(|_| Some(Rect::new(r.i32()?, r.i32()?, r.i32()?, r.i32()?)))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(short)
        };
        let glyphs = rects(&mut r)?;
        let cropping = rects(&mut r)?;
        expect(&mut r, "ListReader")?;
        let n = r.u32().ok_or_else(short)?;
        let characters = (0..n)
            .map(|_| read_char(&mut r))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(short)?;
        let line_spacing = r.i32().ok_or_else(short)?;
        let spacing = r.f32().ok_or_else(short)?;
        expect(&mut r, "ListReader")?;
        let n = r.u32().ok_or_else(short)?;
        let kerning = (0..n)
            .map(|_| Some([r.f32()?, r.f32()?, r.f32()?]))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(short)?;
        let has_default = r.u8().ok_or_else(short)? != 0;
        let default_character = if has_default { read_char(&mut r) } else { None };
        if glyphs.len() != characters.len()
            || cropping.len() != characters.len()
            || kerning.len() != characters.len()
        {
            return Err(Error::format(path, "glyph table lengths differ"));
        }
        Ok(SpriteFont {
            path: path.to_owned(),
            texture_format: fmt,
            texture,
            glyphs,
            cropping,
            characters,
            line_spacing,
            spacing,
            kerning,
            default_character,
        })
    }

    pub fn glyph_index(&self, c: char) -> Option<usize> {
        self.characters.iter().position(|&x| x == c)
    }
}

/// Decodes a texture level to RGBA8. DXT colours are expanded from 5:6:5;
/// XNA font textures are premultiplied, which is kept as is.
pub fn decode_texture(fmt: SurfaceFormat, w: u32, h: u32, data: &[u8]) -> Option<RgbaImage> {
    let mut img = RgbaImage::new(w, h);
    match fmt {
        SurfaceFormat::Color => {
            let n = (w * h * 4) as usize;
            img.pixels.copy_from_slice(data.get(..n)?);
        }
        SurfaceFormat::Dxt1 | SurfaceFormat::Dxt3 | SurfaceFormat::Dxt5 => {
            let bsize = if fmt == SurfaceFormat::Dxt1 { 8 } else { 16 };
            let bw = w.div_ceil(4);
            let bh = h.div_ceil(4);
            for by in 0..bh {
                for bx in 0..bw {
                    let off = ((by * bw + bx) * bsize) as usize;
                    let block = data.get(off..off + bsize as usize)?;
                    let (alpha, color) = if bsize == 16 {
                        (Some(&block[..8]), &block[8..])
                    } else {
                        (None, block)
                    };
                    let px = decode_bc1(color, fmt == SurfaceFormat::Dxt1);
                    let a = match (fmt, alpha) {
                        (SurfaceFormat::Dxt3, Some(a)) => explicit_alpha(a),
                        (SurfaceFormat::Dxt5, Some(a)) => interpolated_alpha(a),
                        _ => px.map(|p| p[3]),
                    };
                    for i in 0..16u32 {
                        let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                        if x < w && y < h {
                            let p = px[i as usize];
                            img.put(x, y, [p[0], p[1], p[2], a[i as usize]]);
                        }
                    }
                }
            }
        }
        SurfaceFormat::Other(_) => return None,
    }
    Some(img)
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [
        ((r * 255 + 15) / 31) as u8,
        ((g * 255 + 31) / 63) as u8,
        ((b * 255 + 15) / 31) as u8,
    ]
}

fn decode_bc1(b: &[u8], dxt1: bool) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([b[0], b[1]]);
    let c1 = u16::from_le_bytes([b[2], b[3]]);
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mix = |a: u8, b: u8, wa: u32, wb: u32| ((a as u32 * wa + b as u32 * wb) / (wa + wb)) as u8;
    let mut pal = [[0u8; 4]; 4];
    pal[0] = [p0[0], p0[1], p0[2], 255];
    pal[1] = [p1[0], p1[1], p1[2], 255];
    if c0 > c1 || !dxt1 {
        for i in 0..3 {
            pal[2][i] = mix(p0[i], p1[i], 2, 1);
            pal[3][i] = mix(p0[i], p1[i], 1, 2);
        }
        pal[2][3] = 255;
        pal[3][3] = 255;
    } else {
        for i in 0..3 {
            pal[2][i] = mix(p0[i], p1[i], 1, 1);
        }
        pal[2][3] = 255;
        pal[3] = [0, 0, 0, 0];
    }
    let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    let mut out = [[0u8; 4]; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = pal[((bits >> (2 * i)) & 3) as usize];
    }
    out
}

fn explicit_alpha(a: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        let nib = (a[i / 2] >> ((i % 2) * 4)) & 0xf;
        *o = nib * 17;
    }
    out
}

fn interpolated_alpha(a: &[u8]) -> [u8; 16] {
    let (a0, a1) = (a[0] as u32, a[1] as u32);
    let mut pal = [0u32; 8];
    pal[0] = a0;
    pal[1] = a1;
    if a0 > a1 {
        for i in 1..7 {
            pal[i + 1] = ((7 - i as u32) * a0 + i as u32 * a1) / 7;
        }
    } else {
        for i in 1..5 {
            pal[i + 1] = ((5 - i as u32) * a0 + i as u32 * a1) / 5;
        }
        pal[6] = 0;
        pal[7] = 255;
    }
    let mut bits = 0u64;
    for (i, &b) in a[2..8].iter().enumerate() {
        bits |= (b as u64) << (8 * i);
    }
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = pal[((bits >> (3 * i)) & 7) as usize] as u8;
    }
    out
}

/// What an XNB holds, for the record (we don't execute effects).
#[derive(Clone, Debug)]
pub struct XnbReport {
    pub path: PathBuf,
    pub header: XnbHeader,
    /// Primary type reader, short name.
    pub primary: String,
    /// For effects: size of the compiled effect blob and its magic.
    pub effect_size: Option<u32>,
    pub effect_magic: Option<u32>,
    /// Printable identifiers found in the effect blob (parameter, sampler,
    /// technique/pass names, shader model strings).
    pub effect_strings: Vec<String>,
}

pub fn report(path: &Path) -> Result<XnbReport> {
    let d = read_file(path)?;
    let header = read_header(path, &d)?;
    let mut r = Reader::at(&d, header.object_offset);
    let id = read_7bit(&mut r).unwrap_or(0);
    let primary = header
        .readers
        .get((id as usize).wrapping_sub(1))
        .map(|t| t.short_name().to_owned())
        .unwrap_or_default();
    let (mut effect_size, mut effect_magic, mut effect_strings) = (None, None, Vec::new());
    if primary == "EffectReader"
        && let Some(n) = r.u32()
    {
        effect_size = Some(n);
        let blob = r.bytes(n as usize).unwrap_or(&[]);
        effect_magic = Reader::new(blob).u32();
        let mut cur = String::new();
        for &b in blob {
            if b.is_ascii_graphic() || b == b' ' {
                cur.push(b as char);
            } else {
                if cur.len() >= 4 && !effect_strings.contains(&cur) {
                    effect_strings.push(cur.clone());
                }
                cur.clear();
            }
        }
    }
    Ok(XnbReport {
        path: path.to_owned(),
        header,
        primary,
        effect_size,
        effect_magic,
        effect_strings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{installs, require};

    #[test]
    fn fonts_decode() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let misc = steam.content.misc_dir();
        for name in ["Archer", "OpenSans", "MonocleDefault"] {
            let f = SpriteFont::load(&misc.join(format!("{name}.xnb"))).unwrap();
            assert!(f.characters.contains(&'A'), "{name}");
            let i = f.glyph_index('A').unwrap();
            let g = f.glyphs[i];
            assert!(g.x + g.w <= f.texture.width as i32 && g.y + g.h <= f.texture.height as i32);
            // The glyph area must contain some opaque pixels.
            let mut ink = 0;
            for y in g.y..g.y + g.h {
                for x in g.x..g.x + g.w {
                    if f.texture.get(x as u32, y as u32)[3] > 128 {
                        ink += 1;
                    }
                }
            }
            assert!(ink > 3, "{name}: glyph A has no ink");
            eprintln!(
                "{name}: {:?} {}x{}, {} glyphs, line spacing {}",
                f.texture_format,
                f.texture.width,
                f.texture.height,
                f.characters.len(),
                f.line_spacing
            );
        }
    }

    #[test]
    fn effects_report() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let misc = steam.content.misc_dir();
        for name in ["LightingEffect", "ScreenEffects"] {
            let r = report(&misc.join(format!("{name}.xnb"))).unwrap();
            assert_eq!(r.primary, "EffectReader");
            assert_eq!(r.effect_magic, Some(0xbcf0_0bcf));
            assert!(
                r.effect_strings.iter().any(|s| s.contains("Sampler")),
                "{:?}",
                r.effect_strings
            );
        }
    }
}
