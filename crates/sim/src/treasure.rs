//! Treasure chests and Versus treasure selection (pickups.md §6–8, §11).
//!
//! * [`Chest`]: the chest entity (types, Time-mode appearance, gravity,
//!   opening by touch/arrow, contents flying out, the bottomless loop).
//! * The `TreasureSpawner`: per-match rates from the tower's mask, the
//!   special-arrow rescale and exclusions ([`compute_rates`]), the weighted
//!   draw ([`get_treasure_spawn`]), chest counts and timers per level
//!   ([`get_chest_spawns_for_level`]), the random arrow type and the
//!   treasure-draft offer. Every draw comes from the world's seeded RNG.

use crate::actor;
use crate::arrow::{self, Arrow};
use crate::collide::{Collider, Hitbox, Placed};
use crate::content::Content;
use crate::entity::{Core, EntityId, Kind, Tag};
use crate::events::{Event, ParticleKind};
use crate::impl_hash;
use crate::impl_hash_enum;
use crate::math::{Vec2, approach};
use crate::pickups::{self, Pickups, variant_any};
use crate::player::Player;
use crate::quiver::ArrowType;
use crate::variants::Variant;
use crate::world::World;

// ------------------------------------------------------------- chests

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChestType {
    Normal,
    AutoOpen,
    Large,
    Special,
    Bottomless,
}
impl_hash_enum!(ChestType);

impl ChestType {
    pub fn large(self) -> bool {
        matches!(self, ChestType::Large | ChestType::Bottomless)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChestState {
    WaitingToAppear,
    Appearing,
    Closed,
    Opening,
    Opened,
}
impl_hash_enum!(ChestState);

/// Appear modes (§6.1; Versus only uses Time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appear {
    Normal,
    /// When no Enemy exists.
    Enemies,
    /// When an archer overlaps the hidden chest.
    Touch,
    /// When every BGTorch is lit.
    Torches,
    /// On `event_spawn`.
    Event,
    Time,
}

impl Appear {
    pub fn from_name(n: &str) -> Option<Appear> {
        Some(match n {
            "Normal" => Appear::Normal,
            "Enemies" => Appear::Enemies,
            "Touch" => Appear::Touch,
            "Torches" => Appear::Torches,
            "Event" => Appear::Event,
            "Time" => Appear::Time,
            _ => return None,
        })
    }
}

impl ChestType {
    pub fn from_name(n: &str) -> Option<ChestType> {
        Some(match n {
            "Normal" => ChestType::Normal,
            "AutoOpen" => ChestType::AutoOpen,
            "Large" => ChestType::Large,
            "Special" => ChestType::Special,
            "Bottomless" => ChestType::Bottomless,
            _ => return None,
        })
    }
}
impl_hash_enum!(Appear);

pub const SMALL_HITBOX: Hitbox = Hitbox::wrapping(10.0, 10.0, -5.0, -5.0);
pub const LARGE_HITBOX: Hitbox = Hitbox::wrapping(20.0, 14.0, -10.0, -9.0);
pub const DEPTH: i32 = -60;
pub const OPEN_FRAMES: f32 = 40.0;
pub const MAX_FALL: f32 = 3.6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chest {
    pub ty: ChestType,
    pub state: ChestState,
    pub appear: Appear,
    pub timer: f32,
    pub contents: [Option<Pickups>; 3],
    pub vspeed: f32,
    pub opener: i8,
    /// Frames into the opening tween.
    pub open_t: f32,
    /// Large chests jitter after an arrow hit.
    pub shake: f32,
    /// Bottomless loop: the item out now, wait counter, phase.
    pub current: Option<EntityId>,
    pub wait: f32,
    pub phase: u8,
    // presentation
    pub scale: Vec2,
}

impl_hash!(Chest {
    ty,
    state,
    appear,
    timer,
    contents,
    vspeed,
    opener,
    open_t,
    shake,
    current,
    wait,
    phase
});

/// A chest to create (`TreasureChest(position, type, appear, timer, items)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChestSpawn {
    /// The level element's position (the chest origin is 5 px lower).
    pub pos: Vec2,
    pub ty: ChestType,
    pub timer: f32,
    pub contents: [Option<Pickups>; 3],
}

