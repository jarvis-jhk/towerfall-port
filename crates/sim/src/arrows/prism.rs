//! The prism that traps an archer who catches a live prism arrow
//! (arrows.md §9.11). Its four bars are solids that are collidable only
//! while the captive's own update runs, so they hold the prisoner and
//! nothing else.

use crate::collide::{Collider, Hitbox};
use crate::components::Wiggler;
use crate::content::Content;
use crate::entity::{Core, EntityId, Kind, Tag, depth};
use crate::events::{Event, ParticleKind};
use crate::hurt::{self, Ctx};
use crate::impl_hash;
use crate::math::Vec2;
use crate::player::Player;
use crate::world::World;

pub const DEPTH: i32 = -99;
pub const CAPTIVE_DEPTH: i32 = -98;
pub const HOLD: f32 = 120.0;
pub const SHAKE: f32 = 30.0;
pub const GROW: f32 = 20.0;

/// The bars, relative to the prism position (= the captive's position).
pub const BARS: [Hitbox; 4] = [
    Hitbox::wrapping(5.0, 40.0, -20.0, -20.0),
    Hitbox::wrapping(5.0, 40.0, 15.0, -20.0),
    Hitbox::wrapping(30.0, 5.0, -20.0, -20.0),
    Hitbox::wrapping(30.0, 5.0, -20.0, 15.0),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prism {
    pub captive: EntityId,
    /// Kill credit for squish/miasma deaths of the captive.
    pub owner: i8,
    pub counter: f32,
    pub age: f32,
    pub bars: [Option<EntityId>; 4],
    pub shaking: bool,
    pub wiggle: Wiggler,
    /// The captive is an enemy (Quest skeletons catching prism arrows).
    pub enemy: bool,
}

impl_hash!(Prism {
    captive,
    owner,
    counter,
    age,
    bars,
    shaking,
    enemy
});

/// `StartPrism(owner)` on the catcher `pid` (its state is `p`).
pub fn start_prism(w: &mut World, _c: &Content, pid: EntityId, p: &mut Player, owner: i8) {
    if let Some(existing) = p.prism
        && w.exists(existing)
    {
        // CaptiveResnared.
        if let Some(Kind::Prism(pr)) = w.kind_mut(existing) {
            pr.counter = HOLD;
            pr.shaking = false;
            pr.wiggle.start();
        }
        w.sound("sfx_prismRestart", w.core(pid).pos.x);
        return;
    }
    let id = create(w, pid, owner, false);
    p.prism = Some(id);
    w.set_depth(pid, CAPTIVE_DEPTH);
}

/// `Enemy` catching a live prism arrow: a new prism around enemy `eid`
/// (returned; the caller stores it), or `CaptiveResnared` on `existing`.
pub fn start_enemy_prism(
    w: &mut World,
    eid: EntityId,
    existing: Option<EntityId>,
    owner: i8,
) -> EntityId {
    if let Some(ex) = existing
        && w.exists(ex)
        && let Some(Kind::Prism(pr)) = w.kind_mut(ex)
    {
        pr.counter = HOLD;
        pr.shaking = false;
        pr.wiggle.start();
        w.sound("sfx_prismRestart", w.core(eid).pos.x);
        return ex;
    }
    create(w, eid, owner, true)
}

fn create(w: &mut World, captive: EntityId, owner: i8, enemy: bool) -> EntityId {
    let at = w.core(captive).pos;
    let mut core = Core::new(at, Collider::None, DEPTH, &[]);
    core.screen_wrap = true;
    let prism = Prism {
        captive,
        owner,
        counter: HOLD,
        age: 0.0,
        bars: [None; 4],
        shaking: false,
        wiggle: Wiggler::new(30.0, 4.0, None),
        enemy,
    };
    let id = w.add(core, Kind::Prism(prism));
    let mut bars = [None; 4];
    for (k, hb) in BARS.iter().enumerate() {
        let mut bc = Core::new(at, Collider::Box(*hb), DEPTH, &[Tag::Solid]);
        bc.collidable = false;
        bc.visible = false;
        bc.active = false;
        bars[k] = Some(w.add(bc, Kind::PrismBar(id)));
    }
    if let Some(Kind::Prism(pr)) = w.kind_mut(id) {
        pr.bars = bars;
    }
    w.sound("sfx_prismForm", at.x);
    id
}

/// `EnableSolids` / `DisableSolids` for the captive's update.
pub fn set_bars(w: &mut World, prism: EntityId, on: bool) {
    let bars = match w.kind(prism) {
        Some(Kind::Prism(pr)) => pr.bars,
        _ => return,
    };
    for b in bars.into_iter().flatten() {
        if w.exists(b) {
            w.core_mut(b).collidable = on;
        }
    }
}

pub fn update(w: &mut World, _c: &Content, id: EntityId, pr: &mut Prism) {
    let tm = w.time.time_mult;
    pr.age += tm;
    pr.wiggle.update(tm);
    pr.counter -= tm;
    let captive_alive = if pr.enemy {
        matches!(w.kind(pr.captive), Some(Kind::Enemy(e)) if e.alive())
    } else {
        w.player(pr.captive).is_some_and(|p| !p.dead)
    };
    if !captive_alive {
        shatter(w, id, pr, &mut None);
        return;
    }
    if pr.counter <= 0.0 && !pr.shaking {
        pr.shaking = true;
        w.sound("sfx_prismShake", w.core(id).pos.x);
    }
    if pr.counter <= -SHAKE {
        shatter(w, id, pr, &mut None);
    }
}

/// `CaptiveForceShatter()` (the captive dodges or jumps): only once the
/// hold time is over.
pub fn force_shatter(w: &mut World, prism: EntityId, ctx: &mut Ctx) {
    if let Some(Kind::Prism(mut pr)) = w.kind(prism).copied()
        && pr.counter <= 0.0
    {
        shatter(w, prism, &mut pr, ctx);
        w.put_kind(prism, Kind::Prism(pr));
    }
}

/// `CaptiveDie()`: break at once.
pub fn captive_die(w: &mut World, prism: EntityId, ctx: &mut Ctx) {
    if let Some(Kind::Prism(mut pr)) = w.kind(prism).copied() {
        shatter(w, prism, &mut pr, ctx);
        w.put_kind(prism, Kind::Prism(pr));
    }
}

fn shatter(w: &mut World, id: EntityId, pr: &mut Prism, ctx: &mut Ctx) {
    if w.core(id).marked_for_removal {
        return;
    }
    let captive = pr.captive;
    if pr.enemy {
        if let Some(Kind::Enemy(e)) = w.kind_mut(captive)
            && e.prism == Some(id)
        {
            e.prism = None;
        }
    } else {
        hurt::with_player_ctx(w, captive, ctx, |_, p| {
            if p.prism == Some(id) {
                p.prism = None;
            }
        });
        if w.exists(captive) {
            w.set_depth(captive, depth::PLAYER);
        }
    }
    for b in pr.bars.into_iter().flatten() {
        w.remove(b);
    }
    let at = w.core(id).pos;
    w.emit(Event::Particles {
        kind: ParticleKind::PrismDebris,
        player: u8::MAX,
        pos: at,
        count: 25,
        dir: 0.0,
    });
    w.sound("sfx_prismShatter", at.x);
    w.remove(id);
}

/// Grow scale for drawing.
pub fn scale(pr: &Prism) -> Vec2 {
    let t = (pr.age / GROW).min(1.0);
    let s = crate::components::Ease::BackOut.apply(t);
    Vec2::new(s, s)
}
