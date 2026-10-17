//! Bat family: eye bat, bomb bat, super bomb bat, crow (enemies.md §4.2).

use super::attack::{self, OnHit};
use super::*;
use crate::math::{approach, min};

pub const IDLE: u8 = 0;
pub const SWOOP: u8 = 1;
pub const DEAD: u8 = 2;
const COOLDOWN: usize = 0;
const PASS: usize = 0;

fn crow(e: &Enemy) -> bool {
    e.ty == EnemyType::Crow
}
fn bomb(e: &Enemy) -> bool {
    matches!(e.ty, EnemyType::BombBat | EnemyType::SuperBombBat)
}

pub fn init(e: &mut Enemy) {
    e.t[COOLDOWN] = 30.0;
    e.sine = SineWave::frames(60.0);
}

pub fn enter(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy, s: u8) {
    match s {
        IDLE => e.sine = SineWave::frames(60.0),
        SWOOP => {
            e.t[COOLDOWN] = 60.0;
            e.n[PASS] = 0;
        }
        _ => {}
    }
}

pub fn leave(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy, s: u8) {
    if s == SWOOP {
        e.target_speed = Vec2::ZERO;
    }
}

pub fn update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    match e.state {
        IDLE => {
            let t = Vec2::new(e.facing as f32 * 0.5, e.sine.value * 0.3);
            approach_speed(e, t, 0.2 * tm);
            if solid_at(w, c, id, Vec2::new(e.facing as f32, 0.0)) {
                e.facing = -e.facing;
            }
            e.t[COOLDOWN] -= tm;
            IDLE
        }
        SWOOP => {
            approach_speed(e, e.target_speed, 0.5 * tm);
            set_facing(e, e.speed.x);
            SWOOP
        }
        _ => {
            e.speed.y = min(e.speed.y + 0.225 * tm, 2.7);
            let f = if on_ground(w, c, id) { 0.2 } else { 0.1 };
            e.speed.x = approach(e.speed.x, 0.0, f * tm);
            DEAD
        }
    }
}

pub fn co(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, step: u16) {
    match (e.state, step) {
        (IDLE, 0) => e.co.yield_frames(10.0, 1),
        (IDLE, _) => {
            if e.t[COOLDOWN] <= 0.0
                && let Some((pid, pp, _)) = closest_player(w, c, id, e, 14400.0, true)
            {
                let v = shortest(w, c, w.core(id).pos, pp);
                let detect = if crow(e) { 100.0 } else { 80.0 };
                if v.x.abs() <= detect && v.y >= -10.0 {
                    e.target = Some(pid);
                    set_state(w, c, id, e, SWOOP);
                    e.co.finish();
                    return;
                }
            }
            e.co.yield_frames(5.0, 1);
        }
        (SWOOP, 0) => {
            let me = w.core(id).pos;
            let tp = e
                .target
                .and_then(|t| w.try_core(t))
                .map(|t| t.pos)
                .unwrap_or(me);
            let v = shortest_open(w, c, me, tp).1;
            set_facing(e, v.x);
            let windup = -v.safe_normalize();
            e.target_pos = windup;
            e.target_speed = windup * 0.3;
            if crow(e) && e.n[PASS] == 0 {
                w.sound("en_crowPreDive", me.x);
            }
            e.co.yield_frames(10.0, 1);
        }
        (SWOOP, 1) => {
            let me = w.core(id).pos;
            w.sound(swoop_sound(e), me.x);
            let (off, bw) = if crow(e) {
                (Vec2::new(6.0, 5.0), 12.0)
            } else {
                (Vec2::new(5.0, 5.0), 10.0)
            };
            let hit = if bomb(e) { OnHit::Explode } else { OnHit::None };
            attack::spawn(w, id, e, off, bw, 10.0, 18.0, false, Vec2::ZERO, hit);
            let tp = e
                .target
                .and_then(|t| w.try_core(t))
                .map(|t| t.pos)
                .unwrap_or(me);
            let v = shortest_open(w, c, me, tp).1;
            e.target_speed = if v.length() < 80.0 {
                v.safe_normalize() * 2.2
            } else {
                -e.target_pos * 2.2
            };
            e.co.yield_frames(20.0, 2);
        }
        (SWOOP, _) => {
            e.n[PASS] += 1;
            let passes = if crow(e) { 2 } else { 1 };
            if e.n[PASS] < passes {
                let ok = e.target.is_some_and(|t| {
                    sees(w, c, id, e, t) && shortest(w, c, w.core(id).pos, w.core(t).pos).y >= -20.0
                });
                if ok {
                    // Straight back into the wind-up, this tick.
                    co(w, c, id, e, 0);
                    return;
                }
            }
            set_state(w, c, id, e, IDLE);
            e.co.finish();
        }
        (_, 0) => {
            if bomb(e) {
                e.co.yield_frames(10.0, 1);
            } else {
                e.co.yield_frames(60.0, 2);
            }
        }
        (_, 1) => {
            explode(w, c, id, e);
            e.co.finish();
        }
        (_, 2) => {
            if lodged_count(e) > 0 {
                e.co.yield_frames(10.0, 2);
            } else {
                e.co.yield_frames(120.0, 3);
            }
        }
        _ => {
            remove_now(w, id, e);
            e.co.finish();
        }
    }
}

pub fn collide_h(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy) {
    match e.state {
        SWOOP => {
            e.speed = Vec2::ZERO;
            e.target_speed = Vec2::ZERO;
        }
        DEAD => e.speed.x *= -0.5,
        _ => e.speed.x = 0.0,
    }
}

pub fn collide_v(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy) {
    match e.state {
        SWOOP => {
            e.speed = Vec2::ZERO;
            e.target_speed = Vec2::ZERO;
        }
        DEAD => {
            e.speed.y = if e.speed.y >= 0.5 { -0.5 } else { 0.0 };
        }
        _ => e.speed.y = 0.0,
    }
}

pub fn touch(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    pid: EntityId,
    p: &mut Player,
) {
    if bomb(e) {
        explode(w, c, id, e);
    } else {
        plain_touch(w, c, id, e, pid, p);
    }
}

/// Any arrow kills a bat outright and lodges in it.
pub fn arrow_hit(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    if !e.alive() {
        return false;
    }
    e.speed = a.speed;
    lodge(w, id, e, aid, a);
    super::die(w, c, id, e, a.player_index);
    true
}

pub fn die(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    untag(w, id, e);
    w.core_mut(id).collider = Collider::Box(Hitbox::wrapping(6.0, 6.0, -3.0, -3.0));
    set_state(w, c, id, e, DEAD);
}

/// `Explode`: explosion at the bat, credited to its killer (-1 while alive;
/// the original credits slot 0 here, enemies.md §6), stuck arrows released.
pub fn explode(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    if w.core(id).marked_for_removal {
        return;
    }
    let owner = if e.alive() { -1 } else { e.killer };
    let at = w.core(id).pos;
    blast::explode(w, c, at, owner, e.ty == EnemyType::SuperBombBat);
    remove_now(w, id, e);
}

/// The swoop cue per bat type (the crow alternates dive sounds per pass).
fn swoop_sound(e: &Enemy) -> &'static str {
    match e.ty {
        EnemyType::BombBat => "en_bombBatSwoop",
        EnemyType::SuperBombBat => "en_superBombBatSwoop",
        EnemyType::Crow => {
            if e.n[PASS] == 0 {
                "en_crowDive1"
            } else {
                "en_crowDive2"
            }
        }
        _ => "en_eyeBatSwoop",
    }
}
