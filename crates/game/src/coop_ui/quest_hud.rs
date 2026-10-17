//! Quest HUD and flow screens (quest.md 3, 4.5, 4.6, 5.4, 5.6, 5.7): the
//! title card, lives gems (with the gem flying to a reviving corpse),
//! steal-a-life hint, wave banner, gauntlet counter, game over and
//! complete screens with the stats, the lives tally, skulls and time.
//!
//! Timelines follow the sim's `phase_t` where the sim owns the timing
//! (game over / complete readiness) and local clocks elsewhere.

use tfp_render::Color;
use tfp_render::glam::Vec2;
use tfp_sim::entity::Kind;
use tfp_sim::quest::{Op, Quest, QuestPhase};
use tfp_sim::{Content, World};

use super::art::Art;
use crate::front::gfx::{Org, Painter, back_out, cube_in, cube_out, lerp, rgba};

const NORMAL: Color = Color::hex(0x96DDFF);
const HARDCORE: Color = Color::hex(0xFF5432);

/// A gem flying from the HUD to a corpse (`SpendLife`).
#[derive(Clone, Debug)]
struct GemFly {
    gem: usize,
    from: Vec2,
    to: Vec2,
    t: u32,
}

pub struct QuestHud {
    pub title: String,
    pub icon: String,
    /// Per active slot: (slot, base archer for the gem).
    pub slots: Vec<(usize, usize)>,
    pub tips: Vec<(String, String, bool)>,
    pub tip: usize,
    t: u32,
    /// Intro: ticks since the card was confirmed.
    confirmed: Option<u32>,
    seen_wait: bool,
    banner: Option<(i32, u32)>,
    last_wave: i32,
    /// Gauntlet counter: total, last value, pulse ticks, ticks shown, zero at.
    g_total: Option<i32>,
    g_last: i32,
    g_pulse: u32,
    g_t: u32,
    g_zero: Option<u32>,
    lives_prev: Vec<i32>,
    flying: Vec<GemFly>,
    pub shake: u32,
    /// How the attempt ended (true = complete).
    pub won: Option<bool>,
}

/// The two-line tips of `questTips.xml` (`|` separates the lines).
pub fn load_tips(inst: &tfp_assets::GameInstall) -> Vec<(String, String, bool)> {
    let path = inst.content.game_data_dir().join("questTips.xml");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.split("<tip>")
        .skip(1)
        .filter_map(|t| {
            let body = t.split("<text>").nth(1)?.split("</text>").next()?;
            let coop = t.contains("<coop>true</coop>");
            let (a, b) = body.split_once('|').unwrap_or((body, ""));
            Some((a.trim().to_owned(), b.trim().to_owned(), coop))
        })
        .collect()
}

impl QuestHud {
    pub fn new(title: String, icon: String, slots: Vec<(usize, usize)>) -> Self {
        Self {
            title,
            icon,
            slots,
            tips: Vec::new(),
            tip: 0,
            t: 0,
            confirmed: None,
            seen_wait: false,
            banner: None,
            last_wave: -1,
            g_total: None,
            g_last: 0,
            g_pulse: 0,
            g_t: 0,
            g_zero: None,
            lives_prev: Vec::new(),
            flying: Vec::new(),
            shake: 0,
            won: None,
        }
    }

    /// The title card is waiting for Confirm (no music yet).
    pub fn card_up(&self) -> bool {
        self.confirmed.is_none()
    }

    fn waiting_confirm(q: &Quest) -> bool {
        q.prog.get(q.pc) == Some(&Op::WaitConfirm)
    }

    fn gem_pos(&self, k: usize, i: usize, w: f32) -> Vec2 {
        let row = (k / 2) as f32 * 12.0;
        if k.is_multiple_of(2) {
            Vec2::new(8.0 + 10.0 * i as f32, 13.0 + row)
        } else {
            Vec2::new(w - 8.0 - 10.0 * i as f32, 13.0 + row)
        }
    }

