//! Hazards: lava, spike ball, icicle, hazard orb (tower-entities.md §8).
//! The Versus miasma is `crate::versus::miasma`.

use super::{
    EnvEvent, Placement, Tower, on_lava_collide, particles, sound, tm, tower, with_player,
    with_tower,
};
use crate::actor::{self, wrap_dist_sq};
use crate::arrow::{self, Arrow};
use crate::collide::{Collider, Hitbox, Placed};
use crate::components::Ease;
use crate::content::Content;
use crate::entity::{Core, EntityId, Kind, Tag};
use crate::events::Event;
use crate::impl_hash;
use crate::math::{PI, Vec2, approach, clamp, cos, min, sin};
use crate::player::{self, DeathCause, Player};
use crate::world::World;

pub fn rumble_pub(w: &mut World, player: i8, strength: f32, frames: u16) {
    rumble(w, player, strength, frames);
}

fn rumble(w: &mut World, player: i8, strength: f32, frames: u16) {
    if player >= 0 {
        w.emit(Event::Rumble {
            player: player as u8,
            strength,
            frames,
        });
    }
}

// ------------------------------------------------------------ §8.1 Lava

/// Lava side.
pub const BOTTOM: u8 = 0;
pub const TOP: u8 = 1;
pub const LEFT: u8 = 2;
pub const RIGHT: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LavaControl {
    pub owner: i8,
    /// Untimed (AlwaysLava variant).
    pub variant: bool,
    /// Resting offset: 15 risen, -50 hidden.
    pub target: f32,
    pub duration: f32,
    pub expired: bool,
    pub lavas: [Option<EntityId>; 4],
    /// `LavaMode.CataclysmBoss`: one bottom slab at target 25, untimed;
    /// `expired` = `BossFinish` was called.
    pub boss: bool,
}
impl_hash!(LavaControl {
    owner,
    variant,
    target,
    duration,
    expired,
    lavas,
    boss
});

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lava {
    pub side: u8,
    pub control: EntityId,
    /// 120-frame sine counter (`sin(c/2)` bobs the target).
    pub sine: f32,
    pub percent: f32,
}
impl_hash!(Lava {
    side,
    control,
    sine,
    percent
});

/// Extra side reach beyond the Steam build's 30 px: 0 @ 320, 20 @ 420 (a
/// linear rule for other widths; §16.5).
fn side_extra(width: f32) -> f32 {
    (width - 320.0) / 5.0
}

pub fn start_lava(w: &mut World, c: &Content, owner: i8, variant: bool) {
    if let Some(l) = w.tower.lava
        && w.exists(l)
    {
        let r = w.rng.range_i(540, 300) as f32;
        with_tower(w, l, |_, t| {
            if let Tower::LavaControl(lc) = t {
                lc.owner = owner;
                lc.target = 15.0;
                if !lc.variant {
                    lc.duration = r;
                    lc.expired = false;
                }
            }
        });
        return;
    }
    // The pickup duration is drawn in the constructor, before the layout.
    let duration = if variant {
        0.0
    } else {
        w.rng.range_i(600, 300) as f32
    };
    let lvl = c.level(w.level);
    let rows = lvl.cells_y as i32;
    let full = |y: i32| (0..lvl.cells_x as i32).all(|x| lvl.solid(x, y));
    let sides: &[u8] = if full(1) && full(rows - 2) {
        &[LEFT, RIGHT]
    } else if w.rng.chance(0.001) {
        &[LEFT, RIGHT, TOP, BOTTOM]
    } else if w.rng.chance(0.5) {
        &[LEFT, RIGHT]
    } else {
        &[TOP, BOTTOM]
    };
    let mut core = Core::new(Vec2::ZERO, Collider::None, 0, &[]);
    core.visible = false;
    core.collidable = false;
    let ctl = w.add(
        core,
        Kind::Tower(Tower::LavaControl(LavaControl {
            owner,
            variant,
            target: 15.0,
            duration,
            expired: false,
            lavas: [None; 4],
            boss: false,
        })),
    );
    let ws = w.world_size(c);
    let mut lavas = [None; 4];
    for (k, &side) in sides.iter().enumerate() {
        let (pos, hb) = match side {
            BOTTOM => (Vec2::new(0.0, 290.0), Hitbox::new(ws.w, 100.0, 0.0, 0.0)),
            TOP => (Vec2::new(0.0, -50.0), Hitbox::new(ws.w, 100.0, 0.0, -100.0)),
            LEFT => (
                Vec2::new(-50.0, 0.0),
                Hitbox::new(100.0, 240.0, -100.0, 0.0),
            ),
            _ => (
                Vec2::new(ws.w + 50.0, 0.0),
                Hitbox::new(100.0, 240.0, 0.0, 0.0),
            ),
        };
        let core = Core::new(pos, Collider::Box(hb), -500, &[Tag::LightSource]);
        lavas[k] = Some(w.add(
            core,
            Kind::Tower(Tower::Lava(Lava {
                side,
                control: ctl,
                sine: 0.0,
                percent: 0.0,
            })),
        ));
    }
    if let Some(Tower::LavaControl(lc)) = super::tower_mut(w, ctl) {
        lc.lavas = lavas;
    }
    w.tower.lava = Some(ctl);
    w.emit(Event::Env(EnvEvent::Loop {
        name: "sfx_lavaLoop",
        owner: ctl,
        on: true,
        volume: 0.0,
    }));
}