/// Add a chest to the level.
pub fn create(w: &mut World, s: &ChestSpawn, appear: Appear) -> EntityId {
    let hb = if s.ty.large() {
        LARGE_HITBOX
    } else {
        SMALL_HITBOX
    };
    let mut core = Core::new(
        s.pos + Vec2::new(0.0, 5.0),
        Collider::Box(hb),
        DEPTH,
        &[
            Tag::TreasureChest,
            Tag::PlayerCollider,
            Tag::PlayerGhostCollider,
            Tag::LightSource,
            Tag::Target,
            Tag::Actor,
        ],
    );
    core.screen_wrap = true;
    let empty = s.contents.iter().all(|c| c.is_none());
    let state = if empty {
        ChestState::Opened
    } else if appear != Appear::Normal {
        core.visible = false;
        core.collidable = false;
        ChestState::WaitingToAppear
    } else {
        ChestState::Closed
    };
    if empty {
        core.tags &= !(Tag::PlayerCollider.bit() | Tag::PlayerGhostCollider.bit());
    }
    // In solo modes a chest that starts Closed is a homing target.
    core.seek = state == ChestState::Closed && crate::coop::solo_mode(w);
    w.add(
        core,
        Kind::Chest(Chest {
            ty: s.ty,
            state,
            appear,
            timer: s.timer,
            contents: s.contents,
            vspeed: 0.0,
            opener: -1,
            open_t: 0.0,
            shake: 0.0,
            current: None,
            wait: 0.0,
            phase: 0,
            scale: Vec2::ONE,
        }),
    )
}

pub fn update(w: &mut World, c: &Content, id: EntityId, ch: &mut Chest) {
    let tm = w.time.time_mult;
    ch.scale.x = approach(ch.scale.x, 1.0, 0.01 * tm);
    ch.scale.y = approach(ch.scale.y, 1.0, 0.01 * tm);
    if ch.shake > 0.0 {
        ch.shake -= tm;
    }
    w.core_mut(id).flash.update(tm);
    match ch.state {
        ChestState::WaitingToAppear => {
            if ready_to_appear(w, c, id, ch) {
                appear(w, id, ch);
            }
            return;
        }
        ChestState::Appearing => {
            if !w.core(id).flash.flashing {
                ch.state = ChestState::Closed;
                w.core_mut(id).collidable = true;
            } else {
                return;
            }
        }
        _ => {}
    }
    // Gravity (Closed and after).
    if !actor::check_below(w, c, id) {
        ch.vspeed = approach(ch.vspeed, MAX_FALL, 0.2 * tm);
        if actor::move_v(w, c, id, ch.vspeed * tm).is_some() {
            if ch.vspeed > 0.0 {
                ch.scale = Vec2::new(1.25, 0.75);
                let name = if ch.ty.large() {
                    "sfx_chestDropBig"
                } else {
                    "sfx_chestDropSmall"
                };
                w.sound(name, w.core(id).pos.x);
            }
            ch.vspeed = 0.0;
        }
    }
    actor::enforce_screen_wrap(w, c, id);
    match ch.state {
        ChestState::Closed => {
            if ch.ty == ChestType::AutoOpen && actor::check_below(w, c, id) {
                open_chest(w, c, id, ch, -1);
            }
        }
        ChestState::Opening => {
            ch.open_t += tm;
            if ch.open_t >= OPEN_FRAMES {
                ch.state = ChestState::Opened;
            }
        }
        _ => {}
    }
    if ch.ty == ChestType::Bottomless && ch.state >= ChestState::Opening {
        bottomless(w, c, id, ch);
    }
}

/// `ReadyToAppear` per appear mode (§6.1/§6.2).
fn ready_to_appear(w: &World, c: &Content, id: EntityId, ch: &mut Chest) -> bool {
    match ch.appear {
        Appear::Normal | Appear::Event => false,
        Appear::Enemies => w.tagged(Tag::Enemy).is_empty(),
        Appear::Touch => w.check_at(c, id, w.core(id).pos, Tag::Player),
        Appear::Torches => w.tagged(Tag::BGTorch).iter().all(|&t| {
            !matches!(
                w.kind(t),
                Some(Kind::Tower(crate::entities::Tower::Torch(tc))) if !tc.lit
            )
        }),
        Appear::Time => {
            if pickups::round_started(w) && ch.timer > 0.0 {
                ch.timer -= w.time.time_mult;
            }
            ch.timer <= 0.0
        }
    }
}

