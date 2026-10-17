//! Boss 3, Cataclysm (`CataclysmEye`, darkworld.md §7.4): the final boss.
//! A sealed eye that drains the archers' arrows, alternates a shielded
//! phase (shield orbs + aimed bullets) with a vulnerable one (blades,
//! bullet bursts, charged laser shots), then an invulnerable bullet-hell
//! finale on the breaking platforms and the heart phase.
//!
//! The finale patterns follow the spec's structure (the contract); exact
//! stream angles are approximated where the spec defers to the original.

use super::Part;
use super::boss::{self, Base, BossKind, BossState, Seq};
use crate::arrow::{self, Arrow};
use crate::checksum::{Hasher, StableHash};
use crate::collide::Hitbox;
use crate::components::Ease;
use crate::content::Content;
use crate::enemies::{EnemyType, portal};
use crate::entity::{Allegiance, EntityId, Kind, Tag};
use crate::math::{PI, Vec2, angle_diff, angle_to_vector, approach_vec};
use crate::player::Player;
use crate::quiver::ArrowType;
use crate::variants::Variant;
use crate::world::World;

pub const INTRO: u8 = 0;
pub const SAFE: u8 = 1;
pub const FREAKOUT: u8 = 2;
pub const EXPLODING: u8 = 3;
pub const DEAD: u8 = 4;

const DEG: f32 = PI / 180.0;

/// One instruction of a finale pattern script (darkworld.md §7.4.1).
/// Angles in radians (0 = right, pi/2 = down); bullet speeds are
/// multiples of the 0.8 px/tick base.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fin {
    Wait(f32),
    /// First half of a burst at each direction: offsets +-pi/10, +-pi/20, 0
    /// (speed x3).
    BurstA([f32; 3], u8),
    /// Second half: offsets +-3pi/40, +-pi/40.
    BurstB([f32; 3], u8),
    /// Normal pattern A: fix the fan step from the boss position.
    FanInit,
    /// Normal pattern A fan: narrow = 9 bullets `pi/2 + step*l`, l in
    /// -4..=4; wide = 10 bullets `pi/2 - step/2 + step*l`, l in -4..=5.
    Fan(bool),
    Missile(f32),
    /// Generator: 0 off, 1 checkerboard, 2 spread checker.
    Gen(u8),
    /// A bullet stream from `a` to `b` over `len` ticks (CubeInOut), one
    /// bullet each `sep` ticks at `speed`; mirrored about pi/2.
    Tween {
        a: f32,
        b: f32,
        speed: f32,
        len: f32,
        sep: f32,
        mirror: bool,
    },
    ShufflePortals,
    /// An enemy at shuffled portal `k`.
    Spawn(u8, &'static str),
    /// Wait while more than one Enemy (the boss) exists.
    WaitEnemies,
    MiasmaOn,
    LavaOn,
    /// The lava recedes and the miasma dissipates.
    HazardsOff,
}

use Fin::*;

fn lerpf(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn burst_ops(dirs: &[f32]) -> [Fin; 3] {
    let mut d = [0.0; 3];
    d[..dirs.len()].copy_from_slice(dirs);
    let n = dirs.len() as u8;
    [BurstA(d, n), Wait(5.0), BurstB(d, n)]
}

/// The finale scripts per difficulty (Normal B A C End, Hardcore B A C End,
/// Legendary B A C D End).
pub fn finale(difficulty: i32) -> Vec<Fin> {
    let q = PI / 4.0;
    let h = PI / 2.0;
    let tq = 3.0 * PI / 4.0;
    let mut v = vec![];
    let tween = |a, b, speed, len, sep, mirror| Tween {
        a,
        b,
        speed,
        len,
        sep,
        mirror,
    };
    let end_dir = lerpf(tq, h, 0.7);
    let four = [
        Missile(PI),
        Missile(lerpf(q, 0.0, 0.5)),
        Missile(lerpf(tq, PI, 0.5)),
        Missile(0.0),
    ];
    match difficulty {
        0 => {
            // B: burst fire.
            let dirs: [&[f32]; 7] = [
                &[0.0, PI],
                &[q, tq],
                &[h],
                &[lerpf(q, h, 0.2), lerpf(tq, h, 0.2)],
                &[0.0, PI, h],
                &[q, tq],
                &[0.0, PI, h],
            ];
            let waits = [60.0, 60.0, 60.0, 60.0, 60.0, 30.0, 60.0];
            for (d, wt) in dirs.iter().zip(waits) {
                v.extend(burst_ops(d));
                v.push(Wait(wt));
            }
            // A: fans.
            v.push(FanInit);
            for _ in 0..3 {
                v.extend([Fan(false), Wait(10.0), Fan(false), Wait(10.0), Wait(40.0)]);
                v.extend([Fan(true), Wait(10.0), Fan(true), Wait(10.0), Wait(40.0)]);
            }
            v.push(Wait(150.0));
            // C: missiles over the checkerboard.
            v.extend([
                Wait(30.0),
                Missile(q),
                Wait(120.0),
                Missile(tq),
                Wait(30.0),
                Gen(1),
                Wait(180.0),
            ]);
            v.extend([Missile(q), Wait(120.0), Missile(tq), Wait(120.0)]);
            v.extend([
                Missile(lerpf(q, h, 0.5)),
                Wait(120.0),
                Missile(lerpf(tq, h, 0.5)),
                Wait(120.0),
            ]);
            v.extend([Missile(h), Missile(PI), Missile(0.0), Gen(0), Wait(240.0)]);
            // End.
            v.extend([tween(tq, end_dir, 2.0, 120.0, 10.0, true), Wait(90.0)]);
            v.extend(four);
            v.push(Wait(30.0));
        }
        1 => {
            // B.
            v.extend([
                Gen(1),
                Wait(180.0),
                ShufflePortals,
                Spawn(0, "PrismCrystal"),
                Wait(360.0),
            ]);
            v.extend([
                Spawn(1, "PrismCrystal"),
                Wait(120.0),
                WaitEnemies,
                Wait(60.0),
                Gen(0),
                Wait(240.0),
            ]);
            // A.
            hard_a(&mut v, false);
            // C.
            v.extend([
                Gen(1),
                Wait(180.0),
                ShufflePortals,
                Spawn(0, "WingSkeleton"),
                Wait(360.0),
            ]);
            v.extend([
                Spawn(1, "WingSkeleton"),
                Wait(360.0),
                Spawn(0, "PrismSkeleton"),
                Wait(360.0),
            ]);
            v.extend([
                Spawn(1, "PrismSkeleton"),
                Wait(120.0),
                WaitEnemies,
                Wait(60.0),
                Gen(0),
                Wait(270.0),
            ]);
            // End.
            v.extend([tween(tq, end_dir, 3.0, 120.0, 8.0, true), Wait(90.0)]);
            v.extend(four);
            v.extend([Wait(60.0), Missile(h)]);
        }
        _ => {
            // B.
            v.extend([
                Gen(2),
                Wait(180.0),
                ShufflePortals,
                Spawn(0, "Exploder"),
                Spawn(1, "PrismCrystal"),
            ]);
            v.extend([
                Wait(360.0),
                Spawn(0, "PrismCrystal"),
                Spawn(1, "Exploder"),
                Wait(120.0),
            ]);
            v.extend([WaitEnemies, Wait(60.0), Gen(0), Wait(240.0)]);
            // A.
            hard_a(&mut v, true);
            // C.
            v.extend([
                Gen(2),
                Wait(180.0),
                ShufflePortals,
                Spawn(0, "WingSkeleton"),
                Wait(360.0),
            ]);
            v.extend([
                Spawn(1, "WingSkeleton"),
                Wait(360.0),
                Spawn(0, "PrismSkeletonS"),
                Wait(60.0),
            ]);
            v.extend([
                Spawn(1, "PrismSkeletonS"),
                Wait(120.0),
                WaitEnemies,
                Wait(60.0),
                Gen(0),
                Wait(300.0),
            ]);
            // D: miasma and the rising lava.
            v.extend([
                MiasmaOn,
                Wait(120.0),
                LavaOn,
                Wait(420.0),
                HazardsOff,
                Wait(60.0),
            ]);
            // End.
            v.extend([tween(tq, end_dir, 3.0, 120.0, 8.0, true), Wait(60.0)]);
            v.extend(four);
            v.extend([Wait(60.0), Missile(h)]);
        }
    }
    v
}

/// Pattern A of Hardcore (and its Legendary variant).
fn hard_a(v: &mut Vec<Fin>, legendary: bool) {
    let q = PI / 4.0;
    let h = PI / 2.0;
    let tq = 3.0 * PI / 4.0;
    let tw = |a, b, len, sep, mirror| Tween {
        a,
        b,
        speed: 3.0,
        len,
        sep,
        mirror,
    };
    let (s3, s6, mid_sep) = if legendary {
        (5.0, 5.0, 7.0)
    } else {
        (6.0, 6.0, 10.0)
    };
    v.extend([
        tw(tq, q, 180.0, 10.0, false),
        Wait(120.0),
        Missile(tq),
        Missile(PI),
        Wait(60.0),
    ]);
    v.extend([
        tw(q, tq, 120.0, 7.0, false),
        Wait(60.0),
        Missile(q),
        Missile(0.0),
        Wait(60.0),
    ]);
    v.extend([
        tw(tq, q, 60.0, s3, false),
        Wait(60.0),
        tw(q, tq, 180.0, 10.0, true),
        Wait(120.0),
    ]);
    if legendary {
        v.push(Missile(h));
    }
    v.extend([Wait(60.0), tw(tq, q, 120.0, mid_sep, true), Wait(90.0)]);
    if legendary {
        v.push(Missile(h));
    }
    v.extend([
        Missile(PI),
        Missile(0.0),
        Wait(30.0),
        tw(q, tq, 60.0, s6, true),
        Wait(60.0),
        Wait(60.0),
    ]);
}

/// A running bullet stream.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BTween {
    pub a: f32,
    pub b: f32,
    pub speed: f32,
    pub len: f32,
    pub sep: f32,
    pub mirror: bool,
    pub t: f32,
    pub timer: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub state: u8,
    pub speed: Vec2,
    pub move_to: Vec2,
    pub move_mult: f32,
    pub sine: f32,
    pub open: bool,
    /// Forced shut by a stomp (ticks left).
    pub closed_t: f32,
    pub charging: bool,
    pub last_target: Option<EntityId>,
    /// Shielded L1 cycle (0, 1, 2), the remembered aim point.
    pub cycle: u8,
    pub aim_at: Vec2,
    /// Vulnerable-phase loop state: L2 arm phase / direction, L3 start,
    /// L4 alternation, the search timer.
    pub l2_num: i32,
    pub l2_dir: f32,
    pub l3_start: f32,
    pub l4_alt: bool,
    pub search: f32,
    pub thresholds: [i32; 3],
    pub n_thresholds: usize,
    pub add: i32,
    pub seq: Seq,
    pub fire: Seq,
    pub target: Option<EntityId>,
    pub aim: f32,
    pub orbs: [Option<EntityId>; 8],
    pub waiting_orbs: bool,
    /// Bullet destroyer: bullets left to kill (4 per tick).
    pub destroying: bool,
    pub slam_t: f32,
    pub slam_from: Vec2,
    pub slam_ctrl: Vec2,
    /// Finale script position and wait.
    pub fin: usize,
    pub fin_wait: f32,
    /// Bullet generator (0 off, 1 checkerboard, 2 spread checker), its
    /// timer and which half fires next.
    pub generator: u8,
    pub gen_t: f32,
    pub gen_row: u8,
    pub tweens: [Option<BTween>; 2],
    pub fan_step: f32,
    pub portal_order: [Option<EntityId>; 4],
    pub lava: Option<EntityId>,
    pub miasma: Option<[EntityId; 2]>,
    pub heart: bool,
    pub heart_health: i32,
    pub adder: bool,
    pub adder_t: f32,
    pub blocks: [Option<EntityId>; 16],
}

