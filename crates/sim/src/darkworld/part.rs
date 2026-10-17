//! `Kind::Dw` entities: the exit portal (`DarkWorldPortal`, §5.5), boss
//! bodies and parts, boss projectiles and the Cataclysm blocks. Each is a
//! small `Copy` value; boss-wide state lives in `DarkWorld::boss`.

use super::{amaranth, boss, cataclysm, cyclops, dreadwood};
use crate::arrow::Arrow;
use crate::checksum::{Hasher, StableHash};
use crate::collide::{Collider, Hitbox};
use crate::content::Content;
use crate::entity::{Core, EntityId, Kind, Tag};
use crate::math::Vec2;
use crate::player::Player;
use crate::world::World;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    /// The exit portal; `t` = ticks since it appeared.
    ExitPortal {
        t: f32,
    },
    /// The boss itself (state in `DarkWorld::boss`).
    Body,
    // ---- The Amaranth
    AmaShot {
        t: f32,
        heading: f32,
        grace: f32,
    },
    /// One-sided beam from the boss along `angle`; deadly from 60 ticks.
    AmaBeam {
        t: f32,
        angle: f32,
    },
    // ---- Dreadwood
    /// Eye `i` (state in the boss).
    Eye(u8),
    /// Floor tentacle `i` (state in the boss).
    Tentacle(u8),
    Spore {
        t: f32,
        phase: f32,
        free: bool,
    },
    // ---- Darkfang
    /// Left (0) / right (1) fist, a moving solid.
    Fist(u8),
    /// The rising pedestal, a moving solid.
    Pedestal,
    CycShot {
        t: f32,
        heading: f32,
    },
    /// The floor bones hazard strip.
    Bones,
    // ---- Cataclysm
    Bullet {
        t: f32,
        vel: Vec2,
    },
    Missile {
        t: f32,
        /// The art's angle; `normal` is the travel direction (it drifts
        /// from `heading` when a wall slide zeroes one component).
        heading: f32,
        speed: f32,
        normal: Vec2,
    },
    Blade {
        t: f32,
        from: Vec2,
        to: Vec2,
        dir: f32,
        pause: f32,
        spin: f32,
    },
    ShieldOrb {
        t: f32,
        home: Vec2,
    },
    /// `CataclysmBlock` (tag `PrismBlock`): bobbing solid.
    Block {
        start: Vec2,
        node: Vec2,
        c: f32,
        sink: f32,
        ride: f32,
        shake: f32,
    },
}

impl StableHash for Part {
    fn stable_hash(&self, h: &mut Hasher) {
        match *self {
            Part::ExitPortal { t } => {
                h.u8(0);
                h.f32(t);
            }
            Part::Body => h.u8(1),
            Part::AmaShot { t, heading, grace } => {
                h.u8(2);
                h.f32(t);
                h.f32(heading);
                h.f32(grace);
            }
            Part::AmaBeam { t, angle } => {
                h.u8(3);
                h.f32(t);
                h.f32(angle);
            }
            Part::Eye(i) => {
                h.u8(4);
                h.u8(i);
            }
            Part::Tentacle(i) => {
                h.u8(5);
                h.u8(i);
            }
            Part::Spore { t, phase, free } => {
                h.u8(6);
                h.f32(t);
                h.f32(phase);
                h.bool(free);
            }
            Part::Fist(i) => {
                h.u8(7);
                h.u8(i);
            }
            Part::Pedestal => h.u8(8),
            Part::CycShot { t, heading } => {
                h.u8(9);
                h.f32(t);
                h.f32(heading);
            }
            Part::Bones => h.u8(15),
            Part::Bullet { t, vel } => {
                h.u8(10);
                h.f32(t);
                h.vec2(vel);
            }
            Part::Missile {
                t,
                heading,
                speed,
                normal,
            } => {
                h.u8(11);
                h.f32(t);
                h.f32(heading);
                h.f32(speed);
                h.vec2(normal);
            }
            Part::Blade {
                t,
                from,
                to,
                dir,
                pause,
                spin,
            } => {
                h.u8(12);
                h.f32(t);
                h.vec2(from);
                h.vec2(to);
                h.f32(dir);
                h.f32(pause);
                h.f32(spin);
            }
            Part::ShieldOrb { t, home } => {
                h.u8(13);
                h.f32(t);
                h.vec2(home);
            }
            Part::Block {
                start,
                node,
                c,
                sink,
                ride,
                shake,
            } => {
                h.u8(14);
                h.vec2(start);
                h.vec2(node);
                h.f32(c);
                h.f32(sink);
                h.f32(ride);
                h.f32(shake);
            }
        }
    }
}

