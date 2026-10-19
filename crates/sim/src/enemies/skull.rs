//! Flaming skull (enemies.md §4.8): bounces diagonally, burns players.

use super::*;
use crate::math::approach;

pub const NORMAL: u8 = 0;
pub const RECOIL: u8 = 1;
pub const DEAD: u8 = 2;
const DEAD_T: usize = 0;

/// A new diagonal `(facing, ±1)`, length 1 (one random sign draw).
fn diagonal(w: &mut World, e: &mut Enemy) {
    let vy = if w.rng.next_max(2) == 0 { 1.0 } else { -1.0 };
    e.speed = Vec2::new(e.facing as f32, vy).normalize();
}

/// Starts on a diagonal: `(facing, ±1)` with a random vertical sign.
pub fn init(w: &mut World, e: &mut Enemy) {
    diagonal(w, e);
    e.sine = SineWave::frames(60.0);
}

pub fn enter(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy, s: u8) {
    if s == DEAD {
        e.t[DEAD_T] = 0.0;
    }
}

pub fn update(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    match e.state {
        NORMAL => {
            e.rotation = e.facing as f32 * 10.0 * PI / 180.0 * e.sine.value;
            NORMAL
        }
        RECOIL => {
            approach_speed(e, Vec2::ZERO, 0.1 * tm);
            if e.speed.is_zero() {
                diagonal(w, e);
                return NORMAL;
            }
            RECOIL
        }
        _ => {
            approach_speed(e, Vec2::ZERO, 0.2 * tm);
            // The death wiggle (60 ticks, 4 Hz, 40 degrees) and the 30-tick
            // alarm, which fires on the 30th update after dying.
            e.t[DEAD_T] += tm;
            let k = e.t[DEAD_T];
            let v = crate::math::cos(k * 2.0 * PI * 4.0 / 60.0) * (1.0 - k / 60.0);
            e.rotation = v * 40.0 * PI / 180.0;
            if k >= 30.0 {
                remove_now(w, id, e);
            }
            DEAD
        }
    }
}

/// After the base update: the squash relaxes.
pub fn post_update(w: &World, e: &mut Enemy) {
    let tm = w.time.time_mult;
    e.scale.x = approach(e.scale.x, 1.0, 0.05 * tm);
    e.scale.y = approach(e.scale.y, 1.0, 0.05 * tm);
}

pub fn co(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy, _step: u16) {
    e.co.finish();
}

pub fn collide_h(w: &mut World, id: EntityId, e: &mut Enemy) {
    e.speed.x = -e.speed.x;
    set_facing(e, e.speed.x);
    e.scale.x = 0.5;
    w.sound("en_flamingSkullCollide", w.core(id).pos.x);
}

pub fn collide_v(w: &mut World, id: EntityId, e: &mut Enemy) {
    e.speed.y = -e.speed.y;
    e.scale.y = 0.5;
    w.sound("en_flamingSkullCollide", w.core(id).pos.x);
}

pub fn touch(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    pid: EntityId,
    p: &mut Player,
) {
    w.sound("en_flamingSkullCollide", w.core(id).pos.x);
    plain_touch(w, c, id, e, pid, p);
    crate::entities::start_fire(w, p);
    if w.core(id).pos.x != w.core(pid).pos.x {
        set_facing(e, shortest(w, c, w.core(id).pos, w.core(pid).pos).x);
    }
    hurt_player(w, c, id, pid, p);
    set_state(w, c, id, e, RECOIL);
}

pub fn die(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    untag(w, id, e);
    set_state(w, c, id, e, DEAD);
}
