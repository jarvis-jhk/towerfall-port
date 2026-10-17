//! Co-op menu (ui.md 5), Quest tower select with the difficulty choice and
//! the Trials level grid (ui.md 9.2), as front-end screens. Confirming
//! zooms into the tower like the versus map and leaves a `CoopLaunch` in
//! the session for the game scene.

use tfp_render::Color;
use tfp_render::glam::{Affine2, Vec2};

use super::{CoopLaunch, CoopMode, RollcallMode, players_from_slots};
use crate::front::assets::UiAssets;
use crate::front::gfx::{Org, Painter, cube_out, lerp, pal, sine};
use crate::front::minput::b;
use crate::front::{Ctx, Go, Screen, ScreenId, Session, View};

pub(crate) const MAP_W: f32 = 472.0;
pub(crate) const MAP_H: f32 = 480.0;

/// A quest tower or trials level on the map.
#[derive(Clone, Debug)]
struct Tower {
    name: String,
    icon: String,
    map_pos: (f32, f32),
    block: &'static str,
    /// Trials: (tower column, tier); quest: (id, 0).
    at: (usize, usize),
    /// Index into the sim content / tower index list.
    level: usize,
    /// Trials thresholds (gold, diamond, dev) in seconds.
    goals: [f32; 3],
    locked: bool,
}

fn load_towers(a: &UiAssets, quest: bool) -> Vec<Tower> {
    let Some(inst) = &a.install else {
        return Vec::new();
    };
    let (Ok(index), Ok(gd)) = (
        tfp_assets::towers::TowerIndex::build(inst),
        tfp_assets::game_data::GameData::load(&inst.content),
    ) else {
        return Vec::new();
    };
    let theme = |id: &str| gd.themes.iter().find(|t| t.id == id);
    if quest {
        index
            .quest
            .iter()
            .enumerate()
            .map(|(i, q)| {
                let th = theme(&q.theme);
                Tower {
                    name: th.map_or(q.name.clone(), |t| t.name.to_uppercase()),
                    icon: th.map_or(String::new(), |t| format!("towerIcons/{}", t.icon)),
                    map_pos: th.map_or((240.0, 240.0), |t| t.map_position),
                    block: match i {
                        8..=10 => "specialLevelBlock",
                        11 => "superSpecialLevelBlock",
                        _ => "levelBlock",
                    },
                    at: (i, 0),
                    level: i,
                    goals: [0.0; 3],
                    locked: false,
                }
            })
            .collect()
    } else {
        index
            .trials
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let th = theme(&t.theme);
                let base = th.map_or("TOWER".into(), |t| t.name.to_uppercase());
                Tower {
                    name: match t.tier {
                        1 => format!("{base} II"),
                        2 => format!("{base} III"),
                        _ => base,
                    },
                    icon: th.map_or(String::new(), |t| format!("towerIcons/{}", t.icon)),
                    map_pos: th.map_or((240.0, 240.0), |t| t.map_position),
                    block: if t.tier == 0 {
                        if t.index >= 12 {
                            "darkLevelBlock"
                        } else {
                            "levelBlock"
                        }
                    } else if t.index >= 12 {
                        "smallDarkLevelBlock"
                    } else {
                        "smallLevelBlock"
                    },
                    at: (t.index, t.tier),
                    level: i,
                    goals: t.goals,
                    locked: !t.path.is_file(),
                }
            })
            .collect()
    }
}

/// The world map backdrop with a camera on `focus` (ui.md 9.1).
pub(crate) fn draw_map(p: &mut Painter, focus: Vec2, zoom: f32, tick: u64) {
    let w = p.w;
    let saved = p.list.transform();
    let xf = Affine2::from_translation(Vec2::new(w / 2.0, 120.0))
        * Affine2::from_scale(Vec2::splat(zoom))
        * Affine2::from_translation(-focus.round());
    p.list.set_transform(xf);
    p.rect(
        -200.0,
        -200.0,
        MAP_W + 400.0,
        MAP_H + 400.0,
        Color::rgb(24, 52, 112),
    );
    if let Some(water) = p.a.m("mapWater") {
        p.sine_texture_v(
            water,
            Vec2::new(-5.0, 0.0),
            tick as f32 * 0.03,
            2.0,
            2,
            std::f32::consts::PI / 8.0,
        );
    }
    p.m("mapLand", Vec2::ZERO, Org::TopLeft, 1.0, Color::WHITE);
    p.list.set_transform(saved);
}

