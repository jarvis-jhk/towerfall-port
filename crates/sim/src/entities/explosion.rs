//! The explosion damage protocol (tower-entities.md §4.3, arrows.md §13.1):
//! exploding orbs and spike balls use it; bomb arrows can share it. The
//! visual (lattice of fire sprites, light fade, debris) is presentation and
//! is sent as one particle event.

use super::{Tower, particles, tower, with_player};
use crate::actor::{wrap_diff_x, wrap_dist_sq};
use crate::arrow::wrap_line_hit;
use crate::collide::{Collider, Hitbox, Placed, collide_point};
use crate::content::Content;
use crate::entity::{EntityId, Kind, Tag};
use crate::math::Vec2;
use crate::player::{self, Player};
use crate::world::World;

const NOBODY: EntityId = EntityId {
    index: u32::MAX,
    generation: u32::MAX,
};

/// `Explosion.Spawn(at, playerIndex, plusOneKill)` (`super_bomb`:
/// `SpawnSuper`). `held` is the archer whose own update is running (its
/// state is outside the arena). Returns false when `at` is inside a
/// non-glass solid (nothing happens); a point inside moon glass shatters it
/// with a harmless glass explosion.
pub fn spawn(
    w: &mut World,
    c: &Content,
    at: Vec2,
    owner: i8,
    plus_one: bool,
    super_bomb: bool,
    held: Option<(EntityId, &mut Player)>,
) -> bool {
    let outer_kills = std::mem::take(&mut w.tower.blast_kills);
    let ok = spawn_inner(w, c, at, owner, super_bomb, held);
    // `plusOneKill`: the bomb that set this off already killed someone.
    let kills = std::mem::replace(&mut w.tower.blast_kills, outer_kills)
        + u32::from(plus_one && owner >= 0);
    if owner >= 0 && kills > 0 {
        crate::versus::stat(w, owner as u8, |s| {
            s.bomb_multi_kill = s.bomb_multi_kill.max(kills)
        });
    }
    ok
}

