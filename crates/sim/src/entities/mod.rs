//! Tower entities: every placed object, hazard, platform and gimmick of the
//! Versus towers (`docs/spec/tower-entities.md`; section numbers in comments
//! refer to it).
//!
//! All of them live in the world arena as one [`Kind::Tower`] variant
//! holding a [`Tower`]; per-type behaviour is in the submodules. Shared,
//! round-wide state (round start, lava/miasma handles, the archers' surface
//! and fire state) is [`TowerState`] in `World::tower`. Hooks for other
//! modules:
//!
//! | Caller | Hook |
//! |---|---|
//! | world | [`spawn_level`], [`frame_start`], [`update`], moon-glass toggles around arrows |
//! | player | surfaces ([`surfaces`]), [`player_collide`], [`jump_pad_launch`], fire, [`on_player_death`] |
//! | arrow | [`on_arrow_hit`] (Target protocol, §4.5) |
//! | round logic | [`start_lava`], [`round_started`] (the Versus miasma is `versus::miasma`) |
//! | bombs | [`explosion::spawn`] (§4.3) |

pub mod art;
pub mod blocks;
pub mod draw;
pub mod explosion;
pub mod hazards;
pub mod placement;
pub mod platforms;
pub mod scenery;
pub mod surfaces;

pub use placement::{
    Dir4, LanternType, Placement, PlacementKind, TagClass, ThemeInfo, TowerLevel, classify,
};

use crate::actor;
use crate::arrow::Arrow;
use crate::checksum::{Hasher, StableHash};
use crate::collide::Placed;
use crate::content::Content;
use crate::entity::{EntityId, Kind, Tag};
use crate::events::Event;
use crate::impl_hash;
use crate::math::{Vec2, approach_vec};
use crate::player::Player;
use crate::world::{MAX_PLAYERS, World};

// ------------------------------------------------------------------ events

/// Presentation events of tower entities (sounds with volume, named
/// particle bursts from §15.1, entity-owned looping sounds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EnvEvent {
    /// `count` particles of the named type (§15.1 table, or
    /// `BombParticle:<type>` for §6.11 debris) spread uniformly in
    /// `[pos - range, pos + range)` per axis, centred on direction `dir`.
    Particles {
        kind: &'static str,
        pos: Vec2,
        count: u16,
        range: Vec2,
        dir: f32,
    },
    /// A sound with an explicit volume (pan from `x`).
    Sound {
        name: &'static str,
        x: f32,
        volume: f32,
    },
    /// A looping sound owned by an entity: start, volume update, stop.
    Loop {
        name: &'static str,
        owner: EntityId,
        on: bool,
        volume: f32,
    },
}

// ------------------------------------------------------------------ state

/// Per-archer surface and fire state (player.md §4 steps 9–12, 26).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerSurface {
    /// The mud strip the archer is in (`inMud`).
    pub in_mud: Option<EntityId>,
    pub on_hot_coals: bool,
    /// Fire (`FireControl`) frames left.
    pub fire: f32,
    /// Frames until burning wings fall off (30 when fire starts with wings).
    pub wings_fire: f32,
}
impl_hash!(PlayerSurface {
    in_mud,
    on_hot_coals,
    fire,
    wings_fire
});

/// Round-wide tower-entity state (part of `World`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TowerState {
    /// `SceneBegin` ran for the level's entities.
    pub begun: bool,
    /// `RoundStarted` (switch blocks, exploding spike ball ramp).
    pub round_started: bool,
    pub round_start_frame: u32,
    /// Shift blocks' shared "sound already played this wave" flags (§5.2).
    pub shift_start_played: bool,
    pub shift_end_played: bool,
    pub lava: Option<EntityId>,
    pub king: Option<EntityId>,
    pub players: [PlayerSurface; MAX_PLAYERS],
    /// Kills of the explosion being resolved (`explosion.Kills`; transient).
    pub blast_kills: u32,
}
impl_hash!(TowerState {
    begun,
    round_started,
    round_start_frame,
    shift_start_played,
    shift_end_played,
    lava,
    king,
    players
});

