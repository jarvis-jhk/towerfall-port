//! Reactive scenery and characters (tower-entities.md §9, §10).

use super::{Placement, PlacementKind, ThemeInfo, Tower, particles, sound, tm, tower, tower_mut};
use crate::actor;
use crate::arrow::{self, Arrow};
use crate::collide::{Collider, Hitbox, Placed};
use crate::content::Content;
use crate::entity::{Core, EntityId, Kind, Tag};
use crate::impl_hash;
use crate::math::{PI, Vec2, approach, clamp, cos, floor, max, min, sin};
use crate::player::{self, PState, Player};
use crate::world::World;

crate::impl_hash_enum!(PlacementKind);

// ------------------------------------------------------------ §9.1 Lantern

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lantern {
    pub chain: Option<EntityId>,
    pub fallen: bool,
    pub dead: bool,
    pub v: f32,
    pub rotation: f32,
}
impl_hash!(Lantern {
    chain,
    fallen,
    dead,
    v,
    rotation
});

pub fn spawn_lantern(w: &mut World, p: &Placement) -> EntityId {
    let mut core = Core::new(
        p.pos,
        Collider::Box(Hitbox::wrapping(6.0, 10.0, -3.0, 0.0)),
        109,
        &[
            Tag::Actor,
            Tag::LightSource,
            Tag::ExplosionCollider,
            Tag::Target,
            Tag::PlayerCollider,
        ],
    );
    core.screen_wrap = true;
    w.add(
        core,
        Kind::Tower(Tower::Lantern(Lantern {
            chain: None,
            fallen: false,
            dead: false,
            v: 0.0,
            rotation: 0.0,
        })),
    )
}

/// The chain whose box overlaps `id`'s box shifted 10 px up.
fn chain_above(w: &World, c: &Content, id: EntityId) -> Option<EntityId> {
    let core = w.core(id);
    let probe = Placed {
        pos: core.pos + Vec2::new(0.0, -10.0),
        collider: core.collider,
    };
    w.first(c, &probe, Tag::Chain, id)
}

pub fn lantern_begin(w: &mut World, c: &Content, id: EntityId, l: &mut Lantern) {
    if let Some(ch) = chain_above(w, c, id) {
        l.chain = Some(ch);
        if let Some(Tower::Chain(chain)) = tower_mut(w, ch) {
            chain.holding = Some(id);
        }
    }
}

/// `Fall()`.
pub fn lantern_fall(w: &mut World, id: EntityId, l: &mut Lantern) {
    if l.fallen {
        return;
    }
    l.fallen = true;
    if let Some(ch) = l.chain.take()
        && let Some(Tower::Chain(chain)) = tower_mut(w, ch)
    {
        chain.holding = None;
    }
    w.core_mut(id).collidable = false;
    w.tag(id, Tag::ExplosionCollider, false);
    w.tag(id, Tag::PlayerCollider, false);
    w.tag(id, Tag::Target, false);
    sound(w, "env_lampFall", w.core(id).pos.x);
}

pub fn lantern_arrow(
    w: &mut World,
    id: EntityId,
    l: &mut Lantern,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    if l.fallen {
        return false;
    }
    if let Some(ch) = l.chain {
        let y = w.core(aid).pos.y;
        chain_push_id(w, ch, y, a.speed.x * 2.0);
    }
    arrow::enter_fall_mode(w, aid, a, true, false, true);
    lantern_fall(w, id, l);
    true
}

pub fn update_lantern(w: &mut World, c: &Content, id: EntityId, l: &mut Lantern) {
    let t = tm(w);
    if !l.dead && w.time.on_interval(5) {
        let at = w.core(id).pos + Vec2::new(0.0, 8.0);
        particles(w, "BGTorch", at, 1, Vec2::new(1.0, 0.0), -PI / 2.0);
    }
    if !l.fallen {
        return;
    }
    if !actor::check_below(w, c, id) {
        l.v = min(l.v + 0.2 * t, 4.0);
        l.rotation = approach(l.rotation, -PI / 2.0, 5.0 * PI / 180.0 * t);
    }
    if actor::move_v(w, c, id, l.v * t).is_some() {
        l.v = 0.0;
        if !l.dead {
            l.dead = true;
            w.tag(id, Tag::LightSource, false);
        }
        sound(w, "env_lampCrash", w.core(id).pos.x);
    }
    actor::enforce_screen_wrap(w, c, id);
}

