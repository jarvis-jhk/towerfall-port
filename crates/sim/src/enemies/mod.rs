//! Enemies (docs/spec/enemies.md): the shared `Enemy` base (§1), the spawn
//! portal (§2), the invisible melee box (§1.7), projectiles and every enemy
//! type of §4, plus the player ghost (player.md §18.1, an `Enemy` subclass).
//!
//! Every enemy is one [`Enemy`] value inside `Kind::Enemy`; per-type
//! behaviour lives in the sibling modules and is dispatched on
//! [`EnemyType`]. Generic per-type counters/flags live in `t`, `n` and
//! `flags` with names defined by each module.

pub mod attack;
pub mod bat;
pub mod birdman;
pub mod blast;
pub mod crystal;
pub mod cultist;
pub mod exploder;
pub mod ghost;
pub mod hazard;
pub mod mole;
pub mod player_ghost;
pub mod portal;
pub mod reaper;
pub mod skeleton;
pub mod skull;
pub mod slime;
pub mod techno;
pub mod worm;

use crate::actor::{self, wrap_diff_x, wrap_dist_sq};
use crate::arrow::{self, Arrow};
use crate::checksum::{Hasher, StableHash};
use crate::collide::{Collider, Hitbox, Placed};
use crate::components::{Coroutine, SineWave};
use crate::content::Content;
use crate::entity::{Allegiance, Core, EntityId, Kind, Tag};
use crate::math::{PI, Vec2, abs_angle_diff, approach_vec, atan2, sign};
use crate::player::{self, DeathCause, Player};
use crate::quiver::ArrowType;
use crate::time::FRAME_SECONDS;
use crate::world::World;

pub const DEPTH: i32 = -90;

/// Skeleton variants (enemies.md §4.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkelKind {
    pub arrow: ArrowType,
    pub shield: bool,
    pub wings: bool,
    pub mimic: bool,
    pub jester: bool,
    pub boss: bool,
}

/// Every spawnable enemy (the spawn-table names of enemies.md §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnemyType {
    Slime,
    RedSlime,
    BlueSlime,
    Bat,
    BombBat,
    SuperBombBat,
    Crow,
    Ghost,
    GreenGhost,
    Elemental,
    GreenElemental,
    Cultist,
    ScytheCultist,
    BossCultist,
    Birdman,
    DarkBirdman,
    Skeleton(SkelKind),
    Mole,
    Worm,
    FlamingSkull,
    Exploder,
    EvilCrystal,
    BlueCrystal,
    BoltCrystal,
    PrismCrystal,
    TechnoMage,
    KingReaper,
    /// `ReturnAsGhosts` / Dark World ghost of a dead player.
    PlayerGhost,
}

impl EnemyType {
    /// Parse a spawn name; `None` for unknown names (reject at load).
    pub fn from_name(name: &str) -> Option<EnemyType> {
        use EnemyType::*;
        let n = name.trim();
        Some(match n {
            "Slime" => Slime,
            "RedSlime" => RedSlime,
            "BlueSlime" => BlueSlime,
            "Bat" => Bat,
            "BombBat" => BombBat,
            "SuperBombBat" => SuperBombBat,
            "Crow" => Crow,
            "Ghost" => Ghost,
            "GreenGhost" => GreenGhost,
            "Elemental" => Elemental,
            "GreenElemental" => GreenElemental,
            "Cultist" => Cultist,
            "ScytheCultist" => ScytheCultist,
            "BossCultist" => BossCultist,
            "Birdman" => Birdman,
            "DarkBirdman" => DarkBirdman,
            "Mole" => Mole,
            "Worm" => Worm,
            "FlamingSkull" => FlamingSkull,
            "Exploder" => Exploder,
            "EvilCrystal" => EvilCrystal,
            "BlueCrystal" => BlueCrystal,
            "BoltCrystal" => BoltCrystal,
            "PrismCrystal" => PrismCrystal,
            "TechnoMage" => TechnoMage,
            _ => return skeleton_from_name(n).map(Skeleton),
        })
    }

    /// Bounty (enemies.md §3; only a "scoring kill" flag).
    pub fn bounty(self) -> u16 {
        use EnemyType::*;
        match self {
            Slime | Bat => 100,
            RedSlime | Crow | Ghost | FlamingSkull | Worm => 200,
            BlueSlime | BombBat | GreenGhost | Cultist | ScytheCultist | Mole | Exploder => 300,
            SuperBombBat | Elemental | GreenElemental => 400,
            Birdman | DarkBirdman | EvilCrystal => 500,
            BlueCrystal | BoltCrystal | PrismCrystal => 600,
            TechnoMage => 800,
            Skeleton(k) => {
                if k.boss {
                    0
                } else {
                    1000
                }
            }
            BossCultist | KingReaper | PlayerGhost => 0,
        }
    }

    /// Position offset of a spawn from its portal.
    pub fn spawn_offset(self) -> Vec2 {
        use EnemyType::*;
        match self {
            Slime | RedSlime | BlueSlime | Worm => Vec2::new(0.0, 5.0),
            Cultist | ScytheCultist | BossCultist => Vec2::new(0.0, 8.0),
            Skeleton(_) => Vec2::new(0.0, 2.0),
            _ => Vec2::ZERO,
        }
    }

    fn colliders(self) -> (Hitbox, Option<Hitbox>) {
        use EnemyType::*;
        let h = Hitbox::wrapping;
        match self {
            Slime | RedSlime | BlueSlime => {
                (h(16.0, 10.0, -8.0, -10.0), Some(h(18.0, 16.0, -9.0, -8.0)))
            }
            Bat | BombBat | SuperBombBat => {
                (h(14.0, 8.0, -7.0, -6.0), Some(h(18.0, 14.0, -9.0, -7.0)))
            }
            Crow => (h(16.0, 8.0, -8.0, -6.0), Some(h(20.0, 14.0, -10.0, -7.0))),
            Ghost | GreenGhost | Elemental | GreenElemental => {
                (h(14.0, 14.0, -7.0, -7.0), Some(h(14.0, 16.0, -7.0, -8.0)))
            }
            Cultist | ScytheCultist | BossCultist => (h(10.0, 16.0, -5.0, -16.0), None),
            Birdman | DarkBirdman => (h(14.0, 16.0, -7.0, -8.0), Some(h(18.0, 18.0, -9.0, -9.0))),
            Skeleton(_) => (h(10.0, 12.0, -5.0, -4.0), Some(h(12.0, 14.0, -6.0, -7.0))),
            Mole => (h(16.0, 16.0, -8.0, -7.0), Some(h(20.0, 20.0, -10.0, -10.0))),
            FlamingSkull => (h(14.0, 8.0, -7.0, -4.0), Some(h(16.0, 16.0, -8.0, -8.0))),
            EvilCrystal | BlueCrystal | BoltCrystal | PrismCrystal => {
                (h(12.0, 16.0, -6.0, -8.0), None)
            }
            Exploder => (h(14.0, 8.0, -7.0, -3.0), Some(h(16.0, 16.0, -8.0, -8.0))),
            Worm => (h(12.0, 10.0, -6.0, -5.0), Some(h(18.0, 16.0, -9.0, -8.0))),
            TechnoMage => (h(12.0, 14.0, -6.0, -7.0), Some(h(12.0, 18.0, -6.0, -7.0))),
            KingReaper => (
                h(20.0, 20.0, -10.0, -10.0),
                Some(h(24.0, 30.0, -12.0, -10.0)),
            ),
            PlayerGhost => (h(8.0, 8.0, -4.0, -4.0), Some(h(12.0, 12.0, -6.0, -6.0))),
        }
    }