/// Every tower entity type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tower {
    MovingPlatform(platforms::MovingPlatform),
    ShiftBlock(platforms::ShiftBlock),
    LoopPlatform(platforms::LoopPlatform),
    GhostPlatform(platforms::GhostPlatform),
    RotatePlatform(platforms::RotatePlatform),
    SensorBlock(platforms::SensorBlock),
    CrackedPlatform(blocks::CrackedPlatform),
    BreakBlock(blocks::BreakBlock),
    CrumbleWall(blocks::CrumbleWall),
    MoonGlass(blocks::MoonGlass),
    Granite,
    SwitchBlock(blocks::SwitchBlock),
    SwitchControl(blocks::SwitchControl),
    ProximityBlock(blocks::ProximityBlock),
    FakeWall(blocks::FakeWall),
    JumpPad(blocks::JumpPad),
    Ice(surfaces::Ice),
    Mud(surfaces::Mud),
    HotCoals,
    SnowClump(surfaces::SnowClump),
    LavaControl(hazards::LavaControl),
    Lava(hazards::Lava),
    Spikeball(hazards::Spikeball),
    Icicle(hazards::Icicle),
    Orb(hazards::Orb),
    Lantern(scenery::Lantern),
    Chain(scenery::Chain),
    Cobwebs(scenery::Cobwebs),
    Torch(scenery::Torch),
    Decor(scenery::Decor),
    BigMushroom(scenery::BigMushroom),
    Skeleton(scenery::Skeleton),
    WaterDrop(scenery::WaterDrop),
    KingIntro(scenery::KingIntro),
    Dummy(scenery::Dummy),
}

impl StableHash for Tower {
    fn stable_hash(&self, h: &mut Hasher) {
        macro_rules! arms {
            ($($v:ident => $n:expr),* ; $($u:ident => $m:expr),*) => {
                match self {
                    $(Tower::$v(e) => { h.u8($n); e.stable_hash(h) })*
                    $(Tower::$u => h.u8($m),)*
                }
            };
        }
        arms!(
            MovingPlatform => 0, ShiftBlock => 1, LoopPlatform => 2, GhostPlatform => 3,
            RotatePlatform => 4, SensorBlock => 5, CrackedPlatform => 6, BreakBlock => 7,
            CrumbleWall => 8, MoonGlass => 9, SwitchBlock => 11, SwitchControl => 12,
            ProximityBlock => 13, FakeWall => 14, JumpPad => 15, Ice => 16, Mud => 17,
            SnowClump => 19, LavaControl => 20, Lava => 21, Spikeball => 23,
            Icicle => 24, Orb => 25, Lantern => 26, Chain => 27, Cobwebs => 28, Torch => 29,
            Decor => 30, BigMushroom => 31, Skeleton => 32, WaterDrop => 33, KingIntro => 34,
            Dummy => 35;
            Granite => 10, HotCoals => 18
        )
    }
}

// ------------------------------------------------------------------ helpers

#[inline]
pub(crate) fn tm(w: &World) -> f32 {
    w.time.time_mult
}

pub(crate) fn sound(w: &mut World, name: &'static str, x: f32) {
    w.emit(Event::Sound { name, x });
}

pub(crate) fn sound_vol(w: &mut World, name: &'static str, x: f32, volume: f32) {
    w.emit(Event::Env(EnvEvent::Sound { name, x, volume }));
}

pub(crate) fn particles(
    w: &mut World,
    kind: &'static str,
    pos: Vec2,
    count: u16,
    range: Vec2,
    dir: f32,
) {
    w.emit(Event::Env(EnvEvent::Particles {
        kind,
        pos,
        count,
        range,
        dir,
    }));
}

/// Run `f` on a tower entity that is not currently updating.
pub(crate) fn with_tower<R>(
    w: &mut World,
    id: EntityId,
    f: impl FnOnce(&mut World, &mut Tower) -> R,
) -> Option<R> {
    if !matches!(w.kind(id), Some(Kind::Tower(_))) {
        return None;
    }
    let Some(Kind::Tower(mut t)) = w.take_kind(id) else {
        return None;
    };
    let r = f(w, &mut t);
    w.put_kind(id, Kind::Tower(t));
    Some(r)
}

