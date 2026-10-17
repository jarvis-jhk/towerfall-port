//! Moving and carried platforms (tower-entities.md §5).

use super::{
    Dir4, Placement, ThemeInfo, Tower, move_to, move_towards, particles, ridden_by_body, sound,
    sound_vol, tm, tower_mut,
};
use crate::collide::{Collider, Hitbox, Placed};
use crate::components::Ease;
use crate::content::Content;
use crate::entity::{Core, EntityId, Kind, Tag, depth};
use crate::math::{PI, Vec2, approach, cos, lerp, sin};
use crate::world::World;
use crate::{impl_hash, impl_hash_enum};

impl_hash_enum!(Dir4);

fn lerp_v(a: Vec2, b: Vec2, t: f32) -> Vec2 {
    Vec2::new(lerp(a.x, b.x, t), lerp(a.y, b.y, t))
}

// ------------------------------------------------------------ §5.1 MovingPlatform

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovingPlatform {
    pub start: Vec2,
    pub end: Vec2,
    pub to_end: bool,
    /// Rest counter (120 at creation and after each leg).
    pub rest: f32,
    /// Cosmetic shake counter.
    pub shake: f32,
    pub cataclysm: bool,
}
impl_hash!(MovingPlatform {
    start,
    end,
    to_end,
    rest,
    shake
});

pub fn spawn_moving(w: &mut World, p: &Placement, theme: &ThemeInfo) -> EntityId {
    let core = Core::new(
        p.pos,
        Collider::Box(Hitbox::new(p.size.x, p.size.y, 0.0, 0.0)),
        depth::SOLID,
        &[Tag::Solid],
    );
    w.add(
        core,
        Kind::Tower(Tower::MovingPlatform(MovingPlatform {
            start: p.pos,
            end: p.node.unwrap_or(p.pos),
            to_end: true,
            rest: 120.0,
            shake: 0.0,
            cataclysm: theme.cataclysm_tiles,
        })),
    )
}

pub fn update_moving(w: &mut World, c: &Content, id: EntityId, m: &mut MovingPlatform) {
    let t = tm(w);
    if m.shake > 0.0 {
        m.shake -= t;
    }
    if m.rest > 0.0 {
        m.rest -= t;
        if m.rest <= 0.0 {
            sound(w, "env_movingBlockStart", w.core(id).pos.x);
            m.shake = 20.0;
        }
        return;
    }
    let target = if m.to_end { m.end } else { m.start };
    move_towards(w, c, id, target, 0.4 * t, false);
    if w.core(id).pos == target {
        m.to_end = !m.to_end;
        m.shake = 20.0;
        m.rest = 120.0;
        sound(w, "env_movingBlockEnd", w.core(id).pos.x);
    }
}

// ------------------------------------------------------------ §5.2 ShiftBlock

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShiftState {
    Waiting,
    Shaking,
    Moving,
    PostMove,
}
impl_hash_enum!(ShiftState);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShiftBlock {
    pub start: Vec2,
    pub node: Vec2,
    pub state: ShiftState,
    pub counter: f32,
    pub t: f32,
    pub from: Vec2,
    pub to: Vec2,
    /// Light colour idle -> angry (cosmetic).
    pub color_lerp: f32,
    /// Cosmetic: face variant (0/1) and the art jitter.
    pub face: u8,
    pub jitter: Vec2,
}
impl_hash!(ShiftBlock {
    start,
    node,
    state,
    counter,
    t,
    from,
    to
});

pub fn spawn_shift(w: &mut World, p: &Placement) -> EntityId {
    let core = Core::new(
        p.pos,
        Collider::Box(Hitbox::new(p.size.x, p.size.y, 0.0, 0.0)),
        depth::SOLID,
        &[Tag::Solid, Tag::LightSource],
    );
    let face = w.cosmetic_rng.next_max(2) as u8;
    w.add(
        core,
        Kind::Tower(Tower::ShiftBlock(ShiftBlock {
            start: p.pos,
            node: p.node.unwrap_or(p.pos),
            state: ShiftState::Waiting,
            counter: 170.0,
            t: 0.0,
            from: p.pos,
            to: p.pos,
            color_lerp: 0.0,
            face,
            jitter: Vec2::ZERO,
        })),
    )
}

fn shift_jitter(w: &mut World) -> Vec2 {
    let x = w.cosmetic_rng.next_max(3) - 1;
    let y = w.cosmetic_rng.next_max(3) - 1;
    Vec2::new(x as f32, y as f32)
}