    /// `IgnoreJumpThrus` (§1.1): bats, ghosts and the flaming skull only;
    /// the other floaters land on jump-thrus like any actor.
    fn flyer(self) -> bool {
        use EnemyType::*;
        matches!(
            self,
            Bat | BombBat
                | SuperBombBat
                | Crow
                | Ghost
                | GreenGhost
                | Elemental
                | GreenElemental
                | FlamingSkull
        )
    }

    /// Sight/seek offset.
    fn sight(self) -> Vec2 {
        use EnemyType::*;
        match self {
            Slime | RedSlime | BlueSlime => Vec2::new(0.0, -5.0),
            Cultist | ScytheCultist | BossCultist => Vec2::new(0.0, -8.0),
            _ => Vec2::ZERO,
        }
    }
}

fn skeleton_from_name(n: &str) -> Option<SkelKind> {
    let mut k = SkelKind {
        arrow: ArrowType::Normal,
        shield: false,
        wings: false,
        mimic: false,
        jester: false,
        boss: false,
    };
    match n {
        "MimicSkeleton" => {
            k.mimic = true;
            return Some(k);
        }
        "Jester" => {
            k.jester = true;
            k.shield = true;
            return Some(k);
        }
        "BossSkeleton" => {
            k.boss = true;
            return Some(k);
        }
        "BossWingSkeleton" => {
            k.boss = true;
            k.wings = true;
            return Some(k);
        }
        _ => {}
    }
    let (base, shield) = match n.strip_suffix('S') {
        Some(b) if b.ends_with("Skeleton") => (b, true),
        _ => (n, false),
    };
    k.shield = shield;
    k.arrow = match base {
        "Skeleton" => ArrowType::Normal,
        "WingSkeleton" => {
            k.wings = true;
            ArrowType::Normal
        }
        "BombSkeleton" => ArrowType::Bomb,
        "LaserSkeleton" => ArrowType::Laser,
        "DrillSkeleton" => ArrowType::Drill,
        "BoltSkeleton" => ArrowType::Bolt,
        "TriggerSkeleton" => ArrowType::Trigger,
        "PrismSkeleton" => ArrowType::Prism,
        _ => return None,
    };
    Some(k)
}

/// `EnemyArrowList` (max 3; specials go to the front).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnemyArrows {
    list: [ArrowType; 3],
    len: u8,
}

impl EnemyArrows {
    pub fn len(&self) -> usize {
        self.len as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn next(&self) -> Option<ArrowType> {
        (self.len > 0).then_some(self.list[0])
    }
    /// The held arrows in firing order (presentation: the arrow HUD).
    pub fn iter(&self) -> impl Iterator<Item = ArrowType> + '_ {
        self.list[..self.len as usize].iter().copied()
    }
    pub fn add(&mut self, t: ArrowType) {
        if self.len >= 3 {
            return;
        }
        if t == ArrowType::Normal {
            self.list[self.len as usize] = t;
        } else {
            self.list.copy_within(0..self.len as usize, 1);
            self.list[0] = t;
        }
        self.len += 1;
    }
    pub fn take(&mut self) -> Option<ArrowType> {
        if self.len == 0 {
            return None;
        }
        let t = self.list[0];
        self.list.copy_within(1..self.len as usize, 0);
        self.len -= 1;
        Some(t)
    }
}

pub const F_CAN_BOUNCE_ON: u16 = 1;
pub const F_BOUNCES_ON_PLAYER: u16 = 2;
pub const F_FLAMING: u16 = 4;
pub const F_EXPLOSIONS_PUSH: u16 = 8;
pub const F_NAIVE_MOVE: u16 = 16;
/// Per-type flags start here.
pub const F_TYPE: u16 = 1 << 8;

/// One enemy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Enemy {
    pub ty: EnemyType,
    pub state: u8,
    pending: Option<u8>,
    in_update: bool,
    pub co: Coroutine,
    pub health: i8,
    pub bounty: u16,
    pub killer: i8,
    pub facing: i8,
    pub speed: Vec2,
    pub prev_pos: Vec2,
    pub target_speed: Vec2,
    pub target: Option<EntityId>,
    pub target_pos: Vec2,
    pub collide_sound: f32,
    /// Per-type counters (names in each module).
    pub t: [f32; 6],
    /// Per-type integers.
    pub n: [i32; 4],
    pub flags: u16,
    pub sine: SineWave,
    pub sine2: SineWave,
    pub nodes: [Vec2; 8],
    pub node_count: u8,
    pub arrows: EnemyArrows,
    /// Arrows lodged in the body (bats).
    pub lodged: [Option<EntityId>; 3],
    /// Owning player slot (player ghost), else -1.
    pub owner: i8,
    /// No longer counted as an `Enemy` (untagged on death).
    pub untagged: bool,
    pub sight: Vec2,
    /// The prism holding this enemy (it caught a live prism arrow).
    pub prism: Option<EntityId>,
    // presentation
    pub scale: Vec2,
    pub rotation: f32,
    pub alpha: f32,
}

impl StableHash for Enemy {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u8(self.state);
        self.pending.stable_hash(h);
        self.co.stable_hash(h);
        h.i32(self.health as i32);
        h.i32(self.killer as i32);
        h.i32(self.facing as i32);
        h.vec2(self.speed);
        h.vec2(self.target_speed);
        self.target.stable_hash(h);
        h.vec2(self.target_pos);
        self.t.stable_hash(h);
        self.n.stable_hash(h);
        h.u32(self.flags as u32);
        h.f32(self.sine.counter);
        h.f32(self.sine2.counter);
        h.u8(self.arrows.len);
        self.lodged.stable_hash(h);
        h.bool(self.untagged);
        self.prism.stable_hash(h);
    }
}

impl Enemy {
    pub fn alive(&self) -> bool {
        self.health > 0
    }
    pub fn has(&self, f: u16) -> bool {
        self.flags & f != 0
    }
    pub fn set(&mut self, f: u16, on: bool) {
        if on {
            self.flags |= f;
        } else {
            self.flags &= !f;
        }
    }
    pub fn nodes(&self) -> &[Vec2] {
        &self.nodes[..self.node_count as usize]
    }
}

// ------------------------------------------------------------ creation

/// Create an enemy (joins the scene next tick) in state 0 unless the type
/// says otherwise.
pub fn spawn(
    w: &mut World,
    c: &Content,
    ty: EnemyType,
    pos: Vec2,
    facing: i8,
    nodes: &[Vec2],
) -> EntityId {
    let (col, target) = ty.colliders();
    let mut tags = vec![
        Tag::Enemy,
        Tag::PlayerCollider,
        Tag::ExplosionCollider,
        Tag::Target,
        Tag::Actor,
        Tag::LightSource,
    ];
    if ty == EnemyType::PlayerGhost {
        tags.retain(|&t| t != Tag::Enemy);
    }
    let mut core = Core::new(pos, Collider::Box(col), DEPTH, &tags);
    core.target_collider = target;
    core.screen_wrap = true;
    core.seek = true;
    core.seek_priority = 50;
    core.allegiance = Allegiance::Red;
    core.ignore_jump_thrus = ty.flyer();
    core.seek_offset = ty.sight();
    let mut e = Enemy {
        ty,
        state: 0,
        pending: None,
        in_update: false,
        co: Coroutine::start(0),
        health: 1,
        bounty: ty.bounty(),
        killer: -1,
        facing: if facing < 0 { -1 } else { 1 },
        speed: Vec2::ZERO,
        prev_pos: pos,
        target_speed: Vec2::ZERO,
        target: None,
        target_pos: Vec2::ZERO,
        collide_sound: 0.0,
        t: [0.0; 6],
        n: [0; 4],
        flags: F_CAN_BOUNCE_ON,
        sine: SineWave::default(),
        sine2: SineWave::default(),
        nodes: [Vec2::ZERO; 8],
        node_count: nodes.len().min(8) as u8,
        arrows: EnemyArrows::default(),
        lodged: [None; 3],
        owner: -1,
        untagged: false,
        sight: ty.sight(),
        prism: None,
        scale: Vec2::ONE,
        rotation: 0.0,
        alpha: 1.0,
    };
    for (i, n) in nodes.iter().take(8).enumerate() {
        e.nodes[i] = *n;
    }
    init(w, c, &mut core, &mut e);
    w.add(core, Kind::Enemy(e))
}

