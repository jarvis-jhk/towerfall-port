//! Ghost family: blue ghost, green ghost, fire elementals (enemies.md §4.3).

use super::*;
use crate::math::{clamp, lerp};

pub const PATROL: u8 = 0;
pub const IDLE: u8 = 1;
pub const CHASE: u8 = 2;
pub const DODGE: u8 = 3;
pub const DIE: u8 = 4;
const DODGE_CD: usize = 0;
const DIE_T: usize = 1;
const MISSES: usize = 0;
const NODE: usize = 1;

fn fire(e: &Enemy) -> bool {
    matches!(e.ty, EnemyType::Elemental | EnemyType::GreenElemental)
}

pub fn init(e: &mut Enemy) {
    e.sine = SineWave::frames(90.0);
    if fire(e) {
        e.flags |= F_FLAMING;
    }
}

pub fn enter(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    match s {
        CHASE => {
            e.target_speed = Vec2::ZERO;
            e.n[MISSES] = 0;
        }
        DODGE => {
            e.t[DODGE_CD] = 60.0;
            w.sound(
                match e.ty {
                    EnemyType::GreenGhost => "en_greenGhostDodge",
                    EnemyType::Elemental | EnemyType::GreenElemental => "en_fireElementalDodge",
                    _ => "en_ghostDodge",
                },
                w.core(id).pos.x,
            );
        }
        DIE => e.t[DIE_T] = 0.0,
        _ => {}
    }
}

fn spot(w: &World, c: &Content, id: EntityId, e: &mut Enemy, d2: f32) -> bool {
    if let Some((pid, _, _)) = closest_player(w, c, id, e, d2, true) {
        e.target = Some(pid);
        true
    } else {
        false
    }
}

pub fn update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    if e.state != DODGE && e.t[DODGE_CD] > 0.0 {
        e.t[DODGE_CD] -= tm;
    }
    set_facing(e, e.speed.x);
    match e.state {
        PATROL => {
            approach_speed(e, e.target_speed, 0.1 * tm);
            if spot(w, c, id, e, 10000.0) {
                CHASE
            } else {
                PATROL
            }
        }
        IDLE => {
            let t = Vec2::new(e.sine.value * 0.3, e.sine.value_over_two * 0.3);
            approach_speed(e, t, 0.04 * tm);
            if spot(w, c, id, e, 14400.0) {
                CHASE
            } else {
                IDLE
            }
        }
        CHASE => {
            approach_speed(e, e.target_speed, 0.1 * tm);
            CHASE
        }
        DODGE => {
            approach_speed(e, Vec2::ZERO, 0.15 * tm);
            DODGE
        }
        _ => {
            e.t[DIE_T] += tm;
            let total = match e.ty {
                EnemyType::Ghost => anim_ticks(0.06, 12),
                EnemyType::GreenGhost => anim_ticks(0.06, 13),
                _ => anim_ticks(0.1, 14),
            };
            if e.t[DIE_T] >= total {
                remove_now(w, id, e);
            }
            DIE
        }
    }
}

pub fn co(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, step: u16) {
    match (e.state, step) {
        (PATROL, 0) => e.co.yield_frames(4.0, 1),
        (PATROL, _) => loop {
            if e.n[NODE] as usize >= e.node_count as usize {
                set_state(w, c, id, e, IDLE);
                e.co.finish();
                return;
            }
            let node = e.nodes[e.n[NODE] as usize];
            let v = shortest(w, c, w.core(id).pos, node);
            if step == 1 {
                let spd = if fire(e) { 0.8 } else { 0.6 };
                e.target_speed = v.safe_normalize() * spd;
            }
            if v.length_sq() >= 100.0 {
                e.co.yield_frames(5.0, 2);
                return;
            }
            e.n[NODE] += 1;
            if e.n[NODE] as usize >= e.node_count as usize {
                continue;
            }
            let node = e.nodes[e.n[NODE] as usize];
            let v = shortest(w, c, w.core(id).pos, node);
            let spd = if fire(e) { 0.8 } else { 0.6 };
            e.target_speed = v.safe_normalize() * spd;
            e.co.yield_frames(5.0, 2);
            return;
        },
        (CHASE, 0) => e.co.yield_frames(15.0, 1),
        (CHASE, _) => {
            let ok = e.target.is_some_and(|t| sees(w, c, id, e, t));
            if !ok {
                e.n[MISSES] += 1;
                if e.n[MISSES] >= 4 {
                    set_state(w, c, id, e, IDLE);
                    e.co.finish();
                    return;
                }
            } else {
                e.n[MISSES] = 0;
                let t = e.target.unwrap();
                let v = shortest(w, c, w.core(id).pos + e.sight, w.core(t).pos);
                let k = clamp((v.length_sq() - 900.0) / (6400.0 - 900.0), 0.0, 1.0);
                let l = if fire(e) {
                    lerp(1.0, 1.4, k)
                } else {
                    lerp(0.5, 1.0, k)
                };
                e.target_speed = v.safe_normalize() * l;
            }
            e.co.yield_frames(15.0, 1);
        }
        (DODGE, 0) => e.co.yield_frames(40.0, 1),
        (DODGE, _) => {
            set_state(w, c, id, e, IDLE);
            e.co.finish();
        }
        _ => e.co.finish(),
    }
}

/// An arrow homing on the ghost: dodge sideways (20..40 px away).
pub fn arrow_seeking(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    aid: EntityId,
    _a: &Arrow,
) {
    if !e.alive() || e.t[DODGE_CD] > 0.0 || e.state == DODGE {
        return;
    }
    let me = w.core(id).pos;
    let v = shortest(w, c, me, w.core(aid).pos);
    let d2 = v.length_sq();
    if d2 <= 400.0 || d2 >= 1600.0 {
        return;
    }
    e.alpha = 2.0;
    let mut dir = Vec2::new(-v.y, v.x).safe_normalize();
    if w.check_at(c, id, me + dir * 9.0, Tag::Solid) {
        dir = -dir;
    }
    e.speed = dir * 3.0;
    set_state(w, c, id, e, DODGE);
}

pub fn touch(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    pid: EntityId,
    p: &mut Player,
) {
    if (e.state == DODGE && !e.speed.is_zero()) || e.t[DODGE_CD] <= 0.0 {
        hurt_player(w, c, id, pid, p);
    }
    plain_touch(w, c, id, e, pid, p);
}

pub fn die(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    e.alpha = 2.0;
    untag(w, id, e);
    e.speed = e.speed.safe_normalize_len(0.4);
    set_state(w, c, id, e, DIE);
}

/// Cooldown star (enemies.md §1.10): dodge cooldown running, not dying.
pub fn cooldown_star(e: &Enemy) -> Option<Vec2> {
    (e.alive() && e.state != DIE && e.t[DODGE_CD] > 0.0).then_some(Vec2::new(0.0, -15.0))
}