/// `Appear()`: visible, 1.2x scale, flashing (non-collidable) for 90
/// (large) or 30 frames.
fn appear(w: &mut World, id: EntityId, ch: &mut Chest) {
    ch.state = ChestState::Appearing;
    ch.scale = Vec2::new(1.2, 1.2);
    let core = w.core_mut(id);
    core.visible = true;
    let frames = if ch.ty.large() { 90.0 } else { 30.0 };
    core.flash
        .start_for(frames, crate::entity::FlashFinish::None);
    let name = if ch.ty.large() {
        "sfx_chestAppearBig"
    } else {
        "sfx_chestAppearSmall"
    };
    w.sound(name, w.core(id).pos.x);
}

/// `EventSpawn()`: an Event-mode chest still waiting appears now.
pub fn event_spawn(w: &mut World, id: EntityId) {
    if let Some(Kind::Chest(mut ch)) = w.kind(id).copied()
        && ch.appear == Appear::Event
        && ch.state == ChestState::WaitingToAppear
    {
        appear(w, id, &mut ch);
        if let Some(Kind::Chest(k)) = w.kind_mut(id) {
            *k = ch;
        }
    }
}

/// The bottomless chest's endless supply (§6.5).
fn bottomless(w: &mut World, c: &Content, id: EntityId, ch: &mut Chest) {
    let tm = w.time.time_mult;
    match ch.phase {
        0 => {
            let out = ch
                .current
                .is_some_and(|e| w.exists(e) && !w.core(e).marked_for_removal);
            if !out {
                ch.wait = 90.0;
                ch.phase = 1;
            }
        }
        1 => {
            ch.wait -= tm;
            if ch.wait <= 0.0 {
                w.core_mut(id)
                    .flash
                    .start_for(10.0, crate::entity::FlashFinish::None);
                let t = get_treasure_spawn(w, 0);
                let at = w.core(id).pos;
                let target = target_position(w, c, at, 0);
                ch.current = Some(pickups::create(w, t, at, target, -1, true));
                w.sound("sfx_bottomlessTreasure", at.x);
                ch.wait = 2.0;
                ch.phase = 2;
            }
        }
        _ => {
            ch.wait -= tm;
            if ch.wait <= 0.0 {
                ch.phase = 0;
            }
        }
    }
}

/// `GetTargetPositionFromChest(chest, i)` (§3.2).
pub fn target_position(w: &World, c: &Content, chest: Vec2, i: usize) -> Vec2 {
    let dx = if i.is_multiple_of(2) {
        -16.0 * (i / 2) as f32
    } else {
        16.0 * i.div_ceil(2) as f32
    };
    let mut p = chest + Vec2::new(dx, -1.0);
    for _ in 0..4 {
        let probe = Placed {
            pos: p,
            collider: Collider::Box(Hitbox::new(4.0, 8.0, -2.0, -12.0)),
        };
        if w.any(
            c,
            &probe,
            Tag::Solid,
            EntityId {
                index: u32::MAX,
                generation: 0,
            },
        ) {
            break;
        }
        p.y -= 8.0;
    }
    p
}

/// `OpenChest(opener)`.
pub fn open_chest(w: &mut World, c: &Content, id: EntityId, ch: &mut Chest, opener: i8) {
    if ch.state >= ChestState::Opening {
        return;
    }
    w.tag(id, Tag::PlayerCollider, false);
    w.tag(id, Tag::PlayerGhostCollider, false);
    w.core_mut(id).seek = false;
    ch.state = ChestState::Opening;
    ch.open_t = 0.0;
    ch.opener = opener;
    ch.scale = Vec2::new(0.8, 1.2);
    let at = w.core(id).pos;
    w.sound("sfx_unlockChest", at.x);
    w.emit(Event::Particles {
        kind: ParticleKind::ChestOpen,
        player: u8::MAX,
        pos: at,
        count: 1,
        dir: 0.0,
    });
    let bottomless = ch.ty == ChestType::Bottomless;
    for (i, item) in ch.contents.iter().enumerate() {
        let Some(t) = *item else { continue };
        let target = target_position(w, c, at, i);
        let e = pickups::create(w, t, at, target, opener, bottomless);
        if bottomless {
            ch.current = Some(e);
            break;
        }
    }
}

