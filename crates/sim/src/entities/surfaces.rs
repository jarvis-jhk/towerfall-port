//! Surfaces: ice, mud, hot coals, snow (tower-entities.md §7) and the
//! archer/corpse queries that use them (player.md §4 steps 9–12, §6.1, §14.3).

use super::{Placement, Tower, particles, sound, tm, tower_mut};
use crate::collide::{Collider, Hitbox, Placed};
use crate::content::Content;
use crate::entity::{Core, EntityId, Kind, Tag};
use crate::impl_hash;
use crate::math::{PI, Vec2, approach, clamp, floor, max};
use crate::player::Player;
use crate::world::World;

fn strip(w: &mut World, pos: Vec2, width: f32, depth: i32, tags: &[Tag], t: Tower) -> EntityId {
    let core = Core::new(
        pos,
        Collider::Box(Hitbox::new(width, 5.0, 0.0, 0.0)),
        depth,
        tags,
    );
    w.add(core, Kind::Tower(t))
}

// ------------------------------------------------------------ §7.1 Ice

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ice {
    /// Original left edge and width (art tiles keep their places).
    pub x0: f32,
    pub w0: f32,
    /// Cosmetic fade of melted tiles.
    pub melt_alpha: f32,
}
impl_hash!(Ice { x0, w0 });

pub fn spawn_ice(w: &mut World, p: &Placement) -> EntityId {
    let width = if p.size.x > 0.0 { p.size.x } else { 20.0 };
    let id = strip(
        w,
        p.pos,
        width,
        -155,
        &[Tag::Ice, Tag::LavaCollider],
        Tower::Ice(Ice {
            x0: p.pos.x,
            w0: width,
            melt_alpha: 0.0,
        }),
    );
    w.core_mut(id).active = false;
    id
}

pub fn update_ice(w: &mut World, _c: &Content, id: EntityId, i: &mut Ice) {
    i.melt_alpha = max(0.0, i.melt_alpha - 0.1 * tm(w));
    if i.melt_alpha <= 0.0 {
        w.core_mut(id).active = false;
    }
}

/// `OnLavaCollide`: a side lava melts one tile per frame from its side,
/// a top/bottom lava the whole strip.
pub fn ice_melt(w: &mut World, _c: &Content, id: EntityId, i: &mut Ice, side: u8) {
    let core = w.core_mut(id);
    let hb = core.hitbox();
    if hb.w <= 0.0 {
        return;
    }
    let nw = match side {
        2 | 3 => hb.w - 10.0,
        _ => 0.0,
    };
    if side == 2 {
        core.pos.x += 10.0;
    }
    core.active = true;
    i.melt_alpha = 1.0;
    if nw <= 0.0 {
        core.collider = Collider::Box(Hitbox::new(0.0, 5.0, 0.0, 0.0));
        core.collidable = false;
    } else {
        core.collider = Collider::Box(Hitbox::new(nw, 5.0, 0.0, 0.0));
    }
}

// ------------------------------------------------------------ §7.2 Mud

pub const MUD_CELLS: usize = 12;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MudCell {
    pub mult: f32,
    pub add: f32,
    /// Frames until the wave hops on (0 = no pending hop).
    pub prop: f32,
    pub hops: i8,
    /// -1 / +1 locked direction, 0 = both.
    pub dir: i8,
}
impl_hash!(MudCell {
    mult,
    add,
    prop,
    hops,
    dir
});

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mud {
    pub n: u8,
    /// Cosmetic wave (global phase advancing pi/30 per frame).
    pub sine: f32,
    pub cells: [MudCell; MUD_CELLS],
    /// Cosmetic: last splash column and its animation clock.
    pub splash_cell: i8,
    pub splash_t: f32,
}
impl_hash!(Mud { n, sine, cells });

pub fn spawn_mud(w: &mut World, p: &Placement) -> EntityId {
    let width = if p.size.x > 0.0 { p.size.x } else { 20.0 };
    strip(
        w,
        p.pos,
        width,
        -149,
        &[Tag::Mud],
        Tower::Mud(Mud {
            n: ((width / 10.0) as usize).clamp(1, MUD_CELLS) as u8,
            sine: 0.0,
            cells: [MudCell::default(); MUD_CELLS],
            splash_cell: -1,
            splash_t: 0.0,
        }),
    )
}

pub fn update_mud(w: &mut World, _c: &Content, _id: EntityId, m: &mut Mud) {
    let t = tm(w);
    m.sine = (m.sine + PI / 30.0 * t) % (PI * 2.0);
    if m.splash_cell >= 0 {
        m.splash_t += t;
        if m.splash_t >= 8.0 * 5.0 {
            m.splash_cell = -1;
        }
    }
    let n = m.n as usize;
    for i in 0..n {
        let cell = m.cells[i];
        if cell.prop > 0.0 {
            m.cells[i].prop -= t;
            if m.cells[i].prop <= 0.0 && cell.hops > 0 {
                for d in [-1i8, 1] {
                    if cell.dir != 0 && cell.dir != d {
                        continue;
                    }
                    let j = i as i32 + d as i32;
                    if j < 0 || j >= n as i32 {
                        continue;
                    }
                    let nb = &mut m.cells[j as usize];
                    nb.mult = cell.mult + 0.05;
                    nb.add = cell.add;
                    nb.prop = 5.0;
                    nb.hops = cell.hops - 1;
                    nb.dir = d;
                }
            }
        }
        let c = &mut m.cells[i];
        c.mult = max(0.0, c.mult - 0.02 * t);
        if c.mult < 0.1 {
            c.mult = 0.0;
        }
        c.add = approach(c.add, 0.0, 0.1 * t);
    }
}