impl StableHash for State {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u8(self.state);
        h.vec2(self.speed);
        h.vec2(self.move_to);
        h.f32(self.move_mult);
        h.f32(self.sine);
        h.bool(self.open);
        h.f32(self.closed_t);
        h.i32(self.add);
        self.seq.stable_hash(h);
        self.fire.stable_hash(h);
        h.f32(self.aim);
        h.bool(self.waiting_orbs);
        h.bool(self.destroying);
        h.f32(self.slam_t);
        h.u32(self.fin as u32);
        h.f32(self.fin_wait);
        h.u8(self.generator);
        h.f32(self.gen_t);
        h.u8(self.gen_row);
        for t in self.tweens.iter().flatten() {
            h.f32(t.t);
            h.f32(t.timer);
        }
        h.bool(self.heart);
        h.i32(self.heart_health);
        h.f32(self.adder_t);
    }
}

pub fn spawn(w: &mut World, _c: &Content, mut base: Base) -> BossState {
    base.max_health = 12
        + 3 * base.difficulty
        + if base.players > 4 {
            2 * (base.players - 2)
        } else {
            0
        };
    base.health = base.max_health;
    base.stomp_damage = 2;
    base.midway_auto = false;
    let m = base.max_health;
    let (thresholds, n) = if base.difficulty == 0 {
        ([m / 2, 0, 0], 2)
    } else {
        ([m * 2 / 3, m / 3, 0], 3)
    };
    let body = boss::add_body(
        w,
        Vec2::new(160.0, 120.0),
        Hitbox::new(52.0, 52.0, -26.0, -26.0),
        Some(Hitbox::new(52.0, 52.0, -26.0, -26.0)),
        -250,
        &[
            Tag::Enemy,
            Tag::PlayerCollider,
            Tag::Target,
            Tag::LightSource,
        ],
    );
    w.core_mut(body).collidable = false;
    // The arena's PrismBlocks (spawned by the tower loader) shake and break
    // with the boss.
    let mut blocks = [None; 16];
    let found: Vec<EntityId> = w
        .entities()
        .filter(|&e| matches!(w.kind(e), Some(Kind::Dw(Part::Block { .. }))))
        .take(16)
        .collect();
    for (k, e) in found.into_iter().enumerate() {
        blocks[k] = Some(e);
    }
    BossState {
        id: 3,
        body,
        base,
        kind: BossKind::Cataclysm(State {
            state: INTRO,
            speed: Vec2::ZERO,
            move_to: Vec2::new(160.0, 120.0),
            move_mult: 1.0,
            sine: 0.0,
            open: false,
            closed_t: 0.0,
            charging: false,
            last_target: None,
            cycle: 0,
            aim_at: Vec2::ZERO,
            l2_num: 0,
            l2_dir: 1.0,
            l3_start: 0.0,
            l4_alt: false,
            search: 0.0,
            thresholds,
            n_thresholds: n,
            add: 0,
            seq: Seq::start(0),
            fire: Seq::default(),
            target: None,
            aim: 0.0,
            orbs: [None; 8],
            waiting_orbs: false,
            destroying: false,
            slam_t: -1.0,
            slam_from: Vec2::ZERO,
            slam_ctrl: Vec2::ZERO,
            fin: 0,
            fin_wait: 0.0,
            generator: 0,
            gen_t: 0.0,
            gen_row: 0,
            tweens: [None; 2],
            fan_step: 0.0,
            portal_order: [None; 4],
            lava: None,
            miasma: None,
            heart: false,
            heart_health: 0,
            adder: false,
            adder_t: 0.0,
            blocks,
        }),
    }
}

fn level(s: &State, b: &Base) -> i32 {
    (b.difficulty + s.add).min(4)
}

fn set_open(w: &mut World, id: EntityId, s: &mut State, open: bool) {
    if s.open == open {
        return;
    }
    s.open = open;
    let hb = if open {
        Hitbox::new(56.0, 46.0, -28.0, -21.0)
    } else {
        Hitbox::new(52.0, 52.0, -26.0, -26.0)
    };
    let tg = if open {
        Hitbox::new(40.0, 40.0, -20.0, -20.0)
    } else {
        Hitbox::new(52.0, 52.0, -26.0, -26.0)
    };
    let core = w.core_mut(id);
    core.collider = crate::collide::Collider::Box(hb);
    core.target_collider = Some(tg);
    w.sound(
        if open {
            "en_finalbossEyeOpen"
        } else {
            "en_finalbossEyeClose"
        },
        160.0,
    );
}