/// Run `f` on a player entity that is not currently updating.
pub(crate) fn with_player<R>(
    w: &mut World,
    id: EntityId,
    f: impl FnOnce(&mut World, &mut Player) -> R,
) -> Option<R> {
    if !matches!(w.kind(id), Some(Kind::Player(_))) {
        return None;
    }
    let Some(Kind::Player(mut p)) = w.take_kind(id) else {
        return None;
    };
    let r = f(w, &mut p);
    w.put_kind(id, Kind::Player(p));
    Some(r)
}

pub(crate) fn tower(w: &World, id: EntityId) -> Option<&Tower> {
    match w.kind(id) {
        Some(Kind::Tower(t)) => Some(t),
        _ => None,
    }
}

pub(crate) fn tower_mut(w: &mut World, id: EntityId) -> Option<&mut Tower> {
    match w.kind_mut(id) {
        Some(Kind::Tower(t)) => Some(t),
        _ => None,
    }
}

/// `Solid.MoveTowards(target, maxStep)` / `JumpThru` likewise.
pub(crate) fn move_towards(
    w: &mut World,
    c: &Content,
    id: EntityId,
    target: Vec2,
    max: f32,
    jump_thru: bool,
) {
    let core = w.core(id);
    let actual = core.pos + core.counter;
    let next = approach_vec(actual, target, max);
    platform_move(w, c, id, next - actual, jump_thru);
}

/// `MoveTo(target)` = `Move(target - actual position)`.
pub(crate) fn move_to(w: &mut World, c: &Content, id: EntityId, target: Vec2, jump_thru: bool) {
    let core = w.core(id);
    let actual = core.pos + core.counter;
    platform_move(w, c, id, target - actual, jump_thru);
}

pub(crate) fn platform_move(w: &mut World, c: &Content, id: EntityId, d: Vec2, jump_thru: bool) {
    if jump_thru {
        crate::platform::jump_thru_move(w, c, id, d);
    } else {
        crate::platform::solid_move(w, c, id, d);
    }
}

/// Any player or corpse rides this jump-thru / solid.
pub(crate) fn ridden_by_body(w: &World, c: &Content, id: EntityId, jump_thru: bool) -> bool {
    w.tagged(Tag::Player)
        .iter()
        .chain(w.tagged(Tag::Corpse))
        .any(|&a| {
            if jump_thru {
                actor::is_riding_jump_thru(w, c, a, id)
            } else {
                actor::is_riding_solid(w, c, a, id)
            }
        })
}

/// Whether any collidable `Actor` overlaps `probe` (switch / cracked /
/// proximity blocks wait for a free space).
pub(crate) fn actor_overlaps(w: &World, c: &Content, probe: &Placed, me: EntityId) -> bool {
    w.any(c, probe, Tag::Actor, me)
}

/// Living archers (`Player` tag, not dead).
pub(crate) fn living_players(w: &World) -> impl Iterator<Item = EntityId> + '_ {
    w.players()
        .filter(|&p| w.player(p).is_some_and(|pl| !pl.dead))
}

// ------------------------------------------------------------------ loading

/// Create the level's tower entities (file order) after the tiles.
pub fn spawn_level(w: &mut World, c: &Content) {
    use crate::variants::Variant;
    let vs = w.setup.variant_set;
    spawn_level_with(w, c, vs.any(Variant::NoHazards));
    // AlwaysLava starts with the round (unfrozen sandboxes start at once).
    if vs.any(Variant::AlwaysLava) && !w.setup.start_frozen {
        start_lava(w, c, -1, true);
    }
}

/// [`spawn_level`] with the NoHazards variant's gating (§3).
pub fn spawn_level_with(w: &mut World, c: &Content, no_hazards: bool) {
    w.tower = TowerState {
        round_started: !w.setup.start_frozen,
        ..TowerState::default()
    };
    let lvl = c.level(w.level);
    let theme = lvl.tower.theme;
    let mut switch = false;
    for p in &lvl.tower.placements {
        if no_hazards && p.kind.is_hazard() {
            continue;
        }
        if matches!(
            p.kind,
            PlacementKind::RedSwitchBlock | PlacementKind::BlueSwitchBlock
        ) {
            switch = true;
        }
        spawn(w, c, p, &theme);
    }
    if switch {
        blocks::spawn_switch_control(w);
    }
}

