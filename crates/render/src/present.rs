//! Presenting the low-res game canvas in a window of any size.

use glam::Vec2;

use crate::color::Color;

/// How the game canvas is scaled into the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ScaleMode {
    /// Largest whole-number scale that fits (crisp, square pixels); falls
    /// back to [`ScaleMode::Fit`] when the window is smaller than the game.
    Integer,
    /// The original's `LargestScale`: the largest fractional scale that
    /// keeps the aspect ratio (pixel widths may differ by one window
    /// pixel), rectangle sizes truncated and centred like Monocle's.
    #[default]
    Fit,
    /// Fill the window, ignoring the aspect ratio.
    Stretch,
}

impl std::str::FromStr for ScaleMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "integer" | "int" => Ok(Self::Integer),
            "fit" | "aspect" => Ok(Self::Fit),
            "stretch" => Ok(Self::Stretch),
            _ => Err(format!("unknown scale mode {s:?} (integer, fit, stretch)")),
        }
    }
}

/// Where the canvas lands in the window, in window pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Window pixels per game pixel (x, y).
    pub scale: Vec2,
}

impl Viewport {
    /// Computes the viewport of a `game`-sized canvas in a `window`.
    pub fn compute(game: (u32, u32), window: (u32, u32), mode: ScaleMode) -> Self {
        let (gw, gh) = (game.0.max(1) as f32, game.1.max(1) as f32);
        let (ww, wh) = (window.0.max(1) as f32, window.1.max(1) as f32);
        let fit = (ww / gw).min(wh / gh);
        let (sx, sy) = match mode {
            ScaleMode::Integer if fit >= 1.0 => (fit.floor(), fit.floor()),
            ScaleMode::Integer | ScaleMode::Fit => (fit, fit),
            ScaleMode::Stretch => (ww / gw, wh / gh),
        };
        // Monocle: ScaledWidth = (int)(W*scale), x = winW/2 - ScaledWidth/2,
        // y = (winH - ScaledHeight)/2 (integer divisions).
        let (w, h) = ((gw * sx).floor(), (gh * sy).floor());
        let x = (ww / 2.0).floor() - (w / 2.0).floor();
        let y = ((wh - h) / 2.0).floor();
        Self {
            x,
            y,
            w,
            h,
            scale: Vec2::new(w / gw, h / gh),
        }
    }

    /// Maps a window pixel position to game pixels (may be outside the
    /// canvas, e.g. on the letterbox).
    pub fn window_to_game(&self, p: Vec2) -> Vec2 {
        (p - Vec2::new(self.x, self.y)) / self.scale
    }

    /// Maps game pixels to window pixels.
    pub fn game_to_window(&self, p: Vec2) -> Vec2 {
        p * self.scale + Vec2::new(self.x, self.y)
    }
}

/// The original's `ScreenEffects` wave (slow motion, replay rewind): rows
/// of the canvas are displaced horizontally by `amplitude *
/// sin(v * frequency + counter)` (a fraction of the width, `v` = 0..1 down
/// the canvas), wrapping around. Slow motion uses frequency 6 and amplitude
/// `0.05 * (1 - TimeRate)`; the counter advances 0.01 per frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wave {
    pub counter: f32,
    pub amplitude: f32,
    pub frequency: f32,
}

/// Settings for [`Renderer::present`](crate::Renderer::present).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresentSettings {
    pub mode: ScaleMode,
    /// Colour around the canvas.
    pub letterbox: Color,
    /// Screen offset in game pixels (Monocle's `Screen.Offset`, used for
    /// screen shake): the canvas is shifted and *wraps around*, like the
    /// original, which tiles the target so no letterbox shows through.
    /// The shift is rounded to whole window pixels.
    pub offset: Vec2,
    /// The screen wave effect; `None` (or amplitude 0) = off.
    pub wave: Option<Wave>,
}

impl Default for PresentSettings {
    fn default() -> Self {
        Self {
            mode: ScaleMode::Fit,
            letterbox: Color::BLACK,
            offset: Vec2::ZERO,
            wave: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_scale_centres() {
        let v = Viewport::compute((320, 240), (1920, 1080), ScaleMode::Integer);
        assert_eq!((v.x, v.y, v.w, v.h), (320.0, 60.0, 1280.0, 960.0));
        assert_eq!(v.scale, Vec2::splat(4.0));
        let v = Viewport::compute((420, 240), (1920, 1080), ScaleMode::Integer);
        assert_eq!((v.w, v.h), (1680.0, 960.0));
        // Window smaller than the game: fit instead of scale 0.
        let v = Viewport::compute((320, 240), (160, 240), ScaleMode::Integer);
        assert_eq!((v.w, v.h, v.y), (160.0, 120.0, 60.0));
    }

    #[test]
    fn fit_and_stretch() {
        let v = Viewport::compute((320, 240), (1920, 1080), ScaleMode::Fit);
        assert_eq!((v.x, v.y, v.w, v.h), (240.0, 0.0, 1440.0, 1080.0));
        // Truncation like Monocle: 1366x768 -> scale 3.2, 1024x768 at x 171.
        let v = Viewport::compute((320, 240), (1366, 768), ScaleMode::Fit);
        assert_eq!((v.x, v.y, v.w, v.h), (171.0, 0.0, 1024.0, 768.0));
        let v = Viewport::compute((420, 240), (1000, 1000), ScaleMode::Fit);
        assert_eq!((v.x, v.y, v.w, v.h), (0.0, 214.0, 1000.0, 571.0));
        let v = Viewport::compute((320, 240), (1920, 1080), ScaleMode::Stretch);
        assert_eq!((v.x, v.y, v.w, v.h), (0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(v.scale, Vec2::new(6.0, 4.5));
    }

    #[test]
    fn mapping_roundtrip() {
        let v = Viewport::compute((320, 240), (1280, 720), ScaleMode::Integer);
        let g = v.window_to_game(Vec2::new(v.x + 30.0, v.y + 3.0));
        assert_eq!(g, Vec2::new(10.0, 1.0));
        assert_eq!(v.game_to_window(g), Vec2::new(v.x + 30.0, v.y + 3.0));
        assert_eq!("fit".parse::<ScaleMode>(), Ok(ScaleMode::Fit));
    }
}