fn bullets(w: &World) -> Vec<EntityId> {
    w.entities()
        .filter(|&e| matches!(super::part::get(w, e), Some(Part::Bullet { .. })))
        .collect()
}

/// `CataclysmBulletDestroyer`: 4 bullets per tick in random order.
fn destroyer_tick(w: &mut World, s: &mut State) {
    if !s.destroying {
        return;
    }
    let mut list = bullets(w);
    w.rng.shuffle(&mut list);
    for e in list.iter().take(4) {
        w.remove(*e);
    }
    if list.len() <= 4 {
        s.destroying = false;
    }
}

pub fn fire_bullet(w: &mut World, from: Vec2, angle: f32, speed: f32) {
    // `Position += speed * 30` before the multiplier: always 24 px out.
    let at = from + angle_to_vector(angle, 24.0);
    super::part::add(
        w,
        at,
        Hitbox::new(4.0, 4.0, -2.0, -2.0),
        -260,
        &[Tag::PlayerCollider],
        Part::Bullet {
            t: 0.0,
            vel: angle_to_vector(angle, speed),
        },
    );
}

fn missile(w: &mut World, from: Vec2, deg: f32) {
    let a = deg * DEG;
    super::part::add(
        w,
        from + angle_to_vector(a, 24.0),
        Hitbox::new(6.0, 6.0, -3.0, -3.0),
        -260,
        &[Tag::PlayerCollider, Tag::Target],
        Part::Missile {
            t: 0.0,
            heading: a,
            speed: 0.5,
            normal: angle_to_vector(a, 1.0),
        },
    );
    w.sound("en_finalbossShootMissile", from.x);
}

pub fn update(w: &mut World, c: &Content, id: EntityId, s: &mut State, b: &mut Base) {
    let tm = w.time.time_mult;
    destroyer_tick(w, s);
    if s.closed_t > 0.0 {
        s.closed_t -= tm;
        if s.closed_t <= 0.0 && s.state == FREAKOUT {
            set_open(w, id, s, true);
        }
    }
    movement(w, id, s, tm);
    if s.adder {
        s.adder_t += tm;
        if s.adder_t >= 10.0 {
            s.adder_t = 0.0;
            for (pid, _) in boss::living(w) {
                if let Some(p) = w.player_mut(pid)
                    && p.arrows.can_force_add_arrow(ArrowType::Laser)
                {
                    p.arrows.force_add_arrow(ArrowType::Laser);
                }
            }
        }
    }
    match s.state {
        INTRO => intro(w, id, s, b, tm),
        SAFE => safe(w, c, id, s, b, tm),
        FREAKOUT => freakout(w, c, id, s, b, tm),
        EXPLODING => exploding(w, c, id, s, b, tm),
        _ => heart_phase(w, c, id, s, b, tm),
    }
}

fn movement(w: &mut World, id: EntityId, s: &mut State, tm: f32) {
    if s.slam_t >= 0.0 {
        // Slam: 60 ticks, CubeIn, quadratic curve to (160, 41).
        s.slam_t += tm;
        let x = Ease::CubeIn.apply((s.slam_t / 60.0).min(1.0));
        let to = Vec2::new(160.0, 41.0);
        let a = s.slam_from + (s.slam_ctrl - s.slam_from) * x;
        let bq = s.slam_ctrl + (to - s.slam_ctrl) * x;
        w.core_mut(id).pos = a + (bq - a) * x;
        if s.slam_t >= 60.0 {
            s.slam_t = -1.0;
            s.speed = Vec2::new(0.0, 4.5);
            boss::shake_level(w, 60.0);
            w.sound("en_finalbossRoofSlam", 160.0);
        }
        return;
    }
    s.sine += 2.0 * PI / 240.0 * tm;
    let (ax, ay) = if s.state == EXPLODING {
        (6.0, 4.0)
    } else {
        (16.0, 8.0)
    };
    let goal = s.move_to + Vec2::new(ax * libm::sinf(s.sine), ay * libm::sinf(2.0 * s.sine));
    if s.speed.length() > 0.5 {
        s.speed = approach_vec(s.speed, Vec2::ZERO, 0.3 * tm);
    } else {
        let d = goal - w.core(id).pos;
        let want = if d.length() > 0.5 * s.move_mult {
            d.safe_normalize() * 0.5 * s.move_mult
        } else {
            d
        };
        s.speed = approach_vec(s.speed, want, 0.05 * tm);
    }
    let mut p = w.core(id).pos + s.speed * tm;
    if p.x < 20.0 || p.x > 300.0 {
        p.x = p.x.clamp(20.0, 300.0);
        s.speed.x *= -0.6;
    }
    if p.y < 20.0 || p.y > 220.0 {
        p.y = p.y.clamp(20.0, 220.0);
        s.speed.y *= -0.6;
    }
    w.core_mut(id).pos = p;
}

fn start_slam(w: &World, id: EntityId, s: &mut State) {
    s.slam_from = w.core(id).pos;
    let side = if s.slam_from.x < 160.0 { -20.0 } else { 20.0 };
    s.slam_ctrl = Vec2::new(160.0 + side, 160.0);
    s.slam_t = 0.0;
}

fn intro(w: &mut World, id: EntityId, s: &mut State, b: &Base, tm: f32) {
    if !s.seq.ready(tm) {
        return;
    }
    match s.seq.step {
        0 => {
            super::set_shoot_lock(w, true);
            if super::round_started(w) {
                s.seq.wait(180.0, 1);
            }
        }
        1 => {
            boss::enter_boss_mode(w, b.max_health);
            s.seq.wait(20.0, 2);
        }
        // One arrow from each quiver every 30 ticks until all are empty.
        2 => {
            let mut any = false;
            for (pid, _) in boss::living(w) {
                if let Some(p) = w.player_mut(pid)
                    && p.arrows.use_arrow().is_some()
                {
                    any = true;
                }
            }
            if any {
                w.sound("en_finalbossArrowTaken", 160.0);
                s.seq.wait(30.0, 2);
            } else {
                super::set_shoot_lock(w, false);
                s.seq.k = 0;
                s.seq.next(3);
            }
        }
        // The rock breaks in stages.
        3 => {
            let waits = [60.0, 60.0, 60.0, 30.0, 30.0];
            let k = s.seq.k as usize;
            if k < waits.len() {
                boss::shake_level(w, 15.0);
                w.sound("en_finalbossIntroBreaks", 160.0);
                s.seq.k += 1;
                s.seq.wait(waits[k], 3);
            } else {
                boss::shake_level(w, 60.0);
                w.core_mut(id).collidable = true;
                set_open(w, id, s, true);
                s.seq.wait(80.0, 4);
            }
        }
        _ => {
            set_open(w, id, s, false);
            enter_safe(w, id, s);
        }
    }
}

fn enter_safe(w: &mut World, id: EntityId, s: &mut State) {
    s.state = SAFE;
    w.core_mut(id).collidable = true;
    w.core_mut(id).seek = true;
    s.seq = Seq::start(0);
    s.fire = Seq::default();
    s.waiting_orbs = false;
}

fn safe(w: &mut World, c: &Content, id: EntityId, s: &mut State, b: &Base, tm: f32) {
    if s.waiting_orbs {
        let left = s.orbs.iter().flatten().any(|&o| w.exists(o));
        if !left {
            s.waiting_orbs = false;
            s.destroying = true;
            enter_freakout(w, id, s, b);
            return;
        }
        fire_aimed(w, c, id, s, b, tm);
        return;
    }
    if !s.seq.ready(tm) {
        return;
    }
    match s.seq.step {
        0 => {
            if s.add == 0 {
                s.move_to = Vec2::new(160.0, 60.0);
                s.seq.wait(120.0, 3);
            } else {
                s.seq.wait(60.0, 1);
            }
        }
        1 => {
            if !s.destroying {
                boss::shake_level(w, 60.0);
                s.seq.wait(20.0, 2);
            }
        }
        2 => {
            start_slam(w, id, s);
            s.move_to = Vec2::new(160.0, 60.0);
            s.seq.wait(60.0 + 60.0, 3);
        }
        3 => {
            if !s.destroying {
                // Shield orbs at every BossMarker (level 0: only y > 100).
                w.sound("en_finalbossOrbsSpawn", 160.0);
                let l = level(s, b);
                let markers = c.level(w.level).markers.boss_markers.clone();
                let mut k = 0;
                for m in markers {
                    if (l > 0 || m.pos.y > 100.0) && k < s.orbs.len() {
                        let o = super::part::add(
                            w,
                            m.pos,
                            Hitbox::new(20.0, 8.0, -10.0, 0.0),
                            -240,
                            &[Tag::PlayerCollider, Tag::Target],
                            Part::ShieldOrb {
                                t: 0.0,
                                home: m.pos,
                            },
                        );
                        w.core_mut(o).target_collider = Some(Hitbox::new(20.0, 20.0, -10.0, -10.0));
                        s.orbs[k] = Some(o);
                        k += 1;
                    }
                }
                s.seq.wait(60.0, 4);
            }
        }
        _ => {
            s.waiting_orbs = true;
            s.fire = Seq::start(0);
        }
    }
}

