//! Mole (enemies.md §4.7): jumps down and drills through solids, rolls.

use super::*;
use crate::math::{angle_approach, angle_to_vector, approach, min};

pub const IDLE: u8 = 0;
pub const JUMP_DOWN: u8 = 1;
pub const DRILL: u8 = 2;
pub const ROLL: u8 = 3;
pub const DEAD: u8 = 4;
const CD: usize = 0;
const ANGLE: usize = 1;
const DSPEED: usize = 2;
const DEAD_T: usize = 3;
const F_FROM_JUMP: u16 = F_TYPE;
/// `hasAttacked`: the cooldown star only shows after the first attack.
const F_ATTACKED: u16 = F_TYPE << 1;
/// `onGround`, sampled before the components each tick (not while drilling).
const F_ON_GROUND: u16 = F_TYPE << 2;

pub fn init(e: &mut Enemy) {
    e.flags |= F_BOUNCES_ON_PLAYER;
    e.t[CD] = 60.0;
}

pub fn drilling(e: &Enemy) -> bool {
    e.state == DRILL
}
pub fn rolling(e: &Enemy) -> bool {
    e.state == ROLL
}
/// `onGround` as last sampled (idle vs jump sprite).
pub fn on_ground(e: &Enemy) -> bool {
    e.has(F_ON_GROUND)
}

/// `Mole.Update` before the base update: ground sample, facing follows the
/// horizontal speed, sprite scale relaxes (all but the scale skipped while
/// drilling).
pub fn pre_update(w: &World, c: &Content, id: EntityId, e: &mut Enemy) {
    let tm = w.time.time_mult;
    if e.state != DRILL {
        let g = solid_at(w, c, id, Vec2::new(0.0, 1.0));
        e.set(F_ON_GROUND, g);
        set_facing(e, e.speed.x);
    }
    e.scale.x = approach(e.scale.x.abs(), 1.0, 0.02 * tm);
    e.scale.y = approach(e.scale.y, 1.0, 0.02 * tm);
}

pub fn enter(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    let x = w.core(id).pos.x;
    match s {
        IDLE => e.set(F_ON_GROUND, false),
        DRILL => {
            e.set(F_ATTACKED, true);
            e.speed = Vec2::ZERO;
            e.t[ANGLE] = if e.has(F_FROM_JUMP) {
                PI / 2.0
            } else {
                -PI / 2.0
            };
            e.t[DSPEED] = 0.5;
            e.t[CD] = 60.0;
            if let Some(t) = e.target
                && w.exists(t)
                && shortest(w, c, w.core(id).pos, w.core(t).pos).y < 10.0
            {
                e.target = None;
            }
        }
        ROLL => {
            w.sound("en_moleRoll", x);
            e.set(F_ATTACKED, true);
            e.t[CD] = 60.0;
        }
        DEAD => e.t[DEAD_T] = 0.0,
        _ => {}
    }
}

pub fn leave(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    if s == ROLL {
        w.sound("en_moleRollEnd", w.core(id).pos.x);
    }
    if s == DRILL {
        e.set(F_FROM_JUMP, false);
    }
}

pub fn update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    match e.state {
        IDLE => {
            e.rotation = angle_approach(e.rotation, 0.0, PI / 12.0 * tm);
            let g = e.has(F_ON_GROUND);
            if !g {
                e.speed.y = approach(e.speed.y, 1.8, 0.15 * tm);
                // Faces last tick's target (it is re-picked below).
                if let Some(t) = e.target.filter(|&t| w.exists(t)) {
                    set_facing(e, shortest(w, c, w.core(id).pos, w.core(t).pos).x);
                }
            }
            e.speed.x = approach(e.speed.x, e.facing as f32 * 0.4, 0.1 * tm);
            e.target = closest_player(w, c, id, e, 14400.0, false).map(|t| t.0);
            if e.t[CD] > 0.0 {
                e.t[CD] -= tm;
            } else if g && let Some(t) = e.target {
                let me = w.core(id).pos;
                let v = shortest(w, c, me, w.core(t).pos);
                let ground_ahead = solid_at(w, c, id, Vec2::new(5.0 * e.facing as f32, 1.0));
                if v.y > 20.0 && v.x.abs() < 35.0 && ground_ahead {
                    set_facing(e, v.x);
                    return JUMP_DOWN;
                }
                if v.y > -20.0
                    && sees(w, c, id, e, t)
                    && vision_cone(
                        w,
                        c,
                        me + e.sight,
                        facing_angle(e.facing),
                        2.0 * PI / 9.0,
                        w.core(t).pos,
                    )
                {
                    set_facing(e, v.x);
                    e.speed.y = -2.0;
                    return ROLL;
                }
            }
            IDLE
        }
        JUMP_DOWN => {
            e.speed.x = approach(e.speed.x, 0.0, 0.1 * tm);
            if !e.has(F_ON_GROUND) {
                let k = if e.speed.y.abs() < 0.5 { 0.5 } else { 1.0 };
                e.speed.y = min(e.speed.y + 0.15 * k * tm, 3.6);
            }
            JUMP_DOWN
        }
        DRILL => {
            if e.target.is_some_and(|t| w.player(t).is_none_or(|p| p.dead)) {
                e.target = None;
            }
            e.t[DSPEED] = min(e.t[DSPEED] + 0.05 * tm, 2.2);
            if let Some(t) = e.target {
                let a = shortest(w, c, w.core(id).pos, w.core(t).pos).angle();
                e.t[ANGLE] = angle_approach(e.t[ANGLE], a, PI / 300.0 * tm);
            }
            e.rotation = e.t[ANGLE] + PI / 2.0;
            // Through the ground; `Speed` stays zero until it breaks out.
            let step = angle_to_vector(e.t[ANGLE], e.t[DSPEED]) * tm;
            crate::actor::move_ignore_solids(w, id, step);
            if !w.check_at(c, id, w.core(id).pos, Tag::Solid) {
                w.sound("en_moleDigOut", w.core(id).pos.x);
                e.speed = angle_to_vector(e.t[ANGLE], 3.0);
                return IDLE;
            }
            DRILL
        }
        ROLL => {
            e.rotation += PI / 9.0 * tm;
            e.speed.x = approach(e.speed.x, 2.0 * e.facing as f32, 0.1 * tm);
            if !e.has(F_ON_GROUND) {
                e.speed.y = approach(e.speed.y, 2.7, 0.3 * tm);
            }
            ROLL
        }
        _ => {
            e.speed = approach_vec(e.speed, Vec2::ZERO, 0.1 * tm);
            e.t[DEAD_T] += tm;
            // Removed when `MoleExplosion` (5 frames of 0.08 s) completes.
            if e.t[DEAD_T] >= anim_ticks(0.08, 5) {
                remove_now(w, id, e);
            }
            DEAD
        }
    }
}