fn init(w: &mut World, c: &Content, core: &mut Core, e: &mut Enemy) {
    use EnemyType::*;
    match e.ty {
        Slime | RedSlime | BlueSlime => slime::init(e),
        Bat | BombBat | SuperBombBat | Crow => bat::init(e),
        Ghost | GreenGhost | Elemental | GreenElemental => ghost::init(e),
        Cultist | ScytheCultist | BossCultist => cultist::init(e),
        Birdman | DarkBirdman => birdman::init(e),
        Skeleton(_) => skeleton::init(w, c, core, e),
        Mole => mole::init(e),
        Worm => {
            worm::init(w, e);
            core.depth = -121;
        }
        FlamingSkull => skull::init(w, e),
        Exploder => exploder::init(e),
        EvilCrystal | BlueCrystal | BoltCrystal | PrismCrystal => crystal::init(e),
        TechnoMage => techno::init(e),
        KingReaper => reaper::init(core, e),
        PlayerGhost => player_ghost::init(e),
    }
}

// ------------------------------------------------------------ state machine

/// Request a state change: deferred while the enemy is inside its own
/// update (applied at step 6), immediate otherwise.
pub fn set_state(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    if e.in_update {
        if s != e.state {
            e.pending = Some(s);
        } else {
            e.pending = None;
        }
        return;
    }
    switch_state(w, c, id, e, s);
}

fn switch_state(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    if s == e.state {
        return;
    }
    let old = e.state;
    leave(w, c, id, e, old);
    e.state = s;
    e.co = Coroutine::start(0);
    enter(w, c, id, e, s);
}

fn enter(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    use EnemyType::*;
    match e.ty {
        Slime | RedSlime | BlueSlime => slime::enter(w, c, id, e, s),
        Bat | BombBat | SuperBombBat | Crow => bat::enter(w, c, id, e, s),
        Ghost | GreenGhost | Elemental | GreenElemental => ghost::enter(w, c, id, e, s),
        Cultist | ScytheCultist | BossCultist => cultist::enter(w, c, id, e, s),
        Birdman | DarkBirdman => birdman::enter(w, c, id, e, s),
        Skeleton(_) => skeleton::enter(w, c, id, e, s),
        Mole => mole::enter(w, c, id, e, s),
        Worm => worm::enter(w, c, id, e, s),
        FlamingSkull => skull::enter(w, c, id, e, s),
        Exploder => exploder::enter(w, c, id, e, s),
        EvilCrystal | BlueCrystal | BoltCrystal | PrismCrystal => crystal::enter(w, c, id, e, s),
        TechnoMage => techno::enter(w, c, id, e, s),
        KingReaper => reaper::enter(w, c, id, e, s),
        PlayerGhost => player_ghost::enter(w, c, id, e, s),
    }
}

fn leave(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    use EnemyType::*;
    match e.ty {
        Bat | BombBat | SuperBombBat | Crow => bat::leave(w, c, id, e, s),
        Skeleton(_) => skeleton::leave(w, c, id, e, s),
        Mole => mole::leave(w, c, id, e, s),
        TechnoMage => techno::leave(w, c, id, e, s),
        Exploder => exploder::leave(w, c, id, e, s),
        _ => {}
    }
}

// ------------------------------------------------------------ update

/// `Enemy.Update` (enemies.md §1.2).
pub fn update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    let tm = w.time.time_mult;
    e.prev_pos = w.core(id).pos;
    if e.collide_sound > 0.0 {
        e.collide_sound -= tm;
    }
    e.in_update = true;
    // The prism's bars hold only their captive (on for its update).
    let prism = e.prism;
    if let Some(pr) = prism {
        crate::arrows::prism::set_bars(w, pr, true);
    }
    update_inner(w, c, id, e);
    if let Some(pr) = prism {
        crate::arrows::prism::set_bars(w, pr, false);
    }
}

fn update_inner(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    use EnemyType::*;
    let tm = w.time.time_mult;
    if let Skeleton(_) = e.ty {
        skeleton::pre_update(w, c, id, e);
        skeleton::trigger_update(w, c, id, e);
    }
    if matches!(e.ty, Mole) {
        mole::pre_update(w, c, id, e);
    }
    if matches!(e.ty, Cultist | ScytheCultist | BossCultist) {
        cultist::pre_update(w, c, id, e);
    }
    match e.ty {
        EvilCrystal | BlueCrystal | BoltCrystal | PrismCrystal => crystal::pre_update(w, e),
        Worm => worm::pre_update(w, e),
        _ => {}
    }
    // 2. components: the coroutine (added first), sines; then wrap and flash.
    if let Some(step) = e.co.update(tm) {
        co_step(w, c, id, e, step);
    }
    e.sine.update(tm);
    e.sine2.update(tm);
    actor::enforce_screen_wrap(w, c, id);
    w.core_mut(id).flash.update(tm);
    // 3. state update.
    if e.pending.is_none() {
        let next = state_update(w, c, id, e);
        if next != e.state {
            e.pending = Some(next);
        }
    }
    if !w.exists(id) || w.core(id).marked_for_removal {
        e.in_update = false;
        return;
    }
    // 4. move.
    if e.has(F_NAIVE_MOVE) {
        let p = w.core(id).pos;
        w.core_mut(id).pos = p + e.speed * tm;
    } else {
        if let Some(s) = actor::move_h(w, c, id, e.speed.x * tm) {
            collide_h(w, c, id, e, s);
        }
        if let Some(s) = actor::move_v(w, c, id, e.speed.y * tm) {
            collide_v(w, c, id, e, s);
        }
    }
    // 5. arrow-holding enemies collect/catch.
    if let Skeleton(_) = e.ty {
        skeleton::collect_arrows(w, c, id, e);
    }
    // 6. pending state switch.
    e.in_update = false;
    if let Some(s) = e.pending.take() {
        switch_state(w, c, id, e, s);
    }
    post_update(w, c, id, e);
    pose_lodged(w, id, e);
}

fn co_step(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, step: u16) {
    use EnemyType::*;
    match e.ty {
        Slime | RedSlime | BlueSlime => slime::co(w, c, id, e, step),
        Bat | BombBat | SuperBombBat | Crow => bat::co(w, c, id, e, step),
        Ghost | GreenGhost | Elemental | GreenElemental => ghost::co(w, c, id, e, step),
        Cultist | ScytheCultist | BossCultist => cultist::co(w, c, id, e, step),
        Birdman | DarkBirdman => birdman::co(w, c, id, e, step),
        Skeleton(_) => skeleton::co(w, c, id, e, step),
        Mole => mole::co(w, c, id, e, step),
        Worm => worm::co(w, c, id, e, step),
        FlamingSkull => skull::co(w, c, id, e, step),
        Exploder => exploder::co(w, c, id, e, step),
        EvilCrystal | BlueCrystal | BoltCrystal | PrismCrystal => crystal::co(w, c, id, e, step),
        TechnoMage => techno::co(w, c, id, e, step),
        KingReaper => reaper::co(w, c, id, e, step),
        PlayerGhost => player_ghost::co(w, c, id, e, step),
    }
}