/// Touched by an archer (PlayerCollider pass with the target box).
pub fn on_player_collide(w: &mut World, c: &Content, e: EntityId, _pid: EntityId, p: &mut Player) {
    let Some(Kind::Chest(mut ch)) = w.kind(e).copied() else {
        return;
    };
    if ch.state == ChestState::Closed && !w.core(e).flash.flashing {
        open_chest(w, c, e, &mut ch, p.index as i8);
        w.put_kind(e, Kind::Chest(ch));
    }
}

/// `TreasureChest.OnArrowHit` (arrows.md §11).
pub fn on_arrow_hit(w: &mut World, c: &Content, e: EntityId, aid: EntityId, a: &mut Arrow) -> bool {
    let Some(Kind::Chest(mut ch)) = w.kind(e).copied() else {
        return false;
    };
    if ch.state != ChestState::Closed {
        return false;
    }
    if ch.ty.large() {
        ch.shake = 10.0;
    } else {
        open_chest(w, c, e, &mut ch, a.player_index);
    }
    w.put_kind(e, Kind::Chest(ch));
    arrow::enter_fall_mode(w, aid, a, true, false, true);
    true
}

/// Opened by a falling world orb (tower entity hook).
/// `OnPlayerGhostCollide`: a ghost opens a small chest (not while it
/// flashes) with its contents replaced by one Bomb (`OpenChestForceBomb`).
pub fn ghost_open(w: &mut World, c: &Content, e: EntityId, owner: i8) {
    if w.core(e).flash.flashing {
        return;
    }
    if let Some(Kind::Chest(mut ch)) = w.kind(e).copied()
        && !ch.ty.large()
        && ch.state < ChestState::Opening
    {
        ch.contents = [Some(Pickups::Bomb), None, None];
        open_chest(w, c, e, &mut ch, owner);
        w.put_kind(e, Kind::Chest(ch));
        crate::entities::hazards::rumble_pub(w, owner, 0.5, 12);
    }
}

pub fn open_by(w: &mut World, c: &Content, e: EntityId, opener: i8) {
    if let Some(Kind::Chest(mut ch)) = w.kind(e).copied() {
        open_chest(w, c, e, &mut ch, opener);
        w.put_kind(e, Kind::Chest(ch));
    }
}

// ------------------------------------------------------------- spawner

/// `DefaultTreasureChances` (indexed by `Pickups`).
pub const DEFAULT_CHANCES: [f32; 20] = [
    1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 0.5, 0.5, 0.25, 0.15, 0.15, 0.15, 0.15,
    0.001, 0.1,
];

/// `ChestChances[n - 2]`: chance that one more small chest spawns given
/// how many exist. Rows 2–4 from the Steam build, 5–8 from the 8-player
/// build, 9–12 the port's 12-player design (versus.md §15.5).
pub const CHEST_CHANCES: [&[f32]; 11] = [
    &[0.9, 0.9, 0.2, 0.1],
    &[0.9, 0.9, 0.8, 0.2, 0.1],
    &[0.9, 0.9, 0.6, 0.8, 0.2, 0.1],
    &[0.9, 0.9, 0.8, 0.8, 0.2, 0.1],
    &[0.9, 0.9, 0.9, 0.8, 0.4, 0.1],
    &[0.9, 0.9, 0.9, 0.8, 0.4, 0.2],
    &[0.9, 0.9, 0.9, 0.9, 0.4, 0.2, 0.1],
    &[0.9, 0.9, 0.9, 0.9, 0.5, 0.3, 0.1],
    &[0.9, 0.9, 0.9, 0.9, 0.6, 0.4, 0.2],
    &[0.9, 0.9, 0.9, 0.9, 0.9, 0.5, 0.3, 0.1],
    &[0.9, 0.9, 0.9, 0.9, 0.9, 0.6, 0.4, 0.2, 0.1],
];

