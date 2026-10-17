//! Cultist (purple), scythe cultist (green), boss cultist (red); enemies.md §4.4.

use super::attack::{self, OnHit};
use super::*;
use crate::math::approach;

pub const IDLE: u8 = 0;
pub const ATTACK: u8 = 1;
pub const ATTACK_DOWN: u8 = 2;
pub const JUMP: u8 = 3;
const CD: usize = 0;
const TX: usize = 1;
const SAVED_VX: usize = 2;
const F_TURN: u16 = F_TYPE;

fn scythe(e: &Enemy) -> bool {
    e.ty == EnemyType::ScytheCultist
}
fn params(e: &Enemy) -> (f32, f32, f32) {
    if scythe(e) {
        (3.0, 30.0, 2116.0)
    } else {
        (2.0, 60.0, 1600.0)
    }
}

pub fn init(e: &mut Enemy) {
    e.flags |= F_BOUNCES_ON_PLAYER;
    e.t[CD] = 30.0;
}

pub fn enter(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy, s: u8) {
    match s {
        ATTACK => e.t[CD] = params(e).1,
        ATTACK_DOWN => {
            e.t[CD] = params(e).1;
            e.t[TX] = 0.0;
        }
        _ => {}
    }
}

pub fn update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    let grounded = on_ground(w, c, id);
    e.speed.x = approach(e.speed.x, e.t[TX], 0.1 * tm);
    if !grounded {
        e.speed.y = approach(e.speed.y, 1.8, 0.15 * tm);
    }
    match e.state {
        IDLE => {
            e.t[TX] = e.facing as f32 * if grounded { 0.5 } else { 0.2 };
            if e.t[CD] > 0.0 {
                e.t[CD] -= tm;
                return IDLE;
            }
            let (lunge, _, detect) = params(e);
            if let Some((_, pp, _)) = closest_player(w, c, id, e, detect, true) {
                let v = shortest(w, c, w.core(id).pos, pp);
                if v.y >= -8.0 {
                    if scythe(e) && v.y > 8.0 && v.x.abs() < 15.0 {
                        return ATTACK_DOWN;
                    }
                    let before = e.facing;
                    set_facing(e, v.x);
                    e.set(F_TURN, e.facing != before);
                    e.speed.x = lunge * e.facing as f32;
                    return ATTACK;
                }
            }
            IDLE
        }
        JUMP => {
            if !grounded {
                IDLE
            } else {
                JUMP
            }
        }
        s => s,
    }
}

pub fn co(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, step: u16) {
    let x = w.core(id).pos.x;
    match (e.state, step) {
        (JUMP, 0) => {
            e.t[TX] = -0.5 * e.facing as f32;
            e.co.yield_frames(20.0, 1);
        }
        (JUMP, _) => {
            e.speed = Vec2::new(1.6 * e.facing as f32, -2.0);
            set_state(w, c, id, e, IDLE);
            e.co.finish();
        }
        (ATTACK, 0) => {
            e.t[TX] = 0.0;
            if e.has(F_TURN) {
                e.t[SAVED_VX] = e.speed.x;
                e.speed.x = 0.0;
                e.co.yield_frames(10.0, 1);
            } else {
                co(w, c, id, e, 1);
            }
        }
        (ATTACK, 1) => {
            if e.has(F_TURN) {
                e.speed.x = e.t[SAVED_VX];
                e.set(F_TURN, false);
            }
            e.speed.y = 0.0;
            w.sound(
                if scythe(e) {
                    "en_cultScyDash"
                } else {
                    "en_cultDash"
                },
                x,
            );
            if scythe(e) {
                e.co.yield_frames(8.0, 10);
            } else {
                attack::spawn(
                    w,
                    id,
                    e,
                    Vec2::new(10.0, -8.0),
                    12.0,
                    12.0,
                    15.0,
                    true,
                    Vec2::ZERO,
                    OnHit::None,
                );
                e.co.yield_frames(8.0, 2);
            }
        }
        (ATTACK, 2) => {
            w.sound("en_cultSlashA", x);
            e.co.yield_frames(27.0, 3);
        }
        (ATTACK, 3) => {
            if !on_ground(w, c, id) || e.ty == EnemyType::BossCultist {
                set_state(w, c, id, e, IDLE);
                e.co.finish();
            } else {
                e.co.yield_frames(5.0, 4);
            }
        }
        (ATTACK, 4) => {
            w.sound("en_cultSlashB", x);
            attack::spawn(
                w,
                id,
                e,
                Vec2::new(0.0, -10.0),
                24.0,
                20.0,
                10.0,
                true,
                Vec2::ZERO,
                OnHit::None,
            );
            e.co.yield_frames(25.0, 5);
        }
        (ATTACK, 10) => {
            attack::spawn(
                w,
                id,
                e,
                Vec2::new(10.0, -8.0),
                12.0,
                18.0,
                15.0,
                true,
                Vec2::ZERO,
                OnHit::None,
            );
            e.co.yield_frames(40.0, 5);
        }
        (ATTACK, _) => {
            set_state(w, c, id, e, IDLE);
            e.co.finish();
        }
        (ATTACK_DOWN, 0) => {
            attack::spawn(
                w,
                id,
                e,
                Vec2::new(0.0, 2.0),
                20.0,
                12.0,
                15.0,
                true,
                Vec2::new(0.0, 1.0),
                OnHit::None,
            );
            e.co.yield_frames(20.0, 1);
        }
        (ATTACK_DOWN, _) => {
            set_state(w, c, id, e, IDLE);
            e.co.finish();
        }
        _ => e.co.finish(),
    }
}

pub fn collide_h(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    if e.state == IDLE {
        let p = w.core(id).pos;
        let ahead = Vec2::new(p.x + 4.0 * e.facing as f32, p.y - 12.0);
        if on_ground(w, c, id) && !w.point_check(c, ahead, Tag::Solid) {
            set_state(w, c, id, e, JUMP);
        } else {
            e.facing = -e.facing;
            e.t[TX] = -e.t[TX];
        }
    }
    e.speed.x = 0.0;
}

pub fn arrow_seeking(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    aid: EntityId,
    _a: &Arrow,
) {
    if e.state != IDLE || e.t[CD] > 0.0 || !e.alive() {
        return;
    }
    let me = w.core(id).pos + e.sight;
    let ap = w.core(aid).pos;
    let d2 = shortest(w, c, me, ap).length_sq();
    if !(900.0..=2500.0).contains(&d2) {
        return;
    }
    if vision_cone(w, c, me, facing_angle(e.facing), 13.0 * PI / 36.0, ap) {
        e.speed.x = -2.0 * e.facing as f32;
        set_state(w, c, id, e, ATTACK);
    } else if scythe(e) && vision_cone(w, c, me, PI / 2.0, 0.8727, ap) {
        set_state(w, c, id, e, ATTACK_DOWN);
    }
}
