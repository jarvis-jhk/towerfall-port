//! Tower (map) select for Versus (ui.md 9).

use tfp_render::glam::{Affine2, Vec2};
use tfp_render::{Color, SubTexture};

use super::assets::UiAssets;
use super::gfx::{Org, Painter, cube_out, lerp, sine};
use super::minput::b;
use super::settings::{FrontEndSetup, TowerChoice};
use super::{Ctx, Go, Screen, ScreenId, Session, View};

const MAP_W: f32 = 472.0;
const MAP_H: f32 = 480.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Select,
    /// RANDOM: hopping toward the pick (target index, frames).
    Hop(usize, u32),
    /// Entering the tower: frames.
    Zoom(u32),
}

pub struct MapScreen {
    /// 0 = RANDOM, 1.. = towers.
    sel: usize,
    strip_x: Vec<f32>,
    cam: (Vec2, Vec2, u64),
    phase: Phase,
    seed: Option<SeedEntry>,
    entry_t: u32,
    picked_random: bool,
    seed_value: Option<u32>,
}

struct SeedEntry {
    digits: [u8; 9],
    cursor: usize,
}

impl MapScreen {
    pub fn new(s: &mut Session, a: &UiAssets) -> Self {
        let sel = if a.towers.is_empty() || s.versus.tower.random {
            0
        } else {
            (s.versus.tower.tower + 1).min(a.towers.len())
        };
        let n = a.towers.len() + 1;
        let w = s.width();
        let mut m = Self {
            sel,
            strip_x: (0..n)
                .map(|i| w / 2.0 + (i as f32 - sel as f32) * 40.0)
                .collect(),
            cam: (Vec2::ZERO, Vec2::ZERO, 0),
            phase: Phase::Select,
            seed: None,
            entry_t: 0,
            picked_random: false,
            seed_value: None,
        };
        let c = m.target(a, w);
        m.cam = (c, c, 0);
        m
    }

    fn clamp_cam(p: Vec2, w: f32) -> Vec2 {
        Vec2::new(
            p.x.clamp(w / 2.0, MAP_W - w / 2.0),
            p.y.clamp(120.0, MAP_H - 120.0),
        )
    }

    fn target(&self, a: &UiAssets, w: f32) -> Vec2 {
        match self.sel {
            0 => Vec2::new(MAP_W / 2.0, 120.0),
            i => {
                let t = &a.towers[i - 1];
                Self::clamp_cam(Vec2::new(t.map_pos.0, t.map_pos.1), w)
            }
        }
    }

    fn cam_at(&self, tick: u64) -> Vec2 {
        let (from, to, t) = self.cam;
        lerp(from, to, cube_out(tick.saturating_sub(t) as f32 / 30.0))
    }

    fn locked(a: &UiAssets, i: usize) -> bool {
        i > 0 && a.towers[i - 1].dark_world && !a.dark_world
    }

    fn select(&mut self, c: &mut Ctx, n: usize) {
        self.sel = n;
        let snd = match n {
            0 => "ui_moveRandMap",
            i if c.a.towers[i - 1].procedural => "ui_mapCataclysm",
            i if c.a.towers[i - 1].dark_world => "ui_mapDark",
            12 => "ui_clickSpecialAsc",
            9..=11 => "ui_clickSpecial",
            _ => "ui_move2",
        };
        c.play(snd, None);
        let now = self.cam_at(c.tick);
        self.cam = (now, self.target(c.a, c.w()), c.tick);
    }

    fn pick_random(&self, c: &mut Ctx) -> usize {
        let n = c.a.towers.len();
        let mut cands: Vec<usize> = (0..n)
            .filter(|&i| {
                !Self::locked(c.a, i + 1) && !c.s.no_random.get(i).copied().unwrap_or(false)
            })
            .collect();
        if cands.is_empty() {
            c.s.no_random.iter_mut().for_each(|f| *f = false);
            cands = (0..n).filter(|&i| !Self::locked(c.a, i + 1)).collect();
        }
        if cands.len() > 1
            && let Some(last) = c.s.last_random
        {
            cands.retain(|&i| i != last);
        }
        let r = c.s.rand() as usize;
        cands.get(r % cands.len().max(1)).copied().unwrap_or(0)
    }