pub fn chest_chances(n: usize) -> &'static [f32] {
    CHEST_CHANCES[n.clamp(2, 12) - 2]
}

fn bits(list: &[Pickups]) -> u32 {
    list.iter().fold(0, |m, p| m | p.bit())
}

/// `GetItemExclusions(customTower)` plus the unlock/DLC additions (§8.1).
pub fn item_exclusions(w: &World) -> u32 {
    use Pickups as P;
    let s = &w.setup.items;
    let v = |x| variant_any(w, x);
    let mut m = 0u32;
    let table: &[(&[Variant], &[Pickups])] = &[
        (&[Variant::MaxArrows, Variant::NoExtraArrows], &[P::Arrows]),
        (
            &[Variant::StartWithShields, Variant::NoShield],
            &[P::Shield],
        ),
        (&[Variant::StartWithWings, Variant::NoWings], &[P::Wings]),
        (
            &[Variant::StartWithSpeedBoots, Variant::NoSpeedBoots],
            &[P::SpeedBoots],
        ),
        (
            &[Variant::StartInvisible, Variant::NoLookingGlass],
            &[P::Mirror],
        ),
        (&[Variant::NoBombArrows], &[P::BombArrows]),
        (&[Variant::NoLaserArrows], &[P::LaserArrows]),
        (&[Variant::NoBrambleArrows], &[P::BrambleArrows]),
        (&[Variant::NoDrillArrows], &[P::DrillArrows]),
        (&[Variant::NoBoltArrows], &[P::BoltArrows]),
        (&[Variant::NoSuperBombArrows], &[P::SuperBombArrows]),
        (&[Variant::NoFeatherArrows], &[P::FeatherArrows]),
        (&[Variant::NoTriggerArrows], &[P::TriggerArrows]),
        (&[Variant::NoPrismArrows], &[P::PrismArrows]),
        (
            &[Variant::AlwaysDark, Variant::NoDarkOrb],
            &[P::DarkOrb, P::ChaosOrb],
        ),
        (
            &[Variant::SlowTime, Variant::NoTimeOrb],
            &[P::TimeOrb, P::ChaosOrb],
        ),
        (
            &[Variant::AlwaysLava, Variant::NoLavaOrb],
            &[P::LavaOrb, P::ChaosOrb],
        ),
        (
            &[Variant::AlwaysScrolling, Variant::NoSpaceOrb],
            &[P::SpaceOrb, P::ChaosOrb],
        ),
        (&[Variant::NoBomb], &[P::Bomb]),
    ];
    for (vs, ps) in table {
        if vs.iter().any(|&x| v(x)) {
            m |= bits(ps);
        }
    }
    if w.setup.variants.slow_time {
        m |= bits(&[P::TimeOrb, P::ChaosOrb]);
    }
    if !s.custom_tower {
        if !s.unlocked_bolt {
            m |= P::BoltArrows.bit();
        }
        if !s.unlocked_super_bomb {
            m |= P::SuperBombArrows.bit();
        }
        if !s.unlocked_feather {
            m |= P::FeatherArrows.bit();
        }
    }
    if !s.dark_world {
        m |= bits(&[P::TriggerArrows, P::PrismArrows]);
    }
    if s.remove_scroll_effects {
        m |= P::SpaceOrb.bit();
    }
    m
}

/// Whether arrow pick-up `p` (1..=9) is available (unlocks, DLC).
fn arrow_available(w: &World, p: Pickups) -> bool {
    let s = &w.setup.items;
    match p {
        Pickups::BoltArrows => s.unlocked_bolt,
        Pickups::SuperBombArrows => s.unlocked_super_bomb,
        Pickups::FeatherArrows => s.unlocked_feather,
        Pickups::TriggerArrows | Pickups::PrismArrows => s.dark_world,
        _ => true,
    }
}

