//! Brambles (arrows.md §9.5.1–9.5.2): the breadth-first growth routine a
//! bramble arrow runs where it first sticks, and the thorn tiles.
//!
//! The growth routine is a coroutine component of the arrow in the
//! original (it pauses while the arrow is inactive and dies with it). Its
//! queue lives in [`crate::pickups::ItemsState::growths`] so entity slots
//! stay small; the arrow ticks it in its component step.

use crate::checksum::{Hasher, StableHash};
use crate::collide::{Collider, Hitbox, Placed};
use crate::content::Content;
use crate::entity::{Core, EntityId, Kind, Tag};
use crate::hurt;
use crate::impl_hash;
use crate::math::Vec2;
use crate::player::{DeathCause, Player};
use crate::world::World;

pub const HITBOX: Hitbox = Hitbox::wrapping(8.0, 8.0, -4.0, -4.0);
pub const FIRE_HITBOX: Hitbox = Hitbox::wrapping(16.0, 16.0, -8.0, -8.0);
pub const DEPTH: i32 = -120;
pub const GROW_FRAMES: f32 = 20.0;
pub const LIFE: f32 = 600.0;
pub const SHORT_LIFE: f32 = 180.0;
pub const BURN_FRAMES: f32 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Hidden,
    Growing,
    Grown,
    Shrinking,
}
crate::impl_hash_enum!(Phase);

/// One 8x8 thorn tile.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bramble {
    pub patch: u32,
    pub owner: i8,
    pub phase: Phase,
    pub delay: f32,
    /// Frames into the grow / shrink tween.
    pub tween: f32,
    pub death: f32,
    pub death_on: bool,
    pub burning: bool,
    pub fire: f32,
    /// The first tile of its patch plays the disappear sound.
    pub lead: bool,
    pub short: bool,
    /// The moving solid it sits on (it moves with it) and that solid's
    /// position last tick.
    pub riding: Option<EntityId>,
    pub ride_at: Vec2,
}

impl_hash!(Bramble {
    patch,
    owner,
    phase,
    delay,
    tween,
    death,
    death_on,
    burning,
    fire,
    lead,
    short,
    riding,
    ride_at
});

impl Bramble {
    /// Grow / shrink scale for drawing.
    pub fn scale(&self) -> f32 {
        use crate::components::Ease;
        match self.phase {
            Phase::Hidden => 0.0,
            Phase::Growing => Ease::BackOut.apply((self.tween / GROW_FRAMES).min(1.0)),
            Phase::Grown => 1.0,
            Phase::Shrinking => 1.0 - Ease::BackIn.apply((self.tween / GROW_FRAMES).min(1.0)),
        }
    }
}

/// A growth routine in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct Growth {
    pub patch: u32,
    /// The arrow whose component this is.
    pub arrow: Option<EntityId>,
    pub at: Vec2,
    pub owner: i8,
    pub short: bool,
    pub queue: Vec<(Vec2, u8)>,
    pub head: usize,
    pub made: Vec<Vec2>,
    pub sound: bool,
}

impl StableHash for Growth {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u32(self.patch);
        self.arrow.stable_hash(h);
        h.vec2(self.at);
        h.i32(self.owner as i32);
        h.bool(self.short);
        h.u32(self.queue.len() as u32);
        for (p, r) in &self.queue {
            h.vec2(*p);
            h.u8(*r);
        }
        h.u32(self.head as u32);
        self.made.stable_hash(h);
        h.bool(self.sound);
    }
}

/// `CreateBrambles(at, owner, onComplete, spread, short)`: queue a new
/// routine; it makes its first tile on the owner's next component update.
pub fn start_growth(
    w: &mut World,
    at: Vec2,
    owner: i8,
    arrow: Option<EntityId>,
    spread: u8,
    short: bool,
) {
    w.items.bramble_patch = w.items.bramble_patch.wrapping_add(1);
    let patch = w.items.bramble_patch;
    w.items.growths.push(Growth {
        patch,
        arrow,
        at,
        owner,
        short,
        queue: vec![(at, spread)],
        head: 0,
        made: vec![],
        sound: false,
    });
}

fn wrap(w: &World, c: &Content, p: Vec2) -> Vec2 {
    let ws = w.world_size(c);
    let mut x = p.x % ws.w;
    if x < 0.0 {
        x += ws.w;
    }
    let mut y = p.y % ws.h;
    if y < 0.0 {
        y += ws.h;
    }
    Vec2::new(x, y)
}

/// `CanSpreadTo(p)`: touching between one and three solids around `p`.
pub fn can_spread_to(w: &World, c: &Content, p: Vec2) -> bool {
    let n = [
        Vec2::new(-4.0, 0.0),
        Vec2::new(4.0, 0.0),
        Vec2::new(0.0, 4.0),
        Vec2::new(0.0, -4.0),
    ]
    .iter()
    .filter(|&&d| w.point_check(c, wrap(w, c, p + d), Tag::Solid))
    .count();
    (1..=3).contains(&n)
}