/// `UpdateTarget(takeHidden)`: living archers shuffled, the last target
/// moved to the end; in the vulnerable phase (2+ archers) the first one in
/// the upper middle (y < 120, 120 < x < 200) wins; else the first in line
/// of sight; else none (the original's hidden fallback is overwritten
/// right after it is set, so it never applies).
fn update_target(w: &mut World, c: &Content, id: EntityId, s: &mut State) -> Option<EntityId> {
    let mut list: Vec<EntityId> = boss::living(w).into_iter().map(|(p, _)| p).collect();
    if list.is_empty() {
        s.target = None;
        return None;
    }
    w.rng.shuffle(&mut list);
    if let Some(last) = s.last_target
        && list.len() > 1
        && let Some(k) = list.iter().position(|&p| p == last)
    {
        let p = list.remove(k);
        list.push(p);
    }
    if s.state == FREAKOUT && list.len() > 1 {
        for &p in &list {
            let at = w.core(p).pos;
            if at.y < 120.0 && at.x > 120.0 && at.x < 200.0 {
                s.target = Some(p);
                s.last_target = Some(p);
                return Some(p);
            }
        }
    }
    let from = w.core(id).pos;
    for &p in &list {
        if crate::enemies::can_see(w, c, from, w.core(p).pos, true) {
            s.target = Some(p);
            s.last_target = Some(p);
            return Some(p);
        }
    }
    s.target = None;
    None
}

/// The aim from the boss to `p` (rotated by `rot`).
fn aim(w: &World, id: EntityId, at: Vec2, rot: f32) -> f32 {
    (at - w.core(id).pos).angle() + rot
}

/// Aimed bullets of the shielded phase, by effective level (L0: one aimed
/// bullet / 60; L1: two aimed, then aimed + a +-20 degree pair 20 later;
/// L2: 4 bullets 15 apart, the aim turning at most 10 degrees a step; L3:
/// aimed, +-20 degrees, +-30 degrees; L4: 6 triples (aim, +-50 degrees),
/// tracking as L2).
fn fire_aimed(w: &mut World, c: &Content, id: EntityId, s: &mut State, b: &Base, tm: f32) {
    if !s.fire.ready(tm) {
        return;
    }
    let from = w.core(id).pos;
    let l = level(s, b);
    if s.fire.step == 0 {
        let Some(t) = update_target(w, c, id, s) else {
            s.fire.wait(15.0, 0);
            return;
        };
        s.aim_at = w.core(t).pos;
        s.aim = aim(w, id, s.aim_at, 0.0);
        s.fire.step = 1;
        s.fire.k = 0;
    }
    let tgt = s.target.filter(|&t| w.exists(t)).map(|t| w.core(t).pos);
    match l {
        0 => {
            fire_bullet(w, from, s.aim, 0.8);
            s.fire.wait(60.0, 0);
        }
        1 => match s.fire.step {
            1 if s.cycle == 2 => {
                fire_bullet(w, from, s.aim, 0.8);
                s.fire.wait(20.0, 2);
            }
            2 => {
                fire_bullet(w, from, aim(w, id, s.aim_at, -PI / 9.0), 0.8);
                fire_bullet(w, from, aim(w, id, s.aim_at, PI / 9.0), 0.8);
                s.cycle = (s.cycle + 1) % 3;
                s.fire.wait(60.0, 0);
            }
            _ => {
                fire_bullet(w, from, s.aim, 0.8);
                s.cycle = (s.cycle + 1) % 3;
                s.fire.wait(60.0, 0);
            }
        },
        2 | 4 => {
            let n = if l == 2 { 4 } else { 6 };
            let k = s.fire.k;
            fire_bullet(w, from, s.aim, 0.8);
            if l == 4 {
                fire_bullet(w, from, s.aim - 0.872_664_6, 0.8);
                fire_bullet(w, from, s.aim + 0.872_664_6, 0.8);
            }
            s.fire.k += 1;
            if k < n - 1 {
                if let Some(t) = tgt {
                    let want = (t - from).angle();
                    s.aim += angle_diff(s.aim, want).clamp(-PI / 18.0, PI / 18.0);
                }
                s.fire.wait(15.0, 1);
            } else {
                // The last wait(15) and then wait(50) / wait(60).
                s.fire.wait(15.0 + if l == 2 { 51.0 } else { 61.0 }, 0);
            }
        }
        _ => match s.fire.step {
            1 => {
                fire_bullet(w, from, s.aim, 0.8);
                s.fire.wait(20.0, 2);
            }
            2 => {
                fire_bullet(w, from, aim(w, id, s.aim_at, -PI / 9.0), 0.8);
                fire_bullet(w, from, aim(w, id, s.aim_at, PI / 9.0), 0.8);
                s.fire.wait(10.0, 3);
            }
            _ => {
                fire_bullet(w, from, aim(w, id, s.aim_at, -PI / 6.0), 0.8);
                fire_bullet(w, from, aim(w, id, s.aim_at, PI / 6.0), 0.8);
                s.fire.wait(60.0, 0);
            }
        },
    }
}

fn enter_freakout(w: &mut World, id: EntityId, s: &mut State, b: &Base) {
    s.state = FREAKOUT;
    s.move_to = Vec2::new(160.0, 120.0);
    // Blades.
    let from = w.core(id).pos;
    let n = if b.difficulty >= 2 {
        if s.add < 2 { 1 } else { 2 }
    } else if s.add >= 1 {
        1
    } else {
        0
    };
    for k in 0..n {
        let dir = if n == 2 {
            if k == 0 { 1.0 } else { -1.0 }
        } else if w.rng.next_max(2) == 0 {
            1.0
        } else {
            -1.0
        };
        let to = if n == 2 {
            if k == 0 {
                Vec2::new(130.0, 210.0)
            } else {
                Vec2::new(190.0, 210.0)
            }
        } else {
            Vec2::new(160.0, 210.0)
        };
        super::part::add(
            w,
            from,
            Hitbox::new(4.0, 4.0, -2.0, -2.0),
            -230,
            &[Tag::PlayerCollider, Tag::Target],
            Part::Blade {
                t: 0.0,
                from,
                to,
                dir,
                pause: 0.0,
                spin: 0.0,
            },
        );
        if k == 0 {
            w.sound("en_finalbossBladeSpawn", from.x);
        }
    }
    s.seq = Seq::start(0);
    s.seq.wait(30.0, 1);
}

fn kill_blades(w: &mut World) {
    let list: Vec<EntityId> = w
        .entities()
        .filter(|&e| matches!(super::part::get(w, e), Some(Part::Blade { .. })))
        .collect();
    for e in list {
        w.remove(e);
    }
}

fn spray(w: &mut World, from: Vec2, angles: &[f32]) {
    for &a in angles {
        fire_bullet(w, from, a, 0.8);
    }
}

const DIAG: [f32; 4] = [-PI / 4.0, -3.0 * PI / 4.0, PI / 4.0, 3.0 * PI / 4.0];
/// `ShootPatternA` / `ShootPatternB` (effective level 4).
const PATTERN_A: [f32; 12] = [
    0.0,
    PI / 12.0,
    -PI / 12.0,
    PI,
    3.403_392,
    11.0 * PI / 12.0,
    -PI / 2.0,
    -1.308_997,
    -7.0 * PI / 12.0,
    PI / 2.0,
    7.0 * PI / 12.0,
    1.308_997,
];
const PATTERN_B: [f32; 12] = [
    -3.0 * PI / 4.0,
    -2.0 * PI / 3.0,
    -2.617_994,
    PI / 4.0,
    PI / 3.0,
    PI / 6.0,
    -PI / 4.0,
    -PI / 6.0,
    -PI / 3.0,
    3.0 * PI / 4.0,
    2.617_994,
    2.0 * PI / 3.0,
];

