//! Rollcall / archer select (ui.md 6, input.md 7 and 10.2).
//!
//! Layouts: 4 slots in one row (320 wide, 60x120 portraits, device names),
//! 8 slots in two rows of four (420 wide, 60x60 portraits, "P1".."P8"),
//! 12 slots in two rows of six (our design). Device binding: with 4 slots
//! devices own slots in device order like the original; with 8 or 12 an
//! unbound device presses Confirm/Start to take the lowest free slot.

use tfp_render::Color;
use tfp_render::glam::Vec2;

use super::assets::{Archer, UiAssets};
use super::gfx::{Org, Painter, cube_out, pal, rgba, sine};
use super::minput::{Dev, b};
use super::settings::{Costume, Owner};
use super::{Ctx, Go, Screen, ScreenId, Session, View};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    NotJoined,
    Joined,
    Done,
}

#[derive(Clone, Debug)]
struct Elem {
    state: State,
    /// Ticks of the last events (animations are computed from them).
    joined_at: Option<u64>,
    moved_at: Option<(u64, i32)>,
    shake_until: u64,
    flip_at: Option<u64>,
    lock_msg: bool,
}

pub struct Rollcall {
    elems: Vec<Elem>,
    /// Ticks until the automatic / forced start (`ForceStart`).
    start_in: Option<u32>,
    leaving: Option<u32>,
    banner_wiggle: u64,
    /// The original's binding: device i owns slot i.
    prebind: bool,
    /// Versus, Quest or Trials limits, title and next screen.
    mode: crate::coop_ui::RollcallMode,
}

/// Archer index `i` with costume `c`, as displayed.
fn look(a: &UiAssets, i: usize, c: Costume) -> &Archer {
    let base = &a.archers[i.min(a.archers.len() - 1)];
    match c {
        Costume::Normal => base,
        Costume::Alt => base.alt.as_deref().unwrap_or(base),
        Costume::Secret => base.secret.as_deref().unwrap_or(base),
    }
}

impl Rollcall {
    pub fn new(s: &mut Session, a: &UiAssets) -> Self {
        let n = s.capacity;
        let prebind = n <= 4;
        let mode = s.coop.mode;
        if mode != crate::coop_ui::RollcallMode::Versus {
            // Co-op rollcalls start empty (ui.md 6).
            for sl in s.slots.iter_mut() {
                sl.joined = false;
            }
        }
        let elems = (0..n)
            .map(|i| {
                let sl = &mut s.slots[i];
                // Secret costumes are not kept for unjoined slots.
                if !sl.joined && sl.costume == Costume::Secret {
                    sl.costume = Costume::Normal;
                }
                if !a.dark_world && sl.costume == Costume::Alt {
                    sl.costume = Costume::Normal;
                }
                Elem {
                    state: if sl.joined && sl.owner.is_some() {
                        State::Joined
                    } else {
                        State::NotJoined
                    },
                    joined_at: None,
                    moved_at: None,
                    shake_until: 0,
                    flip_at: None,
                    lock_msg: false,
                }
            })
            .collect();
        Self {
            elems,
            start_in: None,
            leaving: None,
            banner_wiggle: 0,
            prebind,
            mode,
        }
    }

    fn origin(cap: usize, i: usize) -> Vec2 {
        match cap {
            0..=4 => Vec2::new(52.0 + 72.0 * i as f32, 100.0),
            5..=8 => {
                if i < 4 {
                    Vec2::new(80.0 + 70.0 * i as f32, 75.0)
                } else {
                    Vec2::new(135.0 + 70.0 * (i - 4) as f32, 165.0)
                }
            }
            _ => {
                if i < 6 {
                    Vec2::new(35.0 + 70.0 * i as f32, 75.0)
                } else {
                    Vec2::new(35.0 + 70.0 * (i - 6) as f32, 165.0)
                }
            }
        }
    }

    fn tween_from(cap: usize, i: usize) -> Vec2 {
        let o = Self::origin(cap, i);
        if cap <= 4 {
            match i {
                0 => Vec2::new(-45.0, 100.0),
                1 => Vec2::new(o.x - 17.0, 330.0),
                2 => Vec2::new(o.x + 17.0, 330.0),
                _ => Vec2::new(365.0, 100.0),
            }
        } else {
            let row0 = if cap <= 8 { i < 4 } else { i < 6 };
            Vec2::new(o.x + if row0 { -420.0 } else { 420.0 }, o.y)
        }
    }

