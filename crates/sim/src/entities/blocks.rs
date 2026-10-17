//! Breakable and conditional solids, jump pads (tower-entities.md §6).

use super::{
    Placement, Tower, actor_overlaps, living_players, move_towards, particles, sound, sound_vol,
    tm, tower,
};
use crate::actor::{self, wrap_dist_sq};
use crate::collide::{Collider, Hitbox, Placed};
use crate::components::Ease;
use crate::content::Content;
use crate::entity::{Core, EntityId, FlashFinish, Kind, Tag, depth};
use crate::impl_hash;
use crate::math::{PI, Vec2, approach, clamp, max};
use crate::world::World;

/// A wrap hitbox if the block touches a screen edge (§4.8).
fn edge_wrap_box(c: &Content, w: &World, pos: Vec2, bw: f32, bh: f32) -> Hitbox {
    let ws = w.world_size(c);
    if pos.y > ws.h - bh || pos.y < 0.0 || pos.x < 0.0 || pos.x > ws.w - bw {
        Hitbox::wrapping(bw, bh, 0.0, 0.0)
    } else {
        Hitbox::new(bw, bh, 0.0, 0.0)
    }
}

fn add_solid(w: &mut World, pos: Vec2, hb: Hitbox, tags: &[Tag], t: Tower) -> EntityId {
    let core = Core::new(pos, Collider::Box(hb), depth::SOLID, tags);
    w.add(core, Kind::Tower(t))
}

// ------------------------------------------------------------ §6.1 CrackedPlatform

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrackState {
    Waiting,
    Shifting,
    Shaking,
    Dead,
    Respawning,
}
crate::impl_hash_enum!(CrackState);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrackedPlatform {
    pub start: Vec2,
    pub state: CrackState,
    pub alarm: f32,
    /// Cosmetic art jitter.
    pub jitter: Vec2,
}
impl_hash!(CrackedPlatform {
    start,
    state,
    alarm
});

pub fn spawn_cracked_platform(w: &mut World, p: &Placement) -> EntityId {
    let width = if p.size.x > 0.0 { p.size.x } else { 20.0 };
    add_solid(
        w,
        p.pos,
        Hitbox::new(width, 10.0, 0.0, 0.0),
        &[Tag::Solid, Tag::ExplosionCollider],
        Tower::CrackedPlatform(CrackedPlatform {
            start: p.pos,
            state: CrackState::Waiting,
            alarm: 0.0,
            jitter: Vec2::ZERO,
        }),
    )
}

fn crumble_fx(w: &mut World, id: EntityId) {
    if w.time.on_interval(2) {
        let core = w.core(id);
        let half = core.width() / 2.0;
        let pos = Vec2::new(core.pos.x + half, core.pos.y + 5.0);
        particles(
            w,
            "CrackedBlockCrumble",
            pos,
            1,
            Vec2::new(half - 5.0, 0.0),
            PI / 2.0,
        );
    }
}

pub fn update_cracked_platform(w: &mut World, c: &Content, id: EntityId, b: &mut CrackedPlatform) {
    let t = tm(w);
    w.core_mut(id).flash.update(t);
    match b.state {
        CrackState::Waiting => {
            let ridden = w
                .tagged(Tag::Player)
                .iter()
                .any(|&p| actor::is_riding_solid(w, c, p, id));
            if ridden {
                b.state = CrackState::Shifting;
                let core = w.core(id);
                let x = core.pos.x + core.width() / 2.0;
                sound(w, "env_shakyFloor", x);
            }
        }
        CrackState::Shifting => {
            let target = b.start + Vec2::new(0.0, 3.0);
            move_towards(w, c, id, target, 0.4 * t, false);
            crumble_fx(w, id);
            if w.core(id).pos == target {
                b.state = CrackState::Shaking;
                b.alarm = 80.0;
            }
        }
        CrackState::Shaking => {
            let jx = w.cosmetic_rng.next_max(3) - 1;
            let jy = w.cosmetic_rng.next_max(3) - 1;
            b.jitter = Vec2::new(jx as f32, jy as f32);
            crumble_fx(w, id);
            b.alarm -= t;
            if b.alarm <= 0.0 {
                cracked_platform_die(w, id, b);
            }
        }
        CrackState::Dead => {
            b.alarm -= t;
            if b.alarm <= 0.0 {
                b.state = CrackState::Respawning;
                let core = w.core_mut(id);
                core.pos = b.start;
                core.counter = Vec2::ZERO;
                b.jitter = Vec2::ZERO;
                let x = b.start.x + w.core(id).width() / 2.0;
                sound(w, "env_shakyFloorRevive", x);
            }
        }
        CrackState::Respawning => {
            let probe = Placed {
                pos: b.start,
                collider: w.core(id).collider,
            };
            if !actor_overlaps(w, c, &probe, id) {
                b.state = CrackState::Waiting;
                let core = w.core_mut(id);
                core.collidable = true;
                core.visible = true;
                core.flash.start_for(20.0, FlashFinish::None);
            }
        }
    }
}