/// `GetArrowShufflePickups()`: two special arrow pick-ups.
pub fn arrow_shuffle_pickups(w: &mut World, excl: u32) -> Vec<Pickups> {
    let mut pool: Vec<Pickups> = (1..=9)
        .map(Pickups::from_index)
        .filter(|&p| excl & p.bit() == 0 && arrow_available(w, p))
        .collect();
    w.rng.shuffle(&mut pool);
    let k = pool.len().saturating_sub(2);
    pool.split_off(k)
}

/// The `TreasureSpawner` constructor: `TreasureRates` for the match (§7.3).
pub fn compute_rates(w: &mut World) -> [f32; 20] {
    let s = w.setup.items;
    let excl = item_exclusions(w);
    let ignore = variant_any(w, Variant::IgnoreTowerItemSet);
    let mut rates = [0.0f32; 20];
    for (i, r) in rates.iter_mut().enumerate() {
        let mask = s.treasure_mask.map(|m| m[i] as f32).unwrap_or(1.0);
        *r = if ignore {
            DEFAULT_CHANCES[i]
        } else {
            mask * DEFAULT_CHANCES[i]
        };
    }
    if (s.tower_arrow_shuffle && !s.custom_tower) || variant_any(w, Variant::ArrowShuffle) {
        for r in &mut rates[1..=9] {
            *r = 0.0;
        }
        for p in arrow_shuffle_pickups(w, excl) {
            rates[p as usize] = DEFAULT_CHANCES[p as usize];
        }
    }
    for (i, r) in rates.iter_mut().enumerate() {
        if excl & (1 << i) != 0 {
            *r = 0.0;
        }
    }
    let special = if ignore { 0.6 } else { s.special_arrow_rate };
    adjust_for_special_arrows(&mut rates, special);
    rates
}

/// `AdjustTreasureRatesForSpecialArrows` (§7.3 step 5).
pub fn adjust_for_special_arrows(rates: &mut [f32; 20], special: f32) {
    if rates[14..=17].contains(&0.0) {
        rates[18] = 0.0;
    }
    let total: f32 = rates.iter().filter(|&&r| r > 0.0).sum();
    let arrow_sum: f32 = rates[..=9].iter().filter(|&&r| r > 0.0).sum();
    let has_arrow = rates[..=9].iter().any(|&r| r > 0.0);
    let has_other = rates[10..].iter().any(|&r| r > 0.0);
    if !has_arrow || !has_other {
        return;
    }
    let new_sum = (total - arrow_sum) / (1.0 - special) * special;
    for r in &mut rates[..=9] {
        if *r > 0.0 {
            *r *= new_sum / arrow_sum;
        }
    }
}

/// `CanProvideTreasure(extraExclusions)`.
pub fn can_provide(w: &World, excl: u32) -> bool {
    w.items
        .rates
        .iter()
        .enumerate()
        .any(|(i, &r)| r > 0.0 && excl & (1 << i) == 0)
}

/// `GetTreasureSpawn(extraExclusions)`: the cumulative weighted walk (§7.4).
pub fn get_treasure_spawn(w: &mut World, excl: u32) -> Pickups {
    let rates = w.items.rates;
    let total: f32 = rates
        .iter()
        .enumerate()
        .filter(|(i, _)| excl & (1 << i) == 0)
        .map(|(_, &r)| r)
        .sum();
    let x = w.rng.next_float() * total;
    let mut acc = 0.0;
    for (i, &r) in rates.iter().enumerate().take(19) {
        if r > 0.0 && excl & (1 << i) == 0 {
            acc += r;
            if acc > x {
                return Pickups::from_index(i);
            }
        }
    }
    Pickups::Bomb
}

fn take_mirror(list: &mut Vec<Vec2>, p: Vec2, width: f32) -> Option<Vec2> {
    if p.x == width / 2.0 {
        return None;
    }
    let m = Vec2::new(width - p.x, p.y);
    let i = list.iter().position(|&q| q == m)?;
    Some(list.remove(i))
}

