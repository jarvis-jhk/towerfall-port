//! Options, control options and keyboard configuration (ui.md 11,
//! input.md 4).

use tfp_render::glam::Vec2;
use tfp_render::{Color, ScaleMode};

use super::gfx::{Org, Painter, pal, sine};
use super::minput::{KeyLayout, KeyboardSetup, b, key_icon};
use super::{Capacity, Ctx, Go, Screen, ScreenId, Session, View};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Row {
    Fullscreen,
    Music,
    Sounds,
    ControlOptions,
    Scale,
    Archers,
    Lessons,
    Wave,
    Flash,
    Scroll,
    Vsync,
    /// Ours: send the report of a crash on the next start (crash.rs).
    CrashReports,
    HoldToPause,
    Discovery,
    Rumble,
    KeyboardLayout,
    ConfigureKeyboard,
    /// Ours: gameplay button mapping per controller model.
    ConfigureController,
}

impl Row {
    fn label(self) -> &'static str {
        match self {
            Row::Fullscreen => "FULLSCREEN",
            Row::Music => "MUSIC",
            Row::Sounds => "SOUNDS",
            Row::ControlOptions => "CONTROL OPTIONS",
            Row::Scale => "VIDEO SCALE",
            Row::Archers => "ARCHERS",
            Row::Lessons => "SHOW VERSUS LESSONS",
            Row::Wave => "SCREEN WAVE EFFECTS",
            Row::Flash => "SCREEN FLASH EFFECTS",
            Row::Scroll => "SCREEN SCROLL EFFECTS",
            Row::Vsync => "VERTICAL SYNC",
            Row::CrashReports => "SEND CRASH REPORTS",
            Row::HoldToPause => "HOLD TO PAUSE",
            Row::Discovery => "GAMEPAD DISCOVERY",
            Row::Rumble => "GAMEPAD RUMBLE",
            Row::KeyboardLayout => "KEYBOARD LAYOUT",
            Row::ConfigureKeyboard => "CONFIGURE KEYBOARD",
            Row::ConfigureController => "CONFIGURE CONTROLLER",
        }
    }

    /// Toggle state, if a toggle.
    fn toggle(self, s: &Session) -> Option<bool> {
        let o = &s.options;
        Some(match self {
            Row::Fullscreen => o.fullscreen,
            Row::Lessons => o.lessons,
            Row::Wave => o.screen_wave,
            Row::Flash => o.screen_flash,
            Row::Scroll => o.screen_scroll,
            Row::Vsync => o.vsync,
            Row::CrashReports => o.crash_reports,
            Row::HoldToPause => o.hold_to_pause,
            Row::Discovery => o.discovery,
            Row::Rumble => o.rumble,
            _ => return None,
        })
    }

    fn value_text(self, s: &Session) -> Option<String> {
        let o = &s.options;
        Some(match self {
            Row::Music => o.music.to_string(),
            Row::Sounds => o.sounds.to_string(),
            Row::Scale => match o.scale {
                ScaleMode::Fit => "FIT".into(),
                ScaleMode::Integer => "INTEGER".into(),
                ScaleMode::Stretch => "STRETCH".into(),
            },
            Row::Archers => match o.capacity {
                Capacity::Auto => format!("AUTO ({})", s.capacity),
                Capacity::Fixed(n) => n.to_string(),
            },
            Row::KeyboardLayout => {
                if o.shared_keyboard {
                    "SHARED".into()
                } else {
                    "SINGLE".into()
                }
            }
            _ => return None,
        })
    }

    /// Left/right stepping: -1, 1 => changed?
    fn step(self, s: &mut Session, dir: i32) -> bool {
        let o = &mut s.options;
        match self {
            Row::Music | Row::Sounds => {
                let v = if self == Row::Music {
                    &mut o.music
                } else {
                    &mut o.sounds
                };
                let n = (*v as i32 + dir).clamp(0, 10) as u8;
                let ch = n != *v;
                *v = n;
                ch
            }
            Row::Scale => {
                let all = [ScaleMode::Fit, ScaleMode::Integer, ScaleMode::Stretch];
                let i = all.iter().position(|&m| m == o.scale).unwrap_or(0) as i32 + dir;
                if (0..3).contains(&i) {
                    o.scale = all[i as usize];
                    true
                } else {
                    false
                }
            }
            Row::Archers => {
                let all = [
                    Capacity::Auto,
                    Capacity::Fixed(4),
                    Capacity::Fixed(8),
                    Capacity::Fixed(12),
                ];
                let i = all.iter().position(|&m| m == o.capacity).unwrap_or(0) as i32 + dir;
                if (0..4).contains(&i) {
                    o.capacity = all[i as usize];
                    true
                } else {
                    false
                }
            }
            Row::KeyboardLayout => {
                let want = dir > 0;
                if o.shared_keyboard == want {
                    return false;
                }
                o.shared_keyboard = want;
                o.keys = if want {
                    KeyboardSetup::shared()
                } else {
                    KeyboardSetup::default()
                };
                true
            }
            _ => false,
        }
    }

    fn has_steps(self) -> bool {
        matches!(
            self,
            Row::Music | Row::Sounds | Row::Scale | Row::Archers | Row::KeyboardLayout
        )
    }
}

