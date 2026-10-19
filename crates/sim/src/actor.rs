//! `Actor` movement (engine.md §8.2–8.3): pixel stepping against `Solid`
//! (both axes) and `JumpThru` (downward only, "not already overlapping"),
//! banker's rounding of the sub-pixel remainder.
//!
//! Collision callbacks are returned instead of called: `move_h`/`move_v`
//! return the blocking platform (after zeroing the remainder), and the
//! caller runs its own `onCollide` right away, which is the same order as
//! the original (the move ends at the first block).

use crate::collide::Placed;
use crate::content::Content;
use crate::entity::{EntityId, Kind, Tag};
use crate::math::{Vec2, approach, round_he_i};
use crate::world::World;

/// The direction a pushing solid squishes an actor toward.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SquishDir {
    Left,
    Right,
    Up,
    Down,
}

impl SquishDir {
    pub fn vec(self) -> Vec2 {
        match self {
            SquishDir::Left => Vec2::new(-1.0, 0.0),
            SquishDir::Right => Vec2::new(1.0, 0.0),
            SquishDir::Up => Vec2::new(0.0, -1.0),
            SquishDir::Down => Vec2::new(0.0, 1.0),
        }
    }
}

/// Probe for `Solid` at `pos + d` with the actor's current collider.
#[inline]
fn solid_at(w: &World, c: &Content, id: EntityId, at: Vec2) -> Option<EntityId> {
    w.first_at(c, id, at, Tag::Solid)
}

/// `MoveH(amount)`: returns the solid that blocked the move, if any.
pub fn move_h(w: &mut World, c: &Content, id: EntityId, amount: f32) -> Option<EntityId> {
    let core = w.core_mut(id);
    core.counter.x += amount;
    let mut n = round_he_i(core.counter.x);
    if n == 0 {
        return None;
    }
    let s = n.signum();
    core.counter.x -= n as f32;
    while n != 0 {
        let p = w.core(id).pos;
        if let Some(hit) = solid_at(w, c, id, Vec2::new(p.x + s as f32, p.y)) {
            w.core_mut(id).counter.x = 0.0;
            return Some(hit);
        }
        w.core_mut(id).pos.x += s as f32;
        n -= s;
    }
    None
}

/// `MoveV(amount)`: returns the solid or jump-thru that blocked the move.
pub fn move_v(w: &mut World, c: &Content, id: EntityId, amount: f32) -> Option<EntityId> {
    let core = w.core_mut(id);
    core.counter.y += amount;
    let mut n = round_he_i(core.counter.y);
    if n == 0 {
        return None;
    }
    let s = n.signum();
    core.counter.y -= n as f32;
    while n != 0 {
        let p = w.core(id).pos;
        let at = Vec2::new(p.x, p.y + s as f32);
        let mut hit = solid_at(w, c, id, at);
        if hit.is_none() && s > 0 && !w.core(id).ignore_jump_thrus {
            hit = w.first_outside(c, id, at, Tag::JumpThru);
        }
        if let Some(hit) = hit {
            w.core_mut(id).counter.y = 0.0;
            return Some(hit);
        }
        w.core_mut(id).pos.y += s as f32;
        n -= s;
    }
    None
}

/// Callback for [`move_exact_h`]/[`move_exact_v`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExactCollide {
    None,
    Squish(SquishDir),
    /// `JumpThruRiseCollide` (a rising jump-thru pushes a rider into a solid).
    JumpThruRise(EntityId),
}

fn exact_collide(w: &mut World, c: &Content, id: EntityId, cb: ExactCollide, hit: EntityId) {
    match cb {
        ExactCollide::None => {}
        ExactCollide::Squish(d) => on_squish(w, c, id, d, hit),
        ExactCollide::JumpThruRise(jt) => jump_thru_rise_collide(w, c, id, jt),
    }
}

/// `MoveExactH(n, onCollide)`.
pub fn move_exact_h(w: &mut World, c: &Content, id: EntityId, n: i32, cb: ExactCollide) -> bool {
    let s = n.signum();
    let mut left = n;
    while left != 0 {
        let p = w.core(id).pos;
        if let Some(hit) = solid_at(w, c, id, Vec2::new(p.x + s as f32, p.y)) {
            exact_collide(w, c, id, cb, hit);
            if w.exists(id) && w.core(id).finish_push_on_squish {
                w.core_mut(id).pos.x += left as f32;
            }
            return true;
        }
        w.core_mut(id).pos.x += s as f32;
        left -= s;
    }
    false
}

