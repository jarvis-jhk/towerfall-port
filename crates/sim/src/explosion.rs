//! Explosions and shocks (arrows.md §13). Blasts go through the shared
//! damage step [`crate::entities::explode`] (push and kill radii, the super
//! bomb's cross, moonglass, kill credit = the owner, self-kills included);
//! this module adds the spawn variants the arrows need and the bolt-catch
//! `ShockCircle`.

use crate::actor::wrap_dist_sq;
use crate::arrow::{self, ArrowState};
use crate::collide::{Collider, Hitbox, Placed};
use crate::content::Content;
use crate::entity::{Core, EntityId, Kind, Tag};
use crate::events::{Event, ParticleKind};
use crate::hurt::{self, Ctx};
use crate::math::Vec2;
use crate::world::World;
use crate::{impl_hash, impl_hash_enum};

/// The solid containing point `at`, if any.
fn solid_at(w: &World, c: &Content, at: Vec2) -> Option<EntityId> {
    let lvl = c.level(w.level);
    let ws = w.world_size(c);
    w.tagged(Tag::Solid).iter().copied().find(|&e| {
        let t = w.core(e);
        t.collidable
            && crate::collide::collide_point(
                &Placed {
                    pos: t.pos,
                    collider: t.collider,
                },
                at,
                lvl,
                ws,
            )
    })
}

fn held<'a>(ctx: &'a mut Ctx) -> Option<(EntityId, &'a mut crate::player::Player)> {
    ctx.as_mut().map(|(id, p)| (*id, &mut **p))
}

/// `Explosion.Spawn` (bombs, bomb chests): false when `at` is inside a
/// non-moonglass solid.
pub fn spawn(
    w: &mut World,
    c: &Content,
    at: Vec2,
    player_index: i8,
    plus_one_kill: bool,
    ctx: &mut Ctx,
) -> bool {
    crate::entities::explode(w, c, at, player_index, plus_one_kill, false, held(ctx))
}

/// `Explosion.SpawnSuper` (cross-shaped).
pub fn spawn_super(
    w: &mut World,
    c: &Content,
    at: Vec2,
    player_index: i8,
    plus_one_kill: bool,
    ctx: &mut Ctx,
) -> bool {
    crate::entities::explode(w, c, at, player_index, plus_one_kill, true, held(ctx))
}

/// `Explosion.SpawnGet` (trigger arrows): no live explosion for any solid
/// (moonglass still shatters). Returns whether a live blast went off.
pub fn spawn_get(
    w: &mut World,
    c: &Content,
    at: Vec2,
    player_index: i8,
    plus_one_kill: bool,
    ctx: &mut Ctx,
) -> bool {
    w.items.blast = crate::pickups::Blast::Trigger;
    let r = match solid_at(w, c, at) {
        Some(s) => {
            if w.core(s).has_tag(Tag::Moonglass) {
                crate::entities::explode(w, c, at, player_index, plus_one_kill, false, held(ctx));
            }
            false
        }
        None => crate::entities::explode(w, c, at, player_index, plus_one_kill, false, held(ctx)),
    };
    w.items.blast = crate::pickups::Blast::Normal;
    r
}

/// The frozen victim of a detonated trigger arrow is hurt at once
/// (arrows.md §9.9; a Dying archer is not collidable, so the blast itself
/// misses it).
pub fn hurt_frozen_victim(
    w: &mut World,
    c: &Content,
    at: Vec2,
    player_index: i8,
    victim: EntityId,
    ctx: &mut Ctx,
) {
    hurt::with_player_ctx(w, victim, ctx, |w, p| {
        if !p.dead {
            crate::player::explode_hurt(w, c, victim, p, at, player_index);
        }
    });
}

/// `ExplosionCheckOffsets` of an archer as absolute points.
pub fn check_points(w: &World, id: EntityId) -> Vec<Vec2> {
    let core = w.core(id);
    match w.kind(id) {
        Some(Kind::Player(_)) => {
            let (l, t, r, b) = (core.left(), core.top(), core.right(), core.bottom());
            vec![
                Vec2::new(l, t),
                Vec2::new(r - 1.0, t),
                Vec2::new(l, b - 1.0),
                Vec2::new(r - 1.0, b - 1.0),
            ]
        }
        _ => vec![core.pos],
    }
}

// ------------------------------------------------------------- shocks

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShockKind {
    BoltCatch,
    TeamRevive,
}
impl_hash_enum!(ShockKind);

pub const SHOCK_DEPTH: i32 = -101;
pub const SHOCK_LIFETIME: f32 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shock {
    pub kind: ShockKind,
    pub player_index: i8,
    pub age: f32,
    pub done: bool,
}
impl_hash!(Shock {
    kind,
    player_index,
    age,
    done
});