pub fn update_lava_control(w: &mut World, _c: &Content, id: EntityId, lc: &mut LavaControl) {
    let p0 = lc.lavas[0]
        .and_then(|l| match tower(w, l) {
            Some(Tower::Lava(l)) => Some(l.percent),
            _ => None,
        })
        .unwrap_or(0.0);
    w.emit(Event::Env(EnvEvent::Loop {
        name: "sfx_lavaLoop",
        owner: id,
        on: true,
        volume: clamp(p0, 0.0, 1.0),
    }));
    if lc.boss {
        let y = lc.lavas[0].map_or(999.0, |l| w.core(l).pos.y);
        if lc.expired && y >= 240.0 - lc.target {
            for l in lc.lavas.iter().flatten() {
                w.remove(*l);
            }
            w.remove(id);
            w.tower.lava = None;
            w.emit(Event::Env(EnvEvent::Loop {
                name: "sfx_lavaLoop",
                owner: id,
                on: false,
                volume: 0.0,
            }));
        }
        return;
    }
    if lc.variant {
        return;
    }
    if !lc.expired {
        lc.duration -= tm(w);
        if lc.duration <= 0.0 {
            lc.expired = true;
            lc.target = -50.0;
        }
    } else if p0 <= 0.0 {
        for l in lc.lavas.iter().flatten() {
            w.remove(*l);
        }
        w.remove(id);
        w.tower.lava = None;
        w.emit(Event::Env(EnvEvent::Loop {
            name: "sfx_lavaLoop",
            owner: id,
            on: false,
            volume: 0.0,
        }));
    }
}

fn lava_fire_box(side: u8, ws_w: f32) -> Hitbox {
    match side {
        BOTTOM => Hitbox::new(ws_w, 104.0, 0.0, -4.0),
        TOP => Hitbox::new(ws_w, 104.0, 0.0, -100.0),
        LEFT => Hitbox::new(108.0, 240.0, -100.0, 0.0),
        _ => Hitbox::new(108.0, 240.0, -8.0, 0.0),
    }
}

