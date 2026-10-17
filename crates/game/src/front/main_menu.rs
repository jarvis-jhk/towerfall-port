//! Intro (publisher logo), press-start and the main menu (ui.md 1, 3, 4).

use tfp_render::Color;
use tfp_render::glam::Vec2;

use super::gfx::{Org, Painter, back_out, cube_in, cube_in_out, cube_out, lerp, pal, sine};
use super::minput::{IconSet, b};
use super::{Ctx, Go, LogoMode, Screen, ScreenId, Session, View};

// ---- intro ---------------------------------------------------------------

/// One flying letter of the publisher logo.
struct Letter {
    slot: Vec2,
    start: u32,
    dur: u32,
    spin: f32,
    out_spin: f32,
}

/// `IntroScene` (ui.md 1.1): 14 letters fly in, pop, fly out; ~7.5 s.
pub struct Intro {
    t: u32,
    letters: Vec<Letter>,
    in_done: u32,
}

const DELAYS: [u32; 14] = [2, 2, 2, 14, 2, 2, 2, 2, 8, 2, 2, 2, 2, 14];
const WIDTHS: [f32; 14] = [8., 5., 6., 6., 8., 5., 5., 4., 4., 5., 5., 8., 4., 4.];

impl Intro {
    pub fn new() -> Self {
        let rows: [&[usize]; 3] = [&[0, 1, 2, 3], &[4, 5, 6, 7, 8], &[9, 10, 11, 12, 13]];
        let mut slots = [Vec2::ZERO; 14];
        for (r, row) in rows.iter().enumerate() {
            let width: f32 =
                row.iter().map(|&i| WIDTHS[i] * 4.0).sum::<f32>() + 4.0 * (row.len() - 1) as f32;
            let mut x = 160.0 - width / 2.0;
            for &i in row.iter() {
                slots[i] = Vec2::new(x + WIDTHS[i] * 2.0, 90.0 + 30.0 * r as f32);
                x += WIDTHS[i] * 4.0 + 4.0;
            }
        }
        let mut seed = 0x1234_5678_u64;
        let mut rnd = |n: u64| super::variants::next_rand(&mut seed) % n;
        let mut t = 20;
        let mut letters = Vec::new();
        for i in 0..14 {
            let sign = if rnd(2) == 0 { 1.0 } else { -1.0 };
            letters.push(Letter {
                slot: slots[i],
                start: t,
                dur: 5 + rnd(56) as u32,
                spin: sign * (400.0 + rnd(800) as f32).to_radians(),
                out_spin: sign * (40.0 + rnd(360) as f32).to_radians(),
            });
            t += DELAYS[i] + if i == 8 { 7 } else { 0 };
        }
        let in_done = letters.iter().map(|l| l.start + l.dur).max().unwrap_or(0);
        Self {
            t: 0,
            letters,
            in_done,
        }
    }

    fn out_start(&self) -> u32 {
        self.in_done + 30 + 12 + 25 + 20 + 27
    }
}

impl Screen for Intro {
    fn id(&self) -> ScreenId {
        ScreenId::Intro
    }

    fn menu_background(&self) -> bool {
        false
    }

