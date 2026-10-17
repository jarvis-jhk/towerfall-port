//! Pick-ups (pickups.md §1–3): the `Pickups` enum, the item entity that
//! flies out of a chest, what each item gives, the trapped-chest bomb, and
//! the match-level item state ([`ItemsState`]) and settings ([`ItemSetup`]).
//! Orbs are in [`orbs`], the lava orb's lava in [`lava`].

pub mod orbs;

use crate::arrows::brambles::Growth;
use crate::checksum::{Hasher, StableHash};
use crate::collide::{Collider, Hitbox};
use crate::components::Ease;
use crate::content::Content;
use crate::entity::{Core, EntityId, FlashFinish, Kind, Tag};
use crate::events::{Event, ParticleKind};
use crate::explosion;
use crate::impl_hash;
use crate::math::{Vec2, lerp};
use crate::player::{self, Player};
use crate::quiver::ArrowType;
use crate::world::World;

/// `Pickups` in enum order (the index is the contract for masks and rates).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(u8)]
pub enum Pickups {
    #[default]
    Arrows = 0,
    BombArrows,
    SuperBombArrows,
    LaserArrows,
    BrambleArrows,
    DrillArrows,
    BoltArrows,
    FeatherArrows,
    TriggerArrows,
    PrismArrows,
    Shield,
    Wings,
    SpeedBoots,
    Mirror,
    TimeOrb,
    DarkOrb,
    LavaOrb,
    SpaceOrb,
    ChaosOrb,
    Bomb,
    Gem,
}
crate::impl_hash_enum!(Pickups);

pub const PICKUP_COUNT: usize = 21;
pub const LAST_ARROW_PICKUP: u8 = 9;

pub const ALL_PICKUPS: [Pickups; PICKUP_COUNT] = [
    Pickups::Arrows,
    Pickups::BombArrows,
    Pickups::SuperBombArrows,
    Pickups::LaserArrows,
    Pickups::BrambleArrows,
    Pickups::DrillArrows,
    Pickups::BoltArrows,
    Pickups::FeatherArrows,
    Pickups::TriggerArrows,
    Pickups::PrismArrows,
    Pickups::Shield,
    Pickups::Wings,
    Pickups::SpeedBoots,
    Pickups::Mirror,
    Pickups::TimeOrb,
    Pickups::DarkOrb,
    Pickups::LavaOrb,
    Pickups::SpaceOrb,
    Pickups::ChaosOrb,
    Pickups::Bomb,
    Pickups::Gem,
];

/// Enum names as `tower.xml` lists them.
pub const PICKUP_NAMES: [&str; PICKUP_COUNT] = [
    "Arrows",
    "BombArrows",
    "SuperBombArrows",
    "LaserArrows",
    "BrambleArrows",
    "DrillArrows",
    "BoltArrows",
    "FeatherArrows",
    "TriggerArrows",
    "PrismArrows",
    "Shield",
    "Wings",
    "SpeedBoots",
    "Mirror",
    "TimeOrb",
    "DarkOrb",
    "LavaOrb",
    "SpaceOrb",
    "ChaosOrb",
    "Bomb",
    "Gem",
];

impl Pickups {
    pub fn from_index(i: usize) -> Pickups {
        ALL_PICKUPS[i]
    }
    pub fn from_name(n: &str) -> Option<Pickups> {
        PICKUP_NAMES
            .iter()
            .position(|&s| s == n.trim())
            .map(Pickups::from_index)
    }
    pub fn bit(self) -> u32 {
        1 << self as u8
    }
    pub fn is_arrows(self) -> bool {
        (self as u8) <= LAST_ARROW_PICKUP
    }
    /// The arrow type of an arrow pick-up (no Toy pick-up exists).
    pub fn arrow_type(self) -> Option<ArrowType> {
        Some(match self {
            Pickups::Arrows => ArrowType::Normal,
            Pickups::BombArrows => ArrowType::Bomb,
            Pickups::SuperBombArrows => ArrowType::SuperBomb,
            Pickups::LaserArrows => ArrowType::Laser,
            Pickups::BrambleArrows => ArrowType::Bramble,
            Pickups::DrillArrows => ArrowType::Drill,
            Pickups::BoltArrows => ArrowType::Bolt,
            Pickups::FeatherArrows => ArrowType::Feather,
            Pickups::TriggerArrows => ArrowType::Trigger,
            Pickups::PrismArrows => ArrowType::Prism,
            _ => return None,
        })
    }
    /// The arrow pick-up for an arrow type (`None` for Toy).
    pub fn for_arrow(t: ArrowType) -> Option<Pickups> {
        (0..=LAST_ARROW_PICKUP as usize)
            .map(Pickups::from_index)
            .find(|p| p.arrow_type() == Some(t))
    }
    fn collect_sound(self) -> Option<&'static str> {
        Some(match self {
            Pickups::Arrows => "pu_plus2Arrows",
            Pickups::BombArrows => "pu_bombArrow",
            Pickups::SuperBombArrows => "pu_superBomb",
            Pickups::LaserArrows => "pu_laserArrow",
            Pickups::BrambleArrows => "pu_brambleArrow",
            Pickups::DrillArrows => "pu_drill",
            Pickups::BoltArrows => "pu_boltArrow",
            Pickups::FeatherArrows => "pu_feather",
            Pickups::TriggerArrows => "pu_triggerArrow",
            Pickups::PrismArrows => "pu_prismArrow",
            Pickups::Wings => "pu_wing",
            Pickups::SpeedBoots => "pu_speedBoots",
            Pickups::Mirror => "pu_invisible",
            Pickups::DarkOrb | Pickups::ChaosOrb => "pu_darkOrbCollect",
            Pickups::LavaOrb => "pu_lava",
            Pickups::SpaceOrb => "pu_spaceOrb",
            Pickups::Gem => "sfx_gemCollect",
            _ => return None,
        })
    }
}

