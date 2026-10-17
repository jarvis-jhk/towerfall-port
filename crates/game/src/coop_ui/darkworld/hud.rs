//! Dark World HUD and flow screens (darkworld.md 5.1, 5.5, 6, 9.1, 9.2):
//! lives gems (solo; spending a life flies a gem to the corpse, game over
//! drops them), the boss bar with its trailing damage bar, the level-1
//! title card (waits for Confirm), the LEVEL n / FIGHT!! card with the
//! variant icons, the boss shield card and `DarkWorldBossTitle`, the GO!
//! marker over the exit portal, the fades between levels, the
//! trigger-arrow slide (Darkfang), the game over and the complete screen
//! (continues, deaths, eye awards, time, per-player cards with crowns).
//!
//! Timelines follow the sim's `phase_t`; the HUD keeps local clocks for
//! its own tweens (bars, gems, the slide).

use std::f32::consts::{PI, TAU};

use tfp_render::Color;
use tfp_render::glam::Vec2;
use tfp_sim::darkworld::{DarkWorld, DwPhase, DwResult, Op};
use tfp_sim::entity::Kind;
use tfp_sim::{Content, World};

use super::{boss_title, difficulty_color, difficulty_name, shield_image};
use crate::front::gfx::{Org, Painter, back_out, cube_in, cube_in_out, cube_out, lerp};

const LEVEL_COLOR: Color = Color::hex(0xF878F8);
const FIGHT_COLOR: Color = Color::hex(0xF83800);
const BOSS_NAME: Color = Color::hex(0xEF4A52);
const BOSS_SUB: Color = Color::hex(0x87E2ED);
const GO_A: Color = Color::hex(0xEAC831);
const GO_B: Color = Color::hex(0xFF2B35);
const BAD: Color = Color::hex(0xFF321C);
const TIME_COLOR: Color = Color::hex(0xE57F57);
const DARK_TINT: Color = Color::hex(0x8261B8);

fn sine_in(t: f32) -> f32 {
    1.0 - (t.clamp(0.0, 1.0) * PI / 2.0).cos()
}

fn sine_out(t: f32) -> f32 {
    (t.clamp(0.0, 1.0) * PI / 2.0).sin()
}

fn back_in(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (2.70158 * t - 1.70158)
}

fn up_down(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.5 { t * 2.0 } else { 2.0 - t * 2.0 }
}

fn bezier(a: Vec2, ctrl: Vec2, b: Vec2, t: f32) -> Vec2 {
    lerp(lerp(a, ctrl, t), lerp(ctrl, b, t), t)
}

/// A gem flying from the HUD to a reviving corpse (`SpendLife`).
#[derive(Clone, Debug)]
struct GemFly {
    from: Vec2,
    to: Vec2,
    t: u32,
}

/// The boss bar (`DarkWorldHUD` boss mode).
#[derive(Clone, Debug, Default)]
struct BossBar {
    on: bool,
    ease: f32,
    /// Slide-out clock (bar leaving), `None` while shown.
    out: Option<u32>,
    t_in: u32,
    fill: f32,
    draw: f32,
    draw_bg: f32,
    bg_counter: f32,
    max: f32,
}

/// The trigger-arrow slide before Darkfang's first level (once a run).
#[derive(Clone, Debug)]
pub struct Tutorial {
    t: u32,
    /// Ticks since it was dismissed.
    out: Option<u32>,
    pub icon: Option<String>,
}

impl Tutorial {
    pub fn new(icon: Option<String>) -> Self {
        Self {
            t: 0,
            out: None,
            icon,
        }
    }

    /// Follow the sim's tips phase (`DwPhase::Tips`: in for 10 + 20 ticks,
    /// 60 deaf, closed by a press, 20 out). Returns a sound cue
    /// (`sfx_multiStartLevelControlFlyin` / `...Flyout`) when one starts.
    pub fn sync(&mut self, d: &DarkWorld) -> Option<&'static str> {
        if d.phase != DwPhase::Tips {
            // Over, or skipped (a session started past level 1).
            self.out = Some(20);
            return None;
        }
        let before = (self.t, self.out);
        self.t = d.phase_t as u32;
        self.out = d.tips_closed_at.map(|c| (d.phase_t - c).max(0.0) as u32);
        if before.0 < 10 && self.t >= 10 {
            return Some("sfx_multiStartLevelControlFlyin");
        }
        if before.1.is_none() && self.out.is_some() {
            return Some("sfx_multiStartLevelControlFlyout");
        }
        None
    }

    pub fn finished(&self) -> bool {
        self.out.is_some_and(|o| o >= 20)
    }

    fn x(&self) -> f32 {
        match self.out {
            Some(o) => 160.0 + (-160.0 - 160.0) * cube_in(o as f32 / 20.0),
            None => {
                let k = cube_out((self.t as f32 - 10.0).max(0.0) / 20.0);
                480.0 + (160.0 - 480.0) * k
            }
        }
    }

    pub fn draw(&self, p: &mut Painter) {
        let c = Vec2::new(self.x() + (p.w - 320.0) / 2.0, 120.0);
        p.panel(c.x - 95.0, c.y - 75.0, 190.0, 150.0);
        p.m(
            "tips/triggers",
            c + Vec2::new(0.0, -18.0),
            Org::Center,
            1.0,
            Color::WHITE,
        );
        let y = c.y + 38.0;
        let iw = self
            .icon
            .as_deref()
            .and_then(|i| p.a.m(i))
            .map_or(0.0, |s| s.width() as f32);
        match self.icon.as_deref() {
            Some(i) if iw > 0.0 => {
                let pw = p.measure("PRESS").x;
                let total = pw + 5.0 + iw;
                p.text(
                    "PRESS",
                    Vec2::new(c.x - total / 2.0 + pw / 2.0, y),
                    (0.5, 0.5),
                    1.0,
                    Color::WHITE,
                );
                p.m(
                    i,
                    Vec2::new(c.x + total / 2.0 - iw / 2.0, y),
                    Org::Center,
                    1.0,
                    Color::WHITE,
                );
            }
            // No device icon (bots): the button by name.
            _ => p.text(
                "PRESS ALT SHOOT",
                Vec2::new(c.x, y),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            ),
        }
        p.text(
            "TO DETONATE TRIGGER ARROWS",
            Vec2::new(c.x, y + 14.0),
            (0.5, 0.5),
            1.0,
            Color::WHITE,
        );
    }
}