/// The vulnerable phase (`IdleFreakoutCoroutine`), by effective level.
fn freakout(w: &mut World, c: &Content, id: EntityId, s: &mut State, b: &mut Base, tm: f32) {
    // Health threshold: back to the shielded phase.
    let th = s.thresholds[s.add.min(2) as usize];
    if b.health <= th && (s.add as usize) + 1 < s.n_thresholds {
        s.add += 1;
        set_open(w, id, s, false);
        s.destroying = true;
        s.charging = false;
        kill_blades(w);
        enter_safe(w, id, s);
        return;
    }
    if b.health <= 0 {
        start_exploding(w, id, s);
        return;
    }
    if !s.seq.ready(tm) {
        return;
    }
    let from = w.core(id).pos;
    let l = level(s, b);
    match s.seq.step {
        1 => {
            if !s.destroying {
                w.sound("en_finalbossEyeOpen", 160.0);
                boss::shake_level(w, 30.0);
                s.seq.wait(30.0, 2);
            }
        }
        2 => {
            set_open(w, id, s, true);
            s.seq.wait(60.0, 3);
        }
        // Loop start: SlowDown and the bursts.
        3 => {
            s.move_mult = 0.3;
            s.speed *= 0.5;
            match l {
                0 => {
                    spray(w, from, &DIAG);
                    s.seq.wait(60.0, 4);
                }
                1 => {
                    spray(w, from, &DIAG);
                    spray(w, from, &[-PI / 2.0, PI / 2.0]);
                    s.seq.wait(60.0, 4);
                }
                2 => {
                    s.seq.k = 0;
                    s.seq.next(5);
                }
                3 => {
                    let starts = [
                        0.0,
                        -PI / 4.0,
                        -PI / 2.0,
                        -3.0 * PI / 4.0,
                        PI,
                        3.0 * PI / 4.0,
                        PI / 2.0,
                        PI / 4.0,
                    ];
                    s.l3_start = starts[w.rng.choose_index(8)];
                    s.seq.k = 0;
                    s.seq.next(7);
                }
                _ => {
                    spray(w, from, if s.l4_alt { &PATTERN_B } else { &PATTERN_A });
                    s.seq.wait(60.0, 9);
                }
            }
        }
        4 => {
            if l == 0 {
                spray(w, from, &[0.0, PI, -PI / 2.0, PI / 2.0]);
            } else {
                spray(w, from, &DIAG);
                spray(w, from, &[PI, 0.0]);
            }
            s.move_mult = 1.0;
            s.seq.wait(60.0, 10);
        }
        // L2: four arms, 4 steps 10 apart.
        5 => {
            let j = s.seq.k as f32;
            let off = j * (-PI / 2.0) / 5.0 * s.l2_dir + s.l2_num as f32 * (-PI / 4.0);
            spray(w, from, &[-PI / 2.0 + off, off, PI + off, PI / 2.0 + off]);
            s.seq.k += 1;
            s.seq.wait(10.0, if s.seq.k < 4 { 5 } else { 6 });
        }
        6 => {
            s.move_mult = 1.0;
            s.seq.wait(30.0, 10);
        }
        // L3: 13 opposite pairs 30 degrees apart, 5 ticks each.
        7 => {
            let a = s.l3_start + s.seq.k as f32 * (PI / 6.0);
            spray(w, from, &[a, a + PI]);
            s.seq.k += 1;
            s.seq.wait(5.0, if s.seq.k < 13 { 7 } else { 8 });
        }
        8 => {
            s.move_mult = 1.0;
            s.seq.wait(60.0, 10);
        }
        // L4: the second pattern.
        9 => {
            spray(w, from, if s.l4_alt { &PATTERN_A } else { &PATTERN_B });
            s.l4_alt = !s.l4_alt;
            s.move_mult = 1.0;
            s.seq.wait(30.0, 10);
        }
        // SearchSequence: up to 60 ticks for a visible target.
        10 => {
            s.search = 60.0;
            s.target = None;
            s.seq.next(11);
        }
        11 => {
            let found = s.open && update_target(w, c, id, s).is_some();
            s.search -= tm;
            if found || s.search <= 0.0 {
                if s.open && s.target.is_some() {
                    s.charging = true;
                    w.sound("en_finalbossChargeUp", from.x);
                    s.seq.wait(60.0, 12);
                } else {
                    s.seq.wait(60.0, 20);
                }
            }
        }
        // The shots of the charge.
        12 => {
            match l {
                3 => laser(w, c, id, s, -PI / 9.0, false),
                _ => laser(w, c, id, s, 0.0, false),
            }
            let wt = if l == 4 { 30.0 } else { 5.0 };
            s.seq.wait(wt, 13);
        }
        13 => match l {
            0 | 4 => {
                laser(w, c, id, s, 0.0, false);
                s.seq.wait(10.0, 14);
            }
            1 | 2 => {
                laser(w, c, id, s, PI / 12.0, true);
                s.seq.wait(10.0, 14);
            }
            _ => {
                laser(w, c, id, s, 0.0, false);
                s.seq.wait(5.0, 15);
            }
        },
        15 => {
            laser(w, c, id, s, PI / 9.0, false);
            s.seq.wait(10.0, 14);
        }
        14 => {
            s.charging = false;
            s.seq.wait(if l == 1 || l == 2 { 50.0 } else { 20.0 }, 20);
        }
        // Loop end.
        _ => {
            if l == 2 {
                s.l2_num = (s.l2_num + 1) % 2;
                s.l2_dir = -s.l2_dir;
            }
            s.seq.next(3);
        }
    }
}

/// `Shoot` / `ShootSpread` / `ShootDoubleSpread`: a laser arrow from the
/// rounded boss position at the target rotated by `rot` (both `+-rot` when
/// `double`); refused while an 8x8 box at the boss overlaps a Solid; the
/// boss recoils at 2 away from the target.
fn laser(w: &mut World, c: &Content, id: EntityId, s: &mut State, rot: f32, double: bool) {
    if !s.open {
        return;
    }
    let from = w.core(id).pos;
    let probe = crate::collide::Placed {
        pos: Vec2::new(from.x.trunc() - 4.0, from.y.trunc() - 4.0),
        collider: crate::collide::Collider::Box(Hitbox::new(8.0, 8.0, 0.0, 0.0)),
    };
    if w.any(c, &probe, Tag::Solid, id) {
        return;
    }
    let Some(t) = s.target.filter(|&t| w.exists(t)) else {
        return;
    };
    let tp = w.core(t).pos;
    let at = Vec2::new(crate::math::round_he(from.x), crate::math::round_he(from.y));
    let base = (tp - at).angle();
    let shooter = arrow::Shooter {
        id: Some(id),
        player_index: -1,
        allegiance: Allegiance::Red,
        team_color: Allegiance::Red,
        hyper: false,
    };
    let rots: &[f32] = if double { &[rot, -rot] } else { &[rot] };
    for &r in rots {
        let ar = arrow::create(w, ArrowType::Laser, shooter, at, base + r);
        if let Some(x) = w.arrow_mut(ar) {
            x.cannot_hit_enemies = 10.0;
        }
    }
    s.speed = (at - tp).safe_normalize() * 2.0;
    w.screen_shake(10.0);
    w.sound("en_finalbossShootArrow", from.x);
}

fn start_exploding(w: &mut World, id: EntityId, s: &mut State) {
    s.state = EXPLODING;
    s.speed = Vec2::ZERO;
    set_open(w, id, s, false);
    super::set_shoot_lock(w, false);
    s.destroying = true;
    kill_blades(w);
    if let Some(d) = w.dark_world.as_mut() {
        d.boss_mode = false;
    }
    super::signal_boss_treasure(w);
    w.sound("en_finalbossBullet2", 160.0);
    s.seq = Seq::start(0);
    s.seq.wait(60.0, 1);
}