fn state_update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    use EnemyType::*;
    match e.ty {
        Slime | RedSlime | BlueSlime => slime::update(w, c, id, e),
        Bat | BombBat | SuperBombBat | Crow => bat::update(w, c, id, e),
        Ghost | GreenGhost | Elemental | GreenElemental => ghost::update(w, c, id, e),
        Cultist | ScytheCultist | BossCultist => cultist::update(w, c, id, e),
        Birdman | DarkBirdman => birdman::update(w, c, id, e),
        Skeleton(_) => skeleton::update(w, c, id, e),
        Mole => mole::update(w, c, id, e),
        Worm => worm::update(w, c, id, e),
        FlamingSkull => skull::update(w, c, id, e),
        Exploder => exploder::update(w, c, id, e),
        EvilCrystal | BlueCrystal | BoltCrystal | PrismCrystal => crystal::update(w, c, id, e),
        TechnoMage => techno::update(w, c, id, e),
        KingReaper => reaper::update(w, c, id, e),
        PlayerGhost => player_ghost::update(w, c, id, e),
    }
}

fn post_update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    use EnemyType::*;
    match e.ty {
        TechnoMage => techno::post_update(w, c, id, e),
        FlamingSkull => skull::post_update(w, e),
        Exploder => exploder::post_update(w, e),
        Slime | RedSlime | BlueSlime => slime::post_update(w, c, id, e),
        Bat | BombBat | SuperBombBat | Crow => bat::post_update(w, c, id, e),
        Ghost | GreenGhost | Elemental | GreenElemental => ghost::post_update(w, c, id, e),
        Cultist | ScytheCultist | BossCultist => cultist::post_update(w, c, id, e),
        PlayerGhost => player_ghost::ghost_collisions(w, c, id, e),
        Skeleton(_) => skeleton::post_update(w, c, id, e),
        _ => {}
    }
}

fn collide_h(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, s: EntityId) {
    use EnemyType::*;
    match e.ty {
        Bat | BombBat | SuperBombBat | Crow => bat::collide_h(w, c, id, e),
        Cultist | ScytheCultist | BossCultist => cultist::collide_h(w, c, id, e),
        Birdman | DarkBirdman => birdman::collide_h(e),
        Skeleton(_) => skeleton::collide_h(w, c, id, e),
        Mole => mole::collide_h(w, c, id, e),
        Worm => worm::collide(e, true),
        FlamingSkull => skull::collide_h(w, id, e),
        PlayerGhost => player_ghost::collide(w, c, id, e, true),
        TechnoMage => techno::collide(w, id, e, true),
        _ => e.speed.x = 0.0,
    }
    let _ = s;
}

fn collide_v(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, s: EntityId) {
    use EnemyType::*;
    match e.ty {
        Slime | RedSlime | BlueSlime => slime::collide_v(w, c, id, e),
        Bat | BombBat | SuperBombBat | Crow => bat::collide_v(w, c, id, e),
        Birdman | DarkBirdman => birdman::collide_v(e),
        Skeleton(_) => skeleton::collide_v(w, c, id, e),
        Mole => mole::collide_v(w, c, id, e),
        Cultist | ScytheCultist | BossCultist => cultist::collide_v(w, c, id, e),
        Worm => worm::collide(e, false),
        FlamingSkull => skull::collide_v(w, id, e),
        PlayerGhost => player_ghost::collide(w, c, id, e, false),
        TechnoMage => techno::collide(w, id, e, false),
        _ => e.speed.y = 0.0,
    }
    let _ = s;
}

// ------------------------------------------------------------ damage

/// `Hurt(force, damage, killer)`.
pub fn hurt(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    force: Vec2,
    damage: i8,
    killer: i8,
) {
    if let EnemyType::KingReaper = e.ty {
        reaper::hurt(w, c, id, e, force, damage, killer);
        return;
    }
    if let EnemyType::Skeleton(k) = e.ty
        && k.shield
        && e.has(skeleton::F_SHIELD)
    {
        skeleton::break_shield(w, id, e, force);
        return;
    }
    // Flashing only stops arrows (§1.5), not a direct hurt.
    e.speed = force;
    if e.alive() {
        e.health -= damage;
        if e.health <= 0 {
            die(w, c, id, e, killer);
        }
    }
}

/// `Die(killer)`.
pub fn die(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, killer: i8) {
    if e.health < 0 {
        return;
    }
    e.health = 0;
    e.killer = killer;
    w.core_mut(id).collidable = false;
    if let Some(pr) = e.prism.take() {
        crate::arrows::prism::captive_die(w, pr, &mut None);
    }
    unlink_triggers(w, id);
    let at = w.core(id).pos + e.sight;
    if e.ty != EnemyType::PlayerGhost {
        crate::coop::register_enemy_kill(w, at, killer, e.bounty);
    }
    e.health = -1;
    use EnemyType::*;
    match e.ty {
        Slime | RedSlime | BlueSlime => slime::die(w, c, id, e),
        Bat | BombBat | SuperBombBat | Crow => bat::die(w, c, id, e),
        Ghost | GreenGhost | Elemental | GreenElemental => ghost::die(w, c, id, e),
        Cultist | ScytheCultist | BossCultist | Birdman | DarkBirdman => remove_now(w, id, e),
        Skeleton(_) => skeleton::die(w, c, id, e),
        Mole => mole::die(w, c, id, e),
        Worm => worm::die(w, c, id, e),
        FlamingSkull => skull::die(w, c, id, e),
        Exploder => exploder::die(w, c, id, e),
        EvilCrystal | BlueCrystal | BoltCrystal | PrismCrystal => crystal::die(w, c, id, e),
        TechnoMage => techno::die(w, c, id, e),
        KingReaper => reaper::die(w, c, id, e),
        PlayerGhost => player_ghost::die(w, c, id, e),
    }
    if let Some(s) = death_sound(e) {
        w.sound(s, at.x);
    }
}

/// The death cue per type (exploders, crystals, techno mages and the King
/// Reaper play theirs in their own death code).
pub fn death_sound(e: &Enemy) -> Option<&'static str> {
    use EnemyType::*;
    Some(match e.ty {
        Slime | RedSlime | BlueSlime => "en_slimeDeath",
        Bat => "en_eyeBatDeath",
        BombBat => "en_bombBatDeath",
        SuperBombBat => "en_superBombBatDeath",
        Crow => "en_crowDeath",
        Ghost => "en_ghostDeath",
        GreenGhost => "en_greenGhostDeath",
        Elemental | GreenElemental => "en_fireElementalDeath",
        Cultist | ScytheCultist | BossCultist => "en_cultDeath",
        Birdman | DarkBirdman => "en_birdmanDeath",
        // Jesters use the Cyan archer's death cue (skeleton::die).
        Skeleton(k) if k.jester => return None,
        Skeleton(k) if k.mimic => "en_shadowDeath",
        Skeleton(_) => "en_skelDeath",
        Mole => "en_moleDeath",
        Worm => "en_wormDeath",
        FlamingSkull => "en_flamingSkullDeath",
        PlayerGhost => "en_varGhostDie",
        Exploder | EvilCrystal | BlueCrystal | BoltCrystal | PrismCrystal | TechnoMage
        | KingReaper => return None,
    })
}

/// A voice-set cue played by an enemy (the original uses archer 0's set).
pub fn enemy_char_sound(w: &mut World, sound: crate::events::CharSound, x: f32) {
    w.emit(crate::Event::CharSound {
        sound,
        player: u8::MAX,
        archer: 0,
        x,
    });
}

/// Stop counting as an `Enemy` (dead bodies that linger).
pub fn untag(w: &mut World, id: EntityId, e: &mut Enemy) {
    if !e.untagged {
        e.untagged = true;
        for t in [
            Tag::Enemy,
            Tag::PlayerCollider,
            Tag::Target,
            Tag::ExplosionCollider,
        ] {
            w.tag(id, t, false);
        }
    }
}

