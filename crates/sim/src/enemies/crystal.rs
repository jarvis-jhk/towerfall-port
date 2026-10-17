//! Evil crystals: red/blue (laser), green (bolt), pink (prism); enemies.md §4.9.

use super::*;

pub const IDLE: u8 = 0;
pub const SHOOT: u8 = 1;
pub const DEAD: u8 = 2;
const CD: usize = 0;
const KILL_ANGLE: usize = 1;
const J: usize = 0;
const NODE: usize = 1;
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
}

pub fn enter(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    match s {
        SHOOT => e.t[CD] = 60.0,
        DEAD => {
            e.t[KILL_ANGLE] = (-e.speed).angle();
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
            e.t[CD] -= tm;
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

fn fire(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, offset: f32) {
    let me = w.core(id).pos;
    let tp = e
        .target
        .filter(|&t| w.exists(t))
        .map(|t| w.core(t).pos)
        .unwrap_or(e.target_pos);
    let (open, v) = shortest_open(w, c, me, tp);
    let aim = if open {
        v
    } else {
        shortest(w, c, me, e.target_pos)
    };
    let a = aim.angle() + offset;
    shoot_arrow(w, id, arrow_type(e), me, a);
    e.speed = -aim.safe_normalize();
    w.sound(
        [
            "en_crystalFire",
            "en_greenCrystalFire",
            "en_diamondCrystalFire",
        ][tone(e) as usize],
        me.x,
    );
}

/// A posthumous shot in direction `a` (if no Solid within 10 px), credited
/// to the killer.
fn death_shot(w: &mut World, c: &Content, id: EntityId, e: &Enemy, a: f32) -> bool {
    let me = w.core(id).pos;
    let ws = w.world_size(c);
    if w.point_check(
        c,
        wrap(me + crate::math::angle_to_vector(a, 10.0), ws.w, ws.h),
        Tag::Solid,
    ) {
        return false;
    }
    let shooter = arrow::Shooter {
        id: Some(id),
        player_index: e.killer,
        allegiance: Allegiance::Red,
        team_color: Allegiance::Red,
        hyper: false,
    };
    arrow::create(w, arrow_type(e), shooter, me, a);
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
                if e.t[CD] <= 0.0
                    && let Some((pid, pp, _)) = closest_player(w, c, id, e, 10000.0, true)
                {
                    e.target = Some(pid);
                    e.target_pos = pp;
                    set_state(w, c, id, e, SHOOT);
                    e.co.finish();
                    return;
                }
                e.co.yield_frames(10.0, 1);
                return;
            }
            let node = e.nodes[e.n[NODE] as usize % e.node_count as usize];
            let v = shortest(w, c, w.core(id).pos, node);
            if v.length_sq() >= 100.0 {
                e.target_speed = v.safe_normalize() * 0.5;
                if e.t[CD] <= 0.0
                    && let Some((pid, pp, _)) = closest_player(w, c, id, e, 10000.0, true)
                {
                    e.target = Some(pid);
                    e.target_pos = pp;
                    set_state(w, c, id, e, SHOOT);
                    e.co.finish();
                    return;
                }
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
            e.scale = Vec2::new(0.8, 0.8);
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
            fire(w, c, id, e, 0.0);
            let blue = e.ty == EnemyType::BlueCrystal;
            if blue && e.target.is_some_and(|t| sees(w, c, id, e, t)) {
                e.co.yield_frames(2.0, 2);
            } else {
                e.scale = Vec2::ONE;
                e.co.yield_frames(30.0, 4);
            }
        }
        (SHOOT, 2) => {
            e.scale = Vec2::new(1.5, 1.5);
            let d = [5.0, 5.0, 10.0][w.rng.choose_index(3)];
            e.co.yield_frames(d, 3);
        }
        (SHOOT, 3) => {
            fire(w, c, id, e, -PI / 18.0);
            fire(w, c, id, e, PI / 18.0);
            e.scale = Vec2::ONE;
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
            _ => {
                e.n[SIDE] = if w.rng.next_max(2) == 0 { -1 } else { 1 };
                e.n[J] = -1;
                e.co.yield_frames(10.0, 20);
            }
        },
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