/// One player's result card (`DarkWorldPlayerResults`).
#[derive(Clone, Debug)]
pub struct Card {
    pub slot: usize,
    /// Index into `UiAssets::archers` (alt looks are the alt of the base).
    pub archer: usize,
    pub alt: bool,
}

pub struct DwHud {
    /// Theme name, map block and icon of the tower (title card).
    pub title: String,
    pub block: String,
    pub icon: String,
    pub dark_tint: bool,
    /// Archer of the solo player (gems) and the result cards.
    pub cards: Vec<Card>,
    /// Active curse / power-up icons (level cards).
    pub variant_icons: Vec<String>,
    t: u32,
    /// Title card: ticks since it was confirmed.
    confirmed: Option<u32>,
    seen_wait: bool,
    /// Gems: HUD ease clock since the level started, out clock.
    hud_t: u32,
    hud_out: Option<u32>,
    lives_prev: i32,
    flying: Vec<GemFly>,
    /// Game over: gems falling away (start positions, ticks).
    losing: Option<(Vec<Vec2>, u32)>,
    bar: BossBar,
    /// GO! scale (0..1).
    go: f32,
    level_key: Option<usize>,
    last_phase: Option<DwPhase>,
}

impl DwHud {
    pub fn new(
        title: String,
        block: String,
        icon: String,
        dark_tint: bool,
        cards: Vec<Card>,
        variant_icons: Vec<String>,
    ) -> Self {
        Self {
            title,
            block,
            icon,
            dark_tint,
            cards,
            variant_icons,
            t: 0,
            confirmed: None,
            seen_wait: false,
            hud_t: 0,
            hud_out: None,
            lives_prev: -1,
            flying: Vec::new(),
            losing: None,
            bar: BossBar::default(),
            go: 0.0,
            level_key: None,
            last_phase: None,
        }
    }

    fn waiting_confirm(d: &DarkWorld) -> bool {
        d.prog.get(d.pc) == Some(&Op::WaitConfirm)
    }

    /// The complete screen's sequence is over (the menu may open).
    pub fn complete_done(d: &DarkWorld) -> bool {
        match (d.phase, d.result.as_ref()) {
            (DwPhase::Complete, Some(r)) => d.phase_t >= complete_times(r, d.n_players()).4,
            _ => false,
        }
    }

    fn gem_pos(i: usize, y: f32) -> Vec2 {
        Vec2::new(8.0 + 10.0 * i as f32, y)
    }

    fn gem_y(&self) -> f32 {
        let ease = cube_out(((self.hud_t as f32) - 60.0).max(0.0) / 30.0);
        let ease = match self.hud_out {
            Some(o) => ease * (1.0 - cube_in(o as f32 / 30.0)),
            None => ease,
        };
        (-29.0 + (13.0 + 29.0) * ease).ceil()
    }

