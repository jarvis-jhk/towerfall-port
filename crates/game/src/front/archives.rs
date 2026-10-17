//! ARCHIVES (ui.md 12.1): a paged statistics browser over our records
//! (`save.rs`). Pages: STATS, SESSION, DEATHS, AWARDS, QUEST, DARK WORLD,
//! TRIALS. The controller (page title, scale 2, at y = 45 with bobbing
//! arrows at +-80, dimmed at the ends; 20-frame CubeInOut slide between
//! pages 320 px apart) is the original's; the page layouts are ours (the
//! spec has them as plain text / graph pages). GLOBAL (Steam), LESSONS and
//! ASCENSION are left out. Back returns to the main menu.

use tfp_render::Color;
use tfp_render::glam::Vec2;

use super::gfx::{Painter, cube_in_out, pal, rgba, sine};
use super::minput::b;
use super::{Ctx, Go, Screen, ScreenId, View};
use crate::save::{SaveData, SessionStats};

const PAGES: [&str; 7] = [
    "STATS",
    "SESSION",
    "DEATHS",
    "AWARDS",
    "QUEST",
    "DARK WORLD",
    "TRIALS",
];

const ARCHERS: [&str; 9] = [
    "GREEN", "BLUE", "PINK", "ORANGE", "WHITE", "YELLOW", "CYAN", "PURPLE", "RED",
];

/// Award rows shown at once on the AWARDS page.
const AWARD_ROWS: usize = 14;

pub struct Archives {
    data: SaveData,
    session: SessionStats,
    page: usize,
    /// Slide: from page, frames left.
    slide: Option<(usize, u32)>,
    award_scroll: usize,
    quest_names: Vec<String>,
    dw_names: Vec<String>,
    /// Trials levels: (tower name, tier).
    trials: Vec<(String, usize)>,
}