fn spawn(w: &mut World, c: &Content, p: &Placement, theme: &ThemeInfo) {
    use PlacementKind as K;
    match p.kind {
        K::MovingPlatform => {
            platforms::spawn_moving(w, p, theme);
        }
        K::ShiftBlock => {
            platforms::spawn_shift(w, p);
        }
        K::LoopPlatform => {
            platforms::spawn_loop(w, c, p);
        }
        K::GhostPlatform => {
            platforms::spawn_ghost(w, c, p);
        }
        K::RotatePlatformsCenter => {
            platforms::spawn_rotate_ring(w, p);
        }
        K::SensorBlock => {
            platforms::spawn_sensor(w, p);
        }
        // `CataclysmBlock`: bobs on a sine between start and node
        // (Dark World Cataclysm and the Dark Gauntlet).
        K::PrismBlock => {
            crate::darkworld::cataclysm::spawn_block(w, p);
        }
        K::CrackedPlatform => {
            blocks::spawn_cracked_platform(w, p);
        }
        K::CrackedWall => {
            blocks::spawn_cracked_wall(w, c, p);
        }
        K::CrumbleBlock => {
            blocks::spawn_crumble_block(w, c, p);
        }
        K::CrumbleWall => {
            blocks::spawn_crumble_wall(w, c, p);
        }
        K::MoonGlassBlock => {
            blocks::spawn_moon_glass(w, p);
        }
        K::GraniteBlock => {
            blocks::spawn_granite(w, p, true);
        }
        K::RedSwitchBlock => {
            blocks::spawn_switch(w, c, p, true);
        }
        K::BlueSwitchBlock => {
            blocks::spawn_switch(w, c, p, false);
        }
        K::ProximityBlock => {
            blocks::spawn_proximity(w, p);
        }
        K::FakeWall => {
            blocks::spawn_fake_wall(w, p);
        }
        K::JumpPad => {
            blocks::spawn_jump_pad(w, p);
        }
        K::Ice => {
            surfaces::spawn_ice(w, p);
        }
        K::Mud => {
            surfaces::spawn_mud(w, p);
        }
        K::HotCoals => {
            surfaces::spawn_hot_coals(w, p);
        }
        K::SnowClump => {
            surfaces::spawn_snow(w, p);
        }
        K::SpikeBall => {
            hazards::spawn_spikeball(w, p);
        }
        K::Icicle => {
            hazards::spawn_icicle(w, p);
        }
        K::Orb => {
            hazards::spawn_orb(w, p, false);
        }
        K::ExplodingOrb => {
            hazards::spawn_orb(w, p, true);
        }
        K::Lantern => {
            scenery::spawn_lantern(w, p);
        }
        K::Chain => {
            scenery::spawn_chain(w, p);
        }
        K::Cobwebs => {
            scenery::spawn_cobwebs(w, c, p);
        }
        K::BGLantern => {
            scenery::spawn_torch(w, p, theme);
        }
        K::BGCrystal
        | K::BGMushroom
        | K::GhostShipWindow
        | K::PirateBanner
        | K::RainDrops
        | K::WhiteArcherPrison
        | K::Ogmo => {
            scenery::spawn_decor(w, p);
        }
        K::BGBigMushroom => {
            scenery::spawn_big_mushroom(w, p);
        }
        K::BGSkeleton => {
            scenery::spawn_skeleton(w, p);
        }
        K::WaterDrop => {
            scenery::spawn_water_drop(w, p);
        }
        K::KingIntro => {
            scenery::spawn_king(w, p);
        }
        K::Dummy => {
            scenery::spawn_dummy(w, p);
        }
    }
}

/// Called by `World::step` after the entity lists were updated: runs the
/// level's `SceneBegin` once (the archers are in the scene by then).
pub fn frame_start(w: &mut World, c: &Content) {
    if w.tower.begun {
        return;
    }
    w.tower.begun = true;
    let ids: Vec<EntityId> = w
        .entities()
        .filter(|&e| matches!(w.kind(e), Some(Kind::Tower(_))))
        .collect();
    for id in ids {
        with_tower(w, id, |w, t| scene_begin(w, c, id, t));
    }
}

