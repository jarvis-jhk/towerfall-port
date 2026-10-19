//! Skeleton family (enemies.md §4.6): plain/shielded/typed, winged, mimic,
//! jester, boss. Archers that aim, shoot, dodge and catch arrows.

use super::*;
use crate::events::CharSound;
use crate::math::{angle_to_vector, approach, max, min, snap};

pub const IDLE: u8 = 0;
pub const ALERT: u8 = 1;
pub const SHOOT: u8 = 2;
pub const DODGE: u8 = 3;

const DODGE_CD: usize = 0;
const CAN_SHOOT: usize = 1;
const FAKE_OUT: usize = 2;
const FAIL_ADD: usize = 3;
const TX: usize = 4;
const GRAV: usize = 5;
const JUMP_CD: usize = 0;
const FLAP_CD: usize = 1;
const GLIDE: usize = 2;
const REPEATS: usize = 3;

pub const F_SHIELD: u16 = F_TYPE;
const F_AIMING: u16 = F_TYPE << 1;
const F_MIMICKED: u16 = F_TYPE << 2;
const F_CAN_MIMIC: u16 = F_TYPE << 3;
/// Grounded at the start of the tick (used by gravity and Alert).
const F_ON_GROUND: u16 = F_TYPE << 4;

const COLLECT: Hitbox = Hitbox::wrapping(20.0, 24.0, -10.0, -12.0);

fn kind(e: &Enemy) -> SkelKind {
    match e.ty {
        EnemyType::Skeleton(k) => k,
        _ => unreachable!(),
    }
}

pub fn init(_w: &mut World, _c: &Content, _core: &mut Core, e: &mut Enemy) {
    let k = kind(e);
    e.flags |= F_BOUNCES_ON_PLAYER;
    e.arrows.add(k.arrow);
    e.arrows.add(k.arrow);
    e.set(F_SHIELD, k.shield);
    e.set(F_CAN_MIMIC, k.mimic);
    e.t[CAN_SHOOT] = 60.0;
    e.t[FAKE_OUT] = 30.0;
    e.t[FAIL_ADD] = -0.1;
    e.t[GRAV] = 1.0;
    e.n[FLAP_CD] = 30;
    set_mimic_archer(e, -1);
}

/// Ticks left of the wings' flap animation (glide does not count down
/// while it plays).
fn wing_flap(e: &Enemy) -> f32 {
    e.target_pos.y
}
fn set_wing_flap(e: &mut Enemy, t: f32) {
    e.target_pos.y = t;
}

/// The archer whose look and voice the skeleton uses (jesters: Cyan,
/// mimics: the archer copied), or -1.
fn mimic_archer(e: &Enemy) -> i32 {
    if kind(e).jester {
        6
    } else {
        e.target_speed.x as i32
    }
}
fn set_mimic_archer(e: &mut Enemy, a: i32) {
    e.target_speed.x = a as f32;
}

/// A cue from the skeleton's own set, or the copied archer's voice.
fn sfx(w: &mut World, e: &Enemy, x: f32, own: &'static str, voice: CharSound) {
    let a = mimic_archer(e);
    if a >= 0 {
        w.emit(crate::Event::CharSound {
            sound: voice,
            player: u8::MAX,
            archer: a as u16,
            x,
        });
    } else {
        w.sound(own, x);
    }
}

fn aim_angle(e: &Enemy) -> f32 {
    e.target_pos.x
}
fn set_aim(e: &mut Enemy, a: f32) {
    e.target_pos.x = a;
}

pub fn enter(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    match s {
        ALERT => {
            e.t[TX] = 0.0;
            if let Some(t) = e.target {
                mimic(w, id, e, t);
            }
        }
        DODGE => {
            e.t[FAIL_ADD] = max(0.1, e.t[FAIL_ADD] + 0.1);
            e.t[CAN_SHOOT] = 20.0;
            e.t[DODGE_CD] = if kind(e).jester { 30.0 } else { 90.0 };
            // The archers' dodge cue, by direction.
            let right = if e.speed.x.abs() <= 0.1 {
                e.speed.y < 0.0
            } else {
                e.speed.x > 0.0
            };
            let name = if right { "char_dashR" } else { "char_dashL" };
            w.sound(name, w.core(id).pos.x);
        }
        _ => {}
    }
}

