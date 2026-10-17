//! Boss 2, Darkfang ("The Blind Lich", `CyclopsEye`, darkworld.md §7.3):
//! a skull-eye behind the arena with two stone fists that are moving
//! solids (they carry, push and crush), a rising pedestal, ghost shots
//! while the eye is shut and two Mad phases with enemy waves.

use super::Part;
use super::boss::{self, Base, BossKind, BossState, Seq};
use crate::arrow::{self, Arrow};
use crate::checksum::{Hasher, StableHash};
use crate::collide::Hitbox;
use crate::components::Ease;
use crate::content::Content;
use crate::enemies::{EnemyType, portal};
use crate::entity::{EntityId, Tag};
use crate::math::{PI, Vec2, angle_to_vector, approach_vec};
use crate::player::Player;
use crate::world::World;

pub const INTRO: u8 = 0;
pub const IDLE: u8 = 1;
pub const MAD: u8 = 2;
pub const DEAD: u8 = 3;

const HOME: [Vec2; 2] = [Vec2::new(90.0, 120.0), Vec2::new(230.0, 120.0)];

/// Fist tween easings.
pub const CUBE_IN_OUT: u8 = 0;
pub const CUBE_IN: u8 = 1;
pub const BACK_IN: u8 = 2;
pub const BACK_OUT: u8 = 3;

fn ease(code: u8) -> Ease {
    match code {
        CUBE_IN => Ease::CubeIn,
        BACK_IN => Ease::BackIn,
        BACK_OUT => Ease::BackOut,
        _ => Ease::CubeInOut,
    }
}

/// Fist impacts on arrival.
pub const NONE: u8 = 0;
pub const ARENA: u8 = 1;
pub const FISTS: u8 = 2;
pub const CRACK: u8 = 3;

/// One step of a fist sequence (positions of the left fist; the right
/// one mirrors across x = 160). `units` count `fistRate` ticks; a move
/// lasts `units` and the sequence waits for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FStep {
    /// Move to `to` (quadratic Bezier through `ctrl` if any) with an
    /// easing and an impact on arrival.
    Move {
        to: Vec2,
        ctrl: Option<Vec2>,
        units: f32,
        impact: u8,
        ease: u8,
    },
    Wait(f32),
    /// Set the eye's offset (px, y).
    Eye(f32),
    /// `ShakeLevel()`: screen shake 30, stuck arrows above y 120 fall.
    Shake,
    /// Back to home over 9 units (CubeInOut, Bezier through `ctrl`).
    Home(Option<Vec2>),
}

use FStep::{Eye as E, Home as H, Shake as S, Wait as W};
const fn mv(x: f32, y: f32, units: f32, impact: u8, ease: u8) -> FStep {
    FStep::Move {
        to: Vec2::new(x, y),
        ctrl: None,
        units,
        impact,
        ease,
    }
}
#[allow(clippy::too_many_arguments)]
const fn bz(x: f32, y: f32, cx: f32, cy: f32, units: f32, impact: u8, ease: u8) -> FStep {
    FStep::Move {
        to: Vec2::new(x, y),
        ctrl: Some(Vec2::new(cx, cy)),
        units,
        impact,
        ease,
    }
}
const fn h(x: f32, y: f32) -> FStep {
    H(Some(Vec2::new(x, y)))
}