/// `GetChestSpawnsForLevel(chests, bigChests)` (§7.5). `n` = player count.
pub fn get_chest_spawns_for_level(
    w: &mut World,
    c: &Content,
    small: &[Vec2],
    big: &[Vec2],
    n: usize,
) -> Vec<ChestSpawn> {
    use Pickups as P;
    let width = c.level(w.level).width as f32;
    let mut small = small.to_vec();
    let mut big = big.to_vec();
    w.rng.shuffle(&mut small);
    w.rng.shuffle(&mut big);
    let mut out = vec![];
    let v = |w: &World, x| variant_any(w, x);
    if v(w, Variant::NoTreasure) {
        return out;
    }
    let big_variants = v(w, Variant::AlwaysBigTreasure)
        || v(w, Variant::BottomlessTreasure)
        || v(w, Variant::BombChests);
    if v(w, Variant::TreasureDraft) && !big_variants {
        return out;
    }
    let n = n.max(2);
    if v(w, Variant::BombChests) {
        let add = if n > 4 { 90 } else { 150 };
        let mut timer = 0;
        let mut list = small;
        while !list.is_empty() {
            let p = list.remove(0);
            timer += w.rng.range_i(30, add);
            let spawn = ChestSpawn {
                pos: p,
                ty: ChestType::AutoOpen,
                timer: timer as f32,
                contents: [Some(P::Bomb), None, None],
            };
            out.push(spawn);
            if v(w, Variant::SymmetricalTreasure)
                && let Some(m) = take_mirror(&mut list, p, width)
            {
                out.push(ChestSpawn { pos: m, ..spawn });
            }
        }
        return out;
    }
    let max_treasure = v(w, Variant::MaxTreasure);
    if !max_treasure {
        let bottomless = v(w, Variant::BottomlessTreasure);
        let mut excl = if bottomless { 0 } else { P::Bomb.bit() };
        let is_big = v(w, Variant::AlwaysBigTreasure) || bottomless || w.rng.chance(0.03);
        if !big.is_empty() && is_big && can_provide(w, excl) {
            let mut items = [None; 3];
            for slot in &mut items {
                if !can_provide(w, excl) {
                    break;
                }
                let t = get_treasure_spawn(w, excl);
                if t >= P::Shield {
                    excl |= t.bit();
                }
                *slot = Some(t);
            }
            w.rng.shuffle(&mut big);
            let timer = w.rng.range_i(30, n.min(8) as i32 * 30) as f32;
            let (ty, contents) = if bottomless {
                (ChestType::Bottomless, [items[0], None, None])
            } else {
                (ChestType::Large, items)
            };
            out.push(ChestSpawn {
                pos: big[0],
                ty,
                timer,
                contents,
            });
            return out;
        }
    }
    if small.is_empty() || !can_provide(w, 0) {
        return out;
    }
    let row = chest_chances(n);
    let mut excl = 0u32;
    let mut count = 0usize;
    let (mut wings, mut mirror, mut boots) = (0, 0, 0);
    while !small.is_empty() && can_provide(w, excl) {
        let more = if count >= row.len() {
            false
        } else if max_treasure {
            true
        } else {
            w.rng.chance(row[count])
        };
        if !more {
            break;
        }
        let p = small.remove(0);
        count += 1;
        let timer = w.rng.range_i(10, n.min(8) as i32 * 60) as f32;
        let t = get_treasure_spawn(w, excl);
        let spawn = ChestSpawn {
            pos: p,
            ty: ChestType::Normal,
            timer,
            contents: [Some(t), None, None],
        };
        out.push(spawn);
        if v(w, Variant::SymmetricalTreasure)
            && let Some(m) = take_mirror(&mut small, p, width)
        {
            count += 1;
            out.push(ChestSpawn { pos: m, ..spawn });
        }
        match t {
            P::Wings | P::Mirror | P::SpeedBoots => {
                let k = match t {
                    P::Wings => &mut wings,
                    P::Mirror => &mut mirror,
                    _ => &mut boots,
                };
                *k += 1;
                if *k >= n {
                    excl |= t.bit();
                }
            }
            P::TimeOrb | P::DarkOrb | P::LavaOrb | P::SpaceOrb => {
                excl |= t.bit() | P::ChaosOrb.bit();
            }
            P::ChaosOrb => {
                excl |= bits(&[P::TimeOrb, P::DarkOrb, P::LavaOrb, P::SpaceOrb, P::ChaosOrb]);
            }
            _ => {}
        }
    }
    out
}