pub(crate) fn clamp_cam(p: Vec2, w: f32) -> Vec2 {
    Vec2::new(
        p.x.clamp(w / 2.0, MAP_W - w / 2.0),
        p.y.clamp(120.0, MAP_H - 120.0),
    )
}

// ------------------------------------------------------------ co-op menu

/// `CoOp` (ui.md 5): QUEST and DARK WORLD.
pub struct CoopMenu {
    sel: usize,
    lerp: [f32; 2],
    confirmed: Option<u32>,
    shake: u32,
}

impl CoopMenu {
    pub fn new(s: &mut Session) -> Self {
        Self {
            // DARK WORLD when that was the last co-op mode (ui.md 5).
            sel: usize::from(s.coop.mode == RollcallMode::DarkWorld),
            lerp: [0.0; 2],
            confirmed: None,
            shake: 0,
        }
    }
}

impl Screen for CoopMenu {
    fn id(&self) -> ScreenId {
        ScreenId::CoOp
    }

    fn section(&self) -> f32 {
        1.0
    }

    fn title(&self) -> Option<&'static str> {
        Some("coOp")
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        for i in 0..2 {
            let on = i == self.sel && self.confirmed.is_none();
            self.lerp[i] = (self.lerp[i] + if on { 0.1 } else { -0.1 }).clamp(0.0, 1.0);
        }
        self.shake = self.shake.saturating_sub(1);
        if let Some(t) = &mut self.confirmed {
            *t += 1;
            if *t >= 60 {
                c.s.coop.mode = if self.sel == 1 {
                    RollcallMode::DarkWorld
                } else {
                    RollcallMode::Quest
                };
                return Some(Go::To(ScreenId::Rollcall));
            }
            return None;
        }
        if !c.ready {
            return None;
        }
        if c.mi.p(b::BACK) {
            c.play("ui_clickBack", None);
            c.s.main_from = ScreenId::CoOp;
            return Some(Go::To(ScreenId::Main));
        }
        if c.mi.p(b::LEFT) && self.sel == 1 {
            self.sel = 0;
            c.play("ui_move1", None);
        } else if c.mi.p(b::RIGHT) && self.sel == 0 {
            self.sel = 1;
            c.play("ui_move1", None);
        } else if c.mi.p(b::CONFIRM) || c.mi.p(b::START) {
            if self.sel == 0 || c.a.dark_world {
                c.play("ui_selectVersus", None);
                self.confirmed = Some(0);
            } else {
                // No Dark World content in the install (ui.md 5).
                c.play("ui_invalid", None);
                self.shake = 30;
            }
        }
        None
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let items = [
            (
                "QUEST",
                "1-2 ARCHERS",
                "questMode",
                Vec2::new(100.0, 90.0),
                Vec2::new(-160.0, 120.0),
            ),
            (
                "DARK WORLD",
                "1-4 ARCHERS",
                "darkWorldMode",
                Vec2::new(220.0, 90.0),
                Vec2::new(480.0, 120.0),
            ),
        ];
        let dx = (v.w - 320.0) / 2.0;
        for (i, (title, sub, icon, pos, from)) in items.into_iter().enumerate() {
            let blink = i == self.sel && self.confirmed.is_some_and(|t| (t / 2) % 2 == 1);
            if blink {
                continue;
            }
            let sel = self.lerp[i];
            let mut pos = v.at(from, pos) + Vec2::new(dx, 0.0);
            if i == 1 && self.shake > 0 {
                pos.x += [-2.0, 2.0, 0.0][(v.tick % 3) as usize];
            }
            let lift = (-15.0 - 2.0 * sine(v.tick, 120.0, 0.0)) * sel;
            let ip = pos + Vec2::new(0.0, lift.round());
            let dim = if i == 1 && !p.a.dark_world { 0.5 } else { 1.0 };
            p.m_outlined(icon, ip, Org::Center, 1.0, Color::WHITE * dim);
            p.text_outlined(
                title,
                pos + Vec2::new(0.0, 30.0),
                (0.5, 0.5),
                (2.0 + cube_out(sel)).round(),
                Color::WHITE * 0.85,
            );
            if sel > 0.0 {
                p.text_outlined(
                    sub,
                    pos + Vec2::new(0.0, 46.0),
                    (0.5, 0.5),
                    1.0,
                    pal::SUBTITLE * sel,
                );
            }
        }
        let _ = lerp;
    }

    fn guides(&self) -> Vec<(&'static str, u16)> {
        vec![("BACK", b::BACK), ("SELECT", b::CONFIRM)]
    }
}

