//! Trials HUD and screens (trials.md 5-7): the title card with the goal
//! board, the clock and next goal, the dummy counter icons, the win flash
//! and the results screen (COMPLETE! / FAILURE, time, goal board with the
//! new-best and medal animations).

use tfp_render::Color;
use tfp_render::glam::Vec2;
use tfp_sim::World;
use tfp_sim::trials::{TrialsPhase, ticks, time_string};

use crate::front::gfx::{Org, Painter, back_out, cube_in, cube_out};

const COMPLETE: Color = Color::hex(0xB6FF00);
const NEW_BEST: Color = Color::hex(0xFFDE3D);

pub struct TrialsHud {
    pub title: String,
    pub icon: String,
    /// Best time and medals before this attempt (the board shows them).
    pub best_before: u32,
    pub unlocked_before: [bool; 3],
    t: u32,
    /// Ticks since each dummy icon popped (index = icon).
    pops: Vec<Option<u32>>,
    drawing: u32,
    phase: Option<TrialsPhase>,
    phase_t: u32,
    flash: f32,
    confirmed: Option<u32>,
    /// Yellow gems found before this attempt (the "n / 7" text).
    pub gems_found: usize,
    /// Where the gem pickup was last seen, and the float text (pos, ticks).
    gem_at: Option<tfp_sim::Vec2>,
    gem_text: Option<(tfp_sim::Vec2, u32)>,
}

impl TrialsHud {
    pub fn new(title: String, icon: String, best: u32, unlocked: [bool; 3]) -> Self {
        Self {
            title,
            icon,
            best_before: best,
            unlocked_before: unlocked,
            t: 0,
            pops: Vec::new(),
            drawing: u32::MAX,
            phase: None,
            phase_t: 0,
            flash: 0.0,
            confirmed: None,
            gems_found: 0,
            gem_at: None,
            gem_text: None,
        }
    }

    pub fn after_step(&mut self, w: &World) {
        let Some(t) = w.trials.as_deref() else { return };
        self.t += 1;
        if self.pops.len() != t.dummies_total as usize {
            self.pops = vec![None; t.dummies_total as usize];
            self.drawing = t.dummies_total;
        }
        while self.drawing > t.drawing {
            self.drawing -= 1;
            if let Some(p) = self.pops.get_mut(self.drawing as usize) {
                *p = Some(0);
            }
        }
        for p in self.pops.iter_mut().flatten() {
            *p += 1;
        }
        if self.phase != Some(t.phase) {
            if t.phase == TrialsPhase::Won {
                self.flash = 1.0;
            }
            if self.phase == Some(TrialsPhase::Intro) {
                self.confirmed = None;
            }
            self.phase = Some(t.phase);
            self.phase_t = 0;
        }
        self.phase_t += 1;
        self.flash = (self.flash - 0.02).max(0.0);
        if t.phase == TrialsPhase::Intro
            && t.phase_t < 5.0
            && self.phase_t > 45
            && self.confirmed.is_none()
        {
            self.confirmed = Some(0);
        }
        if let Some(c) = &mut self.confirmed {
            *c += 1;
        }
        // The gem: "n / 7" floats where it was collected (`FloatText`:
        // scale 1, rate 0.5, no rise).
        let gem = w.entities().find_map(|e| match w.kind(e) {
            Some(tfp_sim::entity::Kind::Pickup(p)) if p.kind == tfp_sim::pickups::Pickups::Gem => {
                Some(w.core(e).pos)
            }
            _ => None,
        });
        if gem.is_some() {
            self.gem_at = gem;
        }
        if t.gem_collected && self.gem_text.is_none() {
            let at = self.gem_at.unwrap_or(tfp_sim::Vec2::new(160.0, 120.0));
            self.gem_text = Some((at, 0));
            self.gems_found += 1;
        }
        if let Some((_, k)) = &mut self.gem_text {
            *k += 1;
        }
    }

    /// The yellow gem's float text (alpha CubeOut of a counter falling
    /// 0.007 per tick; the Yellow archer's two colours every 10 ticks).
    fn gem_text(&self, p: &mut Painter, w: &World) {
        let Some((at, k)) = self.gem_text else { return };
        let counter = 1.0 - 0.007 * k as f32;
        if counter <= 0.0 {
            return;
        }
        let a = 1.0 - (1.0 - counter).powi(3);
        let _ = w;
        // Archer 5 (Yellow): `ArcherData.Archers[5]`.
        let (ca, cb) =
            p.a.archers
                .get(5)
                .map_or((Color::WHITE, Color::WHITE), |y| {
                    (
                        crate::front::gfx::rgba(y.color_a),
                        crate::front::gfx::rgba(y.color_b),
                    )
                });
        let col = if (k / 10).is_multiple_of(2) { ca } else { cb };
        p.text_outlined(
            &format!("{} / 7", self.gems_found),
            Vec2::new(at.x.floor(), at.y.floor()),
            (0.5, 0.5),
            1.0,
            col * a,
        );
    }