fn scene_begin(w: &mut World, c: &Content, id: EntityId, t: &mut Tower) {
    match t {
        Tower::SensorBlock(e) => platforms::sensor_begin(w, c, id, e),
        Tower::ProximityBlock(e) => blocks::proximity_begin(w, c, id, e),
        Tower::SnowClump(e) => surfaces::snow_begin(w, c, id, e),
        Tower::Lantern(e) => scenery::lantern_begin(w, c, id, e),
        Tower::Skeleton(e) => scenery::skeleton_begin(w, c, id, e),
        Tower::Dummy(e) => scenery::dummy_begin(w, c, id, e),
        _ => {}
    }
}

// ------------------------------------------------------------------ update

/// Per-frame update of a tower entity (`World::update_entity`).
pub fn update(w: &mut World, c: &Content, id: EntityId, t: &mut Tower) {
    match t {
        Tower::MovingPlatform(e) => platforms::update_moving(w, c, id, e),
        Tower::ShiftBlock(e) => platforms::update_shift(w, c, id, e),
        Tower::LoopPlatform(e) => platforms::update_loop(w, c, id, e),
        Tower::GhostPlatform(e) => platforms::update_ghost(w, c, id, e),
        Tower::RotatePlatform(e) => platforms::update_rotate(w, c, id, e),
        Tower::SensorBlock(e) => platforms::update_sensor(w, c, id, e),
        Tower::CrackedPlatform(e) => blocks::update_cracked_platform(w, c, id, e),
        Tower::BreakBlock(e) => blocks::update_break_block(w, c, id, e),
        Tower::CrumbleWall(e) => blocks::update_crumble_wall(w, c, id, e),
        Tower::MoonGlass(e) => blocks::update_moon_glass(w, c, id, e),
        Tower::SwitchBlock(e) => blocks::update_switch(w, c, id, e),
        Tower::SwitchControl(e) => blocks::update_switch_control(w, c, id, e),
        Tower::ProximityBlock(e) => blocks::update_proximity(w, c, id, e),
        Tower::FakeWall(e) => blocks::update_fake_wall(w, c, id, e),
        Tower::JumpPad(e) => blocks::update_jump_pad(w, c, id, e),
        Tower::Ice(e) => surfaces::update_ice(w, c, id, e),
        Tower::Mud(e) => surfaces::update_mud(w, c, id, e),
        Tower::SnowClump(e) => surfaces::update_snow(w, c, id, e),
        Tower::LavaControl(e) => hazards::update_lava_control(w, c, id, e),
        Tower::Lava(e) => hazards::update_lava(w, c, id, e),
        Tower::Spikeball(e) => hazards::update_spikeball(w, c, id, e),
        Tower::Icicle(e) => hazards::update_icicle(w, c, id, e),
        Tower::Orb(e) => hazards::update_orb(w, c, id, e),
        Tower::Lantern(e) => scenery::update_lantern(w, c, id, e),
        Tower::Chain(e) => scenery::update_chain(w, c, id, e),
        Tower::Torch(e) => scenery::update_torch(w, c, id, e),
        Tower::BigMushroom(e) => scenery::update_big_mushroom(w, c, id, e),
        Tower::Skeleton(e) => scenery::update_skeleton(w, c, id, e),
        Tower::WaterDrop(e) => scenery::update_water_drop(w, c, id, e),
        Tower::KingIntro(e) => scenery::update_king(w, c, id, e),
        Tower::Cobwebs(e) => scenery::update_cobwebs(w, id, e),
        Tower::Dummy(e) => scenery::update_dummy(w, c, id, e),
        Tower::Granite | Tower::HotCoals | Tower::Decor(_) => {}
    }
}

// ------------------------------------------------------------------ protocols

/// Player-collider protocol (§4.4): `PlayerCollider` tower entity `e`
/// overlaps the archer (called from `player_collider_overlaps`, archer held).
pub fn on_player_collide(w: &mut World, c: &Content, e: EntityId, pid: EntityId, p: &mut Player) {
    with_tower(w, e, |w, t| match t {
        Tower::SnowClump(s) => surfaces::snow_player(w, c, e, s, pid, p),
        Tower::Spikeball(s) => hazards::spikeball_player(w, c, e, s, pid, p),
        Tower::Icicle(s) => hazards::icicle_player(w, c, e, s, pid, p),
        Tower::Orb(s) => hazards::orb_player(w, c, e, s, pid, p),
        Tower::Lantern(s) => scenery::lantern_fall(w, e, s),
        Tower::BigMushroom(s) => scenery::big_mushroom_wiggle(w, e, s, p.speed.x * 0.4),
        Tower::Skeleton(s) => scenery::skeleton_crumble(w, e, s),
        Tower::Dummy(s) => scenery::dummy_player(w, c, e, s, pid, p),
        _ => {}
    });
}