fn exploding(w: &mut World, c: &Content, id: EntityId, s: &mut State, b: &Base, tm: f32) {
    // Generators and bullet streams of the finale.
    let from = w.core(id).pos;
    if s.generator > 0 {
        s.gen_t -= tm;
        if s.gen_t <= 0.0 {
            s.gen_t += 121.0;
            let first = s.gen_row.is_multiple_of(2);
            s.gen_row += 1;
            if s.generator == 1 {
                // Checkerboard: 5 at 3pi/4 - k*pi/8, then 4 at 11pi/16 - k*pi/8.
                let (start, n) = if first {
                    (3.0 * PI / 4.0, 5)
                } else {
                    (11.0 * PI / 16.0, 4)
                };
                for k in 0..n {
                    fire_bullet(w, from, start - PI / 8.0 * k as f32, 0.8 * 0.6);
                }
            } else {
                // Spread checker: clusters of 3 (pi/24 apart) at 3pi/4 and
                // the 40 % / 80 % points toward pi/4, then at 20 % / 60 %.
                let tq = 3.0 * PI / 4.0;
                let at = |t: f32| lerpf(tq, PI / 4.0, t);
                let centres: &[f32] = if first {
                    &[tq, at(0.4), at(0.8)]
                } else {
                    &[at(0.2), at(0.6)]
                };
                for k in 0..3 {
                    for &cc in centres {
                        fire_bullet(w, from, cc - PI / 24.0 * k as f32, 0.8 * 0.6);
                    }
                }
            }
        }
    }
    for slot in s.tweens.iter_mut() {
        let Some(mut tw) = *slot else { continue };
        tw.t += tm;
        tw.timer -= tm;
        let x = Ease::CubeInOut.apply((tw.t / tw.len).min(1.0));
        if tw.timer <= 0.0 {
            tw.timer += tw.sep;
            let a = lerpf(tw.a, tw.b, x);
            if tw.mirror {
                if angle_diff(a, PI / 2.0).abs() <= PI / 60.0 {
                    fire_bullet(w, from, PI / 2.0, 0.8 * tw.speed);
                } else {
                    fire_bullet(w, from, a, 0.8 * tw.speed);
                    fire_bullet(w, from, PI - a, 0.8 * tw.speed);
                }
            } else {
                fire_bullet(w, from, a, 0.8 * tw.speed);
            }
        }
        *slot = (tw.t < tw.len).then_some(tw);
    }
    if !s.seq.ready(tm) {
        return;
    }
    match s.seq.step {
        1 => {
            if !s.destroying {
                start_slam(w, id, s);
                s.seq.wait(60.0, 2);
            }
        }
        2 => {
            w.set_depth(id, -400);
            s.move_to = Vec2::new(160.0, 40.0);
            s.seq.wait(60.0, 3);
        }
        3 => {
            for bk in s.blocks.iter().flatten() {
                if let Some(Part::Block {
                    start,
                    node,
                    c: cc,
                    sink,
                    ride,
                    ..
                }) = super::part::get(w, *bk)
                {
                    super::part::set(
                        w,
                        *bk,
                        Part::Block {
                            start,
                            node,
                            c: cc,
                            sink,
                            ride,
                            shake: 60.0,
                        },
                    );
                }
            }
            let mut order: Vec<usize> = (0..s.blocks.len())
                .filter(|&k| s.blocks[k].is_some())
                .collect();
            w.rng.shuffle(&mut order);
            let mut sorted = [None; 16];
            for (k, &o) in order.iter().enumerate() {
                sorted[k] = s.blocks[o];
            }
            s.blocks = sorted;
            s.seq.k = 0;
            s.seq.wait(60.0, 4);
        }
        // The blocks break one by one.
        4 => {
            let k = s.seq.k as usize;
            if let Some(bk) = s.blocks.get(k).copied().flatten() {
                w.sound("en_finalbossPlatformShatter", w.core(bk).pos.x);
                w.remove(bk);
                s.seq.k += 1;
                let waits = [10.0, 10.0, 20.0, 20.0, 30.0];
                let pick = waits[w.rng.choose_index(waits.len())];
                s.seq.wait(pick, 4);
            } else {
                s.seq.wait(90.0, 5);
            }
        }
        5 => {
            w.sound("en_finalbossEyeOpen", 160.0);
            s.seq.wait(30.0, 6);
        }
        6 => {
            set_open(w, id, s, true);
            s.seq.wait(60.0, 7);
        }
        7 => {
            w.sound("en_finalbossEnrage", 160.0);
            s.seq.wait(60.0 + 60.0, 8);
        }
        // The finale script: run instructions until one waits.
        8 => {
            let script = finale(b.difficulty);
            for _ in 0..64 {
                let Some(&op) = script.get(s.fin) else {
                    s.generator = 0;
                    s.seq.wait(60.0, 9);
                    return;
                };
                s.fin += 1;
                let from = w.core(id).pos;
                match op {
                    Wait(t) => {
                        s.seq.wait(t, 8);
                        return;
                    }
                    BurstA(dirs, n) => {
                        for &d in &dirs[..n as usize] {
                            for o in [PI / 10.0, PI / 20.0, 0.0, -PI / 20.0, -PI / 10.0] {
                                fire_bullet(w, from, d + o, 0.8 * 3.0);
                            }
                        }
                        w.sound("en_finalbossBullet", from.x);
                    }
                    BurstB(dirs, n) => {
                        for &d in &dirs[..n as usize] {
                            for o in [3.0 * PI / 40.0, PI / 40.0, -PI / 40.0, -3.0 * PI / 40.0] {
                                fire_bullet(w, from, d + o, 0.8 * 3.0);
                            }
                        }
                    }
                    FanInit => {
                        let full = PI / 2.0 - (Vec2::new(20.0, 160.0) - from).angle();
                        s.fan_step = full / 4.0;
                    }
                    Fan(wide) => {
                        let st = s.fan_step;
                        if wide {
                            for l in -4..=5 {
                                fire_bullet(
                                    w,
                                    from,
                                    PI / 2.0 - st * 0.5 + st * l as f32,
                                    0.8 * 0.8,
                                );
                            }
                        } else {
                            for l in -4..=4 {
                                fire_bullet(w, from, PI / 2.0 + st * l as f32, 0.8 * 0.8);
                            }
                        }
                    }
                    Missile(d) => missile(w, from, d / DEG),
                    Gen(g) => {
                        s.generator = g;
                        s.gen_t = 0.0;
                        s.gen_row = 0;
                    }
                    Tween {
                        a,
                        b: bb,
                        speed,
                        len,
                        sep,
                        mirror,
                    } => {
                        let tw = BTween {
                            a,
                            b: bb,
                            speed,
                            len,
                            sep,
                            mirror,
                            t: 0.0,
                            timer: 0.0,
                        };
                        if let Some(slot) = s.tweens.iter_mut().find(|t| t.is_none()) {
                            *slot = Some(tw);
                        } else {
                            s.tweens[0] = Some(tw);
                        }
                    }
                    ShufflePortals => {
                        let mut ps: Vec<EntityId> = w
                            .dark_world
                            .as_ref()
                            .map(|d| d.portals.iter().map(|p| p.id).collect())
                            .unwrap_or_default();
                        w.rng.shuffle(&mut ps);
                        s.portal_order = [None; 4];
                        for (k, p) in ps.into_iter().take(4).enumerate() {
                            s.portal_order[k] = Some(p);
                        }
                    }
                    Spawn(k, name) => {
                        let ty = EnemyType::from_name(name).unwrap_or(EnemyType::Exploder);
                        if let Some(p) = s.portal_order[k as usize].or(s.portal_order[0]) {
                            portal::appear_and_spawn(w, p, ty);
                        }
                    }
                    WaitEnemies => {
                        if crate::enemies::enemy_count(w) > 1 {
                            s.fin -= 1;
                            s.seq.next(8);
                            return;
                        }
                    }
                    MiasmaOn => {
                        if s.miasma.is_none() {
                            s.miasma = Some(crate::versus::miasma::spawn_mode(
                                w,
                                crate::versus::miasma::MiasmaMode::Cataclysm,
                            ));
                        }
                    }
                    LavaOn => s.lava = Some(crate::entities::hazards::start_boss_lava(w, c)),
                    HazardsOff => {
                        if let Some(l) = s.lava {
                            crate::entities::hazards::boss_lava_finish(w, l);
                        }
                        if let Some(m) = s.miasma.take() {
                            crate::versus::miasma::dissipate(w, m);
                        }
                    }
                }
            }
        }
        // The heart phase waits while the level is ending (a game over).
        9 => {
            if w.dark_world.as_ref().is_some_and(|d| d.ending) {
                return;
            }
            if let Some(m) = s.miasma.take() {
                crate::versus::miasma::dissipate(w, m);
            }
            start_heart(w, id, s);
        }
        _ => {}
    }
}