// ------------------------------------------------------------ tower select

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Select,
    /// Quest: difficulty overlay open.
    Difficulty,
    Zoom(u32),
}

/// Quest tower select (strip of blocks over the map, skull badges) and
/// Trials grid (columns = towers, rows = tiers, medal badges).
pub struct CoopMap {
    quest: bool,
    towers: Vec<Tower>,
    sel: usize,
    hardcore: bool,
    strip_x: Vec<f32>,
    cam: (Vec2, Vec2, u64),
    phase: Phase,
    entry_t: u32,
}

impl CoopMap {
    pub fn quest(s: &mut Session, a: &UiAssets) -> Self {
        Self::new(s, a, true)
    }

    pub fn trials(s: &mut Session, a: &UiAssets) -> Self {
        Self::new(s, a, false)
    }

    fn new(s: &mut Session, a: &UiAssets, quest: bool) -> Self {
        let towers = load_towers(a, quest);
        let want = if quest {
            s.coop.quest_tower
        } else {
            s.coop.trials_level
        };
        let sel = towers.iter().position(|t| t.level == want).unwrap_or(0);
        let mut m = Self {
            quest,
            towers,
            sel,
            hardcore: s.coop.hardcore,
            strip_x: Vec::new(),
            cam: (Vec2::ZERO, Vec2::ZERO, 0),
            phase: Phase::Select,
            entry_t: 0,
        };
        let w = s.width();
        m.strip_x = (0..m.columns()).map(|i| m.col_x(i, w)).collect();
        let c = m.target(w);
        m.cam = (c, c, 0);
        m
    }

    fn columns(&self) -> usize {
        self.towers.iter().map(|t| t.at.0 + 1).max().unwrap_or(0)
    }

    fn sel_col(&self) -> usize {
        self.towers.get(self.sel).map_or(0, |t| t.at.0)
    }

    fn col_x(&self, i: usize, w: f32) -> f32 {
        w / 2.0 + (i as f32 - self.sel_col() as f32) * 40.0
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

    fn find(&self, col: usize, tier: usize) -> Option<usize> {
        self.towers.iter().position(|t| t.at == (col, tier))
    }

    fn select(&mut self, c: &mut Ctx, n: usize) {
        if n == self.sel {
            return;
        }
        self.sel = n;
        c.play("ui_move2", None);
        let now = self.cam_at(c.tick);
        self.cam = (now, self.target(c.w()), c.tick);
    }

    fn launch(&self, c: &mut Ctx) -> CoopLaunch {
        let t = &self.towers[self.sel];
        let max = if self.quest { 4 } else { 1 };
        CoopLaunch {
            mode: if self.quest {
                CoopMode::Quest {
                    tower: t.level,
                    hardcore: self.hardcore,
                }
            } else {
                CoopMode::Trials { level: t.level }
            },
            players: players_from_slots(&c.s.slots, max),
        }
    }

    /// Trials: the found yellow gem, top right of the block (+(10,-10),
    /// small blocks +(10,-3); `MapButton`).
    fn gem_marker(&self, p: &mut Painter, s: &Session, t: &Tower, pos: Vec2, scale: f32) {
        if self.quest || !s.coop.records.trials.get(&t.level).is_some_and(|r| r.gem) {
            return;
        }
        let off = if t.at.1 == 0 {
            Vec2::new(10.0, -10.0)
        } else {
            Vec2::new(10.0, -3.0)
        };
        p.m(
            "trials/yellowGem",
            pos + off * scale,
            Org::Center,
            1.0,
            Color::WHITE,
        );
    }

    fn badge(&self, s: &Session, t: &Tower) -> Option<&'static str> {
        if self.quest {
            let r = s.coop.records.quest.get(&t.level)?;
            Some(if r.no_deaths {
                "questResults/goldSkull"
            } else if r.hardcore {
                "questResults/redSkull"
            } else if r.normal {
                "questResults/whiteSkull"
            } else {
                return None;
            })
        } else {
            let r = s.coop.records.trials.get(&t.level)?;
            Some(if r.unlocked[2] {
                "trials/redDiamondSmall"
            } else if r.unlocked[1] {
                "trials/diamondSmall"
            } else if r.unlocked[0] {
                "trials/goldSmall"
            } else {
                return None;
            })
        }
    }
}