pub fn leave(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy, s: u8) {
    if s == SHOOT {
        e.set(F_AIMING, false);
    }
}

/// Presentation: the aim angle while the skeleton aims.
pub fn aim_view(e: &Enemy) -> Option<f32> {
    e.has(F_AIMING).then(|| aim_angle(e))
}

/// Presentation: a mimic that copied a player (its look follows `target`).
pub fn mimicked(e: &Enemy) -> bool {
    e.has(F_MIMICKED)
}

fn mimic(w: &mut World, id: EntityId, e: &mut Enemy, pid: EntityId) {
    if e.has(F_CAN_MIMIC)
        && let Some(p) = w.player(pid)
    {
        let archer = p.archer as i32;
        e.set(F_CAN_MIMIC, false);
        e.set(F_MIMICKED, true);
        set_mimic_archer(e, archer);
        w.sound("en_shadowTransform", w.core(id).pos.x);
    }
}

/// Ground test, wings, gravity, walking speed and facing (before the
/// components run, enemies.md §4.6 per-tick 1-2).
pub fn pre_update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    let tm = w.time.time_mult;
    let k = kind(e);
    relax_scale(w, e);
    let grounded = on_ground(w, c, id);
    e.set(F_ON_GROUND, grounded);
    if k.wings {
        e.t[GRAV] = approach(e.t[GRAV], 1.0, 0.05 * tm);
        if wing_flap(e) > 0.0 {
            set_wing_flap(e, wing_flap(e) - tm);
        } else if e.n[GLIDE] > 0 {
            e.n[GLIDE] -= 1;
        }
    }
    if e.state == DODGE {
        return;
    }
    if !grounded {
        let term = if e.n[GLIDE] > 0 { 0.9 } else { 1.8 };
        e.speed.y = min(e.speed.y + 0.15 * e.t[GRAV] * tm, term);
    }
    e.speed.x = approach(e.speed.x, e.t[TX], 0.1 * tm);
    set_facing(e, e.speed.x);
}

fn closest_on_side(w: &World, c: &Content, id: EntityId, e: &Enemy) -> Option<EntityId> {
    let from = w.core(id).pos + e.sight;
    let ws = w.world_size(c);
    let mut best: Option<(EntityId, f32)> = None;
    for (pid, pos) in players(w, c, from) {
        let d = wrap_dist_sq(from, pos, ws.w, ws.h);
        let side = sign(shortest(w, c, from, pos).x) as i8 == e.facing;
        if !((d < 14400.0 && side) || d < 2500.0) {
            continue;
        }
        if !can_see(w, c, from, pos, false) {
            continue;
        }
        if best.is_none_or(|b| d < b.1) {
            best = Some((pid, d));
        }
    }
    best.map(|b| b.0)
}

fn snapped_angle(w: &World, c: &Content, id: EntityId, target: EntityId) -> f32 {
    let v = shortest(w, c, w.core(id).pos, w.core(target).pos);
    snap(v.angle(), PI / 8.0)
}

/// `CanShoot`: its hitbox moved 20 px along the direction is clear.
fn can_shoot_dir(w: &World, c: &Content, id: EntityId, angle: f32) -> bool {
    !solid_at(w, c, id, angle_to_vector(angle, 20.0))
}

pub fn update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    match e.state {
        IDLE => {
            e.t[FAIL_ADD] = approach(e.t[FAIL_ADD], 0.0, 0.002 * tm);
            if let Some(t) = closest_on_side(w, c, id, e) {
                e.target = Some(t);
                ALERT
            } else {
                IDLE
            }
        }
        ALERT => alert(w, c, id, e),
        DODGE => {
            approach_speed(e, Vec2::ZERO, 0.15 * tm);
            DODGE
        }
        s => s,
    }
}

/// After the whole base update (move, arrow collection and the state
/// switch): unless dodging or flashing, the counters and the fake-out /
/// arrow dodges (enemies.md §4.6 per-tick 3).
pub fn post_update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    if !e.alive() || e.state == DODGE || w.core(id).flash.flashing {
        return;
    }
    if reactions(w, c, id, e) {
        set_state(w, c, id, e, DODGE);
    }
}

