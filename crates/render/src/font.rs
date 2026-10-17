//! Bitmap fonts shaped like XNA's `SpriteFont` (what the original's
//! `Content/Misc/*.xnb` fonts contain), plus a tiny built-in debug font.
//!
//! The assets crate parses XNB SpriteFonts into the same fields (texture
//! image, per-glyph bounds/cropping/kerning, line spacing, spacing); the
//! game uploads the image and builds a [`SpriteFont`] from them.

use std::collections::BTreeMap;

use glam::Vec2;

use crate::color::Color;
use crate::draw::{DrawList, Rect, SpriteParams, SubTexture};
use crate::image::Image;
use crate::texture::TextureId;

/// One glyph, XNA `SpriteFont` style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    /// Source rectangle in the font texture.
    pub bounds: Rect,
    /// Offset of the bitmap inside the character cell (`cropping.x/y`).
    pub offset: (i32, i32),
    /// Left side bearing, glyph width, right side bearing (XNA `Kerning`).
    pub kerning: [f32; 3],
}

/// A bitmap font: glyphs in one texture.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteFont {
    pub texture: TextureId,
    pub line_spacing: i32,
    /// Extra space between characters.
    pub spacing: f32,
    pub glyphs: BTreeMap<char, Glyph>,
    /// Used for characters without a glyph (`None`: they are skipped).
    pub default_char: Option<char>,
}

impl SpriteFont {
    fn glyph(&self, c: char) -> Option<&Glyph> {
        self.glyphs
            .get(&c)
            .or_else(|| self.default_char.and_then(|d| self.glyphs.get(&d)))
    }

    /// Runs XNA's layout: calls `f(glyph, top_left)` per visible glyph and
    /// returns the measured size (`SpriteFont.MeasureString`).
    fn layout(&self, text: &str, mut f: impl FnMut(&Glyph, Vec2)) -> Vec2 {
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        let mut width = 0.0f32;
        let mut first = true;
        let mut lines = 1;
        for c in text.chars() {
            match c {
                '\r' => continue,
                '\n' => {
                    x = 0.0;
                    y += self.line_spacing as f32;
                    first = true;
                    lines += 1;
                    continue;
                }
                _ => {}
            }
            let Some(g) = self.glyph(c) else { continue };
            let [left, w, right] = g.kerning;
            if first {
                x = left.max(0.0);
                first = false;
            } else {
                x += self.spacing + left;
            }
            f(g, Vec2::new(x + g.offset.0 as f32, y + g.offset.1 as f32));
            x += w + right;
            width = width.max(x);
        }
        Vec2::new(width, (lines * self.line_spacing) as f32)
    }

    /// Size of `text` in pixels.
    pub fn measure(&self, text: &str) -> Vec2 {
        self.layout(text, |_, _| {})
    }

    /// Draws `text` with its top-left at `position` (rounded to whole
    /// pixels), scaled around `position` by `scale`.
    pub fn draw(&self, list: &mut DrawList, text: &str, position: Vec2, color: Color, scale: f32) {
        let tex = self.texture;
        let base = position.round();
        self.layout(text, |g, at| {
            list.sprite(
                SubTexture::new(tex, g.bounds),
                &SpriteParams::at(base + at * scale)
                    .scale(Vec2::splat(scale))
                    .color(color),
            );
        });
    }

    /// Draws `text` so that `anchor` (0..1 of its size, e.g. (0.5, 0.5) =
    /// centred) lands on `position`.
    pub fn draw_anchored(
        &self,
        list: &mut DrawList,
        text: &str,
        position: Vec2,
        anchor: Vec2,
        color: Color,
    ) {
        let size = self.measure(text);
        self.draw(list, text, (position - size * anchor).floor(), color, 1.0);
    }
}