/// Dead: invisible, non-collidable, stuck arrows fall; respawn in 120.
pub fn cracked_platform_die(w: &mut World, id: EntityId, b: &mut CrackedPlatform) {
    b.state = CrackState::Dead;
    b.alarm = 120.0;
    b.jitter = Vec2::ZERO;
    let core = w.core_mut(id);
    core.visible = false;
    core.collidable = false;
    core.flash.stop();
    let width = core.width();
    let pos = core.pos + Vec2::new(width / 2.0, 5.0);
    sound(w, "env_shakyFloorBreak", pos.x);
    particles(
        w,
        "CrackedBlockBreak",
        pos,
        (0.6 * width) as u16,
        Vec2::new(width / 2.0 - 5.0, 4.0),
        0.0,
    );
}

// ------------------------------------------------- §6.2 CrackedWall, §6.3 CrumbleBlock

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BreakBlock {
    /// CrumbleBlock (else CrackedWall).
    pub crumble: bool,
    pub pushed: bool,
    pub counter: f32,
    pub normal: Vec2,
    /// CrumbleBlock: which of the face test points survive (enumeration of
    /// [`crumble_points`]).
    pub points: u64,
    pub w: f32,
    pub h: f32,
    pub crack_color: [u8; 4],
}
impl_hash!(BreakBlock {
    crumble,
    pushed,
    counter,
    normal,
    points,
    w,
    h
});

/// CrackedWall / CrumbleWall test points: the four edge midpoints.
pub const WALL_OFFSETS: [Vec2; 4] = [
    Vec2::new(0.0, 10.0),
    Vec2::new(20.0, 10.0),
    Vec2::new(10.0, 0.0),
    Vec2::new(10.0, 20.0),
];

pub fn spawn_cracked_wall(w: &mut World, c: &Content, p: &Placement) -> EntityId {
    let hb = edge_wrap_box(c, w, p.pos, 20.0, 20.0);
    let crack = c.level(w.level).tower.theme.crack_color;
    let id = add_solid(
        w,
        p.pos,
        hb,
        &[Tag::Solid, Tag::ExplosionCollider],
        Tower::BreakBlock(BreakBlock {
            crumble: false,
            pushed: false,
            counter: 0.0,
            normal: Vec2::ZERO,
            points: 0,
            w: 20.0,
            h: 20.0,
            crack_color: crack,
        }),
    );
    w.core_mut(id).active = false;
    id
}

/// The face test points of a `w x h` crumble block, in test order.
pub fn crumble_points(bw: f32, bh: f32, out: &mut Vec<Vec2>) {
    let mut i = 5.0;
    while i < bw {
        out.push(Vec2::new(i, -1.0));
        out.push(Vec2::new(i, bh));
        i += 10.0;
    }
    let mut j = 5.0;
    while j < bh {
        out.push(Vec2::new(-1.0, j));
        out.push(Vec2::new(bw, j));
        j += 10.0;
    }
}