/// Target protocol (§4.5): a dangerous arrow overlaps a tower target.
/// Returns true when the arrow's processing stops for this frame.
pub fn on_arrow_hit(
    w: &mut World,
    c: &Content,
    target: EntityId,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    with_tower(w, target, |w, t| match t {
        Tower::Spikeball(s) => hazards::spikeball_arrow(w, c, target, s, aid, a),
        Tower::Icicle(s) => hazards::icicle_arrow(w, c, target, s, aid, a),
        Tower::Orb(s) => hazards::orb_arrow(w, c, target, s, aid, a),
        Tower::Lantern(s) => scenery::lantern_arrow(w, target, s, aid, a),
        Tower::BigMushroom(s) => {
            scenery::big_mushroom_wiggle(w, target, s, a.speed.x * 0.3);
            false
        }
        Tower::Dummy(s) => scenery::dummy_arrow(w, target, s, aid, a),
        Tower::Cobwebs(s) => scenery::cobweb_arrow(w, target, s, a),
        _ => false,
    })
    .unwrap_or(false)
}

/// `OnExplodePush(explosion, normal)` for tower entities (§4.3).
pub(crate) fn on_explode_push(w: &mut World, c: &Content, id: EntityId, normal: Vec2, owner: i8) {
    with_tower(w, id, |w, t| match t {
        Tower::CrackedPlatform(e) => blocks::cracked_platform_die(w, id, e),
        Tower::BreakBlock(e) => blocks::break_block_push(w, id, e, normal),
        Tower::CrumbleWall(e) => blocks::crumble_wall_destroy(w, id, e),
        Tower::Icicle(e) => hazards::icicle_explode_push(w, c, id, e, owner),
        Tower::Orb(e) => hazards::orb_fall(w, id, e, owner),
        Tower::Lantern(e) => scenery::lantern_fall(w, id, e),
        Tower::Torch(e) => scenery::torch_relight(w, id, e),
        Tower::Dummy(e) => scenery::dummy_die(w, id, e),
        _ => {}
    });
}

/// `OnSqueakyBounce` (toy arrows): a hanging orb falls for the shooter, a
/// chained lantern pushes its chain 6 px/frame away.
pub(crate) fn on_squeaky_bounce(w: &mut World, id: EntityId, normal: Vec2, owner: i8) {
    with_tower(w, id, |w, t| match t {
        Tower::Orb(e) if !e.falling => hazards::orb_fall(w, id, e, owner),
        Tower::Lantern(e) => {
            if let Some(ch) = e.chain {
                let y = w.core(id).pos.y;
                let s = if normal.x > 0.0 {
                    6.0
                } else if normal.x < 0.0 {
                    -6.0
                } else {
                    0.0
                };
                scenery::chain_push_pub(w, ch, y, s);
            }
        }
        _ => {}
    });
}

/// `OnPlayerGhostCollide` of the tower entities.
pub(crate) fn on_player_ghost(
    w: &mut World,
    _c: &Content,
    id: EntityId,
    ghost: EntityId,
    owner: i8,
    ghost_speed_x: f32,
) {
    with_tower(w, id, |w, t| match t {
        Tower::Orb(e) => hazards::orb_ghost(w, id, e, ghost, owner),
        Tower::Icicle(e) => hazards::icicle_ghost(w, id, e, ghost, owner),
        Tower::BigMushroom(e) => scenery::big_mushroom_wiggle(w, id, e, ghost_speed_x * 0.4),
        _ => {}
    });
}

/// Explosion test points (`ExplosionCheckOffsets`) of a tower entity,
/// relative to its position; empty = the position itself.
pub(crate) fn explosion_offsets(w: &World, c: &Content, id: EntityId, out: &mut Vec<Vec2>) {
    match tower(w, id) {
        Some(Tower::BreakBlock(b)) => blocks::break_block_offsets(w, c, id, b, out),
        Some(Tower::CrumbleWall(_)) => out.extend_from_slice(&blocks::WALL_OFFSETS),
        Some(Tower::Dummy(_)) => out.extend_from_slice(&scenery::DUMMY_OFFSETS),
        _ => {}
    }
}

