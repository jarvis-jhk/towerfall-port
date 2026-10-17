//! Dark World tower select (ui.md 9.2): one block per Dark World tower over
//! the world map with skull / eye badges and the session stats panel
//! (`DarkWorldLevelSelectOverlay`), then the difficulty overlay (NORMAL /
//! HARDCORE / LEGENDARY, `DarkWorldDifficultySelect`) and the variants
//! overlay (power-ups and curses with the per-difficulty power-up cap,
//! BEGIN!, `DarkWorldVariantsSelect`, darkworld.md 5.6), then the zoom
//! into the tower and a `CoopLaunch` for the game scene.

use tfp_render::Color;
use tfp_render::glam::Vec2;
use tfp_sim::darkworld::{POWER_UPS, power_up_cap};
use tfp_sim::variants::{Variant, VariantSet};

use super::{
    curses_for, difficulty_color, difficulty_name, power_ups_on, shield_image, variant_ui,
};
use crate::coop_ui::menus::{MAP_W, clamp_cam, draw_map};
use crate::coop_ui::{CoopLaunch, CoopMode, players_from_slots};
use crate::front::assets::UiAssets;
use crate::front::gfx::{Org, Painter, cube_in, cube_out, lerp, pal, sine};
use crate::front::minput::b;
use crate::front::{Ctx, Go, Screen, ScreenId, Session, View};

const POWER_COLOR: Color = Color::hex(0x8AFF47);
const CURSE_COLOR: Color = Color::hex(0xD589FF);
const DARK_TINT: Color = Color::hex(0x8261B8);
const ALL_SLOTS: u16 = 0x0fff;

/// A Dark World tower on the map.
#[derive(Clone, Debug)]
pub struct DwTowerInfo {
    pub name: String,
    pub icon: String,
    pub block: &'static str,
    pub dark_tint: bool,
    pub map_pos: (f32, f32),
}