/// Add a part entity.
pub fn add(w: &mut World, pos: Vec2, hb: Hitbox, depth: i32, tags: &[Tag], part: Part) -> EntityId {
    let mut core = Core::new(pos, Collider::Box(hb), depth, tags);
    core.screen_wrap = false;
    w.add(core, Kind::Dw(part))
}

pub fn get(w: &World, id: EntityId) -> Option<Part> {
    match w.kind(id) {
        Some(Kind::Dw(p)) => Some(*p),
        _ => None,
    }
}

pub fn set(w: &mut World, id: EntityId, part: Part) {
    if let Some(Kind::Dw(p)) = w.kind_mut(id) {
        *p = part;
    }
}

// ----------------------------------------------------------- exit portal

/// `DarkWorldPortal` at the arena's `EndlessPortal` marker.
pub fn spawn_exit_portal(w: &mut World, at: Vec2) -> EntityId {
    let id = add(
        w,
        at,
        Hitbox::wrapping(16.0, 16.0, -8.0, -8.0),
        -50,
        &[Tag::PlayerCollider, Tag::LightSource],
        Part::ExitPortal { t: 0.0 },
    );
    w.core_mut(id).collidable = false;
    w.sound("sfx_darkWorldPortalAppear", at.x);
    id
}

fn exit_update(w: &mut World, id: EntityId, t: &mut f32) {
    let before = *t;
    *t += w.time.time_mult;
    if before < 60.0 && *t >= 60.0 {
        w.core_mut(id).collidable = true;
        w.sound("sfx_darkWorldGo", w.core(id).pos.x);
    }
}

// ------------------------------------------------------------- dispatch

pub fn update(w: &mut World, c: &Content, id: EntityId, p: &mut Part) {
    match p {
        Part::ExitPortal { t } => exit_update(w, id, t),
        Part::Body => boss::update_body(w, c, id),
        Part::AmaShot { .. } | Part::AmaBeam { .. } => amaranth::part_update(w, c, id, p),
        Part::Eye(_) | Part::Tentacle(_) | Part::Spore { .. } => {
            dreadwood::part_update(w, c, id, p)
        }
        Part::Fist(_) | Part::Pedestal | Part::CycShot { .. } | Part::Bones => {
            cyclops::part_update(w, c, id, p)
        }
        Part::Bullet { .. }
        | Part::Missile { .. }
        | Part::Blade { .. }
        | Part::ShieldOrb { .. }
        | Part::Block { .. } => cataclysm::part_update(w, c, id, p),
    }
}

/// A player overlaps a `PlayerCollider` part.
pub fn on_player_collide(w: &mut World, c: &Content, id: EntityId, pid: EntityId, pl: &mut Player) {
    let Some(p) = get(w, id) else { return };
    match p {
        Part::ExitPortal { .. } => {
            if !pl.dead {
                super::queue_enter(w, pid);
            }
        }
        Part::Body => match boss_id(w) {
            Some(0) => amaranth::body_touch(w, c, id, pid, pl),
            Some(1) => {}
            Some(2) => cyclops::body_touch(w, c, id, pid, pl),
            _ => cataclysm::body_touch(w, c, id, pid, pl),
        },
        Part::AmaShot { .. } => amaranth::shot_touch(w, c, id, pid, pl),
        Part::AmaBeam { .. } => {}
        Part::Eye(_) | Part::Tentacle(_) | Part::Spore { .. } => {
            dreadwood::part_touch(w, c, id, pid, pl)
        }
        Part::CycShot { .. } | Part::Bones => cyclops::shot_touch(w, c, id, pid, pl),
        Part::Fist(_) | Part::Pedestal | Part::Block { .. } => {}
        Part::Bullet { .. }
        | Part::Missile { .. }
        | Part::Blade { .. }
        | Part::ShieldOrb { .. } => cataclysm::part_touch(w, c, id, pid, pl),
    }
}