/// The built-in font: 3x5 pixels, upper case only (lower case is drawn as
/// upper case). Rows top to bottom, `#` = set.
const DEBUG_FONT: &[(char, [&str; 5])] = &[
    ('0', ["###", "#.#", "#.#", "#.#", "###"]),
    ('1', [".#.", "##.", ".#.", ".#.", "###"]),
    ('2', ["##.", "..#", ".#.", "#..", "###"]),
    ('3', ["##.", "..#", ".#.", "..#", "##."]),
    ('4', ["#.#", "#.#", "###", "..#", "..#"]),
    ('5', ["###", "#..", "##.", "..#", "##."]),
    ('6', [".##", "#..", "###", "#.#", "###"]),
    ('7', ["###", "..#", ".#.", ".#.", ".#."]),
    ('8', ["###", "#.#", "###", "#.#", "###"]),
    ('9', ["###", "#.#", "###", "..#", "##."]),
    ('A', [".#.", "#.#", "###", "#.#", "#.#"]),
    ('B', ["##.", "#.#", "##.", "#.#", "##."]),
    ('C', [".##", "#..", "#..", "#..", ".##"]),
    ('D', ["##.", "#.#", "#.#", "#.#", "##."]),
    ('E', ["###", "#..", "##.", "#..", "###"]),
    ('F', ["###", "#..", "##.", "#..", "#.."]),
    ('G', [".##", "#..", "#.#", "#.#", ".##"]),
    ('H', ["#.#", "#.#", "###", "#.#", "#.#"]),
    ('I', ["###", ".#.", ".#.", ".#.", "###"]),
    ('J', ["..#", "..#", "..#", "#.#", ".#."]),
    ('K', ["#.#", "#.#", "##.", "#.#", "#.#"]),
    ('L', ["#..", "#..", "#..", "#..", "###"]),
    ('M', ["#.#", "###", "###", "#.#", "#.#"]),
    ('N', ["##.", "#.#", "#.#", "#.#", "#.#"]),
    ('O', [".#.", "#.#", "#.#", "#.#", ".#."]),
    ('P', ["##.", "#.#", "##.", "#..", "#.."]),
    ('Q', [".#.", "#.#", "#.#", "##.", ".##"]),
    ('R', ["##.", "#.#", "##.", "#.#", "#.#"]),
    ('S', [".##", "#..", ".#.", "..#", "##."]),
    ('T', ["###", ".#.", ".#.", ".#.", ".#."]),
    ('U', ["#.#", "#.#", "#.#", "#.#", "###"]),
    ('V', ["#.#", "#.#", "#.#", "#.#", ".#."]),
    ('W', ["#.#", "#.#", "###", "###", "#.#"]),
    ('X', ["#.#", "#.#", ".#.", "#.#", "#.#"]),
    ('Y', ["#.#", "#.#", ".#.", ".#.", ".#."]),
    ('Z', ["###", "..#", ".#.", "#..", "###"]),
    (' ', ["...", "...", "...", "...", "..."]),
    ('.', ["...", "...", "...", "...", ".#."]),
    (',', ["...", "...", "...", ".#.", "#.."]),
    (':', ["...", ".#.", "...", ".#.", "..."]),
    (';', ["...", ".#.", "...", ".#.", "#.."]),
    ('!', [".#.", ".#.", ".#.", "...", ".#."]),
    ('?', ["##.", "..#", ".#.", "...", ".#."]),
    ('-', ["...", "...", "###", "...", "..."]),
    ('+', ["...", ".#.", "###", ".#.", "..."]),
    ('=', ["...", "###", "...", "###", "..."]),
    ('_', ["...", "...", "...", "...", "###"]),
    ('/', ["..#", "..#", ".#.", "#..", "#.."]),
    ('\\', ["#..", "#..", ".#.", "..#", "..#"]),
    ('(', [".#.", "#..", "#..", "#..", ".#."]),
    (')', [".#.", "..#", "..#", "..#", ".#."]),
    ('[', ["##.", "#..", "#..", "#..", "##."]),
    (']', [".##", "..#", "..#", "..#", ".##"]),
    ('<', ["..#", ".#.", "#..", ".#.", "..#"]),
    ('>', ["#..", ".#.", "..#", ".#.", "#.."]),
    ('\'', [".#.", ".#.", "...", "...", "..."]),
    ('"', ["#.#", "#.#", "...", "...", "..."]),
    ('#', ["#.#", "###", "#.#", "###", "#.#"]),
    ('%', ["#.#", "..#", ".#.", "#..", "#.#"]),
    ('*', ["#.#", ".#.", "#.#", "...", "..."]),
    ('x', ["...", "#.#", ".#.", "#.#", "..."]),
];

/// The built-in debug font's bitmap and metrics: 3x5 glyphs, 1 px apart,
/// line spacing 6. Upload the image with
/// [`Renderer::create_texture`](crate::Renderer::create_texture) and pass
/// the id to [`debug_font`].
pub fn debug_font_image() -> Image {
    let n = DEBUG_FONT.len() as u32;
    let mut img = Image::new(n * 4, 5);
    for (i, (_, rows)) in DEBUG_FONT.iter().enumerate() {
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.bytes().enumerate() {
                if c == b'#' {
                    img.set_pixel(i as u32 * 4 + x as u32, y as u32, [255; 4]);
                }
            }
        }
    }
    img
}

/// The built-in debug font on `texture` (made from [`debug_font_image`]).
/// Lower-case letters map to upper case.
pub fn debug_font(texture: TextureId) -> SpriteFont {
    let mut glyphs = BTreeMap::new();
    for (i, (c, _)) in DEBUG_FONT.iter().enumerate() {
        let g = Glyph {
            bounds: Rect::new(i as i32 * 4, 0, 3, 5),
            offset: (0, 0),
            kerning: [0.0, 3.0, 1.0],
        };
        glyphs.insert(*c, g);
        if c.is_ascii_uppercase() {
            glyphs.entry(c.to_ascii_lowercase()).or_insert(g);
        }
    }
    SpriteFont {
        texture,
        line_spacing: 6,
        spacing: 0.0,
        glyphs,
        default_char: Some('?'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xna_layout_and_measure() {
        let f = debug_font(TextureId(9));
        assert_eq!(f.measure("AB"), Vec2::new(8.0, 6.0));
        assert_eq!(f.measure("A\nABC"), Vec2::new(12.0, 12.0));
        let mut dl = DrawList::new();
        f.draw(&mut dl, "hi 1", Vec2::new(10.4, 20.0), Color::WHITE, 1.0);
        assert_eq!(dl.quad_count(), 4);
        assert_eq!(dl.quads[1].p0, [14.0, 20.0]);
        // Unknown characters fall back to the default glyph.
        assert_eq!(f.measure("\u{263a}"), f.measure("?"));
    }

    #[test]
    fn image_matches_table() {
        let img = debug_font_image();
        assert_eq!(img.width, DEBUG_FONT.len() as u32 * 4);
        // '1' is glyph 1: its top row is ".#.".
        assert_eq!(img.pixel(4 + 1, 0)[3], 255);
        assert_eq!(img.pixel(4, 0)[3], 0);
    }
}