/// Theme name, icon, block and map position of every Dark World tower.
pub fn load_towers(inst: &tfp_assets::GameInstall) -> Vec<DwTowerInfo> {
    let (Ok(index), Ok(gd)) = (
        tfp_assets::towers::TowerIndex::build(inst),
        tfp_assets::game_data::GameData::load(&inst.content),
    ) else {
        return Vec::new();
    };
    index
        .dark_world
        .iter()
        .map(|t| {
            let th = gd.themes.iter().find(|x| x.id == t.theme);
            let cata = th.is_some_and(|x| x.tower_type.eq_ignore_ascii_case("Cataclysm"));
            DwTowerInfo {
                name: th.map_or(t.dir_name.to_uppercase(), |x| x.name.to_uppercase()),
                icon: th.map_or(String::new(), |x| format!("towerIcons/{}", x.icon)),
                block: if cata {
                    "vortexLevelBlock"
                } else {
                    "darkLevelBlock"
                },
                dark_tint: !cata,
                map_pos: th.map_or((240.0, 240.0), |x| x.map_position),
            }
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Select,
    /// Difficulty overlay: ticks since it opened; leaving (back) ticks.
    Difficulty {
        t: u32,
        out: Option<u32>,
    },
    /// Variants overlay: ticks since it opened, leaving ticks.
    Variants {
        t: u32,
        out: Option<u32>,
    },
    Zoom(u32),
}

/// A button of the variants overlay.
#[derive(Clone, Copy, PartialEq, Debug)]
enum VSel {
    Power(usize),
    Curse(usize),
    Begin,
}

pub struct DwMap {
    towers: Vec<DwTowerInfo>,
    sel: usize,
    difficulty: u8,
    /// Smoothed difficulty highlight (0..2).
    diff_lerp: f32,
    variants: VariantSet,
    vsel: VSel,
    /// Last side the BEGIN! button returns to (power-ups / curses).
    begin_up: VSel,
    shake: u32,
    players: usize,
    strip_x: Vec<f32>,
    cam: (Vec2, Vec2, u64),
    phase: Phase,
    entry_t: u32,
    stats_t: f32,
    /// Selection wiggles (difficulty and variant buttons): (which, ticks).
    wiggle: Option<(usize, u32)>,
}

impl DwMap {
    pub fn new(s: &mut Session, a: &UiAssets) -> Self {
        let towers = a.install.as_ref().map(load_towers).unwrap_or_default();
        let sel = s.coop.dw_tower.min(towers.len().saturating_sub(1));
        let players = s.joined_count().clamp(1, 4);
        let mut m = Self {
            towers,
            sel,
            difficulty: s.coop.dw_difficulty.min(2),
            diff_lerp: s.coop.dw_difficulty.min(2) as f32,
            variants: s.coop.dw_variants,
            vsel: VSel::Begin,
            begin_up: VSel::Power(5),
            shake: 0,
            players,
            strip_x: Vec::new(),
            cam: (Vec2::ZERO, Vec2::ZERO, 0),
            phase: Phase::Select,
            entry_t: 0,
            stats_t: 0.0,
            wiggle: None,
        };
        let w = s.width();
        m.strip_x = (0..m.towers.len()).map(|i| m.col_x(i, w)).collect();
        let c = m.target(w);
        m.cam = (c, c, 0);
        m
    }

    fn col_x(&self, i: usize, w: f32) -> f32 {
        w / 2.0 + (i as f32 - self.sel as f32) * 40.0
    }

    fn target(&self, w: f32) -> Vec2 {
        match self.towers.get(self.sel) {
            Some(t) => clamp_cam(Vec2::new(t.map_pos.0, t.map_pos.1), w),
            None => Vec2::new(MAP_W / 2.0, 120.0),
        }
    }

    fn cam_at(&self, tick: u64) -> Vec2 {
        let (from, to, t) = self.cam;
        lerp(from, to, cube_out(tick.saturating_sub(t) as f32 / 30.0))
    }

    fn curses(&self) -> [Variant; 6] {
        curses_for(self.players)
    }

    fn cap(&self) -> usize {
        power_up_cap(self.difficulty)
    }

    /// Power-up / curse buttons: positions (ui.md: 3 x 2 grids, 24 px).
    fn button_pos(sel: VSel, dx: f32) -> Vec2 {
        match sel {
            VSel::Power(i) => Vec2::new(
                75.0 + dx + 24.0 * (i % 3) as f32,
                70.0 + 24.0 * (i / 3) as f32,
            ),
            VSel::Curse(i) => Vec2::new(
                197.0 + dx + 24.0 * (i % 3) as f32,
                70.0 + 24.0 * (i / 3) as f32,
            ),
            VSel::Begin => Vec2::new(160.0 + dx, 155.0),
        }
    }

    fn variant_of(&self, sel: VSel) -> Option<Variant> {
        match sel {
            VSel::Power(i) => POWER_UPS.get(i).copied(),
            VSel::Curse(i) => self.curses().get(i).copied(),
            VSel::Begin => None,
        }
    }

    /// Moves inside the variants overlay (`LinkButtonArray` + the two
    /// cross links and BEGIN!).
    fn nav(&self, sel: VSel, dir: (i32, i32)) -> Option<VSel> {
        use VSel::*;
        match (sel, dir) {
            (Power(i), (1, 0)) if i % 3 < 2 => Some(Power(i + 1)),
            (Power(2), (1, 0)) => Some(Curse(0)),
            (Power(5), (1, 0)) => Some(Curse(3)),
            (Power(i), (-1, 0)) if i % 3 > 0 => Some(Power(i - 1)),
            (Curse(i), (-1, 0)) if i % 3 > 0 => Some(Curse(i - 1)),
            (Curse(0), (-1, 0)) => Some(Power(2)),
            (Curse(3), (-1, 0)) => Some(Power(5)),
            (Curse(i), (1, 0)) if i % 3 < 2 => Some(Curse(i + 1)),
            (Power(i), (0, 1)) if i < 3 => Some(Power(i + 3)),
            (Curse(i), (0, 1)) if i < 3 => Some(Curse(i + 3)),
            (Power(_) | Curse(_), (0, 1)) => Some(Begin),
            (Power(i), (0, -1)) if i >= 3 => Some(Power(i - 3)),
            (Curse(i), (0, -1)) if i >= 3 => Some(Curse(i - 3)),
            (Begin, (0, -1)) => Some(self.begin_up),
            _ => None,
        }
    }

    fn can_toggle(&self, v: Variant) -> bool {
        if !POWER_UPS.contains(&v) {
            return true;
        }
        self.variants.any(v) || power_ups_on(&self.variants) < self.cap()
    }

    fn launch(&self, c: &mut Ctx) -> CoopLaunch {
        let players = players_from_slots(&c.s.slots, 4);
        let mask = players.iter().fold(0u16, |m, p| m | 1 << p.slot);
        let mut v = self.variants;
        v.clean(mask);
        // Curses off the list for this player count go.
        let other = if self.players <= 1 {
            Variant::ExplodingCorpses
        } else {
            Variant::NoTreasure
        };
        v.set_all(other, false, mask);
        tfp_sim::darkworld::enforce_variants(&mut v, self.difficulty, players.len(), mask);
        CoopLaunch {
            mode: CoopMode::DarkWorld {
                tower: self.sel,
                difficulty: self.difficulty,
                variants: v,
            },
            players,
        }
    }

    fn store(&self, s: &mut Session) {
        s.coop.dw_tower = self.sel;
        s.coop.dw_difficulty = self.difficulty;
        s.coop.dw_variants = self.variants;
    }

    fn update_variants(&mut self, c: &mut Ctx) -> Option<Phase> {
        if c.mi.p(b::BACK) {
            c.play("ui_clickBack", None);
            return Some(Phase::Variants {
                t: 99,
                out: Some(0),
            });
        }
        if c.mi.p(b::START) || (c.mi.p(b::CONFIRM) && self.vsel == VSel::Begin) {
            c.play("ui_startQuest", None);
            self.store(c.s);
            c.stop_music();
            return Some(Phase::Zoom(0));
        }
        if c.mi.p(b::CONFIRM)
            && let Some(v) = self.variant_of(self.vsel)
        {
            if self.can_toggle(v) {
                let on = !self.variants.any(v);
                self.variants.set_all(v, on, ALL_SLOTS);
                c.play(
                    if on {
                        "ui_subclickOn"
                    } else {
                        "ui_subclickOff"
                    },
                    None,
                );
                self.wiggle = Some((10 + self.vsel_index(), 0));
            } else {
                c.play("ui_invalid", None);
                self.shake = 30;
            }
            self.store(c.s);
            return None;
        }
        let dirs = [
            (b::LEFT, (-1, 0)),
            (b::RIGHT, (1, 0)),
            (b::UP, (0, -1)),
            (b::DOWN, (0, 1)),
        ];
        for (bit, d) in dirs {
            if c.mi.p(bit)
                && let Some(n) = self.nav(self.vsel, d)
            {
                c.play("ui_move1", None);
                self.vsel = n;
                match n {
                    VSel::Power(_) => self.begin_up = VSel::Power(5),
                    VSel::Curse(_) => self.begin_up = VSel::Curse(3),
                    VSel::Begin => {}
                }
                break;
            }
        }
        None
    }

    fn vsel_index(&self) -> usize {
        match self.vsel {
            VSel::Power(i) => i,
            VSel::Curse(i) => 6 + i,
            VSel::Begin => 12,
        }
    }
}

impl Screen for DwMap {
    fn id(&self) -> ScreenId {
        ScreenId::DarkWorldMap
    }

    fn menu_background(&self) -> bool {
        false
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        self.entry_t += 1;
        self.shake = self.shake.saturating_sub(1);
        if let Some((_, t)) = &mut self.wiggle {
            *t += 1;
            if *t > 20 {
                self.wiggle = None;
            }
        }
        let w = c.w();
        let speed = if self.entry_t < 20 { 12.0 } else { 6.0 };
        for i in 0..self.strip_x.len() {
            let t = self.col_x(i, w);
            let x = &mut self.strip_x[i];
            *x += (t - *x).clamp(-speed, speed);
        }
        let d = self.difficulty as f32;
        self.diff_lerp += (d - self.diff_lerp).clamp(-0.2, 0.2);
        self.stats_t = (self.stats_t - 0.15).max(0.0);
        match &mut self.phase {
            Phase::Zoom(t) => {
                *t += 1;
                if *t == 80 {
                    let l = self.launch(c);
                    c.s.coop.pending = Some(l);
                }
                return None;
            }
            Phase::Difficulty { t, out } => {
                *t += 1;
                if let Some(o) = out {
                    *o += 1;
                    if *o >= 10 {
                        self.phase = Phase::Select;
                    }
                    return None;
                }
                if *t < 20 {
                    return None;
                }
                if c.mi.p(b::BACK) {
                    c.play("ui_clickBack", None);
                    *out = Some(0);
                } else if c.mi.p(b::RIGHT) && self.difficulty < 2 {
                    self.difficulty += 1;
                    self.wiggle = Some((self.difficulty as usize, 0));
                    c.play(
                        if self.difficulty == 2 {
                            "ui_diffSelectLegendary"
                        } else {
                            "ui_diffSelect2"
                        },
                        None,
                    );
                } else if c.mi.p(b::LEFT) && self.difficulty > 0 {
                    self.difficulty -= 1;
                    self.wiggle = Some((self.difficulty as usize, 0));
                    c.play(
                        if self.difficulty == 1 {
                            "ui_diffSelect2"
                        } else {
                            "ui_diffSelect1"
                        },
                        None,
                    );
                } else if c.mi.p(b::CONFIRM) || c.mi.p(b::START) {
                    c.play("ui_click", None);
                    // Power-ups over the new cap go (in variant order).
                    let mut on = 0;
                    for p in POWER_UPS {
                        if self.variants.any(p) {
                            on += 1;
                            if on > self.cap() {
                                self.variants.set_all(p, false, ALL_SLOTS);
                            }
                        }
                    }
                    self.store(c.s);
                    self.vsel = VSel::Begin;
                    self.phase = Phase::Variants { t: 0, out: None };
                }
                return None;
            }
            Phase::Variants { t, out } => {
                *t += 1;
                if let Some(o) = out {
                    *o += 1;
                    if *o >= 20 {
                        self.phase = Phase::Difficulty { t: 99, out: None };
                    }
                    return None;
                }
                if *t < 20 {
                    return None;
                }
                if let Some(p) = self.update_variants(c) {
                    self.phase = p;
                }
                return None;
            }
            Phase::Select => {}
        }
        if !c.ready {
            return None;
        }
        if c.mi.p(b::BACK) {
            c.play("ui_clickBack", None);
            return Some(Go::To(ScreenId::Rollcall));
        }
        if self.towers.is_empty() {
            return None;
        }
        let mut to = None;
        if c.mi.p(b::LEFT) && self.sel > 0 {
            to = Some(self.sel - 1);
        } else if c.mi.p(b::RIGHT) && self.sel + 1 < self.towers.len() {
            to = Some(self.sel + 1);
        } else if (c.mi.p(b::CONFIRM) || c.mi.p(b::START)) && self.entry_t > 20 {
            c.play("ui_click", None);
            self.store(c.s);
            self.phase = Phase::Difficulty { t: 0, out: None };
        }
        if let Some(n) = to {
            self.sel = n;
            c.play("ui_move2", None);
            let now = self.cam_at(c.tick);
            self.cam = (now, self.target(c.w()), c.tick);
            self.stats_t = 1.0;
        }
        None
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let w = v.w;
        let dx = (w - 320.0) / 2.0;
        let cam = self.cam_at(v.tick);
        let (zoom, fade) = match self.phase {
            Phase::Zoom(t) => {
                let e = cube_out((t as f32 / 60.0).min(1.0));
                (1.0 + 2.0 * e, (t as f32 / 60.0).min(1.0))
            }
            _ => (1.0, 0.0),
        };
        let focus = match (self.phase, self.towers.get(self.sel)) {
            (Phase::Zoom(_), Some(t)) => lerp(cam, Vec2::new(t.map_pos.0, t.map_pos.1), fade),
            _ => cam,
        };
        draw_map(p, focus, zoom, v.tick);
        let overlay = !matches!(self.phase, Phase::Select);
        if let Some(t) = self.towers.get(self.sel)
            && !overlay
        {
            let bob = 5.0 * sine(v.tick, 120.0, 0.0);
            let sp = Vec2::new(t.map_pos.0, t.map_pos.1) - cam + Vec2::new(w / 2.0, 120.0);
            p.m(
                "mapCursor",
                sp + Vec2::new(0.0, bob),
                Org::Just(0.5, 1.0),
                1.0,
                Color::WHITE,
            );
        }
        // The strip of blocks drops out while an overlay is open.
        let strip_out = match self.phase {
            Phase::Select => 0.0,
            Phase::Difficulty { t, out } => match out {
                Some(o) => 1.0 - cube_in(o as f32 / 10.0),
                None => cube_out(t as f32 / 20.0),
            },
            _ => 1.0,
        };
        let base_y = lerp(Vec2::splat(300.0), Vec2::splat(212.0), v.k).x + 100.0 * strip_out;
        let draw_block = |p: &mut Painter,
                          t: &DwTowerInfo,
                          pos: Vec2,
                          scale: f32,
                          rec: Option<&super::DwRecord>| {
            p.m(t.block, pos, Org::Center, scale, Color::WHITE);
            let tint = if t.dark_tint { DARK_TINT } else { Color::WHITE };
            p.m(&t.icon, pos, Org::Center, scale, tint);
            if let Some(bd) = rec.and_then(|r| r.badge()) {
                p.m(
                    bd,
                    pos + Vec2::new(9.0, 9.0) * scale,
                    Org::Center,
                    1.0,
                    Color::WHITE,
                );
            }
        };
        for (i, t) in self.towers.iter().enumerate() {
            let x = self.strip_x.get(i).copied().unwrap_or(w / 2.0);
            if i == self.sel || !(-40.0..w + 40.0).contains(&x) {
                continue;
            }
            draw_block(
                p,
                t,
                Vec2::new(x, base_y),
                1.0,
                v.s.coop.records.dark_world.get(&i),
            );
        }
        if let Some(t) = self.towers.get(self.sel) {
            let x = self.strip_x.get(self.sel).copied().unwrap_or(w / 2.0);
            // The block moves aside while the variants are open.
            let side = match self.phase {
                Phase::Variants { t, out } => {
                    let k = cube_out(t as f32 / 20.0);
                    out.map_or(k, |o| 1.0 - cube_out(o as f32 / 20.0))
                }
                Phase::Zoom(_) => 1.0,
                _ => 0.0,
            };
            let (bx, by) = if overlay {
                (x - 25.0 * side, 212.0)
            } else {
                (x, base_y)
            };
            if !matches!(self.phase, Phase::Zoom(_)) {
                draw_block(
                    p,
                    t,
                    Vec2::new(bx, by),
                    1.5,
                    v.s.coop.records.dark_world.get(&self.sel),
                );
            }
            let ty = lerp(Vec2::splat(-18.0), Vec2::splat(12.0), v.k).x;
            let tx = w / 2.0 - 320.0 * side;
            if !matches!(self.phase, Phase::Variants { .. } | Phase::Zoom(_)) || side < 1.0 {
                p.text_outlined(&t.name, Vec2::new(tx, ty), (0.5, 0.5), 2.0, Color::WHITE);
            }
        }
        if !overlay {
            self.stats(p, v, dx);
        }
        if self.towers.is_empty() {
            p.text_outlined(
                "NO DARK WORLD TOWERS (NEEDS THE EXPANSION)",
                Vec2::new(w / 2.0, 120.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            );
        }
        match self.phase {
            Phase::Difficulty { t, out } => {
                let k = match out {
                    Some(o) => 1.0 - cube_out(o as f32 / 10.0),
                    None => cube_out(t as f32 / 20.0),
                };
                self.draw_difficulty(p, v, dx, k, 0.0);
            }
            Phase::Variants { t, out } => {
                let k = match out {
                    Some(o) => 1.0 - (o as f32 / 20.0).min(1.0),
                    None => cube_out(t as f32 / 20.0),
                };
                self.draw_difficulty(p, v, dx, 1.0, k);
                self.draw_variants(p, v, dx, k);
            }
            Phase::Zoom(t) if t < 10 => {
                self.draw_difficulty(p, v, dx, 1.0 - t as f32 / 10.0, 1.0);
            }
            _ => {}
        }
        let entry = (1.0 - self.entry_t as f32 * 0.2).max(0.0);
        let zoom_fade = match self.phase {
            Phase::Zoom(t) if t >= 60 => 1.0,
            Phase::Zoom(_) => 1.0 - (fade * std::f32::consts::FRAC_PI_2).cos(),
            _ => 0.0,
        };
        let f = entry.max(zoom_fade);
        if f > 0.0 {
            p.rect(0.0, 0.0, w, 240.0, Color::BLACK * f);
        }
    }

    fn guides(&self) -> Vec<(&'static str, u16)> {
        // The map scenes show no button guides (only the main menu does);
        // here they would sit on the level strip.
        Vec::new()
    }
}

impl DwMap {
    /// Session stats (`DarkWorldLevelSelectOverlay`): totals on the left,
    /// the selected tower's attempts, deaths, best time and curses on the
    /// right (records of this session; best times are Legendary clears).
    fn stats(&self, p: &mut Painter, v: &View, dx: f32) {
        let recs = &v.s.coop.records.dark_world;
        let k = self.players.clamp(1, 4) - 1;
        let total_time: u32 = recs.values().map(|r| r.best[k]).sum();
        let total_curses: u32 = recs.values().map(|r| r.curses[k]).sum();
        let lx = 10.0 + dx - 210.0 * (1.0 - v.k);
        let mut y = 14.0;
        let label = |p: &mut Painter, a: &str, b: &str, x: f32, y: f32, right: bool| {
            let j = if right { 1.0 } else { 0.0 };
            let sx = if right { -1.0 } else { 1.0 };
            p.text_outlined(
                a,
                Vec2::new(x + 8.0 * sx, y),
                (j, 0.5),
                1.0,
                Color::rgb(211, 211, 211),
            );
            p.text_outlined(
                b,
                Vec2::new(x + 10.0 * sx, y + 8.0),
                (j, 0.5),
                1.0,
                Color::WHITE,
            );
        };
        if total_time > 0 {
            p.m(
                "questResults/goldSkull",
                Vec2::new(lx, y),
                Org::Center,
                1.0,
                Color::WHITE,
            );
            label(
                p,
                "TOTAL TIME:",
                &tfp_sim::trials::time_string(total_time),
                lx,
                y,
                false,
            );
            y += 18.0;
        }
        if total_curses > 0 {
            p.m(
                "questResults/goldSkull",
                Vec2::new(lx, y),
                Org::Center,
                1.0,
                Color::WHITE,
            );
            label(p, "TOTAL CURSES:", &total_curses.to_string(), lx, y, false);
        }
        let Some(r) = recs.get(&self.sel) else { return };
        // The right edge; slides out while the selection changes.
        let rx = p.w + 80.0 * cube_in(self.stats_t) + 180.0 * (1.0 - v.k);
        if r.attempts > 0 {
            label(p, "ATTEMPTS:", &r.attempts.to_string(), rx, 14.0, true);
        }
        if r.deaths > 0 {
            label(p, "DEATHS:", &r.deaths.to_string(), rx, 32.0, true);
        }
        if r.best[k] > 0 {
            p.m(
                "questResults/goldSkull",
                Vec2::new(rx - 10.0, 50.0),
                Org::Center,
                1.0,
                Color::WHITE,
            );
            p.text_outlined(
                &tfp_sim::trials::time_string(r.best[k]),
                Vec2::new(rx - 16.0, 50.0),
                (1.0, 0.5),
                1.0,
                Color::WHITE,
            );
        }
        if r.curses[k] > 0 {
            p.m(
                "questResults/goldSkull",
                Vec2::new(rx - 10.0, 60.0),
                Org::Center,
                1.0,
                Color::WHITE,
            );
            let s = if r.curses[k] == 1 {
                "1 CURSE".to_string()
            } else {
                format!("{} CURSES", r.curses[k])
            };
            p.text_outlined(
                &s,
                Vec2::new(rx - 16.0, 60.0),
                (1.0, 0.5),
                1.0,
                Color::WHITE,
            );
        }
    }

    /// The three difficulty shields (`DarkWorldDifficultySelect`); `k` =
    /// in/out progress, `exp` = expanded for the variants overlay (the
    /// chosen shield flies to the bottom right).
    fn draw_difficulty(&self, p: &mut Painter, v: &View, dx: f32, k: f32, exp: f32) {
        if k <= 0.0 {
            return;
        }
        let y = 70.0 + (0.0 - 70.0) * exp;
        let hgt = (100.0 + (240.0 - 100.0) * exp).ceil();
        p.rect(-40.0, y, 200.0 * k + dx, hgt, Color::BLACK * (0.8 * k));
        // Two panels sliding in from the sides to the centre.
        p.rect(
            p.w + 40.0 - 200.0 * k - dx,
            y,
            200.0 * k + dx,
            hgt,
            Color::BLACK * (0.8 * k),
        );
        let at = [70.0, 160.0, 250.0];
        let from = [-100.0, -100.0, 420.0];
        for i in 0..3 {
            let sel = i as u8 == self.difficulty;
            let base = Vec2::new(from[i] + (at[i] - from[i]) * k + dx, 120.0);
            let lerp_v = match i {
                0 => 1.0 - self.diff_lerp.min(1.0),
                1 => 1.0 - (self.diff_lerp - 1.0).abs(),
                _ => (self.diff_lerp - 1.0).max(0.0),
            };
            let wig = match self.wiggle {
                Some((wi, t)) if wi == i => {
                    let tt = t as f32 / 20.0;
                    (tt * 5.0 * std::f32::consts::TAU).sin() * (1.0 - tt)
                }
                _ => 0.0,
            };
            // Expanded: the chosen shield moves to (188, 212) small; the
            // others leave.
            let (pos, isc, tsc) = if exp > 0.0 {
                if sel {
                    let to = Vec2::new(188.0 + dx, 212.0);
                    let ctrl = Vec2::new(base.x + 60.0, base.y - 20.0);
                    let e = cube_out(exp);
                    let pos = lerp(lerp(base, ctrl, e), lerp(ctrl, to, e), e);
                    (pos, 2.0 + (1.0 - 2.0) * e, 3.0 + (1.0 - 3.0) * e)
                } else {
                    let off = if i == 2 { 320.0 } else { -320.0 };
                    (base + Vec2::new(off * cube_out(exp), 0.0), 1.0, 1.0)
                }
            } else {
                (
                    base,
                    1.0 + lerp_v + 0.2 * wig,
                    (1.0 + 2.0 * lerp_v).round().max(1.0),
                )
            };
            if let Some(s) = p.a.g(shield_image(i as u8)) {
                p.tex(s, pos, Org::Center, isc, Color::WHITE);
            }
            p.text_outlined(
                difficulty_name(i as u8),
                pos,
                (0.5, 0.5),
                tsc.round().max(1.0),
                difficulty_color(i as u8),
            );
        }
        let _ = v;
    }

    fn draw_variants(&self, p: &mut Painter, v: &View, dx: f32, k: f32) {
        if k <= 0.0 {
            return;
        }
        let tick = v.tick;
        let title_x = 480.0 + (160.0 - 480.0) * k + dx;
        p.text_outlined(
            "VARIANTS",
            Vec2::new(title_x, 12.0),
            (0.5, 0.5),
            2.0,
            Color::WHITE,
        );
        let shake = if self.shake > 0 {
            let h = (tick.wrapping_mul(2_654_435_761) >> 7) as u32;
            Vec2::new((h % 5) as f32 - 2.0, ((h >> 3) % 5) as f32 - 2.0)
        } else {
            Vec2::ZERO
        };
        let capped = power_ups_on(&self.variants) >= self.cap();
        let pbox = Vec2::new(99.0 + dx - 320.0 * (1.0 - k), 70.0) + shake;
        let pcol = if capped {
            POWER_COLOR * 0.4
        } else {
            POWER_COLOR
        };
        p.m(
            "darkWorld/variantBox",
            pbox + Vec2::new(0.0, 12.0),
            Org::Center,
            1.0,
            pcol,
        );
        p.text_outlined(
            "POWERUPS",
            pbox + Vec2::new(0.0, 42.0),
            (0.5, 0.5),
            1.0,
            POWER_COLOR,
        );
        let cbox = Vec2::new(221.0 + dx + 320.0 * (1.0 - k), 70.0);
        p.m(
            "darkWorld/variantBox",
            cbox + Vec2::new(0.0, 12.0),
            Org::Center,
            1.0,
            CURSE_COLOR,
        );
        p.text_outlined(
            "CURSES",
            cbox + Vec2::new(0.0, 42.0),
            (0.5, 0.5),
            1.0,
            CURSE_COLOR,
        );
        let curses = self.curses();
        let mut bubble: Option<(Vec2, String)> = None;
        for (side, list) in [(0, &POWER_UPS[..]), (1, &curses[..])] {
            for (i, &var) in list.iter().enumerate() {
                let sel_id = if side == 0 {
                    VSel::Power(i)
                } else {
                    VSel::Curse(i)
                };
                let off = if side == 0 { -320.0 } else { 320.0 };
                let mut pos = Self::button_pos(sel_id, dx) + Vec2::new(off * (1.0 - k), 0.0);
                if side == 0 {
                    pos += shake;
                }
                let selected = self.vsel == sel_id;
                let on = self.variants.any(var);
                let can = !(side == 0 && capped && !on);
                let (title, icon) = variant_ui(var);
                let wig = match self.wiggle {
                    Some((wi, t)) if wi == 10 + if side == 0 { i } else { 6 + i } => {
                        let tt = t as f32 / 20.0;
                        (tt * 3.0 * std::f32::consts::TAU).sin() * (1.0 - tt)
                    }
                    _ => 0.0,
                };
                let scale = (1.0 + 0.2 * wig) * if selected { 1.2 } else { 1.0 };
                let col = if can {
                    Color::WHITE
                } else {
                    Color::WHITE * 0.4
                };
                if let Some(s) = p.a.m(&icon) {
                    if can {
                        p.tex_outlined(s, pos, Org::Center, scale, col);
                    } else {
                        p.tex(s, pos, Org::Center, scale, col);
                    }
                } else {
                    p.text(&title[..1], pos, (0.5, 0.5), 1.0, col);
                }
                if selected {
                    let c = if on { pal::SELECTED } else { pal::IDLE };
                    let n = ((20.0 + sine(tick, 60.0, 0.0) * 2.0) / 2.0).ceil();
                    p.list
                        .hollow_rect(pos.x - n, pos.y - n, n * 2.0, n * 2.0, c);
                    bubble = Some((pos, title));
                } else if on {
                    p.list
                        .hollow_rect(pos.x - 10.0, pos.y - 10.0, 20.0, 20.0, pal::SELECTED);
                }
            }
        }
        // BEGIN!
        let bp = Vec2::new(460.0 + (160.0 - 460.0) * k + dx, 155.0);
        let bsel = self.vsel == VSel::Begin;
        let bc = if bsel { pal::SELECTED } else { pal::IDLE };
        p.text_outlined("BEGIN!", bp, (0.5, 0.5), 2.0, bc);
        if bsel {
            let s3 = 3.0 * sine(tick, 60.0, 0.0);
            if let Some(a) = p.a.m("bowSelection") {
                p.tex(
                    a,
                    bp + Vec2::new(-52.0 - s3, 0.0),
                    Org::Center,
                    1.0,
                    Color::WHITE,
                );
                p.tex_ex(
                    a,
                    bp + Vec2::new(52.0 + s3, 0.0),
                    Org::Center,
                    Vec2::new(-1.0, 1.0),
                    0.0,
                    Color::WHITE,
                    false,
                );
            }
        }
        // Power-up count against the cap.
        p.text_outlined(
            &format!("{}/{}", power_ups_on(&self.variants), self.cap()),
            pbox + Vec2::new(0.0, 54.0),
            (0.5, 0.5),
            1.0,
            pcol,
        );
        if let Some((pos, title)) = bubble {
            let tw = p.measure(&title).x;
            let bw = ((tw / 10.0).ceil() + 1.0) * 10.0;
            let x = (pos.x).clamp(bw / 2.0 + 5.0, p.w - bw / 2.0 - 5.0);
            let y = pos.y - 14.0;
            p.rect(x - bw / 2.0, y - 14.0, bw, 14.0, Color::WHITE);
            p.text(&title, Vec2::new(x, y - 7.0), (0.5, 0.5), 1.0, Color::BLACK);
            p.m(
                "variants/bubbleHook",
                Vec2::new(pos.x, y + 2.0),
                Org::Center,
                1.0,
                Color::WHITE,
            );
        }
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn variant_navigation() {
        let m = DwMap {
            towers: Vec::new(),
            sel: 0,
            difficulty: 1,
            diff_lerp: 1.0,
            variants: VariantSet::default(),
            vsel: VSel::Begin,
            begin_up: VSel::Curse(3),
            shake: 0,
            players: 1,
            strip_x: Vec::new(),
            cam: (Vec2::ZERO, Vec2::ZERO, 0),
            phase: Phase::Select,
            entry_t: 0,
            stats_t: 0.0,
            wiggle: None,
        };
        assert_eq!(m.nav(VSel::Power(2), (1, 0)), Some(VSel::Curse(0)));
        assert_eq!(m.nav(VSel::Curse(3), (-1, 0)), Some(VSel::Power(5)));
        assert_eq!(m.nav(VSel::Power(4), (0, 1)), Some(VSel::Begin));
        assert_eq!(m.nav(VSel::Begin, (0, -1)), Some(VSel::Curse(3)));
        assert_eq!(m.nav(VSel::Power(0), (-1, 0)), None);
        // Hardcore: one power-up.
        let mut v = VariantSet::default();
        v.set_all(Variant::StartWithWings, true, ALL_SLOTS);
        let m = DwMap { variants: v, ..m };
        assert!(!m.can_toggle(Variant::StartWithShields));
        assert!(m.can_toggle(Variant::StartWithWings));
        assert!(m.can_toggle(Variant::CursedBows));
    }
}