/// An arrow hits a `Target` part. Returns "absorbed".
pub fn on_arrow_hit(
    w: &mut World,
    c: &Content,
    id: EntityId,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    let Some(p) = get(w, id) else { return false };
    match p {
        Part::Body => match boss_id(w) {
            Some(0) => amaranth::body_arrow(w, c, id, aid, a),
            Some(2) => cyclops::body_arrow(w, c, id, aid, a),
            Some(3) => cataclysm::body_arrow(w, c, id, aid, a),
            _ => false,
        },
        Part::AmaShot { .. } => amaranth::shot_arrow(w, c, id, aid, a),
        Part::Eye(_) | Part::Tentacle(_) | Part::Spore { .. } => {
            dreadwood::part_arrow(w, c, id, aid, a)
        }
        Part::CycShot { .. } => cyclops::shot_arrow(w, c, id, aid, a),
        Part::Missile { .. } | Part::Blade { .. } | Part::ShieldOrb { .. } => {
            cataclysm::part_arrow(w, c, id, aid, a)
        }
        _ => false,
    }
}

/// Explosions break shots and spores; bosses decide for their bodies.
pub fn on_explode(w: &mut World, c: &Content, id: EntityId, owner: i8, lethal: bool) {
    match get(w, id) {
        Some(Part::Body) if lethal && boss_id(w) == Some(0) => amaranth::body_explode(w, id, owner),
        Some(Part::AmaShot { .. }) | Some(Part::Spore { .. }) | Some(Part::CycShot { .. }) => {
            w.remove(id);
            w.core_mut(id).collidable = false;
        }
        Some(Part::Missile { .. }) => cataclysm::missile_explode(w, c, id),
        _ => {}
    }
}

/// Shock rings break shots, spores and bullets in range.
pub fn on_shock(w: &mut World, c: &Content, me: &crate::collide::Placed, r_sq: f32) {
    let ws = w.world_size(c);
    let list: Vec<EntityId> = w
        .entities()
        .filter(|&e| {
            matches!(
                get(w, e),
                Some(
                    Part::AmaShot { .. }
                        | Part::Spore { .. }
                        | Part::CycShot { .. }
                        | Part::Bullet { .. }
                )
            )
        })
        .collect();
    for e in list {
        if w.check(c, me, e)
            && crate::actor::wrap_dist_sq(me.pos, w.core(e).pos, ws.w, ws.h) <= r_sq
        {
            w.remove(e);
            w.core_mut(e).collidable = false;
        }
    }
}

pub fn boss_id(w: &World) -> Option<u8> {
    w.dark_world.as_ref()?.boss.as_ref().map(|b| b.id)
}

/// Debug colour for the view.
pub fn debug_color(p: &Part) -> [u8; 4] {
    match p {
        Part::ExitPortal { t } => {
            if *t >= 60.0 {
                [120, 255, 120, 230]
            } else {
                [120, 255, 120, 110]
            }
        }
        Part::Body => [200, 40, 60, 255],
        Part::Eye(_) => [255, 255, 255, 255],
        Part::Tentacle(_) => [120, 40, 120, 255],
        Part::Fist(_) | Part::Pedestal | Part::Block { .. } => [140, 140, 150, 255],
        Part::ShieldOrb { .. } => [120, 200, 255, 255],
        _ => [255, 90, 60, 230],
    }
}