// darkworld.md §7.3 "Fist moves" (floor y 190, ceiling y 60).
const STOMP_A: &[FStep] = &[
    mv(90.0, 190.0, 3.0, ARENA, BACK_IN),
    S,
    W(1.0),
    h(90.0, 170.0),
];
const STOMP_B: &[FStep] = &[
    mv(90.0, 190.0, 3.0, ARENA, BACK_IN),
    S,
    W(1.0),
    bz(115.0, 190.0, 115.0, 100.0, 6.0, ARENA, CUBE_IN),
    S,
    W(1.0),
    bz(140.0, 190.0, 140.0, 100.0, 6.0, ARENA, CUBE_IN),
    S,
    W(1.0),
    h(90.0, 170.0),
];
const STOMP_C: &[FStep] = &[
    mv(90.0, 190.0, 3.0, ARENA, BACK_IN),
    S,
    W(1.0),
    bz(140.0, 190.0, 140.0, 100.0, 8.0, ARENA, CUBE_IN),
    S,
    W(1.0),
    h(90.0, 170.0),
];
const STOMP_D: &[FStep] = &[
    mv(90.0, 190.0, 3.0, ARENA, BACK_IN),
    S,
    W(1.0),
    bz(140.0, 190.0, 140.0, 100.0, 6.0, ARENA, CUBE_IN),
    S,
    W(1.0),
    bz(115.0, 190.0, 115.0, 100.0, 4.0, ARENA, CUBE_IN),
    S,
    W(1.0),
    h(90.0, 170.0),
];
const MIDDLE_A: &[FStep] = &[
    E(-10.0),
    bz(140.0, 160.0, 80.0, 160.0, 3.0, FISTS, CUBE_IN),
    S,
    W(2.0),
    E(0.0),
    h(50.0, 160.0),
];
const MIDDLE_B: &[FStep] = &[
    bz(70.0, 190.0, 70.0, 110.0, 4.0, ARENA, CUBE_IN),
    S,
    W(2.0),
    mv(140.0, 190.0, 7.0, FISTS, CUBE_IN),
    S,
    W(3.0),
    h(50.0, 160.0),
];
const MIDDLE_C: &[FStep] = &[
    bz(70.0, 190.0, 70.0, 110.0, 4.0, ARENA, CUBE_IN),
    S,
    W(2.0),
    mv(140.0, 190.0, 6.0, FISTS, CUBE_IN),
    S,
    W(3.0),
    mv(70.0, 190.0, 4.0, ARENA, CUBE_IN),
    S,
    W(2.0),
    h(70.0, 110.0),
];
const MIDDLE_D: &[FStep] = &[
    bz(70.0, 190.0, 70.0, 110.0, 4.0, ARENA, CUBE_IN),
    S,
    W(2.0),
    mv(140.0, 190.0, 6.0, FISTS, CUBE_IN),
    S,
    W(2.0),
    bz(90.0, 60.0, 70.0, 150.0, 4.0, ARENA, CUBE_IN),
    S,
    W(2.0),
    H(None),
];
const CEILING_A: &[FStep] = &[
    E(20.0),
    bz(100.0, 60.0, 100.0, 120.0, 3.0, ARENA, CUBE_IN),
    S,
    W(2.0),
    // A 16 px outward nudge and back (BackOut), then 2 more units.
    bz(100.0, 60.0, 84.0, 60.0, 3.0, NONE, BACK_OUT),
    W(2.0),
    E(0.0),
    h(70.0, 110.0),
];
const CEILING_B: &[FStep] = &[
    E(20.0),
    bz(100.0, 60.0, 100.0, 120.0, 3.0, ARENA, CUBE_IN),
    S,
    W(1.0),
    mv(140.0, 60.0, 2.0, FISTS, CUBE_IN),
    S,
    W(2.0),
    E(0.0),
    h(90.0, 60.0),
];
const CEILING_C: &[FStep] = &[
    bz(100.0, 60.0, 100.0, 120.0, 3.0, ARENA, CUBE_IN),
    S,
    W(1.0),
    bz(100.0, 190.0, 100.0, 120.0, 4.0, ARENA, CUBE_IN),
    S,
    W(1.0),
    h(70.0, 110.0),
];
const CEILING_D: &[FStep] = &[
    bz(100.0, 60.0, 100.0, 120.0, 3.0, ARENA, CUBE_IN),
    S,
    W(1.0),
    bz(140.0, 190.0, 70.0, 190.0, 4.0, FISTS, CUBE_IN),
    S,
    W(1.0),
    h(70.0, 110.0),
];
const SIDE_A: &[FStep] = &[
    mv(40.0, 110.0, 2.0, ARENA, CUBE_IN),
    S,
    W(1.0),
    h(90.0, 150.0),
];
const SIDE_B: &[FStep] = &[
    mv(40.0, 110.0, 2.0, ARENA, CUBE_IN),
    S,
    W(1.0),
    mv(40.0, 150.0, 3.0, ARENA, CUBE_IN),
    S,
    W(3.0),
    h(90.0, 150.0),
];

pub const SEQUENCES: [&[FStep]; 14] = [
    STOMP_A, STOMP_B, STOMP_C, STOMP_D, MIDDLE_A, MIDDLE_B, MIDDLE_C, MIDDLE_D, CEILING_A,
    CEILING_B, CEILING_C, CEILING_D, SIDE_A, SIDE_B,
];