pub fn update_shift(w: &mut World, c: &Content, id: EntityId, s: &mut ShiftBlock) {
    let t = tm(w);
    let target = if s.state == ShiftState::Waiting {
        0.0
    } else {
        1.0
    };
    s.color_lerp = approach(s.color_lerp, target, 0.05 * t);
    match s.state {
        ShiftState::Waiting => {
            s.jitter = Vec2::ZERO;
            s.counter -= t;
            if s.counter <= 0.0 {
                s.state = ShiftState::Shaking;
                s.counter = 30.0;
                if !w.tower.shift_start_played {
                    w.tower.shift_start_played = true;
                    sound_vol(w, "env_greyblockStart", 160.0, 0.6);
                }
            }
        }
        ShiftState::Shaking => {
            s.jitter = shift_jitter(w);
            s.counter -= t;
            if s.counter <= 0.0 {
                s.state = ShiftState::Moving;
                s.t = 0.0;
                s.from = w.core(id).pos;
                s.to = if s.from == s.start { s.node } else { s.start };
                s.jitter = Vec2::ZERO;
                w.tower.shift_start_played = false;
                w.tower.shift_end_played = false;
            }
        }
        ShiftState::Moving => {
            s.t = crate::math::min(s.t + 0.025 * t, 1.0);
            let e = s.t * s.t * s.t;
            move_to(w, c, id, lerp_v(s.from, s.to, e), false);
            if s.t >= 1.0 {
                s.state = ShiftState::PostMove;
                s.counter = 10.0;
            }
        }
        ShiftState::PostMove => {
            s.jitter = shift_jitter(w);
            if !w.tower.shift_end_played {
                w.tower.shift_end_played = true;
                let x = w.world_size(c).w / 2.0;
                sound(w, "env_movingBlockEnd", x);
            }
            s.counter -= t;
            if s.counter <= 0.0 {
                s.state = ShiftState::Waiting;
                s.counter = 170.0;
                s.jitter = Vec2::ZERO;
                w.tower.shift_start_played = false;
                w.tower.shift_end_played = false;
            }
        }
    }
}

// ------------------------------------------------------------ §5.3 LoopPlatform

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopPlatform {
    pub dir: Dir4,
    pub master: bool,
    pub partner: Option<EntityId>,
    pub sink: f32,
    /// The sink delta the master handed over this frame (slave only).
    pub pending_sink: f32,
    /// Light alpha (cosmetic).
    pub light: f32,
}
impl_hash!(LoopPlatform {
    dir,
    master,
    partner,
    sink,
    pending_sink
});

fn loop_core(pos: Vec2, width: f32, dir: Dir4, lit: bool) -> Core {
    let vertical = matches!(dir, Dir4::Up | Dir4::Down);
    let hb = if vertical {
        Hitbox::wrapping(width, 5.0, 0.0, 0.0)
    } else {
        Hitbox::new(width, 5.0, 0.0, 0.0)
    };
    let tags: &[Tag] = if lit {
        &[Tag::JumpThru, Tag::LightSource]
    } else {
        &[Tag::JumpThru]
    };
    let mut core = Core::new(pos, Collider::Box(hb), -149, tags);
    core.screen_wrap = vertical;
    core
}

pub fn spawn_loop(w: &mut World, c: &Content, p: &Placement) -> EntityId {
    let ws = w.world_size(c);
    let width = if p.size.x > 0.0 { p.size.x } else { 20.0 };
    let lp = LoopPlatform {
        dir: p.dir,
        master: true,
        partner: None,
        sink: 0.0,
        pending_sink: 0.0,
        light: 1.0,
    };
    let master = w.add(
        loop_core(p.pos, width, p.dir, true),
        Kind::Tower(Tower::LoopPlatform(lp)),
    );
    // The slave sits exactly one screen away (the original teleports it there
    // on its first update; §16.6 (h)).
    let off = match p.dir {
        Dir4::Right => Vec2::new(-ws.w, 0.0),
        Dir4::Left => Vec2::new(ws.w, 0.0),
        Dir4::Up => Vec2::new(0.0, ws.h),
        Dir4::Down => Vec2::new(0.0, -ws.h),
    };
    let slave = w.add(
        loop_core(p.pos + off, width, p.dir, false),
        Kind::Tower(Tower::LoopPlatform(LoopPlatform {
            master: false,
            partner: Some(master),
            ..lp
        })),
    );
    if let Some(Tower::LoopPlatform(m)) = tower_mut(w, master) {
        m.partner = Some(slave);
    }
    master
}