/// Lava-collider protocol (§4.6): `side` 0 bottom, 1 top, 2 left, 3 right.
pub(crate) fn on_lava_collide(w: &mut World, c: &Content, id: EntityId, side: u8) {
    with_tower(w, id, |w, t| match t {
        Tower::Ice(e) => surfaces::ice_melt(w, c, id, e, side),
        Tower::SnowClump(e) => surfaces::snow_melt(w, id, e),
        _ => {}
    });
}

// ------------------------------------------------------------------ hooks

/// Lava from a lava orb (`variant = false`, owner credited, timed) or the
/// AlwaysLava variant (`variant = true`, untimed) (§8.1). A second orb while
/// lava is up extends it.
pub fn start_lava(w: &mut World, c: &Content, owner: i8, variant: bool) {
    hazards::start_lava(w, c, owner, variant);
}

/// The round has started (the archers were unfrozen): switch blocks start
/// counting, the exploding spike ball's ramp starts, the king gets mad.
pub fn round_started(w: &mut World, c: &Content) {
    if !w.tower.round_started {
        w.tower.round_started = true;
    }
    // AlwaysLava (pickups.md §4.3): at round start, not in solo modes.
    if w.setup
        .variant_set
        .any(crate::variants::Variant::AlwaysLava)
        && !crate::coop::solo_mode(w)
        && w.tower.lava.is_none()
    {
        start_lava(w, c, -1, true);
    }
    w.tower.round_start_frame = w.frame;
}

/// `Seek = SoloMode` of hazard orbs, icicles and closed chests (set by
/// the solo modes once their state exists).
pub fn apply_solo_seek(w: &mut World) {
    let solo = crate::coop::solo_mode(w);
    let ids: Vec<EntityId> = w.entities().collect();
    for id in ids {
        let seek = match w.kind(id) {
            Some(Kind::Tower(Tower::Orb(_) | Tower::Icicle(_))) => solo,
            Some(Kind::Chest(ch)) => solo && ch.state == crate::treasure::ChestState::Closed,
            _ => continue,
        };
        w.core_mut(id).seek = seek;
    }
}

/// `KingIntro.DoBossIntro` (animation part): the King starts transforming.
pub fn king_start_transform(w: &mut World) {
    if let Some(k) = w.tower.king {
        with_tower(w, k, |_, t| {
            if let Tower::KingIntro(e) = t {
                scenery::king_transform(e);
            }
        });
    }
}

/// `KingIntro.GetMad()` (only from idle): the Versus start and every Quest
/// wave start.
pub fn king_get_mad(w: &mut World) {
    if let Some(k) = w.tower.king {
        with_tower(w, k, |w, t| {
            if let Tower::KingIntro(e) = t {
                scenery::king_get_mad(w, k, e);
            }
        });
    }
}

/// `RoundLogic.OnPlayerDeath` reaction of the tower (the king laughs).
pub fn on_player_death(w: &mut World) {
    if let Some(k) = w.tower.king {
        with_tower(w, k, |w, t| {
            if let Tower::KingIntro(e) = t {
                scenery::king_laugh(w, k, e);
            }
        });
    }
}

/// `JumpPad.Launch(x)` from a pad jump (player.md §6.7 step 4).
pub fn jump_pad_launch(w: &mut World, pad: EntityId, x: f32) {
    with_tower(w, pad, |w, t| {
        if let Tower::JumpPad(e) = t {
            blocks::jump_pad_launch(w, pad, e, x);
        }
    });
}

/// Moon glass is not solid for arrows: off while an arrow updates (§6.5).
pub fn moonglass_for_arrows(w: &mut World, arrow_updating: bool) {
    let n = w.tagged(Tag::Moonglass).len();
    for i in 0..n {
        let id = w.tagged(Tag::Moonglass)[i];
        w.core_mut(id).collidable = !arrow_updating;
    }
}

pub use explosion::spawn as explode;
pub use surfaces::{
    corpse_ground_friction, hot_coals_under, ice_under, in_mud, mud_splash_up, on_hot_coals,
    player_fire_update, player_mud_tracking, start_fire,
};