/// Sequence indices per effective level `difficulty + madCounter`.
pub fn sequences_for(e: i32) -> &'static [u8] {
    match e {
        0 => &[0, 8, 12],
        1 => &[1, 8, 12, 4],
        2 => &[1, 9, 13, 5],
        3 => &[2, 10, 13, 6],
        _ => &[3, 11, 13, 7],
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub state: u8,
    pub mad: i32,
    pub fist_rate: f32,
    pub fists: [Option<EntityId>; 2],
    pub pedestal: Option<EntityId>,
    pub pedestal_t: f32,
    pub pedestal_up: bool,
    pub speed: Vec2,
    pub move_offset: Vec2,
    pub sine: f32,
    pub sine_mult: f32,
    pub eye_open: bool,
    pub open_timer: f32,
    pub close_counter: i32,
    pub intro: Seq,
    /// Fist director: wait, then a sequence.
    pub director: Seq,
    pub seq: Option<u8>,
    pub last_seq: Option<u8>,
    pub step: u8,
    /// Left-fist tween: from, to, ctrl, ticks, length, impact.
    pub from: Vec2,
    pub to: Vec2,
    pub ctrl: Option<Vec2>,
    pub t: f32,
    pub len: f32,
    pub impact: u8,
    pub ease: u8,
    pub step_wait: f32,
    pub sway: f32,
    pub swaying: bool,
    pub shooter: Seq,
    pub shot_dir: f32,
    pub mad_seq: Seq,
    pub sets: [[Option<EnemyType>; 3]; 2],
    pub n_sets: usize,
    /// The Mad set being dispatched: (portal, enemy), next index.
    pub pend: [Option<(EntityId, EnemyType)>; 3],
    pub pend_i: usize,
    pub death: Seq,
    pub midway_done: bool,
}

impl StableHash for State {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u8(self.state);
        h.i32(self.mad);
        h.f32(self.fist_rate);
        h.vec2(self.speed);
        h.vec2(self.move_offset);
        h.f32(self.sine);
        h.bool(self.eye_open);
        h.f32(self.open_timer);
        h.i32(self.close_counter);
        self.intro.stable_hash(h);
        self.director.stable_hash(h);
        h.u32(self.seq.map_or(99, |s| s as u32));
        h.u8(self.step);
        h.f32(self.t);
        h.f32(self.step_wait);
        h.f32(self.sway);
        self.shooter.stable_hash(h);
        self.mad_seq.stable_hash(h);
        self.death.stable_hash(h);
        h.f32(self.pedestal_t);
        h.u8(self.ease);
        h.u32(self.pend_i as u32);
    }
}

pub fn spawn(w: &mut World, _c: &Content, mut base: Base) -> BossState {
    base.max_health = if base.players < 3 {
        16 + 6 * base.difficulty
    } else {
        14 + 6 * base.difficulty + 2 * base.players
    };
    base.health = base.max_health;
    base.stomp_damage = 2;
    base.midway_auto = false;
    let body = boss::add_body(
        w,
        Vec2::new(160.0, 90.0),
        Hitbox::new(32.0, 10.0, -16.0, 0.0),
        Some(Hitbox::new(20.0, 20.0, -10.0, -5.0)),
        -300,
        &[
            Tag::Enemy,
            Tag::PlayerCollider,
            Tag::Target,
            Tag::LightSource,
        ],
    );
    w.core_mut(body).collidable = false;
    let mut fists = [None; 2];
    for (k, f) in fists.iter_mut().enumerate() {
        *f = Some(super::part::add(
            w,
            HOME[k],
            Hitbox::new(40.0, 60.0, -20.0, -30.0),
            -150,
            &[Tag::Solid],
            Part::Fist(k as u8),
        ));
    }
    let pedestal = super::part::add(
        w,
        Vec2::new(140.0, 220.0),
        Hitbox::new(40.0, 40.0, 0.0, 0.0),
        -150,
        &[Tag::Solid],
        Part::Pedestal,
    );
    // The floor bones (hazard strip).
    super::part::add(
        w,
        Vec2::new(50.0, 210.0),
        Hitbox::new(220.0, 4.0, 0.0, 0.0),
        -140,
        &[Tag::PlayerCollider],
        Part::Bones,
    );
    BossState {
        id: 2,
        body,
        base,
        kind: BossKind::Cyclops(State {
            state: INTRO,
            mad: 0,
            fist_rate: (13 - base.difficulty) as f32,
            fists,
            pedestal: Some(pedestal),
            pedestal_t: -1.0,
            pedestal_up: false,
            speed: Vec2::ZERO,
            move_offset: Vec2::ZERO,
            sine: 0.0,
            sine_mult: 1.0,
            eye_open: false,
            open_timer: -1.0,
            close_counter: 2,
            intro: Seq::start(0),
            director: Seq::default(),
            seq: None,
            last_seq: None,
            step: 0,
            from: HOME[0],
            to: HOME[0],
            ctrl: None,
            t: 0.0,
            len: 0.0,
            impact: 0,
            ease: CUBE_IN_OUT,
            step_wait: 0.0,
            sway: 0.0,
            swaying: false,
            shooter: Seq::default(),
            shot_dir: 1.0,
            mad_seq: Seq::default(),
            sets: [[None; 3]; 2],
            n_sets: 0,
            pend: [None; 3],
            pend_i: 0,
            death: Seq::default(),
            midway_done: false,
        }),
    }
}