    /// Is (archer, costume) free for slot `me`? With up to 9 joined the
    /// original rule (archer index unique); beyond, the (archer, look)
    /// pair; once no unique look is left, duplicates (ui.md 18).
    fn available(s: &Session, a: &UiAssets, archer: usize, costume: Costume, me: usize) -> bool {
        let taken = |k: usize, c: Costume, pair: bool| {
            s.slots
                .iter()
                .enumerate()
                .take(s.capacity)
                .any(|(i, x)| i != me && x.joined && x.archer == k && (!pair || x.costume == c))
        };
        let usable: Vec<usize> = (0..a.archers.len())
            .filter(|&k| !Self::locked(a, k))
            .collect();
        // The original rule (archer index unique) while a free archer exists.
        if usable.iter().any(|&k| !taken(k, costume, false)) {
            return !taken(archer, costume, false);
        }
        // Then the (archer, look) pair; then duplicates (ui.md 18).
        if usable.iter().any(|&k| !taken(k, costume, true)) {
            return !taken(archer, costume, true);
        }
        true
    }

    /// The carousel moves when there is something to choose.
    fn can_change(s: &Session, a: &UiAssets) -> bool {
        a.archers.len() > s.joined_count() + 1 || s.joined_count() >= 8
    }

    /// Needs content this install lacks (Red without Dark World).
    fn locked(a: &UiAssets, archer: usize) -> bool {
        a.archers.get(archer).is_some_and(|x| x.dark_world) && !a.dark_world
    }

    /// Moves the carousel; `auto` steps (binding, character lock) also skip
    /// locked archers.
    fn step_archer(s: &mut Session, a: &UiAssets, me: usize, dir: i32, auto: bool) {
        let n = a.archers.len() as i32;
        let start = s.slots[me].archer as i32;
        let mut i = start;
        for _ in 0..n {
            i = (i + dir).rem_euclid(n);
            if Self::available(s, a, i as usize, s.slots[me].costume, me)
                && !(auto && Self::locked(a, i as usize))
            {
                s.slots[me].archer = i as usize;
                return;
            }
        }
    }

    fn joined_dev(s: &Session, d: Dev) -> Option<usize> {
        s.slots
            .iter()
            .take(s.capacity)
            .position(|x| x.owner == Some(Owner::Local(d)))
    }

    fn bind(c: &mut Ctx, i: usize, d: Dev) {
        let (name, icons) =
            c.mi.dev(d)
                .map(|x| (x.name.clone(), x.icons))
                .unwrap_or(("???".into(), super::minput::IconSet::Xb360));
        c.s.last_slot.insert(d, i);
        let sl = &mut c.s.slots[i];
        sl.owner = Some(Owner::Local(d));
        sl.dev_name = name;
        sl.icons = Some(icons);
    }

    /// The slot an unowned device takes: the one it used before in this
    /// session if still free, else the lowest free one (input.md 10.2).
    fn free_slot(s: &Session, cap: usize, d: Dev) -> Option<usize> {
        let free = |i: usize| s.slots[i].owner.is_none() && !s.slots[i].joined;
        s.last_slot
            .get(&d)
            .copied()
            .filter(|&i| i < cap && free(i))
            .or_else(|| (0..cap).find(|&i| free(i)))
    }

    fn unbind(s: &mut Session, i: usize) {
        let sl = &mut s.slots[i];
        sl.owner = None;
        sl.dev_name.clear();
        sl.icons = None;
    }

    fn leave_screen(&mut self, c: &mut Ctx) -> Option<Go> {
        for sl in c.s.slots.iter_mut() {
            sl.joined = false;
        }
        c.play("ui_clickBack", None);
        Some(Go::To(if self.mode.entered_from_coop_menu() {
            ScreenId::CoOp
        } else {
            ScreenId::Main
        }))
    }
}

impl Screen for Rollcall {
    fn id(&self) -> ScreenId {
        ScreenId::Rollcall
    }

    fn section(&self) -> f32 {
        1.0
    }

