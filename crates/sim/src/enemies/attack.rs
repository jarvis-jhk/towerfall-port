//! `EnemyAttack` (enemies.md §1.7): a short-lived invisible box bound to an
//! enemy that hurts players and optionally deflects arrows.

use super::*;
use crate::impl_hash;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnHit {
    None,
    /// Bomb bats: the owner explodes.
    Explode,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnemyAttack {
    pub owner: EntityId,
    pub offset: Vec2,
    pub w: f32,
    pub h: f32,
    pub life: f32,
    pub deflect: bool,
    /// Deflected arrows get `dir * 2` (zero = none).
    pub dir: Vec2,
    pub on_hit: OnHit,
}
impl_hash!(EnemyAttack {
    owner,
    offset,
    w,
    h,
    life,
    deflect,
    dir
});

/// Spawn an attack box for `owner` (offset = box centre, x mirrored by facing).
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    w: &mut World,
    owner: EntityId,
    e: &Enemy,
    offset: Vec2,
    bw: f32,
    bh: f32,
    life: f32,
    deflect: bool,
    dir: Vec2,
    on_hit: OnHit,
) {
    let pos = place(w.core(owner).pos, e.facing, offset, bw, bh);
    let mut core = Core::new(
        pos,
        Collider::Box(Hitbox::wrapping(bw, bh, 0.0, 0.0)),
        0,
        &[Tag::PlayerCollider, Tag::Target],
    );
    core.screen_wrap = false;
    w.add(
        core,
        Kind::EnemyAttack(EnemyAttack {
            owner,
            offset,
            w: bw,
            h: bh,
            life,
            deflect,
            dir,
            on_hit,
        }),
    );
}

fn place(owner: Vec2, facing: i8, offset: Vec2, bw: f32, bh: f32) -> Vec2 {
    owner + Vec2::new(offset.x * facing as f32 - bw / 2.0, offset.y - bh / 2.0)
}

pub fn update(w: &mut World, _c: &Content, id: EntityId, a: &mut EnemyAttack) {
    let alive = matches!(w.kind(a.owner), Some(Kind::Enemy(e)) if e.alive());
    a.life -= w.time.time_mult;
    if !alive || a.life <= 0.0 {
        w.remove(id);
        w.core_mut(id).collidable = false;
        return;
    }
    let facing = match w.kind(a.owner) {
        Some(Kind::Enemy(e)) => e.facing,
        _ => 1,
    };
    let op = w.core(a.owner).pos;
    w.core_mut(id).pos = place(op, facing, a.offset, a.w, a.h);
}

/// A player overlaps the box: hurt, then the callback.
pub fn on_player_collide(w: &mut World, c: &Content, id: EntityId, pid: EntityId, p: &mut Player) {
    let Some(Kind::EnemyAttack(a)) = w.kind(id).copied() else {
        return;
    };
    let from = w.try_core(a.owner).map(|o| o.pos).unwrap_or(w.core(id).pos);
    player::hurt(w, c, pid, p, DeathCause::Enemy, from, -1, None);
    if a.on_hit == OnHit::Explode
        && let Some(mut e) = super::take_enemy(w, a.owner)
    {
        bat::explode(w, c, a.owner, &mut e);
        if w.exists(a.owner) {
            w.put_kind(a.owner, Kind::Enemy(e));
        }
    }
}

/// `OnArrowHit`: deflecting boxes drop dangerous arrows.
pub fn on_arrow_hit(w: &mut World, id: EntityId, aid: EntityId, ar: &mut Arrow) -> bool {
    let Some(Kind::EnemyAttack(a)) = w.kind(id).copied() else {
        return false;
    };
    if !a.deflect || !ar.dangerous() {
        return false;
    }
    arrow::enter_fall_mode(w, aid, ar, true, false, true);
    if !a.dir.is_zero() {
        ar.speed = a.dir * 2.0;
    }
    true
}
