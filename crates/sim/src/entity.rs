//! Entity identity, tags and the per-entity data every Monocle `Entity` /
//! TowerFall `LevelEntity` / `Actor` has ([`Core`]). Type-specific state is
//! in [`Kind`].

use crate::checksum::{Hasher, StableHash};
use crate::collide::{Collider, Hitbox};
use crate::math::Vec2;
use crate::{impl_hash, impl_hash_enum};

/// A stable entity handle: slot index plus generation (a removed entity's
/// handle stops resolving).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId {
    pub index: u32,
    pub generation: u32,
}

impl_hash!(EntityId { index, generation });

/// `GameTags` (engine.md §11). Only the ones gameplay queries are listed;
/// the numbering follows the original's enum order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Tag {
    Player = 0,
    Corpse = 1,
    PlayerGhost = 2,
    Solid = 3,
    SolidOnly = 4,
    JumpThru = 5,
    Actor = 6,
    Arrow = 7,
    JumpPad = 8,
    Target = 9,
    Chain = 10,
    TreasureChest = 11,
    PlayerCollider = 12,
    PlayerCollectible = 13,
    ExplosionCollider = 14,
    LavaCollider = 15,
    PlayerGhostCollider = 16,
    ShockCollider = 17,
    Orb = 18,
    Enemy = 19,
    Brambles = 20,
    Dummy = 21,
    TeamReviver = 22,
    CataBullet = 23,
    LightSource = 24,
    PlayerOnlySolid = 25,
    Moonglass = 26,
    Ice = 27,
    HotCoals = 28,
    Mud = 29,
    Hat = 30,
    BGTorch = 31,
    Granite = 32,
    RespawnStatue = 33,
    DarkWorldBoss = 34,
}

pub const TAG_COUNT: usize = 35;

impl Tag {
    #[inline]
    pub const fn bit(self) -> u64 {
        1u64 << self as u8
    }
    pub fn from_index(i: usize) -> Tag {
        assert!(i < TAG_COUNT);
        // SAFETY-free: match on the dense enum.
        const ALL: [Tag; TAG_COUNT] = [
            Tag::Player,
            Tag::Corpse,
            Tag::PlayerGhost,
            Tag::Solid,
            Tag::SolidOnly,
            Tag::JumpThru,
            Tag::Actor,
            Tag::Arrow,
            Tag::JumpPad,
            Tag::Target,
            Tag::Chain,
            Tag::TreasureChest,
            Tag::PlayerCollider,
            Tag::PlayerCollectible,
            Tag::ExplosionCollider,
            Tag::LavaCollider,
            Tag::PlayerGhostCollider,
            Tag::ShockCollider,
            Tag::Orb,
            Tag::Enemy,
            Tag::Brambles,
            Tag::Dummy,
            Tag::TeamReviver,
            Tag::CataBullet,
            Tag::LightSource,
            Tag::PlayerOnlySolid,
            Tag::Moonglass,
            Tag::Ice,
            Tag::HotCoals,
            Tag::Mud,
            Tag::Hat,
            Tag::BGTorch,
            Tag::Granite,
            Tag::RespawnStatue,
            Tag::DarkWorldBoss,
        ];
        ALL[i]
    }
}

/// `Allegiance`: Neutral -1, Blue 0, Red 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(i8)]
pub enum Allegiance {
    #[default]
    Neutral = -1,
    Blue = 0,
    Red = 1,
    /// Teams 2..5 exist only in our 3-6 team Team Deathmatch (versus.md §15.4).
    Green = 2,
    Yellow = 3,
    Purple = 4,
    Orange = 5,
}

impl Allegiance {
    /// The allegiance of team index `t` (0 = Blue, 1 = Red, 2.. = extra teams).
    pub fn team(t: u8) -> Allegiance {
        match t {
            0 => Allegiance::Blue,
            1 => Allegiance::Red,
            2 => Allegiance::Green,
            3 => Allegiance::Yellow,
            4 => Allegiance::Purple,
            5 => Allegiance::Orange,
            _ => Allegiance::Neutral,
        }
    }
}
impl StableHash for Allegiance {
    fn stable_hash(&self, h: &mut Hasher) {
        h.i32(*self as i8 as i32)
    }
}

/// Layer indices of a `Level` (engine.md §2.1), in creation (= update) order.
pub const LAYERS: [i8; 6] = [-3, 0, 2, 3, 4, 10];
pub const GAMEPLAY_LAYER: i8 = 0;

/// Depth constants (engine.md §2.2).
pub mod depth {
    pub const PLAYER: i32 = -100;
    pub const ARROW: i32 = -70;
    pub const CORPSE: i32 = -50;
    pub const HAT: i32 = -52;
    pub const SOLID: i32 = -150;
}

/// `LevelEntity` flash state (engine.md §10).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Flash {
    pub flashing: bool,
    pub interval: f32,
    pub counter: f32,
    pub delay: f32,
    /// What `OnFlash` toggles (players: `DrawSelf`; others: `Visible`).
    pub shown: bool,
    /// The finish action to run when a timed flash ends.
    pub on_finish: FlashFinish,
}
impl_hash!(Flash {
    flashing,
    interval,
    counter,
    delay,
    shown,
    on_finish
});

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlashFinish {
    #[default]
    None,
    /// Arrow decay: remove the entity.
    RemoveSelf,
}
impl_hash_enum!(FlashFinish);