    pub fn draw(&self, p: &mut Painter, w: &World) {
        let Some(t) = w.trials.as_deref() else { return };
        self.gem_text(p, w);
        let width = p.w;
        let cx = width / 2.0;
        match t.phase {
            TrialsPhase::Intro => {
                let out = self
                    .confirmed
                    .map_or(0.0, |c| cube_in((c as f32 / 30.0).min(1.0)));
                if out < 1.0 && t.settings.start_screen {
                    p.rect(0.0, 0.0, width, 240.0, Color::BLACK * (0.5 * (1.0 - out)));
                    let k = ((self.phase_t as f32 - 21.0) / 10.0).clamp(0.0, 1.0);
                    if self.phase_t >= 21 {
                        let scale = 6.0 - 4.0 * cube_in(k);
                        let y = 60.0 - 300.0 * out;
                        if let Some(icon) = p.a.m(&self.icon) {
                            p.tex_ex(
                                icon,
                                Vec2::new(cx, y),
                                Org::Center,
                                Vec2::splat(scale),
                                0.15 * (1.0 - k),
                                Color::WHITE,
                                false,
                            );
                        }
                        let a = cube_out(((self.phase_t as f32 - 21.0) / 60.0).min(1.0));
                        p.text_outlined(
                            &self.title,
                            Vec2::new(cx, 105.0 + 300.0 * out),
                            (0.5, 0.5),
                            2.0,
                            Color::WHITE * a,
                        );
                    }
                    if self.phase_t >= 41 {
                        let s = cube_out(((self.phase_t - 41) as f32 / 20.0).min(1.0));
                        self.board(
                            p,
                            t,
                            Vec2::new(cx - 320.0 * (1.0 - s) - 320.0 * out, 180.0),
                            false,
                            0,
                        );
                    }
                }
                self.hud(p, t, 0.0);
            }
            TrialsPhase::Playing => self.hud(p, t, 0.0),
            TrialsPhase::Won | TrialsPhase::Failed => {
                // HUD slides out near the end of the sequence.
                let end = if t.phase == TrialsPhase::Won {
                    41 + 7 * t.dummies_total + 31
                } else {
                    121
                };
                let k = cube_in(((self.phase_t.saturating_sub(end)) as f32 / 20.0).min(1.0));
                self.hud(p, t, k);
            }
            TrialsPhase::Results | TrialsPhase::Finished => self.results(p, t),
        }
        if self.flash > 0.0 {
            p.rect(0.0, 0.0, width, 240.0, Color::WHITE * self.flash);
        }
    }

    /// The clock, next goal and dummy icons; `out` slides them away.
    fn hud(&self, p: &mut Painter, t: &tfp_sim::trials::Trials, out: f32) {
        let width = p.w;
        let tin = cube_out((self.t as f32 / 20.0).min(1.0));
        let x = -160.0 + 165.0 * tin - 165.0 * out;
        p.text_outlined(
            &time_string(t.time),
            Vec2::new(x, 5.0),
            (0.0, 0.0),
            2.0,
            Color::WHITE,
        );
        let s = &t.settings;
        let (icon, goal) = if !self.unlocked_before[0] {
            (Some("trials/goldSmall"), Some(s.gold))
        } else if !self.unlocked_before[1] {
            (Some("trials/diamondSmall"), Some(s.diamond))
        } else {
            (None, None)
        };
        match (icon, goal) {
            (Some(i), Some(g)) => {
                p.m(i, Vec2::new(x + 5.0, 27.0), Org::Center, 1.0, Color::WHITE);
                p.text_outlined(
                    &time_string(ticks(g)),
                    Vec2::new(x + 12.0, 27.0),
                    (0.0, 0.5),
                    1.0,
                    Color::WHITE,
                );
            }
            _ => {
                p.text_outlined(
                    &format!("BEST: {}", time_string(self.best_before)),
                    Vec2::new(x, 27.0),
                    (0.0, 0.5),
                    1.0,
                    Color::WHITE,
                );
            }
        }
        // Dummy icons at the right edge (cell 0 standing, cell 1 hit).
        let Some(sheet) = p.a.g("dummy/hitCounter") else {
            return;
        };
        for (i, pop) in self.pops.iter().enumerate() {
            let xi = width - 12.0 - 20.0 * i as f32;
            let slide = 210.0 * (1.0 - tin) + 210.0 * out;
            let (cell, scale, rot) = match pop {
                Some(pt) => {
                    let k = back_out((*pt as f32 / 40.0).min(1.0));
                    (1, 1.4 - 0.4 * k, std::f32::consts::TAU * k)
                }
                None => (0, 1.0, 0.0),
            };
            let sub = sheet.sub(28 * cell, 0, 28, 28);
            p.tex_ex(
                sub,
                Vec2::new(xi + slide, 12.0),
                Org::Center,
                Vec2::splat(scale),
                rot,
                Color::WHITE,
                false,
            );
        }
    }

