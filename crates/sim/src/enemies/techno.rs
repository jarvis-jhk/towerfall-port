//! Techno mage and its homing missiles (enemies.md §4.12).

use super::*;
use crate::math::{approach, cos, sin};

pub const PATROL: u8 = 0;
pub const ALERT: u8 = 1;
pub const DEAD: u8 = 2;
const SHOT: usize = 0;

pub fn init(e: &mut Enemy) {
    e.flags |= F_EXPLOSIONS_PUSH;
    e.sine = SineWave::frames(120.0);
}

pub fn enter(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    if s == DEAD {
        w.sound("en_technoMageFreakout", w.core(id).pos.x);
    }
    match s {
        PATROL => e.target_speed.x = 0.8 * e.facing as f32,
        DEAD => e.sine = SineWave::frames(30.0),
        _ => {}
    }
}

pub fn leave(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy, s: u8) {
    if s == ALERT {
        e.target = None;
    }
}

pub fn update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    match e.state {
        PATROL => {
            e.target_speed.y = 0.1 * e.sine.value;
            let ahead = w.core(id).pos + Vec2::new(20.0 * e.facing as f32, 0.0);
            if w.point_check(
                c,
                wrap(ahead, w.world_size(c).w, w.world_size(c).h),
                Tag::Solid,
            ) {
                e.facing = -e.facing;
                e.target_speed.x = e.facing as f32;
            }
            if let Some((pid, _, _)) = closest_player(w, c, id, e, 10000.0, true) {
                e.target = Some(pid);
                return ALERT;
            }
            PATROL
        }
        ALERT => {
            e.target_speed.y = 0.1 * e.sine.value;
            if let Some(t) = e.target.filter(|&t| w.exists(t)) {
                let v = shortest(w, c, w.core(id).pos, w.core(t).pos);
                e.target_speed.x = if v.x.abs() < 40.0 {
                    -0.3 * sign(v.x)
                } else {
                    0.0
                };
            }
            ALERT
        }
        _ => {
            let a = 5.0 * PI / 180.0 * e.facing as f32 * e.sine.value * tm;
            let (s, co) = (sin(a), cos(a));
            let v = e.speed;
            let r = Vec2::new(v.x * co - v.y * s, v.x * s + v.y * co);
            let len = approach(r.length(), 5.0, 0.05 * tm);
            e.speed = r.safe_normalize_len(len);
            e.rotation -= 20.0 * PI / 180.0 * e.facing as f32 * tm;
            DEAD
        }
    }
}

pub fn post_update(w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy) {
    if e.state != DEAD {
        let tm = w.time.time_mult;
        approach_speed(e, e.target_speed, 0.2 * tm);
    }
}

pub fn co(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, step: u16) {
    match (e.state, step) {
        (ALERT, 0) => {
            if let Some(t) = e.target.filter(|&t| w.exists(t)) {
                set_facing(e, shortest(w, c, w.core(id).pos, w.core(t).pos).x);
            }
            e.n[SHOT] = 0;
            e.co.yield_frames(20.0, 1);
        }
        (ALERT, 1) => {
            let me = w.core(id).pos;
            let ok = e.target.is_some_and(|t| sees(w, c, id, e, t));
            let dir = e
                .target
                .filter(|&t| w.exists(t))
                .map(|t| shortest(w, c, me, w.core(t).pos).safe_normalize())
                .unwrap_or(Vec2::new(1.0, 0.0));
            if !ok || w.line_check(c, me, me + dir * 20.0, Tag::Solid) {
                set_state(w, c, id, e, PATROL);
                e.co.finish();
                return;
            }
            w.sound("en_technoMageFire", me.x);
            hazard::spawn_missile(w, me, dir);
            set_facing(e, dir.x);
            e.n[SHOT] += 1;
            if e.n[SHOT] < 2 {
                e.co.yield_frames(60.0, 1);
            } else {
                e.co.yield_frames(60.0, 2);
            }
        }
        (ALERT, _) => {
            e.n[SHOT] = 0;
            e.co.yield_frames(90.0, 1);
        }
        (DEAD, 0) => e.co.yield_frames(60.0, 1),
        (DEAD, _) => {
            explode(w, c, id, e);
            e.co.finish();
        }
        _ => e.co.finish(),
    }
}

pub fn collide(w: &mut World, id: EntityId, e: &mut Enemy, horizontal: bool) {
    if e.state == DEAD {
        w.sound("en_technoMageBounce", w.core(id).pos.x);
        if horizontal {
            e.speed.x = -e.speed.x;
        } else {
            e.speed.y = -e.speed.y;
        }
    } else if horizontal {
        e.speed.x = 0.0;
    } else {
        e.speed.y = 0.0;
    }
}

fn explode(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    let at = w.core(id).pos;
    let k = e.killer;
    w.sound("en_technoMageExplode", at.x);
    remove_now(w, id, e);
    blast::explode(w, c, at, k, false);
}

/// A dead mage explodes on touching a player.
pub fn dead_touch(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    explode(w, c, id, e);
}

/// Killed: only the Enemy tag goes; the body stays a hazard.
pub fn die(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    e.untagged = true;
    w.tag(id, Tag::Enemy, false);
    w.core_mut(id).collider = Collider::Box(Hitbox::wrapping(12.0, 12.0, -6.0, -6.0));
    w.core_mut(id).collidable = true;
    if e.speed.is_zero() {
        e.speed = Vec2::new(e.facing as f32, -1.0);
    }
    set_state(w, c, id, e, DEAD);
}
