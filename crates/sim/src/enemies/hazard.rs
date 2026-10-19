//! Enemy projectiles and boss props: Tornado (§4.5), TechnoMissile
//! (§4.12), and the King Reaper's bombs, laser crystals and beams (§4.13).

use super::*;
use crate::impl_hash_enum;
use crate::math::{angle_approach, angle_to_vector, approach, cos, sin};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HazardType {
    Tornado,
    TechnoMissile,
    ReaperBomb,
    ReaperCrystal,
    ReaperBeam,
}
impl_hash_enum!(HazardType);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hazard {
    pub ty: HazardType,
    pub speed: f32,
    pub dir: Vec2,
    pub vel: Vec2,
    pub target: Option<EntityId>,
    pub owner: Option<EntityId>,
    /// Generic timers: [age, phase timer, misc, misc].
    pub t: [f32; 4],
    pub angle: f32,
    pub sine: SineWave,
    pub alpha: f32,
    pub bolt: bool,
}

impl StableHash for Hazard {
    fn stable_hash(&self, h: &mut Hasher) {
        self.ty.stable_hash(h);
        h.f32(self.speed);
        h.vec2(self.dir);
        h.vec2(self.vel);
        self.target.stable_hash(h);
        self.t.stable_hash(h);
        h.f32(self.angle);
    }
}

const AGE: usize = 0;

fn add(
    w: &mut World,
    pos: Vec2,
    col: Hitbox,
    target_col: Option<Hitbox>,
    tags: &[Tag],
    h: Hazard,
) -> EntityId {
    let mut core = Core::new(pos, Collider::Box(col), -60, tags);
    core.target_collider = target_col;
    core.screen_wrap = true;
    core.ignore_jump_thrus = true;
    w.add(core, Kind::Hazard(h))
}

fn blank(ty: HazardType) -> Hazard {
    Hazard {
        ty,
        speed: 0.0,
        dir: Vec2::new(1.0, 0.0),
        vel: Vec2::ZERO,
        target: None,
        owner: None,
        t: [0.0; 4],
        angle: 0.0,
        sine: SineWave::default(),
        alpha: 1.0,
        bolt: false,
    }
}

pub fn spawn_tornado(w: &mut World, pos: Vec2, dir: Vec2, target: Option<EntityId>) -> EntityId {
    let mut h = blank(HazardType::Tornado);
    h.speed = 2.8;
    h.dir = dir;
    h.target = target;
    h.sine = SineWave::frames(20.0);
    // The fade runs from 1.2 (enemies.md 4.5; drawn clamped to 1).
    h.alpha = 1.2;
    add(
        w,
        pos,
        Hitbox::wrapping(8.0, 8.0, -4.0, -4.0),
        Some(Hitbox::wrapping(16.0, 16.0, -8.0, -8.0)),
        &[Tag::PlayerCollider, Tag::Target],
        h,
    )
}

/// A techno mage's missile homing on `target` (enemies.md §4.12): depth
/// -89, seek-able at priority 0 with neutral allegiance.
pub fn spawn_missile(w: &mut World, pos: Vec2, dir: Vec2, target: Option<EntityId>) -> EntityId {
    let mut h = blank(HazardType::TechnoMissile);
    h.speed = 0.5;
    h.dir = dir;
    h.target = target;
    let id = add(
        w,
        pos,
        Hitbox::wrapping(8.0, 8.0, -4.0, -4.0),
        None,
        &[Tag::PlayerCollider, Tag::Target],
        h,
    );
    let core = w.core_mut(id);
    core.depth = -89;
    core.seek = true;
    core.seek_priority = 0;
    core.allegiance = Allegiance::Neutral;
    id
}

pub fn spawn_bomb(w: &mut World, pos: Vec2, vel: Vec2, owner: EntityId) -> EntityId {
    let mut h = blank(HazardType::ReaperBomb);
    h.vel = vel;
    h.speed = vel.length();
    h.owner = Some(owner);
    let id = add(
        w,
        pos,
        Hitbox::wrapping(8.0, 8.0, -4.0, -4.0),
        None,
        &[Tag::PlayerCollider, Tag::Target],
        h,
    );
    // A plain actor: it lands on jump-thrus.
    w.core_mut(id).ignore_jump_thrus = false;
    id
}