    /// After each sim step.
    pub fn after_step(&mut self, w: &World) {
        let Some(d) = w.dark_world.as_deref() else {
            return;
        };
        self.t += 1;
        // A new level: the HUD comes back.
        if self.level_key != Some(d.round_index) || (d.phase == DwPhase::Intro && d.phase_t <= 1.0)
        {
            if self.level_key != Some(d.round_index) {
                self.bar = BossBar::default();
            }
            self.level_key = Some(d.round_index);
            self.hud_t = 0;
            self.hud_out = None;
            self.flying.clear();
            self.losing = None;
            self.lives_prev = d.extra_lives;
        }
        if Self::waiting_confirm(d) {
            self.seen_wait = true;
        } else if self.seen_wait || !matches!(d.phase, DwPhase::Title | DwPhase::Tips) {
            self.confirmed = Some(self.confirmed.map_or(0, |c| c + 1));
        }
        if d.started {
            self.hud_t += 1;
        }
        // The HUD leaves on game over (60 ticks in), the transition and
        // the completion.
        let leave = match d.phase {
            DwPhase::GameOver => d.phase_t >= 60.0,
            DwPhase::Transition => d.phase_t >= 31.0,
            DwPhase::Complete => true,
            _ => false,
        };
        if leave && self.hud_out.is_none() {
            self.hud_out = Some(0);
        }
        if let Some(o) = &mut self.hud_out {
            *o += 1;
        }
        // Lives: a spent life flies to the corpse; game over drops them.
        if d.n_players() == 1 {
            if d.extra_lives < self.lives_prev && self.lives_prev >= 0 {
                let corpse = w
                    .entities()
                    .find(|&e| matches!(w.kind(e), Some(Kind::Reviver(_))))
                    .map(|e| w.core(e).pos)
                    .or_else(|| {
                        w.entities()
                            .find(|&e| matches!(w.kind(e), Some(Kind::Corpse(_))))
                            .map(|e| w.core(e).pos)
                    });
                if let Some(c) = corpse {
                    let from = Self::gem_pos(d.extra_lives.max(0) as usize, self.gem_y());
                    self.flying.push(GemFly {
                        from,
                        to: Vec2::new(c.x, c.y),
                        t: 0,
                    });
                }
            }
            if d.phase == DwPhase::GameOver && self.losing.is_none() && d.extra_lives > 0 {
                let y = self.gem_y();
                let v = (0..d.extra_lives as usize)
                    .map(|i| Self::gem_pos(i, y))
                    .collect();
                self.losing = Some((v, 0));
            }
        }
        self.lives_prev = d.extra_lives;
        for f in &mut self.flying {
            f.t += 1;
        }
        self.flying.retain(|f| f.t < 61);
        if let Some((_, t)) = &mut self.losing {
            *t += 1;
        }
        // Boss bar.
        match d
            .boss_mode
            .then_some((d.boss_health.max(0), d.max_boss_health))
        {
            Some((h, max)) => {
                let b = &mut self.bar;
                if !b.on {
                    *b = BossBar {
                        on: true,
                        draw: h as f32,
                        draw_bg: h as f32,
                        max: max.max(1) as f32,
                        ..Default::default()
                    };
                }
                b.out = None;
                b.t_in += 1;
                b.ease = cube_out(b.t_in as f32 / 30.0);
                if b.fill < 1.0 {
                    if b.ease >= 1.0 {
                        b.fill = (b.fill + 0.006).min(1.0);
                    }
                } else if b.draw > h as f32 {
                    b.draw = (b.draw - 0.5).max(h as f32);
                    b.bg_counter = 30.0;
                } else if b.bg_counter > 0.0 {
                    b.bg_counter -= 1.0;
                } else if b.draw_bg > h as f32 {
                    b.draw_bg = (b.draw_bg - 0.05).max(h as f32);
                }
                if h as f32 > b.draw {
                    // Healed (a continue): snap.
                    b.draw = h as f32;
                    b.draw_bg = h as f32;
                }
            }
            None => {
                let b = &mut self.bar;
                if b.on {
                    let o = b.out.get_or_insert(0);
                    *o += 1;
                    b.ease = 1.0 - cube_in(*o as f32 / 30.0);
                    if *o >= 30 {
                        b.on = false;
                    }
                }
            }
        }
        if leave && self.bar.on && self.bar.out.is_none() {
            self.bar.out = Some(0);
        }
        // GO! grows once the portal is open, while somebody lives.
        let alive = w.players().any(|id| w.player(id).is_some_and(|p| !p.dead));
        let open = d
            .exit_portal
            .filter(|&e| w.exists(e))
            .and_then(|e| match w.kind(e) {
                Some(Kind::Dw(tfp_sim::darkworld::Part::ExitPortal { t })) => Some(*t >= 60.0),
                _ => None,
            })
            .unwrap_or(false);
        let want = if open && alive && d.phase == DwPhase::Exit {
            1.0
        } else {
            0.0
        };
        self.go = if want > self.go {
            (self.go + 0.1).min(1.0)
        } else {
            (self.go - 0.1).max(0.0)
        };
        self.last_phase = Some(d.phase);
    }

    fn gem(p: &mut Painter, archer: usize, pos: Vec2, scale: f32, rot: f32, tick: u64) {
        let name = format!("Gem{}", archer.min(8));
        let f = p.a.anim_frame(&name, "0", tick);
        match p.a.sprite_frame(&name, f) {
            Some((s, (ox, oy))) => p.tex_ex(
                s,
                pos,
                Org::At(ox, oy),
                Vec2::splat(scale),
                rot,
                Color::WHITE,
                false,
            ),
            None => p.rect(pos.x - 3.0, pos.y - 3.0, 6.0, 6.0, Color::WHITE),
        }
    }

