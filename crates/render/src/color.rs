//! XNA-style colours: four bytes, used as premultiplied values.

/// An RGBA colour in gamma space, like XNA's `Color`.
///
/// As in XNA, colours given to the sprite batch act as *premultiplied*
/// tints: the fragment is `texel * color` and blending is
/// `src + dst * (1 - src.a)`. So a half-transparent white is
/// `Color::WHITE * 0.5` (all four channels scaled), exactly like the
/// original's `Color.White * 0.5f`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const RED: Color = Color::rgb(255, 0, 0);
    pub const GREEN: Color = Color::rgb(0, 128, 0);
    pub const LIME: Color = Color::rgb(0, 255, 0);
    pub const BLUE: Color = Color::rgb(0, 0, 255);
    pub const YELLOW: Color = Color::rgb(255, 255, 0);
    pub const CYAN: Color = Color::rgb(0, 255, 255);
    pub const MAGENTA: Color = Color::rgb(255, 0, 255);
    pub const GRAY: Color = Color::rgb(128, 128, 128);
    /// XNA's `CornflowerBlue` (100, 149, 237).
    pub const CORNFLOWER_BLUE: Color = Color::rgb(100, 149, 237);

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// `0xRRGGBB`, opaque (the format of the game's XML colours).
    pub const fn hex(rgb: u32) -> Self {
        Self::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
    }

    /// Parses `RRGGBB` or `RRGGBBAA` (optional leading `#`).
    pub fn parse_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        let v = u32::from_str_radix(s, 16).ok()?;
        match s.len() {
            6 => Some(Self::hex(v)),
            8 => Some(Self::rgba(
                (v >> 24) as u8,
                (v >> 16) as u8,
                (v >> 8) as u8,
                v as u8,
            )),
            _ => None,
        }
    }

    /// From floats in 0..1 (rounded like XNA's `new Color(float, ...)`).
    pub fn from_f32(r: f32, g: f32, b: f32, a: f32) -> Self {
        let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        Self::rgba(q(r), q(g), q(b), q(a))
    }

    pub fn to_f32(self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }

    /// Linear interpolation of all four channels (XNA `Color.Lerp`).
    pub fn lerp(self, other: Color, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Self::rgba(
            l(self.r, other.r),
            l(self.g, other.g),
            l(self.b, other.b),
            l(self.a, other.a),
        )
    }

    /// Packed for the GPU (`unorm8x4`, r in the lowest byte).
    pub(crate) fn packed(self) -> u32 {
        u32::from_le_bytes([self.r, self.g, self.b, self.a])
    }

    pub(crate) fn wgpu(self) -> wgpu::Color {
        let [r, g, b, a] = self.to_f32();
        wgpu::Color {
            r: r as f64,
            g: g as f64,
            b: b as f64,
            a: a as f64,
        }
    }
}

/// Scales all four channels, like XNA's `Color * float` (truncating:
/// `Color::WHITE * 0.5` is 127 in every channel).
impl std::ops::Mul<f32> for Color {
    type Output = Color;
    fn mul(self, k: f32) -> Color {
        let k = k.clamp(0.0, 1.0);
        let m = |c: u8| (c as f32 * k) as u8;
        Color::rgba(m(self.r), m(self.g), m(self.b), m(self.a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xna_semantics() {
        assert_eq!(Color::WHITE * 0.5, Color::rgba(127, 127, 127, 127));
        assert_eq!(Color::parse_hex("#ff8000"), Some(Color::rgb(255, 128, 0)));
        assert_eq!(
            Color::parse_hex("10203040"),
            Some(Color::rgba(16, 32, 48, 64))
        );
        assert_eq!(Color::parse_hex("xyz"), None);
        assert_eq!(
            Color::BLACK.lerp(Color::WHITE, 0.5),
            Color::rgb(128, 128, 128)
        );
        assert_eq!(Color::rgba(1, 2, 3, 4).packed(), 0x04030201);
    }
}
