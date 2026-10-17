//! `Solid.Move` / `JumpThru.Move` (engine.md §8.4–8.5): moving platforms
//! push actors out of the way and carry their riders. Plus static
//! jump-thrus from the level file and a simple moving solid used by tests
//! and sandboxes until the tower-entity spec brings the real platforms.

use crate::actor::{self, ExactCollide, SquishDir};
use crate::collide::{Collider, Hitbox, Placed};
use crate::content::Content;
use crate::entity::{Core, EntityId, Kind, Tag, depth};
use crate::impl_hash;
use crate::math::{Vec2, approach_vec, round_he_i};
use crate::world::World;

/// Add a static jump-thru (hitbox `width x 5`, depth -150).
pub fn add_jump_thru(w: &mut World, pos: Vec2, width: f32) -> EntityId {
    let core = Core::new(
        pos,
        Collider::Box(Hitbox::new(width, 5.0, 0.0, 0.0)),
        depth::SOLID,
        &[Tag::JumpThru],
    );
    let mut core = core;
    core.active = false;
    w.add(core, Kind::JumpThru)
}

/// Add a static solid block (depth -150, never updates).
pub fn add_static_solid(
    w: &mut World,
    pos: Vec2,
    width: f32,
    height: f32,
    jump_pad: bool,
) -> EntityId {
    let tags: &[Tag] = if jump_pad {
        &[Tag::Solid, Tag::JumpPad, Tag::LightSource]
    } else {
        &[Tag::Solid, Tag::ExplosionCollider]
    };
    let mut core = Core::new(
        pos,
        Collider::Box(Hitbox::new(width, height, 0.0, 0.0)),
        depth::SOLID,
        tags,
    );
    core.active = false;
    w.add(core, Kind::StaticSolid)
}

/// A solid that shuttles between two points at a fixed speed (a stand-in
/// for `MovingPlatform` with linear motion), or a jump-thru doing the same.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovingSolid {
    pub a: Vec2,
    pub b: Vec2,
    pub speed: f32,
    pub to_b: bool,
    pub jump_thru: bool,
    /// Frames to wait at each end.
    pub pause: f32,
    pub wait: f32,
}
impl_hash!(MovingSolid {
    a,
    b,
    speed,
    to_b,
    jump_thru,
    pause,
    wait
});

/// Add a moving solid (or jump-thru) of `w x h` starting at `a`.
pub fn add_moving(
    world: &mut World,
    a: Vec2,
    b: Vec2,
    w: f32,
    h: f32,
    speed: f32,
    jump_thru: bool,
) -> EntityId {
    let tag = if jump_thru { Tag::JumpThru } else { Tag::Solid };
    let core = Core::new(
        a,
        Collider::Box(Hitbox::new(w, h, 0.0, 0.0)),
        depth::SOLID,
        &[tag],
    );
    world.add(
        core,
        Kind::MovingSolid(MovingSolid {
            a,
            b,
            speed,
            to_b: true,
            jump_thru,
            pause: 0.0,
            wait: 0.0,
        }),
    )
}

pub fn update_moving(w: &mut World, c: &Content, id: EntityId, m: &mut MovingSolid) {
    let tm = w.time.time_mult;
    if m.wait > 0.0 {
        m.wait -= tm;
        return;
    }
    let core = w.core(id);
    let actual = core.pos + core.counter;
    let target = if m.to_b { m.b } else { m.a };
    let next = approach_vec(actual, target, m.speed * tm);
    let amount = next - actual;
    if m.jump_thru {
        jump_thru_move(w, c, id, amount);
    } else {
        solid_move(w, c, id, amount);
    }
    if next == target {
        m.to_b = !m.to_b;
        m.wait = m.pause;
    }
}