pub fn remove_now(w: &mut World, id: EntityId, e: &mut Enemy) {
    untag(w, id, e);
    release_lodged(w, e);
    w.remove(id);
}

/// `OnArrowHit` (default §1.5, overridden per type). Returns "absorbed".
pub fn on_arrow_hit(
    w: &mut World,
    c: &Content,
    id: EntityId,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    let Some(mut e) = take_enemy(w, id) else {
        return false;
    };
    let r = arrow_hit(w, c, id, &mut e, aid, a);
    w.put_kind(id, Kind::Enemy(e));
    r
}

fn arrow_hit(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    use EnemyType::*;
    match e.ty {
        Bat | BombBat | SuperBombBat | Crow => return bat::arrow_hit(w, c, id, e, aid, a),
        KingReaper => return reaper::arrow_hit(w, c, id, e, aid, a),
        Mole => return mole::arrow_hit(w, c, id, e, aid, a),
        TechnoMage if !e.alive() => {
            arrow::enter_fall_mode(w, aid, a, true, false, true);
            return true;
        }
        // Ghosts set flammable arrows alight, then the base rule runs.
        Ghost | GreenGhost | Elemental | GreenElemental => {
            crate::arrows::ignite(a);
        }
        // The skull: base hit (unless flashing), the arrow catches fire and
        // always falls.
        FlamingSkull => {
            if !w.core(id).flash.flashing {
                hurt(w, c, id, e, a.speed, 1, a.player_index);
            }
            crate::arrows::ignite(a);
            arrow::enter_fall_mode(w, aid, a, true, false, true);
            return true;
        }
        PlayerGhost if !player_ghost::can_be_hit(w, id, e, aid, a) => return false,
        _ => {}
    }
    if w.core(id).flash.flashing {
        // Crystals and exploders drop the arrow even then (§4.9, §4.10).
        if matches!(
            e.ty,
            EvilCrystal | BlueCrystal | BoltCrystal | PrismCrystal | Exploder
        ) {
            arrow::enter_fall_mode(w, aid, a, true, false, e.ty != Exploder);
            return true;
        }
        return false;
    }
    if let Skeleton(_) = e.ty
        && skeleton::try_catch(w, c, id, e, aid, a)
    {
        return true;
    }
    let had_shield = matches!(e.ty, Skeleton(k) if k.shield) && e.has(skeleton::F_SHIELD);
    hurt(w, c, id, e, a.speed, 1, a.player_index);
    let falls = had_shield
        || matches!(
            e.ty,
            EvilCrystal
                | BlueCrystal
                | BoltCrystal
                | PrismCrystal
                | Exploder
                | FlamingSkull
                | TechnoMage
        );
    if falls {
        // Crystals and techno mages drop it with the collide sound,
        // exploders silently (enemies.md §4.9, §4.10, §4.12).
        let sound = matches!(
            e.ty,
            EvilCrystal | BlueCrystal | BoltCrystal | PrismCrystal | TechnoMage
        );
        arrow::enter_fall_mode(w, aid, a, true, false, sound && !had_shield);
    }
    true
}

/// `OnArrowSeeking`: an arrow is homing on this enemy.
pub fn on_arrow_seeking(w: &mut World, c: &Content, id: EntityId, aid: EntityId, a: &Arrow) {
    let Some(mut e) = take_enemy(w, id) else {
        return;
    };
    use EnemyType::*;
    match e.ty {
        Ghost | GreenGhost | Elemental | GreenElemental => {
            ghost::arrow_seeking(w, c, id, &mut e, aid, a)
        }
        Cultist | ScytheCultist | BossCultist => cultist::arrow_seeking(w, c, id, &mut e, aid, a),
        Birdman | DarkBirdman => birdman::arrow_seeking(w, c, id, &mut e, aid, a),
        _ => {}
    }
    w.put_kind(id, Kind::Enemy(e));
}

/// Explosion hooks (enemies.md §1.5): `kill` = within the kill radius.
pub fn on_explode(w: &mut World, c: &Content, id: EntityId, normal: Vec2, owner: i8, kill: bool) {
    let Some(mut e) = take_enemy(w, id) else {
        return;
    };
    if e.ty == EnemyType::KingReaper {
        if !kill {
            e.speed = normal * 4.0;
        }
    } else if e.has(F_EXPLOSIONS_PUSH) {
        if kill {
            hurt(w, c, id, &mut e, normal * 4.0, 1, owner);
        } else {
            e.speed = normal * 4.0;
        }
    } else if !kill {
        e.speed = normal * 4.0;
        hurt(w, c, id, &mut e, normal * 4.0, 1, owner);
    }
    w.put_kind(id, Kind::Enemy(e));
}

/// Squished by a moving solid (§1.3): pop out 6 px (the pusher counts as
/// solid) and slide back flush against it, else die without `Die`.
pub fn on_squish(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    let mover = w.mover;
    let set_mover = |w: &mut World, on: bool| {
        if let Some(m) = mover {
            w.core_mut(m).collidable = on;
        }
    };
    set_mover(w, true);
    let p = w.core(id).pos;
    for i in -1..=1 {
        for j in -1..=1 {
            if i == 0 && j == 0 {
                continue;
            }
            let at = p + Vec2::new(6.0 * i as f32, 6.0 * j as f32);
            if !w.check_at(c, id, at, Tag::Solid) {
                w.core_mut(id).pos = at;
                if i != 0 {
                    actor::move_exact_h(w, c, id, -6 * i, actor::ExactCollide::None);
                }
                if j != 0 {
                    actor::move_exact_v(w, c, id, -6 * j, actor::ExactCollide::None);
                }
                set_mover(w, false);
                return;
            }
        }
    }
    let killer = mover.map(|m| w.core(m).squisher_index).unwrap_or(-1);
    // A player ghost dies (credited to the pusher's owner) instead.
    if e.ty == EnemyType::PlayerGhost {
        if e.alive() {
            die(w, c, id, e, killer);
        }
        set_mover(w, false);
        return;
    }
    let at = p + e.sight;
    if e.alive() && killer >= 0 {
        crate::coop::register_enemy_kill(w, at, killer, e.bounty);
    }
    // Removed without dying: Quest still counts it (killer -1, no bounty).
    if e.health >= 0 {
        crate::quest::register_kill(w, at, -1, 0);
    }
    e.health = -1;
    remove_now(w, id, e);
    set_mover(w, false);
}

// ------------------------------------------------------------ players

/// `OnPlayerCollide` (§1.4), called from the player's update.
pub fn on_player_collide(w: &mut World, c: &Content, id: EntityId, pid: EntityId, p: &mut Player) {
    let Some(mut e) = take_enemy(w, id) else {
        return;
    };
    player_collide(w, c, id, &mut e, pid, p);
    if w.exists(id) {
        w.put_kind(id, Kind::Enemy(e));
    }
}

fn player_collide(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    pid: EntityId,
    p: &mut Player,
) {
    if e.ty == EnemyType::TechnoMage && e.state == techno::DEAD && !p.dead {
        techno::dead_touch(w, c, id, e);
        return;
    }
    if !e.alive() || p.dead {
        return;
    }
    if e.has(F_FLAMING) {
        crate::entities::start_fire(w, p);
    }
    // Ghosts: nothing while spawning; the owner never collides.
    if e.ty == EnemyType::PlayerGhost
        && (e.state == player_ghost::SPAWN || !player_ghost::collides_with(e, p))
    {
        return;
    }
    let ec = *w.core(id);
    let pc = *w.core(pid);
    let hh = w.world_size(c).h;
    let col = ec.hitbox();
    if e.has(F_CAN_BOUNCE_ON) {
        let enemy_top = e.prev_pos.y + col.y;
        let player_bottom = pc.bottom() - p.speed.y;
        if wrap_diff_x(enemy_top, player_bottom, hh) <= 6.0 {
            stomped(w, c, id, e, pid, p);
            return;
        }
    }
    if e.has(F_BOUNCES_ON_PLAYER) {
        let enemy_bottom = e.prev_pos.y + col.y + col.h;
        let player_top = pc.top() - p.speed.y;
        if wrap_diff_x(player_top, enemy_bottom, hh) <= 5.0 {
            e.speed = (ec.pos + e.sight - pc.pos).safe_normalize_len(2.0);
            player::hurt_bounced_on_pub(w, c, pid, p, -1);
            return;
        }
    }
    touch(w, c, id, e, pid, p);
}