/// `MoveExactV(n, onCollide)`.
pub fn move_exact_v(w: &mut World, c: &Content, id: EntityId, n: i32, cb: ExactCollide) -> bool {
    let s = n.signum();
    let mut left = n;
    while left != 0 {
        let p = w.core(id).pos;
        let at = Vec2::new(p.x, p.y + s as f32);
        let mut hit = solid_at(w, c, id, at);
        if hit.is_none() && s > 0 && !w.core(id).ignore_jump_thrus {
            hit = w.first_outside(c, id, at, Tag::JumpThru);
        }
        if let Some(hit) = hit {
            exact_collide(w, c, id, cb, hit);
            if w.exists(id) && w.core(id).finish_push_on_squish {
                w.core_mut(id).pos.y += left as f32;
            }
            return true;
        }
        w.core_mut(id).pos.y += s as f32;
        left -= s;
    }
    false
}

/// `MoveIgnoreSolids(v)`.
pub fn move_ignore_solids(w: &mut World, id: EntityId, v: Vec2) {
    let core = w.core_mut(id);
    core.counter += v;
    let dx = round_he_i(core.counter.x);
    let dy = round_he_i(core.counter.y);
    core.counter.x -= dx as f32;
    core.counter.y -= dy as f32;
    core.pos.x += dx as f32;
    core.pos.y += dy as f32;
}

/// `MoveTowardsY(target, max)` (scalar approach on the actual position).
pub fn move_towards_y(
    w: &mut World,
    c: &Content,
    id: EntityId,
    target: f32,
    max_move: f32,
) -> Option<EntityId> {
    let core = w.core(id);
    let actual = core.pos.y + core.counter.y;
    let ny = approach(actual, target, max_move);
    move_v(w, c, id, ny - actual)
}

/// `CheckBelow()`.
pub fn check_below(w: &World, c: &Content, id: EntityId) -> bool {
    check_below_x(w, c, id, 0.0)
}

/// `CheckBelow(addX)`.
pub fn check_below_x(w: &World, c: &Content, id: EntityId, add_x: f32) -> bool {
    let core = w.core(id);
    let at = Vec2::new(core.pos.x + add_x, core.pos.y + 1.0);
    w.check_at(c, id, at, Tag::Solid)
        || (!core.ignore_jump_thrus && w.first_outside(c, id, at, Tag::JumpThru).is_some())
}

/// `GetBelow()`.
pub fn get_below(w: &World, c: &Content, id: EntityId) -> Option<EntityId> {
    let core = w.core(id);
    let at = Vec2::new(core.pos.x, core.pos.y + 1.0);
    w.first_at(c, id, at, Tag::Solid).or_else(|| {
        if core.ignore_jump_thrus {
            None
        } else {
            w.first_outside(c, id, at, Tag::JumpThru)
        }
    })
}

/// Base `IsRiding(Solid)`: overlaps the solid one pixel down.
pub fn base_riding_solid(w: &World, c: &Content, id: EntityId, solid: EntityId) -> bool {
    let core = w.core(id);
    let p = Placed {
        pos: Vec2::new(core.pos.x, core.pos.y + 1.0),
        collider: core.collider,
    };
    w.check(c, &p, solid)
}

/// Base `IsRiding(JumpThru)`.
pub fn base_riding_jump_thru(w: &World, c: &Content, id: EntityId, jt: EntityId) -> bool {
    let core = w.core(id);
    if core.ignore_jump_thrus {
        return false;
    }
    let here = Placed {
        pos: core.pos,
        collider: core.collider,
    };
    let below = Placed {
        pos: Vec2::new(core.pos.x, core.pos.y + 1.0),
        ..here
    };
    w.check(c, &below, jt) && !w.check(c, &here, jt)
}

/// Dispatch `IsRiding(Solid)` by entity type.
pub fn is_riding_solid(w: &World, c: &Content, id: EntityId, solid: EntityId) -> bool {
    match w.kind(id) {
        Some(Kind::Player(p)) => crate::player::is_riding_solid(w, c, id, p, solid),
        Some(Kind::Arrow(a)) => crate::arrow::is_riding_solid(w, c, id, a, solid),
        Some(Kind::Corpse(cp)) => crate::corpse::is_riding_solid(w, c, id, cp, solid),
        Some(Kind::Enemy(e)) if e.ty == crate::enemies::EnemyType::Worm => {
            crate::enemies::worm::is_riding_solid(w, c, id, e, solid)
        }
        Some(_) => base_riding_solid(w, c, id, solid),
        None => false,
    }
}