impl Flash {
    /// `Flash()`.
    pub fn start(&mut self) {
        if !self.flashing {
            self.flashing = true;
            self.interval = 3.0;
            self.shown = false;
        }
        self.delay = 0.0;
        self.counter = 0.0;
        self.on_finish = FlashFinish::None;
    }
    /// `Flash(frames, onFinish)`.
    pub fn start_for(&mut self, frames: f32, on_finish: FlashFinish) {
        self.start();
        self.counter = frames;
        self.on_finish = on_finish;
    }
    /// `StopFlashing()`.
    pub fn stop(&mut self) {
        self.flashing = false;
        self.shown = true;
    }
    /// `UpdateFlash`; returns the finish action when a timed flash ended.
    pub fn update(&mut self, tm: f32) -> Option<FlashFinish> {
        if !self.flashing {
            return None;
        }
        if self.delay > 0.0 {
            self.delay -= tm;
            return None;
        }
        self.interval -= tm;
        if self.interval <= 0.0 {
            self.interval = 3.0;
            self.shown = !self.shown;
        }
        if self.counter > 0.0 {
            self.counter -= tm;
            if self.counter <= 0.0 {
                self.flashing = false;
                self.shown = true;
                return Some(self.on_finish);
            }
        }
        None
    }
}

/// What every entity has: Monocle `Entity` fields, `LevelEntity` targeting
/// and flash, `Actor` movement flags and remainder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Core {
    pub pos: Vec2,
    /// `Actor.counter` / `Solid.counter`: the sub-pixel remainder.
    pub counter: Vec2,
    pub collider: Collider,
    /// `LevelEntity.TargetCollider` (arrows and hazards test this if set).
    pub target_collider: Option<Hitbox>,
    pub active: bool,
    pub visible: bool,
    pub collidable: bool,
    pub depth: i32,
    pub layer: i8,
    pub tags: u64,
    pub screen_wrap: bool,
    pub in_scene: bool,
    pub marked_for_removal: bool,
    /// Sort key within (layer, depth): insertion sequence.
    pub seq: u32,
    // Actor
    pub pushable: bool,
    pub naive_push: bool,
    pub ignore_jump_thrus: bool,
    pub finish_push_on_squish: bool,
    // LevelEntity targeting
    pub seek: bool,
    pub seek_priority: i32,
    pub seek_offset: Vec2,
    pub allegiance: Allegiance,
    pub flash: Flash,
    // Solid
    pub squisher_index: i8,
}

impl_hash!(Core {
    pos,
    counter,
    collider,
    target_collider,
    active,
    visible,
    collidable,
    depth,
    layer,
    tags,
    screen_wrap,
    in_scene,
    marked_for_removal,
    seq,
    pushable,
    naive_push,
    ignore_jump_thrus,
    finish_push_on_squish,
    seek,
    seek_priority,
    seek_offset,
    allegiance,
    flash,
    squisher_index
});

impl Core {
    pub fn new(pos: Vec2, collider: Collider, depth: i32, tags: &[Tag]) -> Core {
        Core {
            pos,
            counter: Vec2::ZERO,
            collider,
            target_collider: None,
            active: true,
            visible: true,
            collidable: true,
            depth,
            layer: GAMEPLAY_LAYER,
            tags: tags.iter().fold(0, |m, t| m | t.bit()),
            screen_wrap: false,
            in_scene: false,
            marked_for_removal: false,
            seq: 0,
            pushable: true,
            naive_push: false,
            ignore_jump_thrus: false,
            finish_push_on_squish: false,
            seek: false,
            seek_priority: 0,
            seek_offset: Vec2::ZERO,
            allegiance: Allegiance::Neutral,
            flash: Flash {
                shown: true,
                ..Flash::default()
            },
            squisher_index: -1,
        }
    }

    #[inline]
    pub fn has_tag(&self, t: Tag) -> bool {
        self.tags & t.bit() != 0
    }

    /// The current collider's local box (the grid's full extent for
    /// `LevelTiles`, a zero box for none).
    #[inline]
    pub fn hitbox(&self) -> Hitbox {
        match self.collider {
            Collider::Box(h) => h,
            Collider::Grid { w, h } => Hitbox::new(w, h, 0.0, 0.0),
            Collider::None => Hitbox::new(0.0, 0.0, 0.0, 0.0),
        }
    }
    #[inline]
    pub fn left(&self) -> f32 {
        self.pos.x + self.hitbox().x
    }
    #[inline]
    pub fn right(&self) -> f32 {
        self.pos.x + self.hitbox().x + self.hitbox().w
    }
    #[inline]
    pub fn top(&self) -> f32 {
        self.pos.y + self.hitbox().y
    }
    #[inline]
    pub fn bottom(&self) -> f32 {
        self.pos.y + self.hitbox().y + self.hitbox().h
    }
    #[inline]
    pub fn width(&self) -> f32 {
        self.hitbox().w
    }
    /// `Bottom = v` setter.
    pub fn set_bottom(&mut self, v: f32) {
        let h = self.hitbox();
        self.pos.y = v - (h.y + h.h);
    }
    pub fn set_top(&mut self, v: f32) {
        self.pos.y = v - self.hitbox().y;
    }
    pub fn set_left(&mut self, v: f32) {
        self.pos.x = v - self.hitbox().x;
    }
    pub fn set_right(&mut self, v: f32) {
        let h = self.hitbox();
        self.pos.x = v - (h.x + h.w);
    }
}

