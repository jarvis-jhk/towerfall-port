//! Pause and end-of-match menus (ui.md 10) and the "match ready" screen
//! that shows the menus' result until the game scene exists.

use tfp_render::Color;
use tfp_render::glam::Vec2;

use super::gfx::{Org, Painter, pal, rgba};
use super::minput::b;
use super::settings::{Mode, goal_score};
use super::variants::VARIANTS;
use super::{Ctx, Go, Screen, ScreenId, Session, View};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuType {
    VersusPause,
    VersusMatchEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Resume,
    Quit,
    Rematch,
    MatchSettings,
    ArcherSelect,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Action::Resume => "RESUME",
            Action::Quit => "QUIT",
            Action::Rematch => "REMATCH!",
            Action::MatchSettings => "MATCH SETTINGS",
            Action::ArcherSelect => "ARCHER SELECT",
        }
    }
}

/// The pause menu entity (usable by the level scene).
pub struct PauseMenu {
    pub kind: MenuType,
    pub items: Vec<Action>,
    pub sel: usize,
    pub opened: u32,
    moved_at: u32,
}

impl PauseMenu {
    pub fn new(kind: MenuType) -> Self {
        let items = match kind {
            MenuType::VersusPause => vec![Action::Resume, Action::Quit],
            MenuType::VersusMatchEnd => vec![
                Action::Rematch,
                Action::MatchSettings,
                Action::ArcherSelect,
                Action::Quit,
            ],
        };
        Self {
            kind,
            items,
            sel: 0,
            opened: 0,
            moved_at: 0,
        }
    }

    fn titled(&self) -> bool {
        self.kind == MenuType::VersusPause
    }

    /// Returns the chosen action. Start = resume (pause menus), Back =
    /// resume (both versus menus).
    pub fn update(&mut self, c: &mut Ctx) -> Option<Action> {
        let mut sounds = Vec::new();
        let r = self.step(c.mi, &mut sounds);
        for s in sounds {
            c.play(s, None);
        }
        r
    }

    /// `update` without the front end's context (the in-match pause):
    /// reads any device of `mi`, pushes the UI sounds to play.
    pub fn step(
        &mut self,
        mi: &super::minput::MenuInput,
        sounds: &mut Vec<&'static str>,
    ) -> Option<Action> {
        self.opened += 1;
        if mi.p(b::DOWN) && self.sel + 1 < self.items.len() {
            self.sel += 1;
            self.moved_at = self.opened;
            sounds.push("ui_move1");
        }
        if mi.p(b::UP) && self.sel > 0 {
            self.sel -= 1;
            self.moved_at = self.opened;
            sounds.push("ui_move1");
        }
        if mi.p(b::START) && self.kind == MenuType::VersusPause {
            sounds.push("ui_unpause");
            return Some(Action::Resume);
        }
        if mi.p(b::BACK) {
            sounds.push("ui_unpause");
            return Some(Action::Resume);
        }
        if mi.p(b::CONFIRM) && self.opened > 10 {
            let a = self.items[self.sel];
            sounds.push(if a == Action::Quit {
                "ui_clickBack"
            } else {
                "ui_click"
            });
            return Some(a);
        }
        None
    }

    pub fn draw(&self, p: &mut Painter, tick: u64) {
        let w = p.w;
        let n = self.items.len() as f32;
        let centre = if self.titled() {
            Vec2::new(w / 2.0, 120.0)
        } else {
            Vec2::new(w / 2.0, 205.0)
        };
        let ph = 10.0 * n + 30.0;
        if self.titled() {
            p.rect(0.0, 0.0, w, 240.0, Color::BLACK * 0.5);
        }
        p.panel(centre.x - 60.0, centre.y - ph / 2.0, 120.0, ph);
        if self.titled() {
            let ty = centre.y - ph / 2.0 - 10.0;
            p.m(
                "questResults/arrow",
                Vec2::new(centre.x, ty - 9.0),
                Org::Just(0.5, 1.0),
                1.0,
                Color::WHITE,
            );
            p.text_outlined(
                "PAUSED",
                Vec2::new(centre.x, ty),
                (0.5, 0.5),
                2.0,
                Color::WHITE,
            );
        }
        let y0 = centre.y - ((n - 1.0) * 14.0 + 39.0) / 2.0 + 20.0;
        for (i, a) in self.items.iter().enumerate() {
            let y = y0 + 14.0 * i as f32;
            if i == self.sel {
                let col = if (tick / 4).is_multiple_of(2) {
                    pal::PAUSE_A
                } else {
                    pal::PAUSE_B
                };
                let d = self.opened.saturating_sub(self.moved_at);
                let nudge = if d < 10 {
                    3.0 * (1.0 - d as f32 / 10.0)
                } else {
                    0.0
                };
                p.text(
                    &format!("> {}", a.label()),
                    Vec2::new(centre.x + nudge, y),
                    (0.5, 0.5),
                    1.0,
                    col,
                );
            } else {
                p.text(
                    a.label(),
                    Vec2::new(centre.x, y),
                    (0.5, 0.5),
                    1.0,
                    pal::PAUSE_IDLE,
                );
            }
        }
    }
}