/// Resume the growth routine owned by `arrow` (once per arrow update).
/// Returns true when the routine finished this frame (`FinishBrambles`).
pub fn tick_growth(w: &mut World, c: &Content, arrow: EntityId) -> bool {
    // Routines of arrows that are gone stop for good.
    let live: Vec<bool> = w
        .items
        .growths
        .iter()
        .map(|g| g.arrow.is_none_or(|a| w.exists(a)))
        .collect();
    let mut k = 0;
    w.items.growths.retain(|_| {
        k += 1;
        live[k - 1]
    });
    let Some(gi) = w.items.growths.iter().position(|g| g.arrow == Some(arrow)) else {
        return false;
    };
    let mut g = w.items.growths[gi].clone();
    let finished = step(w, c, &mut g);
    if finished {
        w.items.growths.remove(gi);
    } else {
        w.items.growths[gi] = g;
    }
    finished
}

/// Patches without an arrow (Dark World bramble spores): one resume each
/// per tick.
pub fn tick_free_growths(w: &mut World, c: &Content) {
    let mut k = 0;
    while k < w.items.growths.len() {
        if w.items.growths[k].arrow.is_some() {
            k += 1;
            continue;
        }
        let mut g = w.items.growths[k].clone();
        if step(w, c, &mut g) {
            w.items.growths.remove(k);
        } else {
            w.items.growths[k] = g;
            k += 1;
        }
    }
}

/// One resume of the routine: make at most one tile.
fn step(w: &mut World, c: &Content, g: &mut Growth) -> bool {
    while g.head < g.queue.len() {
        let (p, rem) = g.queue[g.head];
        g.head += 1;
        if g.made.contains(&p) {
            continue;
        }
        let made = g.made.len() as f32;
        let delay = crate::math::max(1.0, 8.0 + (3.0 - rem as f32) * 2.0 - made);
        let lead = g.made.is_empty();
        let at = wrap(w, c, p);
        create(w, at, delay, rem as i32, g.patch, g.owner, g.short, lead);
        g.made.push(p);
        if rem > 0 {
            for n in [
                Vec2::new(-8.0, 0.0),
                Vec2::new(8.0, 0.0),
                Vec2::new(0.0, -8.0),
                Vec2::new(0.0, 8.0),
            ] {
                for o in [
                    Vec2::ZERO,
                    Vec2::new(4.0, 0.0),
                    Vec2::new(-4.0, 0.0),
                    Vec2::new(0.0, 4.0),
                    Vec2::new(0.0, -4.0),
                ] {
                    let q = p + n + o;
                    if can_spread_to(w, c, q) {
                        g.queue.push((q, rem - 1));
                        break;
                    }
                }
            }
        }
        if !g.sound {
            g.sound = true;
            w.sound("pu_brambleGrow", g.at.x);
        }
        return false;
    }
    if !g.sound {
        g.sound = true;
        w.sound("pu_brambleGrow", g.at.x);
    }
    true
}

/// One tile of patch `patch` at `at` (tests and editor tools; growth
/// normally places tiles through the patch routine).
pub fn spawn_tile(w: &mut World, at: Vec2, patch: u32, owner: i8) -> EntityId {
    create(w, at, 0.0, 0, patch, owner, false, false)
}

#[allow(clippy::too_many_arguments)]
fn create(
    w: &mut World,
    at: Vec2,
    delay: f32,
    depth_off: i32,
    patch: u32,
    owner: i8,
    short: bool,
    lead: bool,
) -> EntityId {
    let mut core = Core::new(
        at,
        Collider::Box(HITBOX),
        DEPTH + depth_off,
        &[
            Tag::Brambles,
            Tag::PlayerCollider,
            Tag::ExplosionCollider,
            Tag::ShockCollider,
        ],
    );
    core.visible = false;
    core.collidable = false;
    core.pushable = false;
    core.screen_wrap = true;
    let life = if short { SHORT_LIFE } else { LIFE };
    let b = Bramble {
        patch,
        owner,
        phase: Phase::Hidden,
        delay,
        tween: 0.0,
        death: delay + life,
        death_on: true,
        burning: false,
        fire: 0.0,
        lead,
        short,
        riding: None,
        ride_at: Vec2::ZERO,
    };
    w.add(core, Kind::Bramble(b))
}