pub fn spawn_crumble_block(w: &mut World, c: &Content, p: &Placement) -> EntityId {
    let bw = if p.size.x > 0.0 { p.size.x } else { 10.0 };
    let bh = if p.size.y > 0.0 { p.size.y } else { 10.0 };
    let lvl = c.level(w.level);
    let mut pts = vec![];
    crumble_points(bw, bh, &mut pts);
    let mut mask = 0u64;
    let mut any_left = false;
    for (k, o) in pts.iter().enumerate().take(64) {
        let mut q = p.pos + *o;
        // The original wraps the probes at 320 in both builds (§16.6 (b)).
        if q.x >= 320.0 {
            q.x -= 320.0;
        }
        if q.y >= 240.0 {
            q.y -= 240.0;
        }
        if !crate::collide::grid_point(lvl, q) {
            mask |= 1 << k;
            if o.x == -1.0 {
                any_left = true;
            }
        }
    }
    // Quirk kept: no open left face -> collider widened 2 px to the left.
    let hb = if any_left {
        Hitbox::wrapping(bw, bh, 0.0, 0.0)
    } else {
        Hitbox::wrapping(bw + 2.0, bh, -2.0, 0.0)
    };
    let id = add_solid(
        w,
        p.pos,
        hb,
        &[Tag::Solid, Tag::ExplosionCollider],
        Tower::BreakBlock(BreakBlock {
            crumble: true,
            pushed: false,
            counter: 0.0,
            normal: Vec2::ZERO,
            points: mask,
            w: bw,
            h: bh,
            crack_color: [0xb9, 0xdf, 0xfe, 0xff],
        }),
    );
    w.core_mut(id).active = false;
    id
}

pub fn break_block_offsets(
    _w: &World,
    _c: &Content,
    _id: EntityId,
    b: &BreakBlock,
    out: &mut Vec<Vec2>,
) {
    if !b.crumble {
        out.extend_from_slice(&WALL_OFFSETS);
        return;
    }
    let mut pts = vec![];
    crumble_points(b.w, b.h, &mut pts);
    for (k, o) in pts.into_iter().enumerate().take(64) {
        if b.points & (1 << k) != 0 {
            out.push(o);
        }
    }
    if out.is_empty() {
        // Fully enclosed: only reachable through its position.
        out.push(Vec2::ZERO);
    }
}

pub fn break_block_push(w: &mut World, id: EntityId, b: &mut BreakBlock, normal: Vec2) {
    b.normal = normal;
    if b.pushed {
        return;
    }
    b.pushed = true;
    b.counter = 5.0;
    w.core_mut(id).active = true;
    let x = w.core(id).pos.x + b.w / 2.0;
    sound(w, "env_debris", x);
}

pub fn update_break_block(w: &mut World, _c: &Content, id: EntityId, b: &mut BreakBlock) {
    if !b.pushed {
        return;
    }
    b.counter -= tm(w);
    if b.counter > 0.0 {
        return;
    }
    let pos = w.core(id).pos;
    let kind = if b.crumble {
        "BombParticle:CrumbleBlock"
    } else {
        "BombParticle:CrackedWall"
    };
    let n = if b.crumble {
        ((b.w / 5.0 - 1.0).max(1.0) * (b.h / 5.0 - 1.0).max(1.0)) as u16
    } else {
        9
    };
    particles(
        w,
        kind,
        pos + Vec2::new(b.w / 2.0, b.h / 2.0),
        n,
        Vec2::new(b.w / 2.0 - 5.0, b.h / 2.0 - 5.0),
        b.normal.angle(),
    );
    w.remove(id);
}

// ------------------------------------------------------------ §6.4 CrumbleWall

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrumbleWall {
    pub destroying: bool,
    pub alarm: f32,
    /// Cosmetic: crack overlay alpha and art jitter.
    pub fade: f32,
    pub jitter: Vec2,
}
impl_hash!(CrumbleWall { destroying, alarm });

pub fn spawn_crumble_wall(w: &mut World, c: &Content, p: &Placement) -> EntityId {
    let hb = edge_wrap_box(c, w, p.pos, 20.0, 20.0);
    add_solid(
        w,
        p.pos,
        hb,
        &[Tag::Solid, Tag::ExplosionCollider],
        Tower::CrumbleWall(CrumbleWall {
            destroying: false,
            alarm: 0.0,
            fade: 1.0,
            jitter: Vec2::ZERO,
        }),
    )
}

pub fn crumble_wall_destroy(w: &mut World, id: EntityId, b: &mut CrumbleWall) {
    if b.destroying {
        return;
    }
    b.destroying = true;
    b.alarm = 30.0;
    sound(w, "env_cataclysmBlockShake", w.core(id).pos.x);
}

