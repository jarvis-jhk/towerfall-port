//! The bolt arrow's flight (arrows.md §9.7): no gravity, a narrow homing
//! window, and right-angle turns toward archers it can see.

use crate::arrow::{self, Arrow, SeekQuery, Sight};
use crate::content::Content;
use crate::entity::EntityId;
use crate::events::{Event, ParticleKind};
use crate::math::{PI, angle_between, angle_diff, angle_to_vector, clamp};
use crate::world::World;

pub const SPEED: f32 = 4.0;

/// The bolt's `ShootUpdate`.
pub fn shoot_update(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow) {
    let tm = w.time.time_mult;
    let v = w.variants_for(a.player_index as i32);
    let q = SeekQuery {
        radius_sq: 6400.0,
        max_angle: PI / 6.0,
        ..SeekQuery::default_for(a)
    };
    if let Some(hit) = arrow::find_seek_target(w, c, id, a, &q) {
        if !v.no_seeking_arrows {
            let mut r = PI / 180.0 * tm;
            if v.super_seeking_arrows {
                r *= 2.0;
            }
            let unwrapped = angle_between(w.core(id).pos, hit.point);
            a.direction += clamp(angle_diff(a.direction, unwrapped), -r, r);
            a.speed = angle_to_vector(a.direction, SPEED);
        }
        crate::enemies::on_arrow_seeking(w, c, hit.target, id, a);
        return;
    }
    if a.extra.can_turn > 0.0 {
        a.extra.can_turn -= tm;
        return;
    }
    if !try_turn(w, c, id, a, PI / 2.0) {
        try_turn(w, c, id, a, -PI / 2.0);
    }
}

/// `TryTurn(angle)`.
fn try_turn(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow, angle: f32) -> bool {
    let orig = a.direction;
    a.direction += angle;
    let q = SeekQuery {
        radius_sq: 25600.0,
        min_dist_sq: 100.0,
        max_angle: 2.0 * PI / 15.0,
        min_priority: 1,
        sight: Sight::Normal,
        swept: true,
    };
    let Some(hit) = arrow::find_seek_target(w, c, id, a, &q) else {
        a.direction = orig;
        return false;
    };
    let at = w.core(id).pos;
    w.emit(Event::Particles {
        kind: ParticleKind::SmallShock,
        player: u8::MAX,
        pos: at,
        count: 1,
        dir: 0.0,
    });
    w.sound("sfx_boltArrowTurn", at.x);
    a.direction = hit.angle;
    a.speed = angle_to_vector(a.direction, SPEED);
    a.extra.can_turn = 5.0;
    a.extra.turns = a.extra.turns.saturating_add(1);
    true
}
