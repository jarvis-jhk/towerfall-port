//! The in-match pause (ui.md 10): Start from any device opens the Versus
//! pause menu (RESUME / QUIT); a joined player's controller going missing
//! pauses with the "CONTROLLER (Pn) DISCONNECTED!" panel, which gives way
//! to the menu once the pad is back or after 240 frames. The match scene
//! owns it and stops stepping the sim while it is open.

use tfp_render::Color;
use tfp_render::glam::Vec2;

use crate::front::gfx::{Org, Painter};
use crate::front::minput::MenuInput;
use crate::front::pause::{Action, MenuType, PauseMenu};

/// Frames the disconnect panel stays before the menu shows (input.md 5).
pub const PANEL_FRAMES: u32 = 240;
/// No pausing during the first frames of a level (ui.md 10.1).
pub const LOCKOUT: u32 = 15;

pub struct MatchPause {
    pub menu: PauseMenu,
    /// The slot whose controller is missing, frames shown, its colour B
    /// and its device badge.
    pub panel: Option<Panel>,
}

pub struct Panel {
    pub slot: usize,
    pub t: u32,
    pub color: Color,
    pub badge: String,
}

impl MatchPause {
    pub fn new() -> Self {
        Self {
            menu: PauseMenu::new(MenuType::VersusPause),
            panel: None,
        }
    }

    /// One frame. `present(slot)`: the slot has a controller again.
    /// Returns the menu's action and pushes UI sounds.
    pub fn step(
        &mut self,
        mi: &MenuInput,
        present: impl Fn(usize) -> bool,
        sounds: &mut Vec<&'static str>,
    ) -> Option<Action> {
        if let Some(p) = &mut self.panel {
            p.t += 1;
            if present(p.slot) || p.t >= PANEL_FRAMES {
                self.panel = None;
                // The menu starts fresh (Confirm is ignored for 10 frames).
                self.menu = PauseMenu::new(MenuType::VersusPause);
            }
            return None;
        }
        self.menu.step(mi, sounds)
    }

    pub fn draw(&self, p: &mut Painter, tick: u64) {
        let Some(panel) = &self.panel else {
            self.menu.draw(p, tick);
            return;
        };
        let cx = p.w / 2.0;
        p.rect(0.0, 0.0, p.w, 240.0, Color::BLACK * 0.5);
        p.panel(cx - 85.0, 85.0, 170.0, 70.0);
        p.text(
            &format!("CONTROLLER (P{}) DISCONNECTED!", panel.slot + 1),
            Vec2::new(cx, 135.0),
            (0.5, 0.5),
            1.0,
            panel.color,
        );
        p.badge(&panel.badge, panel.slot, Vec2::new(cx, 115.0));
        // The atlas key is spelled this way in the original's menu atlas.
        p.m(
            "controls/disconected",
            Vec2::new(cx, 115.0),
            Org::Center,
            1.0,
            panel.color,
        );
    }
}