    fn tween_in(&self) -> u32 {
        0
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        self.t += 1;
        if self.t == 20 {
            c.play_jingle("mmg");
        }
        let end = self.out_start() + 20 + 30 + 60;
        if self.t >= end || (self.t > 5 && (c.mi.p(b::CONFIRM) || c.mi.p(b::START))) {
            return Some(Go::To(ScreenId::PressStart));
        }
        None
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let t = self.t.max(v.t.min(self.out_start() - 1));
        let w = p.w;
        let off = (w - 320.0) / 2.0;
        // Scrolling tile background + glow.
        let bg_alpha = if t < self.in_done {
            0.2 + 0.4 * (t as f32 / self.in_done as f32)
        } else {
            0.8
        };
        if let Some(tile) = p.a.m("mmg/bg") {
            let s = (t as f32 * 0.5) % 32.0;
            let mut y = -32.0 + s;
            while y < 240.0 {
                let mut x = -32.0 + s;
                while x < w {
                    p.tex(
                        tile,
                        Vec2::new(x, y),
                        Org::TopLeft,
                        1.0,
                        Color::WHITE * bg_alpha,
                    );
                    x += 32.0;
                }
                y += 32.0;
            }
        }
        p.m(
            "mmg/bgGlow",
            Vec2::new(off, 0.0),
            Org::TopLeft,
            1.0,
            Color::WHITE,
        );
        let out = self.out_start();
        let centre = Vec2::new(160.0 + off, 120.0);
        for (i, l) in self.letters.iter().enumerate() {
            let Some(img) = p.a.m(&format!("mmg/{i}")) else {
                continue;
            };
            let slot = l.slot + Vec2::new(off, 0.0);
            let (pos, scale, rot) = if t < l.start {
                continue;
            } else if t < out {
                let e = back_out((t - l.start) as f32 / l.dur as f32);
                let from = slot + (slot - centre);
                // Pop after everyone arrived.
                let pop = if t > self.in_done + 30 + i as u32 {
                    let k = ((t - self.in_done - 30 - i as u32) as f32 / 12.0).min(1.0);
                    0.5 * (1.0 - cube_in_out(k))
                } else {
                    0.0
                };
                (
                    lerp(from, slot, e),
                    4.0 * e.min(1.2) + pop,
                    l.spin * (1.0 - e),
                )
            } else {
                let e = cube_in(((t - out) as f32 / 20.0).min(1.0));
                (
                    lerp(slot, slot + (slot - centre), e),
                    4.0 * (1.0 - e),
                    l.out_spin * e,
                )
            };
            p.tex_ex(
                img,
                pos,
                Org::Center,
                Vec2::splat(scale.max(0.0)),
                rot,
                Color::WHITE,
                false,
            );
        }
        // Fades: from black at 20 over 30, to black after the letters left.
        let fade = if t < 20 {
            1.0
        } else if t < 50 {
            1.0 - cube_in((t - 20) as f32 / 30.0)
        } else if t > out + 30 {
            cube_in(((t - out - 30) as f32 / 60.0).min(1.0))
        } else {
            0.0
        };
        if fade > 0.0 {
            p.rect(0.0, 0.0, w, 240.0, Color::BLACK * fade);
        }
    }
}

// ---- press start -------------------------------------------------------

pub struct PressStart {
    pressed: Option<u32>,
    /// The first device's button art (a pad if any; ui.md 3).
    first: Option<IconSet>,
}

impl PressStart {
    pub fn new() -> Self {
        Self {
            pressed: None,
            first: None,
        }
    }
}

impl Screen for PressStart {
    fn id(&self) -> ScreenId {
        ScreenId::PressStart
    }