pub fn update_lava(w: &mut World, c: &Content, id: EntityId, l: &mut Lava) {
    let t = tm(w);
    let ws = w.world_size(c);
    let extra = side_extra(ws.w);
    let (target_t, owner) = match tower(w, l.control) {
        Some(Tower::LavaControl(lc)) => (lc.target, lc.owner),
        _ => (-50.0, -1),
    };
    l.sine = (l.sine + 2.0 * PI / 120.0 * t) % (PI * 8.0);
    let s = sin(l.sine / 2.0);
    let core = w.core_mut(id);
    match l.side {
        BOTTOM => core.pos.y = approach(core.pos.y, 243.0 - target_t + s, 0.35 * t),
        TOP => core.pos.y = approach(core.pos.y, target_t + s, 0.35 * t),
        LEFT => core.pos.x = approach(core.pos.x, target_t + 15.0 + extra + s, 0.35 * t),
        _ => core.pos.x = approach(core.pos.x, ws.w - 15.0 - extra - target_t + s, 0.35 * t),
    }
    let p = core.pos;
    l.percent = match l.side {
        TOP => (p.y + 50.0) / 65.0,
        LEFT => (p.x + 35.0 - extra) / 65.0,
        _ => 0.0,
    };
    let kills = match l.side {
        BOTTOM => p.y < 230.0,
        TOP => p.y > 10.0,
        LEFT => p.x > 10.0,
        _ => p.x < ws.w - 10.0,
    };
    core.collidable = kills;
    let push = match l.side {
        BOTTOM => Vec2::new(0.0, -1.0),
        TOP => Vec2::new(0.0, 1.0),
        LEFT => Vec2::new(1.0, 0.0),
        _ => Vec2::new(-1.0, 0.0),
    };
    // Heat shimmer along the surface (cosmetic).
    let kind = match l.side {
        BOTTOM => "LavaHeatBottom",
        TOP => "LavaHeatTop",
        LEFT => "LavaHeatLeft",
        _ => "LavaHeatRight",
    };
    let surf = match l.side {
        BOTTOM => Vec2::new(ws.w / 2.0, p.y),
        TOP => Vec2::new(ws.w / 2.0, p.y),
        LEFT => Vec2::new(p.x, ws.h / 2.0),
        _ => Vec2::new(p.x, ws.h / 2.0),
    };
    let range = if l.side <= TOP {
        Vec2::new(ws.w / 2.0, 0.0)
    } else {
        Vec2::new(0.0, ws.h / 2.0)
    };
    particles(w, kind, surf, 1, range, push.angle());
    let me = w.placed(id);
    if kills {
        for pid in w.all(c, &me, Tag::Player, id) {
            with_player(w, pid, |w, pl| {
                if !pl.dead {
                    player::hurt_lava(w, c, pid, pl, push, owner);
                }
            });
        }
        for a in w.all(c, &me, Tag::Arrow, id) {
            // `HitLava()` per arrow type (arrows.md §3.10).
            crate::arrows::with_arrow(w, a, |w, ar| crate::arrows::hit_lava(w, c, a, ar));
        }
        for h in w.all(c, &me, Tag::Hat, id) {
            sound(w, "sfx_burn", w.core(h).pos.x);
            w.remove(h);
        }
    }
    let fire = Placed {
        pos: p,
        collider: Collider::Box(lava_fire_box(l.side, ws.w)),
    };
    for pid in w.all(c, &fire, Tag::Player, id) {
        if let Some(pl) = w.player(pid).copied()
            && !pl.dead
        {
            super::start_fire(w, &pl);
        }
    }
    for e in w.all(c, &fire, Tag::LavaCollider, id) {
        on_lava_collide(w, c, e, l.side);
    }
    for b in w.all(c, &fire, Tag::Brambles, id) {
        crate::arrows::brambles::ignite(w, b);
    }
    for cp in w.all(c, &fire, Tag::Corpse, id) {
        crate::corpse::lava_collide(w, cp);
    }
}

// ------------------------------------------------------------ §8.5 Spikeball

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spikeball {
    pub pivot: Vec2,
    pub radius: f32,
    pub phase: f32,
    pub spin_dir: f32,
    pub spin_rate: f32,
    pub explodes: bool,
    /// Cosmetic art jitter frames.
    pub jitter: f32,
}
impl_hash!(Spikeball {
    pivot,
    radius,
    phase,
    spin_dir,
    spin_rate,
    explodes
});