/// Type-specific entity state. Every variant is `Copy` (no heap data), so
/// cloning the arena is a memcpy.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Kind {
    /// `LevelTiles`: the static solid grid (inactive, never updates).
    Tiles,
    /// A static `JumpThru` (one-way platform).
    JumpThru,
    /// A static solid block from the level (jump pads, cracked blocks for now).
    StaticSolid,
    /// A test/sandbox moving solid (see `platform.rs`).
    MovingSolid(crate::platform::MovingSolid),
    Player(crate::player::Player),
    /// One side of the Versus miasma wall.
    Miasma(crate::versus::miasma::Miasma),
    /// Quest / Dark World enemies and the player ghost.
    Enemy(crate::enemies::Enemy),
    /// Enemy spawn portal.
    Portal(crate::enemies::portal::Portal),
    /// Invisible enemy melee box.
    EnemyAttack(crate::enemies::attack::EnemyAttack),
    /// Enemy projectiles and boss props.
    Hazard(crate::enemies::hazard::Hazard),
    /// A corpse that can be revived.
    Reviver(crate::revive::TeamReviver),
    /// Trials target dummy.
    Dummy(crate::trials::Dummy),
    Arrow(crate::arrow::Arrow),
    Corpse(crate::corpse::Corpse),
    Hat(crate::hat::Hat),
    // ---- M5: arrows, treasure, pickups (arrows.md, pickups.md)
    Shock(crate::explosion::Shock),
    Bramble(crate::arrows::brambles::Bramble),
    Prism(crate::arrows::prism::Prism),
    /// One of a prism's four solid bars (collidable only during its
    /// captive's update).
    PrismBar(EntityId),
    Pickup(crate::pickups::Pickup),
    Chest(crate::treasure::Chest),
    /// A tower entity (platforms, blocks, hazards, scenery; `entities`).
    Tower(crate::entities::Tower),
    /// Dark World: exit portal, boss bodies and parts, boss projectiles.
    Dw(crate::darkworld::Part),
    /// Quest floor miasma strip.
    FloorMiasma(crate::quest::floor::FloorMiasma),
}

impl StableHash for Kind {
    fn stable_hash(&self, h: &mut Hasher) {
        match self {
            Kind::Tiles => h.u8(0),
            Kind::JumpThru => h.u8(1),
            Kind::MovingSolid(m) => {
                h.u8(2);
                m.stable_hash(h)
            }
            Kind::Player(p) => {
                h.u8(3);
                p.stable_hash(h)
            }
            Kind::Arrow(a) => {
                h.u8(4);
                a.stable_hash(h)
            }
            Kind::Corpse(c) => {
                h.u8(5);
                c.stable_hash(h)
            }
            Kind::Hat(t) => {
                h.u8(6);
                t.stable_hash(h)
            }
            Kind::StaticSolid => h.u8(7),
            Kind::Miasma(m) => {
                h.u8(8);
                m.stable_hash(h)
            }
            Kind::Enemy(e) => {
                h.u8(9);
                e.stable_hash(h)
            }
            Kind::Portal(p) => {
                h.u8(10);
                p.stable_hash(h)
            }
            Kind::EnemyAttack(a) => {
                h.u8(11);
                a.stable_hash(h)
            }
            Kind::Hazard(z) => {
                h.u8(12);
                z.stable_hash(h)
            }
            Kind::Reviver(r) => {
                h.u8(13);
                r.stable_hash(h)
            }
            Kind::Dummy(d) => {
                h.u8(14);
                d.stable_hash(h)
            }
            Kind::Tower(t) => {
                h.u8(9);
                t.stable_hash(h)
            }
            Kind::Dw(p) => {
                h.u8(40);
                p.stable_hash(h)
            }
            Kind::FloorMiasma(f) => {
                h.u8(41);
                f.stable_hash(h)
            }
            Kind::Shock(e) => {
                h.u8(21);
                e.stable_hash(h)
            }
            Kind::Bramble(e) => {
                h.u8(22);
                e.stable_hash(h)
            }
            Kind::Prism(e) => {
                h.u8(23);
                e.stable_hash(h)
            }
            Kind::PrismBar(e) => {
                h.u8(24);
                e.stable_hash(h)
            }
            Kind::Pickup(e) => {
                h.u8(25);
                e.stable_hash(h)
            }
            Kind::Chest(e) => {
                h.u8(26);
                e.stable_hash(h)
            }
        }
    }
}