    /// After each sim step.
    pub fn after_step(&mut self, w: &World, width: f32) {
        let Some(q) = w.quest.as_deref() else { return };
        self.t += 1;
        self.shake = self.shake.saturating_sub(1);
        match q.phase {
            QuestPhase::GameOver => self.won = Some(false),
            QuestPhase::Complete => self.won = Some(true),
            _ => {}
        }
        if Self::waiting_confirm(q) {
            self.seen_wait = true;
        } else if self.seen_wait || q.phase != QuestPhase::Intro {
            self.confirmed = Some(self.confirmed.map_or(0, |c| c + 1));
        }
        if q.current_wave != self.last_wave && q.current_wave >= 0 && !q.ending {
            self.last_wave = q.current_wave;
            self.banner = Some((q.current_wave, 0));
        }
        if let Some((_, t)) = &mut self.banner {
            *t += 1;
            if *t > 12 * q.total_waves.max(1) as u32 + 110 {
                self.banner = None;
            }
        }
        // Gauntlet counter.
        if let Some(g) = q.gauntlet {
            if self.g_total.is_none() {
                self.g_total = Some(g);
                self.g_last = g;
            }
            self.g_t += 1;
            if g != self.g_last {
                self.g_last = g;
                self.g_pulse = 30;
                self.shake = 30;
                if g == 0 {
                    self.g_zero = Some(0);
                }
            }
            self.g_pulse = self.g_pulse.saturating_sub(1);
            if let Some(z) = &mut self.g_zero {
                *z += 1;
            }
        }
        // Lives: a spent life flies to the corpse.
        let slots = q.active_slots();
        if self.lives_prev.len() != slots.len() {
            self.lives_prev = slots.iter().map(|&p| q.lives[p]).collect();
        }
        for (k, &p) in slots.iter().enumerate() {
            let now = q.lives[p];
            if now < self.lives_prev[k] {
                let corpse = w
                    .entities()
                    .find(|&e| matches!(w.kind(e), Some(Kind::Corpse(c)) if c.player_index == p as i8))
                    .map(|e| w.core(e).pos);
                let gem = self.slots.get(k).map_or(0, |s| s.1);
                if let Some(c) = corpse {
                    self.flying.push(GemFly {
                        gem,
                        from: self.gem_pos(k, now.max(0) as usize, width),
                        to: Vec2::new(c.x, c.y),
                        t: 0,
                    });
                }
            }
            self.lives_prev[k] = now;
        }
        for f in &mut self.flying {
            f.t += 1;
        }
        self.flying.retain(|f| f.t < 61);
    }

    #[allow(clippy::too_many_arguments)]
    fn gem(
        &self,
        p: &mut Painter,
        c: &Content,
        art: &Art,
        archer: usize,
        pos: Vec2,
        scale: f32,
        rot: f32,
        tick: u64,
    ) {
        let Some(s) = c.sprite(art.sprite(&format!("Gem{}", archer.min(8)))) else {
            p.rect(pos.x - 3.0, pos.y - 3.0, 6.0, 6.0, Color::WHITE);
            return;
        };
        let name = &c.texture(s.texture).name;
        let Some(tex) = p.a.g(name) else { return };
        let f = super::art::frame_at(s, "0", tick as f32) as i32;
        let (fw, fh) = (s.frame_w as i32, s.frame_h as i32);
        let cols = (tex.width() / fw.max(1)).max(1);
        let sub = tex.sub((f % cols) * fw, (f / cols) * fh, fw, fh);
        p.tex_ex(
            sub,
            pos,
            Org::At(s.origin.x, s.origin.y),
            Vec2::splat(scale),
            rot,
            Color::WHITE,
            false,
        );
    }