pub struct OptionsMenu {
    controls: bool,
    rows: Vec<Row>,
    sel: usize,
    changed_at: (usize, u64),
    selected_at: u64,
    vsync_dialog: bool,
}

impl OptionsMenu {
    pub fn new(controls: bool) -> Self {
        let rows = if controls {
            vec![
                Row::HoldToPause,
                Row::Discovery,
                Row::Rumble,
                Row::KeyboardLayout,
                Row::ConfigureKeyboard,
                Row::ConfigureController,
            ]
        } else {
            vec![
                Row::Fullscreen,
                Row::Music,
                Row::Sounds,
                Row::ControlOptions,
                Row::Scale,
                Row::Archers,
                Row::Lessons,
                Row::Wave,
                Row::Flash,
                Row::Scroll,
                Row::Vsync,
                Row::CrashReports,
            ]
        };
        Self {
            controls,
            rows,
            sel: 0,
            changed_at: (usize::MAX, 0),
            selected_at: 0,
            vsync_dialog: false,
        }
    }

    fn x(w: f32) -> f32 {
        if w > 320.0 { 250.0 } else { 200.0 }
    }
}

impl Screen for OptionsMenu {
    fn id(&self) -> ScreenId {
        if self.controls {
            ScreenId::ControlOptions
        } else {
            ScreenId::Options
        }
    }

    fn section(&self) -> f32 {
        if self.controls { 2.0 } else { 1.0 }
    }