/// `Added()`: older patches give way; InfiniteBrambles cancels the death
/// timer.
pub fn added(w: &mut World, c: &Content, id: EntityId) {
    let Some(Kind::Bramble(b)) = w.kind(id).copied() else {
        return;
    };
    let me = Placed {
        pos: w.core(id).pos,
        collider: Collider::Box(HITBOX),
    };
    let others: Vec<EntityId> = w.tagged(Tag::Brambles).to_vec();
    for o in others {
        if o == id || !w.check(c, &me, o) {
            continue;
        }
        if let Some(Kind::Bramble(ob)) = w.kind(o).copied()
            && ob.patch != b.patch
        {
            shrink(w, o, true);
        }
    }
    if crate::pickups::variant(
        w,
        crate::variants::Variant::InfiniteBrambles,
        b.owner as i32,
    ) && b.owner != -1
        && let Some(Kind::Bramble(bb)) = w.kind_mut(id)
    {
        bb.death_on = false;
    }
    // `riding`: the (non-tile) solid it overlaps when added.
    let probe = Placed {
        pos: w.core(id).pos,
        collider: Collider::Box(Hitbox::wrapping(
            HITBOX.w + 2.0,
            HITBOX.h + 2.0,
            HITBOX.x - 1.0,
            HITBOX.y - 1.0,
        )),
    };
    let ride = w
        .all(c, &probe, Tag::Solid, id)
        .into_iter()
        .find(|&s| !matches!(w.kind(s), Some(Kind::Tiles | Kind::StaticSolid)));
    if let Some(s) = ride {
        let at = w.core(s).pos;
        if let Some(Kind::Bramble(bb)) = w.kind_mut(id) {
            bb.riding = Some(s);
            bb.ride_at = at;
        }
    }
}

/// Start the shrink tween (harmless at once).
pub fn shrink(w: &mut World, id: EntityId, quiet: bool) {
    if let Some(Kind::Bramble(mut b)) = w.kind(id).copied() {
        shrink_state(w, id, &mut b, quiet);
        w.put_kind(id, Kind::Bramble(b));
    }
}

fn shrink_state(w: &mut World, id: EntityId, b: &mut Bramble, quiet: bool) {
    if b.phase == Phase::Shrinking {
        return;
    }
    b.phase = Phase::Shrinking;
    b.tween = 0.0;
    b.death_on = false;
    w.core_mut(id).collidable = false;
    if b.lead && !quiet {
        w.sound("pu_brambleDisappear", w.core(id).pos.x);
    }
}

/// Set a tile on fire (explosion push, burning arrows, lava).
pub fn ignite(w: &mut World, id: EntityId) {
    if let Some(Kind::Bramble(b)) = w.kind_mut(id)
        && !b.burning
    {
        b.burning = true;
        b.fire = 0.0;
    }
}

/// `OnExplode` / `OnShock`: the tile is gone.
pub fn destroy(w: &mut World, id: EntityId) {
    w.remove(id);
}

pub fn update(w: &mut World, c: &Content, id: EntityId, b: &mut Bramble) {
    let tm = w.time.time_mult;
    // Move with the solid it rides (MoveExact just shifts the position).
    if let Some(s) = b.riding {
        match w.try_core(s).map(|sc| sc.pos) {
            Some(at) => {
                let d = at - b.ride_at;
                if !d.is_zero() {
                    w.core_mut(id).pos = w.core(id).pos + d;
                    b.ride_at = at;
                }
            }
            None => b.riding = None,
        }
    }
    if w.time.on_interval(4) && !can_spread_to(w, c, w.core(id).pos) {
        w.remove(id);
        return;
    }
    match b.phase {
        Phase::Hidden => {
            b.delay -= tm;
            if b.delay <= 0.0 {
                b.phase = Phase::Growing;
                b.tween = 0.0;
                w.core_mut(id).visible = true;
            }
        }
        Phase::Growing => {
            b.tween += tm;
            if b.tween >= GROW_FRAMES {
                b.phase = Phase::Grown;
                w.core_mut(id).collidable = true;
            }
        }
        Phase::Grown => {}
        Phase::Shrinking => {
            b.tween += tm;
            if b.tween >= GROW_FRAMES {
                w.remove(id);
                return;
            }
        }
    }
    if b.death_on {
        b.death -= tm;
        if b.death <= 0.0 {
            shrink_state(w, id, b, false);
        }
    }
    if b.burning {
        let before = b.fire;
        b.fire += tm;
        if before < 15.0 && b.fire >= 15.0 {
            w.core_mut(id).collider = Collider::Box(FIRE_HITBOX);
            let me = w.placed(id);
            for o in w.all(c, &me, Tag::Brambles, id) {
                ignite(w, o);
            }
        }
        if b.fire >= BURN_FRAMES {
            w.remove(id);
        }
    }
}

/// `OnPlayerCollide(player)` (from the player's PlayerCollider pass).
pub fn on_player_collide(w: &mut World, c: &Content, bid: EntityId, pid: EntityId, p: &mut Player) {
    let Some(Kind::Bramble(b)) = w.kind(bid).copied() else {
        return;
    };
    if hurt::can_hurt_player(w, b.owner, p.index as i8) {
        let from = w.core(bid).pos;
        hurt::hurt(
            w,
            c,
            pid,
            p,
            DeathCause::Brambles,
            from,
            b.owner,
            Some("env_deathSpike"),
        );
    }
}