pub fn update_crumble_wall(w: &mut World, c: &Content, id: EntityId, b: &mut CrumbleWall) {
    let t = tm(w);
    if !b.destroying {
        let touch = Placed {
            pos: w.core(id).pos,
            collider: Collider::Box(Hitbox::wrapping(24.0, 24.0, -2.0, -2.0)),
        };
        let touched = w.any(c, &touch, Tag::Player, id)
            || w.any(c, &touch, Tag::Corpse, id)
            || w.tagged(Tag::Arrow).iter().any(|&a| {
                w.arrow(a).is_some_and(|ar| {
                    ar.state == crate::arrow::ArrowState::Stuck && ar.stuck_to == Some(id)
                })
            });
        if touched {
            crumble_wall_destroy(w, id, b);
        }
        return;
    }
    b.fade = max(0.0, b.fade - t / 20.0);
    let jx = if w.cosmetic_rng.next_max(2) == 0 {
        -1.0
    } else {
        1.0
    };
    let jy = if w.cosmetic_rng.next_max(2) == 0 {
        -1.0
    } else {
        1.0
    };
    b.jitter = Vec2::new(jx, jy);
    b.alarm -= t;
    if b.alarm <= 0.0 {
        let pos = w.core(id).pos + Vec2::new(10.0, 10.0);
        particles(w, "CrumbleWallChunk", pos, 16, Vec2::new(8.0, 8.0), 0.0);
        sound(w, "env_cataclysmBlockBreak", pos.x);
        w.remove(id);
    }
}

// ------------------------------------------------------------ §6.5 MoonGlassBlock

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoonGlass {
    pub shattering: bool,
    /// Frames until the next shatter step.
    pub wait: f32,
    /// Cells already shattered (row-major, 10 per frame).
    pub cells: u16,
    /// Cosmetic: intro shine progress and shatter scale.
    pub shine: f32,
    pub scale: f32,
}
impl_hash!(MoonGlass {
    shattering,
    wait,
    cells
});

pub fn spawn_moon_glass(w: &mut World, p: &Placement) -> EntityId {
    add_solid(
        w,
        p.pos,
        Hitbox::wrapping(p.size.x, p.size.y, 0.0, 0.0),
        &[Tag::Solid, Tag::Moonglass],
        Tower::MoonGlass(MoonGlass {
            shattering: false,
            wait: 0.0,
            cells: 0,
            shine: 0.0,
            scale: 1.0,
        }),
    )
}

/// `InsideExplode()`: a bomb went off inside the glass (§6.5). `held` is
/// the archer whose update is running (it is not in the arena).
pub fn moon_glass_shatter(
    w: &mut World,
    c: &Content,
    id: EntityId,
    held: Option<(EntityId, &mut crate::player::Player)>,
) {
    let already = matches!(tower(w, id), Some(Tower::MoonGlass(m)) if m.shattering);
    if already {
        return;
    }
    sound_vol(w, "env_moonglassShatter", 160.0, 0.5);
    if let Some(Kind::Tower(Tower::MoonGlass(m))) = w.kind_mut(id) {
        m.shattering = true;
        m.wait = 6.0;
        m.scale = 1.2;
    }
    w.core_mut(id).active = true;
    // Launch bystanders (block shifted temporarily, §6.5 step 2).
    let base = w.core(id).pos;
    // `Bounce(Top)` is read while the block is shifted 2 px up.
    let top = w.core(id).top() - 2.0;
    let col = w.core(id).collider;
    let shifted = |d: Vec2| Placed {
        pos: base + d,
        collider: col,
    };
    let mut held = held;
    let players: Vec<EntityId> = w.tagged(Tag::Player).to_vec();
    let act = |w: &mut World, pid: EntityId, p: &mut crate::player::Player| {
        let up = w.check(c, &shifted(Vec2::new(0.0, -2.0)), pid);
        let left = w.check(c, &shifted(Vec2::new(-2.0, 0.0)), pid);
        let right = w.check(c, &shifted(Vec2::new(2.0, 0.0)), pid);
        if up {
            crate::player::stomp_bounce(w, c, pid, p, top);
        }
        if left {
            p.speed.x = -2.0;
        }
        if right {
            p.speed.x = 2.0;
        }
    };
    for pid in players {
        if let Some((hid, hp)) = held.as_mut()
            && *hid == pid
        {
            act(w, pid, hp);
            continue;
        }
        super::with_player(w, pid, |w, p| act(w, pid, p));
    }
    let corpses: Vec<EntityId> = w.tagged(Tag::Corpse).to_vec();
    for cid in corpses {
        if w.check(c, &shifted(Vec2::new(0.0, -2.0)), cid)
            && let Some(Kind::Corpse(cp)) = w.kind_mut(cid)
        {
            // `PlayerCorpse.Bounce()`.
            cp.speed.y = -3.0;
        }
    }
}