/// A `ShockCircle` of type BoltCatch at the catcher.
pub fn spawn_bolt_catch(w: &mut World, at: Vec2, catcher: i8) -> EntityId {
    let mut core = Core::new(
        at,
        Collider::Box(Hitbox::wrapping(56.0, 56.0, -28.0, -28.0)),
        SHOCK_DEPTH,
        &[Tag::LightSource],
    );
    core.screen_wrap = true;
    w.add(
        core,
        Kind::Shock(Shock {
            kind: ShockKind::BoltCatch,
            player_index: catcher,
            age: 0.0,
            done: false,
        }),
    )
}

/// A `ShockCircle` of type TeamRevive at a revived archer (player.md
/// §18.2): 64x64, radius 32; then 76x76, radius 38 for enemies and
/// shock colliders.
pub fn spawn_team_revive(w: &mut World, at: Vec2, revived: i8) -> EntityId {
    let mut core = Core::new(
        at,
        Collider::Box(Hitbox::wrapping(64.0, 64.0, -32.0, -32.0)),
        SHOCK_DEPTH,
        &[Tag::LightSource],
    );
    core.screen_wrap = true;
    w.add(
        core,
        Kind::Shock(Shock {
            kind: ShockKind::TeamRevive,
            player_index: revived,
            age: 0.0,
            done: false,
        }),
    )
}

/// `ShockCircle.Added()`: hurt archers, knock arrows down, clear brambles.
pub fn shock_added(w: &mut World, c: &Content, id: EntityId) {
    let Some(Kind::Shock(mut s)) = w.kind(id).copied() else {
        return;
    };
    if s.done {
        return;
    }
    s.done = true;
    if let Some(Kind::Shock(slot)) = w.kind_mut(id) {
        *slot = s;
    }
    let team = s.kind == ShockKind::TeamRevive;
    let mut r_sq = if team { 1024.0 } else { 784.0 };
    let centre = w.core(id).pos;
    let me = w.placed(id);
    let ws = w.world_size(c);
    w.emit(Event::Particles {
        kind: ParticleKind::ShockCircle,
        player: u8::MAX,
        pos: centre,
        count: 1,
        dir: 0.0,
    });
    let players: Vec<EntityId> = w.players().collect();
    for pid in players {
        let Some(p) = w.player(pid) else { continue };
        if p.dead || p.index as i8 == s.player_index {
            continue;
        }
        if team && !crate::coop::team_revive_can_hurt(w, s.player_index, p.index as i8) {
            continue;
        }
        if !w.check(c, &me, pid) {
            continue;
        }
        let near = check_points(w, pid)
            .iter()
            .any(|&q| wrap_dist_sq(centre, q, ws.w, ws.h) <= r_sq);
        if near {
            hurt::with_player(w, pid, |w, p| {
                hurt::hurt(
                    w,
                    c,
                    pid,
                    p,
                    crate::player::DeathCause::Shock,
                    centre,
                    s.player_index,
                    None,
                )
            });
        }
    }
    let arrows: Vec<EntityId> = w.tagged(Tag::Arrow).to_vec();
    for a in arrows {
        let Some(ar) = w.arrow(a).copied() else {
            continue;
        };
        if matches!(ar.state, ArrowState::Stuck | ArrowState::LayingOnGround) {
            continue;
        }
        if !w.check(c, &me, a) || wrap_dist_sq(centre, w.core(a).pos, ws.w, ws.h) > r_sq {
            continue;
        }
        crate::arrows::with_arrow(w, a, |w, ar| {
            arrow::enter_fall_mode(w, a, ar, true, false, true)
        });
    }
    let mut me = me;
    if team {
        r_sq = 1444.0;
        me.collider = Collider::Box(Hitbox::wrapping(76.0, 76.0, -38.0, -38.0));
    }
    crate::enemies::on_shock(w, c, &me, r_sq, s.player_index);
    crate::darkworld::part::on_shock(w, c, &me, r_sq);
    let colliders: Vec<EntityId> = w.tagged(Tag::ShockCollider).to_vec();
    for e in colliders {
        if w.check(c, &me, e)
            && wrap_dist_sq(centre, w.core(e).pos, ws.w, ws.h) <= r_sq
            && let Some(Kind::Bramble(_)) = w.kind(e)
        {
            crate::arrows::brambles::destroy(w, e);
        }
    }
}

pub fn shock_update(w: &mut World, _c: &Content, id: EntityId, s: &mut Shock) {
    s.age += w.time.time_mult;
    if s.age >= SHOCK_LIFETIME {
        w.remove(id);
    }
}