fn spawn_inner(
    w: &mut World,
    c: &Content,
    at: Vec2,
    owner: i8,
    super_bomb: bool,
    held: Option<(EntityId, &mut Player)>,
) -> bool {
    let lvl = c.level(w.level);
    let ws = w.world_size(c);
    let hit_solid = w.tagged(Tag::Solid).iter().copied().find(|&e| {
        let core = w.core(e);
        core.collidable
            && collide_point(
                &Placed {
                    pos: core.pos,
                    collider: core.collider,
                },
                at,
                lvl,
                ws,
            )
    });
    if let Some(s) = hit_solid {
        if w.core(s).has_tag(Tag::Moonglass) {
            super::blocks::moon_glass_shatter(w, c, s, held);
            return true;
        }
        return false;
    }
    w.screen_shake(12.0);
    let dark = c.level(w.level).tower.theme.dark;
    let (kind, debris) = match (super_bomb, dark) {
        (true, false) => ("ExplosionSuper", "BombParticle:Bomb"),
        (true, true) => ("ExplosionSuperGreen", "BombParticle:GreenBomb"),
        (false, false) => ("Explosion", "BombParticle:Bomb"),
        (false, true) => ("ExplosionGreen", "BombParticle:GreenBomb"),
    };
    particles(w, kind, at, 1, Vec2::ZERO, 0.0);
    particles(w, debris, at, 6, Vec2::ZERO, 0.0);

    let (reach, kill) = if super_bomb {
        (5625.0, 3025.0)
    } else {
        (2025.0, 1225.0)
    };
    let size = if super_bomb { 150.0 } else { 90.0 };
    let blast = Placed {
        pos: at,
        collider: Collider::Box(Hitbox::wrapping(size, size, -size / 2.0, -size / 2.0)),
    };
    let targets = w.all(c, &blast, Tag::ExplosionCollider, NOBODY);
    let mut held = held;
    let mut offsets = Vec::with_capacity(8);
    for e in targets {
        if !w.exists(e) {
            continue;
        }
        let was = w.core(e).collidable;
        w.core_mut(e).collidable = false;
        offsets.clear();
        let core = *w.core(e);
        match w.kind(e) {
            Some(Kind::Player(_)) | None if core.has_tag(Tag::Player) => {
                let hb = core.hitbox();
                offsets.extend_from_slice(&[
                    Vec2::new(hb.x, hb.y),
                    Vec2::new(hb.x + hb.w - 1.0, hb.y),
                    Vec2::new(hb.x, hb.y + hb.h - 1.0),
                    Vec2::new(hb.x + hb.w - 1.0, hb.y + hb.h - 1.0),
                ]);
            }
            Some(Kind::Tower(_)) => super::explosion_offsets(w, c, e, &mut offsets),
            Some(Kind::Dummy(_)) => offsets.extend_from_slice(&super::scenery::DUMMY_OFFSETS),
            Some(Kind::Enemy(en)) => {
                crate::enemies::explosion_offsets(en, core.hitbox(), &mut offsets)
            }
            _ => {}
        }
        if offsets.is_empty() {
            offsets.push(Vec2::ZERO);
        }
        let mut hit = None;
        for o in &offsets {
            let p = core.pos + *o;
            let mut q = p;
            q.x = q.x.rem_euclid(ws.w);
            q.y = q.y.rem_euclid(ws.h);
            let d = Vec2::new(wrap_diff_x(at.x, q.x, ws.w), wrap_diff_x(at.y, q.y, ws.h));
            let within = wrap_dist_sq(at, q, ws.w, ws.h) <= reach
                && (!super_bomb || d.x.abs() <= 15.0 || d.y.abs() <= 15.0);
            if within && !wrap_line_hit(w, c, at, q, Tag::Solid) {
                hit = Some(d);
                break;
            }
        }
        w.core_mut(e).collidable = was;
        let Some(vec) = hit else { continue };
        let normal = vec.safe_normalize();
        let lethal = vec.length_sq() <= kill;
        if core.has_tag(Tag::Player) {
            let act = |w: &mut World, p: &mut Player| {
                if p.dead {
                    return;
                }
                player::on_explode_push(w, c, e, p, normal);
                if lethal {
                    player::explode_hurt(w, c, e, p, at, owner);
                    // `explosion.Kills` (unfriendly victims only).
                    if p.dead && !crate::hurt::is_friendly(w, owner, p.index as i8) {
                        w.tower.blast_kills += 1;
                    }
                }
            };
            match held.as_mut() {
                Some((hid, hp)) if *hid == e => act(w, hp),
                _ => {
                    with_player(w, e, |w, p| act(w, p));
                }
            }
        } else if let Some(Kind::Corpse(cp)) = w.kind_mut(e) {
            // DieByExplosion: thrown away from the centre at 6, facing back
            // toward the blast, on fire.
            cp.speed = (core.pos - at).safe_normalize_len(6.0);
            cp.death_angle = cp.speed.angle();
            if cp.speed.x > 0.0 {
                cp.facing = -1;
            } else if cp.speed.x < 0.0 {
                cp.facing = 1;
            }
            cp.fire_t = crate::corpse::FIRE_TIME;
        } else if tower(w, e).is_some() {
            super::on_explode_push(w, c, e, normal, owner);
        } else if matches!(w.kind(e), Some(Kind::Enemy(_))) {
            crate::enemies::on_explode(w, c, e, normal, owner, false);
            if lethal {
                crate::enemies::on_explode(w, c, e, normal, owner, true);
            }
        } else if matches!(w.kind(e), Some(Kind::Dw(_))) {
            crate::darkworld::part::on_explode(w, c, e, owner, lethal);
        } else if matches!(w.kind(e), Some(Kind::Dummy(_))) {
            crate::trials::dummy_explode(w, c, e, normal);
        } else if let Some(Kind::Bramble(_)) = w.kind(e) {
            // Brambles burn when pushed and go inside the kill radius.
            crate::arrows::brambles::ignite(w, e);
            if lethal {
                crate::arrows::brambles::destroy(w, e);
            }
        }
    }
    true
}

/// Whether an entity is one of the breakable tower blocks (for tests).
pub fn is_breakable(w: &World, id: EntityId) -> bool {
    matches!(
        tower(w, id),
        Some(Tower::BreakBlock(_) | Tower::CrackedPlatform(_) | Tower::CrumbleWall(_))
    )
}