fn mud_cell(w: &World, id: EntityId, m: &Mud, x: f32) -> usize {
    let mx = w.core(id).pos.x;
    clamp(floor((x - mx) / 10.0), 0.0, m.n as f32 - 1.0) as usize
}

fn mud_splash(w: &mut World, id: EntityId, x: f32, up: bool) {
    let sine = match super::tower(w, id) {
        Some(Tower::Mud(m)) => m.sine,
        _ => return,
    };
    let Some(Tower::Mud(m)) = super::tower(w, id).copied() else {
        return;
    };
    let i = mud_cell(w, id, &m, x);
    if let Some(Tower::Mud(m)) = tower_mut(w, id) {
        let c = &mut m.cells[i];
        c.mult = 1.0;
        c.add = -sine + if up { PI } else { 0.0 };
        c.prop = 3.0;
        c.hops = 4;
        c.dir = 0;
        m.splash_cell = i as i8;
        m.splash_t = 0.0;
    }
}

/// `Mud.SplashDown(x)`.
pub fn mud_splash_down(w: &mut World, id: EntityId, x: f32) {
    mud_splash(w, id, x, false);
}

/// `Mud.SplashUp(x)` (jumping out of mud).
pub fn mud_splash_up(w: &mut World, id: EntityId, x: f32) {
    mud_splash(w, id, x, true);
}

// ------------------------------------------------------------ §7.3 HotCoals

pub fn spawn_hot_coals(w: &mut World, p: &Placement) -> EntityId {
    let width = if p.size.x > 0.0 { p.size.x } else { 30.0 };
    let id = strip(
        w,
        p.pos,
        width,
        -155,
        &[Tag::HotCoals, Tag::LavaCollider, Tag::LightSource],
        Tower::HotCoals,
    );
    w.core_mut(id).active = false;
    id
}

// ------------------------------------------------------------ §7.4 SnowClump

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnowClump {
    pub follow: Option<EntityId>,
    pub offset: Vec2,
    pub melting: bool,
    pub alpha: f32,
    /// Cosmetic frame (3 variants).
    pub variant: u8,
}
impl_hash!(SnowClump {
    follow,
    offset,
    melting,
    alpha
});

pub fn spawn_snow(w: &mut World, p: &Placement) -> EntityId {
    let core = Core::new(
        p.pos,
        Collider::Box(Hitbox::new(10.0, 4.0, 0.0, -3.0)),
        -151,
        &[Tag::PlayerCollider, Tag::LavaCollider],
    );
    let variant = w.cosmetic_rng.next_max(3) as u8;
    w.add(
        core,
        Kind::Tower(Tower::SnowClump(SnowClump {
            follow: None,
            offset: Vec2::ZERO,
            melting: false,
            alpha: 1.0,
            variant,
        })),
    )
}

pub fn snow_begin(w: &mut World, c: &Content, id: EntityId, s: &mut SnowClump) {
    let probe = w.placed(id);
    let below = w.first(c, &probe, Tag::Solid, id);
    match below {
        Some(b) if !matches!(w.kind(b), Some(Kind::Tiles)) => {
            s.follow = Some(b);
            s.offset = w.core(id).pos - w.core(b).pos;
        }
        _ => w.core_mut(id).active = false,
    }
}

fn snow_burst(w: &mut World, id: EntityId) {
    let pos = w.core(id).pos + Vec2::new(5.0, 0.0);
    particles(
        w,
        "SnowClumpDisperse",
        pos,
        10,
        Vec2::new(4.0, 0.0),
        -PI / 2.0,
    );
    w.remove(id);
}

pub fn update_snow(w: &mut World, _c: &Content, id: EntityId, s: &mut SnowClump) {
    let t = tm(w);
    if s.melting {
        s.alpha -= 0.1 * t;
        w.core_mut(id).pos.y += 0.4 * t;
        if s.alpha <= 0.0 {
            w.remove(id);
        }
        return;
    }
    let Some(f) = s.follow else { return };
    if !w.exists(f) {
        snow_burst(w, id);
        return;
    }
    let base = w.core(f).pos + s.offset;
    let mut pos = base;
    // Riding a jump pad segment: lifted with its squash (§6.10).
    if let Some(Tower::JumpPad(p)) = super::tower(w, f) {
        let seg = (s.offset.x / 10.0) as usize;
        if seg < p.n as usize && s.offset.y == 0.0 {
            pos.y = w.core(f).pos.y + 10.0 - 10.0 * p.scales[seg];
        }
    }
    w.core_mut(id).pos = pos;
}