/// A pause menu over the (frozen) match backdrop.
pub struct PauseScreen {
    menu: PauseMenu,
}

impl PauseScreen {
    pub fn new(kind: MenuType) -> Self {
        Self {
            menu: PauseMenu::new(kind),
        }
    }
}

impl Screen for PauseScreen {
    fn id(&self) -> ScreenId {
        match self.menu.kind {
            MenuType::VersusPause => ScreenId::Pause,
            MenuType::VersusMatchEnd => ScreenId::MatchEnd,
        }
    }

    fn menu_background(&self) -> bool {
        false
    }

    fn tween_in(&self) -> u32 {
        1
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        if self.menu.opened == 0 && self.menu.kind == MenuType::VersusPause {
            c.play("ui_pause", None);
        }
        let act = self.menu.update(c)?;
        let clear = |s: &mut Session| s.slots.iter_mut().for_each(|x| x.joined = false);
        Some(match act {
            Action::Resume => Go::To(ScreenId::MatchReady),
            Action::Rematch => Go::Fade(ScreenId::Map),
            Action::MatchSettings => Go::To(ScreenId::VersusOptions),
            Action::ArcherSelect => {
                clear(c.s);
                Go::To(ScreenId::Rollcall)
            }
            Action::Quit if self.menu.kind == MenuType::VersusPause => {
                Go::To(ScreenId::VersusOptions)
            }
            Action::Quit => {
                clear(c.s);
                Go::To(ScreenId::Main)
            }
        })
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        draw_backdrop(p, v);
        self.menu.draw(p, v.tick);
    }
}

/// The level is drawn by the front end when available; otherwise a plain
/// stand-in with the tower's name.
fn draw_backdrop(p: &mut Painter, v: &View) {
    if v.backdrop {
        return;
    }
    let w = v.w;
    p.rect(0.0, 0.0, w, 240.0, Color::rgb(30, 24, 46));
    let name = v.s.setup.as_ref().map_or("", |s| s.tower_name.as_str());
    p.text_outlined(
        name,
        Vec2::new(w / 2.0, 20.0),
        (0.5, 0.5),
        2.0,
        Color::WHITE * 0.5,
    );
}

/// The menus are done: shows the resulting setup. Start opens the pause
/// menu, Confirm the end-of-match menu (until the game scene exists).
pub struct MatchReady {
    printed: bool,
}

impl MatchReady {
    pub fn new(_s: &mut Session) -> Self {
        Self { printed: false }
    }
}

impl Screen for MatchReady {
    fn id(&self) -> ScreenId {
        ScreenId::MatchReady
    }

    fn menu_background(&self) -> bool {
        false
    }

    fn tween_in(&self) -> u32 {
        10
    }

