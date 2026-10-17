//! `PlayerGhost` (player.md §18.1): a dead archer's ghost steered by its
//! owner (Return As Ghosts variant), an `Enemy` that kills enemy archers.

use super::*;
use crate::components::Ease;
use crate::math::approach;

pub const SPAWN: u8 = 0;
pub const NORMAL: u8 = 1;
pub const DODGE: u8 = 2;
pub const DEAD: u8 = 3;
pub const DESPAWN: u8 = 4;
const T: usize = 0;
const MOVE_MAX: usize = 1;
const KILL_DELAY: usize = 2;
const DODGE_CD: usize = 3;
/// Last unquantised aim angle.
const LAST_AIM: usize = 4;

/// Spawn the ghost of `owner` at `corpse_pos + (0, 4)`.
pub fn spawn_for(
    w: &mut World,
    c: &Content,
    owner: u8,
    corpse_pos: Vec2,
    corpse_speed: Vec2,
    allegiance: Allegiance,
) -> EntityId {
    let id = spawn(
        w,
        c,
        EnemyType::PlayerGhost,
        corpse_pos + Vec2::new(0.0, 4.0),
        1,
        &[],
    );
    w.tag(id, Tag::PlayerGhostCollider, true);
    w.tag(id, Tag::PlayerGhost, true);
    let core = w.core_mut(id);
    core.depth = -200;
    core.seek = false;
    core.allegiance = allegiance;
    if let Some(Kind::Enemy(e)) = w.kind_mut(id) {
        e.owner = owner as i8;
        e.speed = if corpse_speed.is_zero() {
            Vec2::new(0.0, -2.0)
        } else {
            -corpse_speed.safe_normalize() * 2.0
        };
    }
    id
}

pub fn init(e: &mut Enemy) {
    e.flags = F_CAN_BOUNCE_ON;
    e.bounty = 0;
    e.t[MOVE_MAX] = 1.6;
    e.t[KILL_DELAY] = 30.0;
    e.sine = SineWave::frames(60.0);
}

pub fn enter(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy, s: u8) {
    e.t[T] = 0.0;
    if s == NORMAL {
        e.t[MOVE_MAX] = 1.6;
    }
}

fn input(w: &World, e: &Enemy) -> crate::input::InputState {
    w.inputs
        .get(e.owner.max(0) as usize)
        .map(|i| i.state)
        .unwrap_or_default()
}

pub fn update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    let inp = input(w, e);
    e.t[T] += tm;
    if e.state != SPAWN && e.t[KILL_DELAY] > 0.0 {
        e.t[KILL_DELAY] -= tm;
    }
    if e.t[DODGE_CD] > 0.0 {
        e.t[DODGE_CD] -= tm;
    }
    match e.state {
        SPAWN => {
            approach_speed(e, Vec2::ZERO, 0.05 * tm);
            if !inp.aim_axis.is_zero() {
                let v = inp.aim_axis * (0.8 * tm);
                if actor::move_h(w, c, id, v.x).is_some() {
                    e.speed.x = 0.0;
                }
                if actor::move_v(w, c, id, v.y).is_some() {
                    e.speed.y = 0.0;
                }
            }
            if e.t[T] >= 30.0 { NORMAL } else { SPAWN }
        }
        NORMAL => {
            if (inp.dodge_pressed || inp.shoot_pressed || inp.alt_shoot_pressed)
                && e.t[DODGE_CD] <= 0.0
            {
                let dir = if inp.aim_axis.is_zero() {
                    Vec2::new(e.facing as f32, 0.0)
                } else {
                    inp.aim_axis.safe_normalize()
                };
                e.speed = dir * 6.0;
                return DODGE;
            }
            let target = if inp.aim_axis.is_zero() {
                e.t[MOVE_MAX] = 1.6;
                Vec2::ZERO
            } else {
                let a = inp.aim_axis.angle();
                if a == e.t[LAST_AIM] && e.speed.length() >= e.t[MOVE_MAX] - 0.1 {
                    e.t[MOVE_MAX] = (e.t[MOVE_MAX] + 0.02 * tm).min(2.8);
                } else {
                    e.t[MOVE_MAX] = 1.6;
                }
                e.t[LAST_AIM] = a;
                inp.aim_axis.safe_normalize() * e.t[MOVE_MAX]
            };
            approach_speed(e, target, 0.3 * tm);
            set_facing(e, inp.aim_axis.x);
            // The bob is a direct move, not a speed.
            let bob = Vec2::new(e.sine.value, e.sine.value_over_two) * (0.2 * tm);
            actor::move_h(w, c, id, bob.x);
            actor::move_v(w, c, id, bob.y);
            NORMAL
        }
        DODGE => {
            approach_speed(e, Vec2::ZERO, 0.4 * tm);
            if e.t[T] >= 10.0 {
                e.t[DODGE_CD] = 30.0;
                return NORMAL;
            }
            DODGE
        }
        DEAD => {
            approach_speed(e, Vec2::ZERO, 0.2 * tm);
            if e.t[T] >= 30.0 {
                remove_now(w, id, e);
            }
            DEAD
        }
        _ => {
            // Fly to the corpse over 30 ticks (CubeIn) and vanish.
            let k = Ease::CubeIn.apply((e.t[T] / 30.0).min(1.0));
            let p = w.core(id).pos;
            w.core_mut(id).pos = p + (e.target_pos - p) * k;
            e.speed = Vec2::ZERO;
            if e.t[T] >= 30.0 {
                remove_now(w, id, e);
            }
            DESPAWN
        }
    }
}