/// The stomp (§1.4 step 2): the player bounces, the enemy takes damage.
fn stomped(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, pid: EntityId, p: &mut Player) {
    use EnemyType::*;
    let top = w.core(id).top();
    if let FlamingSkull = e.ty {
        crate::entities::start_fire(w, p);
    }
    if e.ty == KingReaper && reaper::shielded(e) {
        player::hurt(w, c, pid, p, DeathCause::Enemy, w.core(id).pos, -1, None);
        return;
    }
    // A teammate landing on a ghost only squashes it down.
    if e.ty == PlayerGhost && player_ghost::friendly_stomp(w, id, e, p) {
        return;
    }
    player::bounce_pub(w, c, pid, p, top, false);
    w.emit(crate::Event::Rumble {
        player: p.index,
        strength: 0.4,
        frames: 15,
    });
    hurt(w, c, id, e, Vec2::new(0.0, 3.0), 1, p.index as i8);
}

fn touch(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, pid: EntityId, p: &mut Player) {
    use EnemyType::*;
    match e.ty {
        Slime | RedSlime | BlueSlime => slime::touch(w, c, id, e, pid, p),
        Bat | BombBat | SuperBombBat => bat::touch(w, c, id, e, pid, p),
        Ghost | GreenGhost | Elemental | GreenElemental => ghost::touch(w, c, id, e, pid, p),
        Mole => mole::touch(w, c, id, e, pid, p),
        FlamingSkull => skull::touch(w, c, id, e, pid, p),
        Exploder => exploder::touch(w, c, id, e, pid, p),
        KingReaper => reaper::touch(w, c, id, e, pid, p),
        PlayerGhost => player_ghost::touch(w, c, id, e, pid, p),
        _ => plain_touch(w, c, id, e, pid, p),
    }
}

/// `OnPlayerTouch` default: shove apart, reveal, arrow exchange.
pub fn plain_touch(
    w: &mut World,
    _c: &Content,
    id: EntityId,
    e: &mut Enemy,
    pid: EntityId,
    p: &mut Player,
) {
    let from = w.core(pid).pos;
    let me = w.core(id).pos + e.sight;
    e.speed = (me - from).safe_normalize_len(2.0);
    p.invis_opacity = 1.0;
    let v = e.speed;
    if v.x.abs() < 0.5 {
        p.speed.y = -v.y;
    } else if v.y.abs() < 0.5 {
        p.speed.x = -v.x;
    } else {
        p.speed = -v;
    }
    if let EnemyType::Skeleton(_) = e.ty {
        if !p.arrows.is_empty() && e.arrows.is_empty() && !w.core(id).flash.flashing {
            if let Some(t) = p.arrows.use_arrow() {
                e.arrows.add(t);
            }
            if e.collide_sound <= 0.0 {
                let x = w.core(id).pos.x;
                enemy_char_sound(w, crate::events::CharSound::ArrowSteal, x);
            }
        } else if p.arrows.is_empty()
            && let Some(t) = e.arrows.take()
        {
            // `Player.StealArrow(enemy)`.
            p.arrows.add_arrows(&[t]);
            crate::versus::stat(w, p.index, |s| s.arrows_stolen += 1);
            w.char_sound(
                crate::events::CharSound::ArrowSteal,
                p.index,
                p.archer,
                from.x,
            );
        } else if e.collide_sound <= 0.0 {
            w.sound("sfx_collide", from.x);
        }
    } else if e.collide_sound <= 0.0 {
        // The bump of a plain touch (enemies.md 1.4).
        w.sound("sfx_collide", from.x);
    }
    // Re-armed by every touch: one bump per contact (§1.4).
    e.collide_sound = 10.0;
}

/// Hurt the touching player (`DeathCause::Enemy`), pushed away from the enemy.
pub fn hurt_player(w: &mut World, c: &Content, id: EntityId, pid: EntityId, p: &mut Player) {
    let from = w.core(id).pos;
    player::hurt(w, c, pid, p, DeathCause::Enemy, from, -1, None);
}

// ------------------------------------------------------------ perception

/// `Player.Detectable(from)`.
pub fn detectable(w: &World, pid: EntityId, p: &Player, from: Vec2, c: &Content) -> bool {
    let core = w.core(pid);
    if core.flash.flashing || p.dead {
        return false;
    }
    // The 30 px reveal uses the plain, unwrapped distance (§1.8).
    let _ = c;
    !p.invisible || p.has_shield || p.invis_opacity > 0.6 || (from - core.pos).length_sq() <= 900.0
}

/// Living, detectable players (id, position).
pub fn players(w: &World, c: &Content, from: Vec2) -> Vec<(EntityId, Vec2)> {
    w.players()
        .filter_map(|id| {
            let p = w.player(id)?;
            detectable(w, id, p, from, c).then(|| (id, w.core(id).pos))
        })
        .collect()
}

/// Shortest wrapped vector from `a` to `b`.
pub fn shortest(w: &World, c: &Content, a: Vec2, b: Vec2) -> Vec2 {
    let ws = w.world_size(c);
    Vec2::new(wrap_diff_x(a.x, b.x, ws.w), wrap_diff_x(a.y, b.y, ws.h))
}

/// `CanSeePlayer` line test from `from` to `to`.
pub fn can_see(w: &World, c: &Content, from: Vec2, to: Vec2, ignore_jt: bool) -> bool {
    if arrow::wrap_line_hit(w, c, from, to, Tag::Solid) {
        return false;
    }
    let below = shortest(w, c, from, to).y > 0.0;
    ignore_jt || !below || !arrow::wrap_line_hit(w, c, from, to, Tag::JumpThru)
}

/// `ClosestPlayer[WithSight](maxDistSq)` from the enemy's sight point.
pub fn closest_player(
    w: &World,
    c: &Content,
    id: EntityId,
    e: &Enemy,
    max_d2: f32,
    sight: bool,
) -> Option<(EntityId, Vec2, f32)> {
    let from = w.core(id).pos + e.sight;
    let ws = w.world_size(c);
    let ignore_jt = w.core(id).ignore_jump_thrus;
    let mut best: Option<(EntityId, Vec2, f32)> = None;
    for (pid, pos) in players(w, c, from) {
        let d = wrap_dist_sq(from, pos, ws.w, ws.h);
        if d >= max_d2 {
            continue;
        }
        if sight && !can_see(w, c, from, pos, ignore_jt) {
            continue;
        }
        if best.is_none_or(|b| d < b.2) {
            best = Some((pid, pos, d));
        }
    }
    best
}

/// Can the enemy still see `target` (alive, detectable, line of sight)?
pub fn sees(w: &World, c: &Content, id: EntityId, e: &Enemy, target: EntityId) -> bool {
    let Some(p) = w.player(target) else {
        return false;
    };
    let from = w.core(id).pos + e.sight;
    let to = w.core(target).pos;
    detectable(w, target, p, from, c) && can_see(w, c, from, to, w.core(id).ignore_jump_thrus)
}