    /// The gameplay HUD (title card, lives, banner, counter) and the result
    /// screens, drawn over the composited level.
    pub fn draw(&self, p: &mut Painter, w: &World, c: &Content, art: &Art, tick: u64) {
        let Some(q) = w.quest.as_deref() else { return };
        let width = p.w;
        let cx = width / 2.0;
        // Lives.
        let playing = matches!(q.phase, QuestPhase::Playing | QuestPhase::Ending);
        if playing || q.phase == QuestPhase::Intro {
            for (k, &slot) in q.active_slots().iter().enumerate() {
                let archer = self.slots.get(k).map_or(0, |s| s.1);
                for i in 0..q.lives[slot].max(0) as usize {
                    let pos = self.gem_pos(k, i, width);
                    self.gem(p, c, art, archer, pos, 1.0, 0.0, tick + i as u64 * 7);
                }
                if q.can_steal[slot] && (tick / 20).is_multiple_of(2) {
                    let pos = self.gem_pos(k, 0, width) + Vec2::new(0.0, 10.0);
                    let just = if k % 2 == 0 { (0.0, 0.5) } else { (1.0, 0.5) };
                    p.text_outlined("PRESS JUMP: STEAL A LIFE", pos, just, 1.0, Color::WHITE);
                }
            }
        }
        for f in &self.flying {
            let (pos, scale, rot) = if f.t <= 30 {
                let k = cube_out(f.t as f32 / 30.0);
                let top = Vec2::new(f.to.x, (f.to.y - 40.0).max(30.0));
                (lerp(f.from, top, k), 1.0 + k, -std::f32::consts::TAU * k)
            } else if f.t <= 40 {
                (Vec2::new(f.to.x, (f.to.y - 40.0).max(30.0)), 2.0, 0.0)
            } else {
                let k = cube_in(((f.t - 40) as f32 / 20.0).min(1.0));
                let top = Vec2::new(f.to.x, (f.to.y - 40.0).max(30.0));
                (lerp(top, f.to, k), 2.0 * (1.0 - k), 0.0)
            };
            self.gem(p, c, art, f.gem, pos, scale, rot, tick);
        }
        // Gauntlet counter.
        if let (Some(_), true) = (self.g_total, playing) {
            let tin = cube_out((self.g_t as f32 / 30.0).min(1.0));
            let mut y = -20.0 + 32.0 * tin;
            if let Some(z) = self.g_zero
                && z > 60
            {
                y -= 32.0 * cube_in(((z - 60) as f32 / 30.0).min(1.0));
            }
            let pulse = self.g_pulse as f32 / 30.0;
            let col = Color::WHITE.lerp(Color::hex(0xFF3030), pulse);
            let scale = 1.0 + 0.5 * pulse;
            p.m(
                "questResults/whiteSkull",
                Vec2::new(cx - 10.0, y),
                Org::Center,
                1.0,
                Color::WHITE,
            );
            p.text_outlined(
                &format!("x {}", self.g_last.max(0)),
                Vec2::new(cx - 3.0, y),
                (0.0, 0.5),
                scale.round().max(1.0),
                col,
            );
        }
        // Wave banner.
        if let Some((wave, t)) = self.banner
            && playing
        {
            self.wave_banner(p, q, wave, t);
        }
        match (q.phase, self.won) {
            (QuestPhase::Intro, _) => self.title_card(p, q),
            (QuestPhase::GameOver, _) | (QuestPhase::Finished, Some(false)) => self.game_over(p, q),
            (QuestPhase::Complete, _) | (QuestPhase::Finished, Some(true)) => {
                self.complete(p, w, c, art, q, tick)
            }
            _ => {}
        }
    }