pub fn spawn_spikeball(w: &mut World, p: &Placement) -> EntityId {
    let pivot = p.node.unwrap_or(p.pos + Vec2::new(0.0, 40.0));
    let radius = (p.pos - pivot).length();
    let tags: &[Tag] = if p.flag {
        &[Tag::PlayerCollider, Tag::Target, Tag::LightSource]
    } else {
        &[Tag::PlayerCollider, Tag::Target]
    };
    let core = Core::new(
        pivot + Vec2::new(0.0, -radius),
        Collider::Box(Hitbox::wrapping(14.0, 14.0, -7.0, -7.0)),
        -160,
        tags,
    );
    // The original's spin direction comes from an unseeded generator; ours
    // from the sim PRNG (§1 Randomness).
    let spin_dir = if w.rng.chance(0.5) { 1.0 } else { -1.0 };
    w.add(
        core,
        Kind::Tower(Tower::Spikeball(Spikeball {
            pivot,
            radius,
            phase: 0.0,
            spin_dir,
            spin_rate: 1.0,
            explodes: p.flag,
            jitter: 0.0,
        })),
    )
}

pub fn spikeball_pos(s: &Spikeball) -> Vec2 {
    let p = s.phase;
    let a = 2.0 * PI * (0.4 * Ease::CubeInOut.apply(p) + 0.6 * p);
    let ang = if s.spin_dir >= 0.0 {
        a - PI / 2.0
    } else {
        -PI / 2.0 - a
    };
    s.pivot + Vec2::new(cos(ang), sin(ang)) * s.radius
}

pub fn update_spikeball(w: &mut World, c: &Content, id: EntityId, s: &mut Spikeball) {
    let t = tm(w);
    // Enemies touching the ball: 1 damage, pushed 2 away (no credit); the
    // exploding ball goes off on the first.
    let me = w.placed(id);
    for e in w.all(c, &me, Tag::Enemy, id) {
        if !matches!(w.kind(e), Some(Kind::Enemy(_))) {
            continue;
        }
        let push = (w.core(e).pos - w.core(id).pos).safe_normalize_len(2.0);
        crate::enemies::hurt_by(w, c, e, push, 1, -1);
        if s.explodes {
            spikeball_explode(w, c, id, s, -1, None);
            return;
        }
    }
    if s.jitter > 0.0 {
        s.jitter -= t;
    }
    if s.explodes && w.tower.round_started {
        let since = w.frame.saturating_sub(w.tower.round_start_frame) as f32;
        s.spin_rate = 1.0 + clamp((since - 300.0) / 600.0, 0.0, 1.0);
    }
    let before = s.phase;
    s.phase += s.spin_rate / 180.0 * t;
    if s.phase >= 1.0 {
        s.phase -= 1.0;
    }
    if before < 0.35 && s.phase >= 0.35 {
        let name = if s.explodes {
            "env_ballChainGreen"
        } else {
            "env_ballChain"
        };
        sound(w, name, w.core(id).pos.x);
    }
    w.core_mut(id).pos = spikeball_pos(s);
    if s.explodes {
        particles(
            w,
            "ExplodingOrbGlow",
            w.core(id).pos,
            1,
            Vec2::new(6.0, 6.0),
            0.0,
        );
    }
}

fn spikeball_explode(
    w: &mut World,
    c: &Content,
    id: EntityId,
    s: &Spikeball,
    culprit: i8,
    held: Option<(EntityId, &mut Player)>,
) {
    let at = w.core(id).pos;
    sound(w, "pu_bombArrowExplode", at.x);
    particles(
        w,
        "SpikeballChain",
        s.pivot,
        (s.radius / 8.0) as u16,
        Vec2::new(2.0, 2.0),
        -PI / 2.0,
    );
    w.remove(id);
    super::explosion::spawn(w, c, at, culprit, false, false, held);
}

pub fn spikeball_player(
    w: &mut World,
    c: &Content,
    id: EntityId,
    s: &mut Spikeball,
    pid: EntityId,
    p: &mut Player,
) {
    let at = w.core(id).pos;
    player::hurt(
        w,
        c,
        pid,
        p,
        DeathCause::SpikeBall,
        at,
        -1,
        Some("env_deathBallChain"),
    );
    if s.explodes && !w.core(id).marked_for_removal {
        let held = if p.dead { None } else { Some((pid, p)) };
        spikeball_explode(w, c, id, s, w.player_index_of(pid, held.as_ref()), held);
    }
}