    fn guides(&self) -> Vec<(&'static str, u16)> {
        vec![("MATCH END MENU", b::CONFIRM), ("PAUSE", b::START)]
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        if !self.printed {
            self.printed = true;
            if let Some(s) = &c.s.setup {
                println!("{}", summary(s));
            }
        }
        if !c.ready {
            return None;
        }
        if c.mi.p(b::START) {
            return Some(Go::To(ScreenId::Pause));
        }
        if c.mi.p(b::CONFIRM) {
            return Some(Go::To(ScreenId::MatchEnd));
        }
        None
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        draw_backdrop(p, v);
        let Some(su) = &v.s.setup else {
            p.text_outlined(
                "NO MATCH SET UP",
                Vec2::new(v.cx(), 120.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            );
            return;
        };
        let set = &su.settings;
        let n = su.slots.iter().filter(|s| s.joined).count();
        let cx = v.cx();
        if v.backdrop {
            p.rect(0.0, 34.0, v.w, 156.0, Color::BLACK * 0.6);
        }
        p.text_outlined(
            "MATCH READY",
            Vec2::new(cx, 44.0),
            (0.5, 0.5),
            2.0,
            pal::SELECTED,
        );
        p.text_outlined(
            &format!(
                "{} - {} - PLAY TO {}",
                set.mode.name(),
                set.length.name(),
                goal_score(set.mode, set.length, n)
            ),
            Vec2::new(cx, 60.0),
            (0.5, 0.5),
            1.0,
            Color::WHITE,
        );
        let tower = if set.tower.random {
            format!("{} (RANDOM)", su.tower_name)
        } else {
            su.tower_name.clone()
        };
        let seed = if set.tower.seed > 0 {
            format!("  SEED {:09}", set.tower.seed)
        } else {
            String::new()
        };
        p.text_outlined(
            &format!("TOWER: {tower}{seed}"),
            Vec2::new(cx, 70.0),
            (0.5, 0.5),
            1.0,
            Color::WHITE,
        );
        let joined: Vec<usize> = (0..su.slots.len())
            .filter(|&i| su.slots[i].joined)
            .collect();
        let cols = if joined.len() > 6 { 2 } else { 1 };
        for (k, &i) in joined.iter().enumerate() {
            let sl = &su.slots[i];
            let arch = &p.a.archers[sl.archer.min(p.a.archers.len() - 1)];
            let tn = if set.mode == Mode::TeamDeathmatch {
                set.teams[i].name()
            } else {
                ""
            };
            let team = if tn.is_empty() {
                String::new()
            } else {
                format!(" {tn}")
            };
            let costume = match sl.costume {
                super::settings::Costume::Normal => "",
                super::settings::Costume::Alt => " (ALT)",
                super::settings::Costume::Secret => " (SECRET)",
            };
            let line = if cols == 1 {
                format!(
                    "P{} {} {}{}{} - {}",
                    i + 1,
                    arch.name0,
                    arch.name1,
                    costume,
                    team,
                    sl.dev_name
                )
            } else {
                // Two columns of v.w / 2: the full name when it fits
                // (about 6 px per glyph), else its first line.
                let full = format!(
                    "P{} {} {}{}{}",
                    i + 1,
                    arch.name0,
                    arch.name1,
                    costume,
                    team
                );
                if full.len() as f32 * 6.0 <= v.w / 2.0 - 8.0 {
                    full
                } else {
                    format!("P{} {}{}{}", i + 1, arch.name0, costume, team)
                }
            };
            let col_x = if cols == 1 {
                cx
            } else if k % 2 == 0 {
                cx - v.w / 4.0
            } else {
                cx + v.w / 4.0
            };
            let row = if cols == 1 { k } else { k / 2 };
            p.text_outlined(
                &line,
                Vec2::new(col_x, 86.0 + 10.0 * row as f32),
                (0.5, 0.5),
                1.0,
                rgba(arch.color_b),
            );
        }
        let act: Vec<usize> = set.variants.active().collect();
        let y = 160.0;
        p.text_outlined(
            &format!("VARIANTS: {}", act.len()),
            Vec2::new(cx, y),
            (0.5, 0.5),
            1.0,
            Color::WHITE,
        );
        let shown = act.len().min(16);
        let x0 = cx - (shown as f32 - 1.0) * 9.0;
        for (k, &vi) in act.iter().take(shown).enumerate() {
            p.m_outlined(
                &VARIANTS[vi].icon(),
                Vec2::new(x0 + 18.0 * k as f32, y + 16.0),
                Org::Center,
                0.8,
                Color::WHITE,
            );
        }
    }
}

/// One-line description of the menus' result (printed for logs/tests).
pub fn summary(s: &super::settings::FrontEndSetup) -> String {
    let set = &s.settings;
    let players: Vec<String> = s
        .slots
        .iter()
        .enumerate()
        .filter(|(_, x)| x.joined)
        .map(|(i, x)| {
            format!(
                "P{}:archer{}{:?}{}",
                i + 1,
                x.archer,
                x.costume,
                if set.mode.is_team() {
                    format!("/{:?}", set.teams[i])
                } else {
                    String::new()
                }
            )
        })
        .collect();
    let vars: Vec<String> = set
        .variants
        .active()
        .map(|i| VARIANTS[i].field.to_string())
        .collect();
    format!(
        "match setup: {:?} {:?}, tower {} (#{}{}), seed {}, players [{}], variants [{}]",
        set.mode,
        set.length,
        s.tower_name,
        set.tower.tower,
        if set.tower.random { ", random" } else { "" },
        set.tower.seed,
        players.join(" "),
        vars.join(" ")
    )
}