fn alert(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let Some(t) = e.target.filter(|&t| w.player(t).is_some_and(|p| !p.dead)) else {
        return IDLE;
    };
    if !e.arrows.is_empty() && e.t[CAN_SHOOT] <= 0.0 {
        let a = snapped_angle(w, c, id, t);
        if can_shoot_dir(w, c, id, a) {
            return SHOOT;
        }
    }
    let me = w.core(id).pos;
    let v = shortest(w, c, me, w.core(t).pos);
    e.t[TX] = sign(v.x) * 0.8;
    let ax = v.x.abs();
    let ay = -v.y;
    if ax <= 40.0 && ay >= 5.0 {
        e.t[TX] = -e.t[TX];
    }
    if e.t[DODGE_CD] <= 0.0 && (10.0..=40.0).contains(&ay) && ax <= 20.0 {
        // The dash replaces the whole velocity.
        e.speed = Vec2::new(-3.0 * sign(v.x), 0.0);
        return DODGE;
    }
    if e.n[FLAP_CD] > 0 {
        e.n[FLAP_CD] -= 1;
    }
    let ahead = solid_at(w, c, id, Vec2::new(e.facing as f32, 0.0));
    let x = me.x;
    if e.has(F_ON_GROUND) {
        if e.n[JUMP_CD] > 0 {
            e.n[JUMP_CD] -= 1;
        } else if ay >= -12.0 || ahead {
            sfx(w, e, x, "en_skelJump", CharSound::Jump);
            e.speed.y = -3.5;
            e.n[JUMP_CD] = 10;
        }
    } else if e.n[FLAP_CD] <= 0 && kind(e).wings && (ay <= -12.0 || ahead) {
        e.speed.y = -2.0;
        e.t[GRAV] = 0.0;
        e.n[FLAP_CD] = 30;
        e.n[GLIDE] = if w.rng.next_max(2) == 0 { 30 } else { 60 };
        w.sound("pu_wingFlySkel", x);
        set_wing_flap(e, anim_ticks(0.05, 5));
    }
    ALERT
}

