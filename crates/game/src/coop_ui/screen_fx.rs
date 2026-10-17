//! Screen effects of the co-op scene that the sim exposes but the level
//! renderer does not do by itself: the camera transform of the composited
//! level (Dark World's exit-portal zoom, darkworld.md 5.5; the space orb /
//! Quest scroll waves' camera offset with the playfield wrapping,
//! `View::camera_offset`), and the slow-motion screen wave of the time orb
//! / Quest slow waves (rendering.md 8: frequency 6, amplitude
//! `0.05 * (1 - TimeRate)`, counter +0.01 per tick). The dark orb / Quest
//! dark waves need no code here: `View::dark` goes to `TowerView`.
//!
//! The level is composited into a canvas (`TowerView::screen_target`) and
//! drawn back transformed; the HUD stays untransformed on top.

use tfp_render::glam::Vec2;
use tfp_render::{Color, DrawList, Rect, SpriteParams, SubTexture, TextureId, Wave};

/// How the composited level is drawn to the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LevelXform {
    /// Zoom about a point (scale, point).
    pub zoom: Option<(f32, Vec2)>,
    /// Camera offset in pixels (the image wraps).
    pub offset: Vec2,
}

impl LevelXform {
    pub fn active(&self) -> bool {
        self.zoom.is_some() || self.offset.x.abs() >= 0.5 || self.offset.y.abs() >= 0.5
    }
}

/// Draws the level canvas `tex` (`size`) to the default target.
pub fn draw_level(list: &mut DrawList, tex: TextureId, size: (u32, u32), x: &LevelXform) {
    let (w, h) = (size.0 as f32, size.1 as f32);
    let sub = SubTexture::new(tex, Rect::new(0, 0, size.0 as i32, size.1 as i32));
    list.begin_pass(None, Some(Color::BLACK));
    if let Some((k, focus)) = x.zoom {
        list.sprite(
            sub,
            &SpriteParams::at(focus).origin(focus).scale(Vec2::splat(k)),
        );
        return;
    }
    // The camera at `offset`: the image moves the other way and wraps.
    let ox = (-x.offset.x).round().rem_euclid(w);
    let oy = (-x.offset.y).round().rem_euclid(h);
    for dx in [ox - w, ox] {
        for dy in [oy - h, oy] {
            list.sprite(sub, &SpriteParams::at(Vec2::new(dx, dy)));
        }
    }
}

/// The slow-motion screen wave while time runs slow (`None` at full speed).
pub fn slow_wave(time_rate: f32, counter: f32) -> Option<Wave> {
    (time_rate < 1.0).then_some(Wave {
        counter,
        amplitude: 0.05 * (1.0 - time_rate.max(0.0)),
        frequency: 6.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_and_xform() {
        assert!(slow_wave(1.0, 0.0).is_none());
        let w = slow_wave(0.5, 1.0).unwrap();
        assert!((w.amplitude - 0.025).abs() < 1e-6 && w.frequency == 6.0);
        assert!(!LevelXform::default().active());
        assert!(
            LevelXform {
                offset: Vec2::new(3.0, 0.0),
                ..Default::default()
            }
            .active()
        );
    }
}