fn mirror(p: Vec2) -> Vec2 {
    Vec2::new(320.0 - p.x, p.y)
}

/// Move both fists so the left one is at `left`.
fn place_fists(w: &mut World, c: &Content, s: &State, left: Vec2) {
    for (k, f) in s.fists.iter().enumerate() {
        let Some(id) = *f else { continue };
        if !w.exists(id) {
            continue;
        }
        let target = if k == 0 { left } else { mirror(left) };
        let d = target - w.core(id).pos;
        if !d.is_zero() {
            crate::platform::solid_move(w, c, id, d);
        }
    }
}

fn left_pos(w: &World, s: &State) -> Vec2 {
    s.fists[0].map_or(HOME[0], |f| w.core(f).pos)
}

fn set_eye(w: &mut World, id: EntityId, s: &mut State, open: bool) {
    if s.eye_open == open {
        return;
    }
    s.eye_open = open;
    w.core_mut(id).collidable = open;
    w.sound(
        if open {
            "en_rambossEyeOpen"
        } else {
            "en_rambossEyeClose"
        },
        160.0,
    );
}

pub fn update(w: &mut World, c: &Content, id: EntityId, s: &mut State, b: &mut Base) {
    let tm = w.time.time_mult;
    // Hover.
    s.sine += 2.0 * PI / 240.0 * tm;
    let goal = Vec2::new(160.0, 90.0)
        + s.move_offset
        + Vec2::new(30.0 * libm::sinf(s.sine * 0.5), 10.0 * libm::sinf(s.sine)) * s.sine_mult;
    let pos = w.core(id).pos;
    let d = goal - pos;
    if d.length_sq() > 100.0 {
        s.speed = approach_vec(s.speed, d.safe_normalize() * 0.4, 0.2 * tm);
    } else {
        s.speed = approach_vec(s.speed, Vec2::ZERO, 0.1 * tm);
    }
    w.core_mut(id).pos = pos + s.speed * tm;
    // Pedestal tween (220 <-> 190 over 120).
    if s.pedestal_t >= 0.0
        && let Some(p) = s.pedestal
    {
        s.pedestal_t += tm;
        let x = (s.pedestal_t / 120.0).min(1.0);
        let y = if s.pedestal_up {
            220.0 - 30.0 * Ease::CubeOut.apply(x)
        } else {
            190.0 + 30.0 * Ease::CubeIn.apply(x)
        };
        let dy = y - w.core(p).pos.y;
        crate::platform::solid_move(w, c, p, Vec2::new(0.0, dy));
        if s.pedestal_t >= 120.0 {
            s.pedestal_t = -1.0;
        }
    }
    match s.state {
        INTRO => intro(w, id, s, b, tm),
        IDLE => {
            if s.open_timer > 0.0 {
                s.open_timer -= tm;
                if s.open_timer <= 0.0 {
                    set_eye(w, id, s, true);
                }
            }
            director(w, c, s, b, tm);
            fists_tick(w, c, s, tm);
            shooter(w, c, s, b, tm);
            if b.health <= 0 {
                start_death(w, id, s);
            } else if b.health <= b.max_health * (2 - s.mad) / 3 {
                start_mad(w, id, s);
            }
        }
        MAD => mad(w, c, id, s, b, tm),
        _ => death(w, c, id, s, tm),
    }
}

fn intro(w: &mut World, id: EntityId, s: &mut State, b: &Base, tm: f32) {
    if !s.intro.ready(tm) {
        return;
    }
    match s.intro.step {
        0 => {
            if super::round_started(w) {
                s.intro.wait(180.0, 1);
            }
        }
        1 => {
            boss::enter_boss_mode(w, b.max_health);
            w.sound("en_rambossAppear", 160.0);
            s.intro.wait(180.0, 2);
        }
        2 => {
            w.sound("en_rambossFistsActivate", 160.0);
            s.intro.wait(60.0, 3);
        }
        _ => {
            s.intro.stop();
            enter_idle(w, id, s);
        }
    }
}

fn enter_idle(w: &mut World, id: EntityId, s: &mut State) {
    s.state = IDLE;
    s.close_counter = 2;
    s.sine_mult = 1.0;
    if s.mad > 0 {
        s.open_timer = 120.0;
    } else {
        set_eye(w, id, s, true);
    }
    s.director = Seq::start(0);
    s.shooter = Seq::start(0);
    s.swaying = true;
    s.shot_dir = if w.rng.next_max(2) == 0 { 1.0 } else { -1.0 };
}

