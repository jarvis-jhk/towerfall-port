//! Versus settings, team select and the variants menu (ui.md 7, 8).

use tfp_render::Color;
use tfp_render::glam::Vec2;

use super::assets::UiAssets;
use super::gfx::{Org, Painter, cube_out, lerp, pal, rgba, sine};
use super::minput::{Dev, b};
use super::settings::{Length, Mode, Owner, Team, goal_score};
use super::variants::{SECTIONS, VARIANTS};
use super::{Ctx, Go, Screen, ScreenId, Session, View};

/// `BorderButton`: text with bow brackets when selected.
#[allow(clippy::too_many_arguments)]
fn border_button(
    p: &mut Painter,
    pos: Vec2,
    w: f32,
    h: f32,
    selected: bool,
    tick: u64,
    left: bool,
    right: bool,
) {
    if !selected {
        return;
    }
    let pulse = 1.0 + 0.05 * sine(tick, 60.0, 0.0);
    if let Some(bow) = p.a.m("bowSelection") {
        let sc = (h / bow.height() as f32).max(0.5) * pulse;
        let dx = w / 2.0 * pulse;
        p.tex_ex(
            bow,
            pos + Vec2::new(-dx, 0.0),
            Org::Center,
            Vec2::splat(sc),
            0.0,
            Color::WHITE,
            false,
        );
        p.tex_ex(
            bow,
            pos + Vec2::new(dx, 0.0),
            Org::Center,
            Vec2::splat(sc),
            0.0,
            Color::WHITE,
            true,
        );
    }
    if let Some(ar) = p.a.m("bowSelectionArrow") {
        let off = 0.525 * w + 11.0 + 2.0 * sine(tick, 60.0, 0.0);
        if left {
            p.tex_ex(
                ar,
                pos + Vec2::new(-off, 0.0),
                Org::Center,
                Vec2::ONE,
                0.0,
                Color::WHITE,
                true,
            );
        }
        if right {
            p.tex_ex(
                ar,
                pos + Vec2::new(off, 0.0),
                Org::Center,
                Vec2::ONE,
                0.0,
                Color::WHITE,
                false,
            );
        }
    }
}

fn button_color(sel: bool) -> Color {
    if sel { pal::YELLOW } else { pal::IDLE }
}

// ---- versus options ----------------------------------------------------

pub struct VersusOptions {
    sel: usize,
    changed_at: u64,
}

impl VersusOptions {
    pub fn new(s: &mut Session) -> Self {
        let joined = s.joined();
        s.versus.clean(&joined);
        // ui.md 7: back from the variants the cursor is on VARIANTS.
        Self {
            sel: if s.prev_screen == Some(ScreenId::Variants) {
                2
            } else {
                3
            },
            changed_at: 0,
        }
    }

    fn playable(s: &Session) -> bool {
        s.versus.can_play(s.joined_count()).is_ok()
    }
}

impl Screen for VersusOptions {
    fn id(&self) -> ScreenId {
        ScreenId::VersusOptions
    }

    fn section(&self) -> f32 {
        2.0
    }