    fn title(&self) -> Option<&'static str> {
        Some(self.mode.title())
    }

    fn update(&mut self, c: &mut Ctx) -> Option<Go> {
        let cap = c.s.capacity;
        let w = c.w();
        if let Some(t) = &mut self.leaving {
            *t += 1;
            if *t >= 12 {
                return Some(Go::To(match self.mode {
                    crate::coop_ui::RollcallMode::Versus => ScreenId::VersusOptions,
                    crate::coop_ui::RollcallMode::Quest => ScreenId::QuestMap,
                    crate::coop_ui::RollcallMode::Trials => ScreenId::TrialsMap,
                    crate::coop_ui::RollcallMode::DarkWorld => ScreenId::DarkWorldMap,
                }));
            }
            return None;
        }
        if let Some(t) = &mut self.start_in {
            if *t == 0 {
                self.leaving = Some(0);
                return None;
            }
            *t -= 1;
        }
        if !c.ready {
            return None;
        }
        // Hot-plug: a joined/bound slot whose device vanished leaves.
        for i in 0..cap {
            if let Some(Owner::Local(d)) = c.s.slots[i].owner
                && c.mi.dev(d).is_none()
                && !matches!(d, Dev::Pad(id) if id >= 10_000)
            {
                c.s.slots[i].joined = false;
                Self::unbind(c.s, i);
                self.elems[i].state = State::NotJoined;
            }
        }
        // The original's binding: free slots go to unbound devices in order.
        if self.prebind {
            let devs: Vec<Dev> = {
                let mut pads: Vec<Dev> =
                    c.mi.devs
                        .iter()
                        .map(|d| d.dev)
                        .filter(|d| matches!(d, Dev::Pad(_)))
                        .collect();
                pads.extend(
                    c.mi.devs
                        .iter()
                        .map(|d| d.dev)
                        .filter(|d| matches!(d, Dev::Keyboard(_))),
                );
                pads
            };
            // AssignInputs: unjoined slots follow the device order.
            for i in 0..cap {
                if !c.s.slots[i].joined {
                    Self::unbind(c.s, i);
                }
            }
            for d in devs {
                if Self::joined_dev(c.s, d).is_none()
                    && let Some(i) = (0..cap).find(|&i| c.s.slots[i].owner.is_none())
                {
                    Self::bind(c, i, d);
                }
            }
        }
        // Unbound devices: join (8/12 slots) or leave the screen.
        let mut just_bound = Vec::new();
        let devs: Vec<(Dev, u16)> = c.mi.devs.iter().map(|d| (d.dev, d.pressed)).collect();
        for (d, pressed) in devs {
            if Self::joined_dev(c.s, d).is_some() {
                continue;
            }
            if pressed & b::BACK != 0 && self.start_in.is_none() {
                return self.leave_screen(c);
            }
            if !self.prebind
                && pressed & (b::CONFIRM | b::START) != 0
                && let Some(i) = Self::free_slot(c.s, cap, d)
            {
                Self::bind(c, i, d);
                just_bound.push(d);
                let sl = &c.s.slots[i];
                if !Self::available(c.s, c.a, sl.archer, sl.costume, i)
                    || Self::locked(c.a, sl.archer)
                {
                    Self::step_archer(c.s, c.a, i, 1, true);
                }
                c.play("ui_move2", Some(Self::origin(cap, i).x));
                self.elems[i].moved_at = Some((c.tick, 1));
            }
        }
        // Per element: its own device only.
        for i in 0..cap {
            let Some(Owner::Local(d)) = c.s.slots[i].owner.clone() else {
                continue;
            };
            let Some(ds) = c.mi.dev(d).cloned() else {
                continue;
            };
            if just_bound.contains(&d) {
                continue;
            }
            let x = Self::origin(cap, i).x;
            match self.elems[i].state {
                State::Done => {}
                State::NotJoined => {
                    if ds.p(b::BACK) {
                        if self.prebind {
                            return self.leave_screen(c);
                        }
                        Self::unbind(c.s, i);
                        c.play("ui_clickBack", Some(x));
                        continue;
                    }
                    if Self::can_change(c.s, c.a) {
                        for (bit, dir) in [(b::LEFT, -1), (b::RIGHT, 1)] {
                            if ds.p(bit) {
                                Self::step_archer(c.s, c.a, i, dir, false);
                                self.elems[i].moved_at = Some((c.tick, dir));
                                self.elems[i].lock_msg = false;
                                c.play("ui_move2", Some(x));
                            }
                        }
                    }
                    if ds.p(b::ALT) && c.a.dark_world {
                        let sl = &mut c.s.slots[i];
                        sl.costume = if sl.costume == Costume::Alt {
                            Costume::Normal
                        } else {
                            Costume::Alt
                        };
                        self.elems[i].flip_at = Some(c.tick);
                        c.play("ui_altCostumeShift", Some(x));
                    }
                    if ds.p(b::CONFIRM) || ds.p(b::START) {
                        let sl = c.s.slots[i].clone();
                        let joined = c.s.joined_count();
                        let max = self.mode.limits(cap).1;
                        if !Self::available(c.s, c.a, sl.archer, sl.costume, i) || joined >= max {
                            continue;
                        }
                        let needs_dw =
                            c.a.archers[sl.archer].dark_world || sl.costume == Costume::Alt;
                        if needs_dw && !c.a.dark_world {
                            self.elems[i].lock_msg = true;
                            self.elems[i].shake_until = c.tick + 30;
                            c.play("ui_invalid", Some(x));
                            c.rumble(d, 1.0, 20);
                            continue;
                        }
                        if ds.h(b::ALT2)
                            && sl.costume == Costume::Normal
                            && c.a.archers[sl.archer].secret.is_some()
                        {
                            c.s.slots[i].costume = Costume::Secret;
                        }
                        c.s.slots[i].joined = true;
                        let e = &mut self.elems[i];
                        e.state = State::Joined;
                        e.joined_at = Some(c.tick);
                        e.shake_until = c.tick + 20;
                        self.banner_wiggle = c.tick;
                        let arch = look(c.a, sl.archer, c.s.slots[i].costume);
                        let n = c.a.voices.get(arch.voice).map_or(0, |v| v.ready.len());
                        let k = c.s.pick_ready(arch.voice, n);
                        let voice =
                            c.a.voices
                                .get(arch.voice)
                                .and_then(|v| v.ready.get(k).cloned());
                        c.play_arc(voice, x);
                        c.play("ui_characterLock", Some(x));
                        c.rumble(d, 1.0, 20);
                        // EnforceCharacterLock: unjoined slots showing it step on.
                        for j in 0..cap {
                            if j != i
                                && !c.s.slots[j].joined
                                && !Self::available(
                                    c.s,
                                    c.a,
                                    c.s.slots[j].archer,
                                    c.s.slots[j].costume,
                                    j,
                                )
                            {
                                Self::step_archer(c.s, c.a, j, 1, true);
                            }
                        }
                        if c.s.joined_count() == self.mode.limits(cap).1 {
                            // Auto-start; with 12 a 1-second grace (ui.md 18).
                            self.start_in = Some(if cap >= 12 { 60 } else { 20 });
                        }
                    }
                }
                State::Joined => {
                    if ds.p(b::BACK) {
                        self.start_in = None;
                        c.rumble(d, 0.5, 10);
                        let sl = &mut c.s.slots[i];
                        sl.joined = false;
                        if sl.costume == Costume::Secret {
                            sl.costume = Costume::Normal;
                        }
                        let arch = look(c.a, sl.archer, sl.costume);
                        let voice = c.a.voices.get(arch.voice).and_then(|v| v.deselect.clone());
                        c.play_arc(voice, x);
                        self.elems[i].state = State::NotJoined;
                        self.elems[i].shake_until = c.tick + 10;
                        if !self.prebind {
                            Self::unbind(c.s, i);
                        }
                        continue;
                    }
                    if (ds.p(b::CONFIRM) || ds.p(b::START))
                        && c.s.joined_count() >= self.mode.limits(cap).0
                    {
                        c.play("ui_startGameType", None);
                        for e in &mut self.elems {
                            e.state = State::Done;
                        }
                        self.leaving = Some(0);
                        return None;
                    }
                }
            }
        }
        let _ = w;
        None
    }

    fn draw(&self, p: &mut Painter, v: &View) {
        let s = v.s;
        let cap = s.capacity;
        let a = p.a;
        let small = cap > 4;
        let k = match self.leaving {
            Some(t) => v.k * (1.0 - cube_out(t as f32 / 12.0)),
            None => v.k,
        };
        for i in 0..cap {
            let e = &self.elems[i];
            let sl = &s.slots[i];
            let to = Self::origin(cap, i);
            let mut o = super::gfx::lerp(Self::tween_from(cap, i), to, k);
            if v.tick < e.shake_until {
                o.x += [-1.0, 1.0, 0.0][(v.tick % 3) as usize];
            }
            let joined = sl.joined;
            let arch = look(a, sl.archer, sl.costume);
            let ca = rgba(arch.color_a);
            let cb = rgba(arch.color_b);
            let bound = sl.owner.is_some();
            // Portrait (card flip on alt toggle).
            let flip = e.flip_at.map_or(1.0, |t| {
                let f = ((v.tick.saturating_sub(t)) as f32 / 10.0).min(1.0);
                (f * std::f32::consts::PI).cos().abs()
            });
            let name = if joined {
                &arch.joined
            } else {
                &arch.not_joined
            };
            let pic = if small {
                a.short_portrait(name)
            } else {
                a.m(name)
            };
            let (pw, ph) = if small { (60.0, 60.0) } else { (60.0, 120.0) };
            let frame_c = if joined { ca } else { Color::BLACK };
            p.rect(
                o.x - pw / 2.0 - 2.0,
                o.y - ph / 2.0 - 2.0,
                pw + 4.0,
                ph + 4.0,
                frame_c,
            );
            match pic {
                Some(pic) => {
                    let tint = if joined {
                        Color::WHITE
                    } else {
                        Color::rgb(140, 140, 140)
                    };
                    let tint = if bound || joined {
                        tint
                    } else {
                        Color::rgb(70, 70, 70)
                    };
                    p.tex_ex(
                        pic,
                        o,
                        Org::Center,
                        Vec2::new(flip.max(0.05), 1.0),
                        0.0,
                        tint,
                        false,
                    );
                }
                None => p.rect(o.x - pw / 2.0, o.y - ph / 2.0, pw, ph, ca * 0.5),
            }
            // Join flash.
            if let Some(t) = e.joined_at {
                let d = v.tick.saturating_sub(t);
                if d < 8 {
                    p.rect(
                        o.x - pw / 2.0,
                        o.y - ph / 2.0,
                        pw,
                        ph,
                        Color::WHITE * (1.0 - d as f32 / 8.0),
                    );
                }
            }
            // Gem.
            let gem_y = if small { 30.0 } else { 60.0 };
            let gem_id = format!("Gem{}", sl.archer.min(8));
            let gf = if joined {
                a.anim_frame(
                    &gem_id,
                    "on",
                    v.tick.saturating_sub(e.joined_at.unwrap_or(0)),
                )
            } else {
                0
            };
            if let Some((g, _)) = a.sprite_frame(&gem_id, gf) {
                let sc = if joined && !small { 1.5 } else { 1.0 };
                p.tex_outlined(g, o + Vec2::new(0.0, gem_y), Org::Center, sc, Color::WHITE);
            }
            if !joined {
                if !small {
                    // Name plate.
                    p.rect(o.x - pw / 2.0, o.y + 19.0, pw, 21.0, Color::BLACK * 0.75);
                    p.text(&arch.name0, o + Vec2::new(0.0, 26.0), (0.5, 0.5), 1.0, ca);
                    p.text(&arch.name1, o + Vec2::new(0.0, 34.0), (0.5, 0.5), 1.0, ca);
                }
                if bound && Rollcall::can_change(s, a) {
                    let tint = cb.lerp(Color::WHITE, 0.5);
                    let sx = 20.0 + 3.0 * sine(v.tick, 60.0, 0.0);
                    let bounce = |dir: i32| match e.moved_at {
                        Some((t, d)) if d == dir && v.tick.saturating_sub(t) < 10 => {
                            6.0 * (1.0 - v.tick.saturating_sub(t) as f32 / 10.0)
                        }
                        _ => 0.0,
                    };
                    if let Some(ar) = a.m("portraits/arrow") {
                        p.tex_ex(
                            ar,
                            o + Vec2::new(-sx - bounce(-1), gem_y),
                            Org::Center,
                            Vec2::ONE,
                            0.0,
                            tint,
                            true,
                        );
                        p.tex_ex(
                            ar,
                            o + Vec2::new(sx + bounce(1), gem_y),
                            Org::Center,
                            Vec2::ONE,
                            0.0,
                            tint,
                            false,
                        );
                    }
                }
                if bound && let Some(ic) = sl.icons {
                    let hint = ic.icon(b::CONFIRM, &s.options.keys);
                    p.icon(
                        &hint,
                        o + Vec2::new(
                            30.0,
                            if small { 15.0 } else { 40.0 } + 2.0 * sine(v.tick, 60.0, 0.0),
                        ),
                        Org::Center,
                    );
                }
            }
            if e.lock_msg {
                p.text_outlined(
                    "REQUIRES",
                    o + Vec2::new(0.0, -12.0),
                    (0.5, 0.5),
                    1.0,
                    Color::WHITE,
                );
                p.text_outlined("DARK WORLD", o, (0.5, 0.5), 1.0, pal::GOLD);
                p.text_outlined(
                    "EXPANSION",
                    o + Vec2::new(0.0, 10.0),
                    (0.5, 0.5),
                    1.0,
                    pal::GOLD,
                );
            }
            // Device badge + label.
            let label_c = if joined { cb } else { ca };
            if small {
                let tag = format!("P{}", i + 1);
                match sl.icons {
                    Some(ic) => p.badge(&ic.badge(i), i, o + Vec2::new(-15.0, -34.0)),
                    None => p.m(
                        "controls/generic/player1",
                        o + Vec2::new(-15.0, -34.0),
                        Org::Center,
                        1.0,
                        Color::WHITE * 0.4,
                    ),
                }
                p.text_outlined(
                    &tag,
                    o + Vec2::new(15.0, -34.0),
                    (0.5, 0.5),
                    2.0,
                    if bound { label_c } else { Color::GRAY },
                );
                if !bound {
                    p.text_outlined(
                        "PRESS",
                        o + Vec2::new(0.0, -6.0),
                        (0.5, 0.5),
                        1.0,
                        Color::WHITE,
                    );
                    p.text_outlined(
                        "TO JOIN",
                        o + Vec2::new(0.0, 4.0),
                        (0.5, 0.5),
                        1.0,
                        Color::WHITE,
                    );
                } else if matches!(sl.owner, Some(Owner::Remote { .. })) {
                    p.text_outlined(
                        &sl.dev_name,
                        o + Vec2::new(0.0, -22.0),
                        (0.5, 0.5),
                        1.0,
                        label_c,
                    );
                }
            } else {
                if let Some(ic) = sl.icons {
                    p.badge(&ic.badge(i), i, o + Vec2::new(0.0, 90.0));
                }
                let dn = if bound { sl.dev_name.as_str() } else { "" };
                p.text_outlined(dn, o + Vec2::new(0.0, 105.0), (0.5, 0.5), 1.0, label_c);
            }
        }
        // Ready banner.
        let banner = self.mode != crate::coop_ui::RollcallMode::Trials;
        if banner
            && s.joined_count() >= self.mode.limits(cap).0
            && self.elems.iter().all(|e| e.state != State::Done)
        {
            let t = v.tick.saturating_sub(self.banner_wiggle);
            let wig = if t < 20 {
                1.0 + 0.2 * (1.0 - t as f32 / 20.0)
            } else {
                1.0
            };
            let pos = Vec2::new(v.cx(), 225.0);
            if let Some(bn) = a.m("portraits/readyBanner") {
                p.tex(bn, pos, Org::Center, wig, Color::WHITE);
            }
            let mut icons: Vec<String> = Vec::new();
            for sl in s.slots.iter().take(cap).filter(|x| x.joined) {
                if let Some(ic) = sl.icons {
                    let n = ic.icon(b::CONFIRM, &s.options.keys);
                    if !icons.contains(&n) {
                        icons.push(n);
                    }
                }
            }
            let iw: Vec<f32> = icons
                .iter()
                .map(|n| a.m(n).map_or(10.0, |x| x.width() as f32))
                .collect();
            let tw = p.measure("FIGHT!").x * 2.0;
            let total = iw.iter().sum::<f32>() + 6.0 * icons.len() as f32 + tw;
            let mut x = pos.x - total / 2.0;
            for (n, w) in icons.iter().zip(iw) {
                p.icon(n, Vec2::new(x, pos.y), Org::Just(0.0, 0.5));
                x += w + 6.0;
            }
            p.text_outlined("FIGHT!", Vec2::new(x, pos.y), (0.0, 0.5), 2.0, Color::WHITE);
        }
    }
}