pub fn spawn_crystal(w: &mut World, owner: EntityId, angle: f32, bolt: bool) -> EntityId {
    let mut h = blank(HazardType::ReaperCrystal);
    h.owner = Some(owner);
    h.angle = angle;
    h.bolt = bolt;
    let pos = w.core(owner).pos;
    add(
        w,
        pos,
        Hitbox::wrapping(10.0, 10.0, -5.0, -5.0),
        Some(Hitbox::wrapping(14.0, 14.0, -7.0, -7.0)),
        &[Tag::PlayerCollider, Tag::Target],
        h,
    )
}

pub fn spawn_beam(w: &mut World, owner: EntityId, angle: f32) -> EntityId {
    let mut h = blank(HazardType::ReaperBeam);
    h.owner = Some(owner);
    h.angle = angle;
    let pos = w.core(owner).pos;
    add(w, pos, Hitbox::new(0.0, 0.0, 0.0, 0.0), None, &[], h)
}

pub fn update(w: &mut World, c: &Content, id: EntityId, h: &mut Hazard) {
    let tm = w.time.time_mult;
    h.t[AGE] += tm;
    h.sine.update(tm);
    match h.ty {
        HazardType::Tornado => tornado(w, c, id, h, tm),
        HazardType::TechnoMissile => missile(w, c, id, h, tm),
        HazardType::ReaperBomb => bomb(w, c, id, h, tm),
        HazardType::ReaperCrystal => reaper::crystal_update(w, c, id, h, tm),
        HazardType::ReaperBeam => reaper::beam_update(w, c, id, h, tm),
    }
    if w.exists(id) {
        actor::enforce_screen_wrap(w, c, id);
    }
}

fn tornado(w: &mut World, c: &Content, id: EntityId, h: &mut Hazard, tm: f32) {
    // Steer toward a living player target; a dead one is dropped for good.
    if let Some(t) = h.target {
        if w.player(t).is_none_or(|p| p.dead) {
            h.target = None;
        } else {
            let me = w.core(id).pos;
            let v = shortest(w, c, me, w.core(t).pos);
            let a = angle_approach(h.dir.angle(), v.angle(), PI / 90.0 * tm);
            h.dir = angle_to_vector(a, 1.0);
        }
    }
    h.speed = approach(h.speed, 0.0, 0.05 * tm);
    // A free (non-actor) move per axis: a step into a Solid is undone and
    // flips that axis of the direction (enemies.md §4.5).
    let perp = Vec2::new(-h.dir.y, h.dir.x);
    let v = (h.dir + perp * (0.4 * h.sine.value)) * h.speed * tm;
    let p = w.core(id).pos;
    let px = Vec2::new(p.x + v.x, p.y);
    if w.check_at(c, id, px, Tag::Solid) {
        h.dir.x = -h.dir.x;
    } else {
        w.core_mut(id).pos = px;
    }
    let p = w.core(id).pos;
    let py = Vec2::new(p.x, p.y + v.y);
    if w.check_at(c, id, py, Tag::Solid) {
        h.dir.y = -h.dir.y;
    } else {
        w.core_mut(id).pos = py;
    }
    // Dangerous until it slows below 0.6; the fade starts the tick after.
    if w.core(id).collidable {
        if h.speed < 0.6 {
            w.core_mut(id).collidable = false;
        }
        return;
    }
    h.alpha -= 0.1 * tm;
    if h.alpha <= 0.0 {
        w.remove(id);
    }
}

/// Heading turns at most 0.5 degrees a tick toward its (living) target;
/// moves directly, sliding along walls for 30 updates, then exploding on
/// contact (§4.12).
fn missile(w: &mut World, c: &Content, id: EntityId, h: &mut Hazard, tm: f32) {
    let me = w.core(id).pos;
    if let Some(t) = h.target
        && w.player(t).is_some_and(|p| !p.dead)
    {
        let to = w.core(t).pos - me;
        let max = PI / 360.0 * tm;
        let a = crate::math::clamp(
            crate::math::angle_diff(h.dir.angle(), to.angle()),
            -max,
            max,
        );
        let (s, co) = (sin(a), cos(a));
        h.dir = Vec2::new(h.dir.x * co - h.dir.y * s, h.dir.x * s + h.dir.y * co);
    }
    h.speed = approach(h.speed, 2.5, 0.05 * tm);
    let step = h.speed * tm;
    if h.t[AGE] <= 30.0 {
        let p = w.core(id).pos + Vec2::new(h.dir.x * step, 0.0);
        if w.check_at(c, id, p, Tag::Solid) {
            h.dir.x = 0.0;
            h.dir = h.dir.safe_normalize();
        } else {
            w.core_mut(id).pos = p;
        }
        let p = w.core(id).pos + Vec2::new(0.0, h.dir.y * step);
        if w.check_at(c, id, p, Tag::Solid) {
            h.dir.y = 0.0;
            h.dir = h.dir.safe_normalize();
        } else {
            w.core_mut(id).pos = p;
        }
    } else {
        let p = w.core(id).pos + h.dir * step;
        if w.check_at(c, id, p, Tag::Solid) {
            missile_explode(w, c, id);
        } else {
            w.core_mut(id).pos = p;
        }
    }
}