    fn title(&self) -> Option<&'static str> {
        Some("fight")
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        if !c.ready {
            return None;
        }
        let ok = Self::playable(c.s);
        if !ok {
            self.sel = 0;
        }
        if c.mi.p(b::BACK) {
            c.play("ui_clickBack", None);
            return Some(Go::To(ScreenId::Rollcall));
        }
        let begin = |c: &mut Ctx| {
            c.play("ui_startGameType", None);
            if c.s.versus.mode.is_team() {
                Some(Go::To(ScreenId::TeamSelect))
            } else {
                Some(Go::Fade(ScreenId::Map))
            }
        };
        if ok && c.mi.p(b::START) {
            return begin(c);
        }
        if ok {
            if c.mi.p(b::DOWN) && self.sel < 3 {
                self.sel += 1;
                c.play("ui_move1", None);
            }
            if c.mi.p(b::UP) && self.sel > 0 {
                self.sel -= 1;
                c.play("ui_move1", None);
            }
        }
        let dir = if c.mi.p(b::LEFT) {
            -1
        } else if c.mi.p(b::RIGHT) {
            1
        } else {
            0
        };
        if self.sel == 0 && c.s.versus.mode.is_team() && c.mi.p(b::ALT) {
            let n = (c.s.joined_count() as u8).clamp(2, 6);
            let t = c.s.versus.team_count + 1;
            c.s.versus.team_count = if t > n { 2 } else { t };
            c.play("ui_move2", None);
        }
        match self.sel {
            0 if dir != 0 => {
                let i = Mode::ALL
                    .iter()
                    .position(|&m| m == c.s.versus.mode)
                    .unwrap_or(0) as i32
                    + dir;
                if (0..3).contains(&i) {
                    c.s.versus.mode = Mode::ALL[i as usize];
                    self.changed_at = c.tick;
                    c.play("ui_move2", None);
                }
            }
            1 if dir != 0 => {
                let i = Length::ALL
                    .iter()
                    .position(|&m| m == c.s.versus.length)
                    .unwrap_or(0) as i32
                    + dir;
                if (0..4).contains(&i) {
                    c.s.versus.length = Length::ALL[i as usize];
                    c.play("ui_move2", None);
                }
            }
            2 if c.mi.p(b::CONFIRM) => {
                c.play("ui_click", None);
                return Some(Go::To(ScreenId::Variants));
            }
            3 if c.mi.p(b::CONFIRM) && ok => return begin(c),
            _ => {}
        }
        None
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let s = v.s;
        let cx = v.cx();
        let n = s.joined_count();
        let ok = Self::playable(s);
        let set = &s.versus;
        // Mode.
        let pos = v.at(Vec2::new(-100.0, 90.0), Vec2::new(cx, 90.0));
        let mi = Mode::ALL.iter().position(|&m| m == set.mode).unwrap_or(0);
        border_button(p, pos, 200.0, 30.0, self.sel == 0, v.tick, mi > 0, mi < 2);
        let wig = if v.tick.saturating_sub(self.changed_at) < 20 {
            1.1
        } else {
            1.0
        };
        if let Some(ic) = p.a.m(set.mode.icon()) {
            p.tex_outlined(
                ic,
                pos + Vec2::new(0.0, -20.0),
                Org::Center,
                wig,
                Color::WHITE,
            );
        }
        p.text_outlined(
            "GAME MODE:",
            pos + Vec2::new(0.0, -5.0),
            (0.5, 0.5),
            1.0,
            Color::WHITE,
        );
        p.text_outlined(
            set.mode.name(),
            pos + Vec2::new(0.0, 6.0),
            (0.5, 0.5),
            2.0,
            button_color(self.sel == 0),
        );
        if set.mode.is_team() && set.can_play(n).is_ok() {
            p.text_outlined(
                &format!("TEAMS: {}", set.team_count.clamp(2, 6)),
                pos + Vec2::new(0.0, 17.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            );
        }
        if let Err([l1, l2]) = set.can_play(n) {
            p.text_outlined(&l1, pos + Vec2::new(0.0, 30.0), (0.5, 0.5), 1.0, pal::RED);
            p.text_outlined(&l2, pos + Vec2::new(0.0, 40.0), (0.5, 0.5), 1.0, pal::RED);
        }
        if !ok {
            return;
        }
        // Length.
        let pos = v.at(Vec2::new(v.w + 100.0, 135.0), Vec2::new(cx, 135.0));
        let li = Length::ALL
            .iter()
            .position(|&m| m == set.length)
            .unwrap_or(0);
        border_button(p, pos, 168.0, 34.0, self.sel == 1, v.tick, li > 0, li < 3);
        p.text_outlined(
            set.length.name(),
            pos + Vec2::new(0.0, -6.0),
            (0.5, 0.5),
            2.0,
            button_color(self.sel == 1),
        );
        let goal = goal_score(set.mode, set.length, n);
        let label = format!("PLAY TO:   x {goal}");
        p.text_outlined(
            &label,
            pos + Vec2::new(0.0, 9.0),
            (0.5, 0.5),
            1.0,
            Color::WHITE,
        );
        let iw = p.measure(&label).x;
        let ip = pos + Vec2::new(-iw / 2.0 + p.measure("PLAY TO: ").x + 3.0, 8.0);
        // DeathSkull (12x10, 0.1 s: 0,0,1..12) or coin (8x10, 0.08 s: 0,0,0,1..7).
        let anim =
            |sheet: Option<tfp_render::SubTexture>, fw: i32, fh: i32, per: f32, seq: &[i32]| {
                sheet.map(|c| {
                    let i = ((v.tick as f32 / (per * 60.0)) as usize) % seq.len();
                    let f = seq[i];
                    let cols = (c.width() / fw).max(1);
                    c.sub((f % cols) * fw, (f / cols) * fh, fw, fh)
                })
            };
        let icon = if set.mode == Mode::HeadHunters {
            anim(
                p.a.g("versus/skull"),
                12,
                10,
                0.1,
                &[0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            )
        } else {
            anim(
                p.a.g("pickups/coin"),
                8,
                10,
                0.08,
                &[0, 0, 0, 1, 2, 3, 4, 5, 6, 7],
            )
        };
        if let Some(ic) = icon {
            p.tex_outlined(ic, ip, Org::Center, 1.0, Color::WHITE);
        }
        // Variants.
        let pos = v.at(Vec2::new(-100.0, 170.0), Vec2::new(cx, 170.0));
        border_button(p, pos, 180.0, 34.0, self.sel == 2, v.tick, false, false);
        let act: Vec<usize> = set.variants.active().collect();
        if act.is_empty() {
            p.text_outlined(
                "VARIANTS",
                pos,
                (0.5, 0.5),
                2.0,
                button_color(self.sel == 2),
            );
        } else {
            p.text_outlined(
                "VARIANTS",
                pos + Vec2::new(0.0, -8.0),
                (0.5, 0.5),
                2.0,
                button_color(self.sel == 2),
            );
            let icons: Vec<String> = if set.variants.is_tournament(false) {
                vec!["variants/tournamentRules1v1".into()]
            } else if set.variants.is_tournament(true) {
                vec!["variants/tournamentRules2v2".into()]
            } else {
                act.iter().map(|&i| VARIANTS[i].icon()).collect()
            };
            let shown = icons.len().min(10);
            let x0 = pos.x - (shown as f32 - 1.0) * 8.0;
            for (k, ic) in icons.iter().take(shown).enumerate() {
                p.m_outlined(
                    ic,
                    Vec2::new(x0 + 16.0 * k as f32, pos.y + 8.0),
                    Org::Center,
                    0.7,
                    Color::WHITE,
                );
            }
        }
        // BEGIN!
        let pos = v.at(Vec2::new(cx, 480.0), Vec2::new(cx, 210.0));
        border_button(p, pos, 120.0, 30.0, self.sel == 3, v.tick, false, false);
        p.text_outlined("BEGIN!", pos, (0.5, 0.5), 2.0, button_color(self.sel == 3));
    }
}

// ---- team select -------------------------------------------------------

pub struct TeamSelect {
    started: bool,
    moved_at: Vec<u64>,
}

impl TeamSelect {
    pub fn new(s: &mut Session) -> Self {
        let n = s.joined_count().max(2);
        // Screenshots: `TFP_TEAMS=n --screen teams` (3-6 team layouts),
        // with the players dealt round-robin.
        if let Some(t) = std::env::var("TFP_TEAMS")
            .ok()
            .and_then(|v| v.parse::<u8>().ok())
        {
            s.versus.team_count = t;
            let joined = s.joined();
            let mut k = 0;
            for (i, &j) in joined.iter().enumerate() {
                if j {
                    s.versus.teams[i] = Team::from_index(k % t.clamp(2, 6));
                    k += 1;
                }
            }
        }
        s.versus.team_count = s.versus.team_count.clamp(2, 6).min(n as u8).max(2);
        Self {
            started: false,
            moved_at: vec![0; 12],
        }
    }

