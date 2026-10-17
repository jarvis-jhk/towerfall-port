//! `FloorMiasma` (tower-entities.md §8.4, quest.md §4): Quest floor strips
//! that a wave switches on by group. A strip grows in, then an archer
//! landing on it arms it; 48 ticks later it flares for 20 ticks, killing
//! archers that step on it and pushing enemies up.

use crate::collide::{Collider, Hitbox};
use crate::content::Content;
use crate::entity::{Core, EntityId, Kind, Tag};
use crate::impl_hash;
use crate::math::{Vec2, approach};
use crate::player::{self, DeathCause, Player};
use crate::world::World;

pub const INVISIBLE: u8 = 0;
pub const START_UP: u8 = 1;
pub const IDLE: u8 = 2;
pub const ACTIVATING: u8 = 3;
pub const DANGEROUS: u8 = 4;
pub const DIE_OUT: u8 = 5;

/// The intro / arming animation: 10 frames of 0.08 s.
pub const ANIM_TICKS: f32 = 48.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloorMiasma {
    pub group: i32,
    pub state: u8,
    /// Sprite Y scale (presentation, but arming needs it settled at 0.5).
    pub scale: f32,
    /// Ticks into the running animation / flare.
    pub t: f32,
    pub light: f32,
}
impl_hash!(FloorMiasma {
    group,
    state,
    scale,
    t,
    light
});

/// One strip per `FloorMiasma` marker (invisible, harmless).
pub fn spawn_all(w: &mut World, c: &Content) {
    for f in c.level(w.level).markers.floor_miasma.clone() {
        let mut core = Core::new(
            f.pos,
            Collider::Box(Hitbox::new(f.width, 3.0, 0.0, 2.0)),
            -110,
            &[Tag::PlayerCollider, Tag::LightSource],
        );
        core.collidable = false;
        core.visible = false;
        w.add(
            core,
            Kind::FloorMiasma(FloorMiasma {
                group: f.group,
                state: INVISIBLE,
                scale: 0.0,
                t: 0.0,
                light: 0.0,
            }),
        );
    }
}

fn strips(w: &World) -> Vec<EntityId> {
    w.entities()
        .filter(|&e| matches!(w.kind(e), Some(Kind::FloorMiasma(_))))
        .collect()
}

/// `StartUp` for every strip of the listed groups.
pub fn start_groups(w: &mut World, groups: &[i32]) {
    for e in strips(w) {
        let core_visible = w.core(e).visible;
        if let Some(Kind::FloorMiasma(f)) = w.kind_mut(e)
            && groups.contains(&f.group)
            && !core_visible
        {
            f.state = START_UP;
            f.scale = 0.0;
            f.t = 0.0;
            w.core_mut(e).visible = true;
        }
    }
}

/// `DieOut` for every visible strip (wave end).
pub fn die_out_all(w: &mut World) {
    for e in strips(w) {
        if !w.core(e).visible {
            continue;
        }
        if let Some(Kind::FloorMiasma(f)) = w.kind_mut(e) {
            f.state = DIE_OUT;
            f.t = 0.0;
        }
        w.core_mut(e).collidable = false;
    }
}

pub fn update(w: &mut World, c: &Content, id: EntityId, f: &mut FloorMiasma) {
    let tm = w.time.time_mult;
    let target = if f.state == DANGEROUS { 1.0 } else { 0.5 };
    f.scale = approach(f.scale, target, 0.02 * tm);
    let lit = matches!(f.state, START_UP | IDLE | ACTIVATING | DANGEROUS);
    f.light = approach(f.light, if lit { 1.0 } else { 0.0 }, 0.05 * tm);
    f.t += tm;
    match f.state {
        START_UP if f.t >= ANIM_TICKS => {
            f.state = IDLE;
            w.core_mut(id).collidable = true;
        }
        ACTIVATING if f.t >= ANIM_TICKS => {
            f.state = DANGEROUS;
            f.t = 0.0;
            f.scale = 2.0;
            let at = w.core(id).pos.x;
            w.sound("env_floorFireFlare", at);
            let me = w.placed(id);
            for e in w.all(c, &me, Tag::Enemy, id) {
                crate::enemies::on_explode(w, c, e, Vec2::new(0.0, -0.6), -1, false);
            }
        }
        DANGEROUS if f.t >= 21.0 => f.state = IDLE,
        DIE_OUT if f.t >= ANIM_TICKS => {
            f.state = INVISIBLE;
            w.core_mut(id).visible = false;
        }
        _ => {}
    }
}

/// An archer touches the strip (only when moving down or level).
pub fn on_player_collide(w: &mut World, c: &Content, id: EntityId, pid: EntityId, p: &mut Player) {
    if p.speed.y < 0.0 {
        return;
    }
    let Some(Kind::FloorMiasma(mut f)) = w.kind(id).copied() else {
        return;
    };
    if f.state == IDLE && f.scale == 0.5 {
        f.state = ACTIVATING;
        f.t = 0.0;
        f.scale = 1.0;
        let x = w.core(id).pos.x + w.core(id).hitbox().w / 2.0;
        w.sound("env_floorFireBegin", x);
        if let Some(Kind::FloorMiasma(slot)) = w.kind_mut(id) {
            *slot = f;
        }
    } else if f.state == DANGEROUS {
        let at = w.core(pid).pos + Vec2::new(0.0, 1.0);
        player::hurt(w, c, pid, p, DeathCause::Miasma, at, -1, None);
    }
}