fn director(w: &mut World, _c: &Content, s: &mut State, b: &Base, tm: f32) {
    if s.seq.is_some() || !s.director.ready(tm) {
        return;
    }
    match s.director.step {
        0 => s.director.wait((60 - 10 * b.difficulty).max(0) as f32, 1),
        _ => {
            let list = sequences_for(b.difficulty + s.mad);
            let pool: Vec<u8> = if list.len() > 1 {
                list.iter()
                    .copied()
                    .filter(|&q| Some(q) != s.last_seq)
                    .collect()
            } else {
                list.to_vec()
            };
            let q = pool[w.rng.choose_index(pool.len())];
            s.seq = Some(q);
            s.last_seq = Some(q);
            s.step = 0;
            s.len = 0.0;
            s.step_wait = 0.0;
            s.swaying = false;
            s.director.step = 0;
        }
    }
}

/// `FistMirrorTo`: start a tween of the left fist (the right mirrors).
fn fists_to(
    w: &mut World,
    s: &mut State,
    to: Vec2,
    ctrl: Option<Vec2>,
    e: u8,
    len: f32,
    impact: u8,
) {
    s.from = left_pos(w, s);
    s.to = to;
    s.ctrl = ctrl;
    s.ease = e;
    s.t = 0.0;
    s.len = len;
    s.impact = impact;
    s.swaying = false;
    if impact != NONE {
        w.sound("en_rambossMove", 160.0);
    }
}

/// Advance the fist tween; on arrival play the impact. Returns whether a
/// tween is still running.
fn tween_fists(w: &mut World, c: &Content, s: &mut State, tm: f32) -> bool {
    if s.len <= 0.0 {
        return false;
    }
    s.t += tm;
    let x = (s.t / s.len).min(1.0);
    let e = ease(s.ease).apply(x);
    let p = match s.ctrl {
        Some(k) => {
            let a = s.from + (k - s.from) * e;
            let b = k + (s.to - k) * e;
            a + (b - a) * e
        }
        None => s.from + (s.to - s.from) * e,
    };
    place_fists(w, c, s, p);
    if s.t >= s.len {
        s.len = 0.0;
        match s.impact {
            ARENA => w.sound("en_rambossHitArena", 160.0),
            FISTS => w.sound("en_rambossHitTogether", 160.0),
            CRACK => w.sound("en_rambossHitCrack", 160.0),
            _ => {}
        }
        return false;
    }
    true
}

/// Run the current fist sequence (or sway at home).
fn fists_tick(w: &mut World, c: &Content, s: &mut State, tm: f32) {
    let Some(q) = s.seq else {
        if s.swaying {
            s.sway += 2.0 * PI / 240.0 * tm;
            let off = Vec2::new(-8.0 * libm::sinf(s.sway), 8.0 * libm::sinf(s.sway * 0.5));
            place_fists(w, c, s, HOME[0] + off);
        }
        return;
    };
    if tween_fists(w, c, s, tm) {
        return;
    }
    if s.step_wait > 0.0 {
        s.step_wait -= tm;
        return;
    }
    let steps = SEQUENCES[q as usize];
    let Some(&st) = steps.get(s.step as usize) else {
        s.seq = None;
        s.swaying = true;
        s.sway = 0.0;
        return;
    };
    s.step += 1;
    let u = s.fist_rate;
    match st {
        FStep::Move {
            to,
            ctrl,
            units,
            impact,
            ease,
        } => fists_to(w, s, to, ctrl, ease, units * u, impact),
        FStep::Wait(units) => s.step_wait = units * u,
        FStep::Eye(y) => s.move_offset.y = y,
        FStep::Shake => boss::shake_level(w, 30.0),
        FStep::Home(ctrl) => fists_to(w, s, HOME[0], ctrl, CUBE_IN_OUT, 9.0 * u, NONE),
    }
}

fn shooter(w: &mut World, c: &Content, s: &mut State, b: &Base, tm: f32) {
    if !s.shooter.ready(tm) {
        return;
    }
    // The eye is checked at the loop head only: a started wait(25) always
    // ends in a shot.
    if s.eye_open && s.shooter.step == 0 {
        s.shooter.next(0);
        return;
    }
    match s.shooter.step {
        0 => s.shooter.wait(25.0, 1),
        _ => {
            let from = c
                .level(w.level)
                .markers
                .boss_markers
                .first()
                .map_or(Vec2::new(160.0, 40.0), |m| m.pos);
            let at = from + Vec2::new((w.rng.next_max(100) - 50) as f32, 0.0);
            let base = (Vec2::new(160.0, 220.0) - at).angle();
            let off = (5.0 + w.rng.next_float() * 40.0) * PI / 180.0 * s.shot_dir;
            s.shot_dir = -s.shot_dir;
            let id = super::part::add(
                w,
                at,
                Hitbox::new(14.0, 6.0, -7.0, -4.0),
                -200,
                &[Tag::PlayerCollider, Tag::Target, Tag::ExplosionCollider],
                Part::CycShot {
                    t: 0.0,
                    heading: base + off,
                },
            );
            w.core_mut(id).target_collider = Some(Hitbox::new(16.0, 16.0, -8.0, -8.0));
            w.sound("en_rambossGhostAppear", at.x);
            s.shooter.wait((45 - 20 * b.difficulty).max(0) as f32, 0);
        }
    }
}