    /// `TrialsTimes` at `at` (trials.md 7.4). `anim` animates new unlocks
    /// and the new best from tick `t0` of the results screen.
    fn board(&self, p: &mut Painter, t: &tfp_sim::trials::Trials, at: Vec2, anim: bool, t0: u32) {
        let s = &t.settings;
        let best_old = self.best_before;
        let result = t.result.unwrap_or_default();
        let x = if best_old > 0 || (anim && result.new_best) {
            -60.0
        } else {
            0.0
        };
        let rows = [
            ("trials/gold", s.gold, 0usize, -15.0),
            ("trials/diamond", s.diamond, 1, 15.0),
        ];
        let medal_at = |i: usize| 70 + 30 * i as u32;
        for (k, (icon, secs, i, dy)) in rows.iter().enumerate() {
            let row_t = if anim {
                t0.saturating_sub(10 * k as u32)
            } else {
                99
            };
            let slide = cube_out((row_t as f32 / 20.0).min(1.0));
            let pos = at + Vec2::new(x - 200.0 * (1.0 - slide), *dy);
            p.m(
                icon,
                pos + Vec2::new(-24.0, 0.0),
                Org::Center,
                1.0,
                Color::WHITE,
            );
            p.text_outlined(
                &time_string(ticks(*secs)),
                pos + Vec2::new(-10.0, 0.0),
                (0.0, 0.5),
                2.0,
                Color::WHITE,
            );
            let had = self.unlocked_before[*i];
            let new = anim && result.unlocked[*i];
            if had || (new && t0 >= medal_at(*i)) {
                let pk = if new {
                    ((t0 - medal_at(*i)) as f32 / 10.0).min(1.0)
                } else {
                    1.0
                };
                p.m(
                    "trials/check",
                    pos + Vec2::new(-24.0, 0.0),
                    Org::Center,
                    2.0 - pk,
                    Color::WHITE * pk,
                );
            }
        }
        let dev_had = self.unlocked_before[2];
        let dev_new = anim && result.unlocked[2];
        if dev_had || (dev_new && t0 >= medal_at(2)) {
            let a = if dev_new {
                ((t0 - medal_at(2)) as f32 / 60.0).min(1.0)
            } else {
                1.0
            };
            p.m(
                "trials/redDiamond",
                at + Vec2::new(60.0, 15.0),
                Org::Center,
                1.0,
                Color::WHITE * a,
            );
        }
        if best_old > 0 || (anim && result.new_best) {
            let pos = at + Vec2::new(60.0, 0.0);
            p.text_outlined(
                "BEST:",
                pos + Vec2::new(0.0, -14.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            );
            let switched = anim && result.new_best && t0 >= 60;
            let (txt, col) = if switched {
                (time_string(result.time), NEW_BEST)
            } else if best_old > 0 {
                (time_string(best_old), Color::WHITE)
            } else {
                ("---".to_owned(), Color::WHITE)
            };
            let wig = if switched && t0 < 90 {
                ((t0 - 60) as f32 * 0.8).sin() * 2.0
            } else {
                0.0
            };
            p.text_outlined(&txt, pos + Vec2::new(wig, 2.0), (0.5, 0.5), 3.0, col);
        }
    }

    fn results(&self, p: &mut Painter, t: &tfp_sim::trials::Trials) {
        let width = p.w;
        let cx = width / 2.0;
        p.rect(0.0, 0.0, width, 240.0, Color::BLACK * 0.5);
        let rt = self.phase_t;
        let result = t.result.unwrap_or_default();
        if result.time > 0 {
            let k = back_out((rt as f32 / 20.0).min(1.0));
            p.text_outlined(
                "COMPLETE!",
                Vec2::new(cx, 30.0),
                (0.5, 0.5),
                (3.0 * k).max(0.1),
                COMPLETE,
            );
            if rt >= 40 {
                let kx = cube_out(((rt - 40) as f32 / 10.0).min(1.0));
                let x = 400.0 + (cx - 400.0) * kx;
                p.text_outlined(
                    &time_string(result.time),
                    Vec2::new(x, 70.0),
                    (0.5, 0.5),
                    3.0,
                    Color::WHITE,
                );
            }
            if rt >= 50 {
                self.board(p, t, Vec2::new(cx, 140.0), true, rt - 50);
            }
        } else {
            let k = back_out((rt.saturating_sub(20) as f32 / 20.0).min(1.0));
            if rt >= 20 {
                let tilt = if rt >= 50 {
                    0.17 * back_out(((rt - 50) as f32 / 60.0).min(1.0))
                } else {
                    0.0
                };
                let _ = tilt;
                p.text_outlined(
                    "FAILURE",
                    Vec2::new(cx, 40.0),
                    (0.5, 0.5),
                    (3.0 * k).max(0.1),
                    Color::hex(0xFF3030),
                );
            }
            if rt >= 40 {
                self.board(p, t, Vec2::new(cx, 130.0), false, 0);
            }
        }
    }
}
