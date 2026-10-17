//! Controller button mapping (ours; the original has no pad rebinding,
//! input.md 2.1): pick a controller by pressing a button on it, Confirm to
//! capture one input per gameplay action, Alt to reset the model to the
//! default layout. Maps are kept per controller model (SDL name) in the
//! options; menus keep their fixed buttons.

use sdl2::controller::{Axis, Button};
use tfp_render::Color;
use tfp_render::glam::Vec2;

use super::gfx::{Org, Painter, pal};
use super::minput::b;
use super::{Ctx, Go, Screen, ScreenId, View};
use crate::padmap::{BINDABLE, PadAction, PadMap};

pub struct ControllerConfig {
    /// The controller being configured: SDL instance id and model name.
    pad: Option<(u32, String)>,
    /// Capturing the input for action `i`.
    prompt: Option<usize>,
    building: PadMap,
    /// Inputs must be released before the next capture.
    released: bool,
    prev_triggers: (f32, f32),
    invalid_at: u64,
}

impl ControllerConfig {
    pub fn new() -> Self {
        Self {
            pad: None,
            prompt: None,
            building: PadMap::default(),
            released: false,
            prev_triggers: (0.0, 0.0),
            invalid_at: 0,
        }
    }

    fn current(&self, v: &View) -> PadMap {
        match (&self.prompt, &self.pad) {
            (Some(_), _) => self.building.clone(),
            (None, Some((_, name))) => v.s.options.pads.get(name).cloned().unwrap_or_default(),
            (None, None) => PadMap::default(),
        }
    }
}

impl Screen for ControllerConfig {
    fn id(&self) -> ScreenId {
        ScreenId::ControllerConfig
    }

    fn section(&self) -> f32 {
        3.0
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        if !c.ready {
            return None;
        }
        if let Some(i) = self.prompt {
            let Some((id, name)) = self.pad.clone() else {
                self.prompt = None;
                return None;
            };
            let Some(p) = c.mi.raw_pads.iter().find(|p| p.id == id) else {
                // Unplugged while configuring: cancel.
                self.prompt = None;
                self.pad = None;
                c.play("ui_invalid", None);
                return None;
            };
            let triggers = (p.axis(Axis::TriggerLeft), p.axis(Axis::TriggerRight));
            // The controller's Back/View button (or the keyboard) cancels.
            let kb_back =
                c.mi.devs
                    .iter()
                    .any(|d| matches!(d.dev, super::minput::Dev::Keyboard(_)) && d.p(b::BACK));
            if p.pressed.contains(Button::Back) || kb_back {
                self.prompt = None;
                c.play("ui_clickBack", None);
                return None;
            }
            let any_held = BINDABLE.iter().any(|&bt| p.held.contains(bt))
                || triggers.0 >= 0.5
                || triggers.1 >= 0.5;
            if !self.released {
                self.released = !any_held;
                self.prev_triggers = triggers;
                return None;
            }
            let got = PadMap::captured(p, self.prev_triggers);
            self.prev_triggers = triggers;
            let input = got.filter(|g| *g != crate::padmap::PadInput::Button(Button::Back))?;
            if self.building.binds[..i].iter().any(|v| v.contains(&input)) {
                self.invalid_at = c.tick;
                c.play("ui_invalid", None);
                return None;
            }
            self.building.binds[i] = vec![input];
            self.released = false;
            c.play("ui_click", None);
            if i + 1 < PadAction::ALL.len() {
                self.prompt = Some(i + 1);
            } else {
                self.prompt = None;
                c.s.options.pads.insert(name, self.building.clone());
                return Some(Go::To(ScreenId::ControlOptions));
            }
            return None;
        }
        // Pick the controller that presses something (the first one at first).
        if let Some(p) = c.mi.raw_pads.iter().find(|p| p.pressed.0 != 0) {
            if self.pad.as_ref().map(|x| x.0) != Some(p.id) {
                c.play("ui_move1", None);
            }
            self.pad = Some((p.id, p.name.clone()));
        } else if self
            .pad
            .as_ref()
            .is_none_or(|(id, _)| !c.mi.raw_pads.iter().any(|p| p.id == *id))
        {
            self.pad = c.mi.raw_pads.first().map(|p| (p.id, p.name.clone()));
        }
        if c.mi.p(b::BACK) {
            c.play("ui_clickBack", None);
            return Some(Go::To(ScreenId::ControlOptions));
        }
        let (_, name) = self.pad.clone()?;
        if c.mi.p(b::ALT) {
            c.s.options.pads.remove(&name);
            c.play("ui_clickBack", None);
        }
        if c.mi.p(b::CONFIRM) {
            self.building = c.s.options.pads.get(&name).cloned().unwrap_or_default();
            self.prompt = Some(0);
            self.released = false;
            c.play("ui_click", None);
        }
        None
    }