    /// Team column x positions and the neutral x.
    fn columns(w: f32, t: u8) -> (Vec<f32>, f32) {
        let cx = w / 2.0;
        if t <= 2 {
            let (l, r) = if w > 320.0 {
                (114.0, 306.0)
            } else {
                (64.0, 256.0)
            };
            return (vec![l, r], cx);
        }
        // 3-6 teams: spread over the width; neutral tags sit at the centre.
        let margin = 40.0;
        let xs = (0..t)
            .map(|k| margin + (w - 2.0 * margin) * k as f32 / (t as f32 - 1.0))
            .collect();
        (xs, cx)
    }

    /// Player row y and text scale. With 3-6 teams the rows start under
    /// the banner band; 12 rows then use scale 1 so the last one clears
    /// the FIGHT! banner and the button guide.
    fn row_y(cap: usize, teams: u8, i: usize) -> (f32, f32) {
        let i = i as f32;
        match (cap, teams > 2) {
            (0..=4, false) => (135.0 + 18.0 * i, 2.0),
            (0..=4, true) => (100.0 + 20.0 * i, 2.0),
            (5..=8, false) => (65.0 + 20.0 * i, 2.0),
            (5..=8, true) => (80.0 + 17.0 * i, 2.0),
            (_, false) => (42.0 + 15.0 * i, 1.5),
            (_, true) => (80.0 + 12.0 * i, 1.0),
        }
    }
}

impl Screen for TeamSelect {
    fn id(&self) -> ScreenId {
        ScreenId::TeamSelect
    }

    fn section(&self) -> f32 {
        3.0
    }

    fn title(&self) -> Option<&'static str> {
        Some("teams")
    }