pub fn snow_player(
    w: &mut World,
    _c: &Content,
    id: EntityId,
    s: &mut SnowClump,
    _pid: EntityId,
    p: &mut Player,
) {
    if !s.melting && p.state() == crate::player::PState::Dodging {
        snow_burst(w, id);
    }
}

pub fn snow_melt(w: &mut World, id: EntityId, s: &mut SnowClump) {
    if s.melting {
        return;
    }
    s.melting = true;
    let core = w.core_mut(id);
    core.collidable = false;
    core.active = true;
}

// ------------------------------------------------------------ archer hooks

/// An `Ice` strip at `Position + (0, 1)` (player.md §4 step 9).
pub fn ice_under(w: &World, c: &Content, id: EntityId) -> bool {
    if w.tagged(Tag::Ice).is_empty() {
        return false;
    }
    let at = w.core(id).pos + Vec2::new(0.0, 1.0);
    w.check_at(c, id, at, Tag::Ice)
}

/// A `HotCoals` strip at `Position + (0, 1)` (step 10).
pub fn hot_coals_under(w: &World, c: &Content, id: EntityId) -> bool {
    if w.tagged(Tag::HotCoals).is_empty() {
        return false;
    }
    let at = w.core(id).pos + Vec2::new(0.0, 1.0);
    w.check_at(c, id, at, Tag::HotCoals)
}

/// `onHotCoals` of the archer (set by its step 10).
pub fn on_hot_coals(w: &World, p: &Player) -> bool {
    w.tower.players[p.index as usize].on_hot_coals
}

/// `inMud` of the archer.
pub fn in_mud(w: &World, p: &Player) -> Option<EntityId> {
    w.tower.players[p.index as usize]
        .in_mud
        .filter(|&m| w.exists(m))
}

/// Step 10 and 12: hot coals and mud tracking.
pub fn player_mud_tracking(w: &mut World, c: &Content, id: EntityId, p: &mut Player) {
    let i = p.index as usize;
    w.tower.players[i].on_hot_coals = p.on_ground && hot_coals_under(w, c, id);
    if w.tagged(Tag::Mud).is_empty() {
        w.tower.players[i].in_mud = None;
        return;
    }
    let probe = w.placed(id);
    let x = w.core(id).pos.x;
    let now = w.first(c, &probe, Tag::Mud, id);
    if w.tower.players[i].in_mud.is_none() {
        if let Some(m) = now {
            mud_splash_down(w, m, x);
            sound(w, "env_mudPlayerLand", x);
        }
    } else if let Some(m) = now
        && p.speed.x != 0.0
    {
        let at = Vec2::new(x + 4.0 * p.speed.x, w.core(m).pos.y);
        particles(w, "MudPlayerRun", at, 1, Vec2::new(1.0, 0.0), -PI / 2.0);
    }
    w.tower.players[i].in_mud = now;
}

/// `FireControl.Start()` on an archer: 60 frames; wings burn off 30
/// frames after catching fire.
pub fn start_fire(w: &mut World, p: &Player) {
    let s = &mut w.tower.players[p.index as usize];
    if p.has_wings && (s.fire <= 0.0 || s.wings_fire <= 0.0) {
        s.wings_fire = 30.0;
    }
    s.fire = 60.0;
}

/// Fire component update and step 26 (wings burning off).
pub fn player_fire_update(w: &mut World, id: EntityId, p: &mut Player) {
    let t = tm(w);
    let x = w.core(id).pos.x;
    let s = &mut w.tower.players[p.index as usize];
    if s.fire <= 0.0 {
        return;
    }
    s.fire -= t;
    if s.wings_fire > 0.0 {
        s.wings_fire -= t;
        if s.wings_fire <= 0.0 {
            s.wings_fire = 0.0;
            if p.has_wings {
                p.has_wings = false;
                sound(w, "sfx_burn", x);
            }
        }
    }
    if w.time.on_interval(2) {
        let pos = w.core(id).pos;
        particles(w, "Fire", pos, 1, Vec2::new(3.0, 4.0), -PI / 2.0);
    }
}

/// Reset an archer's surface state when it (re)spawns.
pub fn player_reset(w: &mut World, index: u8) {
    w.tower.players[index as usize] = super::PlayerSurface::default();
}

// ------------------------------------------------------------ corpse hook

/// Ground friction of a grounded corpse (player.md §14.3 (c) step 3):
/// 0.4 in mud, 0.02 on ice/slippery floors, else 0.2.
pub fn corpse_ground_friction(w: &World, c: &Content, id: EntityId, owner: i8) -> f32 {
    if !w.tagged(Tag::Mud).is_empty() && w.any(c, &w.placed(id), Tag::Mud, id) {
        return 0.4;
    }
    // Per-archer variants of the corpse's owner.
    let v = w.variants_for(owner as i32);
    if !v.no_slipping && (v.slippery_floors || ice_under(w, c, id)) {
        return 0.02;
    }
    0.2
}

/// Whether a probe overlaps any hot coals (corpses burn on them).
pub fn probe_hot_coals(w: &World, c: &Content, probe: &Placed, me: EntityId) -> bool {
    w.any(c, probe, Tag::HotCoals, me)
}