fn kill_shots(w: &mut World) {
    let shots: Vec<EntityId> = w
        .entities()
        .filter(|&e| matches!(super::part::get(w, e), Some(Part::CycShot { .. })))
        .collect();
    for e in shots {
        w.sound("en_rambossGhostDeath", w.core(e).pos.x);
        w.remove(e);
    }
}

// ----------------------------------------------------------------- mad

fn mad_sets(d: i32, mad: i32) -> ([[Option<EnemyType>; 3]; 2], usize) {
    use EnemyType::{Exploder as X, TechnoMage as T};
    match (d, mad) {
        (0, 0) => ([[Some(X), Some(X), None], [None; 3]], 1),
        (0, _) => ([[Some(X), Some(X), Some(X)], [Some(T), None, None]], 2),
        (1, 0) => ([[Some(X), Some(X), Some(X)], [None; 3]], 1),
        (1, _) => ([[Some(X), Some(T), None], [Some(T), Some(T), None]], 2),
        (_, 0) => ([[Some(X), Some(T), None], [None; 3]], 1),
        (_, _) => (
            [[Some(X), Some(X), Some(T)], [Some(X), Some(T), Some(T)]],
            2,
        ),
    }
}

fn start_mad(w: &mut World, id: EntityId, s: &mut State) {
    s.state = MAD;
    s.director.stop();
    s.shooter.stop();
    s.seq = None;
    s.swaying = false;
    // The fists stop where they are.
    s.len = 0.0;
    set_eye(w, id, s, false);
    s.open_timer = -1.0;
    s.sine_mult = 1.5;
    s.fist_rate = (s.fist_rate - 1.0).max(4.0);
    kill_shots(w);
    let (sets, n) = mad_sets(
        w.dark_world
            .as_ref()
            .map_or(0, |d| d.settings.difficulty as i32),
        s.mad,
    );
    s.sets = sets;
    s.n_sets = n;
    s.mad_seq = Seq::start(0);
    s.mad_seq.wait(30.0, 1);
}

fn portals(w: &World) -> Vec<EntityId> {
    w.dark_world
        .as_ref()
        .map(|d| d.portals.iter().map(|p| p.id).collect())
        .unwrap_or_default()
}

