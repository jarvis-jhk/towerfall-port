//! Exploder (enemies.md §4.10): a ghost-like chaser that arms and explodes.

use super::*;
use crate::components::Ease;
use crate::math::{clamp, lerp};

pub const PATROL: u8 = 0;
pub const IDLE: u8 = 1;
pub const CHASE: u8 = 2;
pub const DEAD: u8 = 3;
pub const ARMED: u8 = 4;
const CAN_ARM: usize = 0;
/// Ticks to the next chase sound (`chaseSoundCounter`).
const CHASE_SOUND: usize = 1;
/// Ticks into the arm/death swell tween.
const SWELL: usize = 2;
const MISSES: usize = 0;
const NODE: usize = 1;
/// Player slot credited for the explosion.
const ARMER: usize = 2;

pub fn init(e: &mut Enemy) {
    e.t[CAN_ARM] = 60.0;
    e.sine = SineWave::frames(120.0);
    // The rocking sine (doubled while chasing).
    e.sine2 = SineWave::frames(120.0);
    e.n[ARMER] = -1;
}

/// After the base update: the arming grace counts down, facing follows
/// `vx` while alive, the armed/dead body rocks and swells (enemies.md §4.10).
pub fn post_update(w: &World, e: &mut Enemy) {
    let tm = w.time.time_mult;
    if e.t[CAN_ARM] > 0.0 {
        e.t[CAN_ARM] -= tm;
    }
    if e.state < DEAD {
        set_facing(e, e.speed.x);
        e.rotation = 0.0;
    } else {
        e.rotation = e.sine2.value * 15.0 * PI / 180.0;
        e.t[SWELL] += tm;
        let k = (e.t[SWELL] / 30.0).min(1.0);
        let s = lerp(1.0, 1.4, Ease::CubeIn.apply(k));
        e.scale = Vec2::new(s, s);
    }
}

pub fn enter(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    match s {
        CHASE => {
            e.target_speed = Vec2::ZERO;
            e.n[MISSES] = 0;
            e.t[CHASE_SOUND] = 0.0;
            e.sine2.rate *= 2.0;
        }
        ARMED | DEAD => {
            w.sound("en_blowfishDeath1", w.core(id).pos.x);
            e.t[SWELL] = 0.0;
        }
        _ => {}
    }
}

pub fn leave(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy, s: u8) {
    if s == CHASE {
        e.sine2.rate /= 2.0;
    }
}

pub fn update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    match e.state {
        PATROL => {
            approach_speed(e, e.target_speed, 0.1 * tm);
            if let Some((pid, _, _)) = closest_player(w, c, id, e, 10000.0, true) {
                e.target = Some(pid);
                return CHASE;
            }
            PATROL
        }
        IDLE => {
            let t = Vec2::new(e.sine.value * 0.3, e.sine.value_over_two * 0.3);
            approach_speed(e, t, 0.04 * tm);
            if let Some((pid, _, _)) = closest_player(w, c, id, e, 14400.0, true) {
                e.target = Some(pid);
                return CHASE;
            }
            IDLE
        }
        CHASE => {
            // A chase sound every 20 ticks, a particle every 3 (§4.10).
            if e.t[CHASE_SOUND] > 0.0 {
                e.t[CHASE_SOUND] -= tm;
            } else {
                w.sound("en_blowfishMove", w.core(id).pos.x);
                e.t[CHASE_SOUND] = 20.0;
            }
            if w.time.on_interval(3) {
                let at = w.core(id).pos;
                crate::entities::particles(w, "EnExploderChase", at, 1, Vec2::new(4.0, 4.0), 0.0);
            }
            approach_speed(e, e.target_speed, 0.1 * tm);
            CHASE
        }
        ARMED => {
            approach_speed(e, Vec2::ZERO, 0.02 * tm);
            ARMED
        }
        _ => {
            approach_speed(e, Vec2::ZERO, 0.15 * tm);
            DEAD
        }
    }
}

pub fn co(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, step: u16) {
    match (e.state, step) {
        (PATROL, 0) => e.co.yield_frames(4.0, 1),
        (PATROL, 1) => {
            // Head for the next node (aimed once per node).
            if e.n[NODE] as usize >= e.node_count as usize {
                set_state(w, c, id, e, IDLE);
                e.co.finish();
                return;
            }
            let node = e.nodes[e.n[NODE] as usize];
            e.target_speed = (node - w.core(id).pos).safe_normalize_len(0.5);
            co(w, c, id, e, 2);
        }
        (PATROL, _) => {
            let node = e.nodes[e.n[NODE] as usize];
            if (node - w.core(id).pos).length_sq() >= 100.0 {
                e.co.yield_frames(5.0, 2);
            } else {
                e.n[NODE] += 1;
                co(w, c, id, e, 1);
            }
        }
        (CHASE, 0) => e.co.yield_frames(15.0, 1),
        (CHASE, _) => {
            if !e.target.is_some_and(|t| sees(w, c, id, e, t)) {
                e.n[MISSES] += 1;
                if e.n[MISSES] >= 4 {
                    set_state(w, c, id, e, IDLE);
                    e.co.finish();
                    return;
                }
            } else {
                e.n[MISSES] = 0;
                let t = e.target.unwrap();
                let v = shortest(w, c, w.core(id).pos, w.core(t).pos);
                let d2 = v.length_sq();
                let k = clamp((d2 - 900.0) / (6400.0 - 900.0), 0.0, 1.0);
                e.target_speed = v.safe_normalize_len(lerp(0.8, 1.2, k));
                // Arming only happens on these 15-tick checks.
                if e.t[CAN_ARM] <= 0.0 && d2 <= 900.0 {
                    e.n[ARMER] = w.player(t).map(|p| p.index as i32).unwrap_or(-1);
                    set_state(w, c, id, e, ARMED);
                    e.co.finish();
                    return;
                }
            }
            e.co.yield_frames(15.0, 1);
        }
        (ARMED, 0) | (DEAD, 0) => e.co.yield_frames(30.0, 1),
        (ARMED, _) | (DEAD, _) => {
            let at = w.core(id).pos;
            let owner = e.n[ARMER] as i8;
            e.co.finish();
            w.sound("en_blowfishDeath2", at.x);
            remove_now(w, id, e);
            blast::explode(w, c, at, owner, false);
        }
        _ => e.co.finish(),
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
    plain_touch(w, c, id, e, pid, p);
    e.speed *= 0.6;
    if e.t[CAN_ARM] <= 0.0 && e.state != DEAD {
        e.target = Some(pid);
        e.n[ARMER] = p.index as i32;
        set_state(w, c, id, e, ARMED);
    }
}

/// Killed: still an Enemy until its 30-tick fuse ends (only the Target tag goes).
pub fn die(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    e.n[ARMER] = e.killer as i32;
    w.tag(id, Tag::Target, false);
    set_state(w, c, id, e, DEAD);
}