    fn guides(&self) -> Vec<(&'static str, u16)> {
        if self.prompt.is_some() || self.pad.is_none() {
            vec![("BACK", b::BACK)]
        } else {
            vec![
                ("CONFIGURE", b::CONFIRM),
                ("BACK", b::BACK),
                ("RESET", b::ALT),
            ]
        }
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let cx = v.at(Vec2::new(v.w + 160.0, 0.0), Vec2::new(v.cx(), 0.0)).x;
        p.text_outlined(
            "CONTROLLER",
            Vec2::new(cx, 20.0),
            (0.5, 0.5),
            2.0,
            Color::WHITE,
        );
        let Some((_, name)) = &self.pad else {
            p.text_outlined(
                "CONNECT A CONTROLLER",
                Vec2::new(cx, 120.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            );
            return;
        };
        let custom = v.s.options.pads.contains_key(name);
        p.text_outlined(
            &format!(
                "{}{}",
                name.to_uppercase(),
                if custom { "" } else { " (DEFAULT)" }
            ),
            Vec2::new(cx, 36.0),
            (0.5, 0.5),
            1.0,
            pal::YELLOW,
        );
        p.m(
            "controls/gamepad",
            Vec2::new(cx, 120.0),
            Org::Center,
            1.0,
            Color::WHITE * 0.35,
        );
        let map = self.current(v);
        for (i, a) in PadAction::ALL.iter().enumerate() {
            let y = 60.0 + 14.0 * i as f32;
            let active = self.prompt == Some(i);
            let col = if active { pal::SELECTED } else { pal::IDLE };
            p.text_outlined(a.label(), Vec2::new(cx + 10.0, y), (1.0, 0.5), 1.0, col);
            let names: Vec<String> = map.binds[i].iter().map(|x| x.label()).collect();
            let text = if names.is_empty() {
                "-".to_owned()
            } else {
                names.join(" / ")
            };
            p.text_outlined(
                &text,
                Vec2::new(cx + 22.0, y),
                (0.0, 0.5),
                1.0,
                Color::WHITE,
            );
        }
        let prompt_y = 200.0;
        match self.prompt {
            Some(i) => {
                let flash = if (v.tick / 4).is_multiple_of(2) {
                    pal::YELLOW
                } else {
                    pal::PAUSE_A
                };
                let bad = v.tick.saturating_sub(self.invalid_at) < 20;
                p.text_outlined(
                    &format!("PRESS BUTTON FOR: {}", PadAction::ALL[i].label()),
                    Vec2::new(cx, prompt_y),
                    (0.5, 0.5),
                    1.0,
                    if bad { pal::RED } else { flash },
                );
                p.text_outlined(
                    "BACK / VIEW BUTTON: CANCEL",
                    Vec2::new(cx, prompt_y + 12.0),
                    (0.5, 0.5),
                    1.0,
                    Color::GRAY,
                );
            }
            None => p.text_outlined(
                "PRESS A BUTTON ON A CONTROLLER TO PICK IT",
                Vec2::new(cx, prompt_y),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            ),
        }
    }
}
