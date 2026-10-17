//! Flaming skull (enemies.md §4.8): bounces diagonally, burns players.

use super::*;

pub const NORMAL: u8 = 0;
pub const RECOIL: u8 = 1;
pub const DEAD: u8 = 2;
const DEAD_T: usize = 0;

fn diagonal(w: &mut World, e: &mut Enemy) {
    let vy = if w.rng.next_max(2) == 0 { -1.0 } else { 1.0 };
    e.speed = Vec2::new(e.facing as f32, vy).normalize();
}

/// Starts on a diagonal: `(facing, ±1)` with a random vertical sign.
pub fn init(w: &mut World, e: &mut Enemy) {
    let vy = if w.rng.next_max(2) == 0 { 1.0 } else { -1.0 };
    e.speed = Vec2::new(e.facing as f32, vy).normalize();
    e.sine = SineWave::frames(60.0);
}

pub fn enter(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy, s: u8) {
    if s == DEAD {
        e.t[DEAD_T] = 0.0;
    }
}

pub fn update(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    e.rotation = e.facing as f32 * 10.0 * PI / 180.0 * e.sine.value;
    match e.state {
        NORMAL => {
            e.speed = e.speed.safe_normalize();
            set_facing(e, e.speed.x);
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
            e.t[DEAD_T] += tm;
            if e.t[DEAD_T] >= 31.0 {
                remove_now(w, id, e);
            }
            DEAD
        }
    }
}

pub fn co(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy, _step: u16) {
    e.co.finish();
}

pub fn collide_h(w: &mut World, id: EntityId, e: &mut Enemy) {
    e.speed.x = -e.speed.x;
    e.scale = Vec2::new(0.8, 1.2);
    w.sound("en_flamingSkullCollide", w.core(id).pos.x);
}

pub fn collide_v(w: &mut World, id: EntityId, e: &mut Enemy) {
    e.speed.y = -e.speed.y;
    e.scale = Vec2::new(1.2, 0.8);
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
    set_facing(e, shortest(w, c, w.core(id).pos, w.core(pid).pos).x);
    hurt_player(w, c, id, pid, p);
    set_state(w, c, id, e, RECOIL);
}

pub fn die(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    untag(w, id, e);
    set_state(w, c, id, e, DEAD);
}