    fn guides(&self) -> Vec<(&'static str, u16)> {
        vec![("BALANCE", b::ALT)]
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        if !c.ready || self.started {
            return None;
        }
        if c.mi.p(b::BACK) {
            c.play("ui_clickBack", None);
            return Some(Go::To(ScreenId::VersusOptions));
        }
        let joined = c.s.joined();
        if c.mi.p(b::ALT) {
            c.s.versus.balance_teams(&joined);
            c.play("ui_click", None);
        }
        let t = c.s.versus.team_count.clamp(2, 6);
        let valid = c.s.versus.teams_valid(&joined);
        #[allow(clippy::needless_range_loop)]
        for i in 0..c.s.capacity {
            if !joined[i] {
                continue;
            }
            let Some(Owner::Local(d)) = c.s.slots[i].owner.clone() else {
                continue;
            };
            let Some(ds) = c.mi.dev(d).cloned() else {
                continue;
            };
            let cur = c.s.versus.teams[i].index();
            let next = if ds.p(b::LEFT) {
                Some(match cur {
                    None => 0,
                    Some(k) => k.saturating_sub(1),
                })
            } else if ds.p(b::RIGHT) {
                Some(match cur {
                    None => t - 1,
                    Some(k) => (k + 1).min(t - 1),
                })
            } else {
                None
            };
            if let Some(k) = next
                && Some(k) != cur
            {
                c.s.versus.teams[i] = Team::from_index(k);
                self.moved_at[i] = c.tick;
                c.play("ui_move2", Some(c.w() / 2.0));
            }
            if valid && (ds.p(b::CONFIRM) || ds.p(b::START)) {
                self.started = true;
                c.play("ui_startGameType", None);
                return Some(Go::Fade(ScreenId::Map));
            }
        }
        let _ = Dev::Keyboard(0);
        None
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let s = v.s;
        let t = s.versus.team_count.clamp(2, 6);
        let (cols, neutral) = Self::columns(v.w, t);
        let small = t > 2;
        let banner_scale = if small { 0.5 } else { 1.0 };
        // 3-6 teams: half-size banners in a band under the screen title,
        // the player rows below it (row_y).
        let by = if small { 46.0 } else { 80.0 };
        for (k, &x) in cols.iter().enumerate() {
            let from = Vec2::new(
                if (k as f32) < t as f32 / 2.0 {
                    -40.0
                } else {
                    v.w + 40.0
                },
                by - 30.0,
            );
            let pos = v.at(from, Vec2::new(x, by));
            let team = Team::from_index(k as u8);
            let (name, tint) = match k {
                0 => ("teamA2x", Color::WHITE),
                1 => ("teamB2x", Color::WHITE),
                _ => ("teamA2x", Color::hex(team.colors().1)),
            };
            p.m(name, pos, Org::Center, banner_scale, tint);
            if t > 2 {
                p.text_outlined(
                    team.name(),
                    pos + Vec2::new(0.0, 24.0),
                    (0.5, 0.5),
                    1.0,
                    Color::hex(team.colors().1),
                );
            }
        }
        let joined = s.joined();
        #[allow(clippy::needless_range_loop)]
        for i in 0..s.capacity {
            if !joined[i] {
                continue;
            }
            let (y, scale) = Self::row_y(s.capacity, t, i);
            let team = s.versus.teams[i];
            let tx = team.index().map_or(neutral, |k| cols[k as usize]);
            let from = Vec2::new(if i % 2 == 0 { -40.0 } else { v.w + 40.0 }, y);
            let pos = lerp(from, Vec2::new(tx, y), v.k);
            let sl = &s.slots[i];
            let a = &p.a.archers[sl.archer.min(p.a.archers.len() - 1)];
            let c = match team.index() {
                None if (v.tick / 6).is_multiple_of(2) => rgba(a.color_a),
                None => rgba(a.color_b),
                Some(_) => Color::hex(team.colors().1),
            };
            p.text_outlined(&format!("P{}", i + 1), pos, (0.5, 0.5), scale, c);
            let mk = 16.0 + 2.0 * sine(v.tick, 60.0, 0.0);
            if team.index() != Some(0) {
                p.text_outlined(
                    "<",
                    pos + Vec2::new(-mk, 0.0),
                    (0.5, 0.5),
                    scale,
                    Color::WHITE,
                );
            }
            if team.index() != Some(t - 1) {
                p.text_outlined(
                    ">",
                    pos + Vec2::new(mk, 0.0),
                    (0.5, 0.5),
                    scale,
                    Color::WHITE,
                );
            }
        }
        if s.versus.teams_valid(&joined) {
            // On 320 the banner reaches the button guide's corner: higher.
            let y = if v.w > 320.0 { 225.0 } else { 211.0 };
            let pos = Vec2::new(v.cx(), y);
            p.m("portraits/readyBanner", pos, Org::Center, 1.0, Color::WHITE);
            p.text_outlined("FIGHT!", pos, (0.5, 0.5), 2.0, Color::WHITE);
        }
    }
}