    fn title(&self) -> Option<&'static str> {
        Some("options")
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        if !c.ready {
            return None;
        }
        if self.vsync_dialog {
            if c.mi.p(b::CONFIRM) || c.mi.p(b::START) || c.mi.p(b::BACK) {
                self.vsync_dialog = false;
                c.play("ui_click", None);
            }
            return None;
        }
        if c.mi.p(b::BACK) {
            c.play("ui_clickBack", None);
            return Some(Go::To(if self.controls {
                ScreenId::Options
            } else {
                ScreenId::Main
            }));
        }
        if c.mi.p(b::DOWN) && self.sel + 1 < self.rows.len() {
            self.sel += 1;
            self.selected_at = c.tick;
            c.play("ui_move1", None);
        }
        if c.mi.p(b::UP) && self.sel > 0 {
            self.sel -= 1;
            self.selected_at = c.tick;
            c.play("ui_move1", None);
        }
        let row = self.rows[self.sel];
        for (bit, dir) in [(b::LEFT, -1), (b::RIGHT, 1)] {
            if c.mi.p(bit) && row.step(c.s, dir) {
                self.changed_at = (self.sel, c.tick);
                c.play("ui_move1", None);
            }
        }
        if c.mi.p(b::CONFIRM) {
            match row {
                Row::ControlOptions => {
                    c.play("ui_click", None);
                    return Some(Go::To(ScreenId::ControlOptions));
                }
                Row::ConfigureKeyboard => {
                    c.play("ui_click", None);
                    return Some(Go::To(ScreenId::KeyboardConfig));
                }
                Row::ConfigureController => {
                    c.play("ui_click", None);
                    return Some(Go::To(ScreenId::ControllerConfig));
                }
                _ => {}
            }
            if let Some(on) = row.toggle(c.s) {
                let o = &mut c.s.options;
                let v = !on;
                match row {
                    Row::Fullscreen => {
                        o.fullscreen = v;
                        c.fx.host.fullscreen = Some(v);
                    }
                    Row::Lessons => o.lessons = v,
                    Row::Wave => o.screen_wave = v,
                    Row::Flash => o.screen_flash = v,
                    Row::Scroll => o.screen_scroll = v,
                    Row::Vsync => {
                        o.vsync = v;
                        self.vsync_dialog = true;
                    }
                    Row::CrashReports => o.crash_reports = v,
                    Row::HoldToPause => o.hold_to_pause = v,
                    Row::Discovery => o.discovery = v,
                    Row::Rumble => o.rumble = v,
                    _ => {}
                }
                self.changed_at = (self.sel, c.tick);
                c.play(if v { "ui_subclickOn" } else { "ui_subclickOff" }, None);
            }
        }
        None
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let x = Self::x(v.w);
        for (i, row) in self.rows.iter().enumerate() {
            let y = 45.0 + 12.0 * i as f32;
            let from_x = if i % 2 == 0 { -160.0 } else { v.w + 160.0 };
            let pos = v.at(Vec2::new(from_x, y), Vec2::new(x, y));
            let sel = i == self.sel;
            let col = if sel { pal::SELECTED } else { pal::IDLE };
            let nudge = if sel && v.tick.saturating_sub(self.selected_at) < 10 {
                5.0 * (1.0 - v.tick.saturating_sub(self.selected_at) as f32 / 10.0)
            } else {
                0.0
            };
            p.text_outlined(
                row.label(),
                pos + Vec2::new(-5.0 + nudge, 0.0),
                (1.0, 0.5),
                1.0,
                col,
            );
            let wig = if self.changed_at.0 == i && v.tick.saturating_sub(self.changed_at.1) < 30 {
                2.0 * ((v.tick - self.changed_at.1) as f32 * 0.8).sin()
                    * (1.0 - (v.tick - self.changed_at.1) as f32 / 30.0)
            } else {
                0.0
            };
            let vpos = pos + Vec2::new(30.0 + wig, 0.0);
            if let Some(on) = row.toggle(v.s) {
                p.m_outlined(
                    if on { "optionOn" } else { "optionOff" },
                    vpos,
                    Org::Center,
                    1.0,
                    Color::WHITE,
                );
            } else if let Some(t) = row.value_text(v.s) {
                p.text_outlined(&t, vpos, (0.5, 0.5), 1.0, col);
            }
            if sel && row.has_steps() {
                let bob = 20.0 + 2.0 * sine(v.tick, 60.0, 0.0);
                let half = p.measure(&row.value_text(v.s).unwrap_or_default()).x / 2.0;
                if let Some(ar) = p.a.m("portraits/arrow") {
                    p.tex_ex(
                        ar,
                        vpos + Vec2::new(-bob.max(half + 8.0), 0.0),
                        Org::Center,
                        Vec2::ONE,
                        0.0,
                        Color::WHITE,
                        true,
                    );
                    p.tex_ex(
                        ar,
                        vpos + Vec2::new(bob.max(half + 8.0), 0.0),
                        Org::Center,
                        Vec2::ONE,
                        0.0,
                        Color::WHITE,
                        false,
                    );
                }
            }
        }
        p.text_outlined(
            concat!("TOWERFALL-PORT V", env!("CARGO_PKG_VERSION")),
            Vec2::new(v.w - 2.0, 238.0),
            (1.0, 1.0),
            1.0,
            Color::WHITE * 0.6,
        );
        if self.vsync_dialog {
            p.panel(v.cx() - 100.0, 80.0, 200.0, 80.0);
            p.text_outlined(
                "VERTICAL SYNC CHANGES WILL NOT BE",
                Vec2::new(v.cx(), 112.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            );
            p.text_outlined(
                "APPLIED UNTIL THE GAME IS RESTARTED",
                Vec2::new(v.cx(), 124.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            );
        }
    }
}

// ---- keyboard configuration ----------------------------------------------

pub struct KeyboardConfig {
    layout: usize,
    /// Prompt index while configuring.
    prompt: Option<usize>,
    building: KeyLayout,
    invalid_at: u64,
    /// An existing config was confirmed: CONFIGURE (true, right) or
    /// CLEAR / RESET TO DEFAULT (false, left), confirmed again
    /// (input.md 4).
    choice: Option<bool>,
}

impl KeyboardConfig {
    pub fn new() -> Self {
        Self {
            layout: 0,
            prompt: None,
            building: KeyLayout::empty(),
            invalid_at: 0,
            choice: None,
        }
    }
}

impl Screen for KeyboardConfig {
    fn id(&self) -> ScreenId {
        ScreenId::KeyboardConfig
    }