/// The `PlayerGhostCollider` pass (after moving, not while spawning):
/// other ghosts are pushed apart, orbs and icicles fall, small chests
/// open with a forced Bomb, big mushrooms wiggle.
pub fn ghost_collisions(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    if !w.core(id).collidable || e.state == SPAWN {
        return;
    }
    let list = w.tagged(Tag::PlayerGhostCollider).to_vec();
    for t in list {
        if t == id || !w.exists(t) {
            continue;
        }
        let me = w.placed(id);
        if !w.check(c, &me, t) {
            continue;
        }
        match w.kind(t).copied() {
            Some(Kind::Enemy(mut o)) if o.ty == EnemyType::PlayerGhost => {
                if o.state != SPAWN {
                    let v = (w.core(id).pos - w.core(t).pos).safe_normalize();
                    o.speed = -v * 2.0;
                    e.speed = v * 2.0;
                    w.put_kind(t, Kind::Enemy(o));
                }
            }
            Some(Kind::Chest(_)) => crate::treasure::ghost_open(w, c, t, e.owner),
            Some(Kind::Tower(_)) => {
                crate::entities::on_player_ghost(w, c, t, id, e.owner, e.speed.x)
            }
            _ => {}
        }
        if !w.core(id).collidable {
            break;
        }
    }
}

pub fn co(_w: &mut World, _c: &Content, _id: EntityId, e: &mut Enemy, _step: u16) {
    e.co.finish();
}

/// `OnCollideH/V`: first try to slip up to 4 px around the corner (one
/// pixel per hit); otherwise bounce back (above 2.2) or stop.
pub fn collide(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, horizontal: bool) {
    let s = if horizontal {
        crate::math::sign(e.speed.x)
    } else {
        crate::math::sign(e.speed.y)
    };
    let at = w.core(id).pos;
    for i in 1..=2 {
        for j in [-1.0f32, 1.0] {
            let off = if horizontal {
                Vec2::new(s, i as f32 * j * 2.0)
            } else {
                Vec2::new(i as f32 * j * 2.0, s)
            };
            if !w.check_at(c, id, at + off, Tag::Solid) {
                if horizontal {
                    actor::move_v(w, c, id, j);
                } else {
                    actor::move_h(w, c, id, j);
                }
                return;
            }
        }
    }
    let v = if horizontal {
        &mut e.speed.x
    } else {
        &mut e.speed.y
    };
    if v.abs() > 2.2 {
        *v = -*v * 0.6;
    } else {
        *v = 0.0;
    }
}

/// Arrows: not while spawning; with No Friendly Fire not the owner's own
/// arrows nor arrows of the ghost's (non-neutral) colour.
pub fn can_be_hit(w: &World, id: EntityId, e: &Enemy, aid: EntityId, a: &Arrow) -> bool {
    if e.state == SPAWN || !e.alive() {
        return false;
    }
    let mine = w.core(id).allegiance;
    let same_colour = mine != Allegiance::Neutral && w.core(aid).allegiance == mine;
    if w.variants_for(-1).no_friendly_fire && (same_colour || a.player_index == e.owner) {
        return false;
    }
    true
}

/// The ghost's own archer (revived meanwhile) and dying ghosts collide
/// with nobody.
pub fn collides_with(e: &Enemy, p: &Player) -> bool {
    p.index as i8 != e.owner && e.state != DESPAWN && e.state != DEAD
}

/// `OnPlayerBounce` of a ghost: an ally of its colour pushes it down
/// (no damage); returns whether that happened.
pub fn friendly_stomp(w: &World, id: EntityId, e: &mut Enemy, p: &Player) -> bool {
    let a = w.core(id).allegiance;
    if a != Allegiance::Neutral && !p.is_enemy(a) {
        e.speed.y = 3.0;
        e.scale = Vec2::new(1.5, 0.5);
        return true;
    }
    false
}

/// Touching an archer: kill an enemy archer once the kill delay is over,
/// else push it away; the ghost is flung back either way.
pub fn touch(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    pid: EntityId,
    p: &mut Player,
) {
    if p.index as i8 == e.owner || e.state == DESPAWN || e.state == DEAD {
        return;
    }
    let me = w.core(id).pos;
    let pp = w.core(pid).pos;
    let friendly =
        !p.is_enemy(w.core(id).allegiance) && w.core(id).allegiance != Allegiance::Neutral;
    if e.t[KILL_DELAY] <= 0.0 && !friendly {
        player::hurt(w, c, pid, p, DeathCause::Enemy, me, e.owner, None);
        if p.dead && e.owner >= 0 {
            crate::versus::stat(w, e.owner as u8, |s| s.kills_as_ghost += 1);
        }
        w.emit(crate::Event::Rumble {
            player: e.owner.max(0) as u8,
            strength: 1.0,
            frames: 30,
        });
    } else {
        p.speed = (pp - me).safe_normalize_len(3.2);
    }
    e.speed = (me - pp).safe_normalize_len(3.0);
    let _ = approach(0.0, 0.0, 0.0);
}

pub fn die(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    if e.killer >= 0 {
        crate::versus::stat(w, e.killer as u8, |s| s.ghost_kills += 1);
    }
    untag(w, id, e);
    set_state(w, c, id, e, DEAD);
}

/// `Despawn(corpse)`: the owner was revived.
pub fn despawn(w: &mut World, c: &Content, owner: u8, corpse_pos: Vec2) {
    let ids: Vec<EntityId> = w.entities().collect();
    for id in ids {
        if let Some(Kind::Enemy(mut e)) = w.kind(id).copied()
            && e.ty == EnemyType::PlayerGhost
            && e.owner == owner as i8
            && e.state != DESPAWN
        {
            e.target_pos = corpse_pos;
            switch_state(w, c, id, &mut e, DESPAWN);
            w.core_mut(id).collidable = false;
            if let Some(Kind::Enemy(slot)) = w.kind_mut(id) {
                *slot = e;
            }
        }
    }
}