// ---- variants ------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum VItem {
    DisableAll,
    Randomize,
    Tournament,
    TeamTournament,
    Preset(usize),
    Variant(usize),
}

struct Placed {
    item: VItem,
    pos: Vec2,
}

pub struct VariantsMenu {
    items: Vec<Placed>,
    headings: Vec<(String, f32, bool)>,
    sel: usize,
    cam: (f32, f32, u64),
    picker: Option<usize>,
    explain: bool,
    last_set: Option<bool>,
    toggled_at: (usize, u64),
    height: f32,
}

impl VariantsMenu {
    pub fn new(s: &mut Session, a: &UiAssets) -> Self {
        let x0 = (s.width() - 320.0) / 2.0;
        let avail = s.variant_availability(a.dark_world);
        let mut items = Vec::new();
        let actions = if a.dark_world {
            vec![
                VItem::DisableAll,
                VItem::Randomize,
                VItem::Tournament,
                VItem::TeamTournament,
            ]
        } else {
            vec![VItem::DisableAll, VItem::Randomize, VItem::Tournament]
        };
        let n = actions.len() as f32;
        for (i, it) in actions.into_iter().enumerate() {
            items.push(Placed {
                item: it,
                pos: Vec2::new(x0 + 160.0 + (i as f32 - (n - 1.0) / 2.0) * 40.0, 50.0),
            });
        }
        for i in 0..5 {
            items.push(Placed {
                item: VItem::Preset(i),
                pos: Vec2::new(x0 + 160.0 + (i as f32 - 2.0) * 30.0, 70.0),
            });
        }
        let mut headings = Vec::new();
        let mut y = 90.0;
        // Usable sections in order, then the locked expansion section.
        let mut groups: Vec<(String, Vec<usize>, bool)> = SECTIONS
            .iter()
            .enumerate()
            .map(|(si, name)| {
                let list: Vec<usize> = (0..VARIANTS.len())
                    .filter(|&i| VARIANTS[i].section == si && avail.usable(&VARIANTS[i]))
                    .collect();
                (name.to_string(), list, false)
            })
            .collect();
        if !a.dark_world {
            let locked: Vec<usize> = (0..VARIANTS.len())
                .filter(|&i| VARIANTS[i].dark_world && avail.visible(&VARIANTS[i]))
                .collect();
            groups.push(("DARK WORLD EXPANSION".into(), locked, true));
        }
        for (name, list, locked) in groups.into_iter() {
            if list.is_empty() {
                continue;
            }
            headings.push((name, y, locked));
            y += 20.0;
            for (k, &vi) in list.iter().enumerate() {
                if k > 0 && k % 7 == 0 {
                    y += 26.0;
                }
                items.push(Placed {
                    item: VItem::Variant(vi),
                    pos: Vec2::new(x0 + 70.0 + 26.0 * (k % 7) as f32, y),
                });
            }
            y += 26.0 + 20.0;
        }
        Self {
            items,
            headings,
            sel: 0,
            cam: (0.0, 0.0, 0),
            picker: None,
            explain: false,
            last_set: None,
            toggled_at: (usize::MAX, 0),
            height: y,
        }
    }

    fn cam_y(&self, tick: u64) -> f32 {
        let (from, to, t) = self.cam;
        lerp(
            Vec2::splat(from),
            Vec2::splat(to),
            cube_out(tick.saturating_sub(t) as f32 / 10.0),
        )
        .x
    }

    fn locked(&self, a: &UiAssets, i: usize) -> bool {
        matches!(self.items[i].item, VItem::Variant(v) if VARIANTS[v].dark_world && !a.dark_world)
            || (self.items[i].item == VItem::TeamTournament && !a.dark_world)
    }

    fn select(&mut self, c: &mut Ctx, n: usize) {
        if n == self.sel {
            return;
        }
        self.sel = n;
        self.explain = false;
        c.play("ui_move1", None);
        let target = (self.items[n].pos.y - 120.0)
            .max(0.0)
            .min((self.height - 240.0).max(0.0));
        self.cam = (self.cam_y(c.tick), target, c.tick);
        // Painting: holding Confirm applies the last toggle.
        if let (Some(on), VItem::Variant(v)) = (self.last_set, self.items[n].item)
            && c.mi.h(b::CONFIRM)
            && !self.locked(c.a, n)
            && c.s.versus.variants.value(v) != on
        {
            let joined = c.s.joined();
            c.s.versus.variants.set(v, on, &joined);
            self.toggled_at = (n, c.tick);
        }
    }