pub fn update_moon_glass(w: &mut World, c: &Content, id: EntityId, m: &mut MoonGlass) {
    let t = tm(w);
    if m.shine < 1.0 {
        m.shine = crate::math::min(1.0, m.shine + t / 30.0);
    }
    if !m.shattering {
        return;
    }
    m.scale = approach(m.scale, 1.0, 0.2 / 6.0 * t);
    m.wait -= t;
    if m.wait > 0.0 {
        return;
    }
    let core = w.core(id);
    let hb = core.hitbox();
    let (cols, rows) = ((hb.w / 10.0) as u16, (hb.h / 10.0) as u16);
    let total = cols.max(1) * rows.max(1);
    let ws = w.world_size(c);
    let pos = core.pos;
    let end = (m.cells + 10).min(total);
    for k in m.cells..end {
        let (cx, cy) = (k % cols.max(1), k / cols.max(1));
        let mut p = pos + Vec2::new(cx as f32 * 10.0 + 5.0, cy as f32 * 10.0 + 5.0);
        p.x = p.x.rem_euclid(ws.w);
        p.y = p.y.rem_euclid(ws.h);
        let dir = w.cosmetic_rng.next_angle();
        particles(w, "MoonglassShatter", p, 1, Vec2::new(3.0, 3.0), dir);
    }
    m.cells = end;
    m.wait = 1.0;
    if m.cells >= total {
        w.remove(id);
    }
}

// ------------------------------------------------------------ §6.6 GraniteBlock

pub fn spawn_granite(w: &mut World, p: &Placement, granite: bool) -> EntityId {
    let tags: &[Tag] = if granite {
        &[Tag::Solid, Tag::Granite]
    } else {
        &[Tag::Solid]
    };
    let id = add_solid(
        w,
        p.pos,
        Hitbox::new(p.size.x.max(10.0), p.size.y.max(10.0), 0.0, 0.0),
        tags,
        Tower::Granite,
    );
    w.core_mut(id).active = false;
    id
}

// ------------------------------------------------------------ §6.7 SwitchBlock

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwitchBlock {
    pub red: bool,
    pub on: bool,
    /// Switched on, waiting for its space to be free.
    pub pending: bool,
    /// Cosmetic: flicker frames, squash tween frames, warning alpha.
    pub flicker: f32,
    pub squash: f32,
    pub warning: f32,
}
impl_hash!(SwitchBlock { red, on, pending });

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwitchControl {
    pub timer: f32,
    /// Trials: flip once, after the clock starts, then remove.
    pub once: bool,
}
impl_hash!(SwitchControl { timer, once });

pub fn spawn_switch(w: &mut World, c: &Content, p: &Placement, red: bool) -> EntityId {
    let hb = edge_wrap_box(c, w, p.pos, p.size.x, p.size.y);
    let id = add_solid(
        w,
        p.pos,
        hb,
        &[Tag::Solid],
        Tower::SwitchBlock(SwitchBlock {
            red,
            on: red,
            pending: false,
            flicker: 0.0,
            squash: 0.0,
            warning: 0.0,
        }),
    );
    w.core_mut(id).collidable = red;
    id
}

pub fn spawn_switch_control(w: &mut World) -> EntityId {
    let mut core = Core::new(Vec2::ZERO, Collider::None, 0, &[]);
    core.visible = false;
    core.collidable = false;
    w.add(
        core,
        Kind::Tower(Tower::SwitchControl(SwitchControl {
            timer: 300.0,
            once: false,
        })),
    )
}

fn switch_blocks(w: &World) -> Vec<EntityId> {
    w.tagged(Tag::Solid)
        .iter()
        .copied()
        .filter(|&e| matches!(tower(w, e), Some(Tower::SwitchBlock(_))))
        .collect()
}