/// `Platform.StuckPair`: the other half of a looping platform.
pub fn stuck_pair(w: &World, id: EntityId) -> Option<EntityId> {
    match w.kind(id) {
        Some(Kind::Tower(Tower::LoopPlatform(l))) => l.partner,
        _ => None,
    }
}

pub fn update_loop(w: &mut World, c: &Content, id: EntityId, l: &mut LoopPlatform) {
    // Trials: still until the clock starts.
    if w.trials.as_ref().is_some_and(|t| !t.started) {
        return;
    }
    let t = tm(w);
    let ws = w.world_size(c);
    let horizontal = matches!(l.dir, Dir4::Left | Dir4::Right);
    let mut dy = 0.0;
    if horizontal {
        if l.master {
            let ridden = ridden_by_body(w, c, id, true)
                || l.partner
                    .is_some_and(|s| w.exists(s) && ridden_by_body(w, c, s, true));
            let target = if ridden { 2.0 } else { 0.0 };
            let ns = approach(l.sink, target, 0.2 * t);
            dy = ns - l.sink;
            l.sink = ns;
            if let Some(s) = l.partner
                && let Some(Tower::LoopPlatform(sl)) = tower_mut(w, s)
            {
                sl.pending_sink = dy;
                sl.sink = ns;
            }
        } else {
            dy = l.pending_sink;
            l.pending_sink = 0.0;
        }
    }
    let v = match l.dir {
        Dir4::Right => Vec2::new(0.5 * t, 0.0),
        Dir4::Left => Vec2::new(-0.5 * t, 0.0),
        Dir4::Up => Vec2::new(0.0, -0.5 * t),
        Dir4::Down => Vec2::new(0.0, 0.5 * t),
    };
    super::platform_move(w, c, id, v + Vec2::new(0.0, dy), true);
    let width = w.core(id).width();
    let core = w.core_mut(id);
    match (l.dir, l.master) {
        (Dir4::Right, true) if core.pos.x >= ws.w => core.pos.x -= ws.w,
        (Dir4::Right, false) if core.pos.x >= 0.0 => core.pos.x -= ws.w,
        (Dir4::Left, true) if core.pos.x < -width => core.pos.x += ws.w,
        (Dir4::Left, false) if core.pos.x < ws.w - width => core.pos.x += ws.w,
        (Dir4::Down, true) if core.pos.y >= ws.h => core.pos.y -= ws.h,
        (Dir4::Down, false) if core.pos.y >= 0.0 => core.pos.y -= ws.h,
        (Dir4::Up, true) if core.pos.y < -5.0 => core.pos.y += ws.h,
        (Dir4::Up, false) if core.pos.y < ws.h - 5.0 => core.pos.y += ws.h,
        _ => {}
    }
    // Light: fades out while the platform is inside a wall (cosmetic).
    if l.master {
        let core = w.core(id);
        let lvl = w.level_def(c);
        let cy = core.pos.y + 2.0;
        let a = Vec2::new(core.left() + 15.0, cy);
        let b = Vec2::new(core.right() - 15.0, cy);
        let inside = crate::collide::grid_point(lvl, a) && crate::collide::grid_point(lvl, b);
        l.light = approach(l.light, if inside { 0.0 } else { 1.0 }, 0.04 * t);
    }
}

// ------------------------------------------------------------ §5.4 GhostPlatform

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GhostPlatform {
    pub start: Vec2,
    pub can_sink: bool,
    pub sink: f32,
    /// Cosmetic: alpha and strip frame.
    pub alpha: f32,
    pub frame: f32,
}
impl_hash!(GhostPlatform {
    start,
    can_sink,
    sink
});

pub fn spawn_ghost(w: &mut World, c: &Content, p: &Placement) -> EntityId {
    let width = if p.size.x > 0.0 { p.size.x } else { 20.0 };
    let lvl = w.level_def(c);
    let solid = |x: f32, y: f32| crate::collide::grid_point(lvl, Vec2::new(x, y));
    let (x, y) = (p.pos.x, p.pos.y);
    let bridging = (solid(x - 1.0, y) && !solid(x - 1.0, y - 1.0))
        || (solid(x + width, y) && !solid(x + width, y - 1.0));
    let core = Core::new(
        p.pos,
        Collider::Box(Hitbox::new(width, 5.0, 0.0, 0.0)),
        -149,
        &[Tag::JumpThru, Tag::LightSource],
    );
    w.add(
        core,
        Kind::Tower(Tower::GhostPlatform(GhostPlatform {
            start: p.pos,
            can_sink: !bridging,
            sink: 0.0,
            alpha: 0.8,
            frame: 0.0,
        })),
    )
}