/// `HorizontalPlayer(from, facing)`.
pub fn horizontal_player(w: &World, c: &Content, from: Vec2, facing: i8) -> Option<EntityId> {
    let ws = w.world_size(c);
    let mut p = from;
    for _ in 0..64 {
        p.x += 5.0 * facing as f32;
        let wp = wrap(p, ws.w, ws.h);
        // Players first, then the wall test (§1.8).
        for dy in [0.0, 10.0, -10.0, -20.0] {
            let q = wrap(Vec2::new(wp.x, wp.y + dy), ws.w, ws.h);
            if let Some(id) = w.players().find(|&id| {
                w.player(id).is_some_and(|pl| !pl.dead) && {
                    let core = w.core(id);
                    crate::collide::collide_point(
                        &Placed {
                            pos: core.pos,
                            collider: core.collider,
                        },
                        q,
                        c.level(w.level),
                        ws,
                    )
                }
            }) {
                return Some(id);
            }
        }
        if w.point_check(c, wp, Tag::Solid) {
            return None;
        }
    }
    None
}

pub fn wrap(p: Vec2, ww: f32, hh: f32) -> Vec2 {
    let x = (if p.x < 0.0 { p.x + ww } else { p.x }) % ww;
    let y = (if p.y < 0.0 { p.y + hh } else { p.y }) % hh;
    Vec2::new(x, y)
}

/// `VisionCone(facing angle, halfAngle, point)`.
pub fn vision_cone(w: &World, c: &Content, from: Vec2, angle: f32, half: f32, point: Vec2) -> bool {
    let v = shortest(w, c, from, point);
    abs_angle_diff(angle, atan2(v.y, v.x)) <= half
}

pub fn facing_angle(f: i8) -> f32 {
    if f < 0 { PI } else { 0.0 }
}

/// `WrapMath.ShortestOpen(from, to)` (engine.md §9.3).
pub fn shortest_open(w: &World, c: &Content, from: Vec2, to: Vec2) -> (bool, Vec2) {
    let ws = w.world_size(c);
    let alt = |a: f32, b: f32, size: f32| {
        let direct = b - a;
        let wrapped = if a < b { -(size - b + a) } else { size - a + b };
        if wrapped.abs() < direct.abs() {
            (wrapped, direct)
        } else {
            (direct, wrapped)
        }
    };
    let (px, ax) = alt(from.x, to.x, ws.w);
    let (py, ay) = alt(from.y, to.y, ws.h);
    let c0 = Vec2::new(px, py);
    let (c1, c2) = if px * px + ay * ay < ax * ax + py * py {
        (Vec2::new(px, ay), Vec2::new(ax, py))
    } else {
        (Vec2::new(ax, py), Vec2::new(px, ay))
    };
    let c3 = Vec2::new(ax, ay);
    for v in [c0, c1, c2, c3] {
        if w.line_check(c, from, from + v, Tag::Solid) {
            continue;
        }
        let mut z = Vec2::ZERO;
        let e = from + v;
        if e.x < 0.0 || e.x >= ws.w {
            z.x = to.x - v.x;
        }
        if e.y < 0.0 || e.y >= ws.h {
            z.y = to.y - v.y;
        }
        if z.is_zero() || !w.line_check(c, to, to + z, Tag::Solid) {
            return (true, v);
        }
    }
    (false, c0)
}

/// Turn to face `x` (sign).
pub fn set_facing(e: &mut Enemy, x: f32) {
    if x != 0.0 {
        e.facing = sign(x) as i8;
    }
}

/// Velocity approach (Monocle vector approach).
pub fn approach_speed(e: &mut Enemy, target: Vec2, step: f32) {
    e.speed = approach_vec(e.speed, target, step);
}

/// Ticks until a sprite animation of `frames` frames at `delay` s finishes
/// (the f32 sprite clock of engine.md §4.10).
pub fn anim_ticks(delay: f32, frames: u32) -> f32 {
    let mut timer = 0.0f32;
    let mut k = 0;
    let mut done = 0;
    while done < frames && k < 100_000 {
        k += 1;
        timer += FRAME_SECONDS;
        while timer >= delay {
            timer -= delay;
            done += 1;
        }
    }
    k as f32
}

/// Is a solid at `pos + d` for this enemy's collider?
pub fn solid_at(w: &World, c: &Content, id: EntityId, d: Vec2) -> bool {
    let p = w.core(id).pos;
    w.check_at(c, id, p + d, Tag::Solid)
}

pub fn on_ground(w: &World, c: &Content, id: EntityId) -> bool {
    actor::check_below(w, c, id)
}

// ------------------------------------------------------------ lodged arrows

/// Lodge an arrow in the body (bats): it follows the enemy until released.
pub fn lodge(w: &mut World, id: EntityId, e: &mut Enemy, aid: EntityId, a: &mut Arrow) {
    if let Some(slot) = e.lodged.iter_mut().find(|s| s.is_none()) {
        *slot = Some(aid);
        arrow::bury(w, aid, a, id);
    }
}

fn pose_lodged(w: &mut World, id: EntityId, e: &mut Enemy) {
    let pos = w.core(id).pos;
    for s in &mut e.lodged {
        if let Some(a) = *s {
            let keep = matches!(w.arrow(a), Some(ar) if ar.buried_in == Some(id))
                && !w.core(a).marked_for_removal;
            if keep {
                w.core_mut(a).pos = pos;
            } else {
                *s = None;
            }
        }
    }
}

pub fn lodged_count(e: &Enemy) -> usize {
    e.lodged.iter().filter(|s| s.is_some()).count()
}

/// Release lodged arrows (they fall).
pub fn release_lodged(w: &mut World, e: &mut Enemy) {
    for s in &mut e.lodged {
        if let Some(a) = s.take()
            && let Some(Kind::Arrow(mut ar)) = w.kind(a).copied()
        {
            ar.buried_in = None;
            ar.state = arrow::ArrowState::Falling;
            ar.speed = Vec2::new(0.0, -1.0);
            if let Some(Kind::Arrow(slot)) = w.kind_mut(a) {
                *slot = ar;
            }
        }
    }
}

/// Number of living enemies (`Enemy` tag, not yet untagged).
pub fn enemy_count(w: &World) -> usize {
    w.tagged(Tag::Enemy)
        .iter()
        .filter(|&&id| match w.kind(id) {
            Some(Kind::Enemy(e)) => !e.untagged,
            Some(_) => true,
            None => true,
        })
        .count()
}

/// Kill every enemy (`Die(-1)`; King Reaper's death).
pub fn kill_all(w: &mut World, c: &Content, except: EntityId) {
    let ids: Vec<EntityId> = w.tagged(Tag::Enemy).to_vec();
    for id in ids {
        if id == except {
            continue;
        }
        if let Some(mut e) = take_enemy(w, id) {
            if e.alive() {
                die(w, c, id, &mut e, -1);
            }
            if w.exists(id) {
                w.put_kind(id, Kind::Enemy(e));
            }
        }
    }
}

/// Fire an enemy-owned arrow.
pub fn shoot_arrow(w: &mut World, id: EntityId, t: ArrowType, pos: Vec2, dir: f32) -> EntityId {
    let shooter = arrow::Shooter {
        id: Some(id),
        player_index: -1,
        allegiance: Allegiance::Red,
        team_color: Allegiance::Red,
        hyper: false,
    };
    let a = arrow::create(w, t, shooter, pos, dir);
    if let Some(ar) = w.arrow_mut(a) {
        ar.cannot_hit_enemies = 10.0;
        // Enemy trigger arrows are detonated by their shooter (§1.9).
        if t == ArrowType::Trigger {
            ar.extra.detonator = Some(id);
            ar.extra.ping = crate::arrows::TRIGGER_PING;
        }
    }
    a
}