// ------------------------------------------------------------- settings

/// Per-match item settings: the tower's treasure data and unlock state
/// (variants come from [`crate::variants::VariantSet`]). Part of
/// [`crate::MatchSetup`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemSetup {
    /// `TreasureMask` (count per `Pickups` index); `None` = the tower has no
    /// `<treasure>` element (all 1).
    pub treasure_mask: Option<[u8; PICKUP_COUNT]>,
    /// `SpecialArrowRate` (`arrows` attribute, default 0.6).
    pub special_arrow_rate: f32,
    /// The tower's `arrowShuffle` attribute.
    pub tower_arrow_shuffle: bool,
    /// Workshop tower (no unlock gating, never shuffles by tower data).
    pub custom_tower: bool,
    /// Dark World content present (Trigger/Prism arrows).
    pub dark_world: bool,
    /// Unlocks (the port treats everything as unlocked by default).
    pub unlocked_bolt: bool,
    pub unlocked_super_bomb: bool,
    pub unlocked_feather: bool,
    /// The RemoveScrollEffects option.
    pub remove_scroll_effects: bool,
}

impl Default for ItemSetup {
    fn default() -> Self {
        ItemSetup {
            treasure_mask: None,
            special_arrow_rate: 0.6,
            tower_arrow_shuffle: false,
            custom_tower: false,
            dark_world: true,
            unlocked_bolt: true,
            unlocked_super_bomb: true,
            unlocked_feather: true,
            remove_scroll_effects: false,
        }
    }
}

impl ItemSetup {
    /// From a tower's `<treasure>` entries (`tfp_assets::towers::VersusTower`).
    pub fn with_tower(mut self, names: Option<&[String]>, rate: f32, shuffle: bool) -> ItemSetup {
        self.treasure_mask = names.map(|list| {
            let mut m = [0u8; PICKUP_COUNT];
            for n in list {
                if let Some(p) = Pickups::from_name(n) {
                    m[p as usize] = m[p as usize].saturating_add(1);
                }
            }
            m
        });
        self.special_arrow_rate = rate;
        self.tower_arrow_shuffle = shuffle;
        self
    }
}

// ------------------------------------------------------------- state

/// Match-level item state (lives in [`World::items`]).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ItemsState {
    /// `TreasureRates` (indexed by `Pickups`, 20 entries; Gem never spawns).
    pub rates: [f32; 20],
    /// Global bramble patch id counter.
    pub bramble_patch: u32,
    /// Bramble growth routines in progress.
    pub growths: Vec<Growth>,
    pub orbs: orbs::Orbs,
    /// `Session.RoundRandomArrowType`.
    pub round_arrow: ArrowType,
    /// The kind of the explosion being resolved (for its kill stats; only
    /// set during one `Explosion.Spawn`, never across ticks).
    pub blast: Blast,
}

/// What made an explosion (stats: trigger-bomb and bomb-trap kills).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Blast {
    #[default]
    Normal,
    Trigger,
    Trap,
}

impl StableHash for ItemsState {
    fn stable_hash(&self, h: &mut Hasher) {
        self.rates.stable_hash(h);
        h.u32(self.bramble_patch);
        self.growths.stable_hash(h);
        self.orbs.stable_hash(h);
        self.round_arrow.stable_hash(h);
    }
}

impl ItemsState {
    /// A new level scene: per-round state goes, the match's rates stay.
    pub fn new_round(&mut self) {
        self.growths.clear();
        self.orbs = orbs::Orbs::default();
    }
}

// ------------------------------------------------------------- the item