    /// Title card (quest.md 3 step 3): emblem slam, theme title, mode line,
    /// waits for Confirm, then slides off.
    fn title_card(&self, p: &mut Painter, q: &Quest) {
        let width = p.w;
        let cx = width / 2.0;
        let t = q.phase_t;
        // Program: wait(30) wait(30) -> slam; the card leaves when confirmed.
        let out = self
            .confirmed
            .map_or(0.0, |c| cube_in((c as f32 / 30.0).min(1.0)));
        if out >= 1.0 {
            return;
        }
        p.rect(0.0, 0.0, width, 240.0, Color::BLACK * (0.5 * (1.0 - out)));
        let slam = ((t - 62.0) / 10.0).clamp(0.0, 1.0);
        if t >= 62.0 {
            let scale = 6.0 - 4.0 * cube_in(slam);
            let rot = 0.15 * (1.0 - slam);
            let y = 80.0 - 300.0 * out;
            if let Some(icon) = p.a.m(&self.icon) {
                p.tex_ex(
                    icon,
                    Vec2::new(cx, y),
                    Org::Center,
                    Vec2::splat(scale),
                    rot,
                    Color::WHITE,
                    false,
                );
            }
            let k = cube_out(((t - 62.0) / 60.0).min(1.0));
            let ty = 130.0 + 20.0 * (1.0 - k) + 300.0 * out;
            p.text_outlined(
                &self.title,
                Vec2::new(cx, ty),
                (0.5, 0.5),
                2.0,
                Color::WHITE * k,
            );
            let (line, col) = if q.settings.hardcore {
                ("HARDCORE MODE", HARDCORE)
            } else {
                ("NORMAL MODE", NORMAL)
            };
            p.text_outlined(line, Vec2::new(cx, ty + 18.0), (0.5, 0.5), 1.0, col * k);
            if Self::waiting_confirm(q) && (self.t / 30).is_multiple_of(2) {
                p.text_outlined(
                    "PRESS CONFIRM",
                    Vec2::new(cx, 200.0),
                    (0.5, 0.5),
                    1.0,
                    Color::WHITE,
                );
            }
        }
    }

    /// Wave numerals (quest.md 4.5).
    fn wave_banner(&self, p: &mut Painter, q: &Quest, wave: i32, t: u32) {
        let n = q.total_waves.max(1) as usize;
        let cx = p.w / 2.0;
        let Some(strip) = p.a.g("quest/waveNumerals") else {
            p.text_outlined(
                &format!("WAVE {}", wave + 1),
                Vec2::new(cx, 100.0),
                (0.5, 0.5),
                2.0,
                Color::WHITE,
            );
            return;
        };
        let shield = if q.settings.hardcore {
            "quest/shieldHardcore"
        } else {
            "quest/shield"
        };
        let t_out0 = 5 * n as u32 + 30 + 20 + 40;
        for i in 0..n {
            let x = cx + (i as f32 - (n as f32 - 1.0) / 2.0) * 24.0;
            let tin = t as f32 - 5.0 * i as f32;
            let kin = back_out((tin / 30.0).clamp(0.0, 1.0));
            let tout = t as f32 - t_out0 as f32 - 5.0 * i as f32;
            let kout = cube_in((tout / 30.0).clamp(0.0, 1.0));
            if kin <= 0.0 || kout >= 1.0 {
                continue;
            }
            let y = 130.0 - 30.0 * kin + 40.0 * kout;
            let a = (kin.min(1.0) * (1.0 - kout)).clamp(0.0, 1.0);
            let cur = i as i32 == wave;
            let grow_t = t as f32 - (5 * n as u32 + 30) as f32;
            let g = if cur {
                back_out((grow_t / 20.0).clamp(0.0, 1.0))
            } else {
                0.0
            };
            let scale = 1.0 + g;
            if cur && let Some(s) = p.a.g(shield) {
                p.tex(
                    s,
                    Vec2::new(x, y),
                    Org::Center,
                    0.5 + 0.5 * g,
                    Color::WHITE * a,
                );
            }
            let cell = strip.sub(0, 15 * (i.min(9) as i32), 22, 15);
            let col = if cur {
                Color::WHITE
            } else {
                Color::WHITE * 0.8
            };
            p.tex(cell, Vec2::new(x, y), Org::Center, scale, col * a);
        }
    }