pub fn update_switch_control(w: &mut World, _c: &Content, id: EntityId, s: &mut SwitchControl) {
    if !w.tower.round_started {
        return;
    }
    // Trials: inactive until the clock starts.
    if s.once && !w.trials.as_ref().is_some_and(|t| t.started) {
        return;
    }
    let before = s.timer;
    s.timer -= tm(w);
    if before > 40.0 && s.timer <= 40.0 {
        for b in switch_blocks(w) {
            if let Some(Kind::Tower(Tower::SwitchBlock(sb))) = w.kind_mut(b) {
                sb.warning = 1.0;
            }
        }
    }
    if s.timer <= 0.0 {
        for b in switch_blocks(w) {
            super::with_tower(w, b, |w, t| {
                if let Tower::SwitchBlock(sb) = t {
                    switch_block_switch(w, b, sb);
                }
            });
        }
        sound(w, "env_blockSwitch", 0.0);
        s.timer = 300.0;
        if s.once {
            w.remove(id);
        }
    }
}

/// `Switch()`.
pub fn switch_block_switch(w: &mut World, id: EntityId, b: &mut SwitchBlock) {
    b.on = !b.on;
    if b.on {
        b.pending = true;
    } else {
        b.pending = false;
        w.core_mut(id).collidable = false;
        b.flicker = 30.0;
        b.squash = 60.0;
    }
}

pub fn update_switch(w: &mut World, c: &Content, id: EntityId, b: &mut SwitchBlock) {
    let t = tm(w);
    if b.flicker > 0.0 {
        b.flicker -= t;
    }
    if b.squash > 0.0 {
        b.squash -= t;
    }
    b.warning = max(0.0, b.warning - 0.04 * t);
    if b.pending {
        let probe = w.placed(id);
        if !actor_overlaps(w, c, &probe, id) {
            b.pending = false;
            w.core_mut(id).collidable = true;
            b.flicker = 30.0;
            b.squash = 60.0;
        }
    }
}

// ------------------------------------------------------------ §6.8 ProximityBlock

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProximityBlock {
    /// Frames of the running appear/disappear tween (0 = idle).
    pub transition: f32,
    pub appearing: bool,
    /// Cosmetic art state (eased scale and rotation, animation frame).
    pub scale: f32,
    pub rotation: f32,
    pub frame: f32,
}
impl_hash!(ProximityBlock {
    transition,
    appearing
});

pub fn spawn_proximity(w: &mut World, p: &Placement) -> EntityId {
    let mut core = Core::new(
        p.pos,
        Collider::Box(Hitbox::wrapping(20.0, 20.0, -10.0, -10.0)),
        depth::SOLID,
        &[Tag::Solid],
    );
    core.screen_wrap = false;
    w.add(
        core,
        Kind::Tower(Tower::ProximityBlock(ProximityBlock {
            transition: 0.0,
            appearing: false,
            scale: 1.0,
            rotation: 0.0,
            frame: 0.0,
        })),
    )
}

fn player_within(w: &World, c: &Content, id: EntityId, d2: f32) -> bool {
    let ws = w.world_size(c);
    let at = w.core(id).pos;
    living_players(w).any(|p| wrap_dist_sq(w.core(p).pos, at, ws.w, ws.h) <= d2)
}

pub fn proximity_begin(w: &mut World, c: &Content, id: EntityId, b: &mut ProximityBlock) {
    if player_within(w, c, id, 1764.0) {
        w.core_mut(id).collidable = false;
        b.scale = 0.0;
        b.frame = 13.0;
    }
}

pub fn update_proximity(w: &mut World, c: &Content, id: EntityId, b: &mut ProximityBlock) {
    let t = tm(w);
    if b.transition > 0.0 {
        b.transition -= t;
        if b.appearing {
            let p = 1.0 - max(0.0, b.transition) / 20.0;
            let e = Ease::BackOut.apply(p);
            b.scale = e;
            b.rotation = e * 2.0 * PI;
            b.frame = 13.0 - (p * 14.0).min(13.0);
        } else {
            let p = 1.0 - max(0.0, b.transition) / 12.0;
            b.rotation = Ease::BackIn.apply(p) * PI / 2.0;
            b.frame = (p * 14.0).min(13.0);
            if b.transition <= 0.0 {
                b.scale = 0.0;
            }
        }
        return;
    }
    let x = w.core(id).pos.x;
    if w.core(id).collidable {
        if player_within(w, c, id, 1764.0) {
            w.core_mut(id).collidable = false;
            sound_vol(w, "env_proxBlockOut", x, 0.7);
            b.appearing = false;
            b.transition = 12.0;
        }
    } else if !player_within(w, c, id, 2704.0) {
        let probe = Placed {
            pos: w.core(id).pos,
            collider: w.core(id).collider,
        };
        if !actor_overlaps(w, c, &probe, id) {
            w.core_mut(id).collidable = true;
            sound_vol(w, "env_proxBlockIn", x, 0.7);
            b.appearing = true;
            b.transition = 20.0;
            b.scale = 0.0;
        }
    }
}