    /// The HUD and flow screens over the composited level.
    pub fn draw(&self, p: &mut Painter, w: &World, c: &Content, flash: f32, tick: u64) {
        let Some(d) = w.dark_world.as_deref() else {
            return;
        };
        let width = p.w;
        let dx = (width - 320.0) / 2.0;
        // Screen flash (boss deaths).
        if flash > 0.0 {
            p.rect(0.0, 0.0, width, 240.0, Color::WHITE * flash.min(1.0));
        }
        // Gems (solo).
        let solo_archer = self.cards.first().map_or(0, |c| c.archer);
        if d.n_players() == 1 && self.losing.is_none() {
            let y = self.gem_y();
            let spent = self.flying.len() as i32;
            let n = d.extra_lives.max(0);
            let _ = spent;
            for i in 0..n as usize {
                Self::gem(
                    p,
                    solo_archer,
                    Self::gem_pos(i, y),
                    1.0,
                    0.0,
                    tick + i as u64 * 7,
                );
            }
        }
        for f in &self.flying {
            let top = Vec2::new(f.to.x, (f.to.y - 40.0).max(30.0));
            let (pos, scale, rot) = if f.t <= 30 {
                let k = cube_in_out(f.t as f32 / 30.0);
                (lerp(f.from, top, k), 1.0 + k, -TAU * k)
            } else if f.t <= 40 {
                (top, 2.0, 0.0)
            } else {
                let k = ((f.t - 40) as f32 / 20.0).min(1.0);
                let fin = Vec2::new(f.to.x, f.to.y - 10.0);
                (
                    lerp(top, fin, k),
                    2.0 * (1.0 - back_in(k).clamp(0.0, 1.0)),
                    0.0,
                )
            };
            Self::gem(p, solo_archer, pos, scale, rot, tick);
        }
        if let Some((gems, t)) = &self.losing {
            for (i, g) in gems.iter().rev().enumerate() {
                let len = 30.0 + 4.0 * i as f32;
                let k = (*t as f32 / len).min(1.0);
                if k >= 1.0 {
                    continue;
                }
                let pos = bezier(
                    *g,
                    *g + Vec2::new(40.0, 10.0),
                    *g + Vec2::new(-60.0, 40.0),
                    sine_in(k),
                );
                Self::gem(p, solo_archer, pos, 1.0, -4.0 * PI * sine_out(k), tick);
            }
        }
        // Boss bar.
        if self.bar.on {
            self.boss_bar(p, tick, dx);
        }
        // GO! over the exit portal.
        if self.go > 0.0
            && let Some(e) = d.exit_portal.filter(|&e| w.exists(e))
        {
            let at = w.core(e).pos;
            let bob = 4.0 * (tick as f32 / 60.0 * TAU).sin();
            let col = if (tick / 5).is_multiple_of(2) {
                GO_A
            } else {
                GO_B
            };
            let pos = Vec2::new(at.x.floor() + 1.0, at.y.floor() - 25.0 + bob);
            if self.go >= 1.0 {
                p.text_outlined("GO!", pos, (0.5, 1.0), 2.0, col);
            } else {
                p.text_outlined(
                    "GO!",
                    pos,
                    (0.5, 1.0),
                    (2.0 * self.go).max(1.0).round(),
                    col,
                );
            }
            if let Some(ind) = p.a.g("versus/playerIndicator") {
                let ip = Vec2::new(at.x.floor(), at.y.floor() - 20.0 + bob);
                p.tex_ex(
                    ind,
                    ip,
                    Org::Just(0.5, 1.0),
                    Vec2::new(self.go, 1.0),
                    0.0,
                    col,
                    false,
                );
            }
        }
        // Level fades.
        match d.phase {
            DwPhase::Intro => {
                let a = 1.0 - cube_in(d.phase_t / 30.0);
                if a > 0.0 {
                    p.rect(0.0, 0.0, width, 240.0, Color::BLACK * a);
                }
            }
            DwPhase::Transition => {
                let a = ((d.phase_t - 132.0) / 30.0).clamp(0.0, 1.0);
                if a > 0.0 {
                    p.rect(0.0, 0.0, width, 240.0, Color::BLACK * a);
                }
            }
            _ => {}
        }
        match d.phase {
            DwPhase::Title => self.title_card(p, d, dx),
            DwPhase::Playing | DwPhase::Exit => {
                if self.confirmed.is_some_and(|c| c < 31) && d.round_index == 0 {
                    self.title_card(p, d, dx);
                }
                if let Some(b) = d.level(c).boss {
                    self.boss_card(p, d, b, dx);
                } else {
                    self.level_card(p, d, dx);
                }
            }
            DwPhase::GameOver => self.game_over(p, d, dx),
            DwPhase::Complete => self.complete(p, d, dx, tick),
            _ => {}
        }
    }

    fn boss_bar(&self, p: &mut Painter, tick: u64, dx: f32) {
        let b = &self.bar;
        let y = (-8.0 + 16.0 * b.ease).round();
        let front = if b.fill < 1.0 {
            b.fill * 196.0
        } else {
            b.draw / b.max * 196.0
        };
        let back = if b.fill < 1.0 {
            0.0
        } else {
            b.draw_bg / b.max * 196.0
        };
        p.m(
            "bossHud/hudBottom",
            Vec2::new(60.0 + dx, y),
            Org::TopLeft,
            1.0,
            Color::WHITE,
        );
        let frame = ((tick / 6) % 5) as i32;
        for (name, len) in [("bossHud/hudAnimHit", back), ("bossHud/hudFullAnim", front)] {
            let Some(strip) = p.a.m(name) else { continue };
            let mut x = 0.0;
            while x < len {
                let seg = (50.0f32).min(len - x) as i32;
                if seg <= 0 {
                    break;
                }
                let cell = strip.sub(0, frame * 8, seg, 8);
                p.tex(
                    cell,
                    Vec2::new(62.0 + dx + x, y),
                    Org::TopLeft,
                    1.0,
                    Color::WHITE,
                );
                x += 50.0;
            }
        }
        p.m(
            "bossHud/hudTop",
            Vec2::new(59.0 + dx, y - 1.0),
            Org::TopLeft,
            1.0,
            Color::WHITE,
        );
    }

