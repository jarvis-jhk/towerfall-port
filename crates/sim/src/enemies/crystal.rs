//! Evil crystals: red/blue (laser), green (bolt), pink (prism); enemies.md §4.9.

use super::*;
use crate::components::Ease;
use crate::math::approach;

pub const IDLE: u8 = 0;
pub const SHOOT: u8 = 1;
pub const DEAD: u8 = 2;
const CD: usize = 0;
const KILL_ANGLE: usize = 1;
/// The scale the sprite relaxes toward.
const TSCALE: usize = 2;
/// Ticks into the shoot spin tween (< 0: none).
const SPIN: usize = 3;
/// Ticks into the death wiggle.
const WIGGLE: usize = 4;
/// Spin direction (±1).
const SPIN_DIR: usize = 5;
const J: usize = 0;
const NODE: usize = 1;
/// `atNode`: where the current round of the node walk started.
const START_NODE: usize = 2;
const SIDE: usize = 3;

fn arrow_type(e: &Enemy) -> ArrowType {
    match e.ty {
        EnemyType::BoltCrystal => ArrowType::Bolt,
        EnemyType::PrismCrystal => ArrowType::Prism,
        _ => ArrowType::Laser,
    }
}

pub fn init(e: &mut Enemy) {
    e.t[CD] = 30.0;
    e.t[TSCALE] = 1.0;
    e.t[SPIN] = -1.0;
}

/// Before the components: the scale relaxes toward its target, the shoot
/// spin tween and the death wiggle (40 ticks, 4 Hz, 50 degrees) advance.
pub fn pre_update(w: &World, e: &mut Enemy) {
    let tm = w.time.time_mult;
    let ts = e.t[TSCALE];
    e.scale.x = approach(e.scale.x, ts, 0.05 * tm);
    e.scale.y = approach(e.scale.y, ts, 0.05 * tm);
    if e.t[SPIN] >= 0.0 {
        e.t[SPIN] += tm;
        let k = (e.t[SPIN] / 30.0).min(1.0);
        e.rotation = 2.0 * PI * Ease::CubeInOut.apply(k) * e.t[SPIN_DIR];
        if k >= 1.0 {
            e.t[SPIN] = -1.0;
        }
    }
    if e.state == DEAD && e.t[WIGGLE] < 40.0 {
        e.t[WIGGLE] += tm;
        let k = e.t[WIGGLE];
        let v = crate::math::cos(k * 2.0 * PI * 4.0 / 60.0) * (1.0 - k / 40.0).max(0.0);
        e.rotation = 0.872_664_6 * v;
    }
}

pub fn enter(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    if s != SHOOT {
        // `ShootLeave`.
        e.t[TSCALE] = 1.0;
    }
    match s {
        SHOOT => e.t[CD] = 60.0,
        DEAD => {
            e.t[KILL_ANGLE] = (-e.speed).angle();
            e.t[WIGGLE] = 0.0;
            w.core_mut(id).collider = Collider::Box(Hitbox::wrapping(10.0, 10.0, -5.0, -5.0));
            w.sound(
                [
                    "en_crystalDeath",
                    "en_greenCrystalDeath",
                    "en_diamondCrystalDeath",
                ][tone(e) as usize],
                w.core(id).pos.x,
            );
        }
        _ => {}
    }
}

pub fn update(w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    match e.state {
        IDLE => {
            approach_speed(e, e.target_speed, 0.04 * tm);
            if e.t[CD] > 0.0 {
                e.t[CD] -= tm;
            }
            IDLE
        }
        SHOOT => {
            approach_speed(e, Vec2::ZERO, 0.05 * tm);
            SHOOT
        }
        _ => {
            approach_speed(e, Vec2::ZERO, 0.2 * tm);
            DEAD
        }
    }
}

fn fire_sound(w: &mut World, e: &Enemy, x: f32) {
    w.sound(
        [
            "en_crystalFire",
            "en_greenCrystalFire",
            "en_diamondCrystalFire",
        ][tone(e) as usize],
        x,
    );
}

/// `Shoot`: aim along the open path to the target (remembering where it
/// was seen) or at the last sighting; recoil; one sound, then the arrows.
fn fire(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, second: bool) {
    let me = w.core(id).pos;
    let tp = e
        .target
        .filter(|&t| w.exists(t))
        .map(|t| w.core(t).pos)
        .unwrap_or(e.target_pos);
    let (open, v) = shortest_open(w, c, me, tp);
    let aim = if open {
        e.target_pos = tp;
        v
    } else {
        shortest(w, c, me, e.target_pos)
    };
    let a = aim.angle();
    e.speed = -aim.safe_normalize();
    fire_sound(w, e, me.x);
    if second {
        shoot_arrow(w, id, ArrowType::Laser, me, a + PI / 18.0);
        shoot_arrow(w, id, ArrowType::Laser, me, a - PI / 18.0);
    } else {
        shoot_arrow(w, id, arrow_type(e), me, a);
    }
}