fn mad(w: &mut World, c: &Content, id: EntityId, s: &mut State, _b: &Base, tm: f32) {
    // Fist tweens keep running during the cutscene; once swaying again
    // (late in the sequence) they sway around home.
    if !tween_fists(w, c, s, tm) && s.swaying {
        s.sway += 2.0 * PI / 240.0 * tm;
        let off = Vec2::new(-8.0 * libm::sinf(s.sway), 8.0 * libm::sinf(s.sway * 0.5));
        place_fists(w, c, s, HOME[0] + off);
    }
    if !s.mad_seq.ready(tm) {
        return;
    }
    match s.mad_seq.step {
        // Fists low: slam to the ceiling (arena impact), shake, wait(10).
        1 => {
            let low = s
                .fists
                .iter()
                .flatten()
                .any(|&f| w.core(f).pos.y > HOME[0].y - 20.0);
            if low {
                fists_to(w, s, Vec2::new(90.0, 60.0), None, CUBE_IN, 40.0, ARENA);
                s.mad_seq.wait(40.0, 2);
            } else {
                s.mad_seq.next(3);
            }
        }
        2 => {
            boss::shake_level(w, 30.0);
            s.mad_seq.wait(10.0, 3);
        }
        // The head dips, the horn breaks (90 ticks of shaking).
        3 => {
            s.move_offset.y = 20.0;
            w.sound("en_rambossHornBreak", 160.0);
            s.mad_seq.wait(90.0, 4);
        }
        // Level shake; the horn's recoil knocks the head sideways.
        4 => {
            boss::shake_level(w, 30.0);
            s.speed = Vec2::new(if s.mad == 0 { 3.0 } else { -3.0 }, 0.0);
            s.mad_seq.wait(15.0 + 15.0 + 30.0, 5);
        }
        // The head rises and fades; the fists clap out to the sides.
        5 => {
            s.move_offset.y = -40.0;
            w.sound("en_rambossDisappear", 160.0);
            fists_to(
                w,
                s,
                Vec2::new(40.0, 150.0),
                Some(Vec2::new(40.0, 60.0)),
                CUBE_IN,
                60.0,
                CRACK,
            );
            s.mad_seq.wait(30.0, 6);
        }
        // Midway treasure (first Mad only) and the pedestal rises.
        6 => {
            if !s.midway_done {
                s.midway_done = true;
                super::signal_boss_treasure(w);
            }
            s.pedestal_up = true;
            s.pedestal_t = 0.0;
            s.mad_seq.wait(30.0, 7);
        }
        7 => {
            boss::shake_level(w, 30.0);
            s.mad_seq.wait(30.0 + 30.0, 8);
        }
        // Enemy sets: one per portal, `wait(5)` apart, then `wait(60)`.
        8 => {
            let k = s.mad_seq.k as usize;
            if k >= s.n_sets {
                s.mad_seq.next(11);
                return;
            }
            let mut ps = portals(w);
            w.rng.shuffle(&mut ps);
            let mut set: Vec<EnemyType> = s.sets[k].iter().flatten().copied().collect();
            w.rng.shuffle(&mut set);
            s.pend = [None; 3];
            for (j, ty) in set.into_iter().enumerate().take(3) {
                if let Some(&p) = ps.get(j % ps.len().max(1)) {
                    s.pend[j] = Some((p, ty));
                }
            }
            s.pend_i = 0;
            s.mad_seq.next(9);
        }
        9 => {
            while s.pend_i < 3 && s.pend[s.pend_i].is_none() {
                s.pend_i += 1;
            }
            if s.pend_i >= 3 {
                s.mad_seq.wait(60.0, 10);
                return;
            }
            if let Some((p, ty)) = s.pend[s.pend_i] {
                portal::appear_and_spawn(w, p, ty);
            }
            s.pend_i += 1;
            s.mad_seq.wait(5.0, 9);
        }
        10 => {
            if portal::queued_total(w) == 0 && crate::enemies::enemy_count(w) <= 1 {
                s.mad_seq.k += 1;
                s.mad_seq.next(8);
            }
        }
        // The fists light up, the pedestal lowers, the fists go home.
        11 => {
            w.sound("en_rambossFistsActivate", 160.0);
            s.mad_seq.wait(30.0, 12);
        }
        12 => {
            s.pedestal_up = false;
            s.pedestal_t = 0.0;
            s.mad_seq.wait(30.0, 13);
        }
        13 => {
            fists_to(
                w,
                s,
                HOME[0],
                Some(HOME[0] + Vec2::new(0.0, 30.0)),
                CUBE_IN_OUT,
                60.0,
                NONE,
            );
            w.sound("en_rambossReappear", 160.0);
            s.mad_seq.wait(60.0, 14);
        }
        // Sway again; the head shows 90 ticks later.
        14 => {
            s.swaying = true;
            s.sway = 0.0;
            s.mad_seq.wait(90.0, 15);
        }
        _ => {
            s.mad_seq.stop();
            s.mad += 1;
            s.move_offset = Vec2::ZERO;
            enter_idle(w, id, s);
        }
    }
}

// --------------------------------------------------------------- death

fn start_death(w: &mut World, id: EntityId, s: &mut State) {
    s.state = DEAD;
    s.len = 0.0;
    s.director.stop();
    s.shooter.stop();
    s.seq = None;
    s.swaying = false;
    kill_shots(w);
    w.core_mut(id).collidable = false;
    w.tag(id, Tag::Enemy, false);
    w.sound("en_rambossDeath", 160.0);
    s.death = Seq::start(0);
    s.death.wait(30.0, 1);
}

fn death(w: &mut World, c: &Content, id: EntityId, s: &mut State, tm: f32) {
    tween_fists(w, c, s, tm);
    if !s.death.ready(tm) {
        return;
    }
    match s.death.step {
        1 => {
            fists_to(w, s, HOME[0], None, CUBE_IN_OUT, 60.0, NONE);
            s.death.wait(60.0 + 60.0, 2);
        }
        // The fists shatter.
        2 => s.death.wait(20.0 + 60.0 + 70.0, 3),
        3 => {
            for f in s.fists.iter_mut() {
                if let Some(e) = f.take() {
                    w.remove(e);
                }
            }
            s.death.wait(30.0 + 40.0, 4);
        }
        4 => {
            boss::shake_level(w, 60.0);
            s.death.wait(300.0, 5);
        }
        5 => {
            let at = w.core(id).pos;
            crate::explosion::spawn(w, c, at, -1, false, &mut None);
            w.screen_shake(60.0);
            w.core_mut(id).visible = false;
            s.death.wait(180.0, 6);
        }
        _ => {
            super::boss_finished(w);
            w.remove(id);
            s.death.stop();
        }
    }
}

// ------------------------------------------------------------ the eye