/// Counters, fake-out and arrow dodges; true when it starts a dodge
/// (velocity, facing and target already set).
fn reactions(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> bool {
    let tm = w.time.time_mult;
    let k = kind(e);
    if e.t[DODGE_CD] > 0.0 {
        e.t[DODGE_CD] -= tm;
    }
    if e.t[CAN_SHOOT] > 0.0 {
        e.t[CAN_SHOOT] -= tm;
    }
    let me = w.core(id).pos;
    let eye = me + e.sight;
    let ws = w.world_size(c);
    let ignore_jt = w.core(id).ignore_jump_thrus;
    let shielded = e.has(F_SHIELD);
    e.t[FAKE_OUT] -= tm;
    if e.t[FAKE_OUT] <= 0.0 {
        e.t[FAKE_OUT] = 10.0;
        if e.t[DODGE_CD] <= 0.0 && !shielded {
            for (pid, pp) in players(w, c, eye) {
                if !vision_cone(w, c, eye, facing_angle(e.facing), 11.0 * PI / 18.0, pp)
                    || wrap_dist_sq(eye, pp, ws.w, ws.h) > 10000.0
                    || !can_see(w, c, eye, pp, ignore_jt)
                {
                    continue;
                }
                let pl = w.player(pid).unwrap();
                let mut chance = 0.2;
                if !k.jester && pl.aiming {
                    let to_me = shortest(w, c, pp, me).angle();
                    if abs_angle_diff(pl.aim_direction, to_me) <= 50.0 * PI / 180.0 {
                        chance = 0.4;
                    }
                }
                if e.arrows.is_empty() {
                    chance += 0.2;
                }
                if w.rng.chance(chance) {
                    // The plain (unwrapped) vector to the archer.
                    e.set(F_AIMING, false);
                    e.target = Some(pid);
                    e.speed = (pp - me).safe_normalize() * 3.0;
                    set_facing(e, e.speed.x);
                    return true;
                }
            }
        }
    }
    if e.t[DODGE_CD] > 0.0 || shielded {
        return false;
    }
    let arrows: Vec<EntityId> = w.tagged(Tag::Arrow).to_vec();
    for aid in arrows {
        let Some(a) = w.arrow(aid).copied() else {
            continue;
        };
        if a.owner == Some(id) {
            continue;
        }
        let special = matches!(
            a.arrow_type,
            ArrowType::Bomb | ArrowType::SuperBomb | ArrowType::Trigger
        ) && !w.core(aid).marked_for_removal
            && a.can_catch(id);
        if !(a.dangerous() || special) {
            continue;
        }
        let ap = w.core(aid).pos;
        if wrap_dist_sq(me, ap, ws.w, ws.h) > 1764.0 {
            continue;
        }
        if !k.jester {
            let front = vision_cone(w, c, eye, facing_angle(e.facing), 1.745_329_3, ap);
            let f = if front { 0.1 } else { 0.2 } + e.t[FAIL_ADD];
            if w.rng.chance(f) {
                e.t[DODGE_CD] = 20.0;
                return false;
            }
        }
        e.set(F_AIMING, false);
        let owner = a.owner.filter(|&o| w.player(o).is_some());
        if let Some(o) = owner {
            mimic(w, id, e, o);
            e.target = Some(o);
        }
        e.speed = (ap - me).safe_normalize() * 3.0;
        set_facing(e, e.speed.x);
        return true;
    }
    false
}

pub fn co(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, step: u16) {
    let x = w.core(id).pos.x;
    match (e.state, step) {
        (IDLE, 0) => {
            e.t[TX] = e.facing as f32 * 0.3;
            let d = [60.0, 120.0, 180.0][w.rng.choose_index(3)];
            e.co.yield_frames(d, 1);
        }
        (IDLE, 1) => {
            e.t[TX] = 0.0;
            let d = [40.0, 60.0][w.rng.choose_index(2)];
            e.co.yield_frames(d, 2);
        }
        (IDLE, _) => {
            e.facing = if w.rng.next_max(2) == 0 { -1 } else { 1 };
            if solid_at(w, c, id, Vec2::new(6.0 * e.facing as f32, 0.0)) {
                e.facing = -e.facing;
            }
            co(w, c, id, e, 0);
        }
        // Step 0 (entry) and 2 (30 ticks after losing sight): give up
        // unless it sees the target; step 1 checks every 30 ticks.
        (ALERT, _) => {
            let sees_t = e.target.is_some_and(|t| sees(w, c, id, e, t));
            if sees_t {
                e.co.yield_frames(30.0, 1);
            } else if step == 1 {
                e.co.yield_frames(30.0, 2);
            } else {
                e.target = None;
                set_state(w, c, id, e, IDLE);
                e.co.finish();
            }
        }
        (SHOOT, 0) => {
            sfx(w, e, x, "en_skelDraw", CharSound::Aim);
            aim(w, c, id, e, true);
            e.co.yield_frames(10.0, 1);
        }
        (SHOOT, 1) => {
            let ok = !e.arrows.is_empty() && e.target.is_some_and(|t| sees(w, c, id, e, t));
            if !ok {
                end_shoot(w, c, id, e);
                return;
            }
            if !e.has(F_AIMING) {
                sfx(w, e, x, "en_skelDraw", CharSound::Aim);
            }
            e.set(F_AIMING, true);
            let t = e.target.unwrap();
            if shortest(w, c, w.core(id).pos, w.core(t).pos).length_sq() >= 900.0 {
                aim(w, c, id, e, true);
                e.co.yield_frames(10.0, 2);
            } else {
                co(w, c, id, e, 2);
            }
        }
        (SHOOT, 2) => {
            e.n[REPEATS] = [0, 0, 0, 0, 1, 1, 2, 3, 4][w.rng.choose_index(9)];
            co(w, c, id, e, 3);
        }
        (SHOOT, 3) => {
            if e.n[REPEATS] > 0 {
                aim(w, c, id, e, false);
                e.n[REPEATS] -= 1;
                e.co.yield_frames(5.0, 3);
            } else {
                co(w, c, id, e, 4);
            }
        }
        (SHOOT, 4) => {
            aim(w, c, id, e, false);
            e.set(F_AIMING, false);
            let Some(next) = e.arrows.next() else {
                end_shoot(w, c, id, e);
                return;
            };
            let visible = e.target.is_some_and(|t| sees(w, c, id, e, t));
            if next != ArrowType::Drill && (!can_shoot_dir(w, c, id, aim_angle(e)) || !visible) {
                end_shoot(w, c, id, e);
                return;
            }
            sfx(w, e, x, "en_skelShoot", CharSound::FireArrow);
            e.arrows.take();
            let pos = w.core(id).pos;
            let a = shoot_arrow(w, id, next, pos, aim_angle(e));
            laser_start(w, a);
            e.co.yield_frames(5.0, 1);
        }
        (SHOOT, _) => end_shoot(w, c, id, e),
        (DODGE, 0) => e.co.yield_frames(20.0, 1),
        (DODGE, 1) => {
            if kind(e).jester && jester_warp(w, c, id, e) {
                e.co.yield_frames(10.0, 2);
            } else {
                co(w, c, id, e, 2);
            }
        }
        (DODGE, _) => {
            let s = if e.target.is_some() { ALERT } else { IDLE };
            set_state(w, c, id, e, s);
            e.co.finish();
        }
        _ => e.co.finish(),
    }
}

fn end_shoot(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    e.set(F_AIMING, false);
    set_state(w, c, id, e, ALERT);
    e.co.finish();
}

/// Aim at the target, snapped to 22.5 degrees, with a random jitter.
fn aim(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, jitter: bool) {
    let Some(t) = e.target.filter(|&t| w.exists(t)) else {
        return;
    };
    let mut a = snapped_angle(w, c, id, t);
    if jitter {
        let d = 22.5 * PI / 180.0;
        a += [-d, 0.0, 0.0, d][w.rng.choose_index(4)];
    }
    set_aim(e, a);
    e.set(F_AIMING, true);
}

/// Jester: teleport to the second-best `Event` marker (§4.6).
fn jester_warp(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> bool {
    let me = w.core(id).pos;
    let tp = e
        .target
        .and_then(|t| w.try_core(t))
        .map(|t| t.pos)
        .unwrap_or(me);
    let ws = w.world_size(c);
    let mut pts = c.level(w.level).markers.events.clone();
    if pts.len() < 2 {
        return false;
    }
    // Points within 20 px (plain distance) of the jester go last.
    pts.sort_by(|a, b| {
        let na = (*a - me).length_sq() <= 400.0;
        let nb = (*b - me).length_sq() <= 400.0;
        na.cmp(&nb)
            .then(wrap_dist_sq(*a, tp, ws.w, ws.h).total_cmp(&wrap_dist_sq(*b, tp, ws.w, ws.h)))
    });
    w.sound("sfx_cyanWarp", me.x);
    w.core_mut(id).pos = pts[1];
    true
}

pub fn collide_h(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy) {
    if e.state == IDLE {
        e.facing = -e.facing;
        e.t[TX] = -e.t[TX];
    }
    e.speed.x = 0.0;
}

/// Landing (or a ceiling): stop, land cue, body squash (relaxed by the
/// renderer at 0.04/tick).
pub fn collide_v(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy) {
    e.speed.y = 0.0;
    let x = w.core(id).pos.x;
    let own = if e.has(F_CAN_MIMIC) {
        "en_shadowLand"
    } else {
        "en_skelLand"
    };
    sfx(w, e, x, own, CharSound::Land);
    e.scale = Vec2::new(1.2, 0.8);
}

/// Body squash relaxing toward 1 (the humanoid renderer's 0.04/tick).
pub fn relax_scale(w: &World, e: &mut Enemy) {
    let tm = w.time.time_mult;
    e.scale.x = approach(e.scale.x, 1.0, 0.04 * tm);
    e.scale.y = approach(e.scale.y, 1.0, 0.04 * tm);
}

pub fn break_shield(w: &mut World, id: EntityId, e: &mut Enemy, force: Vec2) {
    e.speed = force;
    e.set(F_SHIELD, false);
    w.core_mut(id)
        .flash
        .start_for(30.0, crate::entity::FlashFinish::None);
    w.sound("pu_shieldImp", w.core(id).pos.x);
}

/// Catch an arrow while dodging without a shield.
pub fn try_catch(
    w: &mut World,
    _c: &Content,
    id: EntityId,
    e: &mut Enemy,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    if e.state != DODGE
        || e.has(F_SHIELD)
        || e.arrows.len() >= 3
        || a.cannot_hit == Some(id)
        || !a.can_catch(id)
    {
        return false;
    }
    let at = w.core(id).pos;
    if crate::arrows::prism_check(a) {
        // Trapped in a prism instead of holding the arrow (§1.9).
        e.prism = Some(crate::arrows::prism::start_enemy_prism(
            w,
            id,
            e.prism,
            a.player_index,
        ));
    } else {
        if a.arrow_type == ArrowType::Bolt && a.dangerous() {
            // The bolt's enemy-catch callback: a BoltCatch shock owned by
            // nobody (it hurts every archer in range, never enemies).
            crate::explosion::spawn_bolt_catch(w, at, -1);
            w.sound("sfx_boltArrowExplode", at.x);
        }
        if a.arrow_type == ArrowType::Trigger {
            crate::arrows::remove_detonator(w, aid, a, &mut None);
        }
        e.arrows.add(a.arrow_type);
    }
    e.speed = Vec2::ZERO;
    w.core_mut(aid).collidable = false;
    w.remove(aid);
    super::enemy_char_sound(w, crate::events::CharSound::ArrowGrab, at.x);
    true
}

/// `LaserBounceStart = 6`: skeleton lasers bounce once, then stick.
fn laser_start(w: &mut World, a: EntityId) {
    if let Some(ar) = w.arrow_mut(a)
        && ar.arrow_type == ArrowType::Laser
    {
        ar.extra.bounced = 6;
    }
}

/// Trigger arrows this skeleton shot (it is their detonator, arrows.md
/// §9.9): every 30 ticks each one detonates with chance 1/2 when a
/// detectable archer is within 45 px of it with a clear line, unless the
/// skeleton itself is that close with a clear line.
pub fn trigger_update(w: &mut World, c: &Content, id: EntityId, e: &Enemy) {
    if !e.alive() || !w.time.on_interval(30) {
        return;
    }
    let ws = w.world_size(c);
    let me = w.core(id).pos;
    let mine: Vec<EntityId> = w
        .tagged(Tag::Arrow)
        .iter()
        .copied()
        .filter(|&a| w.arrow(a).is_some_and(|ar| ar.extra.detonator == Some(id)))
        .collect();
    for a in mine {
        if !w.rng.chance(0.5) {
            continue;
        }
        let at = w.core(a).pos;
        let near = |w: &World, p: Vec2| {
            wrap_dist_sq(at, p, ws.w, ws.h) <= 2025.0 && can_see(w, c, at, p, true)
        };
        if near(w, me) {
            continue;
        }
        if !players(w, c, at).iter().any(|&(_, p)| near(w, p)) {
            continue;
        }
        if let Some(Kind::Arrow(mut ar)) = w.take_kind(a) {
            crate::arrows::detonate(w, c, a, &mut ar, &mut None);
            if w.exists(a) {
                w.put_kind(a, Kind::Arrow(ar));
            }
        }
    }
}

/// Pick up resting arrows / catch flying ones (while fewer than 3 held).
pub fn collect_arrows(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    if e.arrows.len() >= 3 || !e.alive() {
        return;
    }
    let probe = Placed {
        pos: w.core(id).pos,
        collider: Collider::Box(COLLECT),
    };
    for aid in w.all(c, &probe, Tag::Arrow, id) {
        if e.arrows.len() >= 3 {
            break;
        }
        let Some(Kind::Arrow(mut a)) = w.kind(aid).copied() else {
            continue;
        };
        if a.collectible() {
            e.arrows.add(a.arrow_type);
            w.core_mut(aid).collidable = false;
            w.remove(aid);
            w.sound("en_skelArrowRecover", w.core(id).pos.x);
        } else if try_catch(w, c, id, e, aid, &mut a) {
            continue;
        }
    }
}

pub fn die(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy) {
    if e.has(F_AIMING)
        && let Some(t) = e.arrows.take()
    {
        let pos = w.core(id).pos;
        let a = shoot_arrow(w, id, t, pos, aim_angle(e));
        laser_start(w, a);
        if let Some(Kind::Arrow(mut ar)) = w.take_kind(a) {
            arrow::drop(w, a, &mut ar, -(e.facing as f32));
            w.put_kind(a, Kind::Arrow(ar));
        }
        w.sound("sfx_dropDrawn", pos.x);
    }
    if kind(e).jester {
        sfx(w, e, w.core(id).pos.x, "", CharSound::Die);
    }
    remove_now(w, id, e);
}

/// Cooldown star (enemies.md §1.10): dodge cooldown running, not dodging,
/// not mimicking.
pub fn cooldown_star(e: &Enemy) -> Option<Vec2> {
    (e.alive() && e.state != DODGE && e.t[DODGE_CD] > 0.0 && !e.has(F_MIMICKED))
        .then_some(Vec2::new(0.0, -11.0))
}