/// A posthumous shot in direction `a` if the body, placed 10 px that way,
/// is free of Solids; credited to the killer.
fn death_shot(w: &mut World, c: &Content, id: EntityId, e: &Enemy, a: f32) -> bool {
    let me = w.core(id).pos;
    if w.check_at(
        c,
        id,
        me + crate::math::angle_to_vector(a, 10.0),
        Tag::Solid,
    ) {
        return false;
    }
    fire_sound(w, e, me.x);
    let shooter = arrow::Shooter {
        id: Some(id),
        player_index: e.killer,
        allegiance: Allegiance::Red,
        team_color: Allegiance::Red,
        hyper: false,
    };
    let aid = arrow::create(w, arrow_type(e), shooter, me, a);
    if let Some(ar) = w.arrow_mut(aid) {
        ar.cannot_hit_enemies = 10.0;
    }
    true
}

pub fn co(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, step: u16) {
    match (e.state, step) {
        (IDLE, 0) => {
            e.n[START_NODE] = e.n[NODE];
            co(w, c, id, e, 1);
        }
        (IDLE, _) => {
            if e.node_count == 0 {
                if e.t[CD] <= 0.0 && spot(w, c, id, e) {
                    return;
                }
                e.co.yield_frames(10.0, 1);
                return;
            }
            let node = e.nodes[e.n[NODE] as usize % e.node_count as usize];
            let v = node - w.core(id).pos;
            if v.length_sq() >= 100.0 {
                e.target_speed = v.safe_normalize_len(0.5);
                if e.t[CD] <= 0.0 && spot(w, c, id, e) {
                    return;
                }
                e.n[START_NODE] = e.n[NODE];
                e.co.yield_frames(10.0, 1);
            } else {
                e.n[NODE] = (e.n[NODE] + 1) % e.node_count as i32;
                if e.n[NODE] == e.n[START_NODE] {
                    e.co.yield_next(1);
                } else {
                    co(w, c, id, e, 1);
                }
            }
        }
        (SHOOT, 0) => {
            e.t[TSCALE] = 0.8;
            e.t[SPIN_DIR] = if w.rng.next_max(2) == 0 { 1.0 } else { -1.0 };
            e.t[SPIN] = 0.0;
            w.sound(
                [
                    "en_crystalCharge",
                    "en_greenCrystalCharge",
                    "en_diamondCrystalCharge",
                ][tone(e) as usize],
                w.core(id).pos.x,
            );
            e.co.yield_frames(30.0, 1);
        }
        (SHOOT, 1) => {
            fire(w, c, id, e, false);
            let blue = e.ty == EnemyType::BlueCrystal;
            if blue && e.target.is_some_and(|t| sees(w, c, id, e, t)) {
                e.co.yield_frames(2.0, 2);
            } else {
                e.t[TSCALE] = 1.0;
                e.co.yield_frames(30.0, 4);
            }
        }
        (SHOOT, 2) => {
            e.scale = Vec2::new(1.5, 1.5);
            let d = [5.0, 5.0, 10.0][w.rng.choose_index(3)];
            e.co.yield_frames(d, 3);
        }
        (SHOOT, 3) => {
            fire(w, c, id, e, true);
            e.t[TSCALE] = 1.0;
            e.co.yield_frames(30.0, 4);
        }
        (SHOOT, _) => {
            set_state(w, c, id, e, IDLE);
            e.co.finish();
        }
        (DEAD, 0) => match e.ty {
            EnemyType::EvilCrystal | EnemyType::BlueCrystal => {
                e.n[J] = 0;
                e.co.yield_frames(20.0, 1);
            }
            EnemyType::BoltCrystal => e.co.yield_frames(10.0, 10),
            _ => e.co.yield_frames(10.0, 19),
        },
        (DEAD, 19) => {
            // The side is drawn after the first wait.
            e.n[SIDE] = if w.rng.next_max(2) == 0 { 1 } else { -1 };
            e.n[J] = -1;
            co(w, c, id, e, 20);
        }
        (DEAD, 1) => {
            if e.n[J] < 4 {
                let a = -PI / 4.0 - e.n[J] as f32 * PI / 2.0;
                death_shot(w, c, id, e, a);
                e.n[J] += 1;
                e.co.yield_frames(3.0, 1);
            } else {
                e.co.yield_frames(20.0, 99);
            }
        }
        (DEAD, 10) => {
            let ka = e.t[KILL_ANGLE];
            if !death_shot(w, c, id, e, ka) {
                death_shot(w, c, id, e, ka + PI);
            }
            e.co.yield_frames(30.0, 99);
        }
        (DEAD, 20) => {
            if e.n[J] <= 1 {
                let a = e.t[KILL_ANGLE] + (PI / 15.0) * e.n[SIDE] as f32 * e.n[J] as f32;
                death_shot(w, c, id, e, a);
                e.n[J] += 1;
                e.co.yield_frames(2.0, 20);
            } else {
                e.co.yield_frames(30.0, 99);
            }
        }
        (DEAD, _) => {
            remove_now(w, id, e);
            e.co.finish();
        }
        _ => e.co.finish(),
    }
}

/// A visible player within 100 px: remember it, -> Shoot.
fn spot(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> bool {
    let Some((pid, pp, _)) = closest_player(w, c, id, e, 10000.0, true) else {
        return false;
    };
    e.target = Some(pid);
    e.target_pos = pp;
    set_state(w, c, id, e, SHOOT);
    e.co.finish();
    true
}

pub fn die(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    untag(w, id, e);
    set_state(w, c, id, e, DEAD);
}

fn tone(e: &Enemy) -> u8 {
    match e.ty {
        EnemyType::PrismCrystal => 2,
        EnemyType::BoltCrystal => 1,
        _ => 0,
    }
}