    fn section(&self) -> f32 {
        3.0
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        if !c.ready {
            return None;
        }
        if let Some(i) = self.prompt {
            // Capture the first newly pressed key.
            let &key = c.mi.keys_pressed.first()?;
            let essential = i < 9;
            if key_icon(key).is_none()
                || (essential && self.building.essential_keys().contains(&key))
            {
                self.invalid_at = c.tick;
                c.play("ui_invalid", None);
                return None;
            }
            self.building.bindings_mut()[i].1.push(key);
            c.play("ui_click", None);
            if i + 1 < 11 {
                self.prompt = Some(i + 1);
            } else {
                let layouts = &mut c.s.options.keys.layouts;
                if self.layout < layouts.len() {
                    layouts[self.layout] = self.building.clone();
                } else {
                    layouts.push(self.building.clone());
                }
                self.prompt = None;
                // Ignore the keys of the last binding as menu input.
                return Some(Go::To(ScreenId::ControlOptions));
            }
            return None;
        }
        let n = c.s.options.keys.layouts.len();
        if let Some(configure) = self.choice {
            if c.mi.p(b::BACK) {
                self.choice = None;
                c.play("ui_clickBack", None);
            } else if c.mi.p(b::LEFT) && configure {
                self.choice = Some(false);
                c.play("ui_move1", None);
            } else if c.mi.p(b::RIGHT) && !configure {
                self.choice = Some(true);
                c.play("ui_move1", None);
            } else if c.mi.p(b::CONFIRM) {
                self.choice = None;
                if configure {
                    self.building = KeyLayout::empty();
                    self.prompt = Some(0);
                    c.play("ui_click", None);
                } else {
                    // Config 0 resets to the default; the others go.
                    if self.layout == 0 {
                        c.s.options.keys.layouts[0] = KeyLayout::default_config();
                    } else if self.layout < c.s.options.keys.layouts.len() {
                        c.s.options.keys.layouts.remove(self.layout);
                    }
                    c.play("ui_clickBack", None);
                }
            }
            return None;
        }
        if c.mi.p(b::BACK) {
            c.play("ui_clickBack", None);
            return Some(Go::To(ScreenId::ControlOptions));
        }
        if c.mi.p(b::LEFT) && self.layout > 0 {
            self.layout -= 1;
            c.play("ui_move1", None);
        }
        if c.mi.p(b::RIGHT) && self.layout < n.min(3) {
            self.layout += 1;
            c.play("ui_move1", None);
        }
        if c.mi.p(b::CONFIRM) {
            if self.layout < n {
                // An existing config: CONFIGURE or CLEAR first.
                self.choice = Some(true);
            } else {
                self.building = KeyLayout::empty();
                self.prompt = Some(0);
            }
            c.play("ui_click", None);
        }
        None
    }