    /// `QuestGameOver` (quest.md 5.6).
    fn game_over(&self, p: &mut Painter, q: &Quest) {
        let width = p.w;
        let cx = width / 2.0;
        let t = q.phase_t;
        let dark = ((t - 60.0) / 90.0).clamp(0.0, 1.0);
        p.rect(0.0, 0.0, width, 240.0, Color::BLACK * (0.5 * dark));
        if t < 60.0 {
            return;
        }
        let title_y = 66.0;
        // Arrow: flies in at ~211, pops, breaks at ~246.
        let arrow_t = t - 211.0;
        if (0.0..35.0).contains(&arrow_t) {
            let x = 480.0 + (cx - 480.0) * cube_out((arrow_t / 10.0).min(1.0));
            p.m(
                "questResults/arrow",
                Vec2::new(x, title_y),
                Org::Center,
                1.0,
                Color::WHITE,
            );
        } else if arrow_t >= 35.0 {
            let k = ((arrow_t - 35.0) / 20.0).min(1.0);
            let drift = 40.0 * cube_out(k);
            let fall = 60.0 * k * k;
            p.list.sprite(
                p.a.m("questResults/brokenArrowA")
                    .unwrap_or_else(|| p.a.m("questResults/arrow").unwrap()),
                &tfp_render::SpriteParams::at(Vec2::new(cx - 44.0 - drift, title_y + fall).floor())
                    .origin(Vec2::new(44.0, 13.0))
                    .rotation(-0.6 * k)
                    .color(Color::WHITE * (1.0 - k)),
            );
            if let Some(b) = p.a.m("questResults/brokenArrowB") {
                p.list.sprite(
                    b,
                    &tfp_render::SpriteParams::at(
                        Vec2::new(cx + 42.0 + drift, title_y + fall).floor(),
                    )
                    .origin(Vec2::new(41.0, 13.0))
                    .rotation(0.6 * k)
                    .color(Color::WHITE * (1.0 - k)),
                );
            }
        }
        let fade = ((t - 60.0) / 90.0).clamp(0.0, 1.0);
        let pop_t = t - 226.0;
        let pop = if (0.0..20.0).contains(&pop_t) {
            1.0 + 0.2 * (pop_t / 20.0 * std::f32::consts::PI).sin()
        } else {
            1.0
        };
        if let Some(s) = p.a.m("questResults/gameOver") {
            let col = Color::WHITE * fade;
            if pop_t >= 0.0 {
                p.tex_outlined(s, Vec2::new(cx, title_y), Org::Center, 2.0 * pop, col);
            } else {
                p.tex(s, Vec2::new(cx, title_y), Org::Center, 2.0, col);
            }
        } else {
            p.text_outlined(
                "GAME OVER",
                Vec2::new(cx, title_y),
                (0.5, 0.5),
                3.0,
                Color::WHITE * fade,
            );
        }
        let status_t = t - 296.0;
        if status_t >= 0.0 {
            let a = (status_t / 60.0).min(1.0);
            let line = match (q.gauntlet, self.g_total) {
                (Some(left), Some(total)) => format!("{left} OF {total} MONSTERS REMAIN"),
                _ => format!("WAVE {} OF {}", (q.current_wave + 1).max(1), q.total_waves),
            };
            p.text_outlined(
                &line,
                Vec2::new(cx, 96.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE * a,
            );
            if q.settings.hardcore {
                p.text_outlined(
                    "HARDCORE MODE",
                    Vec2::new(cx, 108.0),
                    (0.5, 0.5),
                    1.0,
                    HARDCORE * a,
                );
            }
        }
        let tip_t = t - 356.0;
        if !q.settings.hardcore && tip_t >= 0.0 && !self.tips.is_empty() {
            let solo = q.active_slots().len() < 2;
            let usable: Vec<&(String, String, bool)> =
                self.tips.iter().filter(|t| !(solo && t.2)).collect();
            if !usable.is_empty() {
                let tip = usable[self.tip % usable.len()];
                let a = (tip_t / 30.0).min(1.0);
                p.m(
                    "questResults/tipBanner",
                    Vec2::new(cx, 145.0),
                    Org::Center,
                    1.0,
                    Color::WHITE * a,
                );
                p.text_outlined(
                    &tip.0,
                    Vec2::new(cx, 139.0),
                    (0.5, 0.5),
                    1.0,
                    Color::WHITE * a,
                );
                p.text_outlined(
                    &tip.1,
                    Vec2::new(cx, 151.0),
                    (0.5, 0.5),
                    1.0,
                    Color::WHITE * a,
                );
            }
        }
    }

    /// `QuestComplete` (quest.md 5.7), on the sim's completion timeline.
    fn complete(&self, p: &mut Painter, w: &World, c: &Content, art: &Art, q: &Quest, tick: u64) {
        let width = p.w;
        let cx = width / 2.0;
        let t = q.phase_t;
        let dark = ((t - 60.0) / 30.0).clamp(0.0, 1.0);
        p.rect(0.0, 0.0, width, 240.0, Color::BLACK * (0.6 * dark));
        if t < 60.0 {
            return;
        }
        // Banner flies in from the left.
        let k = cube_out(((t - 61.0) / 30.0).clamp(0.0, 1.0));
        let by = 36.0;
        let bx = -160.0 + (cx + 160.0) * k;
        p.m(
            "questResults/top",
            Vec2::new(bx, by),
            Org::Center,
            1.0,
            Color::WHITE,
        );
        let pop_t = t - 136.0;
        let pop = if (0.0..20.0).contains(&pop_t) {
            1.0 + 0.2 * (pop_t / 20.0 * std::f32::consts::PI).sin()
        } else {
            1.0
        };
        p.m_outlined(
            "questResults/complete",
            Vec2::new(bx, by),
            Org::Center,
            2.0 * pop,
            Color::WHITE,
        );
        let arrow_t = t - 121.0;
        if arrow_t >= 0.0 {
            let x = 480.0 + (cx - 480.0) * cube_out((arrow_t / 10.0).min(1.0));
            p.m(
                "questResults/arrow",
                Vec2::new(x, by + 20.0),
                Org::Center,
                1.0,
                Color::WHITE,
            );
        }
        // Bottom banner drops with a bounce (mode, later the time).
        let drop_t = t - 157.0;
        let time_t = t - (544.0 + 22.0 * total_lives(q) as f32);
        if drop_t >= 0.0 {
            let k = back_out((drop_t / 60.0).min(1.0));
            let y = -20.0 + (172.0 + 20.0) * k;
            p.m(
                "questResults/bottom",
                Vec2::new(cx, y),
                Org::Center,
                1.0,
                Color::WHITE,
            );
            if time_t >= 0.0 {
                let a = (time_t / 20.0).min(1.0);
                p.text(
                    &format!("TIME: {}", tfp_sim::trials::time_string(q.time)),
                    Vec2::new(cx, y),
                    (0.5, 0.5),
                    1.0,
                    Color::WHITE * a,
                );
            } else {
                let mode = if q.settings.hardcore {
                    ("HARDCORE", HARDCORE)
                } else {
                    ("NORMAL", NORMAL)
                };
                p.text(mode.0, Vec2::new(cx, y), (0.5, 0.5), 1.0, mode.1);
            }
        }
        // Per-player stats.
        let slots = q.active_slots();
        let n = slots.len().max(1);
        let stats_t = t - 272.0;
        if stats_t >= 0.0 {
            let (kw, dw, cw) = best_categories(q, &slots);
            for (k, &slot) in slots.iter().enumerate() {
                let x =
                    cx + (k as f32 - (n as f32 - 1.0) / 2.0) * (if n > 2 { 70.0 } else { 100.0 });
                let archer = self.slots.get(k).map_or(0, |s| s.1);
                if let Some(a) = p.a.archers.get(archer) {
                    p.m(
                        &a.joined,
                        Vec2::new(x, 84.0),
                        Org::Center,
                        0.5,
                        Color::WHITE,
                    );
                }
                let rows = [
                    ("KILLS", q.kills[slot], kw.contains(&slot)),
                    ("DEATHS", q.deaths[slot], dw.contains(&slot)),
                    ("COMBO", q.best_combo[slot], cw.contains(&slot)),
                ];
                for (r, (label, v, best)) in rows.iter().enumerate() {
                    if stats_t >= 30.0 * (r + 1) as f32 {
                        let col = if *best && n > 1 {
                            Color::hex(0xFFDE3D)
                        } else {
                            Color::WHITE
                        };
                        p.text_outlined(
                            &format!("{label} {v}"),
                            Vec2::new(x, 118.0 + 10.0 * r as f32),
                            (0.5, 0.5),
                            1.0,
                            col,
                        );
                    }
                }
            }
        }
        // Lives tally.
        let lives: Vec<(usize, usize)> = slots
            .iter()
            .enumerate()
            .flat_map(|(k, &s)| {
                let gem = self.slots.get(k).map_or(0, |x| x.1);
                std::iter::repeat_n(gem, q.lives[s].max(0) as usize).map(move |g| (k, g))
            })
            .collect();
        let tally_t = t - 513.0;
        let total = lives.len();
        if tally_t >= 0.0 {
            for (i, (_, gem)) in lives.iter().enumerate() {
                let at = 10.0 + 22.0 * i as f32;
                if tally_t >= at {
                    let x = cx + (i as f32 - (total as f32 - 1.0) / 2.0) * 12.0;
                    self.gem(p, c, art, *gem, Vec2::new(x, 154.0), 1.0, 0.0, tick);
                }
            }
        }
        let no_deaths = total >= if slots.len() <= 1 { 5 } else { 8 };
        let skull_t = t - (513.0 + 22.0 * total as f32);
        if skull_t >= 0.0 {
            let name = if no_deaths && q.settings.hardcore {
                "questResults/bigGoldSkull"
            } else if q.settings.hardcore {
                "questResults/bigRedSkull"
            } else {
                "questResults/bigWhiteSkull"
            };
            let s = back_out((skull_t / 20.0).min(1.0));
            let x = cx + (total as f32 / 2.0) * 12.0 + 16.0;
            p.m(name, Vec2::new(x, 154.0), Org::Center, s, Color::WHITE);
        }
        let _ = (w, rgba([0; 4]));
    }
}

fn total_lives(q: &Quest) -> usize {
    q.active_slots()
        .iter()
        .map(|&s| q.lives[s].max(0) as usize)
        .sum()
}

/// Category winners for 2+ players: most kills, fewest deaths, best combo.
pub fn best_categories(q: &Quest, slots: &[usize]) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    let best = |f: &dyn Fn(usize) -> i64| -> Vec<usize> {
        let m = slots.iter().map(|&s| f(s)).max().unwrap_or(0);
        slots.iter().copied().filter(|&s| f(s) == m).collect()
    };
    (
        best(&|s| q.kills[s] as i64),
        best(&|s| -(q.deaths[s] as i64)),
        best(&|s| q.best_combo[s] as i64),
    )
}

/// Crowns after a clear with 2+ players (quest.md 5.5): the slot(s) with
/// the most category wins.
pub fn crowns(q: &Quest) -> Vec<usize> {
    let slots = q.active_slots();
    if slots.len() < 2 {
        return Vec::new();
    }
    let (k, d, c) = best_categories(q, &slots);
    let wins = |s: usize| [&k, &d, &c].iter().filter(|v| v.contains(&s)).count();
    let m = slots.iter().map(|&s| wins(s)).max().unwrap_or(0);
    slots.into_iter().filter(|&s| wins(s) == m).collect()
}