    fn logo(&self) -> LogoMode {
        if self.pressed.is_some() {
            LogoMode::Logo
        } else {
            LogoMode::WithLetters
        }
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        self.first = c.mi.first().map(|d| d.icons);
        if let Some(t) = &mut self.pressed {
            *t += 1;
            if *t >= 40 {
                c.s.main_from = ScreenId::PressStart;
                return Some(Go::To(ScreenId::Main));
            }
        } else if c.ready && (c.mi.p(b::CONFIRM) || c.mi.p(b::START)) {
            c.play("ui_startGame", None);
            self.pressed = Some(0);
        }
        None
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let pos = v.at(Vec2::new(v.cx(), 300.0), Vec2::new(v.cx(), 180.0));
        let (scale, visible) = match self.pressed {
            Some(t) => {
                let wig = (t as f32 * 0.6).sin() * (1.0 - t as f32 / 40.0);
                (1.0 - 0.3 * wig, (t / 3) % 2 == 0)
            }
            None => (1.0, true),
        };
        if !visible {
            return;
        }
        let first = self
            .first
            .or_else(|| v.s.slots.first().and_then(|s| s.icons));
        let keyboard = !matches!(first, Some(IconSet::Xb360 | IconSet::DualShock));
        let keys = &v.s.options.keys;
        if keyboard {
            let set = IconSet::Keyboard(0);
            let l = keys.layouts.first();
            let key = |k: Option<&sdl2::keyboard::Scancode>| {
                format!(
                    "controls/keyboard/{}",
                    k.and_then(|&s| super::minput::key_icon(s))
                        .unwrap_or("undefined")
                )
            };
            if let Some(l) = l {
                for (k, off) in [
                    (&l.up, (-55.0, -7.0)),
                    (&l.down, (-55.0, 8.0)),
                    (&l.left, (-68.0, 8.0)),
                    (&l.right, (-42.0, 8.0)),
                ] {
                    p.icon(&key(k.first()), pos + Vec2::new(off.0, off.1), Org::Center);
                }
            }
            p.text_outlined(
                "NAVIGATE",
                pos + Vec2::new(-55.0, 24.0),
                (0.5, 0.5),
                scale,
                Color::WHITE,
            );
            p.icon(&set.icon(b::BACK, keys), pos, Org::Center);
            p.text_outlined(
                "CANCEL",
                pos + Vec2::new(0.0, 14.0),
                (0.5, 0.5),
                scale,
                Color::WHITE,
            );
            p.icon(
                &set.icon(b::CONFIRM, keys),
                pos + Vec2::new(50.0, 0.0),
                Org::Center,
            );
            p.text_outlined(
                "CONFIRM",
                pos + Vec2::new(50.0, 14.0),
                (0.5, 0.5),
                scale,
                Color::WHITE,
            );
        } else {
            let icon = first.unwrap_or(IconSet::Xb360).icon(b::START, keys);
            let tw = p.measure("PRESS").x * 2.0;
            let iw = p.a.m(&icon).map_or(10.0, |s| s.width() as f32);
            let x0 = pos.x - (tw + 4.0 + iw) / 2.0;
            p.text_outlined(
                "PRESS",
                Vec2::new(x0, pos.y),
                (0.0, 0.5),
                2.0 * scale,
                Color::WHITE,
            );
            p.icon(&icon, Vec2::new(x0 + tw + 4.0, pos.y), Org::Just(0.0, 0.5));
        }
    }
}

// ---- main menu ---------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Item {
    Versus,
    CoOp,
    Trials,
    Archives,
    Workshop,
    /// Ours: online play over Warpzone (the online scene).
    Online,
    Options,
    Credits,
    Quit,
}

struct Button {
    item: Item,
    pos: Vec2,
    from: Vec2,
    big: bool,
    /// up, down, left, right
    links: [Option<Item>; 4],
}

pub struct MainMenu {
    buttons: Vec<Button>,
    sel: Item,
    lerp: Vec<f32>,
    selected_at: u64,
    confirmed: Option<(Item, u32)>,
    wide: bool,
}