pub fn co(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, step: u16) {
    match (e.state, step) {
        (IDLE, 0) => e.co.yield_frames(20.0, 1),
        (IDLE, _) => {
            if e.target.is_none() && w.rng.chance(0.3) {
                e.facing = -e.facing;
            }
            e.co.yield_frames(20.0, 1);
        }
        (JUMP_DOWN, 0) => {
            w.sound("en_molePreDig", w.core(id).pos.x);
            e.scale = Vec2::new(0.8, 1.2);
            e.speed.y = -2.2;
            e.co.yield_frames(10.0, 1);
        }
        (JUMP_DOWN, 1) => e.co.yield_frames(6.0, 2),
        (ROLL, 0) => e.co.yield_frames(60.0, 1),
        (ROLL, _) => {
            set_state(w, c, id, e, IDLE);
            e.co.finish();
        }
        _ => e.co.finish(),
    }
}

pub fn collide_h(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy) {
    match e.state {
        IDLE => {
            e.facing = -e.facing;
            e.speed.x = 0.0;
        }
        JUMP_DOWN => e.speed.x = 0.0,
        ROLL => {
            w.sound("en_moleRollImpact", w.core(id).pos.x);
            e.facing = -e.facing;
            e.speed.x = -e.speed.x;
        }
        // Drill (its break-out tick) and Dead keep their speed.
        _ => {}
    }
}

pub fn collide_v(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    match e.state {
        IDLE => {
            e.speed.y = 0.0;
            w.sound("en_moleLand", w.core(id).pos.x);
        }
        JUMP_DOWN => {
            if e.speed.y > 0.0 {
                w.core_mut(id).pos.y += 1.0;
                w.sound("en_moleDigIn", w.core(id).pos.x);
                e.set(F_FROM_JUMP, true);
                set_state(w, c, id, e, DRILL);
            }
        }
        ROLL => {
            w.sound("en_moleRollImpact", w.core(id).pos.x);
            e.speed.y = if e.speed.y > 1.8 {
                e.speed.y * -0.6
            } else {
                0.0
            };
        }
        _ => {}
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
    if e.state == ROLL {
        hurt_player(w, c, id, pid, p);
    } else {
        plain_touch(w, c, id, e, pid, p);
    }
}

pub fn die(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    untag(w, id, e);
    e.speed *= 0.6;
    set_state(w, c, id, e, DEAD);
}

/// `OnArrowHit`: a rolling mole shrugs the arrow off (it falls with its
/// sound); otherwise the base hit (none while flashing) and the arrow falls
/// silently either way.
pub fn arrow_hit(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    if e.state == ROLL {
        arrow::enter_fall_mode(w, aid, a, true, false, true);
        return true;
    }
    if !w.core(id).flash.flashing {
        hurt(w, c, id, e, a.speed, 1, a.player_index);
    }
    arrow::enter_fall_mode(w, aid, a, true, false, false);
    true
}

/// Cooldown star (enemies.md §1.10): attack cooldown running while idle.
pub fn cooldown_star(e: &Enemy) -> Option<Vec2> {
    (e.alive() && e.state == IDLE && e.t[CD] > 0.0 && e.has(F_ATTACKED))
        .then_some(Vec2::new(0.0, -10.0))
}