    /// Level 1 title card (5.1): emblem slam, tower name, difficulty line;
    /// waits for Confirm, then slides down while the overlay fades.
    fn title_card(&self, p: &mut Painter, d: &DarkWorld, dx: f32) {
        let width = p.w;
        let cx = 160.0 + dx;
        let out = self
            .confirmed
            .map_or(0.0, |c| cube_in((c as f32 / 30.0).min(1.0)));
        if out >= 1.0 {
            return;
        }
        p.rect(0.0, 0.0, width, 240.0, Color::BLACK * (0.5 * (1.0 - out)));
        let t = if d.phase == DwPhase::Title {
            d.phase_t
        } else {
            1000.0
        };
        if t < 62.0 {
            return;
        }
        let drop = 230.0 * out;
        let slam = cube_in((t - 62.0) / 10.0);
        let scale = 6.0 + (2.0 - 6.0) * slam;
        let rot = (PI / 18.0) * (1.0 - slam);
        let img = Vec2::new(cx, 70.0) + Vec2::new(5.0, -20.0) * (1.0 - slam) + Vec2::new(0.0, drop);
        let tint = if self.dark_tint {
            DARK_TINT
        } else {
            Color::WHITE
        };
        for (name, col) in [
            (self.block.as_str(), Color::WHITE),
            (self.icon.as_str(), tint),
        ] {
            if let Some(s) = p.a.m(name) {
                p.tex_ex(s, img, Org::Center, Vec2::splat(scale), rot, col, false);
            }
        }
        let k = cube_out((t - 62.0) / 60.0);
        let ty = 120.0 + 30.0 * (1.0 - k) + drop;
        p.text_outlined(
            &self.title,
            Vec2::new(cx, ty),
            (0.5, 0.5),
            2.0,
            Color::WHITE * k,
        );
        let line = format!("{} MODE", difficulty_name(d.settings.difficulty));
        p.text_outlined(
            &line,
            Vec2::new(cx, ty + 12.0),
            (0.5, 0.5),
            1.0,
            difficulty_color(d.settings.difficulty) * k,
        );
        if Self::waiting_confirm(d) && (self.t / 30).is_multiple_of(2) && t > 125.0 {
            p.text_outlined(
                "PRESS CONFIRM",
                Vec2::new(cx, 200.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            );
        }
    }

    fn variant_strip(&self, p: &mut Painter, t: f32, cx: f32) {
        let n = self.variant_icons.len();
        if n == 0 {
            return;
        }
        // Pop in one per tick after 5, hold 120, pop out one per tick.
        let out_start = 5.0 + n as f32 + 120.0;
        for (k, icon) in self.variant_icons.iter().enumerate() {
            let col = k % 14;
            let row = k / 14;
            let in_row = (14).min(n - row * 14) as f32 - 1.0;
            let x = cx - in_row * 9.0 + col as f32 * 18.0;
            let y = 190.0 + row as f32 * 18.0;
            let tin = t - 6.0 - k as f32;
            let tout = t - out_start - k as f32;
            let s = if tout >= 0.0 {
                1.0 - back_in(tout / 8.0)
            } else {
                back_out(tin / 12.0)
            };
            if tin < 0.0 || s <= 0.0 {
                continue;
            }
            if let Some(sub) = p.a.m(icon) {
                let black = Color::BLACK;
                for (ox, oy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                    p.tex(sub, Vec2::new(x + ox, y + oy), Org::Center, s, black);
                }
                p.tex(sub, Vec2::new(x, y), Org::Center, s, Color::WHITE);
            }
        }
    }

    /// "LEVEL n" / FIGHT!! (5.1), on the Playing clock.
    fn level_card(&self, p: &mut Painter, d: &DarkWorld, dx: f32) {
        let t = d.phase_t;
        if d.phase != DwPhase::Playing || t >= 156.0 {
            return;
        }
        let cx = 160.0 + dx;
        self.variant_strip(p, t, cx);
        let exit = cube_in((t - 134.0) / 20.0).max(0.0);
        let shield_x = if t < 31.0 {
            -240.0
        } else if exit > 0.0 {
            240.0 * exit
        } else {
            -240.0 * (1.0 - cube_out((t - 31.0) / 20.0))
        };
        let rot = if exit > 0.0 {
            -PI / 9.0 * exit
        } else {
            -PI / 9.0 * (1.0 - cube_out((t - 31.0) / 20.0))
        };
        let scale = 1.0 + back_out((t - 73.0) / 20.0).max(0.0) * f32::from(t >= 73.0);
        if let Some(s) = p.a.g(shield_image(d.settings.difficulty)) {
            p.tex_ex(
                s,
                Vec2::new(cx + shield_x, 120.0),
                Org::Center,
                Vec2::splat(scale),
                rot,
                Color::WHITE,
                false,
            );
        }
        let ly = 110.0 - 130.0 * exit;
        p.text_outlined(
            &format!("LEVEL {}", d.round_index + 1),
            Vec2::new(cx, ly),
            (0.5, 0.5),
            2.0,
            LEVEL_COLOR,
        );
        if t >= 62.0 {
            let k = ((t - 62.0) / 10.0).min(1.0);
            let s = (8.0 + (2.0 - 8.0) * k).round();
            let fy = 130.0 + 130.0 * exit;
            p.text_outlined("FIGHT!!", Vec2::new(cx, fy), (0.5, 0.5), s, FIGHT_COLOR);
        }
    }

    /// Boss levels: the shield in/out and `DarkWorldBossTitle` (6).
    fn boss_card(&self, p: &mut Painter, d: &DarkWorld, boss: u8, dx: f32) {
        let t = d.phase_t;
        if d.phase != DwPhase::Playing {
            return;
        }
        let cx = 160.0 + dx;
        if t < 214.0 {
            self.variant_strip(p, t, cx);
            let exit = cube_in((t - 133.0) / 20.0).max(0.0);
            let shield_x = if t < 31.0 {
                -240.0
            } else if exit > 0.0 {
                240.0 * exit
            } else {
                -240.0 * (1.0 - cube_out((t - 31.0) / 20.0))
            };
            let rot = if exit > 0.0 {
                -PI / 9.0 * exit
            } else {
                -PI / 9.0 * (1.0 - cube_out((t - 31.0) / 20.0))
            };
            let scale = 1.0 + back_out((t - 72.0) / 20.0).max(0.0) * f32::from(t >= 72.0);
            if let Some(s) = p.a.g(shield_image(d.settings.difficulty)) {
                p.tex_ex(
                    s,
                    Vec2::new(cx + shield_x, 120.0),
                    Org::Center,
                    Vec2::splat(scale),
                    rot,
                    Color::WHITE,
                    false,
                );
            }
        }
        // The title: name, line, subtitles.
        if !(31.0..363.0).contains(&t) {
            return;
        }
        let (name, sub, sub_b) = boss_title(boss);
        let fade_out = ((t - 302.0) / 60.0).clamp(0.0, 1.0);
        let name_a = ((t - 31.0) / 60.0).clamp(0.0, 1.0) * (1.0 - fade_out);
        let sub_a = ((t - 62.0) / 60.0).clamp(0.0, 1.0) * (1.0 - fade_out);
        let line_w = 200.0 * ((t - 31.0) / 600.0).clamp(0.0, 1.0);
        if name_a > 0.0 {
            let half = (line_w / 2.0).floor();
            p.rect(
                cx - half - 1.0,
                119.0,
                2.0 * half + 2.0,
                3.0,
                Color::BLACK * name_a,
            );
            p.rect(cx - half, 120.0, 2.0 * half, 1.0, Color::WHITE * name_a);
            p.text_outlined(
                name,
                Vec2::new(cx, 110.0),
                (0.5, 0.5),
                2.0,
                BOSS_NAME * name_a,
            );
        }
        if sub_a > 0.0 {
            p.text_outlined(sub, Vec2::new(cx, 130.0), (0.5, 0.5), 1.0, BOSS_SUB * sub_a);
            if !sub_b.is_empty() {
                p.text_outlined(
                    sub_b,
                    Vec2::new(cx, 138.0),
                    (0.5, 0.5),
                    1.0,
                    BOSS_SUB * sub_a,
                );
            }
        }
    }

    /// `DarkWorldGameOver` (9.1).
    fn game_over(&self, p: &mut Painter, d: &DarkWorld, dx: f32) {
        let t = d.phase_t;
        let cx = 160.0 + dx;
        let k = ((t - 61.0) / 90.0).clamp(0.0, 1.0);
        if k > 0.0 {
            p.rect(0.0, 0.0, p.w, 240.0, Color::BLACK * (0.5 * k));
        }
        let banner = 0.5 * ((t - 122.0) / 60.0).clamp(0.0, 1.0);
        if banner > 0.0
            && let Some(b) = p.a.m("questResults/darkWorldBanner")
        {
            // `TextureBannerV`: 5-px rows swaying sideways.
            let mut y = 0;
            let mut i = 0;
            while y < b.height() {
                let h = 5.min(b.height() - y);
                let off = ((t * 0.03 + PI / 8.0 * i as f32).sin() * 4.0).round();
                p.tex(
                    b.sub(0, y, b.width(), h),
                    Vec2::new(cx - 100.0 + off, 28.0 + y as f32),
                    Org::TopLeft,
                    1.0,
                    Color::WHITE * banner,
                );
                y += 5;
                i += 1;
            }
        }
        // Arrow: flies in at 153, breaks at 180.
        let at = t - 153.0;
        if (0.0..27.0).contains(&at) {
            let x = 480.0 + (cx - 480.0) * cube_out(at / 10.0);
            p.m(
                "questResults/arrow",
                Vec2::new(x, 40.0),
                Org::Center,
                1.0,
                Color::WHITE,
            );
        } else if at >= 27.0 {
            let kb = cube_out((at - 27.0) / 20.0);
            let a = Color::WHITE * (1.0 - ((at - 60.0) / 60.0).clamp(0.0, 1.0));
            if let Some(s) = p.a.m("questResults/brokenArrowA") {
                p.tex_ex(
                    s,
                    Vec2::new(cx, 40.0),
                    Org::At(84.0, 13.0),
                    Vec2::ONE,
                    PI / 20.0 * kb,
                    a,
                    false,
                );
            }
            if let Some(s) = p.a.m("questResults/brokenArrowB") {
                p.tex_ex(
                    s,
                    Vec2::new(cx, 40.0 + 10.0 * kb),
                    Org::At(84.0 - 88.0, 13.0),
                    Vec2::ONE,
                    -PI / 10.0 * kb,
                    a,
                    false,
                );
            }
        }
        if k > 0.0 {
            let pop = 1.0 + 0.2 * up_down(cube_out((t - 159.0) / 20.0)) * f32::from(t >= 159.0);
            let col = Color::WHITE * k;
            match p.a.m("questResults/gameOver") {
                Some(s) if t >= 159.0 => {
                    p.tex_outlined(s, Vec2::new(cx, 40.0), Org::Center, pop, col)
                }
                Some(s) => p.tex(s, Vec2::new(cx, 40.0), Org::Center, 1.0, col),
                None => p.text_outlined("GAME OVER", Vec2::new(cx, 40.0), (0.5, 0.5), 2.0, col),
            }
        }
        // Ours: the level and what is left, under the banner.
        let st = ((t - 200.0) / 30.0).clamp(0.0, 1.0);
        if st > 0.0 {
            let levels = d.prog.len().max(1);
            let _ = levels;
            p.text_outlined(
                &format!("LEVEL {}", d.round_index + 1),
                Vec2::new(cx, 66.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE * st,
            );
            let cont = d.continues_remaining();
            let line = match cont {
                -1 => "CONTINUES: UNLIMITED".to_string(),
                n => format!("CONTINUES LEFT: {n}"),
            };
            let col = if cont == 0 { BAD } else { Color::WHITE };
            p.text_outlined(&line, Vec2::new(cx, 78.0), (0.5, 0.5), 1.0, col * st);
            p.text_outlined(
                &format!("{} MODE", difficulty_name(d.settings.difficulty)),
                Vec2::new(cx, 90.0),
                (0.5, 0.5),
                1.0,
                difficulty_color(d.settings.difficulty) * st,
            );
        }
    }

    /// `DarkWorldComplete` (9.2) with the per-player cards.
    fn complete(&self, p: &mut Painter, d: &DarkWorld, dx: f32, tick: u64) {
        let Some(r) = d.result.as_ref() else { return };
        let t = d.phase_t;
        let cx = 160.0 + dx;
        let n = d.n_players();
        let (eye_t, gold_t, time_t, cards_t, _) = complete_times(r, n);
        let fly = cube_out((t - 62.0) / 30.0);
        let fade = 0.6 * fly.max(0.0) * f32::from(t >= 62.0);
        if fade > 0.0 {
            p.rect(0.0, 0.0, p.w, 240.0, Color::BLACK * fade);
        }
        if t >= 123.0 {
            let x = 480.0 + (cx - 480.0) * cube_out((t - 123.0) / 10.0);
            p.m(
                "questResults/arrow",
                Vec2::new(x, 20.0),
                Org::Center,
                1.0,
                Color::WHITE,
            );
        }
        if t >= 62.0 {
            let y = 280.0 + (20.0 - 280.0) * fly;
            let pop = 1.0 + 0.2 * up_down(cube_out((t - 129.0) / 20.0)) * f32::from(t >= 129.0);
            p.m_outlined(
                "questResults/complete",
                Vec2::new(cx, y),
                Org::Center,
                pop,
                Color::WHITE,
            );
        }
        if t >= 123.0 {
            let k = cube_out((t - 123.0) / 60.0);
            p.text_outlined(
                &format!("{} MODE", difficulty_name(r.difficulty)),
                Vec2::new(cx, 55.0 - 20.0 * k),
                (0.5, 0.5),
                1.0,
                difficulty_color(r.difficulty) * k,
            );
        }
        let gold = super::LEGENDARY;
        let line = |p: &mut Painter, text: &str, at: f32, from_x: f32, y: f32, col: Color| {
            if t < at {
                return;
            }
            let k = cube_out((t - at) / 30.0);
            p.text_outlined(
                text,
                Vec2::new(cx + from_x * (1.0 - k), y),
                (0.5, 0.5),
                1.0,
                col * k,
            );
        };
        line(
            p,
            &format!("CONTINUES: {}", r.continues),
            181.0,
            -25.0,
            58.0,
            if r.continues == 0 { gold } else { BAD },
        );
        line(
            p,
            &format!("DEATHS: {}", r.deaths),
            212.0,
            25.0,
            68.0,
            if r.deaths == 0 { gold } else { BAD },
        );
        // Eye awards (ours: right of the two lines; the original centres
        // them over the lines).
        let eye_at = Vec2::new(cx + 62.0, 63.0);
        if let Some(et) = eye_t
            && t >= et
        {
            let k = cube_out((t - et) / 60.0);
            let (eye_x, gold_k) = match gold_t {
                Some(gt) if t >= gt => (
                    -12.0 * ((t - gt) / 60.0).min(1.0),
                    Some(cube_out((t - gt) / 60.0)),
                ),
                _ => (0.0, None),
            };
            let col = Color::WHITE * k;
            if let Some(s) = p.a.m("questResults/bigEye") {
                p.tex_outlined(s, eye_at + Vec2::new(eye_x, 0.0), Org::Center, 2.0 - k, col);
            }
            if let (Some(gk), Some(gt)) = (gold_k, gold_t)
                && let Some(s) = p.a.m("questResults/bigGoldEye")
            {
                let gx = 12.0 * ((t - gt) / 60.0).min(1.0);
                p.tex_outlined(
                    s,
                    eye_at + Vec2::new(gx, 0.0),
                    Org::Center,
                    2.0 - gk,
                    Color::WHITE * gk,
                );
            }
        }
        // Total time with a wiggle.
        if t >= time_t {
            let wv = {
                let tt = t - time_t;
                if tt < 30.0 {
                    (tt / 30.0 * 4.0 * TAU).sin() * (1.0 - tt / 30.0)
                } else {
                    0.0
                }
            };
            p.text_outlined(
                "TIME",
                Vec2::new(cx, 90.0),
                (0.5, 0.5),
                (1.0 + 0.2 * wv).round().max(1.0),
                TIME_COLOR,
            );
            p.text_outlined(
                &tfp_sim::trials::time_string(r.time),
                Vec2::new(cx, 104.0),
                (0.5, 0.5),
                2.0,
                TIME_COLOR,
            );
        }
        if t >= cards_t {
            self.cards(p, d, r, t - cards_t, dx, tick);
        }
    }

    fn cards(&self, p: &mut Painter, d: &DarkWorld, r: &DwResult, t: f32, dx: f32, tick: u64) {
        let slots = d.active_slots();
        let n = slots.len().max(1);
        let to: Vec<f32> = match n {
            1 => vec![160.0],
            2 => vec![110.0, 210.0],
            3 => vec![80.0, 160.0, 240.0],
            4 => vec![40.0, 120.0, 200.0, 280.0],
            // Ours: an even spread beyond four.
            _ => (0..n)
                .map(|i| 320.0 * (i as f32 + 0.5) / n as f32)
                .collect(),
        };
        let from: Vec<f32> = match n {
            1 => vec![160.0],
            2 => vec![80.0, 240.0],
            3 => vec![50.0, 160.0, 270.0],
            4 => vec![10.0, 110.0, 210.0, 310.0],
            _ => to.clone(),
        };
        let gold = super::LEGENDARY;
        let best = |vals: &[u32; 12], max: bool| -> u32 {
            let it = slots.iter().map(|&s| vals[s]);
            if max {
                it.max().unwrap_or(0)
            } else {
                it.min().unwrap_or(0)
            }
        };
        let (bk, bd, br, bb) = (
            best(&r.kills, true),
            best(&r.deaths_per, false),
            best(&r.revives, true),
            best(&r.boss_hits, true),
        );
        let k = cube_out(t / 30.0);
        for (i, &slot) in slots.iter().enumerate() {
            let x = from[i] + (to[i] - from[i]) * k + dx;
            let y = 360.0 + (220.0 - 360.0) * k;
            let won = n > 1 && r.crowns[slot];
            let bg = if won || n == 1 {
                "versusResults/winnerbg"
            } else {
                "versusResults/bg"
            };
            p.m(bg, Vec2::new(x, y), Org::Center, 1.0, Color::WHITE);
            let top = y - 100.0;
            let card = self.cards.iter().find(|c| c.slot == slot);
            let look = card.and_then(|c| {
                let base = p.a.archers.get(c.archer)?;
                Some(if c.alt {
                    base.alt.as_deref().unwrap_or(base).clone()
                } else {
                    base.clone()
                })
            });
            let mut py = top + 15.0 + 30.0;
            if let Some(l) = look
                && let Some(pt) = p.a.m(&l.joined.replace("joined", "win"))
            {
                let ph = pt.height() as f32;
                py = top + 15.0 + ph / 2.0;
                p.tex(pt, Vec2::new(x, py), Org::Center, 1.0, Color::WHITE);
                py += ph / 2.0;
            }
            if won {
                if let Some(cr) = p.a.g("versus/crown") {
                    p.tex(cr, Vec2::new(x, top - 6.0), Org::Center, 1.0, Color::WHITE);
                }
                let _ = tick;
            }
            let mut rows: Vec<(String, bool)> = vec![
                (
                    format!("KILLS: {}", r.kills[slot]),
                    n > 1 && r.kills[slot] == bk,
                ),
                (
                    format!("DEATHS: {}", r.deaths_per[slot]),
                    n > 1 && r.deaths_per[slot] == bd,
                ),
            ];
            if n > 1 {
                rows.push((
                    format!("REVIVES: {}", r.revives[slot]),
                    r.revives[slot] == br,
                ));
                rows.push((
                    format!("BOSS: {}", r.boss_hits[slot]),
                    r.boss_hits[slot] == bb,
                ));
            } else {
                rows.push((format!("COMBO: {}", r.best_combo[slot]), false));
            }
            for (j, (text, best)) in rows.iter().enumerate() {
                let at = 30.0 * (j as f32 + 1.0);
                if t < at {
                    continue;
                }
                let kk = cube_out((t - at) / 30.0);
                let col = if *best { gold } else { Color::WHITE };
                p.text_outlined(
                    text,
                    Vec2::new(x - 20.0 * (1.0 - kk), py + 10.0 * (j as f32 + 1.0)),
                    (0.5, 0.5),
                    1.0,
                    col * kk,
                );
            }
        }
    }
}

/// Completion timeline (`DarkWorldComplete`): (eye, gold eye, time, cards,
/// end) in ticks of the Complete phase.
pub fn complete_times(r: &DwResult, players: usize) -> (Option<f32>, Option<f32>, f32, f32, f32) {
    let mut after = 243.0;
    let mut eye = None;
    let mut gold = None;
    if r.difficulty >= 2 && r.continues == 0 {
        let e = 243.0 + 20.0;
        eye = Some(e);
        after = e + 60.0;
        if r.deaths == 0 {
            let g = after + 90.0;
            gold = Some(g);
            after = g + 60.0;
        }
        after += 60.0;
    }
    let time = after + 30.0;
    let cards = time + 40.0;
    let end = cards + 90.0 + if players > 1 { 30.0 } else { 0.0 } + 60.0;
    (eye, gold, time, cards, end)
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn complete_timeline() {
        let mut r = DwResult {
            completed: true,
            tower: 0,
            difficulty: 0,
            players: 1,
            time: 100,
            continues: 0,
            deaths: 0,
            curses: 0,
            eye: false,
            gold_eye: false,
            kills: [0; 12],
            deaths_per: [0; 12],
            revives: [0; 12],
            boss_hits: [0; 12],
            best_combo: [0; 12],
            crowns: [false; 12],
        };
        let (e, g, time, cards, end) = complete_times(&r, 1);
        assert!(e.is_none() && g.is_none());
        assert!(time < cards && cards < end);
        r.difficulty = 2;
        let (e, g, time2, _, _) = complete_times(&r, 1);
        assert!(e.is_some() && g.is_some() && time2 > time);
        let t = Tutorial::new(None);
        assert!(!t.finished());
    }
}
