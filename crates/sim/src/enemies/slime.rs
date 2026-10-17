//! Slime (green / red / blue), enemies.md §4.1.

use super::*;
use crate::math::{approach, min};

pub const WALK: u8 = 0;
pub const AIR: u8 = 1;
pub const JUMP: u8 = 2;
pub const LAND: u8 = 3;
pub const DIE: u8 = 4;
/// Timer of the Land / Die states.
const T: usize = 0;

fn params(ty: EnemyType) -> (f32, f32, f32, bool) {
    match ty {
        EnemyType::Slime => (0.4, 1.6, -3.5, false),
        _ => (0.6, 2.4, -3.0, true),
    }
}

pub fn init(e: &mut Enemy) {
    e.sine = SineWave::frames(40.0);
}

pub fn enter(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    match s {
        LAND => {
            w.sound("en_slimeLand", w.core(id).pos.x);
            e.speed = Vec2::ZERO;
            e.t[T] = 0.0;
        }
        DIE => e.t[T] = 0.0,
        _ => {}
    }
}

pub fn update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    let (walk, _, _, _) = params(e.ty);
    match e.state {
        WALK => {
            if !on_ground(w, c, id) {
                return AIR;
            }
            if solid_at(w, c, id, Vec2::new(2.0 * e.facing as f32, 0.0)) {
                e.facing = -e.facing;
            }
            e.speed.x = approach(e.speed.x, walk * e.facing as f32, 0.1 * tm);
            let pos = w.core(id).pos;
            if let Some(pid) = horizontal_player(w, c, pos + Vec2::new(0.0, -3.0), e.facing) {
                let pp = w.core(pid).pos;
                let v = shortest(w, c, pos, pp);
                if sign(v.x) as i8 == e.facing
                    && v.x.abs() >= 25.0
                    && v.x.abs() <= 68.0 + min(0.0, v.y)
                {
                    return JUMP;
                }
            }
            WALK
        }
        AIR => {
            if on_ground(w, c, id) && e.speed.y >= 0.0 {
                return WALK;
            }
            let mut tx = 0.0;
            if let Some((_, pp, _)) = closest_player(w, c, id, e, 6400.0, true) {
                let v = shortest(w, c, w.core(id).pos, pp);
                if v.x.abs() <= 60.0 {
                    tx = sign(v.x) * 0.6;
                }
            }
            e.speed.x = approach(e.speed.x, tx, 0.04 * tm);
            e.speed.y = approach(e.speed.y, 1.8, 0.15 * tm);
            AIR
        }
        LAND => {
            e.t[T] += tm;
            if e.t[T] >= anim_ticks(0.06, 12) {
                WALK
            } else {
                LAND
            }
        }
        DIE => {
            e.t[T] += tm;
            if e.t[T] >= anim_ticks(0.1, 5) {
                remove_now(w, id, e);
            }
            DIE
        }
        s => s,
    }
}

pub fn co(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, step: u16) {
    if e.state != JUMP {
        e.co.finish();
        return;
    }
    let (_, jh, jv, red) = params(e.ty);
    match step {
        0 => {
            e.speed.x = 0.0;
            if red {
                w.sound("en_redSlimePreJump", w.core(id).pos.x);
            }
            e.co.yield_frames(5.0, 1);
        }
        1 => {
            e.scale.y = approach(e.scale.y, 0.6, 0.1);
            if e.scale.y <= 0.61 {
                e.co.yield_frames(5.0, 2);
            } else {
                e.co.yield_next(1);
            }
        }
        _ => {
            w.sound("en_slimeJump", w.core(id).pos.x);
            e.scale = Vec2::new(0.5, 1.2);
            e.speed = Vec2::new(jh * e.facing as f32, jv);
            set_state(w, c, id, e, AIR);
            e.co.finish();
        }
    }
}

pub fn collide_v(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    if e.speed.y > 0.0 && e.state != DIE {
        if !actor::check_below_x(w, c, id, -2.0 * e.facing as f32) {
            actor::move_h(w, c, id, -2.0 * e.facing as f32);
            actor::move_v(w, c, id, 1.0);
        } else {
            e.scale.y = 0.6;
            let s = if e.speed.y >= 1.8 { LAND } else { WALK };
            set_state(w, c, id, e, s);
        }
    }
    e.speed.y = 0.0;
}

pub fn touch(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    pid: EntityId,
    p: &mut Player,
) {
    let me = w.core(id).pos;
    let pp = w.core(pid).pos;
    let v = shortest(w, c, me, pp);
    let toward = sign(v.x) as i8 == e.facing && e.speed.x.abs() > 0.05;
    if toward || pp.y >= me.y - 2.0 {
        hurt_player(w, c, id, pid, p);
        e.speed = (me - pp).safe_normalize_len(2.0);
    } else {
        plain_touch(w, c, id, e, pid, p);
    }
}

pub fn die(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    untag(w, id, e);
    e.speed = e.speed.safe_normalize_len(0.4);
    set_state(w, c, id, e, DIE);
}

pub fn post_update(w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy) {
    let tm = w.time.time_mult;
    e.scale.x = 1.0 + 0.2 * e.sine.value;
    e.scale.y = approach(e.scale.y, 1.0, 0.02 * tm);
}
