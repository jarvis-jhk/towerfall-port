//! Worm (enemies.md §4.11): crawls along walls and ceilings around corners
//! and vomits a small bramble patch every few seconds.

use super::*;
use crate::math::{approach, min};

pub const FALL: u8 = 0;
pub const WALK: u8 = 1;
pub const DEAD: u8 = 2;
const MULT: usize = 0;
const DEAD_T: usize = 1;

/// Rotate by 90 degrees * `f` (y down: +1 = clockwise).
fn rot(v: Vec2, f: i8) -> Vec2 {
    if f >= 0 {
        Vec2::new(-v.y, v.x)
    } else {
        Vec2::new(v.y, -v.x)
    }
}

fn stick(e: &Enemy) -> Vec2 {
    e.target_pos
}

pub fn init(w: &mut World, e: &mut Enemy) {
    e.facing = if w.rng.next_max(2) == 0 { -1 } else { 1 };
    e.target_pos = Vec2::new(0.0, 1.0);
    e.target_speed = Vec2::new(e.facing as f32, 0.0);
    e.t[MULT] = 1.0;
}

pub fn enter(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy, s: u8) {
    match s {
        WALK => {
            e.target_pos = Vec2::new(0.0, 1.0);
            e.target_speed = Vec2::new(e.facing as f32, 0.0);
        }
        DEAD => e.t[DEAD_T] = 0.0,
        _ => {}
    }
}

pub fn update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    match e.state {
        FALL => {
            e.speed.y = approach(e.speed.y, 1.8, 0.15 * tm);
            e.speed.x = approach(e.speed.x, 0.0, 0.1 * tm);
            FALL
        }
        WALK => {
            let s = stick(e);
            if !solid_at(w, c, id, s) {
                let v = rot(s, e.facing);
                if !solid_at(w, c, id, s * 2.0 + v * 4.0) {
                    return FALL;
                }
                w.core_mut(id).pos += s;
                for _ in 0..12 {
                    if solid_at(w, c, id, v * 4.0) {
                        break;
                    }
                    w.core_mut(id).pos += s;
                }
                for _ in 0..12 {
                    if solid_at(w, c, id, v) {
                        break;
                    }
                    w.core_mut(id).pos += v;
                }
                e.target_pos = v;
                e.target_speed = rot(e.target_speed, e.facing);
            }
            let target = e.target_speed * (0.4 * e.t[MULT]);
            approach_speed(e, target, 0.1 * tm);
            e.rotation = crate::math::angle_approach(
                e.rotation,
                stick(e).angle() - PI / 2.0,
                PI / 18.0 * tm,
            );
            WALK
        }
        _ => {
            e.t[DEAD_T] += tm;
            if e.t[DEAD_T] >= anim_ticks(0.08, 5) {
                remove_now(w, id, e);
            }
            DEAD
        }
    }
}

pub fn co(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, step: u16) {
    if e.state != WALK {
        e.co.finish();
        return;
    }
    match step {
        0 => e.co.yield_frames(90.0, 1),
        1 => {
            // Already in brambles: try again in 30.
            let me = w.placed(id);
            if w.any(c, &me, Tag::Brambles, id) {
                e.co.yield_frames(30.0, 1);
                return;
            }
            e.t[MULT] = 0.0;
            e.co.yield_frames(15.0, 2);
        }
        2 => {
            w.sound("en_wormBrambly", w.core(id).pos.x);
            e.co.yield_frames(10.0, 3);
        }
        3 => {
            let at = w.core(id).pos + stick(e) * 6.0;
            crate::arrows::brambles::start_growth(w, at, -1, None, 1, false);
            e.co.yield_frames(50.0, 4);
        }
        _ => {
            e.t[MULT] = 1.0;
            e.co.yield_frames(240.0, 1);
        }
    }
}

/// `OnCollideH` / `OnCollideV`: only the hit axis stops; a falling worm
/// starts walking on any vertical hit; a walking one turns the inside
/// corner.
pub fn collide(e: &mut Enemy, horizontal: bool) {
    if horizontal {
        e.speed.x = 0.0;
    } else {
        e.speed.y = 0.0;
    }
    match e.state {
        FALL if !horizontal => e.pending_walk(),
        WALK => {
            e.target_pos = rot(e.target_pos, -e.facing);
            e.target_speed = rot(e.target_speed, -e.facing);
        }
        _ => {}
    }
}

impl Enemy {
    fn pending_walk(&mut self) {
        self.pending = Some(WALK);
    }
}

pub fn die(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    untag(w, id, e);
    e.speed = e.speed.safe_normalize_len(0.4);
    set_state(w, c, id, e, DEAD);
    let _ = min(0.0, 0.0);
}