impl MainMenu {
    pub fn new(s: &mut Session) -> Self {
        use Item::*;
        let wide = s.width() > 320.0;
        let b = |item, pos: (f32, f32), from: (f32, f32), big, links| Button {
            item,
            pos: Vec2::new(pos.0, pos.1),
            from: Vec2::new(from.0, from.1),
            big,
            links,
        };
        // ONLINE is our addition. On the 420 layout it heads the blade
        // stack; on 320 that spot is under the VERSUS label, so it sits
        // alone in the top-left corner (up from VERSUS) and the original
        // three blades keep their places.
        let blades = |up_top: Item| {
            vec![
                if wide {
                    b(
                        Online,
                        (-30.0, 174.0),
                        (-130.0, 174.0),
                        false,
                        [Some(up_top), Some(Options), None, Some(Versus)],
                    )
                } else {
                    b(
                        Online,
                        (-30.0, 14.0),
                        (-130.0, 14.0),
                        false,
                        [None, Some(Versus), None, Some(Versus)],
                    )
                },
                b(
                    Options,
                    (-30.0, 192.0),
                    (-130.0, 192.0),
                    false,
                    [
                        Some(if wide { Online } else { up_top }),
                        Some(Credits),
                        None,
                        Some(if wide { Versus } else { Trials }),
                    ],
                ),
                b(
                    Credits,
                    (-30.0, 210.0),
                    (-130.0, 210.0),
                    false,
                    [
                        Some(Options),
                        Some(Quit),
                        None,
                        Some(if wide { Versus } else { Trials }),
                    ],
                ),
                b(
                    Quit,
                    (-30.0, 228.0),
                    (-130.0, 228.0),
                    false,
                    [
                        Some(Credits),
                        None,
                        None,
                        Some(if wide { Versus } else { Trials }),
                    ],
                ),
            ]
        };
        let buttons = if wide {
            // 420-wide (5-12 archers): FIGHT! in the middle, CO-OP (Quest
            // stays 1-4 archers) on the right under the logo, TRIALS and
            // ARCHIVES below it; the blades on the left.
            let mut v = vec![
                b(
                    Versus,
                    (210.0, 160.0),
                    (210.0, 300.0),
                    true,
                    [None, None, Some(Options), Some(CoOp)],
                ),
                // Below the logo's DARK WORLD and clear of the right edge
                // (its skulls orbit ~44 px out).
                b(
                    CoOp,
                    (372.0, 150.0),
                    (572.0, 150.0),
                    true,
                    [None, Some(Archives), Some(Versus), None],
                ),
                b(
                    Trials,
                    (310.0, 210.0),
                    (310.0, 300.0),
                    false,
                    [Some(Versus), None, Some(Versus), Some(Archives)],
                ),
                b(
                    Archives,
                    (375.0, 210.0),
                    (375.0, 300.0),
                    false,
                    [Some(CoOp), None, Some(Trials), None],
                ),
            ];
            v.extend(blades(Versus));
            v
        } else {
            let mut v = vec![
                b(
                    Versus,
                    (100.0, 140.0),
                    (-160.0, 120.0),
                    true,
                    [Some(Online), Some(Options), None, Some(CoOp)],
                ),
                b(
                    CoOp,
                    (220.0, 140.0),
                    (480.0, 120.0),
                    true,
                    [None, Some(Archives), Some(Versus), None],
                ),
                b(
                    Trials,
                    (130.0, 210.0),
                    (130.0, 300.0),
                    false,
                    [Some(Versus), None, Some(Options), Some(Archives)],
                ),
                b(
                    Archives,
                    (200.0, 210.0),
                    (200.0, 300.0),
                    false,
                    [Some(CoOp), None, Some(Trials), Some(Workshop)],
                ),
                b(
                    Workshop,
                    (270.0, 210.0),
                    (270.0, 300.0),
                    false,
                    [Some(CoOp), None, Some(Archives), None],
                ),
            ];
            v.extend(blades(Versus));
            v
        };
        let sel = match s.main_from {
            ScreenId::Options | ScreenId::ControlOptions => Options,
            ScreenId::Archives => Archives,
            ScreenId::CoOp | ScreenId::QuestMap | ScreenId::DarkWorldMap => CoOp,
            ScreenId::TrialsMap => Trials,
            // Back from ONLINE (main_from = Main marks it).
            ScreenId::Main => Online,
            _ => Versus,
        };
        let n = buttons.len();
        let mut m = Self {
            buttons,
            sel,
            lerp: vec![0.0; n],
            selected_at: 0,
            confirmed: None,
            wide,
        };
        if let Some(i) = m.index(sel) {
            m.lerp[i] = 0.0;
        }
        m
    }

    fn index(&self, it: Item) -> Option<usize> {
        self.buttons.iter().position(|b| b.item == it)
    }