/// `Solid.Move(amount)`.
pub fn solid_move(w: &mut World, c: &Content, id: EntityId, amount: Vec2) {
    let core = w.core_mut(id);
    core.counter += amount;
    let dx = round_he_i(core.counter.x);
    let dy = round_he_i(core.counter.y);
    if dx == 0 && dy == 0 {
        return;
    }
    w.mover = Some(id);
    // Riding set before the move (tag-list order).
    let actors: Vec<EntityId> = w.tagged(Tag::Actor).to_vec();
    let mut riding: Vec<EntityId> = vec![];
    for &a in &actors {
        if actor::is_riding_solid(w, c, a, id) {
            riding.push(a);
            if actor::finish_push_on_squish_riding(w, a) {
                w.core_mut(a).finish_push_on_squish = true;
            }
        }
    }
    w.core_mut(id).collidable = false;
    if dx != 0 {
        let core = w.core_mut(id);
        core.counter.x -= dx as f32;
        core.pos.x += dx as f32;
        for &a in &actors {
            if !w.exists(a) {
                continue;
            }
            let solid = w.placed(id);
            let ac = w.core(a);
            if ac.pushable && ac.collidable && overlaps(w, c, &solid, a) {
                if ac.naive_push {
                    w.core_mut(a).pos.x += dx as f32;
                } else if dx > 0 {
                    let n = (w.core(id).right() - w.core(a).left()) as i32;
                    actor::move_exact_h(w, c, a, n, ExactCollide::Squish(SquishDir::Right));
                } else {
                    let n = (w.core(id).left() - w.core(a).right()) as i32;
                    actor::move_exact_h(w, c, a, n, ExactCollide::Squish(SquishDir::Left));
                }
            } else if riding.contains(&a) {
                actor::move_exact_h(w, c, a, dx, ExactCollide::None);
            }
        }
    }
    if dy != 0 {
        let core = w.core_mut(id);
        core.counter.y -= dy as f32;
        core.pos.y += dy as f32;
        for &a in &actors {
            if !w.exists(a) {
                continue;
            }
            let solid = w.placed(id);
            let ac = w.core(a);
            if ac.pushable && ac.collidable && overlaps(w, c, &solid, a) {
                if ac.naive_push {
                    w.core_mut(a).pos.y += dy as f32;
                } else if dy > 0 {
                    let n = (w.core(id).bottom() - w.core(a).top()) as i32;
                    actor::move_exact_v(w, c, a, n, ExactCollide::Squish(SquishDir::Down));
                } else {
                    let n = (w.core(id).top() - w.core(a).bottom()) as i32;
                    actor::move_exact_v(w, c, a, n, ExactCollide::Squish(SquishDir::Up));
                }
            } else if riding.contains(&a) {
                actor::move_exact_v(w, c, a, dy, ExactCollide::None);
            }
        }
    }
    for &a in &riding {
        if w.exists(a) && actor::finish_push_on_squish_riding(w, a) {
            w.core_mut(a).finish_push_on_squish = false;
        }
    }
    w.core_mut(id).collidable = true;
    w.mover = None;
}

/// The solid's collider (receiver) vs the actor; the actor must be collidable.
fn overlaps(w: &World, c: &Content, solid: &Placed, a: EntityId) -> bool {
    w.check(c, solid, a)
}

/// `JumpThru.Move(amount)`: carries riders only; never pushes.
pub fn jump_thru_move(w: &mut World, c: &Content, id: EntityId, amount: Vec2) {
    let core = w.core_mut(id);
    core.counter += amount;
    let dx = round_he_i(core.counter.x);
    let dy = round_he_i(core.counter.y);
    if dx == 0 && dy == 0 {
        return;
    }
    w.mover = Some(id);
    let riding: Vec<EntityId> = w
        .tagged(Tag::Actor)
        .iter()
        .copied()
        .filter(|&a| actor::is_riding_jump_thru(w, c, a, id))
        .collect();
    w.core_mut(id).collidable = false;
    if dx != 0 {
        let core = w.core_mut(id);
        core.counter.x -= dx as f32;
        core.pos.x += dx as f32;
        for &a in &riding {
            actor::move_exact_h(w, c, a, dx, ExactCollide::None);
        }
    }
    if dy != 0 {
        let core = w.core_mut(id);
        core.counter.y -= dy as f32;
        core.pos.y += dy as f32;
        for &a in &riding {
            let cb = if dy < 0 {
                ExactCollide::JumpThruRise(id)
            } else {
                ExactCollide::None
            };
            actor::move_exact_v(w, c, a, dy, cb);
        }
    }
    w.core_mut(id).collidable = true;
    w.mover = None;
}
