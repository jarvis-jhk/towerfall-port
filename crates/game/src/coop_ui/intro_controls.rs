//! `IntroControls` (quest.md 3 step 3): the controls panel before the
//! first Quest tower's title card in Normal mode. One 60x206 panel per
//! distinct joined device, 76 px apart: the device badge above, MOVE + AIM
//! (the stick, or the four direction keys), JUMP, SHOOT, DODGE + CATCH with
//! the device's icons. Flies in from the top (20 ticks after its sound,
//! 20 ticks CubeOut), waits for Confirm, leaves downward (20 ticks CubeIn).
//! The quest waits for it (the scene does not step the sim meanwhile).

use tfp_render::Color;
use tfp_render::glam::Vec2;

use crate::front::gfx::{Org, Painter, cube_in, cube_out};
use crate::front::minput::{IconSet, KeyLayout, key_icon};

/// One device's column.
#[derive(Clone, Debug)]
pub struct Column {
    pub badge: String,
    /// The stick, or `None` for the four direction keys.
    pub stick: Option<String>,
    /// Up, down, left, right.
    pub keys: [String; 4],
    pub jump: String,
    pub shoot: String,
    pub dodge: String,
}

impl Column {
    pub fn pad(set: IconSet, n: usize) -> Column {
        let (stick, jump, shoot, dodge) = match set {
            IconSet::DualShock => (
                "controls/dualshock/stick",
                "controls/dualshock/x",
                "controls/dualshock/square",
                "controls/dualshock/r2",
            ),
            _ => (
                "controls/xb360/stick",
                "controls/xb360/a",
                "controls/xb360/x",
                "controls/xb360/rt",
            ),
        };
        Column {
            badge: set.badge(n),
            stick: Some(stick.into()),
            keys: Default::default(),
            jump: jump.into(),
            shoot: shoot.into(),
            dodge: dodge.into(),
        }
    }

    pub fn keyboard(l: &KeyLayout) -> Column {
        let k = |v: &[sdl2::keyboard::Scancode]| {
            format!(
                "controls/keyboard/{}",
                v.first().and_then(|&s| key_icon(s)).unwrap_or("undefined")
            )
        };
        Column {
            badge: "controls/keyboard/icon".into(),
            stick: None,
            keys: [k(&l.up), k(&l.down), k(&l.left), k(&l.right)],
            jump: k(&l.jump),
            shoot: k(&l.shoot),
            dodge: k(&l.dodge),
        }
    }
}

pub struct IntroControls {
    pub columns: Vec<Column>,
    t: u32,
    out: Option<u32>,
}

impl IntroControls {
    pub fn new(columns: Vec<Column>) -> Self {
        Self {
            columns,
            t: 0,
            out: None,
        }
    }

    /// One tick; `confirm` = a Confirm press. Returns a sound cue.
    pub fn tick(&mut self, confirm: bool) -> Option<&'static str> {
        self.t += 1;
        if let Some(o) = &mut self.out {
            *o += 1;
            return None;
        }
        if self.t == 1 {
            return Some("sfx_multiStartLevelControlFlyin");
        }
        if self.t > 40 && confirm {
            self.out = Some(0);
            return Some("sfx_multiStartLevelControlFlyout");
        }
        None
    }

    pub fn ticks(&self) -> u32 {
        self.t
    }

    pub fn finished(&self) -> bool {
        self.out.is_some_and(|o| o >= 20)
    }

    fn y(&self) -> f32 {
        match self.out {
            Some(o) => 120.0 + 240.0 * cube_in(o as f32 / 20.0),
            None => -120.0 + 240.0 * cube_out((self.t as f32 - 20.0).max(0.0) / 20.0),
        }
    }

    pub fn draw(&self, p: &mut Painter) {
        let n = self.columns.len().max(1);
        let cx = p.w / 2.0;
        let y0 = self.y();
        let span = 76.0 * (n as f32 - 1.0);
        let gray = Color::rgb(128, 128, 128);
        for (i, col) in self.columns.iter().enumerate() {
            let x = cx - span / 2.0 + 76.0 * i as f32;
            p.panel(x - 30.0, y0 - 100.0, 60.0, 206.0);
            let mut at = Vec2::new(x, y0 - 88.0 + 16.0);
            p.m(
                &col.badge,
                Vec2::new(x, y0 - 88.0 - 30.0 + 16.0),
                Org::Center,
                1.0,
                Color::WHITE,
            );
            match &col.stick {
                Some(s) => p.m(s, at, Org::Center, 1.0, Color::WHITE),
                None => {
                    for (k, off) in col.keys.iter().zip([
                        Vec2::new(0.0, -7.0),
                        Vec2::new(0.0, 8.0),
                        Vec2::new(-13.0, 8.0),
                        Vec2::new(13.0, 8.0),
                    ]) {
                        p.m(k, at + off, Org::Center, 1.0, Color::WHITE);
                    }
                }
            }
            p.text("MOVE", at + Vec2::new(0.0, 20.0), (0.5, 0.0), 1.0, gray);
            p.text("+ AIM", at + Vec2::new(0.0, 26.0), (0.5, 0.0), 1.0, gray);
            at.y += 44.0;
            p.m(
                &col.jump,
                at + Vec2::new(0.0, 3.0),
                Org::Center,
                1.0,
                Color::WHITE,
            );
            p.text("JUMP", at + Vec2::new(0.0, 18.0), (0.5, 0.0), 1.0, gray);
            at.y += 44.0;
            p.m(
                &col.shoot,
                at + Vec2::new(0.0, 3.0),
                Org::Center,
                1.0,
                Color::WHITE,
            );
            p.text("SHOOT", at + Vec2::new(0.0, 18.0), (0.5, 0.0), 1.0, gray);
            at.y += 44.0;
            p.m(
                &col.dodge,
                at + Vec2::new(0.0, 1.0),
                Org::Center,
                1.0,
                Color::WHITE,
            );
            p.text("DODGE", at + Vec2::new(0.0, 18.0), (0.5, 0.0), 1.0, gray);
            p.text("+ CATCH", at + Vec2::new(0.0, 24.0), (0.5, 0.0), 1.0, gray);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline() {
        let mut c = IntroControls::new(vec![Column::keyboard(&KeyLayout::default_config())]);
        assert_eq!(c.tick(false), Some("sfx_multiStartLevelControlFlyin"));
        for _ in 0..40 {
            assert!(c.tick(true).is_none() || c.out.is_some());
        }
        let s = c.tick(true);
        assert!(s.is_some() || c.out.is_some());
        for _ in 0..25 {
            c.tick(false);
        }
        assert!(c.finished());
        assert_eq!(c.columns[0].jump, "controls/keyboard/C");
    }
}
