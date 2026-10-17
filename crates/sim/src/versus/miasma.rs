//! The Versus miasma (versus.md §9): two walls closing in from the screen
//! edges. Each side is its own entity (a box `width x 240`).

use crate::collide::{Collider, Hitbox};
use crate::components::Ease;
use crate::content::Content;
use crate::entity::{Core, EntityId, Kind, Tag};
use crate::impl_hash;
use crate::math::{Vec2, floor, lerp, max};
use crate::player::{self, DeathCause, Player};
use crate::world::World;

pub const DEPTH: i32 = -600;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Miasma {
    pub left: bool,
    /// Ticks since it spawned (`TimeMult`-scaled).
    pub t: f32,
    pub percent: f32,
    pub dissipating: bool,
    pub dis_from: f32,
    pub dis_dur: f32,
    pub dis_t: f32,
    pub remove_in: f32,
    /// `Modes`: Versus, Amaranth boss, Cataclysm boss, or the Dreadwood
    /// boss's bottom miasma (darkworld.md §7).
    pub mode: MiasmaMode,
    /// `SideWeight` (Amaranth: the cover sways between the sides).
    pub side: f32,
    pub dir: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MiasmaMode {
    #[default]
    Versus,
    Amaranth,
    Cataclysm,
    /// `BottomMiasma` (one entity, rising from the floor).
    Bottom,
}
crate::impl_hash_enum!(MiasmaMode);

impl_hash!(Miasma {
    left,
    t,
    percent,
    dissipating,
    dis_from,
    dis_dur,
    dis_t,
    remove_in,
    mode,
    side
});

/// Percent and side weight of a boss-mode miasma at `t` ticks.
pub fn boss_timeline(mode: MiasmaMode, t: f32, dir: f32) -> (f32, f32) {
    let cube = |a: f32, b: f32, x: f32, e: Ease| lerp(a, b, e.apply(x.clamp(0.0, 1.0)));
    match mode {
        MiasmaMode::Amaranth => {
            let p = cube(-0.1, 0.25, t / 180.0, Ease::CubeOut);
            let side = if t < 180.0 {
                0.0
            } else {
                let k = (t - 180.0) % 960.0;
                if k < 240.0 {
                    cube(0.0, 0.25 * dir, k / 240.0, Ease::CubeOut)
                } else if k < 720.0 {
                    cube(
                        0.25 * dir,
                        -0.25 * dir,
                        (k - 240.0) / 480.0,
                        Ease::CubeInOut,
                    )
                } else {
                    cube(-0.25 * dir, 0.0, (k - 720.0) / 240.0, Ease::CubeIn)
                }
            };
            (p, side)
        }
        MiasmaMode::Cataclysm => (cube(-0.1, 0.2, t / 180.0, Ease::CubeOut), 0.0),
        MiasmaMode::Bottom => {
            if t < 180.0 {
                (cube(-0.1, 0.0, t / 180.0, Ease::CubeOut), 0.0)
            } else {
                let k = (t - 180.0) % 600.0;
                let p = if k < 240.0 {
                    cube(0.0, 0.28, k / 240.0, Ease::CubeInOut)
                } else if k < 480.0 {
                    cube(0.28, 0.0, (k - 240.0) / 240.0, Ease::CubeInOut)
                } else {
                    0.0
                };
                (p, 0.0)
            }
        }
        MiasmaMode::Versus => (percent_at(t), 0.0),
    }
}

/// A boss-mode miasma (both sides; the bottom one is a single entity,
/// returned twice).
pub fn spawn_mode(w: &mut World, mode: MiasmaMode) -> [EntityId; 2] {
    let dir = if w.rng.next_max(2) == 0 { 1.0 } else { -1.0 };
    if mode == MiasmaMode::Bottom {
        let mut core = Core::new(
            Vec2::new(0.0, 240.0),
            Collider::Box(Hitbox::new(320.0, 0.0, 0.0, 0.0)),
            DEPTH,
            &[Tag::PlayerCollider, Tag::LightSource],
        );
        core.collidable = false;
        let id = w.add(core, Kind::Miasma(blank(true, mode, dir)));
        return [id, id];
    }
    let ids = spawn(w);
    for id in ids {
        if let Some(Kind::Miasma(m)) = w.kind_mut(id) {
            m.mode = mode;
            m.dir = dir;
        }
    }
    ids
}

fn blank(left: bool, mode: MiasmaMode, dir: f32) -> Miasma {
    Miasma {
        left,
        t: 0.0,
        percent: -0.1,
        dissipating: false,
        dis_from: 0.0,
        dis_dur: 0.0,
        dis_t: 0.0,
        remove_in: 0.0,
        mode,
        side: 0.0,
        dir,
    }
}

/// `P(t)` of the timeline.
pub fn percent_at(t: f32) -> f32 {
    if t <= 540.0 {
        -0.1 + 0.43 * Ease::CubeOut.apply(t / 540.0)
    } else if t <= 720.0 {
        0.33 - 0.18 * Ease::CubeOut.apply((t - 540.0) / 180.0)
    } else if t <= 1380.0 {
        0.15 + 0.90 * Ease::CubeInOut.apply((t - 720.0) / 660.0)
    } else if t <= 1620.0 {
        1.05 - 1.15 * Ease::CubeIn.apply((t - 1380.0) / 240.0)
    } else {
        -0.1
    }
}

/// Spawn both sides (non-collidable at first).
pub fn spawn(w: &mut World) -> [EntityId; 2] {
    let mut ids = [EntityId {
        index: 0,
        generation: 0,
    }; 2];
    for (k, left) in [true, false].into_iter().enumerate() {
        let mut core = Core::new(
            Vec2::ZERO,
            Collider::Box(Hitbox::new(0.0, 240.0, 0.0, 0.0)),
            DEPTH,
            &[Tag::PlayerCollider, Tag::LightSource],
        );
        core.collidable = false;
        ids[k] = w.add(core, Kind::Miasma(blank(left, MiasmaMode::Versus, 1.0)));
    }
    ids
}

pub fn update(w: &mut World, c: &Content, id: EntityId, m: &mut Miasma) {
    let tm = w.time.time_mult;
    let ww = w.world_size(c).w;
    m.t += tm;
    if m.dissipating {
        m.dis_t += tm;
        let k = (m.dis_t / m.dis_dur).min(1.0);
        m.percent = lerp(m.dis_from, -0.1, Ease::CubeInOut.apply(k));
        m.side = lerp(m.side, 0.0, Ease::CubeInOut.apply(k));
        m.remove_in -= tm;
        if m.remove_in <= 0.0 {
            w.remove(id);
        }
    } else {
        let (p, side) = boss_timeline(m.mode, m.t, m.dir);
        m.percent = p;
        m.side = side;
        if m.t >= 120.0 {
            w.core_mut(id).collidable = true;
        }
    }
    if m.mode == MiasmaMode::Bottom {
        let h = max(0.0, m.percent * 120.0 - 5.0);
        let core = w.core_mut(id);
        core.collider = Collider::Box(Hitbox::new(ww, h, 0.0, 0.0));
        core.pos = Vec2::new(0.0, 240.0 - h);
        return;
    }
    let weight = if m.left { m.side } else { -m.side };
    let width = max(0.0, (m.percent + weight) * (ww / 2.0) - 5.0);
    let core = w.core_mut(id);
    core.collider = Collider::Box(Hitbox::new(width, 240.0, 0.0, 0.0));
    core.pos = Vec2::new(if m.left { 0.0 } else { ww - width }, 0.0);
}

/// `Miasma.Dissipate()` for both sides.
pub fn dissipate(w: &mut World, ids: [EntityId; 2]) {
    for id in ids {
        if !w.exists(id) {
            continue;
        }
        w.core_mut(id).collidable = false;
        if let Some(Kind::Miasma(m)) = w.kind_mut(id)
            && !m.dissipating
        {
            m.dissipating = true;
            m.dis_from = m.percent;
            m.dis_dur = max(20.0, floor(60.0 * m.percent));
            m.dis_t = 0.0;
            m.remove_in = 100.0;
        }
    }
}

/// Ticks since spawn of the left side (for "nerves of steel").
pub fn age(w: &World, ids: [EntityId; 2]) -> Option<f32> {
    match w.kind(ids[0]) {
        Some(Kind::Miasma(m)) => Some(m.t),
        _ => None,
    }
}

/// `OnPlayerCollide`: hurt the archer away from the wall.
pub fn on_player_collide(
    w: &mut World,
    c: &Content,
    _mid: EntityId,
    pid: EntityId,
    p: &mut Player,
) {
    let ww = w.world_size(c).w;
    let pos = w.core(pid).pos;
    if let Some(Kind::Miasma(m)) = w.kind(_mid)
        && m.mode == MiasmaMode::Bottom
    {
        if crate::player::can_hurt(w, pid, p) {
            player::hurt(
                w,
                c,
                pid,
                p,
                DeathCause::Miasma,
                pos + Vec2::new(0.0, 20.0),
                -1,
                None,
            );
        }
        return;
    }
    let from = if pos.x <= ww / 2.0 {
        pos - Vec2::new(1.0, 0.0)
    } else {
        pos + Vec2::new(1.0, 0.0)
    };
    player::hurt(w, c, pid, p, DeathCause::Miasma, from, -1, None);
}
