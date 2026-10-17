//! A tiny deterministic game for netcode tests: a 320×240 arena with up to 12
//! "archers" (dots) that run, jump, dodge-dash and shoot bouncing arrows,
//! with horizontal screen wrap and rounds every [`ROUND_FRAMES`] frames.
//! Integer math only, so every machine computes bit-identical states. It
//! follows the leave rule of docs/net.md: a disconnected slot is inert for
//! the rest of the round and removed at the round end.
//!
//! Also an input bot that behaves like a human (holds an input for a while,
//! then changes it) and [`ToyMachine`], one machine playing the toy over a
//! [`crate::NetSession`].

use crate::game::{NetGame, Request};
use crate::input::TfInput;
use crate::session::{InputStatus, NetEvent, NetSession};
use crate::wire::{DesyncReport, DoneReport};
use std::collections::BTreeMap;

const SUB: i32 = 256;
const W: i32 = 320 * SUB;
const H: i32 = 240 * SUB;
const R: i32 = 5 * SUB;
const MAX_ARROWS: usize = 48;
/// Frames per toy round (10 s at 60 Hz).
pub const ROUND_FRAMES: i32 = 600;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dot {
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    pub score: u32,
    pub deaths: u32,
    /// Plays (false once removed after a disconnect).
    pub alive: bool,
    /// Its machine left: inert now, removed at the round end.
    pub gone: bool,
    pub dodge: u8,
    pub prev_buttons: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Arrow {
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    pub owner: u8,
    pub life: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toy {
    /// Frames simulated (= sim frame of this state).
    pub frame: i32,
    pub round: u32,
    pub rng: u64,
    pub dots: Vec<Dot>,
    pub arrows: Vec<Arrow>,
    /// Desync injection (tests): this machine's state is corrupted when it
    /// reaches this frame (on every timeline, so re-simulation keeps it). Local configuration, not part of the checksum.
    pub corrupt_at: Option<i32>,
}

impl Toy {
    pub fn new(players: usize, seed: u64) -> Toy {
        let mut t = Toy {
            frame: 0,
            round: 0,
            rng: seed | 1,
            dots: vec![Dot::default(); players],
            arrows: Vec::new(),
            corrupt_at: None,
        };
        for i in 0..players {
            t.spawn(i);
            t.dots[i].alive = true;
        }
        t
    }

    fn next(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn spawn(&mut self, i: usize) {
        let x = R + (self.next() % (W - 2 * R) as u64) as i32;
        let y = R + (self.next() % (H / 2) as u64) as i32;
        let d = &mut self.dots[i];
        (d.x, d.y, d.vx, d.vy, d.dodge) = (x, y, 0, 0, 0);
    }

    fn step(&mut self, inputs: &[(TfInput, InputStatus)]) {
        for i in 0..self.dots.len() {
            let (inp, status) = inputs
                .get(i)
                .copied()
                .unwrap_or((TfInput::default(), InputStatus::Disconnected));
            let d = &mut self.dots[i];
            if status == InputStatus::Disconnected {
                d.gone = true; // input is zero from now on: inert
            }
            if !d.alive {
                continue;
            }
            let pressed = inp.buttons & !d.prev_buttons;
            d.prev_buttons = inp.buttons;
            let on_ground = d.y >= H - R - SUB;
            d.vx += inp.aim_x as i32 * 3;
            d.vy += 48;
            if pressed & TfInput::BTN_JUMP != 0 && on_ground {
                d.vy = -1400;
            }
            if pressed & TfInput::BTN_DODGE != 0 && d.dodge == 0 {
                d.vx += inp.aim_x as i32 * 30;
                d.vy += inp.aim_y as i32 * 30;
                d.dodge = 40;
            }
            d.dodge = d.dodge.saturating_sub(1);
            d.vx -= d.vx / 16;
            d.vy -= d.vy / 64;
            d.x = (d.x + d.vx).rem_euclid(W);
            d.y += d.vy;
            if d.y > H - R {
                d.y = H - R;
                d.vy = 0;
            }
            if d.y < R {
                d.y = R;
                d.vy = -d.vy / 2;
            }
            if pressed & TfInput::BTN_SHOOT != 0 && self.arrows.len() < MAX_ARROWS {
                let (ax, ay) = if inp.aim_x == 0 && inp.aim_y == 0 {
                    (TfInput::AXIS_MAX as i32, 0)
                } else {
                    (inp.aim_x as i32, inp.aim_y as i32)
                };
                let (x, y) = (d.x, d.y);
                self.arrows.push(Arrow {
                    x,
                    y,
                    vx: ax * 12,
                    vy: ay * 12,
                    owner: i as u8,
                    life: 120,
                });
            }
        }
        let mut hits = Vec::new();
        for a in self.arrows.iter_mut() {
            a.vy += 20;
            a.x = (a.x + a.vx).rem_euclid(W);
            a.y += a.vy;
            if a.y > H || a.y < 0 {
                a.vy = -a.vy * 3 / 4;
                a.y = a.y.clamp(0, H);
            }
            a.life = a.life.saturating_sub(1);
            for (j, d) in self.dots.iter().enumerate() {
                if j == a.owner as usize || !d.alive || d.dodge > 30 {
                    continue;
                }
                let (dx, dy) = ((a.x - d.x) as i64, (a.y - d.y) as i64);
                if dx * dx + dy * dy < (R as i64 * 2).pow(2) {
                    hits.push((a.owner as usize, j));
                    a.life = 0;
                    break;
                }
            }
        }
        self.arrows.retain(|a| a.life > 0);
        for (owner, victim) in hits {
            self.dots[owner].score += 1;
            self.dots[victim].deaths += 1;
            self.spawn(victim);
        }
        self.frame += 1;
        if self.frame % ROUND_FRAMES == 0 {
            self.round += 1;
            self.arrows.clear();
            for i in 0..self.dots.len() {
                if self.dots[i].gone {
                    self.dots[i].alive = false;
                } else {
                    self.spawn(i);
                }
            }
        }
        if self.corrupt_at == Some(self.frame) {
            // Persistent: positions heal on respawn, the PRNG and counters
            // do not.
            self.dots[0].x ^= 0x100;
            self.dots[0].deaths += 1000;
            self.rng = self.rng.rotate_left(7) ^ 1;
        }
    }

    /// FNV-1a over the whole state, two lanes.
    pub fn checksum(&self) -> u128 {
        let mut a: u64 = 0xcbf29ce484222325;
        let mut b: u64 = 0x84222325cbf29ce4;
        let mut eat = |v: u64| {
            for byte in v.to_le_bytes() {
                a = (a ^ byte as u64).wrapping_mul(0x100000001b3);
                b = (b ^ (byte as u64).wrapping_add(0x5f)).wrapping_mul(0x100000001b3);
            }
        };
        eat(self.frame as u64);
        eat(self.round as u64);
        eat(self.rng);
        for d in &self.dots {
            for v in [d.x, d.y, d.vx, d.vy] {
                eat(v as u32 as u64);
            }
            eat(((d.score as u64) << 32) | d.deaths as u64);
            eat(d.alive as u64 | (d.gone as u64) << 1 | (d.dodge as u64) << 8);
            eat(d.prev_buttons as u64);
        }
        eat(self.arrows.len() as u64);
        for ar in &self.arrows {
            for v in [ar.x, ar.y, ar.vx, ar.vy] {
                eat(v as u32 as u64);
            }
            eat(ar.owner as u64 | (ar.life as u64) << 8);
        }
        ((a as u128) << 64) | b as u128
    }

    /// Slots still in the game.
    pub fn alive(&self) -> usize {
        self.dots.iter().filter(|d| d.alive).count()
    }
}

impl NetGame for Toy {
    type Input = TfInput;

    fn advance(&mut self, inputs: &[(TfInput, InputStatus)]) {
        self.step(inputs);
    }

    fn checksum(&self) -> u128 {
        Toy::checksum(self)
    }
}

/// Plays a [`Toy`] from requests and remembers each frame's checksum.
pub struct ToyRunner {
    pub state: Toy,
    /// state.frame -> checksum (re-simulation overwrites).
    pub history: BTreeMap<i32, u128>,
}

impl ToyRunner {
    pub fn new(players: usize, seed: u64) -> ToyRunner {
        ToyRunner {
            state: Toy::new(players, seed),
            history: BTreeMap::new(),
        }
    }

    pub fn handle(&mut self, reqs: Vec<Request<Toy>>) {
        for r in reqs {
            match r {
                Request::Save { cell, frame } => {
                    debug_assert_eq!(frame, self.state.frame);
                    cell.save(frame, Some(self.state.clone()), Some(self.state.checksum()));
                }
                Request::Load { cell, frame } => {
                    let corrupt = self.state.corrupt_at;
                    self.state = cell.load().expect("saved state");
                    self.state.corrupt_at = corrupt;
                    debug_assert_eq!(frame, self.state.frame);
                }
                Request::Advance { frame, inputs, .. } => {
                    debug_assert_eq!(frame, self.state.frame);
                    self.state.step(&inputs);
                    self.history.insert(self.state.frame, self.state.checksum());
                }
            }
        }
        while self.history.len() > 8192 {
            self.history.pop_first();
        }
    }
}

/// A pseudo-human: holds an input for 4–40 frames, then picks another.
pub struct Bot {
    rng: u64,
    hold: u32,
    cur: TfInput,
}

impl Bot {
    pub fn new(seed: u64) -> Bot {
        Bot {
            rng: seed.wrapping_mul(0x9E3779B97F4A7C15) | 1,
            hold: 0,
            cur: TfInput::default(),
        }
    }

    fn next(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    pub fn input(&mut self) -> TfInput {
        if self.hold == 0 {
            let r = self.next();
            self.hold = 4 + (r % 37) as u32;
            const LEVELS: [i8; 5] = [-120, -64, 0, 64, 120];
            let b = (r >> 32) as u8;
            let mut buttons = 0;
            if b & 0b11 == 0 {
                buttons |= TfInput::BTN_JUMP;
            }
            if (b >> 2) & 0b11 == 0 {
                buttons |= TfInput::BTN_SHOOT;
            }
            if (b >> 4) & 0b111 == 0 {
                buttons |= TfInput::BTN_DODGE;
            }
            self.cur = TfInput {
                aim_x: LEVELS[(r % 5) as usize],
                aim_y: LEVELS[((r >> 8) % 5) as usize],
                buttons,
                reserved: 0,
            };
        }
        self.hold -= 1;
        self.cur
    }
}

/// How a match ended for a [`ToyMachine`].
#[derive(Clone, Debug)]
pub struct ToyResult {
    pub reports: Vec<DoneReport>,
    pub agreed: bool,
    pub desyncs: Vec<DesyncReport>,
}

/// One machine playing the toy game over a [`NetSession`]: starts the sim on
/// `MatchStarted`, feeds bot inputs for the local slots, fulfils requests,
/// and reports `finish_match` once frame `target` is confirmed.
pub struct ToyMachine {
    pub net: NetSession<Toy>,
    pub runner: Option<ToyRunner>,
    bots: BTreeMap<usize, Bot>,
    /// Frame at which the match ends (its checksum is compared).
    pub target: i32,
    pub result: Option<ToyResult>,
    pub aborted: Option<String>,
    pub salt: u64,
    /// Desync injection: corrupt this machine's state from this frame on.
    pub corrupt_at: Option<i32>,
    /// `Desync` events seen here.
    pub desyncs_seen: u64,
}

impl ToyMachine {
    pub fn new(net: NetSession<Toy>, target: i32, salt: u64) -> ToyMachine {
        ToyMachine {
            net,
            runner: None,
            bots: BTreeMap::new(),
            target,
            result: None,
            aborted: None,
            salt,
            corrupt_at: None,
            desyncs_seen: 0,
        }
    }

    /// Poll and handle events; returns them for logging. Call often.
    pub fn poll(&mut self) -> Vec<NetEvent> {
        self.net.poll();
        let mut out = Vec::new();
        while let Some(e) = self.net.next_event() {
            match &e {
                NetEvent::MatchStarted(ms) => {
                    let mut r = ToyRunner::new(ms.players.len(), ms.seed);
                    r.state.corrupt_at = self.corrupt_at;
                    self.runner = Some(r);
                    self.bots = self
                        .net
                        .local_slots()
                        .iter()
                        .map(|&s| {
                            (
                                s,
                                Bot::new(self.salt.wrapping_mul(31).wrapping_add(s as u64 + 1)),
                            )
                        })
                        .collect();
                    self.result = None;
                    self.aborted = None;
                }
                NetEvent::MatchEnded {
                    reports,
                    agreed,
                    desyncs,
                    ..
                } => {
                    self.result = Some(ToyResult {
                        reports: reports.clone(),
                        agreed: *agreed,
                        desyncs: desyncs.clone(),
                    })
                }
                NetEvent::MatchAborted { reason } => self.aborted = Some(reason.clone()),
                NetEvent::Desync { .. } => self.desyncs_seen += 1,
                _ => {}
            }
            out.push(e);
        }
        out
    }

    /// One sim step (call at 60 Hz).
    pub fn step(&mut self) {
        let Some(r) = self.runner.as_mut() else {
            return;
        };
        if self.net.current_match().is_none() {
            return;
        }
        for (&slot, bot) in self.bots.iter_mut() {
            let _ = self.net.add_local_input(slot, bot.input());
        }
        let reqs = self.net.advance();
        r.handle(reqs);
        if !self.net.is_spectator()
            && self.net.settled_frame() >= self.target
            && let Some(&c) = r.history.get(&self.target)
        {
            let _ = self.net.finish_match(self.target, c);
        }
    }

    /// Our checksum for the target frame, if simulated.
    pub fn target_checksum(&self) -> Option<u128> {
        self.runner.as_ref()?.history.get(&self.target).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(frames: i32, gone_from: Option<(usize, i32)>) -> Toy {
        let mut t = Toy::new(12, 42);
        let mut bots: Vec<Bot> = (0..12).map(|i| Bot::new(i + 1)).collect();
        for f in 0..frames {
            let inputs: Vec<_> = bots
                .iter_mut()
                .enumerate()
                .map(|(i, b)| match gone_from {
                    Some((s, g)) if s == i && f >= g => {
                        (TfInput::default(), InputStatus::Disconnected)
                    }
                    _ => (b.input(), InputStatus::Confirmed),
                })
                .collect();
            t.advance(&inputs);
        }
        t
    }

    #[test]
    fn deterministic() {
        assert_eq!(play(2000, None).checksum(), play(2000, None).checksum());
        assert_ne!(Toy::new(12, 1).checksum(), Toy::new(12, 2).checksum());
        let t = play(2000, None);
        assert!(
            t.dots.iter().map(|d| d.score).sum::<u32>() > 0,
            "arrows hit something"
        );
    }

    #[test]
    fn disconnected_slot_inert_then_removed() {
        let t = play(ROUND_FRAMES - 10, Some((3, 100)));
        assert!(
            t.dots[3].gone && t.dots[3].alive,
            "inert until the round ends"
        );
        let t = play(ROUND_FRAMES + 1, Some((3, 100)));
        assert!(!t.dots[3].alive, "removed at the round end");
        assert_eq!(t.alive(), 11);
    }

    #[test]
    fn corruption_changes_checksum() {
        let mut a = Toy::new(2, 5);
        let mut b = a.clone();
        b.corrupt_at = Some(3);
        let z = [(TfInput::default(), InputStatus::Confirmed); 2];
        for _ in 0..2 {
            a.advance(&z);
            b.advance(&z);
        }
        assert_eq!(a.checksum(), b.checksum());
        a.advance(&z);
        b.advance(&z);
        assert_ne!(a.checksum(), b.checksum());
    }
}