pub fn spikeball_arrow(
    w: &mut World,
    c: &Content,
    id: EntityId,
    s: &mut Spikeball,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    if s.explodes {
        if !w.core(id).marked_for_removal {
            spikeball_explode(w, c, id, s, a.player_index, None);
        }
    } else {
        s.jitter = 30.0;
    }
    arrow::enter_fall_mode(w, aid, a, true, false, true);
    true
}

// ------------------------------------------------------------ §8.6 Icicle

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IcicleState {
    Hanging,
    Shivering,
    Falling,
}
crate::impl_hash_enum!(IcicleState);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Icicle {
    pub state: IcicleState,
    pub timer: f32,
    pub steps: u8,
    pub v: f32,
    pub owner: i8,
    pub cannot_hit: Option<EntityId>,
    pub cannot_hit_arrow: Option<EntityId>,
    pub fall_frames: f32,
    /// Cosmetic shiver offset.
    pub shift: f32,
}
impl_hash!(Icicle {
    state,
    timer,
    steps,
    v,
    owner,
    cannot_hit,
    cannot_hit_arrow,
    fall_frames
});

pub fn spawn_icicle(w: &mut World, p: &Placement) -> EntityId {
    let mut core = Core::new(
        p.pos,
        Collider::Box(Hitbox::wrapping(8.0, 12.0, -4.0, 0.0)),
        0,
        &[
            Tag::PlayerCollider,
            Tag::ExplosionCollider,
            Tag::PlayerGhostCollider,
            Tag::Target,
        ],
    );
    core.target_collider = Some(Hitbox::wrapping(12.0, 16.0, -6.0, 0.0));
    core.screen_wrap = true;
    w.add(
        core,
        Kind::Tower(Tower::Icicle(Icicle {
            state: IcicleState::Hanging,
            timer: 0.0,
            steps: 0,
            v: 0.0,
            owner: -1,
            cannot_hit: None,
            cannot_hit_arrow: None,
            fall_frames: 0.0,
            shift: 0.0,
        })),
    )
}

fn icicle_trigger(w: &mut World, id: EntityId, i: &mut Icicle, owner: i8) {
    if i.state != IcicleState::Hanging {
        return;
    }
    i.state = IcicleState::Shivering;
    i.owner = owner;
    i.timer = 3.0;
    i.steps = 0;
    rumble(w, owner, 0.5, 10);
    sound(w, "sfx_icicleTouch", w.core(id).pos.x);
}

fn icicle_explode(w: &mut World, id: EntityId) {
    if w.core(id).marked_for_removal {
        return;
    }
    let at = w.core(id).pos + Vec2::new(0.0, 6.0);
    sound(w, "sfx_icicleHit", at.x);
    particles(w, "IcicleBreak", at, 12, Vec2::new(8.0, 8.0), 0.0);
    particles(w, "BombParticle:Icicle", at, 6, Vec2::new(4.0, 4.0), 0.0);
    w.remove(id);
}

pub fn update_icicle(w: &mut World, c: &Content, id: EntityId, i: &mut Icicle) {
    let t = tm(w);
    match i.state {
        IcicleState::Hanging => {}
        IcicleState::Shivering => {
            let at = w.core(id).pos + Vec2::new(0.0, 6.0);
            particles(w, "IcicleShift", at, 1, Vec2::new(3.0, 4.0), PI / 2.0);
            i.timer -= t;
            if i.timer <= 0.0 {
                i.steps += 1;
                i.timer = 3.0;
                i.shift = if i.steps == 1 {
                    if w.cosmetic_rng.chance(0.5) {
                        1.0
                    } else {
                        -1.0
                    }
                } else {
                    -i.shift
                };
                if i.steps >= 5 {
                    i.state = IcicleState::Falling;
                    i.shift = 0.0;
                }
            }
        }
        IcicleState::Falling => {
            i.v = min(i.v + 0.3 * t, 5.0);
            w.core_mut(id).pos.y += i.v * t;
            i.fall_frames += t;
            let me = w.placed(id);
            if w.any(c, &me, Tag::Solid, id) {
                icicle_explode(w, id);
                return;
            }
            // An enemy below (not `cannotHit`) is hurt; a chest opens.
            if let Some(e) = w
                .all(c, &me, Tag::Enemy, id)
                .into_iter()
                .find(|&e| Some(e) != i.cannot_hit && matches!(w.kind(e), Some(Kind::Enemy(_))))
            {
                crate::enemies::hurt_by(w, c, e, Vec2::new(0.0, 3.0), 1, i.owner);
                icicle_explode(w, id);
                return;
            }
            if let Some(ch) = w.first(c, &me, Tag::TreasureChest, id) {
                crate::treasure::open_by(w, c, ch, i.owner);
                icicle_explode(w, id);
                return;
            }
            if i.fall_frames > 20.0
                && let Some(ch) = i.cannot_hit
                && (!w.exists(ch) || !w.check(c, &me, ch))
            {
                i.cannot_hit = None;
            }
        }
    }
}