/// `SpawnTreasureChestsVersus` (§7.1): at round start, unless overtime.
pub fn spawn_versus_chests(w: &mut World, c: &Content, n: usize, overtime: bool) -> Vec<EntityId> {
    let lvl = c.level(w.level);
    // `CanSpawnTreasure`: never with a White Archer prison in the level.
    let prison = lvl
        .tower
        .placements
        .iter()
        .any(|p| p.kind == crate::entities::placement::PlacementKind::WhiteArcherPrison);
    if overtime || prison {
        return vec![];
    }
    let (small, big) = (lvl.treasure.clone(), lvl.big_treasure.clone());
    let spawns = get_chest_spawns_for_level(w, c, &small, &big, n);
    spawns.iter().map(|s| create(w, s, Appear::Time)).collect()
}

/// Everything item-related that happens at `OnRoundStart`: chests (Versus)
/// and the AlwaysDark / AlwaysLava variants.
pub fn round_start(w: &mut World, c: &Content, n: usize, overtime: bool, chests: bool) {
    if chests {
        spawn_versus_chests(w, c, n, overtime);
    }
    pickups::orbs::round_start(w, c);
}

/// `GetRandomArrowType(includeDefaultArrows)` (§7.7).
pub fn get_random_arrow_type(w: &mut World, include_default: bool) -> ArrowType {
    let excl = item_exclusions(w);
    let mut list: Vec<ArrowType> = vec![];
    if include_default && excl & Pickups::Arrows.bit() == 0 {
        list.push(ArrowType::Normal);
    }
    for p in [
        Pickups::BombArrows,
        Pickups::LaserArrows,
        Pickups::BrambleArrows,
        Pickups::DrillArrows,
        Pickups::BoltArrows,
        Pickups::SuperBombArrows,
        Pickups::FeatherArrows,
        Pickups::TriggerArrows,
        Pickups::PrismArrows,
    ] {
        if excl & p.bit() == 0 && arrow_available(w, p) {
            list.push(p.arrow_type().unwrap_or_default());
        }
    }
    if list.is_empty() {
        return ArrowType::Normal;
    }
    list[w.rng.choose_index(list.len())]
}

/// `GetPickupsForTreasureDraft(count)` (§11): `count + 1` offers.
pub fn draft_pickups(
    w: &mut World,
    count: usize,
    all_tied: bool,
    spawn_shield: bool,
    even_teams: bool,
) -> Vec<Pickups> {
    use Pickups as P;
    let mut excl = bits(&[
        P::DarkOrb,
        P::LavaOrb,
        P::SpaceOrb,
        P::TimeOrb,
        P::ChaosOrb,
        P::Bomb,
        P::Shield,
    ]);
    let mut out = vec![];
    if !can_provide(w, excl) {
        return out;
    }
    excl &= !P::Shield.bit();
    if all_tied || spawn_shield || w.rng.chance(0.25) {
        excl |= P::Shield.bit();
    }
    let mut shield_drawn = false;
    while out.len() < count + 1 {
        if !can_provide(w, excl) {
            break;
        }
        let t = get_treasure_spawn(w, excl);
        out.push(t);
        if t == P::Shield {
            shield_drawn = true;
            excl |= P::Shield.bit();
            if even_teams && out.len() < count + 1 {
                out.push(P::Shield);
            }
        } else {
            let copies = out.iter().filter(|&&x| x == t).count();
            if copies == 2 || w.rng.chance(0.25) {
                let tmp = excl & P::Shield.bit() == 0 && !shield_drawn && out.len() < count;
                if tmp {
                    excl |= P::Shield.bit();
                }
                excl |= t.bit();
                if !can_provide(w, excl) {
                    excl &= !t.bit();
                }
                if tmp {
                    excl &= !P::Shield.bit();
                }
            }
        }
        if even_teams && out.len() == count && excl & P::Shield.bit() == 0 {
            excl |= P::Shield.bit();
        }
    }
    out
}