pub const DEPTH: i32 = -160;
pub const HITBOX: Hitbox = Hitbox::wrapping(16.0, 16.0, -8.0, -8.0);
pub const UNPACK: f32 = 30.0;
pub const BOMB_SWELL: f32 = 40.0;
pub const TIMEOUT: f32 = 240.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pickup {
    pub kind: Pickups,
    pub start: Vec2,
    pub target: Vec2,
    /// Frames into the unpack tween.
    pub t: f32,
    pub unpacked: bool,
    pub collidable_in: f32,
    /// Decorative bob (sine counter).
    pub bob: f32,
    /// Bottomless chest items time out.
    pub timeout: f32,
    pub timeout_on: bool,
    /// Who opened the chest (bomb kill credit).
    pub creator: i8,
    /// Bomb: frames into the swell.
    pub swell: f32,
}

impl_hash!(Pickup {
    kind,
    start,
    target,
    t,
    unpacked,
    collidable_in,
    bob,
    timeout,
    timeout_on,
    creator,
    swell
});

impl Pickup {
    /// Sprite scale (the unpack tween scales the art).
    pub fn scale(&self) -> f32 {
        let e = Ease::BackOut.apply((self.t / UNPACK).min(1.0));
        if self.kind == Pickups::Bomb && self.unpacked {
            1.0 + 2.0 * Ease::CubeIn.apply((self.swell / BOMB_SWELL).min(1.0))
        } else {
            e
        }
    }
    /// The cosmetic bob offset.
    pub fn draw_offset(&self) -> Vec2 {
        let s = crate::math::sin(self.bob);
        let s2 = crate::math::sin(self.bob / 2.0);
        Vec2::new(3.0 * s, 3.0 * s2)
    }
}

/// `new Pickup(position, target)` via `Pickup.CreatePickup(type)`.
pub fn create(
    w: &mut World,
    kind: Pickups,
    at: Vec2,
    target: Vec2,
    creator: i8,
    timeout: bool,
) -> EntityId {
    let bomb = kind == Pickups::Bomb;
    let tags: &[Tag] = if bomb {
        &[Tag::LightSource]
    } else {
        &[Tag::PlayerCollectible, Tag::LightSource]
    };
    let collider = if bomb {
        Collider::None
    } else {
        Collider::Box(HITBOX)
    };
    let mut core = Core::new(at, collider, DEPTH, tags);
    core.screen_wrap = true;
    core.collidable = false;
    let target = Vec2::new(target.x, crate::math::max(6.0, target.y));
    let p = Pickup {
        kind,
        start: at,
        target,
        t: 0.0,
        unpacked: false,
        collidable_in: 10.0,
        bob: 0.0,
        timeout: TIMEOUT,
        timeout_on: timeout,
        creator,
        swell: 0.0,
    };
    let id = w.add(core, Kind::Pickup(p));
    if bomb {
        w.emit(Event::EntityLoop {
            name: "sfx_bombChestLoop",
            entity: id,
            on: true,
            x: at.x,
        });
    }
    id
}

pub fn update(w: &mut World, c: &Content, id: EntityId, p: &mut Pickup) {
    let tm = w.time.time_mult;
    // Components: unpack tween, collidable alarm, bob, flash.
    if !p.unpacked {
        p.t += tm;
        let e = Ease::BackOut.apply((p.t / UNPACK).min(1.0));
        w.core_mut(id).pos = Vec2::new(
            lerp(p.start.x, p.target.x, e),
            lerp(p.start.y, p.target.y, e),
        );
        if p.t >= UNPACK {
            p.unpacked = true;
        }
    } else if p.kind == Pickups::Bomb {
        p.swell += tm;
        if p.swell >= BOMB_SWELL {
            let at = w.core(id).pos;
            w.items.blast = Blast::Trap;
            explosion::spawn(w, c, at, p.creator, false, &mut None);
            w.items.blast = Blast::Normal;
            w.sound("pu_bombArrowExplode", at.x);
            w.emit(Event::EntityLoop {
                name: "sfx_bombChestLoop",
                entity: id,
                on: false,
                x: at.x,
            });
            w.remove(id);
            return;
        }
    }
    if p.collidable_in > 0.0 {
        p.collidable_in -= tm;
        if p.collidable_in <= 0.0 && p.kind != Pickups::Bomb {
            w.core_mut(id).collidable = true;
        }
    }
    p.bob = (p.bob + crate::math::TAU / 120.0 * tm) % (crate::math::PI * 8.0);
    if let Some(FlashFinish::RemoveSelf) = w.core_mut(id).flash.update(tm) {
        w.remove(id);
        return;
    }
    // Pickup.Update.
    let pos = w.core(id).pos;
    if w.point_check(c, pos, Tag::Solid) {
        w.core_mut(id).pos.y += 0.5 * tm;
    }
    if p.timeout_on {
        p.timeout -= tm;
        if p.timeout <= 0.0 {
            p.timeout_on = false;
            w.core_mut(id)
                .flash
                .start_for(60.0, FlashFinish::RemoveSelf);
        }
    }
    crate::actor::enforce_screen_wrap(w, c, id);
}