pub fn icicle_player(
    w: &mut World,
    c: &Content,
    id: EntityId,
    i: &mut Icicle,
    pid: EntityId,
    p: &mut Player,
) {
    match i.state {
        IcicleState::Hanging => {
            i.cannot_hit = Some(pid);
            icicle_trigger(w, id, i, p.index as i8);
        }
        IcicleState::Falling => {
            if i.cannot_hit == Some(pid) {
                return;
            }
            let core = w.core(id);
            let prev_bottom = core.bottom() - i.v;
            let py = w.core(pid).pos.y - p.speed.y - 2.0;
            if prev_bottom <= py && player::can_hurt(w, pid, p) {
                let at = w.core(id).pos;
                player::hurt(w, c, pid, p, DeathCause::FallingObject, at, i.owner, None);
                icicle_explode(w, id);
            }
        }
        IcicleState::Shivering => {}
    }
}

pub fn icicle_arrow(
    w: &mut World,
    _c: &Content,
    id: EntityId,
    i: &mut Icicle,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    match i.state {
        IcicleState::Hanging | IcicleState::Shivering => {
            if i.cannot_hit_arrow == Some(aid) {
                return false;
            }
            i.cannot_hit_arrow = Some(aid);
            arrow::enter_fall_mode(w, aid, a, true, false, true);
            icicle_trigger(w, id, i, a.player_index);
            true
        }
        IcicleState::Falling => {
            if i.cannot_hit_arrow == Some(aid) {
                return false;
            }
            arrow::enter_fall_mode(w, aid, a, true, false, true);
            icicle_explode(w, id);
            true
        }
    }
}

pub fn icicle_explode_push(w: &mut World, _c: &Content, id: EntityId, i: &mut Icicle, owner: i8) {
    match i.state {
        IcicleState::Falling => icicle_explode(w, id),
        _ => icicle_trigger(w, id, i, owner),
    }
}

// ------------------------------------------------------------ §8.7 hazard Orb

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orb {
    pub start_y: f32,
    pub sine: f32,
    pub falling: bool,
    pub v: f32,
    pub owner: i8,
    pub cannot_hit: Option<EntityId>,
    pub explodes: bool,
    pub plus_one: bool,
}
impl_hash!(Orb {
    start_y,
    sine,
    falling,
    v,
    owner,
    cannot_hit,
    explodes,
    plus_one
});

pub fn spawn_orb(w: &mut World, p: &Placement, explodes: bool) -> EntityId {
    let mut core = Core::new(
        p.pos,
        Collider::Box(Hitbox::wrapping(12.0, 8.0, -6.0, -4.0)),
        0,
        &[
            Tag::Actor,
            Tag::Orb,
            Tag::PlayerCollider,
            Tag::ExplosionCollider,
            Tag::LightSource,
            Tag::PlayerGhostCollider,
            Tag::Target,
        ],
    );
    core.target_collider = Some(Hitbox::wrapping(12.0, 12.0, -6.0, -6.0));
    core.screen_wrap = true;
    // Random hover phase: it moves the orb, so it comes from the sim PRNG.
    let sine = w.rng.next_float() * PI * 4.0;
    w.add(
        core,
        Kind::Tower(Tower::Orb(Orb {
            start_y: p.pos.y,
            sine,
            falling: false,
            v: 0.0,
            owner: -1,
            cannot_hit: None,
            explodes,
            plus_one: false,
        })),
    )
}