pub fn update_ghost(w: &mut World, c: &Content, id: EntityId, g: &mut GhostPlatform) {
    let t = tm(w);
    let ridden = ridden_by_body(w, c, id, true);
    if g.can_sink {
        g.sink = approach(g.sink, if ridden { 2.0 } else { 0.0 }, 0.2 * t);
        move_to(w, c, id, g.start + Vec2::new(0.0, g.sink), true);
    }
    g.alpha = approach(g.alpha, if ridden { 0.9 } else { 0.8 }, 0.1 * t);
    g.frame = approach(g.frame, if ridden { 2.0 } else { 0.0 }, 0.2 * t);
}

// ------------------------------------------------------------ §5.5 RotatePlatform

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotatePlatform {
    pub centre: Vec2,
    pub radius: f32,
    pub angle: f32,
    /// Radians per frame.
    pub speed: f32,
    pub speed_mult: f32,
    pub half_w: f32,
    pub sink: f32,
}
impl_hash!(RotatePlatform {
    centre,
    radius,
    angle,
    speed,
    speed_mult,
    half_w,
    sink
});

pub fn spawn_rotate_ring(w: &mut World, p: &Placement) {
    let n = p.amount.max(1);
    let width = if p.ring_width > 0.0 {
        p.ring_width
    } else {
        40.0
    };
    for i in 0..n {
        let a = -PI / 2.0 + i as f32 * (PI * 2.0 / n as f32);
        let pos = p.pos + Vec2::new(cos(a), sin(a)) * p.radius + Vec2::new(-width / 2.0, 0.0);
        let core = Core::new(
            pos,
            Collider::Box(Hitbox::new(width, 5.0, 0.0, 0.0)),
            depth::SOLID,
            &[Tag::JumpThru, Tag::LightSource],
        );
        w.add(
            core,
            Kind::Tower(Tower::RotatePlatform(RotatePlatform {
                centre: p.pos,
                radius: p.radius,
                angle: a,
                speed: p.deg_speed * PI / 180.0,
                speed_mult: 1.0,
                half_w: width / 2.0,
                sink: 0.0,
            })),
        );
    }
}

pub fn update_rotate(w: &mut World, c: &Content, id: EntityId, r: &mut RotatePlatform) {
    let t = tm(w);
    r.angle += r.speed * r.speed_mult * t;
    let ridden = ridden_by_body(w, c, id, true);
    r.sink = approach(r.sink, if ridden { 4.0 } else { 0.0 }, 0.2 * t);
    let target =
        r.centre + Vec2::new(cos(r.angle), sin(r.angle)) * r.radius + Vec2::new(-r.half_w, r.sink);
    move_to(w, c, id, target, true);
}

// ------------------------------------------------------------ §5.7 SensorBlock

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SensorBlock {
    pub start: Vec2,
    pub target: Vec2,
    /// -2 idle (sensing), -1 triggered (sequence starts next frame), else
    /// the frame of the running sequence.
    pub seq: i32,
    pub squisher: i8,
    /// Cosmetic: shake frames left, trigger colour (0..1), light alpha.
    pub shake: f32,
    pub glow: f32,
    pub cataclysm: bool,
    /// `CanSense`: Dark World's "disable dangers" turns it off.
    pub can_sense: bool,
}
impl_hash!(SensorBlock {
    start,
    target,
    seq,
    squisher,
    can_sense
});

/// `DisableDangers`: no sensor block reacts any more.
pub fn disable_sensors(w: &mut World) {
    let ids: Vec<EntityId> = w.entities().collect();
    for id in ids {
        if let Some(Kind::Tower(Tower::SensorBlock(s))) = w.kind_mut(id) {
            s.can_sense = false;
        }
    }
}

pub fn spawn_sensor(w: &mut World, p: &Placement) -> EntityId {
    let width = if p.size.x > 0.0 { p.size.x } else { 40.0 };
    let core = Core::new(
        p.pos,
        Collider::Box(Hitbox::new(width, 30.0, 0.0, 0.0)),
        depth::SOLID,
        &[Tag::Solid, Tag::LightSource],
    );
    w.add(
        core,
        Kind::Tower(Tower::SensorBlock(SensorBlock {
            start: p.pos,
            target: p.pos,
            seq: -2,
            squisher: -1,
            shake: 0.0,
            glow: 0.0,
            cataclysm: false,
            can_sense: true,
        })),
    )
}