    fn confirm(&mut self, c: &mut Ctx) {
        let i = self.sel - 1;
        c.s.versus.tower = TowerChoice {
            tower: i,
            random: self.picked_random,
            seed: if c.a.towers[i].procedural {
                self.seed_value
                    .unwrap_or((c.s.rand() % 1_000_000_000) as u32)
            } else {
                0
            },
        };
        if self.picked_random {
            c.s.last_random = Some(i);
        }
        c.s.setup = Some(FrontEndSetup {
            slots: c.s.slots.clone(),
            settings: c.s.versus.clone(),
            tower_name: c.a.towers[i].name.clone(),
        });
        c.stop_music();
        c.play("ui_mapZoom", None);
        self.phase = Phase::Zoom(0);
    }
}

impl Screen for MapScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Map
    }

    fn menu_background(&self) -> bool {
        false
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        self.entry_t += 1;
        let w = c.w();
        // Strip motion: 6 px per frame toward the target (12 early on).
        let speed = if self.entry_t < 20 { 12.0 } else { 6.0 };
        for (i, x) in self.strip_x.iter_mut().enumerate() {
            let t = w / 2.0 + (i as f32 - self.sel as f32) * 40.0;
            *x += (t - *x).clamp(-speed, speed);
        }
        match &mut self.phase {
            Phase::Zoom(t) => {
                *t += 1;
                if *t >= 80 {
                    return Some(Go::To(ScreenId::MatchReady));
                }
                return None;
            }
            Phase::Hop(target, t) => {
                *t += 1;
                let target = *target;
                if self.sel != target {
                    if (*t).is_multiple_of(6) {
                        let n = (self.sel % (self.strip_x.len() - 1)) + 1;
                        self.select(c, n);
                    }
                } else if *t > 30 {
                    self.picked_random = true;
                    self.confirm(c);
                }
                return None;
            }
            Phase::Select => {}
        }
        if !c.ready || c.a.towers.is_empty() {
            if c.ready && c.mi.p(b::BACK) {
                return Some(Go::To(ScreenId::VersusOptions));
            }
            return None;
        }
        // Seed entry.
        if let Some(se) = &mut self.seed {
            if c.mi.p(b::BACK) {
                self.seed = None;
                c.play("ui_clickBack", None);
            } else if c.mi.p(b::LEFT) {
                se.cursor = (se.cursor + 8) % 9;
                c.play("ui_move2", None);
            } else if c.mi.p(b::RIGHT) {
                se.cursor = (se.cursor + 1) % 9;
                c.play("ui_move2", None);
            } else if c.mi.p(b::UP) {
                se.digits[se.cursor] = (se.digits[se.cursor] + 1) % 10;
                c.play("ui_seedSfx", None);
            } else if c.mi.p(b::DOWN) {
                se.digits[se.cursor] = (se.digits[se.cursor] + 9) % 10;
                c.play("ui_seedSfx", None);
            } else if c.mi.p(b::CONFIRM) || c.mi.p(b::START) {
                let v = se.digits.iter().fold(0u32, |a, &d| a * 10 + d as u32);
                self.seed_value = Some(v);
                self.seed = None;
                self.picked_random = false;
                self.confirm(c);
            }
            return None;
        }
        if c.mi.p(b::BACK) {
            c.play("ui_clickBack", None);
            return Some(Go::To(ScreenId::VersusOptions));
        }
        // Scroll mode while Alt is held.
        if c.mi.h(b::ALT) {
            let mut d = Vec2::ZERO;
            for (bit, v) in [
                (b::LEFT, Vec2::NEG_X),
                (b::RIGHT, Vec2::X),
                (b::UP, Vec2::NEG_Y),
                (b::DOWN, Vec2::Y),
            ] {
                if c.mi.h(bit) {
                    d += v;
                }
            }
            let now = self.cam_at(c.tick);
            let to = Self::clamp_cam(now + d.normalize_or_zero() * 3.0, w);
            self.cam = (to, to, 0);
            return None;
        }
        let n = self.strip_x.len();
        if c.mi.p(b::LEFT) && self.sel > 0 {
            self.select(c, self.sel - 1);
        } else if c.mi.p(b::RIGHT) && self.sel + 1 < n {
            self.select(c, self.sel + 1);
        } else if c.mi.p(b::DOWN) && self.sel > 0 && c.a.towers[self.sel - 1].procedural {
            self.seed = Some(SeedEntry {
                digits: [0; 9],
                cursor: 0,
            });
            c.play("ui_click", None);
        } else if c.mi.p(b::ALT2) {
            if self.sel == 0 {
                c.s.no_random.iter_mut().for_each(|f| *f = false);
                c.play("ui_subclickOn", None);
            } else if let Some(f) = c.s.no_random.get_mut(self.sel - 1) {
                *f = !*f;
                let on = *f;
                c.play(
                    if on {
                        "ui_subclickOff"
                    } else {
                        "ui_subclickOn"
                    },
                    None,
                );
            }
        } else if (c.mi.p(b::CONFIRM) || c.mi.p(b::START)) && self.entry_t > 20 {
            if Self::locked(c.a, self.sel) {
                c.play("ui_levelLock", None);
                return None;
            }
            c.play("ui_click", None);
            if self.sel == 0 {
                let pick = self.pick_random(c) + 1;
                c.stop_music();
                self.phase = Phase::Hop(pick, 0);
            } else {
                self.picked_random = false;
                self.seed_value = None;
                self.confirm(c);
            }
        }
        None
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let a = p.a;
        let w = v.w;
        let cam = self.cam_at(v.tick);
        let (zoom, fade) = match self.phase {
            Phase::Zoom(t) => {
                let e = cube_out((t as f32 / 60.0).min(1.0));
                (1.0 + 2.0 * e, (t as f32 / 60.0).min(1.0))
            }
            _ => (1.0, 0.0),
        };
        let focus = if let Phase::Zoom(_) = self.phase
            && self.sel > 0
        {
            let t = &a.towers[self.sel - 1];
            lerp(cam, Vec2::new(t.map_pos.0, t.map_pos.1), fade)
        } else {
            cam
        };
        let saved = p.list.transform();
        let xf = Affine2::from_translation(Vec2::new(w / 2.0, 120.0))
            * Affine2::from_scale(Vec2::splat(zoom))
            * Affine2::from_translation(-focus.round());
        p.list.set_transform(xf);
        // Water (sine-sliced, offset (5, 0)) then the land.
        p.rect(
            -200.0,
            -200.0,
            MAP_W + 400.0,
            MAP_H + 400.0,
            Color::rgb(24, 52, 112),
        );
        if let Some(water) = a.m("mapWater") {
            p.sine_texture_v(
                water,
                Vec2::new(-5.0, 0.0),
                v.tick as f32 * 0.03,
                2.0,
                2,
                std::f32::consts::PI / 8.0,
            );
        }
        p.m("mapLand", Vec2::ZERO, Org::TopLeft, 1.0, Color::WHITE);
        // Cursor over the selected tower.
        if self.sel > 0 && !matches!(self.phase, Phase::Zoom(_)) {
            let t = &a.towers[self.sel - 1];
            let bob = 5.0 * sine(v.tick, 120.0, 0.0);
            p.m(
                "mapCursor",
                Vec2::new(t.map_pos.0, t.map_pos.1 + bob),
                Org::Just(0.5, 1.0),
                1.0,
                Color::WHITE,
            );
        }
        p.list.set_transform(saved);

        // Strip of blocks.
        let strip_k = v.k
            * if matches!(self.phase, Phase::Zoom(_)) {
                0.0
            } else {
                1.0
            };
        let y = lerp(Vec2::splat(300.0), Vec2::splat(212.0), strip_k).x;
        let n = self.strip_x.len();
        let draw_block = |p: &mut Painter, i: usize, scale: f32| {
            let x = self.strip_x[i];
            let pos = Vec2::new(x, y);
            let (block, icon, tint) = if i == 0 {
                (
                    "randomLevelBlock",
                    "towerIcons/random".to_string(),
                    Color::WHITE,
                )
            } else {
                let t = &a.towers[i - 1];
                let tint = if Self::locked(a, i) {
                    Color::WHITE * 0.67
                } else {
                    Color::WHITE
                };
                (t.block, t.icon.clone(), tint)
            };
            p.m(block, pos, Org::Center, scale, tint);
            p.m(&icon, pos, Org::Center, scale, tint);
            if Self::locked(a, i) {
                p.m(
                    "map/smallLock",
                    pos + Vec2::new(0.0, 4.0 * scale),
                    Org::Center,
                    scale,
                    Color::WHITE,
                );
            }
            if i > 0 && v.s.no_random.get(i - 1).copied().unwrap_or(false) {
                p.m(
                    "towerIcons/noRandom",
                    pos + Vec2::new(12.0, 12.0) * scale,
                    Org::Center,
                    1.0,
                    Color::WHITE,
                );
            }
        };
        for i in 0..n {
            if i != self.sel && (self.strip_x[i] > -40.0 && self.strip_x[i] < w + 40.0) {
                draw_block(p, i, 1.0);
            }
        }
        if n > 0 {
            draw_block(p, self.sel, 1.5);
        }
        // Title.
        let ty = lerp(Vec2::splat(-18.0), Vec2::splat(12.0), strip_k).x;
        let title = if self.sel == 0 {
            "RANDOM".to_string()
        } else {
            a.towers
                .get(self.sel - 1)
                .map_or(String::new(), |t| t.name.clone())
        };
        p.text_outlined(
            &title,
            Vec2::new(w / 2.0, ty),
            (0.5, 0.5),
            2.0,
            Color::WHITE,
        );
        if a.towers.is_empty() {
            p.text_outlined(
                "NO TOWERS FOUND (SET TF_GAME_DIR)",
                Vec2::new(w / 2.0, 120.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            );
        }
        // Seed window.
        if let Some(se) = &self.seed {
            let anchor = Vec2::new(w / 2.0, 185.0);
            p.m("seedWindow", anchor, Org::Just(0.5, 1.0), 1.0, Color::WHITE);
            let digits: Option<SubTexture> = a.m("seedDigits");
            for (k, &d) in se.digits.iter().enumerate() {
                let pos = anchor + Vec2::new(-56.0 + 14.0 * k as f32, -20.0);
                match digits {
                    Some(sd) => p.tex(
                        sd.sub(d as i32 * 12, 0, 12, 10),
                        pos,
                        Org::Center,
                        1.0,
                        Color::WHITE,
                    ),
                    None => p.text(&d.to_string(), pos, (0.5, 0.5), 1.0, Color::WHITE),
                }
                if k == se.cursor
                    && let Some(ar) = a.m("seedArrow")
                {
                    p.tex(
                        ar,
                        pos + Vec2::new(0.0, -10.0),
                        Org::Center,
                        1.0,
                        Color::WHITE,
                    );
                    p.tex_ex(
                        ar,
                        pos + Vec2::new(0.0, 10.0),
                        Org::Center,
                        Vec2::new(1.0, -1.0),
                        0.0,
                        Color::WHITE,
                        false,
                    );
                }
            }
        }
        // Entry fade-in and zoom fade-out.
        let entry = (1.0 - self.entry_t as f32 * 0.2).max(0.0);
        // SineIn of the zoom progress.
        let zoom_fade = if matches!(self.phase, Phase::Zoom(_)) {
            1.0 - (fade * std::f32::consts::FRAC_PI_2).cos()
        } else {
            0.0
        };
        let f = entry.max(zoom_fade);
        let f = if let Phase::Zoom(t) = self.phase {
            if t >= 60 { 1.0 } else { f }
        } else {
            f
        };
        if f > 0.0 {
            p.rect(0.0, 0.0, w, 240.0, Color::BLACK * f);
        }
    }
}