fn hurt_eye(
    w: &mut World,
    id: EntityId,
    s: &mut State,
    b: &mut Base,
    dmg: i32,
    killer: i8,
    stomp: bool,
) {
    if !s.eye_open || s.state == DEAD {
        return;
    }
    w.sound("en_rambossHurt", 160.0);
    if s.state == IDLE {
        if stomp {
            s.close_counter = 1;
        }
        s.close_counter -= 1;
        if s.close_counter <= 0 {
            s.close_counter = 2;
            set_eye(w, id, s, false);
            s.open_timer = 120.0 + 60.0 * b.difficulty as f32;
        }
    }
    b.flash = 20.0;
    w.screen_shake(20.0);
    boss::hurt(w, b, dmg, killer);
}

pub fn body_arrow(w: &mut World, _c: &Content, id: EntityId, aid: EntityId, a: &mut Arrow) -> bool {
    if !a.dangerous() {
        return false;
    }
    let killer = a.player_index;
    let hit = boss::with_boss(w, |w, bs| {
        let (BossKind::Cyclops(s), base) = (&mut bs.kind, &mut bs.base) else {
            return false;
        };
        if !s.eye_open {
            return false;
        }
        hurt_eye(w, id, s, base, 1, killer, false);
        true
    })
    .unwrap_or(false);
    if hit {
        arrow::enter_fall_mode(w, aid, a, true, false, false);
    }
    hit
}

pub fn body_touch(w: &mut World, c: &Content, id: EntityId, pid: EntityId, p: &mut Player) {
    if !(p.speed.y >= 0.0 && boss::is_stomp(w, id, pid, p)) {
        return;
    }
    boss::bounce(w, c, id, pid, p);
    let killer = p.index as i8;
    boss::with_boss(w, |w, bs| {
        if let (BossKind::Cyclops(s), base) = (&mut bs.kind, &mut bs.base) {
            let dmg = boss::stomp_damage(base);
            hurt_eye(w, id, s, base, dmg, killer, true);
        }
    });
}

// ----------------------------------------------------------- the parts

pub fn part_update(w: &mut World, _c: &Content, id: EntityId, p: &mut Part) {
    let tm = w.time.time_mult;
    if let Part::CycShot { t, heading } = p {
        *t += tm;
        let pos = w.core(id).pos + angle_to_vector(*heading, 0.6 * tm);
        w.core_mut(id).pos = pos;
        if !(-40.0..=360.0).contains(&pos.x) || !(-40.0..=280.0).contains(&pos.y) {
            w.remove(id);
        }
    }
}

pub fn shot_touch(w: &mut World, c: &Content, id: EntityId, pid: EntityId, p: &mut Player) {
    match super::part::get(w, id) {
        Some(Part::CycShot { t, .. }) => {
            if t < 15.0 {
                return;
            }
            let top = w.core(id).top();
            let pc = w.core(pid);
            if p.speed.y >= 0.0 && pc.bottom() - p.speed.y <= top + 6.0 {
                crate::player::bounce_pub(w, c, pid, p, top, false);
            } else {
                boss::hurt_player(w, c, pid, p, w.core(id).pos);
            }
            w.sound("en_rambossGhostDeath", w.core(id).pos.x);
            w.remove(id);
            w.core_mut(id).collidable = false;
        }
        Some(Part::Bones) => {
            boss::hurt_player(w, c, pid, p, w.core(id).pos + Vec2::new(110.0, 4.0))
        }
        _ => {}
    }
}

pub fn shot_arrow(w: &mut World, _c: &Content, id: EntityId, aid: EntityId, a: &mut Arrow) -> bool {
    let Some(Part::CycShot { t, .. }) = super::part::get(w, id) else {
        return false;
    };
    if t < 15.0 {
        return false;
    }
    w.sound("en_rambossGhostDeath", w.core(id).pos.x);
    w.remove(id);
    w.core_mut(id).collidable = false;
    arrow::enter_fall_mode(w, aid, a, true, false, false);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fist_sequences_shake_after_strikes_and_end_home() {
        for (k, q) in SEQUENCES.iter().enumerate() {
            assert!(
                matches!(q.last(), Some(FStep::Home(_))),
                "sequence {k} ends home"
            );
            for (i, st) in q.iter().enumerate() {
                if let FStep::Move { impact, .. } = st
                    && *impact != NONE
                {
                    assert_eq!(q.get(i + 1), Some(&FStep::Shake), "sequence {k} step {i}");
                }
            }
        }
        // The first stomp eases in with BackIn.
        assert!(matches!(
            STOMP_A[0],
            FStep::Move {
                ease: BACK_IN,
                impact: ARENA,
                ..
            }
        ));
    }
}