/// Fall target (computed once at scene begin).
pub fn sensor_begin(w: &mut World, c: &Content, id: EntityId, s: &mut SensorBlock) {
    if s.start.y >= 210.0 {
        s.target = s.start;
        return;
    }
    let col = w.core(id).collider;
    let mut target = s.start + Vec2::new(0.0, 5.0);
    while target.y < 210.0 {
        let probe = Placed {
            pos: target + Vec2::new(0.0, 5.0),
            collider: col,
        };
        if w.any(c, &probe, Tag::Solid, id) {
            break;
        }
        target.y += 5.0;
    }
    s.target = target;
}

fn sensor_rect(w: &World, id: EntityId, s: &SensorBlock) -> Placed {
    let width = w.core(id).width();
    Placed {
        pos: Vec2::new(s.start.x + 4.0, s.start.y + 30.0),
        collider: Collider::Box(Hitbox::new(width - 8.0, s.target.y - s.start.y, 0.0, 0.0)),
    }
}

fn sensor_detect(w: &World, c: &Content, id: EntityId, s: &SensorBlock) -> Option<i8> {
    if s.target.y <= s.start.y {
        return None;
    }
    let rect = sensor_rect(w, id, s);
    for &p in w.tagged(Tag::Player) {
        let Some(pl) = w.player(p) else { continue };
        if pl.dead || w.core(p).flash.flashing || pl.speed.is_zero() {
            continue;
        }
        if w.check(c, &rect, p) {
            return Some(pl.index as i8);
        }
    }
    for &a in w.tagged(Tag::Arrow) {
        let Some(ar) = w.arrow(a) else { continue };
        if ar.speed.is_zero() || ar.player_index == -1 {
            continue;
        }
        if w.check(c, &rect, a) {
            return Some(ar.player_index);
        }
    }
    // Moving player ghosts (Return As Ghosts) trigger it too.
    for &g in w.tagged(Tag::PlayerGhost) {
        let Some(Kind::Enemy(e)) = w.kind(g) else {
            continue;
        };
        if e.speed.is_zero() {
            continue;
        }
        let gc = w.core(g);
        let probe = Placed {
            pos: gc.pos,
            collider: gc.collider,
        };
        if crate::collide::collide(&probe, &rect, c.level(w.level), w.world_size(c)) {
            return Some(e.owner);
        }
    }
    None
}

pub fn update_sensor(w: &mut World, c: &Content, id: EntityId, s: &mut SensorBlock) {
    let t = tm(w);
    if s.shake > 0.0 {
        s.shake -= t;
    }
    if s.seq >= -1 {
        s.seq += 1;
        let k = s.seq;
        let x = w.core(id).pos.x;
        match k {
            0 => {
                s.shake = 40.0;
                sound(w, "env_crusherDetect", x);
            }
            41 => sound(w, "env_crusherSlam", x),
            42..=61 => {
                let e = Ease::CubeIn.apply((k - 41) as f32 / 20.0);
                move_to(w, c, id, lerp_v(s.start, s.target, e), false);
            }
            62 => {
                s.shake = 10.0;
                let core = w.core(id);
                let pos = Vec2::new(core.left() + core.width() / 2.0, s.target.y + 30.0);
                let range = Vec2::new(core.width() / 2.0 + 4.0, 0.0);
                particles(w, "SensorBlockDust", pos, 12, range, -PI / 2.0);
                particles(w, "SensorBlockDust2", pos, 12, range, -PI / 2.0);
            }
            83 => sound(w, "env_crusherRise", x),
            84..=143 => {
                let e = Ease::CubeInOut.apply((k - 83) as f32 / 60.0);
                move_to(w, c, id, lerp_v(s.target, s.start, e), false);
            }
            157 => {
                s.seq = -2;
                s.squisher = -1;
                w.core_mut(id).squisher_index = -1;
            }
            _ => {}
        }
        s.glow = if (0..131).contains(&k) {
            crate::math::min(1.0, (k + 1) as f32 / 20.0)
        } else if (131..151).contains(&k) {
            1.0 - (k - 130) as f32 / 20.0
        } else {
            0.0
        };
    }
    if s.seq == -2
        && s.can_sense
        && let Some(who) = sensor_detect(w, c, id, s)
    {
        s.seq = -1;
        s.squisher = who;
        w.core_mut(id).squisher_index = who;
    }
}