    fn guides(&self) -> Vec<(&'static str, u16)> {
        if self.prompt.is_some() {
            vec![]
        } else {
            vec![("SELECT", b::CONFIRM), ("BACK", b::BACK)]
        }
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let cx = v.at(Vec2::new(v.w + 160.0, 0.0), Vec2::new(v.cx(), 0.0)).x;
        let n = v.s.options.keys.layouts.len();
        let title = format!("KEYBOARD PLAYER {}", self.layout + 1);
        p.text_outlined(&title, Vec2::new(cx, 20.0), (0.5, 0.5), 2.0, Color::WHITE);
        if self.layout > 0 {
            p.text_outlined(
                "<",
                Vec2::new(cx - 90.0, 20.0),
                (0.5, 0.5),
                2.0,
                Color::WHITE,
            );
        }
        if self.layout < n.min(3) {
            p.text_outlined(
                ">",
                Vec2::new(cx + 90.0, 20.0),
                (0.5, 0.5),
                2.0,
                Color::WHITE,
            );
        }
        if self.layout > 0 {
            p.text_outlined(
                "MOST KEYBOARDS WILL NOT WORK CORRECTLY WITH MORE THAN ONE PLAYER!",
                Vec2::new(cx, 34.0),
                (0.5, 0.5),
                1.0,
                pal::RED,
            );
        }
        p.m(
            "controls/gamepad",
            Vec2::new(cx, 120.0),
            Org::Center,
            1.0,
            Color::WHITE * 0.35,
        );
        let mut shown = match self.prompt {
            Some(_) => self.building.clone(),
            None => {
                v.s.options
                    .keys
                    .layouts
                    .get(self.layout)
                    .cloned()
                    .unwrap_or_else(KeyLayout::empty)
            }
        };
        for (i, (label, keys)) in shown.bindings_mut().into_iter().enumerate() {
            let y = 52.0 + 12.0 * i as f32;
            let active = self.prompt == Some(i);
            let col = if active { pal::SELECTED } else { pal::IDLE };
            p.text_outlined(label, Vec2::new(cx + 10.0, y), (1.0, 0.5), 1.0, col);
            let name = keys.first().and_then(|&k| key_icon(k));
            match name {
                Some(n) => p.icon(
                    &format!("controls/keyboard/{n}"),
                    Vec2::new(cx + 22.0, y),
                    Org::Just(0.0, 0.5),
                ),
                None => p.text_outlined("-", Vec2::new(cx + 22.0, y), (0.0, 0.5), 1.0, Color::GRAY),
            }
        }
        let prompt_y = 200.0;
        match self.prompt {
            Some(i) => {
                let label = KeyLayout::empty().bindings_mut()[i].0;
                let flash = if (v.tick / 4).is_multiple_of(2) {
                    pal::YELLOW
                } else {
                    pal::PAUSE_A
                };
                let bad = v.tick.saturating_sub(self.invalid_at) < 20;
                p.text_outlined(
                    &format!("PRESS KEY FOR: {label}"),
                    Vec2::new(cx, prompt_y),
                    (0.5, 0.5),
                    1.0,
                    if bad { pal::RED } else { flash },
                );
            }
            None if self.choice.is_some() => {
                let configure = self.choice == Some(true);
                let clear = if self.layout == 0 {
                    "RESET TO DEFAULT"
                } else {
                    "CLEAR"
                };
                let col = |on: bool| if on { pal::SELECTED } else { pal::IDLE };
                p.text_outlined(
                    clear,
                    Vec2::new(cx - 60.0, prompt_y),
                    (0.5, 0.5),
                    1.0,
                    col(!configure),
                );
                p.text_outlined(
                    "CONFIGURE",
                    Vec2::new(cx + 60.0, prompt_y),
                    (0.5, 0.5),
                    1.0,
                    col(configure),
                );
                let x = if configure { cx + 60.0 } else { cx - 60.0 };
                let w = p.measure(if configure { "CONFIGURE" } else { clear }).x;
                p.text_outlined(
                    ">",
                    Vec2::new(x - w / 2.0 - 8.0, prompt_y),
                    (0.5, 0.5),
                    1.0,
                    pal::SELECTED,
                );
            }
            None => {
                p.text_outlined(
                    if self.layout < n {
                        "CONFIRM TO EDIT"
                    } else {
                        "CONFIRM TO ADD THIS KEYBOARD PLAYER"
                    },
                    Vec2::new(cx, prompt_y),
                    (0.5, 0.5),
                    1.0,
                    Color::WHITE,
                );
            }
        }
    }
}