fn time(ticks: u64) -> String {
    let s = ticks / 60;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn short_time(ticks: u32) -> String {
    if ticks == 0 {
        "--".into()
    } else {
        tfp_sim::trials::time_string(ticks)
    }
}

impl Archives {
    pub fn new() -> Self {
        let (data, session) = crate::save::snapshot();
        let installs = tfp_assets::Installs::discover();
        let (mut quest_names, mut dw_names, mut trials) = (Vec::new(), Vec::new(), Vec::new());
        if let Some(inst) = installs.steam.as_ref().or(installs.eight_player.as_ref()) {
            let gd = tfp_assets::game_data::GameData::load(&inst.content).ok();
            let name = |theme: &str| {
                gd.as_ref()
                    .and_then(|g| g.theme(theme))
                    .map_or(theme.to_uppercase(), |t| t.name.to_uppercase())
            };
            if let Ok(idx) = tfp_assets::towers::TowerIndex::build(inst) {
                quest_names = idx.quest.iter().map(|t| name(&t.theme)).collect();
                dw_names = idx.dark_world.iter().map(|t| name(&t.theme)).collect();
                trials = idx
                    .trials
                    .iter()
                    .map(|t| (name(&t.theme), t.tier))
                    .collect();
            }
        }
        // Screenshots: `TFP_ARCHIVES_PAGE=n --screen archives`.
        let page = std::env::var("TFP_ARCHIVES_PAGE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
            .min(PAGES.len() - 1);
        Self {
            data,
            session,
            page,
            slide: None,
            award_scroll: 0,
            quest_names,
            dw_names,
            trials,
        }
    }

    #[allow(clippy::needless_range_loop)] // parallel per-archer arrays
    fn draw_page(&self, p: &mut Painter, page: usize, x0: f32) {
        let cx = x0;
        let white = Color::WHITE;
        let grey = pal::IDLE;
        let line = |p: &mut Painter, l: &str, r: String, x: f32, y: f32, w: f32, c: Color| {
            p.text_outlined(l, Vec2::new(x, y), (0.0, 0.5), 1.0, c);
            p.text_outlined(&r, Vec2::new(x + w, y), (1.0, 0.5), 1.0, white);
        };
        let archer_col =
            |p: &Painter, a: usize| p.a.archers.get(a).map_or(white, |x| rgba(x.color_a));
        let v = &self.data.versus;
        let y0 = 74.0;
        match PAGES[page] {
            "STATS" => {
                let rows = [
                    ("MATCHES", v.matches.to_string()),
                    ("ROUNDS", v.rounds.to_string()),
                    ("KILLS", v.kills.total.to_string()),
                    ("ARROWS SHOT", v.arrows_shot.to_string()),
                    ("ARROWS COLLECTED", v.arrows_collected.to_string()),
                    ("ARROWS CAUGHT", v.arrows_caught.to_string()),
                    ("TREASURES", v.treasures_taken.to_string()),
                    ("DODGES", v.dodges.to_string()),
                    ("JUMPS", v.jumps.to_string()),
                    ("BATTLE TIME", time(v.time_played)),
                    ("TIMES LAUNCHED", v.times_launched.to_string()),
                ];
                for (i, (l, r)) in rows.into_iter().enumerate() {
                    line(p, l, r, cx - 150.0, y0 + 12.0 * i as f32, 130.0, grey);
                }
                p.text_outlined(
                    "KILLS",
                    Vec2::new(cx + 95.0, y0 - 2.0),
                    (1.0, 0.5),
                    1.0,
                    pal::YELLOW,
                );
                p.text_outlined(
                    "WINS",
                    Vec2::new(cx + 140.0, y0 - 2.0),
                    (1.0, 0.5),
                    1.0,
                    pal::YELLOW,
                );
                for a in 0..9 {
                    let y = y0 + 10.0 + 12.0 * a as f32;
                    let c = archer_col(p, a);
                    p.text_outlined(ARCHERS[a], Vec2::new(cx + 10.0, y), (0.0, 0.5), 1.0, c);
                    let k = v.kills.per_archer[a].to_string();
                    p.text_outlined(&k, Vec2::new(cx + 95.0, y), (1.0, 0.5), 1.0, white);
                    let w = v.wins[a].to_string();
                    p.text_outlined(&w, Vec2::new(cx + 140.0, y), (1.0, 0.5), 1.0, white);
                }
            }
            "SESSION" => {
                let s = &self.session;
                for (i, h) in ["KILLS", "DEATHS", "SELF", "WINS"].iter().enumerate() {
                    let x = cx - 10.0 + 40.0 * i as f32;
                    p.text_outlined(h, Vec2::new(x, y0 - 2.0), (1.0, 0.5), 1.0, pal::YELLOW);
                }
                for a in 0..9 {
                    let y = y0 + 10.0 + 11.0 * a as f32;
                    let c = archer_col(p, a);
                    p.text_outlined(ARCHERS[a], Vec2::new(cx - 140.0, y), (0.0, 0.5), 1.0, c);
                    for (i, n) in [s.kills[a], s.deaths[a], s.self_kills[a], s.wins[a]]
                        .into_iter()
                        .enumerate()
                    {
                        let x = cx - 10.0 + 40.0 * i as f32;
                        p.text_outlined(&n.to_string(), Vec2::new(x, y), (1.0, 0.5), 1.0, white);
                    }
                }
                let y = y0 + 116.0;
                line(
                    p,
                    "MATCHES",
                    s.matches.to_string(),
                    cx - 140.0,
                    y,
                    100.0,
                    grey,
                );
                line(p, "ROUNDS", s.rounds.to_string(), cx + 20.0, y, 100.0, grey);
                line(
                    p,
                    "BATTLE TIME",
                    time(s.time_played),
                    cx - 140.0,
                    y + 12.0,
                    100.0,
                    grey,
                );
                p.text_outlined(
                    "SESSION STATS RESET WHEN YOU EXIT THE GAME",
                    Vec2::new(cx, y + 30.0),
                    (0.5, 0.5),
                    1.0,
                    grey,
                );
            }
            "DEATHS" => {
                let mut causes: Vec<(&String, &u64)> = v.deaths.by_cause.iter().collect();
                causes.sort_by(|a, b| b.1.cmp(a.1));
                line(
                    p,
                    "TOTAL",
                    v.deaths.total.to_string(),
                    cx - 150.0,
                    y0,
                    130.0,
                    pal::YELLOW,
                );
                line(
                    p,
                    "SELF",
                    v.deaths.own.to_string(),
                    cx - 150.0,
                    y0 + 12.0,
                    130.0,
                    grey,
                );
                line(
                    p,
                    "TEAM",
                    v.deaths.team.to_string(),
                    cx - 150.0,
                    y0 + 24.0,
                    130.0,
                    grey,
                );
                let env = v.deaths.environment.to_string();
                line(p, "ENVIRONMENT", env, cx - 150.0, y0 + 36.0, 130.0, grey);
                for (i, (k, n)) in causes.iter().enumerate().take(13) {
                    let name = match k.as_str() {
                        "JumpedOn" => "STOMPED".to_owned(),
                        "SpikeBall" => "SPIKE BALL".to_owned(),
                        "FallingObject" => "FALLING".to_owned(),
                        o => o.to_uppercase(),
                    };
                    line(
                        p,
                        &name,
                        n.to_string(),
                        cx + 10.0,
                        y0 + 11.0 * i as f32,
                        130.0,
                        grey,
                    );
                }
            }
            "AWARDS" => {
                let names = tfp_sim::versus::stats::AWARD_NAMES;
                let total: u64 = v.awards.iter().flatten().sum();
                p.text_outlined(
                    &format!("{total} AWARDS GIVEN   UP/DOWN: SCROLL"),
                    Vec2::new(cx, y0 - 2.0),
                    (0.5, 0.5),
                    1.0,
                    grey,
                );
                for r in 0..AWARD_ROWS {
                    let i = self.award_scroll + r;
                    let Some(name) = names.get(i) else { break };
                    let y = y0 + 10.0 + 10.0 * r as f32;
                    let row = v.awards.get(i).copied().unwrap_or([0; 9]);
                    let sum: u64 = row.iter().sum();
                    let c = if sum > 0 { white } else { grey };
                    p.text_outlined(
                        &name.to_uppercase(),
                        Vec2::new(cx - 150.0, y),
                        (0.0, 0.5),
                        1.0,
                        c,
                    );
                    for (a, n) in row.iter().enumerate() {
                        if *n > 0 {
                            let x = cx + 20.0 + 15.0 * a as f32;
                            let col = archer_col(p, a);
                            p.text_outlined(&n.to_string(), Vec2::new(x, y), (0.5, 0.5), 1.0, col);
                        }
                    }
                }
            }
            "QUEST" => {
                for (i, h) in ["N", "H", "ND", "1P", "2P+", "TRIES"].iter().enumerate() {
                    let x = cx - 20.0 + [-15.0, 0.0, 18.0, 75.0, 125.0, 170.0][i];
                    p.text_outlined(h, Vec2::new(x, y0 - 2.0), (1.0, 0.5), 1.0, pal::YELLOW);
                }
                let n = self
                    .quest_names
                    .len()
                    .max(self.data.quest.keys().max().map_or(0, |k| k + 1));
                for t in 0..n.min(14) {
                    let y = y0 + 10.0 + 10.0 * t as f32;
                    let q = self.data.quest.get(&t).copied().unwrap_or_default();
                    let name = self
                        .quest_names
                        .get(t)
                        .cloned()
                        .unwrap_or(format!("TOWER {t}"));
                    let c = if q.revealed || q.total_attempts > 0 {
                        white
                    } else {
                        grey
                    };
                    p.text_outlined(&name, Vec2::new(cx - 150.0, y), (0.0, 0.5), 1.0, c);
                    let mark = |b: bool| if b { "*" } else { "-" };
                    let cells = [
                        mark(q.completed_normal).to_owned(),
                        mark(q.completed_hardcore).to_owned(),
                        mark(q.completed_no_deaths).to_owned(),
                        short_time(q.best[0]),
                        short_time(q.best[1]),
                        q.total_attempts.to_string(),
                    ];
                    for (i, s) in cells.iter().enumerate() {
                        let x = cx - 20.0 + [-15.0, 0.0, 18.0, 75.0, 125.0, 170.0][i];
                        p.text_outlined(s, Vec2::new(x, y), (1.0, 0.5), 1.0, white);
                    }
                }
            }
            "DARK WORLD" => {
                for (t, name) in self.dw_names.iter().enumerate().take(5) {
                    let d = self.data.dark_world.get(&t).copied().unwrap_or_default();
                    let y = y0 + 30.0 * t as f32;
                    let c = if d.revealed || d.attempts > 0 {
                        white
                    } else {
                        grey
                    };
                    p.text_outlined(name, Vec2::new(cx - 150.0, y), (0.0, 0.5), 1.0, c);
                    let flags = format!(
                        "{}{}{}{}",
                        if d.completed_normal { "NORMAL " } else { "" },
                        if d.completed_hardcore {
                            "HARDCORE "
                        } else {
                            ""
                        },
                        if d.completed_legendary {
                            "LEGENDARY "
                        } else {
                            ""
                        },
                        if d.earned_gold_eye {
                            "GOLD EYE"
                        } else if d.earned_eye {
                            "EYE"
                        } else {
                            ""
                        }
                    );
                    p.text_outlined(
                        &flags,
                        Vec2::new(cx + 150.0, y),
                        (1.0, 0.5),
                        1.0,
                        pal::YELLOW,
                    );
                    let best = (0..4)
                        .map(|k| format!("{}P {}", k + 1, short_time(d.best[k])))
                        .collect::<Vec<_>>()
                        .join("  ");
                    p.text_outlined(
                        &best,
                        Vec2::new(cx - 140.0, y + 10.0),
                        (0.0, 0.5),
                        1.0,
                        grey,
                    );
                    let tail = format!(
                        "CURSES {}  DEATHS {}  TRIES {}",
                        d.most_curses.iter().max().unwrap_or(&0),
                        d.deaths,
                        d.attempts
                    );
                    p.text_outlined(
                        &tail,
                        Vec2::new(cx + 150.0, y + 10.0),
                        (1.0, 0.5),
                        1.0,
                        grey,
                    );
                }
            }
            _ => {
                // TRIALS: medal totals, then one row per tower (3 tiers).
                let medals = |k: usize| self.data.trials.values().filter(|t| t.unlocked[k]).count();
                let n = self.trials.len();
                // The yellow gems once one is found (trials.md 9): "n / 7".
                let gems = self.data.trials.values().filter(|t| t.gem).count();
                let gem_text = if gems > 0 {
                    format!("  GEMS {gems}/7")
                } else {
                    String::new()
                };
                let head = format!(
                    "GOLD {}/{n}  DIAMOND {}/{n}  DEV {}/{n}{gem_text}",
                    medals(0),
                    medals(1),
                    medals(2)
                );
                p.text_outlined(&head, Vec2::new(cx, y0 - 2.0), (0.5, 0.5), 1.0, pal::YELLOW);
                let mut towers: Vec<String> = Vec::new();
                for (name, _) in &self.trials {
                    if !towers.contains(name) {
                        towers.push(name.clone());
                    }
                }
                for (r, tower) in towers.iter().enumerate().take(16) {
                    let y = y0 + 10.0 + 9.0 * r as f32;
                    p.text_outlined(tower, Vec2::new(cx - 150.0, y), (0.0, 0.5), 1.0, grey);
                    let levels = self
                        .trials
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| &t.0 == tower);
                    for (col, (i, _)) in levels.enumerate() {
                        let t = self.data.trials.get(&i).copied().unwrap_or_default();
                        let medal = if t.unlocked[2] {
                            "D"
                        } else if t.unlocked[1] {
                            "*"
                        } else if t.unlocked[0] {
                            "+"
                        } else {
                            " "
                        };
                        let s = format!("{} {}", short_time(t.best), medal);
                        let x = cx - 20.0 + 58.0 * col as f32;
                        p.text_outlined(&s, Vec2::new(x, y), (0.0, 0.5), 1.0, white);
                    }
                }
            }
        }
    }
}

impl Screen for Archives {
    fn id(&self) -> ScreenId {
        ScreenId::Archives
    }

    fn section(&self) -> f32 {
        1.0
    }

    fn title(&self) -> Option<&'static str> {
        Some("archives")
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        if let Some((_, n)) = &mut self.slide {
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.slide = None;
            }
        }
        if !c.ready {
            return None;
        }
        if c.mi.p(b::BACK) {
            c.play("ui_clickBack", None);
            return Some(Go::To(ScreenId::Main));
        }
        if self.slide.is_none() {
            let from = self.page;
            if c.mi.p(b::LEFT) && self.page > 0 {
                self.page -= 1;
            } else if c.mi.p(b::RIGHT) && self.page + 1 < PAGES.len() {
                self.page += 1;
            }
            if self.page != from {
                self.slide = Some((from, 20));
                c.play("ui_move1", None);
            }
        }
        if PAGES[self.page] == "AWARDS" {
            let max = tfp_sim::versus::stats::AWARD_NAMES.len() - AWARD_ROWS;
            if c.mi.p(b::DOWN) && self.award_scroll < max {
                self.award_scroll += 1;
            }
            if c.mi.p(b::UP) && self.award_scroll > 0 {
                self.award_scroll -= 1;
            }
        }
        None
    }

    fn guides(&self) -> Vec<(&'static str, u16)> {
        vec![("BACK", b::BACK)]
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let cx = v.at(Vec2::new(v.w + 160.0, 0.0), Vec2::new(v.cx(), 0.0)).x;
        // Controller: title and arrows.
        p.text_outlined(
            PAGES[self.page],
            Vec2::new(cx, 45.0),
            (0.5, 0.5),
            2.0,
            Color::WHITE,
        );
        let bob = 2.0 * sine(v.tick, 40.0, 0.0);
        for (dx, s, on) in [
            (-80.0, "<", self.page > 0),
            (80.0, ">", self.page + 1 < PAGES.len()),
        ] {
            let c = if on { Color::WHITE } else { Color::WHITE * 0.3 };
            let x = cx + dx + if dx < 0.0 { -bob } else { bob };
            p.text_outlined(s, Vec2::new(x, 45.0), (0.5, 0.5), 2.0, c);
        }
        // Pages 320 px apart, sliding.
        match self.slide {
            Some((from, n)) => {
                let k = cube_in_out(1.0 - n as f32 / 20.0);
                let dir = if self.page > from { 1.0 } else { -1.0 };
                let off = 320.0 * dir * (1.0 - k);
                self.draw_page(p, from, cx + off - 320.0 * dir);
                self.draw_page(p, self.page, cx + off);
            }
            None => self.draw_page(p, self.page, cx),
        }
    }
}