/// Trigger arrows detonated by `id` lose their detonator (it died).
fn unlink_triggers(w: &mut World, id: EntityId) {
    let arrows: Vec<EntityId> = w.tagged(Tag::Arrow).to_vec();
    for a in arrows {
        if let Some(ar) = w.arrow_mut(a)
            && ar.extra.detonator == Some(id)
        {
            ar.extra.detonator = None;
        }
    }
}

// ------------------------------------------------------- M5 interactions

/// `ShockCircle` (arrows.md §13.3): every enemy whose box overlaps the
/// circle's box `me` and one of whose check points (position, sight
/// point) is within the radius is hurt with force 5 away from the centre
/// (only for player-owned shocks).
pub fn on_shock(w: &mut World, c: &Content, me: &crate::collide::Placed, r_sq: f32, owner: i8) {
    if owner < 0 {
        return;
    }
    let centre = me.pos;
    let ws = w.world_size(c);
    let list: Vec<EntityId> = w.tagged(Tag::Enemy).to_vec();
    for id in list {
        if !w.exists(id) || !w.check(c, me, id) {
            continue;
        }
        let at = w.core(id).pos;
        let sight = match w.kind(id) {
            Some(Kind::Enemy(e)) => e.sight,
            _ => Vec2::ZERO,
        };
        if [at, at + sight]
            .iter()
            .all(|&q| wrap_dist_sq(centre, q, ws.w, ws.h) > r_sq)
        {
            continue;
        }
        let force = shortest(w, c, centre, at).safe_normalize() * 5.0;
        if let Some(mut e) = take_enemy(w, id) {
            hurt(w, c, id, &mut e, force, 1, owner);
            if w.exists(id) {
                w.put_kind(id, Kind::Enemy(e));
            }
        }
    }
}

/// `OnSqueakyBounce` (toy arrows, arrows.md §9.8): `Speed = 4 * v`.
/// `ExplosionCheckOffsets` (enemies.md §1.5), relative to the position,
/// fixed at construction from the initial main collider (not `_hb`, the
/// current one); empty = the position itself.
pub fn explosion_offsets(e: &Enemy, _hb: crate::collide::Hitbox, out: &mut Vec<Vec2>) {
    use EnemyType::*;
    let hb = e.ty.colliders().0;
    let (cx, cy) = (hb.x + hb.w / 2.0, hb.y + hb.h / 2.0);
    match e.ty {
        Slime | RedSlime | BlueSlime => out.push(Vec2::new(0.0, -5.0)),
        Bat | BombBat | SuperBombBat | Crow => out.push(Vec2::new(cx, cy)),
        Cultist | ScytheCultist | BossCultist => {
            out.extend_from_slice(&[Vec2::new(cx, cy + 4.0), Vec2::new(cx, cy - 4.0)]);
        }
        Birdman | DarkBirdman => out.extend_from_slice(&[
            Vec2::new(cx, hb.y + hb.h),
            Vec2::new(cx, hb.y),
            Vec2::new(hb.x, cy),
            Vec2::new(hb.x + hb.w, cy),
        ]),
        Skeleton(_) | Mole => out.extend_from_slice(&[
            Vec2::new(cx, hb.y),
            Vec2::new(cx, hb.y + hb.h),
            Vec2::new(hb.x, cy),
            Vec2::new(hb.x + hb.w, cy),
        ]),
        _ => {}
    }
}

pub fn on_squeaky_bounce(w: &mut World, id: EntityId, v: Vec2) {
    if let Some(Kind::Enemy(e)) = w.kind_mut(id) {
        e.speed = v * 4.0;
    }
}

/// A falling world orb (pickups.md §5) touching an enemy other than the
/// one that bounced it: hurt it (force 3 down) for the dropper. Returns
/// whether the orb hit something (it then shatters).
pub fn orb_touch(
    w: &mut World,
    c: &Content,
    orb: EntityId,
    cannot_hit: Option<EntityId>,
    dropper: i8,
) -> bool {
    let me = w.placed(orb);
    let Some(id) = w
        .all(c, &me, Tag::Enemy, orb)
        .into_iter()
        .find(|&e| Some(e) != cannot_hit)
    else {
        return false;
    };
    if let Some(mut e) = take_enemy(w, id) {
        hurt(w, c, id, &mut e, Vec2::new(0.0, 3.0), 1, dropper);
        if w.exists(id) {
            w.put_kind(id, Kind::Enemy(e));
        }
    }
    true
}

/// `Enemy.Hurt(force, damage, killer)` on enemy `id` from outside its
/// update (spike balls, icicles). Returns whether it was an enemy.
pub fn hurt_by(
    w: &mut World,
    c: &Content,
    id: EntityId,
    force: Vec2,
    damage: i8,
    killer: i8,
) -> bool {
    let Some(mut e) = take_enemy(w, id) else {
        return false;
    };
    hurt(w, c, id, &mut e, force, damage, killer);
    if w.exists(id) {
        w.put_kind(id, Kind::Enemy(e));
    }
    true
}

/// Placeholder colour per enemy family (until enemy sprites are wired).
pub fn debug_color(e: &Enemy) -> [u8; 4] {
    use EnemyType::*;
    let a = if e.alive() { 255 } else { 110 };
    let c = match e.ty {
        Slime | RedSlime | BlueSlime => [80, 220, 80],
        Bat | BombBat | SuperBombBat | Crow => [150, 80, 200],
        Ghost | GreenGhost | Elemental | GreenElemental => [140, 200, 255],
        Cultist | ScytheCultist | BossCultist => [170, 60, 170],
        Birdman | DarkBirdman => [230, 200, 90],
        Skeleton(_) => [230, 230, 230],
        Mole => [140, 100, 60],
        Worm => [200, 140, 160],
        FlamingSkull => [255, 120, 30],
        Exploder => [255, 60, 60],
        EvilCrystal | BlueCrystal | BoltCrystal | PrismCrystal => [255, 80, 160],
        TechnoMage => [80, 255, 220],
        KingReaper => [255, 30, 30],
        PlayerGhost => [200, 255, 255],
    };
    [c[0], c[1], c[2], a]
}

/// Take an enemy's state out of its slot (only if it is an enemy: other
/// kinds stay where they are).
pub fn take_enemy(w: &mut World, id: EntityId) -> Option<Enemy> {
    if matches!(w.kind(id), Some(Kind::Enemy(_))) {
        match w.take_kind(id) {
            Some(Kind::Enemy(e)) => Some(e),
            _ => None,
        }
    } else {
        None
    }
}

/// The enemy's cooldown display offset while lit (enemies.md §1.10).
pub fn cooldown_star(e: &Enemy) -> Option<Vec2> {
    use EnemyType::*;
    match e.ty {
        Ghost | GreenGhost | Elemental | GreenElemental => ghost::cooldown_star(e),
        Birdman | DarkBirdman => birdman::cooldown_star(e),
        Skeleton(_) => skeleton::cooldown_star(e),
        Mole => mole::cooldown_star(e),
        _ => None,
    }
}

/// A drilling drill arrow meets enemies that are themselves drilling
/// (arrows.md §9.6 step 5): each is hurt by the arrow. Returns whether any
/// was hit.
pub fn drill_hit(w: &mut World, c: &Content, drill: &Placed, aid: EntityId, a: &Arrow) -> bool {
    let mut hit = false;
    for id in w.all(c, drill, Tag::Enemy, aid) {
        let drilling = matches!(w.kind(id), Some(Kind::Enemy(e)) if e.alive() && e.ty == EnemyType::Mole && mole::drilling(e));
        if !drilling {
            continue;
        }
        if let Some(mut e) = take_enemy(w, id) {
            hurt(w, c, id, &mut e, a.speed, 1, a.player_index);
            if w.exists(id) {
                w.put_kind(id, Kind::Enemy(e));
            }
            hit = true;
        }
    }
    hit
}