// ------------------------------------------------------------ §6.9 FakeWall

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FakeWall {
    pub alpha: f32,
    pub force_open: bool,
}
impl_hash!(FakeWall { alpha, force_open });

pub fn spawn_fake_wall(w: &mut World, p: &Placement) -> EntityId {
    let core = Core::new(
        p.pos,
        Collider::Box(Hitbox::new(p.size.x, p.size.y, 0.0, 0.0)),
        -151,
        &[],
    );
    w.add(
        core,
        Kind::Tower(Tower::FakeWall(FakeWall {
            alpha: 1.0,
            force_open: false,
        })),
    )
}

pub fn update_fake_wall(w: &mut World, c: &Content, id: EntityId, f: &mut FakeWall) {
    let probe = w.placed(id);
    let open = f.force_open || w.any(c, &probe, Tag::Player, id);
    f.alpha = approach(f.alpha, if open { 0.0 } else { 1.0 }, 0.05 * tm(w));
}

// ------------------------------------------------------------ §6.10 JumpPad

pub const PAD_SEGMENTS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JumpPad {
    pub n: u8,
    /// Per-segment squash (`Scale.Y`).
    pub scales: [f32; PAD_SEGMENTS],
    pub on: bool,
    pub light: f32,
}
impl_hash!(JumpPad {
    n,
    scales,
    on,
    light
});

pub fn spawn_jump_pad(w: &mut World, p: &Placement) -> EntityId {
    let width = if p.size.x > 0.0 { p.size.x } else { 20.0 };
    let n = ((width / 10.0) as usize).clamp(1, PAD_SEGMENTS) as u8;
    add_solid(
        w,
        p.pos,
        Hitbox::new(width, 10.0, 0.0, 0.0),
        &[Tag::Solid, Tag::JumpPad, Tag::LightSource],
        Tower::JumpPad(JumpPad {
            n,
            scales: [1.0; PAD_SEGMENTS],
            on: false,
            light: 0.0,
        }),
    )
}

/// `Launch(x)`: squash the segments around the jumper.
pub fn jump_pad_launch(w: &mut World, id: EntityId, b: &mut JumpPad, x: f32) {
    let px = w.core(id).pos.x;
    let n = b.n as f32;
    let k = clamp((x - px) / 10.0, 0.0, n - 1.0);
    for i in 0..b.n as usize {
        let d = (k - 0.5 - i as f32).abs();
        let s = if d <= 0.3 {
            1.8
        } else if d <= 1.0 {
            1.6
        } else if d <= 2.0 {
            1.4
        } else if d <= 3.0 {
            1.2
        } else {
            continue;
        };
        b.scales[i] = max(b.scales[i], s);
    }
}

pub fn update_jump_pad(w: &mut World, c: &Content, id: EntityId, b: &mut JumpPad) {
    let t = tm(w);
    for s in b.scales.iter_mut().take(b.n as usize) {
        *s = approach(*s, 1.0, 0.02 * t);
    }
    let probe = Placed {
        pos: w.core(id).pos + Vec2::new(0.0, -1.0),
        collider: w.core(id).collider,
    };
    let stood = w.any(c, &probe, Tag::Player, id);
    let core = w.core(id);
    let (x, width) = (core.pos.x, core.width());
    if stood && !b.on {
        b.on = true;
        if w.time.frame_counter >= 30.0 {
            sound(w, "env_superBlockLand", x + width / 2.0);
        }
    } else if !stood && b.on {
        b.on = false;
        sound(w, "env_superBlockJump", x);
    }
    b.light = approach(b.light, if b.on { 1.0 } else { 0.0 }, 0.1 * t);
}