/// `Fall(dropper)`.
pub fn orb_fall(w: &mut World, _id: EntityId, o: &mut Orb, owner: i8) {
    if o.falling {
        return;
    }
    o.falling = true;
    o.owner = owner;
    let _ = w;
}

/// `Orb.Shatter()` from outside (a dummy touched it).
pub fn shatter_orb(w: &mut World, c: &Content, id: EntityId) {
    if let Some(Kind::Tower(Tower::Orb(o))) = w.kind(id).copied() {
        orb_shatter(w, c, id, &o, None);
    }
}

fn orb_shatter(
    w: &mut World,
    c: &Content,
    id: EntityId,
    o: &Orb,
    held: Option<(EntityId, &mut Player)>,
) {
    if w.core(id).marked_for_removal {
        return;
    }
    let at = w.core(id).pos;
    w.core_mut(id).collidable = false;
    w.remove(id);
    if o.explodes {
        sound(w, "pu_bombArrowExplode", at.x);
        super::explosion::spawn(w, c, at, o.owner, o.plus_one, false, held);
    } else {
        sound(w, "env_ballCrash", at.x);
        particles(w, "OrbShatter", at, 12, Vec2::new(6.0, 6.0), 0.0);
    }
}

pub fn update_orb(w: &mut World, c: &Content, id: EntityId, o: &mut Orb) {
    let t = tm(w);
    if w.time.on_interval(6) {
        let kind = if o.explodes {
            "ExplodingOrbGlow"
        } else {
            "OrbGlow"
        };
        particles(w, kind, w.core(id).pos, 1, Vec2::new(6.0, 6.0), 0.0);
    }
    if !o.falling {
        o.sine = (o.sine + 2.0 * PI / 80.0 * t) % (PI * 8.0);
        w.core_mut(id).pos.y = o.start_y + 2.0 * sin(o.sine);
        actor::enforce_screen_wrap(w, c, id);
        return;
    }
    o.v = min(o.v + 0.2 * t, 4.0);
    if actor::move_v(w, c, id, o.v * t).is_some() {
        orb_shatter(w, c, id, o, None);
        return;
    }
    actor::enforce_screen_wrap(w, c, id);
    let me = w.placed(id);
    if let Some(other) = w.first(c, &me, Tag::Orb, id) {
        let owner = o.owner;
        with_tower(w, other, |w, t| {
            if let Tower::Orb(oo) = t {
                orb_fall(w, other, oo, owner);
            }
        });
        orb_shatter(w, c, id, o, None);
        return;
    }
    if crate::enemies::orb_touch(w, c, id, o.cannot_hit, o.owner) {
        orb_shatter(w, c, id, o, None);
        return;
    }
    let me = w.placed(id);
    if let Some(ch) = w.first(c, &me, Tag::TreasureChest, id) {
        orb_shatter(w, c, id, o, None);
        crate::treasure::open_by(w, c, ch, o.owner);
        return;
    }
    if let Some(ch) = o.cannot_hit {
        let ws = w.world_size(c);
        let far = w
            .try_core(ch)
            .is_none_or(|cc| wrap_dist_sq(w.core(id).pos, cc.pos, ws.w, ws.h) > 400.0);
        if far {
            o.cannot_hit = None;
        }
    }
}

/// A player ghost touching a hanging orb: it falls, owned by the ghost.
pub fn orb_ghost(w: &mut World, id: EntityId, o: &mut Orb, ghost: EntityId, owner: i8) {
    if o.falling {
        return;
    }
    o.cannot_hit = Some(ghost);
    orb_fall(w, id, o, owner);
    rumble(w, owner, 0.5, 10);
    let name = if o.explodes {
        "env_ballTouchGreenJump"
    } else {
        "env_ballTouchJump"
    };
    sound(w, name, w.core(id).pos.x);
}

/// A player ghost touching a hanging icicle: it starts to fall.
pub fn icicle_ghost(w: &mut World, id: EntityId, i: &mut Icicle, ghost: EntityId, owner: i8) {
    if i.state == IcicleState::Hanging {
        i.cannot_hit = Some(ghost);
        icicle_trigger(w, id, i, owner);
    }
}