// ------------------------------------------------------------ §9.2 Chain

pub const CHAIN_LINKS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chain {
    pub n: u8,
    pub theta: [f32; CHAIN_LINKS],
    pub omega: [f32; CHAIN_LINKS],
    pub cooldown: f32,
    pub holding: Option<EntityId>,
    /// Cosmetic link variants (2 bits each).
    pub variants: u16,
}
impl_hash!(Chain {
    n,
    theta,
    omega,
    cooldown,
    holding
});

pub fn spawn_chain(w: &mut World, p: &Placement) -> EntityId {
    let h = if p.size.y > 0.0 { p.size.y } else { 10.0 };
    let n = ((h / 10.0) as usize).clamp(1, CHAIN_LINKS) as u8;
    let mut variants = 0u16;
    for i in 0..n {
        variants |= (w.cosmetic_rng.next_max(4) as u16) << (2 * i);
    }
    let core = Core::new(
        p.pos,
        Collider::Box(Hitbox::new(8.0, h, -4.0, 0.0)),
        0,
        &[Tag::Chain],
    );
    w.add(
        core,
        Kind::Tower(Tower::Chain(Chain {
            n,
            theta: [0.0; CHAIN_LINKS],
            omega: [0.0; CHAIN_LINKS],
            cooldown: 0.0,
            holding: None,
            variants,
        })),
    )
}

/// Link `i`'s top-left offset from the anchor, and the chain end.
pub fn chain_links(ch: &Chain) -> ([Vec2; CHAIN_LINKS], Vec2) {
    let mut out = [Vec2::ZERO; CHAIN_LINKS];
    let mut pos = Vec2::ZERO;
    for (slot, &th) in out.iter_mut().zip(&ch.theta).take(ch.n as usize) {
        *slot = pos;
        pos += Vec2::new(-sin(th) * 10.0, cos(th) * 10.0);
    }
    (out, pos)
}

fn chain_push(w: &mut World, id: EntityId, ch: &mut Chain, y: f32, s: f32) {
    sound(w, "sfx_chainRattle", w.core(id).pos.x);
    let mag = clamp(s.abs(), 2.2, 3.6);
    let s = if s < 0.0 { -mag } else { mag };
    ch.cooldown = 12.0;
    let a = -s * PI / 180.0;
    let n = ch.n as i32;
    let k = clamp(floor((y - w.core(id).pos.y) / 10.0), 0.0, (n - 1) as f32) as i32;
    for i in 0..n {
        let m = if i == k {
            1.0
        } else if i < k {
            max(0.0, 1.0 - 0.2 * (k - i) as f32)
        } else {
            1.0 + 0.2 * (i - k) as f32
        };
        ch.omega[i as usize] += a * m;
    }
}

pub fn chain_push_pub(w: &mut World, ch: EntityId, y: f32, s: f32) {
    chain_push_id(w, ch, y, s);
}

fn chain_push_id(w: &mut World, ch: EntityId, y: f32, s: f32) {
    super::with_tower(w, ch, |w, t| {
        if let Tower::Chain(c) = t {
            chain_push(w, ch, c, y, s);
        }
    });
}