fn start_heart(w: &mut World, id: EntityId, s: &mut State) {
    s.state = DEAD;
    s.seq = Seq::start(0);
    s.seq.wait(60.0, 1);
    let _ = (w, id);
}

fn heart_phase(w: &mut World, c: &Content, id: EntityId, s: &mut State, b: &Base, tm: f32) {
    if !s.seq.ready(tm) {
        return;
    }
    let from = w.core(id).pos;
    match s.seq.step {
        // Explosions around the boss: 5 x 30 ticks, then 5 x 15.
        // wait(60) is over: shooting locks, the shell drifts up and real
        // explosions (credited to no one) go off around it.
        1 => {
            w.sound("en_finalbossDeathExplode", from.x);
            super::set_shoot_lock(w, true);
            s.move_to = Vec2::new(160.0, 100.0);
            s.seq.k = 0;
            s.seq.next(2);
        }
        2 => {
            let k = s.seq.k;
            if k < 10 {
                let off = Vec2::new(w.rng.range_f(-30.0, 60.0), w.rng.range_f(-30.0, 60.0));
                crate::explosion::spawn(w, c, from + off, -1, false, &mut None);
                s.seq.k += 1;
                s.seq.wait(if k < 5 { 30.0 } else { 15.0 }, 2);
            } else {
                s.seq.wait(30.0, 10);
            }
        }
        // The shell closes for good; wait(30); the 48-tick "die" animation.
        10 => {
            set_open(w, id, s, false);
            s.seq.wait(30.0, 11);
        }
        11 => s.seq.wait(48.0, 3),
        3 => {
            for o in [
                Vec2::ZERO,
                Vec2::new(-20.0, -20.0),
                Vec2::new(20.0, -20.0),
                Vec2::new(-20.0, 20.0),
                Vec2::new(20.0, 20.0),
            ] {
                crate::explosion::spawn(w, c, from + o, -1, false, &mut None);
            }
            s.seq.wait(1.0, 4);
        }
        4 => {
            // The heart.
            s.heart = true;
            s.move_mult = 0.2;
            let set_mask = 0xffff;
            if let Some(d) = w.dark_world.as_mut() {
                d.settings
                    .variants
                    .set_all(Variant::NoFriendlyFire, true, set_mask);
                d.settings
                    .variants
                    .set_all(Variant::SuperSeekingArrows, true, set_mask);
            }
            w.setup
                .variant_set
                .set_all(Variant::NoFriendlyFire, true, set_mask);
            w.setup
                .variant_set
                .set_all(Variant::SuperSeekingArrows, true, set_mask);
            let core = w.core_mut(id);
            core.collidable = true;
            core.collider = crate::collide::Collider::Box(Hitbox::new(16.0, 16.0, -8.0, -8.0));
            core.target_collider = Some(Hitbox::new(16.0, 16.0, -8.0, -8.0));
            s.seq.wait(120.0, 5);
        }
        5 => {
            s.adder = true;
            s.seq.wait(60.0, 6);
        }
        6 => {
            super::set_shoot_lock(w, false);
            s.heart_health = 10 + 4 * b.players;
            s.seq.next(7);
        }
        7 => {
            if s.heart_health <= 0 {
                // The heart dies: quivers emptied, arrows gone.
                w.sound("en_finalbossHeartDeathSequence", from.x);
                s.adder = false;
                for (pid, _) in boss::living(w) {
                    if let Some(p) = w.player_mut(pid) {
                        p.arrows = crate::quiver::Quiver::default();
                    }
                }
                if let Some(d) = w.dark_world.as_mut() {
                    for inv in d.last_inv.iter_mut() {
                        inv.arrows = crate::quiver::Quiver::default();
                    }
                }
                let arrows: Vec<EntityId> = w.tagged(Tag::Arrow).to_vec();
                for a in arrows {
                    w.remove(a);
                }
                w.core_mut(id).collidable = false;
                w.tag(id, Tag::Enemy, false);
                s.move_to = Vec2::new(160.0, 120.0);
                super::set_shoot_lock(w, true);
                s.seq.wait(180.0, 12);
            } else {
                if w.time.on_interval(60) {
                    w.sound("en_finalbossHeartBeat", from.x);
                }
            }
        }
        // Shooting returns once the heart has faded.
        12 => {
            super::set_shoot_lock(w, false);
            s.seq.wait(120.0 + 240.0 + 420.0, 8);
        }
        8 => {
            boss::shake_level(w, 150.0);
            s.seq.wait(360.0, 9);
        }
        _ => {
            super::boss_finished(w);
            w.remove(id);
            s.seq.stop();
        }
    }
}

// ----------------------------------------------------------------- hits

fn heart_hit(w: &mut World, id: EntityId, s: &mut State, kick: Vec2) {
    s.heart_health -= 1;
    s.speed += kick;
    w.sound("en_finalbossHeartHurt", w.core(id).pos.x);
}

pub fn body_arrow(w: &mut World, _c: &Content, id: EntityId, aid: EntityId, a: &mut Arrow) -> bool {
    if !a.dangerous() || a.player_index < 0 {
        return false;
    }
    let killer = a.player_index;
    let speed = a.speed;
    let r = boss::with_boss(w, |w, bs| {
        let (BossKind::Cataclysm(s), base) = (&mut bs.kind, &mut bs.base) else {
            return 0;
        };
        if s.heart && s.state == DEAD {
            let perp = Vec2::new(-speed.y, speed.x).safe_normalize();
            let sign = if w.rng.next_max(2) == 0 { 1.0 } else { -1.0 };
            heart_hit(w, id, s, perp * sign);
            return 2;
        }
        if s.state == FREAKOUT && s.open {
            boss::hurt(w, base, 1, killer);
            s.speed = speed.safe_normalize() * 0.5;
            w.sound("en_finalbossHurt", w.core(id).pos.x);
            return 2;
        }
        w.sound("en_finalbossShieldedHit", w.core(id).pos.x);
        1
    })
    .unwrap_or(0);
    match r {
        2 => {
            w.remove(aid);
            w.core_mut(aid).collidable = false;
            true
        }
        1 => {
            arrow::enter_fall_mode(w, aid, a, true, false, true);
            true
        }
        _ => false,
    }
}

pub fn body_touch(w: &mut World, c: &Content, id: EntityId, pid: EntityId, p: &mut Player) {
    let stomp = boss::is_stomp(w, id, pid, p);
    let at = w.core(pid).pos;
    boss::with_boss(w, |w, bs| {
        let (BossKind::Cataclysm(s), base) = (&mut bs.kind, &mut bs.base) else {
            return;
        };
        let center = w.core(id).pos;
        if s.heart && s.state == DEAD {
            if stomp {
                boss::bounce(w, c, id, pid, p);
                heart_hit(
                    w,
                    id,
                    s,
                    Vec2::new(if at.x < center.x { 2.0 } else { -2.0 }, 0.0),
                );
            } else if p.speed.y >= 0.0 || at.y > center.y {
                boss::hurt_player(w, c, pid, p, center);
            }
            return;
        }
        // The closed (or exploding) shell is spiky: anyone below it or
        // coming down onto it is hurt, stomps included.
        if !s.open || s.state >= EXPLODING {
            if at.y > center.y || p.speed.y >= 0.0 {
                boss::hurt_player(w, c, pid, p, center);
                s.speed = (center - at).safe_normalize();
            }
            return;
        }
        if s.state == FREAKOUT && stomp {
            boss::bounce(w, c, id, pid, p);
            p.speed.x = super::amaranth::shove(at.x, center.x);
            let dmg = boss::stomp_damage(base);
            boss::hurt(w, base, dmg, p.index as i8);
            if !s.charging {
                set_open(w, id, s, false);
                s.closed_t = 120.0;
            }
            s.speed = Vec2::new(if at.x < center.x { 3.0 } else { -3.0 }, 0.4);
            return;
        }
        if stomp {
            boss::bounce(w, c, id, pid, p);
            return;
        }
        if at.y > center.y || p.speed.y >= 0.0 || s.open {
            boss::hurt_player(w, c, pid, p, center);
            s.speed = (center - at).safe_normalize();
        }
    });
}

