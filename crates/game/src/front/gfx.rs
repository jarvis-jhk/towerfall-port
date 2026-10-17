//! Drawing helpers in the style of Monocle's `Draw` (ui.md 14): positions
//! floored, outlined text and textures, justified text, 9-slice panels,
//! sine-sliced textures.

use tfp_render::glam::Vec2;
use tfp_render::{Color, DrawList, SpriteFont, SpriteParams, SubTexture};

use super::assets::UiAssets;

/// Menu palette (ui.md 0).
pub mod pal {
    use tfp_render::Color;
    pub const SELECTED: Color = Color::hex(0xD8F878);
    pub const YELLOW: Color = Color::hex(0xEAC831);
    pub const IDLE: Color = Color::hex(0x3CBCFC);
    pub const SUBTITLE: Color = Color::hex(0xFFDC6B);
    pub const BLADE: Color = Color::hex(0xFFDA9B);
    pub const PAUSE_A: Color = Color::hex(0xF87858);
    pub const PAUSE_B: Color = Color::hex(0xF7BB59);
    pub const PAUSE_IDLE: Color = Color::hex(0xF8F8F8);
    pub const RED: Color = Color::hex(0xFF3030);
    pub const GOLD: Color = Color::hex(0xEFD96B);
}

pub fn rgba(c: [u8; 4]) -> Color {
    Color::rgba(c[0], c[1], c[2], c[3])
}

/// Easing (engine.md 4).
pub fn cube_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    t * t * t + 1.0
}

pub fn cube_in(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t
}

pub fn cube_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

pub fn back_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    let s = 1.70158;
    t * t * ((s + 1.0) * t + s) + 1.0
}

pub fn lerp(a: Vec2, b: Vec2, t: f32) -> Vec2 {
    a + (b - a) * t
}

/// Sine wave value of a Monocle `SineWave(frames)` at `tick`.
pub fn sine(tick: u64, period: f32, phase: f32) -> f32 {
    (tick as f32 / period * std::f32::consts::TAU + phase).sin()
}

pub struct Painter<'a> {
    pub list: &'a mut DrawList,
    pub a: &'a UiAssets,
    /// Virtual screen width (320 / 420).
    pub w: f32,
}

#[derive(Clone, Copy)]
pub enum Org {
    TopLeft,
    Center,
    /// Fraction of the size (justify).
    Just(f32, f32),
    /// Pixels.
    At(f32, f32),
}

impl Org {
    fn px(self, size: Vec2) -> Vec2 {
        match self {
            Org::TopLeft => Vec2::ZERO,
            Org::Center => (size / 2.0).floor(),
            Org::Just(x, y) => size * Vec2::new(x, y),
            Org::At(x, y) => Vec2::new(x, y),
        }
    }
}