pub fn orb_player(
    w: &mut World,
    c: &Content,
    id: EntityId,
    o: &mut Orb,
    pid: EntityId,
    p: &mut Player,
) {
    if w.core(id).marked_for_removal {
        return;
    }
    if !o.falling {
        let orb_y = w.core(id).pos.y;
        let top = w.core(id).top();
        let pc = *w.core(pid);
        if pc.bottom() - p.speed.y <= orb_y + 6.0 && p.speed.y >= 0.0 {
            player::stomp_bounce(w, c, pid, p, top);
        } else {
            player::stomp_bounce(w, c, pid, p, pc.bottom());
        }
        o.cannot_hit = Some(pid);
        rumble(w, p.index as i8, 0.5, 10);
        let name = if o.explodes {
            "env_ballTouchGreenJump"
        } else {
            "env_ballTouchJump"
        };
        sound(w, name, w.core(id).pos.x);
        orb_fall(w, id, o, p.index as i8);
        return;
    }
    if o.cannot_hit == Some(pid) {
        return;
    }
    if w.core(pid).pos.y > w.core(id).pos.y + 2.0 && player::can_hurt(w, pid, p) {
        let at = w.core(id).pos;
        player::hurt(w, c, pid, p, DeathCause::FallingObject, at, o.owner, None);
        o.plus_one = true;
        let held = if p.dead { None } else { Some((pid, p)) };
        orb_shatter(w, c, id, o, held);
    }
}

pub fn orb_arrow(
    w: &mut World,
    c: &Content,
    id: EntityId,
    o: &mut Orb,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    arrow::enter_fall_mode(w, aid, a, true, false, true);
    if o.falling {
        o.owner = a.player_index;
        orb_shatter(w, c, id, o, None);
    } else {
        let name = if o.explodes {
            "env_ballTouchGreenArrow"
        } else {
            "env_ballTouchArrow"
        };
        sound(w, name, w.core(id).pos.x);
        orb_fall(w, id, o, a.player_index);
    }
    true
}

impl World {
    /// Player index of `pid`, also when it is the held (updating) archer.
    pub(crate) fn player_index_of(
        &self,
        pid: EntityId,
        held: Option<&(EntityId, &mut Player)>,
    ) -> i8 {
        if let Some((h, p)) = held
            && *h == pid
        {
            return p.index as i8;
        }
        self.player(pid).map(|p| p.index as i8).unwrap_or(-1)
    }
}

/// `LavaControl(CataclysmBoss)`: a single bottom lava rising to target 25
/// (darkworld.md §7.4, Legendary finale D'').
pub fn start_boss_lava(w: &mut World, c: &Content) -> EntityId {
    let mut core = Core::new(Vec2::ZERO, Collider::None, 0, &[]);
    core.visible = false;
    core.collidable = false;
    let ctl = w.add(
        core,
        Kind::Tower(Tower::LavaControl(LavaControl {
            owner: -1,
            variant: true,
            target: 25.0,
            duration: 0.0,
            expired: false,
            lavas: [None; 4],
            boss: true,
        })),
    );
    let ws = w.world_size(c);
    let core = Core::new(
        Vec2::new(0.0, 290.0),
        Collider::Box(Hitbox::new(ws.w, 100.0, 0.0, 0.0)),
        -500,
        &[Tag::LightSource],
    );
    let lava = w.add(
        core,
        Kind::Tower(Tower::Lava(Lava {
            side: BOTTOM,
            control: ctl,
            sine: 0.0,
            percent: 0.0,
        })),
    );
    if let Some(Tower::LavaControl(lc)) = super::tower_mut(w, ctl) {
        lc.lavas[0] = Some(lava);
    }
    w.tower.lava = Some(ctl);
    ctl
}

/// `LavaControl.BossFinish()`: the boss lava recedes and goes.
pub fn boss_lava_finish(w: &mut World, ctl: EntityId) {
    if let Some(Tower::LavaControl(lc)) = super::tower_mut(w, ctl) {
        lc.expired = true;
        lc.target = -50.0;
    }
}
