//! Birdman (normal / dark) and its tornado attack (enemies.md §4.5).

use super::*;
use crate::math::{approach, min};

pub const IDLE: u8 = 0;
pub const ATTACK: u8 = 1;
const FLAP_CD: usize = 0;
const ATTACK_CD: usize = 1;
const FLAP_T: usize = 2;
const F_ARROW_ATTACK: u16 = F_TYPE;
const F_TARGET_ARROW: u16 = F_TYPE << 1;

pub fn init(e: &mut Enemy) {
    e.t[ATTACK_CD] = 60.0;
    e.flags |= F_ARROW_ATTACK;
}

pub fn enter(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    if s == ATTACK {
        let me = w.core(id).pos;
        let tp = target_point(w, e).unwrap_or(me);
        set_facing(e, shortest(w, c, me, tp).x);
        e.set(F_ARROW_ATTACK, false);
        e.t[ATTACK_CD] = 120.0;
    }
}

fn target_point(w: &World, e: &Enemy) -> Option<Vec2> {
    e.target.and_then(|t| w.try_core(t)).map(|t| t.pos)
}

pub fn update(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    match e.state {
        IDLE => {
            e.speed.x = approach(e.speed.x, e.target_speed.x, 0.1 * tm);
            e.speed.y = min(e.speed.y + 0.06 * tm, 0.72);
            if e.t[FLAP_CD] > 0.0 {
                e.t[FLAP_CD] -= tm;
            } else if e.target_speed.y < 0.0 {
                e.t[FLAP_CD] = 30.0;
                e.t[FLAP_T] = 18.0;
            }
            if e.t[FLAP_T] > 0.0 {
                e.t[FLAP_T] -= tm;
                if e.t[FLAP_T] <= 0.0 {
                    e.speed.y = -1.5;
                    e.scale = Vec2::new(1.2, 0.8);
                    w.sound("en_birdmanWingflap", w.core(id).pos.x);
                }
            }
            e.t[ATTACK_CD] -= tm;
            IDLE
        }
        _ => {
            approach_speed(e, e.target_speed, 0.1 * tm);
            ATTACK
        }
    }
}

pub fn co(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, step: u16) {
    match (e.state, step) {
        (IDLE, 0) => {
            if w.rng.chance(0.2) {
                e.facing = -e.facing;
            }
            let vy = if w.rng.next_max(2) == 0 { -1.0 } else { 1.0 };
            e.target_speed = Vec2::new(e.facing as f32 * 0.5, vy);
            e.co.yield_frames(40.0, 1);
        }
        (IDLE, _) => {
            if e.t[ATTACK_CD] <= 0.0
                && let Some((pid, pp, _)) = closest_player(w, c, id, e, 10000.0, true)
            {
                let me = w.core(id).pos;
                let dir = shortest(w, c, me, pp).safe_normalize();
                if !w.line_check(c, me, me + dir * 20.0, Tag::Solid) {
                    e.target = Some(pid);
                    e.set(F_TARGET_ARROW, false);
                    set_state(w, c, id, e, ATTACK);
                    e.co.finish();
                    return;
                }
            }
            co(w, c, id, e, 0);
        }
        (ATTACK, 0) => {
            e.target_speed = Vec2::ZERO;
            if e.has(F_TARGET_ARROW) {
                e.co.yield_frames(4.0, 2);
            } else {
                e.co.yield_frames(6.0, 1);
            }
        }
        (ATTACK, 1) => e.co.yield_frames(4.0, 2),
        (ATTACK, 2) => {
            let me = w.core(id).pos;
            w.sound("en_birdmanTornado", me.x);
            let tp = target_point(w, e).unwrap_or(me + Vec2::new(e.facing as f32, 0.0));
            let dir = shortest(w, c, me, tp).safe_normalize();
            let player_target = if e.has(F_TARGET_ARROW) {
                None
            } else {
                e.target
            };
            hazard::spawn_tornado(w, me, dir, player_target);
            e.speed = -dir * 3.0;
            e.co.yield_frames(60.0, 3);
        }
        (ATTACK, _) => {
            set_state(w, c, id, e, IDLE);
            e.co.finish();
        }
        _ => e.co.finish(),
    }
}

pub fn collide_h(e: &mut Enemy) {
    if sign(e.speed.x) == sign(e.target_speed.x) && e.target_speed.x != 0.0 {
        e.target_speed.x = -e.target_speed.x;
        e.facing = -e.facing;
    }
    e.speed.x = 0.0;
}

pub fn collide_v(e: &mut Enemy) {
    if sign(e.speed.y) == sign(e.target_speed.y) && e.target_speed.y != 0.0 {
        e.target_speed.y = -e.target_speed.y;
    }
    e.speed.y = 0.0;
}

pub fn arrow_seeking(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    aid: EntityId,
    _a: &Arrow,
) {
    if e.state != IDLE || !e.alive() || !(e.has(F_ARROW_ATTACK) || e.t[ATTACK_CD] <= 0.0) {
        return;
    }
    let d2 = shortest(w, c, w.core(id).pos, w.core(aid).pos).length_sq();
    if d2 > 900.0 && d2 < 3600.0 {
        e.target = Some(aid);
        e.set(F_TARGET_ARROW, true);
        set_state(w, c, id, e, ATTACK);
    }
}

/// Cooldown star (enemies.md §1.10): attack cooldown running after the
/// free first attack was used, not attacking.
pub fn cooldown_star(e: &Enemy) -> Option<Vec2> {
    (e.alive() && e.state == IDLE && e.t[ATTACK_CD] > 0.0 && !e.has(F_ARROW_ATTACK))
        .then_some(Vec2::new(0.0, -14.0))
}