pub fn update_chain(w: &mut World, c: &Content, id: EntityId, ch: &mut Chain) {
    let t = tm(w);
    for i in 0..ch.n as usize {
        ch.theta[i] += ch.omega[i] * t;
        ch.omega[i] -= ch.theta[i] * 0.02 * t;
        let step = if ch.theta[i].abs() < PI / 90.0 {
            PI / 360.0
        } else {
            0.004363323
        };
        ch.omega[i] = approach(ch.omega[i], -ch.theta[i], step * t);
    }
    if ch.cooldown > 0.0 {
        ch.cooldown -= t;
    } else {
        let me = w.placed(id);
        let mut push = None;
        for a in w.all(c, &me, Tag::Arrow, id) {
            if let Some(ar) = w.arrow(a)
                && ar.speed.x != 0.0
            {
                push = Some((w.core(a).pos.y, ar.speed.x));
                break;
            }
        }
        if push.is_none() {
            for p in w.all(c, &me, Tag::Player, id) {
                if let Some(pl) = w.player(p)
                    && pl.speed.x != 0.0
                {
                    push = Some((w.core(p).pos.y, pl.speed.x * 1.5));
                    break;
                }
            }
        }
        if let Some((y, s)) = push {
            chain_push(w, id, ch, y, s);
        }
    }
    if let Some(h) = ch.holding {
        if !w.exists(h) {
            ch.holding = None;
            return;
        }
        let (_, end) = chain_links(ch);
        let base = w.core(id).pos + end;
        let rot = ch.theta[ch.n as usize - 1];
        match w.kind_mut(h) {
            Some(Kind::Tower(Tower::Lantern(l))) => {
                l.rotation = rot;
                w.core_mut(h).pos = base;
            }
            Some(Kind::Tower(Tower::Dummy(_)) | Kind::Dummy(_)) => {
                w.core_mut(h).pos = base - Vec2::new(0.0, 6.0);
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------ §9.3 Cobwebs

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cobwebs {
    /// 0 above-left, 1 above-right, 2 below-left, 3 below-right.
    pub corner: u8,
    pub frame: u8,
    pub alpha: f32,
    /// Burn frames so far (`< 0`: not burning; removed after 15).
    pub burn: f32,
}
impl_hash!(Cobwebs { corner, burn });

/// A burning arrow passing through the web sets it alight (§9.3).
pub fn cobweb_arrow(
    w: &mut World,
    id: EntityId,
    cw: &mut Cobwebs,
    a: &crate::arrow::Arrow,
) -> bool {
    if a.extra.burning && cw.burn < 0.0 {
        cw.burn = 0.0;
        w.core_mut(id).active = true;
    }
    false
}

pub fn update_cobwebs(w: &mut World, id: EntityId, cw: &mut Cobwebs) {
    if cw.burn < 0.0 {
        return;
    }
    cw.burn += tm(w);
    let at = w.core(id).pos + Vec2::new(5.0, 5.0);
    particles(w, "Fire", at, 1, Vec2::new(5.0, 5.0), -PI / 2.0);
    if cw.burn >= 15.0 {
        w.remove(id);
    }
}

pub fn spawn_cobwebs(w: &mut World, c: &Content, p: &Placement) -> EntityId {
    let lvl = c.level(w.level);
    let s = |dx: f32, dy: f32| crate::collide::grid_point(lvl, p.pos + Vec2::new(dx, dy));
    let (above, below, left, right) = (s(0.0, -11.0), s(0.0, 11.0), s(-11.0, 0.0), s(11.0, 0.0));
    let corner = if above && left {
        0
    } else if above && right {
        1
    } else if below && left {
        2
    } else if below && right {
        3
    } else {
        0
    };
    let cell = p.pos
        + Vec2::new(
            if corner % 2 == 0 { -10.0 } else { 0.0 },
            if corner < 2 { -10.0 } else { 0.0 },
        );
    let frame = w.cosmetic_rng.next_max(3) as u8 + if corner < 2 { 0 } else { 3 };
    let alpha = 0.4 + 0.2 * w.cosmetic_rng.next_float();
    let mut core = Core::new(
        cell,
        Collider::Box(Hitbox::new(10.0, 10.0, 0.0, 0.0)),
        -110,
        &[Tag::Target],
    );
    core.active = false;
    w.add(
        core,
        Kind::Tower(Tower::Cobwebs(Cobwebs {
            corner,
            frame,
            alpha,
            burn: -1.0,
        })),
    )
}

// ------------------------------------------------------------ §9.4 BGTorch

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Torch {
    pub lit: bool,
    pub radius: f32,
    /// Flame direction (cosmetic) and its rest direction.
    pub dir: f32,
    pub rest: f32,
    pub lanterns: u8,
}
impl_hash!(Torch { lit, radius });

pub fn spawn_torch(w: &mut World, p: &Placement, theme: &ThemeInfo) -> EntityId {
    let mut core = Core::new(
        p.pos,
        Collider::Box(Hitbox::wrapping(10.0, 12.0, -5.0, -5.0)),
        0,
        &[Tag::LightSource, Tag::ExplosionCollider, Tag::BGTorch],
    );
    core.layer = -3;
    core.screen_wrap = true;
    let rest = -PI / 2.0 + PI / 3.0 * theme.wind;
    w.add(
        core,
        Kind::Tower(Tower::Torch(Torch {
            lit: p.flag,
            radius: if p.flag { 25.0 } else { 0.0 },
            dir: rest,
            rest,
            lanterns: theme.lanterns as u8,
        })),
    )
}

pub fn torch_relight(w: &mut World, _id: EntityId, t: &mut Torch) {
    let _ = w;
    t.lit = true;
    t.radius = 25.0;
}

pub fn update_torch(w: &mut World, c: &Content, id: EntityId, t: &mut Torch) {
    let tmul = tm(w);
    t.dir = approach(t.dir, t.rest, 2.0 * PI / 180.0 * tmul);
    let me = w.placed(id);
    if !t.lit {
        t.radius = max(0.0, t.radius - 0.8 * tmul);
        // A burning arrow relights it.
        for a in w.all(c, &me, Tag::Arrow, id) {
            if let Some(ar) = w.arrow(a).copied()
                && ar.extra.burning
            {
                t.lit = true;
                t.radius = 25.0;
                if !ar.speed.is_zero() {
                    t.dir = ar.speed.angle();
                }
                w.sound("sfx_burn", me.pos.x);
                break;
            }
        }
        return;
    }
    // Fire-capable arrows passing a lit torch catch fire.
    for a in w.all(c, &me, Tag::Arrow, id) {
        if let Some(ar) = w.arrow_mut(a)
            && crate::arrows::ignite(ar)
        {
            let v = ar.speed;
            if !v.is_zero() {
                t.dir = v.angle();
            }
        }
    }
    for p in w.all(c, &me, Tag::Player, id) {
        let Some(pl) = w.player(p) else { continue };
        if pl.state() == PState::Dodging || pl.is_hyper() {
            t.lit = false;
            return;
        }
        if !pl.speed.is_zero() {
            t.dir = pl.speed.angle();
        }
    }
    if w.time.on_interval(3) {
        let at = w.core(id).pos + Vec2::new(0.0, 2.0);
        particles(w, "BGTorch", at, 1, Vec2::new(3.0, 0.0), t.dir);
    }
}

// ------------------------------------------------- §9.5–9.12 decorations

/// Purely cosmetic placed art (crystals, wall mushrooms, ship windows,
/// banners, rain splashes, the broken prison, Ogmo).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decor {
    pub kind: PlacementKind,
    pub variant: u8,
    pub flip: bool,
    pub phase: f32,
}
// `variant` / `phase` are art picks from the cosmetic generator (not hashed).
impl_hash!(Decor { kind, flip });

pub fn spawn_decor(w: &mut World, p: &Placement) -> EntityId {
    use PlacementKind as K;
    let (layer, depth, tags): (i8, i32, &[Tag]) = match p.kind {
        K::BGCrystal | K::BGMushroom => (-3, 0, &[Tag::LightSource]),
        K::GhostShipWindow => (0, -160, &[Tag::LightSource]),
        K::PirateBanner if p.flag => (0, -159, &[]),
        K::PirateBanner => (-3, 10000, &[]),
        K::RainDrops => (0, -101, &[]),
        K::WhiteArcherPrison => (0, 122, &[]),
        _ => (0, 0, &[]),
    };
    let (pos, size) = match p.kind {
        // Unlock model: everything is unlocked, so the prison is always the
        // already-broken prop (§10.4, §16.1).
        K::WhiteArcherPrison => (p.pos + Vec2::new(2.0, 0.0), Vec2::new(30.0, 30.0)),
        K::RainDrops => (p.pos, Vec2::new(p.size.x.max(10.0), 10.0)),
        _ => (p.pos, Vec2::new(10.0, 10.0)),
    };
    let mut core = Core::new(
        pos,
        Collider::Box(Hitbox::new(size.x, size.y, 0.0, 0.0)),
        depth,
        tags,
    );
    core.layer = layer;
    core.active = false;
    core.collidable = false;
    core.screen_wrap = p.kind == K::GhostShipWindow;
    let variant = w.cosmetic_rng.next_max(2) as u8;
    let phase = w.cosmetic_rng.next_float() * PI * 2.0;
    w.add(
        core,
        Kind::Tower(Tower::Decor(Decor {
            kind: p.kind,
            variant,
            flip: false,
            phase,
        })),
    )
}

// ------------------------------------------------------------ §9.6 BGBigMushroom

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BigMushroom {
    /// Wiggler: counter 1 -> 0 over 90 frames, amplitude.
    pub counter: f32,
    pub amp: f32,
}
impl_hash!(BigMushroom { counter, amp });

pub fn spawn_big_mushroom(w: &mut World, p: &Placement) -> EntityId {
    let core = Core::new(
        p.pos,
        Collider::Box(Hitbox::new(8.0, 20.0, -4.0, -20.0)),
        0,
        &[Tag::PlayerCollider, Tag::PlayerGhostCollider, Tag::Target],
    );
    w.add(
        core,
        Kind::Tower(Tower::BigMushroom(BigMushroom {
            counter: 0.0,
            amp: 0.0,
        })),
    )
}

pub fn big_mushroom_wiggle(w: &mut World, id: EntityId, m: &mut BigMushroom, amount: f32) {
    if m.counter < 0.8 && amount.abs() >= 0.5 {
        sound(w, "env_backfireMushWobbz", w.core(id).pos.x);
        m.amp = amount;
        m.counter = 1.0;
    }
}

pub fn update_big_mushroom(w: &mut World, _c: &Content, _id: EntityId, m: &mut BigMushroom) {
    if m.counter > 0.0 {
        m.counter = max(0.0, m.counter - tm(w) / 90.0);
    }
}

/// Current wiggle value (`sin` at frequency 3 over the wiggle, decaying).
pub fn big_mushroom_value(m: &BigMushroom) -> f32 {
    let t = 1.0 - m.counter;
    sin(t * PI * 2.0 * 3.0) * m.counter * m.amp
}

// ------------------------------------------------------------ §9.7 BGSkeleton

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Skeleton {
    pub crumbling: bool,
    pub scale_y: f32,
    pub flip: bool,
    pub frame: u8,
}
impl_hash!(Skeleton {
    crumbling,
    scale_y,
    flip
});

pub fn spawn_skeleton(w: &mut World, p: &Placement) -> EntityId {
    let mut core = Core::new(
        p.pos,
        Collider::Box(Hitbox::new(10.0, 6.0, -10.0, -6.0)),
        0,
        &[Tag::PlayerCollider],
    );
    core.active = false;
    let frame = w.cosmetic_rng.next_max(3) as u8;
    w.add(
        core,
        Kind::Tower(Tower::Skeleton(Skeleton {
            crumbling: false,
            scale_y: 1.0,
            flip: false,
            frame,
        })),
    )
}

pub fn skeleton_begin(w: &mut World, c: &Content, id: EntityId, s: &mut Skeleton) {
    let at = w.core(id).pos;
    if !w.point_check(c, at + Vec2::new(-11.0, -1.0), Tag::Solid)
        && w.point_check(c, at + Vec2::new(11.0, -1.0), Tag::Solid)
    {
        w.core_mut(id).collider = Collider::Box(Hitbox::new(10.0, 6.0, 0.0, -6.0));
        s.flip = true;
    }
}

pub fn skeleton_crumble(w: &mut World, id: EntityId, s: &mut Skeleton) {
    if s.crumbling {
        return;
    }
    s.crumbling = true;
    let core = w.core_mut(id);
    core.collidable = false;
    core.active = true;
    sound(w, "env_twilightSkelCrumble", w.core(id).pos.x);
}

pub fn update_skeleton(w: &mut World, _c: &Content, id: EntityId, s: &mut Skeleton) {
    if !s.crumbling {
        return;
    }
    s.scale_y -= 0.02 * tm(w);
    let core = w.core(id);
    let at = Vec2::new(core.left() + 5.0, core.top() + 5.0);
    particles(w, "BGSkeletonBreak", at, 1, Vec2::new(5.0, 2.0), -PI / 2.0);
    if s.scale_y <= 0.0 {
        w.remove(id);
    }
}

// ------------------------------------------------------------ §9.10 WaterDrop

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropState {
    Hidden,
    Forming,
    Falling,
    Splash,
}
crate::impl_hash_enum!(DropState);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterDrop {
    pub start: Vec2,
    pub state: DropState,
    pub timer: f32,
    pub v: f32,
    /// Splash on a creature (animation 2) instead of a solid (1).
    pub on_creature: bool,
}
impl_hash!(WaterDrop {
    start,
    state,
    timer,
    v,
    on_creature
});

pub fn spawn_water_drop(w: &mut World, p: &Placement) -> EntityId {
    let mut core = Core::new(
        p.pos,
        Collider::Box(Hitbox::wrapping(4.0, 4.0, -2.0, -2.0)),
        0,
        &[],
    );
    core.screen_wrap = true;
    core.visible = false;
    let timer = w.rng.range_i(60, 300) as f32;
    w.add(
        core,
        Kind::Tower(Tower::WaterDrop(WaterDrop {
            start: p.pos,
            state: DropState::Hidden,
            timer,
            v: 0.0,
            on_creature: false,
        })),
    )
}

/// Forming: 6 frames of 3 ticks; splash: 3 frames of 3 ticks.
const DROP_FORM: f32 = 18.0;
const DROP_SPLASH: f32 = 9.0;

pub fn update_water_drop(w: &mut World, c: &Content, id: EntityId, d: &mut WaterDrop) {
    let t = tm(w);
    match d.state {
        DropState::Hidden => {
            d.timer -= t;
            if d.timer <= 0.0 {
                d.state = DropState::Forming;
                d.timer = DROP_FORM;
                let core = w.core_mut(id);
                core.pos = d.start;
                core.visible = true;
            }
        }
        DropState::Forming => {
            d.timer -= t;
            if d.timer <= 0.0 {
                d.state = DropState::Falling;
                d.v = 0.0;
            }
        }
        DropState::Falling => {
            d.v = min(d.v + 0.2 * t, 4.0);
            w.core_mut(id).pos.y += d.v * t;
            actor::enforce_screen_wrap(w, c, id);
            let me = w.placed(id);
            if w.any(c, &me, Tag::Solid, id) {
                let core = w.core_mut(id);
                core.pos.y = floor(core.pos.y);
                for _ in 0..4 {
                    let me = w.placed(id);
                    if !w.any(c, &me, Tag::Solid, id) {
                        break;
                    }
                    w.core_mut(id).pos.y -= 1.0;
                }
                w.core_mut(id).pos.y -= 3.0;
                d.on_creature = false;
                d.state = DropState::Splash;
                d.timer = DROP_SPLASH;
                sound(w, "env_sunkenDrip", w.core(id).pos.x);
            } else if w.any(c, &me, Tag::Player, id) || w.any(c, &me, Tag::Arrow, id) {
                d.on_creature = true;
                d.state = DropState::Splash;
                d.timer = DROP_SPLASH;
                sound(w, "env_sunkenDrip", w.core(id).pos.x);
            }
        }
        DropState::Splash => {
            d.timer -= t;
            if d.timer <= 0.0 {
                d.state = DropState::Hidden;
                d.timer = w.rng.range_i(60, 300) as f32;
                w.core_mut(id).visible = false;
            }
        }
    }
}

// ------------------------------------------------------------ §10.2 KingIntro

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KingIntro {
    /// 0 idle, 1 pissed, 2 happy, 3 transforming into the Reaper (stays).
    pub anim: u8,
    pub t: f32,
}
impl_hash!(KingIntro { anim, t });

/// Animation lengths (ticks): pissed 7 frames x 6, happy 14 x 6.
pub const KING_PISSED: f32 = 42.0;
pub const KING_HAPPY: f32 = 84.0;

pub fn spawn_king(w: &mut World, p: &Placement) -> EntityId {
    let mut core = Core::new(p.pos, Collider::None, 150, &[Tag::LightSource]);
    core.collidable = false;
    let id = w.add(
        core,
        Kind::Tower(Tower::KingIntro(KingIntro { anim: 0, t: 0.0 })),
    );
    w.tower.king = Some(id);
    id
}

pub fn king_laugh(w: &mut World, id: EntityId, k: &mut KingIntro) {
    if k.anim == 0 {
        k.anim = 2;
        k.t = 0.0;
        sound(w, "boss_humanLaugh", w.core(id).pos.x);
    }
}

pub fn king_get_mad(w: &mut World, id: EntityId, k: &mut KingIntro) {
    if k.anim == 0 {
        k.anim = 1;
        k.t = 0.0;
        sound(w, "boss_humanYell", w.core(id).pos.x);
    }
}

/// The King's `transform` animation (16 frames at 0.1 s): the Reaper takes
/// over when it shows frame 16 (index 12, 72 ticks in).
pub const KING_TRANSFORM_REAPER: f32 = 72.0;

/// `DoBossIntro`'s animation part: the King transforms (for good).
pub fn king_transform(k: &mut KingIntro) {
    k.anim = 3;
    k.t = 0.0;
}

pub fn update_king(w: &mut World, _c: &Content, _id: EntityId, k: &mut KingIntro) {
    if k.anim == 0 {
        return;
    }
    k.t += tm(w);
    if k.anim == 3 {
        return;
    }
    let len = if k.anim == 1 { KING_PISSED } else { KING_HAPPY };
    if k.t >= len {
        k.anim = 0;
        k.t = 0.0;
    }
}

// ------------------------------------------------------------ §10.1 Dummy

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dummy {
    pub facing_left: bool,
    pub chain: Option<EntityId>,
}
impl_hash!(Dummy { facing_left, chain });

pub const DUMMY_OFFSETS: [Vec2; 4] = [
    Vec2::new(0.0, 2.0),
    Vec2::new(0.0, 20.0),
    Vec2::new(-4.0, 11.0),
    Vec2::new(4.0, 11.0),
];

pub fn spawn_dummy(w: &mut World, p: &Placement) -> EntityId {
    let mut core = Core::new(
        p.pos,
        Collider::Box(Hitbox::new(8.0, 18.0, -4.0, 2.0)),
        -99,
        &[
            Tag::Dummy,
            Tag::PlayerCollider,
            Tag::LightSource,
            Tag::ExplosionCollider,
            Tag::Target,
        ],
    );
    core.seek = true;
    core.seek_priority = 50;
    core.seek_offset = Vec2::new(0.0, 13.0);
    w.add(
        core,
        Kind::Tower(Tower::Dummy(Dummy {
            facing_left: p.flag,
            chain: None,
        })),
    )
}

pub fn dummy_begin(w: &mut World, c: &Content, id: EntityId, d: &mut Dummy) {
    if let Some(ch) = chain_above(w, c, id) {
        d.chain = Some(ch);
        if let Some(Tower::Chain(chain)) = tower_mut(w, ch) {
            chain.holding = Some(id);
        }
    }
}

/// `Die()`: the dummy bursts (its corpse and flying head are Trials
/// presentation and not simulated).
pub fn dummy_die(w: &mut World, id: EntityId, d: &mut Dummy) {
    if w.core(id).marked_for_removal {
        return;
    }
    let at = w.core(id).pos + Vec2::new(0.0, 4.0);
    sound(w, "sfx_dummyImp", at.x);
    particles(w, "DummyDie", at, 20, Vec2::new(10.0, 10.0), -PI / 2.0);
    if let Some(ch) = d.chain.take()
        && let Some(Tower::Chain(chain)) = tower_mut(w, ch)
    {
        chain.holding = None;
    }
    w.remove(id);
}

/// `Dummy.Update` hazards (trials.md §8): touching an orb shatters it and
/// kills the dummy; brambles and bramble-covered corpses kill it too.
/// Returns whether the dummy must die.
pub fn dummy_hazards(w: &mut World, c: &Content, id: EntityId) -> bool {
    let me = w.placed(id);
    if let Some(o) = w.first(c, &me, Tag::Orb, id) {
        super::hazards::shatter_orb(w, c, o);
        return true;
    }
    if w.any(c, &me, Tag::Brambles, id) {
        sound(w, "env_deathSpike", w.core(id).pos.x);
        return true;
    }
    w.all(c, &me, Tag::Corpse, id)
        .into_iter()
        .any(|k| matches!(w.kind(k), Some(Kind::Corpse(cp)) if cp.brambles_harmful()))
}

pub fn update_dummy(w: &mut World, c: &Content, id: EntityId, d: &mut Dummy) {
    if dummy_hazards(w, c, id) {
        dummy_die(w, id, d);
    }
}

pub fn dummy_arrow(
    w: &mut World,
    id: EntityId,
    d: &mut Dummy,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    if let Some(ch) = d.chain {
        let y = w.core(aid).pos.y;
        chain_push_id(w, ch, y, a.speed.x);
    }
    dummy_die(w, id, d);
    true
}

pub fn dummy_player(
    w: &mut World,
    c: &Content,
    id: EntityId,
    d: &mut Dummy,
    pid: EntityId,
    p: &mut Player,
) {
    let top = w.core(id).top();
    if w.core(pid).bottom() - p.speed.y <= top + 10.0 && p.speed.y >= 0.0 {
        player::stomp_bounce(w, c, pid, p, top);
        dummy_die(w, id, d);
    }
}

/// The chain a lantern or dummy hangs from, if any (for the view).
pub fn hung_from(w: &World, id: EntityId) -> Option<EntityId> {
    match tower(w, id) {
        Some(Tower::Lantern(l)) => l.chain,
        Some(Tower::Dummy(d)) => d.chain,
        _ => None,
    }
}
