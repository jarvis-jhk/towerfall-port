//! The drill arrow's wall drilling (arrows.md §9.6).

use crate::actor;
use crate::arrow::{self, Arrow, ArrowState, SeekQuery, Sight};
use crate::collide::{Collider, Hitbox, Placed};
use crate::content::Content;
use crate::entity::{EntityId, Kind, Tag, depth};
use crate::events::{Event, ParticleKind};
use crate::math::{PI, angle_diff, angle_to_vector, clamp};
use crate::world::World;

pub const DRILL_HITBOX: Hitbox = Hitbox::wrapping(6.0, 6.0, -3.0, -3.0);
pub const DEPTH_DRILLING: i32 = -158;

/// Granite blocks stop drills (tower entity; tagged `Granite`).
pub fn is_granite(w: &World, platform: EntityId) -> bool {
    w.try_core(platform)
        .is_some_and(|c| c.has_tag(Tag::Granite))
}

/// The seek sight rule of a drill arrow.
pub fn sight(a: &Arrow) -> Sight {
    if a.extra.has_drilled && a.state != ArrowState::Drilling {
        Sight::Normal
    } else {
        Sight::GraniteOnly
    }
}

/// Wall contact in Shooting/Gravity: drill instead of sticking. Returns
/// true when the contact was consumed.
pub fn on_collide(w: &mut World, id: EntityId, a: &mut Arrow, platform: EntityId) -> bool {
    let granite = is_granite(w, platform);
    if !a.extra.has_drilled && a.state < ArrowState::Falling && !granite {
        start_drilling(w, id, a);
        return true;
    }
    if granite {
        w.sound("sfx_drillCollide", w.core(id).pos.x);
    }
    false
}

/// `StartDrilling()`.
pub fn start_drilling(w: &mut World, id: EntityId, a: &mut Arrow) {
    w.set_depth(id, DEPTH_DRILLING);
    a.state = ArrowState::Drilling;
    a.speed *= 0.3;
    if !crate::pickups::variant(
        w,
        crate::variants::Variant::InfiniteDrills,
        a.player_index as i32,
    ) {
        a.extra.has_drilled = true;
    }
    w.core_mut(id).naive_push = true;
    let at = w.core(id).pos;
    w.sound("sfx_drillIn", at.x);
    w.emit(Event::Particles {
        kind: ParticleKind::DrillDig,
        player: u8::MAX,
        pos: at,
        count: 6,
        dir: a.direction,
    });
}

/// One `Drilling` frame of `ArrowUpdate`.
pub fn drilling_update(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow) {
    let tm = w.time.time_mult;
    let v = w.variants_for(a.player_index as i32);
    // 1. speed up toward 3, then seek through ordinary walls.
    let len = a.speed.length();
    if len < 3.0 && len > 0.0 {
        a.speed = a.speed.normalize() * crate::math::min(3.0, len + 0.05 * tm);
    }
    if !v.no_seeking_arrows
        && let Some(hit) = arrow::find_seek_target(w, c, id, a, &SeekQuery::default_for(a))
    {
        let mut r = PI / 90.0 * tm;
        if v.super_seeking_arrows {
            r *= 2.0;
        }
        a.direction += clamp(angle_diff(a.direction, hit.angle), -r, r);
        a.speed = angle_to_vector(a.direction, a.speed.length());
        crate::enemies::on_arrow_seeking(w, c, hit.target, id, a);
    }
    // 2.
    actor::move_ignore_solids(w, id, a.speed * tm);
    let pos = w.core(id).pos;
    let drill = Placed {
        pos,
        collider: Collider::Box(DRILL_HITBOX),
    };
    // 3. out of the wall
    if !w.any(c, &drill, Tag::Solid, id) && !w.any(c, &drill, Tag::JumpThru, id) {
        w.set_depth(id, depth::ARROW);
        a.state = ArrowState::Shooting;
        a.speed = a.speed.safe_normalize() * 4.0;
        a.direction = a.speed.angle();
        a.shooting = 15.0;
        w.sound("sfx_drillOut", pos.x);
        w.emit(Event::Particles {
            kind: ParticleKind::DrillDig,
            player: u8::MAX,
            pos,
            count: 6,
            dir: a.direction,
        });
        return;
    }
    // 4. two drills meeting inside a wall destroy each other
    if let Some(o) = w.first(c, &drill, Tag::Arrow, id)
        && let Some(Kind::Arrow(oa)) = w.kind(o).copied()
        && oa.state == ArrowState::Drilling
        && a.speed.dot(oa.speed) < 0.5
    {
        w.remove(id);
        w.remove(o);
        w.sound("sfx_drillCollide", pos.x);
        return;
    }
    // 5. a drilling enemy (the mole underground) in the drill box is hurt
    // by the arrow, and the arrow is gone.
    if crate::enemies::drill_hit(w, c, &drill, id, a) {
        w.remove(id);
        w.core_mut(id).collidable = false;
        return;
    }
    // 6. granite: stuck for good
    let tip = Placed {
        pos,
        collider: Collider::Box(arrow::HITBOX),
    };
    if w.any(c, &tip, Tag::Granite, id) {
        a.extra.inert = true;
        let core = w.core_mut(id);
        core.active = false;
        core.collidable = false;
    }
}