/// Dispatch `IsRiding(JumpThru)` by entity type.
pub fn is_riding_jump_thru(w: &World, c: &Content, id: EntityId, jt: EntityId) -> bool {
    match w.kind(id) {
        Some(Kind::Player(p)) => {
            !p.state_is_frozen() && p.speed.y >= 0.0 && base_riding_jump_thru(w, c, id, jt)
        }
        Some(Kind::Arrow(a)) => crate::arrow::is_riding_jump_thru(w, c, id, a, jt),
        Some(Kind::Corpse(cp)) => {
            !cp.is_squished() && cp.speed.y >= 0.0 && base_riding_jump_thru(w, c, id, jt)
        }
        Some(_) => base_riding_jump_thru(w, c, id, jt),
        None => false,
    }
}

/// `FinishPushOnSquishRiding` (arrows: stuck).
pub fn finish_push_on_squish_riding(w: &World, id: EntityId) -> bool {
    matches!(w.kind(id), Some(Kind::Arrow(a)) if a.state == crate::arrow::ArrowState::Stuck)
}

/// `OnSquish{Left,Right,Up,Down}(platform)` by entity type (default: remove).
pub fn on_squish(w: &mut World, c: &Content, id: EntityId, dir: SquishDir, platform: EntityId) {
    let Some(kind) = w.take_kind(id) else { return };
    let kind = match kind {
        Kind::Player(mut p) => {
            crate::player::do_squish(w, c, id, &mut p, dir, platform);
            Kind::Player(p)
        }
        Kind::Arrow(mut a) => {
            crate::arrow::on_squish(w, id, &mut a);
            Kind::Arrow(a)
        }
        Kind::Corpse(mut cp) => {
            crate::corpse::on_squish(w, id, &mut cp, dir);
            Kind::Corpse(cp)
        }
        Kind::Enemy(mut e) => {
            crate::enemies::on_squish(w, c, id, &mut e);
            Kind::Enemy(e)
        }
        k => {
            w.remove(id);
            k
        }
    };
    w.put_kind(id, kind);
}

/// `JumpThruRiseCollide`: snap the feet to the platform top, then try
/// sideways offsets -2, +2, -4, +4 for a free spot.
pub fn jump_thru_rise_collide(w: &mut World, c: &Content, id: EntityId, jt: EntityId) {
    let p = w.core(id).pos;
    let top = w.core(jt).top();
    w.core_mut(id).set_bottom(top);
    let base = w.core(id).pos;
    for i in 1..=2 {
        for j in [-1.0f32, 1.0] {
            let x = p.x + i as f32 * j * 2.0;
            if !w.check_at(c, id, Vec2::new(x, base.y), Tag::Solid) {
                w.core_mut(id).pos.x = x;
                return;
            }
        }
    }
    w.core_mut(id).pos = p;
}

/// `LevelEntity.EnforceScreenWrap`: returns how many axes wrapped.
pub fn enforce_screen_wrap(w: &mut World, c: &Content, id: EntityId) -> u32 {
    let ws = w.world_size(c);
    let core = w.core_mut(id);
    let mut n = 0;
    if core.pos.x >= ws.w {
        core.pos.x -= ws.w;
        n += 1;
    } else if core.pos.x < 0.0 {
        core.pos.x += ws.w;
        n += 1;
    }
    if core.pos.y >= ws.h {
        core.pos.y -= ws.h;
        n += 1;
    } else if core.pos.y < 0.0 {
        core.pos.y += ws.h;
        n += 1;
    }
    n
}

/// `WrapMath.DiffX` (shortest signed displacement through the wrap).
pub fn wrap_diff_x(from: f32, to: f32, width: f32) -> f32 {
    let alt = if from < to {
        -(width - to + from)
    } else {
        width - from + to
    };
    if alt.abs() < (to - from).abs() {
        alt
    } else {
        to - from
    }
}

/// `WrapMath.WrapDistanceSquared`.
pub fn wrap_dist_sq(a: Vec2, b: Vec2, w: f32, h: f32) -> f32 {
    let horiz = if a.x + (w - b.x) < (b.x - a.x).abs() {
        a.dist_sq(b - Vec2::new(w, 0.0))
    } else if b.x + (w - a.x) < (b.x - a.x).abs() {
        (a - Vec2::new(w, 0.0)).dist_sq(b)
    } else {
        a.dist_sq(b)
    };
    let vert = if a.y + (h - b.y) < (b.y - a.y).abs() {
        a.dist_sq(b - Vec2::new(0.0, h))
    } else if b.y + (h - a.y) < (b.y - a.y).abs() {
        (a - Vec2::new(0.0, h)).dist_sq(b)
    } else {
        a.dist_sq(b)
    };
    crate::math::min(horiz, vert)
}