impl Screen for CoopMap {
    fn id(&self) -> ScreenId {
        if self.quest {
            ScreenId::QuestMap
        } else {
            ScreenId::TrialsMap
        }
    }

    fn menu_background(&self) -> bool {
        false
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        self.entry_t += 1;
        let w = c.w();
        let speed = if self.entry_t < 20 { 12.0 } else { 6.0 };
        for i in 0..self.strip_x.len() {
            let t = self.col_x(i, w);
            let x = &mut self.strip_x[i];
            *x += (t - *x).clamp(-speed, speed);
        }
        match &mut self.phase {
            Phase::Zoom(t) => {
                *t += 1;
                if *t == 80 {
                    let l = self.launch(c);
                    c.s.coop.pending = Some(l);
                }
                return None;
            }
            Phase::Difficulty => {
                if c.mi.p(b::BACK) {
                    c.play("ui_clickBack", None);
                    self.phase = Phase::Select;
                } else if c.mi.p(b::UP) || c.mi.p(b::DOWN) {
                    self.hardcore = !self.hardcore;
                    c.play("ui_move1", None);
                } else if c.mi.p(b::CONFIRM) || c.mi.p(b::START) {
                    c.s.coop.hardcore = self.hardcore;
                    c.s.coop.quest_tower = self.towers[self.sel].level;
                    c.play("ui_mapZoom", None);
                    c.stop_music();
                    self.phase = Phase::Zoom(0);
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
        let (col, tier) = self.towers[self.sel].at;
        let mut to = None;
        if c.mi.p(b::LEFT) && col > 0 {
            to = (0..col)
                .rev()
                .find_map(|k| self.find(k, tier).or(self.find(k, 0)));
        } else if c.mi.p(b::RIGHT) {
            to = (col + 1..self.columns()).find_map(|k| self.find(k, tier).or(self.find(k, 0)));
        } else if !self.quest && c.mi.p(b::UP) {
            to = self.find(col, tier + 1);
        } else if !self.quest && c.mi.p(b::DOWN) && tier > 0 {
            to = self.find(col, tier - 1);
        } else if (c.mi.p(b::CONFIRM) || c.mi.p(b::START)) && self.entry_t > 20 {
            let t = &self.towers[self.sel];
            if t.locked {
                c.play("ui_levelLock", None);
                return None;
            }
            c.play("ui_click", None);
            if self.quest {
                self.phase = Phase::Difficulty;
            } else {
                c.s.coop.trials_level = t.level;
                c.play("ui_mapZoom", None);
                c.stop_music();
                self.phase = Phase::Zoom(0);
            }
        }
        if let Some(n) = to {
            self.select(c, n);
        }
        None
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let w = v.w;
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
        if let Some(t) = self.towers.get(self.sel)
            && !matches!(self.phase, Phase::Zoom(_))
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
        let strip_k = v.k
            * if matches!(self.phase, Phase::Zoom(_)) {
                0.0
            } else {
                1.0
            };
        let base_y = lerp(Vec2::splat(300.0), Vec2::splat(212.0), strip_k).x;
        for (i, t) in self.towers.iter().enumerate() {
            let Some(&x) = self.strip_x.get(t.at.0) else {
                continue;
            };
            if !(-40.0..w + 40.0).contains(&x) {
                continue;
            }
            let selected = i == self.sel;
            let scale = if selected { 1.5 } else { 1.0 };
            let y = base_y - [0.0, 30.0, 52.0][t.at.1.min(2)];
            let pos = Vec2::new(x, y);
            let tint = if t.locked {
                Color::WHITE * 0.67
            } else {
                Color::WHITE
            };
            if selected {
                continue;
            }
            p.m(t.block, pos, Org::Center, scale, tint);
            if t.at.1 == 0 {
                p.m(&t.icon, pos, Org::Center, scale, tint);
            } else {
                p.m(
                    &format!("towerIcons/numerals/{}", t.at.1 + 1),
                    pos,
                    Org::Center,
                    scale,
                    tint,
                );
            }
            if let Some(bd) = self.badge(v.s, t) {
                p.m(
                    bd,
                    pos + Vec2::new(9.0, 9.0) * scale,
                    Org::Center,
                    1.0,
                    Color::WHITE,
                );
            }
            self.gem_marker(p, v.s, t, pos, scale);
        }
        if let Some(t) = self.towers.get(self.sel) {
            let x = self.strip_x.get(t.at.0).copied().unwrap_or(w / 2.0);
            let y = base_y - [0.0, 30.0, 52.0][t.at.1.min(2)];
            let pos = Vec2::new(x, y);
            p.m(t.block, pos, Org::Center, 1.5, Color::WHITE);
            if t.at.1 == 0 {
                p.m(&t.icon, pos, Org::Center, 1.5, Color::WHITE);
            } else {
                p.m(
                    &format!("towerIcons/numerals/{}", t.at.1 + 1),
                    pos,
                    Org::Center,
                    1.5,
                    Color::WHITE,
                );
            }
            if let Some(bd) = self.badge(v.s, t) {
                p.m(
                    bd,
                    pos + Vec2::new(13.0, 13.0),
                    Org::Center,
                    1.0,
                    Color::WHITE,
                );
            }
            self.gem_marker(p, v.s, t, pos, 1.5);
            let ty = lerp(Vec2::splat(-18.0), Vec2::splat(12.0), strip_k).x;
            p.text_outlined(
                &t.name,
                Vec2::new(w / 2.0, ty),
                (0.5, 0.5),
                2.0,
                Color::WHITE,
            );
            // Info line under the title.
            let info = if self.quest {
                v.s.coop.records.quest.get(&t.level).map(|r| {
                    let best = r.best.iter().filter(|&&b| b > 0).min().copied();
                    match best {
                        Some(b) => format!("BEST: {}", tfp_sim::trials::time_string(b)),
                        None => String::new(),
                    }
                })
            } else {
                let r = v.s.coop.records.trials.get(&t.level);
                Some(format!(
                    "GOLD {}  DIAMOND {}{}",
                    tfp_sim::trials::time_string(tfp_sim::trials::ticks(t.goals[0])),
                    tfp_sim::trials::time_string(tfp_sim::trials::ticks(t.goals[1])),
                    r.filter(|r| r.best > 0).map_or(String::new(), |r| format!(
                        "  BEST {}",
                        tfp_sim::trials::time_string(r.best)
                    ))
                ))
            };
            if let Some(s) = info.filter(|s| !s.is_empty()) {
                p.text_outlined(
                    &s,
                    Vec2::new(w / 2.0, ty + 16.0),
                    (0.5, 0.5),
                    1.0,
                    pal::SUBTITLE,
                );
            }
        }
        if self.towers.is_empty() {
            p.text_outlined(
                "NO TOWERS FOUND (SET TF_GAME_DIR)",
                Vec2::new(w / 2.0, 120.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            );
        }
        // Difficulty overlay (quest).
        if self.phase == Phase::Difficulty
            || matches!(self.phase, Phase::Zoom(t) if t < 10 && self.quest)
        {
            let c = Vec2::new(w / 2.0, 120.0);
            p.panel(c.x - 70.0, c.y - 30.0, 140.0, 60.0);
            for (k, (label, col)) in [
                ("NORMAL", Color::hex(0x96DDFF)),
                ("HARDCORE", Color::hex(0xFF5432)),
            ]
            .into_iter()
            .enumerate()
            {
                let on = (k == 1) == self.hardcore;
                let y = c.y - 8.0 + 16.0 * k as f32;
                let text = if on {
                    format!("> {label}")
                } else {
                    label.to_string()
                };
                let colr = if on { col } else { pal::PAUSE_IDLE };
                p.text(&text, Vec2::new(c.x, y), (0.5, 0.5), 1.0, colr);
            }
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
}