impl Painter<'_> {
    pub fn tex(&mut self, sub: SubTexture, pos: Vec2, org: Org, scale: f32, color: Color) {
        self.tex_ex(sub, pos, org, Vec2::splat(scale), 0.0, color, false);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn tex_ex(
        &mut self,
        sub: SubTexture,
        pos: Vec2,
        org: Org,
        scale: Vec2,
        rot: f32,
        color: Color,
        flip_x: bool,
    ) {
        self.list.sprite(
            sub,
            &SpriteParams::at(pos.floor())
                .origin(org.px(sub.rect.size()))
                .scale(scale)
                .rotation(rot)
                .color(color)
                .flip(flip_x, false),
        );
    }

    /// Eight black copies offset by one pixel, then the texture.
    pub fn tex_outlined(&mut self, sub: SubTexture, pos: Vec2, org: Org, scale: f32, color: Color) {
        let black = Color::BLACK * (color.a as f32 / 255.0);
        for (dx, dy) in NEIGHBOURS {
            self.tex(sub, pos + Vec2::new(dx, dy), org, scale, black);
        }
        self.tex(sub, pos, org, scale, color);
    }

    /// A named menu-atlas image; a magenta box if it is missing.
    pub fn m(&mut self, name: &str, pos: Vec2, org: Org, scale: f32, color: Color) {
        match self.a.m(name) {
            Some(s) => self.tex(s, pos, org, scale, color),
            None => self.missing(pos, org, scale),
        }
    }

    pub fn m_outlined(&mut self, name: &str, pos: Vec2, org: Org, scale: f32, color: Color) {
        match self.a.m(name) {
            Some(s) => self.tex_outlined(s, pos, org, scale, color),
            None => self.missing(pos, org, scale),
        }
    }

    fn missing(&mut self, pos: Vec2, org: Org, scale: f32) {
        let s = Vec2::splat(8.0 * scale);
        let p = pos - org.px(s);
        self.list.hollow_rect(p.x, p.y, s.x, s.y, Color::MAGENTA);
    }

    fn font(&self) -> &SpriteFont {
        &self.a.font
    }

    pub fn measure(&self, s: &str) -> Vec2 {
        self.font().measure(s)
    }

    /// Text justified at (jx, jy) of its size, scaled about that point.
    pub fn text(&mut self, s: &str, pos: Vec2, just: (f32, f32), scale: f32, color: Color) {
        let size = self.font().measure(s);
        let at = (pos - size * Vec2::new(just.0, just.1) * scale).floor();
        self.a.font.draw(self.list, s, at, color, scale);
    }

    pub fn text_outlined(
        &mut self,
        s: &str,
        pos: Vec2,
        just: (f32, f32),
        scale: f32,
        color: Color,
    ) {
        let black = Color::BLACK * (color.a as f32 / 255.0);
        for (dx, dy) in NEIGHBOURS {
            self.text(s, pos + Vec2::new(dx, dy), just, scale, black);
        }
        self.text(s, pos, just, scale, color);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Color) {
        self.list.rect(x, y, w, h, c);
    }

    /// `MenuPanel.DrawPanel`: black fill inset 2 px, 30-px corners, 10-px
    /// edges tiled.
    pub fn panel(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let (x, y, w, h) = (x.floor(), y.floor(), w.floor(), h.floor());
        self.rect(x + 2.0, y + 2.0, w - 4.0, h - 4.0, Color::BLACK);
        let Some(tl) = self.a.m("panel/topLeft") else {
            self.list.hollow_rect(x, y, w, h, Color::WHITE);
            return;
        };
        let get = |n: &str| self.a.m(n).unwrap_or(tl);
        let (t, b, l, r) = (
            get("panel/top"),
            get("panel/bottom"),
            get("panel/left"),
            get("panel/right"),
        );
        let (tr, bl, br) = (
            get("panel/topRight"),
            get("panel/bottomLeft"),
            get("panel/bottomRight"),
        );
        let wc = 30.0f32.min(w / 2.0);
        let hc = 30.0f32.min(h / 2.0);
        // Edges: tile segments between the corners.
        let mut xx = x + wc;
        while xx < x + w - wc {
            let seg = (x + w - wc - xx).min(t.width() as f32) as i32;
            self.tex(
                t.sub(0, 0, seg, t.height()),
                Vec2::new(xx, y),
                Org::TopLeft,
                1.0,
                Color::WHITE,
            );
            self.tex(
                b.sub(0, 0, seg, b.height()),
                Vec2::new(xx, y + h - b.height() as f32),
                Org::TopLeft,
                1.0,
                Color::WHITE,
            );
            xx += t.width() as f32;
        }
        let mut yy = y + hc;
        while yy < y + h - hc {
            let seg = (y + h - hc - yy).min(l.height() as f32) as i32;
            self.tex(
                l.sub(0, 0, l.width(), seg),
                Vec2::new(x, yy),
                Org::TopLeft,
                1.0,
                Color::WHITE,
            );
            self.tex(
                r.sub(0, 0, r.width(), seg),
                Vec2::new(x + w - r.width() as f32, yy),
                Org::TopLeft,
                1.0,
                Color::WHITE,
            );
            yy += l.height() as f32;
        }
        let c = |s: SubTexture, cw: f32, ch: f32, right: bool, bottom: bool| {
            let sx = if right { s.width() - cw as i32 } else { 0 };
            let sy = if bottom { s.height() - ch as i32 } else { 0 };
            s.sub(sx, sy, cw as i32, ch as i32)
        };
        self.tex(
            c(tl, wc, hc, false, false),
            Vec2::new(x, y),
            Org::TopLeft,
            1.0,
            Color::WHITE,
        );
        self.tex(
            c(tr, wc, hc, true, false),
            Vec2::new(x + w - wc, y),
            Org::TopLeft,
            1.0,
            Color::WHITE,
        );
        self.tex(
            c(bl, wc, hc, false, true),
            Vec2::new(x, y + h - hc),
            Org::TopLeft,
            1.0,
            Color::WHITE,
        );
        self.tex(
            c(br, wc, hc, true, true),
            Vec2::new(x + w - wc, y + h - hc),
            Org::TopLeft,
            1.0,
            Color::WHITE,
        );
    }

    /// `Draw.SineTextureV`: horizontal slices of `slice` px, each shifted by
    /// `round(sin(counter + add*i) * amp)`.
    pub fn sine_texture_v(
        &mut self,
        sub: SubTexture,
        pos: Vec2,
        counter: f32,
        amp: f32,
        slice: i32,
        add: f32,
    ) {
        let pos = pos.floor();
        let mut y = 0;
        let mut i = 0;
        while y < sub.height() {
            let hgt = slice.min(sub.height() - y);
            let dx = ((counter + add * i as f32).sin() * amp).round();
            self.tex(
                sub.sub(0, y, sub.width(), hgt),
                pos + Vec2::new(dx, y as f32),
                Org::TopLeft,
                1.0,
                Color::WHITE,
            );
            y += slice;
            i += 1;
        }
    }

    /// A key/button icon of a device function.
    pub fn icon(&mut self, name: &str, pos: Vec2, org: Org) {
        self.m(name, pos, org, 1.0, Color::WHITE);
    }

    /// A device badge for player `n` (0-based). The art only exists for
    /// players 1-4; for 5-12 (ours) the 1-4 art is dimmed and the player
    /// number is drawn over it, in place of the lit quadrant.
    pub fn badge(&mut self, name: &str, n: usize, pos: Vec2) {
        if n < 4 {
            self.icon(name, pos, Org::Center);
            return;
        }
        self.m(name, pos, Org::Center, 1.0, Color::WHITE * 0.45);
        self.text_outlined(&format!("{}", n + 1), pos, (0.5, 0.5), 1.0, Color::WHITE);
    }
}

const NEIGHBOURS: [(f32, f32); 8] = [
    (-1.0, -1.0),
    (0.0, -1.0),
    (1.0, -1.0),
    (-1.0, 0.0),
    (1.0, 0.0),
    (-1.0, 1.0),
    (0.0, 1.0),
    (1.0, 1.0),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_ends() {
        for f in [cube_out, cube_in, cube_in_out, back_out] {
            assert!(f(0.0).abs() < 1e-6 && (f(1.0) - 1.0).abs() < 1e-6);
        }
        assert!(back_out(0.8) > 1.0, "overshoots");
    }
}