    fn vertical(&self, dir: f32) -> Option<usize> {
        let cur = &self.items[self.sel];
        let rows: Vec<f32> = {
            let mut ys: Vec<f32> = self.items.iter().map(|i| i.pos.y).collect();
            ys.dedup();
            ys
        };
        let ri = rows.iter().position(|&y| y == cur.pos.y)?;
        let next = ri as i32 + dir as i32;
        let ny = *rows.get(usize::try_from(next).ok()?)?;
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.pos.y == ny)
            .min_by(|(_, a), (_, b)| {
                (a.pos.x - cur.pos.x)
                    .abs()
                    .total_cmp(&(b.pos.x - cur.pos.x).abs())
            })
            .map(|(k, _)| k)
    }
}

impl Screen for VariantsMenu {
    fn id(&self) -> ScreenId {
        ScreenId::Variants
    }

    fn section(&self) -> f32 {
        3.0
    }

    fn title(&self) -> Option<&'static str> {
        Some("variants")
    }

    fn guides(&self) -> Vec<(&'static str, u16)> {
        let mut g = vec![("SELECT", b::CONFIRM), ("DONE", b::BACK)];
        if let VItem::Variant(v) = self.items[self.sel].item {
            if VARIANTS[v].per_player {
                g.push(("PLAYERS", b::ALT));
            }
            if !VARIANTS[v].description.is_empty() {
                g.push(("EXPLAIN", b::ALT2));
            }
        }
        g
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        if !c.ready {
            return None;
        }
        let joined = c.s.joined();
        // Per-player picker.
        if let Some(cur) = &mut self.picker {
            let VItem::Variant(v) = self.items[self.sel].item else {
                self.picker = None;
                return None;
            };
            let slots: Vec<usize> = (0..c.s.capacity).filter(|&i| joined[i]).collect();
            let k = slots.iter().position(|&s| s == *cur).unwrap_or(0);
            if c.mi.p(b::BACK) || c.mi.p(b::ALT) {
                self.picker = None;
                c.play("ui_clickBack", None);
            } else if c.mi.p(b::LEFT) && k > 0 {
                *cur = slots[k - 1];
                c.play("ui_move1", None);
            } else if c.mi.p(b::RIGHT) && k + 1 < slots.len() {
                *cur = slots[k + 1];
                c.play("ui_move1", None);
            } else if c.mi.p(b::CONFIRM) {
                let on = !c.s.versus.variants.slot(v, *cur);
                c.s.versus.variants.set_slot(v, *cur, on);
                c.play(
                    if on {
                        "ui_subclickOn"
                    } else {
                        "ui_subclickOff"
                    },
                    None,
                );
            }
            return None;
        }
        if c.mi.p(b::BACK) {
            c.play("ui_clickBack", None);
            return Some(Go::To(ScreenId::VersusOptions));
        }
        if c.mi.p(b::RIGHT) && self.sel + 1 < self.items.len() {
            self.select(c, self.sel + 1);
        } else if c.mi.p(b::LEFT) && self.sel > 0 {
            self.select(c, self.sel - 1);
        } else if c.mi.p(b::DOWN) {
            if let Some(n) = self.vertical(1.0) {
                self.select(c, n);
            }
        } else if c.mi.p(b::UP)
            && let Some(n) = self.vertical(-1.0)
        {
            self.select(c, n);
        }
        let it = self.items[self.sel].item;
        if c.mi.p(b::ALT2) && matches!(it, VItem::Variant(v) if !VARIANTS[v].description.is_empty())
        {
            self.explain = !self.explain;
        }
        if c.mi.p(b::ALT) {
            match it {
                VItem::Variant(v) if VARIANTS[v].per_player && !self.locked(c.a, self.sel) => {
                    self.picker = (0..c.s.capacity).find(|&i| joined[i]);
                    c.play("ui_click", None);
                }
                VItem::Preset(k) if c.s.presets[k].is_some() => {
                    c.s.presets[k] = None;
                    c.play("ui_clickBack", None);
                }
                _ => {}
            }
        }
        if c.mi.p(b::CONFIRM) {
            if self.locked(c.a, self.sel) {
                c.play("ui_invalid", None);
                return None;
            }
            let avail = c.s.variant_availability(c.a.dark_world);
            let set = &mut c.s.versus.variants;
            match it {
                VItem::DisableAll => {
                    set.clear_all();
                    c.play("ui_clickBack", None);
                }
                VItem::Randomize => {
                    let mut r = c.s.rng;
                    c.s.versus.variants.randomize(avail, &joined, &mut r);
                    c.s.rng = r;
                    c.play("ui_click", None);
                }
                VItem::Tournament | VItem::TeamTournament => {
                    set.apply_tournament(it == VItem::TeamTournament, &joined);
                    c.play("ui_click", None);
                }
                VItem::Preset(k) => {
                    match c.s.presets[k].clone() {
                        None => c.s.presets[k] = Some(c.s.versus.variants.to_titles()),
                        Some(t) => c.s.versus.variants.load_titles(&t, &joined),
                    }
                    c.play("ui_click", None);
                }
                VItem::Variant(v) => {
                    let on = !set.value(v);
                    set.set(v, on, &joined);
                    self.last_set = Some(on);
                    self.toggled_at = (self.sel, c.tick);
                    c.play(
                        if on {
                            "ui_subclickOn"
                        } else {
                            "ui_subclickOff"
                        },
                        None,
                    );
                }
            }
        }
        None
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let s = v.s;
        let a = p.a;
        let cam = self.cam_y(v.tick).round();
        let slide = (1.0 - v.k) * 300.0;
        let fade = |y: f32| (1.0 - (cam + 40.0 - y) / 6.0).clamp(0.0, 1.0);
        for (name, y, locked) in &self.headings {
            let yy = y - cam;
            let al = fade(*y);
            if al > 0.0 {
                let c = if *locked {
                    pal::GOLD
                } else {
                    Color::hex(0x6CD86C)
                };
                p.text_outlined(
                    name,
                    Vec2::new((v.w - 320.0) / 2.0 + 62.0 + slide, yy),
                    (0.0, 0.5),
                    1.0,
                    c * al,
                );
            }
        }
        for (i, it) in self.items.iter().enumerate() {
            let pos = it.pos - Vec2::new(-slide, cam);
            let al = fade(it.pos.y);
            if al <= 0.0 || pos.y > 250.0 {
                continue;
            }
            let sel = i == self.sel;
            let locked = self.locked(a, i);
            let (icon, active) = match it.item {
                VItem::DisableAll => ("variants/disableAll".to_string(), false),
                VItem::Randomize => ("variants/randomize".into(), false),
                VItem::Tournament => (
                    "variants/tournamentRules1v1".into(),
                    s.versus.variants.is_tournament(false),
                ),
                VItem::TeamTournament => (
                    "variants/tournamentRules2v2".into(),
                    s.versus.variants.is_tournament(true),
                ),
                VItem::Preset(k) => {
                    let l = (b'A' + k as u8) as char;
                    (
                        if s.presets[k].is_some() {
                            format!("variants/presetOn{l}")
                        } else {
                            format!("variants/presetOff{l}")
                        },
                        false,
                    )
                }
                VItem::Variant(vi) => (VARIANTS[vi].icon(), s.versus.variants.value(vi)),
            };
            let pop = if self.toggled_at.0 == i && v.tick.saturating_sub(self.toggled_at.1) < 10 {
                0.2
            } else {
                0.0
            };
            let scale = if sel { 1.2 } else { 1.0 } + pop;
            let tint = if locked {
                Color::WHITE * 0.4
            } else {
                Color::WHITE
            } * al;
            p.m_outlined(&icon, pos, Org::Center, scale, tint);
            let pulse = if sel {
                sine(v.tick, 60.0, 0.0).round()
            } else {
                0.0
            };
            // ui.md 8.2: the selected item's box is D8F878 when on and
            // 3CBCFC when off (pulsing +/-1 px); unselected items show a
            // D8F878 box only while on.
            let frame_c = if active { pal::SELECTED } else { pal::IDLE };
            if sel || active {
                let sz = 20.0 + 2.0 * pulse;
                p.list.hollow_rect(
                    (pos.x - sz / 2.0).floor(),
                    (pos.y - sz / 2.0).floor(),
                    sz,
                    sz,
                    frame_c * al,
                );
            }
            // Per-player banners.
            if let VItem::Variant(vi) = it.item
                && VARIANTS[vi].per_player
                && active
            {
                let joined: Vec<usize> = (0..s.capacity).filter(|&k| s.slots[k].joined).collect();
                let step = if s.capacity > 8 { 2.0 } else { 5.0 };
                let x0 = pos.x - (joined.len() as f32 - 1.0) * step / 2.0;
                for (n, &k) in joined.iter().enumerate() {
                    if s.versus.variants.slot(vi, k) {
                        let col = rgba(a.archers[s.slots[k].archer.min(8)].color_a);
                        match a.m("variants/playerBanner") {
                            Some(bn) => p.tex(
                                bn,
                                Vec2::new(x0 + step * n as f32, pos.y + 12.0),
                                Org::Just(0.5, 0.0),
                                1.0,
                                col,
                            ),
                            None => p.rect(x0 + step * n as f32, pos.y + 12.0, 1.0, 3.0, col),
                        }
                    }
                }
            }
        }
        // Name bubble over the selection.
        let it = &self.items[self.sel];
        let pos = it.pos - Vec2::new(-slide, cam);
        let name = match it.item {
            VItem::DisableAll => "DISABLE ALL".to_string(),
            VItem::Randomize => "RANDOMIZE".into(),
            VItem::Tournament => "TOURNAMENT RULES".into(),
            VItem::TeamTournament => "TEAM TOURNAMENT RULES".into(),
            VItem::Preset(k) => {
                let l = (b'A' + k as u8) as char;
                if s.presets[k].is_some() {
                    format!("LOAD PRESET {l}")
                } else {
                    format!("STORE PRESET {l}")
                }
            }
            VItem::Variant(vi) => VARIANTS[vi].title(),
        };
        let tw = (p.measure(&name).x / 10.0).ceil() * 10.0 + 6.0;
        let by = pos.y - 22.0;
        let bx = (pos.x - tw / 2.0).clamp(2.0, v.w - tw - 2.0);
        match (
            a.m("variants/bubbleMiddle"),
            a.m("variants/bubbleEdge"),
            a.m("variants/bubbleHook"),
        ) {
            (Some(mid), Some(edge), hook) => {
                let h = mid.height() as f32;
                p.tex_ex(
                    mid,
                    Vec2::new(bx, by - h / 2.0),
                    Org::TopLeft,
                    Vec2::new(tw / mid.width() as f32, 1.0),
                    0.0,
                    Color::WHITE,
                    false,
                );
                p.tex(
                    edge,
                    Vec2::new(bx - edge.width() as f32, by - h / 2.0),
                    Org::TopLeft,
                    1.0,
                    Color::WHITE,
                );
                p.tex_ex(
                    edge,
                    Vec2::new(bx + tw, by - h / 2.0),
                    Org::TopLeft,
                    Vec2::ONE,
                    0.0,
                    Color::WHITE,
                    true,
                );
                if let Some(hk) = hook {
                    p.tex(
                        hk,
                        Vec2::new(pos.x, by + h / 2.0 - 1.0),
                        Org::Just(0.5, 0.0),
                        1.0,
                        Color::WHITE,
                    );
                }
            }
            _ => p.rect(bx, by - 6.0, tw, 12.0, Color::WHITE),
        }
        p.text(
            &name,
            Vec2::new(bx + tw / 2.0, by),
            (0.5, 0.5),
            1.0,
            Color::BLACK,
        );
        // Description.
        if self.explain
            && let VItem::Variant(vi) = it.item
        {
            let d = VARIANTS[vi].description;
            let y = (pos.y + 30.0).min(180.0);
            let w = p.measure(d).x + 12.0;
            p.panel(v.cx() - w / 2.0 - 10.0, y - 12.0, w + 20.0, 24.0);
            p.text_outlined(d, Vec2::new(v.cx(), y), (0.5, 0.5), 1.0, Color::WHITE);
        }
        // Per-player picker.
        if let (Some(cur), VItem::Variant(vi)) = (self.picker, it.item) {
            let w = match s.capacity {
                0..=4 => 160.0,
                5..=8 => 320.0,
                _ => 340.0,
            };
            let step = if s.capacity > 8 { 26.0 } else { 30.0 };
            let y = (pos.y + 26.0).min(190.0);
            p.panel(v.cx() - w / 2.0, y - 15.0, w, 46.0);
            p.text_outlined(
                &VARIANTS[vi].title(),
                Vec2::new(v.cx(), y - 4.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            );
            let joined: Vec<usize> = (0..s.capacity).filter(|&k| s.slots[k].joined).collect();
            let x0 = v.cx() - (joined.len() as f32 - 1.0) * step / 2.0;
            for (n, &k) in joined.iter().enumerate() {
                let x = x0 + step * n as f32;
                let arch = &a.archers[s.slots[k].archer.min(8)];
                let on = s.versus.variants.slot(vi, k);
                let col = rgba(if k == cur { arch.color_b } else { arch.color_a });
                p.text_outlined(
                    &format!("P{}", k + 1),
                    Vec2::new(x, y + 12.0),
                    (0.5, 0.5),
                    if on { 2.0 } else { 1.0 },
                    col,
                );
                if k == cur {
                    let bob = 2.0 * sine(v.tick, 30.0, 0.0);
                    p.m(
                        "variantPerPlayerArrow",
                        Vec2::new(x, y + 2.0 + bob),
                        Org::Center,
                        0.5,
                        Color::WHITE,
                    );
                }
            }
        }
    }
}