// ----------------------------------------------------------------- parts

pub fn part_update(w: &mut World, c: &Content, id: EntityId, p: &mut Part) {
    let tm = w.time.time_mult;
    match p {
        Part::Bullet { t, vel } => {
            *t += tm;
            let pos = w.core(id).pos + *vel * tm;
            w.core_mut(id).pos = pos;
            if !(-10.0..=330.0).contains(&pos.x) || !(-10.0..=250.0).contains(&pos.y) {
                w.remove(id);
            }
        }
        // Homes on the archer nearest its heading; the first 30 ticks it
        // slides along walls (the blocked component is dropped), after
        // that touching a Solid blows it up.
        Part::Missile {
            t,
            heading,
            speed,
            normal,
        } => {
            *t += tm;
            let at = w.core(id).pos;
            let best = boss::living(w)
                .into_iter()
                .map(|(_, p)| angle_diff(*heading, (p - at).angle()))
                .min_by(|a, b| a.abs().total_cmp(&b.abs()));
            if let Some(d) = best {
                let r = d.clamp(-PI / 225.0 * tm, PI / 225.0 * tm);
                let (sn, cs) = (libm::sinf(r), libm::cosf(r));
                *normal = Vec2::new(normal.x * cs - normal.y * sn, normal.x * sn + normal.y * cs);
                *heading += r;
            }
            *speed = crate::math::approach(*speed, 2.5, 0.05 * tm);
            let step = *speed * tm;
            if *t <= 30.0 {
                let mut p = at;
                p.x += normal.x * step;
                if w.check_at(c, id, p, Tag::Solid) {
                    p.x -= normal.x * step;
                    normal.x = 0.0;
                    *normal = normal.safe_normalize();
                }
                p.y += normal.y * step;
                if w.check_at(c, id, p, Tag::Solid) {
                    p.y -= normal.y * step;
                    normal.y = 0.0;
                    *normal = normal.safe_normalize();
                }
                w.core_mut(id).pos = p;
            } else {
                let p = at + *normal * step;
                if w.check_at(c, id, p, Tag::Solid) {
                    missile_explode(w, c, id);
                } else {
                    w.core_mut(id).pos = p;
                }
            }
        }
        Part::Blade {
            t,
            from,
            to,
            dir,
            pause,
            spin,
        } => {
            *t += tm;
            *spin += PI / 30.0 * tm;
            if *t <= 60.0 {
                let x = Ease::CubeOut.apply(*t / 60.0);
                w.core_mut(id).pos = *from + (*to - *from) * x;
                return;
            }
            if *pause > 0.0 {
                *pause -= tm;
                return;
            }
            let mut pos = w.core(id).pos;
            pos.x += *dir * tm;
            pos.y = to.y + 3.0 * libm::sinf(*t * 2.0 * PI / 120.0);
            if pos.x <= 25.0 || pos.x >= 295.0 {
                pos.x = pos.x.clamp(25.0, 295.0);
                *dir = -*dir;
                *pause = 20.0;
            }
            w.core_mut(id).pos = pos;
        }
        Part::ShieldOrb { t, home } => {
            *t += tm;
            let o = Vec2::new(
                4.0 * libm::sinf(*t * 2.0 * PI / 120.0),
                6.0 * libm::sinf(*t * 4.0 * PI / 120.0),
            );
            w.core_mut(id).pos = *home + o;
        }
        Part::Block {
            start,
            node,
            c: cc,
            sink,
            ride,
            shake,
        } => {
            *cc += 2.0 * PI / 300.0 * tm;
            if *shake > 0.0 {
                *shake -= tm;
            }
            // A rider sinks the block up to 5 px (and for 10 ticks after).
            let me = w.placed(id);
            let probe = crate::collide::Placed {
                pos: me.pos + Vec2::new(0.0, -1.0),
                collider: me.collider,
            };
            let riding = w.any(c, &probe, Tag::Player, id);
            if riding {
                *ride = 10.0;
            } else if *ride > 0.0 {
                *ride -= tm;
            }
            *sink = crate::math::approach(*sink, if *ride > 0.0 { 5.0 } else { 0.0 }, 0.25 * tm);
            let k = 0.5 + 0.5 * libm::sinf(*cc);
            let target = *start + (*node - *start) * k + Vec2::new(0.0, *sink);
            let d = target - w.core(id).pos;
            if !d.is_zero() {
                crate::platform::solid_move(w, c, id, d);
            }
        }
        _ => {}
    }
}

/// A `PrismBlock` placement: a solid `CataclysmBlock` (tower-entities.md
/// §5.6), bobbing from `start` toward its node.
pub fn spawn_block(w: &mut World, p: &crate::entities::Placement) -> EntityId {
    let node = p.node.unwrap_or(p.pos);
    super::part::add(
        w,
        p.pos,
        Hitbox::new(p.size.x.max(10.0), p.size.y.max(10.0), 0.0, 0.0),
        -150,
        &[Tag::Solid],
        Part::Block {
            start: p.pos,
            node,
            c: 3.0 * PI / 2.0,
            sink: 0.0,
            ride: 0.0,
            shake: 0.0,
        },
    )
}

/// The missile's bomb-sized explosion (credited to no one).
pub fn missile_explode(w: &mut World, c: &Content, id: EntityId) {
    if !w.exists(id) || w.core(id).marked_for_removal {
        return;
    }
    let at = w.core(id).pos;
    w.remove(id);
    w.core_mut(id).collidable = false;
    crate::explosion::spawn(w, c, at, -1, false, &mut None);
}

pub fn part_touch(w: &mut World, c: &Content, id: EntityId, pid: EntityId, pl: &mut Player) {
    match super::part::get(w, id) {
        Some(Part::Bullet { t, .. }) => {
            if t >= 15.0 {
                boss::hurt_player(w, c, pid, pl, w.core(id).pos);
            }
            w.remove(id);
            w.core_mut(id).collidable = false;
        }
        Some(Part::Missile { t, .. }) => {
            if t > 20.0 {
                missile_explode(w, c, id);
            }
        }
        Some(Part::Blade { .. }) => boss::hurt_player(w, c, pid, pl, w.core(id).pos),
        Some(Part::ShieldOrb { .. }) => {
            let top = w.core(id).top();
            let pc = w.core(pid);
            if pl.speed.y >= 0.0 && pc.bottom() - pl.speed.y <= top + 6.0 {
                crate::player::bounce_pub(w, c, pid, pl, top, false);
                w.sound("en_finalbossOrbDestroy", w.core(id).pos.x);
                w.remove(id);
                w.core_mut(id).collidable = false;
            }
        }
        _ => {}
    }
}

pub fn part_arrow(w: &mut World, c: &Content, id: EntityId, aid: EntityId, a: &mut Arrow) -> bool {
    match super::part::get(w, id) {
        Some(Part::Missile { .. }) => {
            missile_explode(w, c, id);
            arrow::enter_fall_mode(w, aid, a, true, false, false);
            true
        }
        Some(Part::Blade {
            t,
            from,
            to,
            dir,
            spin,
            ..
        }) => {
            w.sound("en_finalbossBladeHit", w.core(id).pos.x);
            super::part::set(
                w,
                id,
                Part::Blade {
                    t,
                    from,
                    to,
                    dir,
                    pause: 30.0,
                    spin,
                },
            );
            w.remove(aid);
            w.core_mut(aid).collidable = false;
            true
        }
        Some(Part::ShieldOrb { .. }) => {
            if a.player_index < 0 {
                return false;
            }
            w.sound("en_finalbossOrbDestroy", w.core(id).pos.x);
            w.remove(id);
            w.core_mut(id).collidable = false;
            w.remove(aid);
            w.core_mut(aid).collidable = false;
            true
        }
        _ => false,
    }
}