    fn label(&self, it: Item, players: usize) -> (&'static str, String) {
        match it {
            Item::Versus if self.wide => ("FIGHT!", format!("2-{players} ARCHERS")),
            Item::Versus => ("VERSUS", "2-4 ARCHERS".into()),
            Item::CoOp => ("CO-OP", "1-4 ARCHERS".into()),
            Item::Trials => ("TRIALS", String::new()),
            Item::Archives => ("ARCHIVES", String::new()),
            Item::Workshop => ("WORKSHOP", String::new()),
            Item::Online => ("ONLINE", String::new()),
            Item::Options => ("OPTIONS", String::new()),
            Item::Credits => ("CREDITS", String::new()),
            Item::Quit => ("QUIT", String::new()),
        }
    }
}

impl Screen for MainMenu {
    fn id(&self) -> ScreenId {
        ScreenId::Main
    }

    fn logo(&self) -> LogoMode {
        LogoMode::Logo
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        for (i, b) in self.buttons.iter().enumerate() {
            let on = b.item == self.sel && self.confirmed.is_none();
            self.lerp[i] = (self.lerp[i] + if on { 0.1 } else { -0.1 }).clamp(0.0, 1.0);
        }
        if let Some((it, t)) = &mut self.confirmed {
            *t += 1;
            let big = self.buttons.iter().any(|b| b.item == *it && b.big);
            if *t >= if big { 60 } else { 1 } {
                let it = *it;
                self.confirmed = None;
                return match it {
                    // Coming back to the main menu selects what was
                    // chosen last (ui.md 4.1).
                    Item::Versus => {
                        c.s.main_from = ScreenId::Rollcall;
                        c.s.coop.mode = crate::coop_ui::RollcallMode::Versus;
                        Some(Go::To(ScreenId::Rollcall))
                    }
                    Item::CoOp => {
                        c.s.main_from = ScreenId::CoOp;
                        Some(Go::To(ScreenId::CoOp))
                    }
                    Item::Trials => {
                        c.s.main_from = ScreenId::TrialsMap;
                        c.s.coop.mode = crate::coop_ui::RollcallMode::Trials;
                        Some(Go::To(ScreenId::Rollcall))
                    }
                    Item::Options => {
                        c.s.main_from = ScreenId::Options;
                        Some(Go::To(ScreenId::Options))
                    }
                    Item::Quit => Some(Go::Quit),
                    Item::Online => {
                        c.s.main_from = ScreenId::Main;
                        Some(Go::Online)
                    }
                    Item::Archives => {
                        c.s.main_from = ScreenId::Archives;
                        Some(Go::To(ScreenId::Archives))
                    }
                    _ => None,
                };
            }
            return None;
        }
        if !c.ready {
            return None;
        }
        if c.mi.p(b::BACK) {
            c.play("ui_clickBack", None);
            return Some(Go::To(ScreenId::PressStart));
        }
        let i = self.index(self.sel)?;
        if c.mi.p(b::CONFIRM) {
            let it = self.sel;
            match it {
                Item::Versus | Item::CoOp | Item::Trials => {
                    c.play("ui_selectVersus", None);
                    self.confirmed = Some((it, 0));
                }
                Item::Options | Item::Quit | Item::Online | Item::Archives => {
                    c.play("ui_click", None);
                    self.confirmed = Some((it, 0));
                }
                // Not part of the port yet.
                _ => c.play("ui_invalid", None),
            }
            return None;
        }
        // Right/Left/Up/Down, first match wins (ui.md 2.2).
        for (k, bit) in [(3, b::RIGHT), (2, b::LEFT), (0, b::UP), (1, b::DOWN)] {
            if c.mi.p(bit)
                && let Some(n) = self.buttons[i].links[k]
            {
                self.sel = n;
                self.selected_at = c.tick;
                c.play(
                    if n == Item::Versus {
                        "ui_moveVersus"
                    } else {
                        "ui_move1"
                    },
                    None,
                );
                break;
            }
        }
        None
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let players = v.s.capacity.max(4);
        for (i, bt) in self.buttons.iter().enumerate() {
            let sel = self.lerp[i];
            let pos = v.at(bt.from, bt.pos);
            let (title, sub) = self.label(bt.item, players);
            let blink =
                matches!(self.confirmed, Some((it, t)) if it == bt.item && (t / 2) % 2 == 1);
            if blink {
                continue;
            }
            if matches!(
                bt.item,
                Item::Online | Item::Options | Item::Credits | Item::Quit
            ) {
                // Blade button: slides right by 30 when selected.
                let x = pos.x + 30.0 * cube_out(sel);
                let tint = Color::WHITE.lerp(pal::BLADE, sel);
                p.m(
                    "bladeButton",
                    Vec2::new(x, pos.y),
                    Org::Just(0.0, 0.5),
                    1.0,
                    tint,
                );
                p.text(
                    title,
                    Vec2::new(x + 85.0, pos.y),
                    (1.0, 0.5),
                    1.0,
                    Color::BLACK,
                );
                continue;
            }
            let k = if bt.big { 1.0 } else { 0.4 };
            let lift = (-15.0 * k - 2.0 * sine(v.tick, 120.0, 0.0)) * sel;
            let ip = pos + Vec2::new(0.0, lift.round());
            match bt.item {
                Item::Versus => {
                    let spin = if bt.item == self.sel && v.tick >= self.selected_at {
                        let t = ((v.tick - self.selected_at) as f32 / 40.0).min(1.0);
                        std::f32::consts::TAU * cube_in_out(t)
                    } else {
                        0.0
                    };
                    if let Some(a) = p.a.g("versus/introArrow") {
                        for flip in [false, true] {
                            for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                                p.tex_ex(
                                    a,
                                    ip + Vec2::new(dx, dy),
                                    Org::Center,
                                    Vec2::ONE,
                                    spin,
                                    Color::BLACK,
                                    flip,
                                );
                            }
                        }
                        for flip in [false, true] {
                            p.tex_ex(a, ip, Org::Center, Vec2::ONE, spin, Color::WHITE, flip);
                        }
                    }
                }
                Item::CoOp => {
                    let f = p.a.anim_frame("CoopIcon", "0", v.tick);
                    if let Some((s, _)) = p.a.sprite_frame("CoopIcon", f) {
                        p.tex(s, ip, Org::Center, 1.0, Color::WHITE);
                    }
                    if let Some(sk) = p.a.m("coopSkulls") {
                        let ang = v.tick as f32 * std::f32::consts::PI / 360.0;
                        let alpha = 0.5 + 0.5 * sel;
                        let d = 30.0 + 5.0 * sine(v.tick, 120.0, 0.0) * alpha;
                        for j in 0..4 {
                            let a = ang + j as f32 * std::f32::consts::FRAC_PI_2;
                            let sp = ip + Vec2::new(a.cos(), a.sin()) * d;
                            p.tex(
                                sk.sub(j * 19, 0, 19, 19),
                                sp,
                                Org::Center,
                                1.0,
                                Color::WHITE * alpha,
                            );
                        }
                    }
                }
                Item::Trials => p.m_outlined("trialsModeClear", ip, Org::Center, 1.0, Color::WHITE),
                Item::Archives => {
                    p.m_outlined("archivesButton", ip, Org::Center, 1.0, Color::WHITE)
                }
                Item::Workshop => {
                    p.m_outlined("workshopButton", ip, Org::Center, 1.0, Color::WHITE)
                }
                _ => {}
            }
            let base = if bt.big { 2.0 } else { 1.0 };
            let scale = base + if bt.big { 1.0 } else { 0.0 } * cube_out(sel);
            let dim = if matches!(bt.item, Item::Versus) {
                1.0
            } else {
                0.85
            };
            p.text_outlined(
                title,
                pos + Vec2::new(0.0, 30.0 * k),
                (0.5, 0.5),
                scale.round(),
                Color::WHITE * dim,
            );
            if !sub.is_empty() && sel > 0.0 {
                p.text_outlined(
                    &sub,
                    pos + Vec2::new(0.0, 42.0 * k),
                    (0.5, 0.5),
                    1.0,
                    pal::SUBTITLE * sel,
                );
            }
        }
    }
}