/// `OnPlayerCollide(player)`: the guard, then the gain. The item is left
/// in place when the player cannot use it.
pub fn on_player_collide(w: &mut World, c: &Content, pk: EntityId, pid: EntityId, p: &mut Player) {
    let Some(Kind::Pickup(item)) = w.kind(pk).copied() else {
        return;
    };
    if item.kind == Pickups::Bomb {
        return;
    }
    if gain(w, c, pid, p, item.kind, true) {
        crate::versus::stat(w, p.index, |s| s.treasures_taken += 1);
        let at = w.core(pk).pos;
        w.emit(Event::Particles {
            kind: ParticleKind::LightFade,
            player: p.index,
            pos: at,
            count: 1,
            dir: 0.0,
        });
        w.remove(pk);
    }
}

/// What an item gives (`OnPlayerCollide` with guards, `DoPlayerCollect`
/// without). Returns whether it was taken.
pub fn gain(
    w: &mut World,
    c: &Content,
    pid: EntityId,
    p: &mut Player,
    kind: Pickups,
    guard: bool,
) -> bool {
    let at = w.core(pid).pos;
    let ok = match kind {
        k if k.is_arrows() => {
            let t = k.arrow_type().unwrap_or_default();
            let ok = player::collect_arrows(w, pid, p, &[t, t]) || !guard;
            if ok {
                w.emit(Event::PickupText {
                    pickup: k as u8,
                    pos: at,
                });
            }
            ok
        }
        Pickups::Shield => {
            if guard && p.has_shield {
                false
            } else {
                if !p.has_shield {
                    player::gain_shield(w, pid, p);
                }
                true
            }
        }
        Pickups::Wings => {
            if guard && p.has_wings {
                false
            } else {
                p.has_wings = true;
                true
            }
        }
        Pickups::SpeedBoots => {
            if guard && p.speed_boots {
                false
            } else {
                p.speed_boots = true;
                w.emit(Event::Particles {
                    kind: ParticleKind::SpeedBoots,
                    player: p.index,
                    pos: at,
                    count: 30,
                    dir: 0.0,
                });
                true
            }
        }
        Pickups::Mirror => {
            if guard && p.invisible {
                false
            } else {
                p.invisible = true;
                true
            }
        }
        Pickups::TimeOrb => {
            orbs::do_time_orb(w, false);
            true
        }
        Pickups::DarkOrb => {
            orbs::do_dark_orb(w);
            true
        }
        Pickups::LavaOrb => {
            orbs::do_lava_orb(w, c, p.index as i8);
            true
        }
        Pickups::SpaceOrb => {
            orbs::do_space_orb(w, c);
            true
        }
        Pickups::ChaosOrb => {
            orbs::do_dark_orb(w);
            orbs::do_time_orb(w, true);
            orbs::do_lava_orb(w, c, p.index as i8);
            if !w.setup.items.remove_scroll_effects {
                orbs::do_space_orb_delayed(w);
            }
            true
        }
        Pickups::Gem => {
            // Trials records the find; the app saves it (trials.md §9).
            if let Some(t) = w.trials.as_mut() {
                t.gem_collected = true;
            }
            true
        }
        _ => false,
    };
    if ok && let Some(s) = kind.collect_sound() {
        w.sound(s, at.x);
    }
    ok
}

/// Every `PlayerCollectible` the archer's current collider touches
/// (player.md §4 step 25a; the PlayerCollider pass 25b is in `player`).
pub fn collectible_overlaps(w: &mut World, c: &Content, pid: EntityId, p: &mut Player) {
    let me = w.placed(pid);
    for e in w.all(c, &me, Tag::PlayerCollectible, pid) {
        if p.dead {
            return;
        }
        if let Some(Kind::Pickup(_)) = w.kind(e) {
            on_player_collide(w, c, e, pid, p);
        }
    }
}

/// `RoundLogic.RoundStarted` (true outside a Versus match).
pub fn round_started(w: &World) -> bool {
    w.versus.as_ref().is_none_or(|m| m.round.started)
}

/// `Level.Ending`: the round is decided (orb entry points do nothing).
pub fn level_ending(w: &World) -> bool {
    w.versus.as_ref().is_some_and(|m| m.round.ending)
}

/// A variant for player slot `p` (per-player variants read false for -1).
pub fn variant(w: &World, v: crate::variants::Variant, p: i32) -> bool {
    w.setup.variant_set.get(v, p)
}

/// `Variant.Value`: on for any slot (or the global flag).
pub fn variant_any(w: &World, v: crate::variants::Variant) -> bool {
    w.setup.variant_set.any(v)
}