/// Harmless and untouchable by arrows for its first 20 updates.
fn missile_live(h: &Hazard) -> bool {
    h.t[AGE] >= 20.0
}

fn missile_explode(w: &mut World, c: &Content, id: EntityId) {
    if w.core(id).marked_for_removal {
        return;
    }
    w.sound("pu_bombArrowExplode", w.core(id).pos.x);
    explode(w, c, id, -1);
}

/// Reaper bomb (enemies.md §4.13): slows while moving; the tick after it
/// is found at rest its 20-tick swell starts (`t[3]` = started, `t[1]` =
/// ticks left), once, and it explodes when the swell ends.
fn bomb(w: &mut World, c: &Content, id: EntityId, h: &mut Hazard, tm: f32) {
    if h.t[3] > 0.0 {
        h.t[1] -= tm;
        if h.t[1] <= 0.0 {
            bomb_explode(w, c, id);
            return;
        }
    }
    if h.vel == Vec2::ZERO {
        if h.t[3] == 0.0 {
            h.t[3] = 1.0;
            h.t[1] = 20.0;
        }
    } else {
        h.vel = crate::math::approach_vec(h.vel, Vec2::ZERO, 0.032 * tm);
        if actor::move_h(w, c, id, h.vel.x * tm).is_some() {
            h.vel.x *= -0.5;
        }
        if actor::move_v(w, c, id, h.vel.y * tm).is_some() {
            h.vel.y *= -0.5;
        }
    }
    h.speed = h.vel.length();
}

/// The reaper bomb goes off: its sound, a plain ownerless explosion.
fn bomb_explode(w: &mut World, c: &Content, id: EntityId) {
    if w.core(id).marked_for_removal {
        return;
    }
    w.sound("pu_bombArrowExplode", w.core(id).pos.x);
    explode(w, c, id, -1);
}

pub fn explode(w: &mut World, c: &Content, id: EntityId, owner: i8) {
    if w.core(id).marked_for_removal {
        return;
    }
    let at = w.core(id).pos;
    w.core_mut(id).collidable = false;
    w.remove(id);
    blast::explode(w, c, at, owner, false);
}

/// A player touches a hazard.
pub fn on_player_collide(w: &mut World, c: &Content, id: EntityId, pid: EntityId, p: &mut Player) {
    let Some(Kind::Hazard(h)) = w.kind(id).copied() else {
        return;
    };
    match h.ty {
        HazardType::Tornado | HazardType::ReaperCrystal => {
            let from = w.core(id).pos;
            player::hurt(w, c, pid, p, DeathCause::Enemy, from, -1, None);
        }
        HazardType::TechnoMissile => {
            if missile_live(&h) {
                missile_explode(w, c, id);
            }
        }
        HazardType::ReaperBomb => bomb_explode(w, c, id),
        HazardType::ReaperBeam => {}
    }
}

/// An arrow hits a hazard.
pub fn on_arrow_hit(
    w: &mut World,
    c: &Content,
    id: EntityId,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    let Some(Kind::Hazard(h)) = w.kind(id).copied() else {
        return false;
    };
    match h.ty {
        HazardType::Tornado => {
            arrow::enter_fall_mode(w, aid, a, true, false, true);
            true
        }
        HazardType::TechnoMissile => {
            if !missile_live(&h) {
                return false;
            }
            arrow::enter_fall_mode(w, aid, a, true, false, false);
            missile_explode(w, c, id);
            true
        }
        HazardType::ReaperBomb => {
            if let Some(Kind::Hazard(hh)) = w.kind_mut(id) {
                hh.vel = a.speed.safe_normalize();
                hh.speed = 1.0;
            }
            arrow::enter_fall_mode(w, aid, a, true, false, true);
            true
        }
        HazardType::ReaperCrystal => {
            arrow::enter_fall_mode(w, aid, a, true, false, false);
            reaper::crystal_break(w, id);
            true
        }
        HazardType::ReaperBeam => false,
    }
}

/// Unit vector of an angle (helper for beams).
pub fn unit(a: f32) -> Vec2 {
    Vec2::new(cos(a), sin(a))
}
