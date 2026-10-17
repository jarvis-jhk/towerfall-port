//! The world: everything that changes during a match, cheap to clone.
//!
//! Monocle list semantics (engine.md §2): entities are added/removed through
//! queues processed at the start of the next tick; each layer's list is
//! sorted by depth (larger first) then insertion sequence; tag lists are in
//! "order of becoming tagged". Entity data is split into a [`Core`]
//! (position, collider, flags: visible to every query) and a [`Kind`]
//! (type state), which an entity's update takes out of its slot while it
//! runs so it can borrow the world mutably.

use crate::checksum::{Hasher, StableHash};
use crate::collide::{self, Collider, Hitbox, Placed, WorldSize};
use crate::content::{Content, LevelDef};
use crate::entity::{Allegiance, Core, EntityId, Kind, LAYERS, TAG_COUNT, Tag, depth};
use crate::events::{CharSound, Event};
use crate::input::{InputState, PlayerInput};
use crate::math::Vec2;
use crate::rng::Rng;
use crate::time::{OrbLogic, Time};
use crate::{arrow, corpse, hat, impl_hash, platform, player};

pub const MAX_PLAYERS: usize = 12;

/// Match variants that touch M3 gameplay (player.md §17, arrows.md §12).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Variants {
    pub free_aiming: bool,
    pub double_jumping: bool,
    pub no_dodging: bool,
    pub super_dodging: bool,
    pub cursed_bows: bool,
    pub cursed_dodges: bool,
    pub slippery_floors: bool,
    pub no_slipping: bool,
    pub encumbrance: bool,
    pub regenerating_arrows: bool,
    pub regenerating_shields: bool,
    pub anti_gravity_arrows: bool,
    pub no_seeking_arrows: bool,
    pub super_seeking_arrows: bool,
    pub no_friendly_fire: bool,
    pub slow_time: bool,
    pub clumsy_archers: bool,
}
impl Variants {
    /// Field-wise OR.
    pub fn or(&self, o: &Variants) -> Variants {
        Variants {
            free_aiming: self.free_aiming || o.free_aiming,
            double_jumping: self.double_jumping || o.double_jumping,
            no_dodging: self.no_dodging || o.no_dodging,
            super_dodging: self.super_dodging || o.super_dodging,
            cursed_bows: self.cursed_bows || o.cursed_bows,
            cursed_dodges: self.cursed_dodges || o.cursed_dodges,
            slippery_floors: self.slippery_floors || o.slippery_floors,
            no_slipping: self.no_slipping || o.no_slipping,
            encumbrance: self.encumbrance || o.encumbrance,
            regenerating_arrows: self.regenerating_arrows || o.regenerating_arrows,
            regenerating_shields: self.regenerating_shields || o.regenerating_shields,
            anti_gravity_arrows: self.anti_gravity_arrows || o.anti_gravity_arrows,
            no_seeking_arrows: self.no_seeking_arrows || o.no_seeking_arrows,
            super_seeking_arrows: self.super_seeking_arrows || o.super_seeking_arrows,
            no_friendly_fire: self.no_friendly_fire || o.no_friendly_fire,
            slow_time: self.slow_time || o.slow_time,
            clumsy_archers: self.clumsy_archers || o.clumsy_archers,
        }
    }
}

impl_hash!(Variants {
    free_aiming,
    double_jumping,
    no_dodging,
    super_dodging,
    cursed_bows,
    cursed_dodges,
    slippery_floors,
    no_slipping,
    encumbrance,
    regenerating_arrows,
    regenerating_shields,
    anti_gravity_arrows,
    no_seeking_arrows,
    super_seeking_arrows,
    no_friendly_fire,
    slow_time,
    clumsy_archers
});

/// One participating archer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerSetup {
    /// Index into [`Content::archers`].
    pub archer: u16,
    pub allegiance: Allegiance,
    pub team_color: Allegiance,
    pub hat: player::HatState,
    pub arrows: u8,
    /// Start with a shield / wings / boots / invisibility (debug flags
    /// until pickups exist; granted on the original's schedule).
    pub shield: bool,
    pub wings: bool,
    pub speed_boots: bool,
    pub invisible: bool,
    /// Explicit spawn position (`Position`); `None` = a level spawn point.
    pub spawn: Option<Vec2>,
}

impl Default for PlayerSetup {
    fn default() -> Self {
        PlayerSetup {
            archer: 0,
            allegiance: Allegiance::Neutral,
            team_color: Allegiance::Neutral,
            hat: player::HatState::Normal,
            arrows: 3,
            shield: false,
            wings: false,
            speed_boots: false,
            invisible: false,
            spawn: None,
        }
    }
}

/// Shared by all peers; everything the sim needs to start a match.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatchSetup {
    pub players: [Option<PlayerSetup>; MAX_PLAYERS],
    /// Index into [`Content::levels`].
    pub level: usize,
    pub seed: i32,
    pub variants: Variants,
    /// Spawn archers frozen + flashing (Versus round intro) instead of live.
    pub start_frozen: bool,
    /// Sandbox only (off by default; Versus matches never use it): respawn
    /// a dead archer after this many frames at a random spawn point,
    /// flashing (invulnerable) for 60 frames.
    pub respawn_frames: Option<u16>,
    /// The typed variant switches (versus.md §13); per-player reads go
    /// through [`World::variants_for`].
    pub variant_set: crate::variants::VariantSet,
    /// Tower treasure data and unlocks (pickups.md §7.2).
    pub items: crate::pickups::ItemSetup,
}

impl Default for MatchSetup {
    fn default() -> Self {
        MatchSetup {
            players: [None; MAX_PLAYERS],
            level: 0,
            seed: 0,
            variants: Variants::default(),
            start_frozen: false,
            respawn_frames: None,
            variant_set: crate::variants::VariantSet::default(),
            items: crate::pickups::ItemSetup::default(),
        }
    }
}

impl MatchSetup {
    /// `n` archers with default settings (archer i, 3 arrows).
    pub fn sandbox(n: usize) -> MatchSetup {
        let mut s = MatchSetup::default();
        for i in 0..n.min(MAX_PLAYERS) {
            s.players[i] = Some(PlayerSetup {
                archer: i as u16,
                ..Default::default()
            });
        }
        s
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InputSlot {
    pub prev_buttons: u8,
    pub state: InputState,
}
impl_hash!(InputSlot {
    prev_buttons,
    state
});

#[derive(Clone, Copy, Debug, PartialEq)]
struct Slot {
    generation: u32,
    alive: bool,
    core: Core,
    kind: Option<Kind>,
}

#[derive(Clone, Debug, PartialEq)]
struct Layer {
    index: i8,
    active: bool,
    list: Vec<EntityId>,
    to_add: Vec<EntityId>,
    to_remove: Vec<EntityId>,
    resort: bool,
    /// Per-depth insertion counters (never reset).
    depth_seq: Vec<(i32, u32)>,
}

impl Layer {
    fn next_seq(&mut self, d: i32) -> u32 {
        if let Some(e) = self.depth_seq.iter_mut().find(|e| e.0 == d) {
            e.1 += 1;
            e.1
        } else {
            self.depth_seq.push((d, 0));
            0
        }
    }
}

/// The simulation state. See the module docs.
#[derive(Clone, Debug)]
pub struct World {
    pub frame: u32,
    pub time: Time,
    pub orb: OrbLogic,
    /// Gameplay randomness.
    pub rng: Rng,
    /// Randomness that only affects presentation-facing state.
    pub cosmetic_rng: Rng,
    pub level: usize,
    pub frozen: bool,
    pub shake: f32,
    /// `Player.collideCounter` (shared by all archers).
    pub collide_counter: f32,
    pub setup: MatchSetup,
    pub inputs: [InputSlot; MAX_PLAYERS],
    /// Sandbox respawn countdowns per player slot.
    pub respawn: [f32; MAX_PLAYERS],
    slots: Vec<Slot>,
    free: Vec<u32>,
    layers: Vec<Layer>,
    tags: Vec<Vec<EntityId>>,
    tag_ops: Vec<(EntityId, Tag, bool)>,
    pub(crate) events: Vec<Event>,
    /// `Solid.Mover` / `JumpThru.Pusher`: the platform currently moving
    /// (only set inside a platform move; never across ticks).
    pub(crate) mover: Option<EntityId>,
    /// Per slot: false once the slot's machine left (docs/net.md leave
    /// rule); its input is zero from then on.
    pub connected: [bool; MAX_PLAYERS],
    /// The Versus match flow (`None` for sandboxes / tests of one level).
    pub versus: Option<Box<crate::versus::Match>>,
    /// Treasure rates, orbs, bramble growth (arrows.md / pickups.md).
    pub items: crate::pickups::ItemsState,
    /// Round-wide tower-entity state (`entities`).
    pub tower: crate::entities::TowerState,
    /// Quest mode state.
    pub quest: Option<Box<crate::quest::Quest>>,
    /// Trials state.
    pub trials: Option<Box<crate::trials::Trials>>,
    /// Gunn Style, Dark Portals (`variant_fx`).
    pub vfx: crate::variant_fx::VariantFx,
    /// Dark World session state.
    pub dark_world: Option<Box<crate::darkworld::DarkWorld>>,
}

impl World {
    /// Load the level of `setup` and spawn the archers (they enter the scene
    /// on the first [`World::step`], like the original's round start).
    pub fn new(c: &Content, setup: &MatchSetup) -> World {
        let mut w = World {
            frame: 0,
            time: Time::default(),
            orb: OrbLogic::default(),
            rng: Rng::new(setup.seed),
            cosmetic_rng: Rng::new(setup.seed ^ 0x5eed),
            level: setup.level,
            frozen: false,
            shake: 0.0,
            collide_counter: 0.0,
            setup: *setup,
            inputs: [InputSlot::default(); MAX_PLAYERS],
            respawn: [0.0; MAX_PLAYERS],
            slots: Vec::with_capacity(64),
            free: vec![],
            layers: LAYERS
                .iter()
                .map(|&index| Layer {
                    index,
                    active: true,
                    list: vec![],
                    to_add: vec![],
                    to_remove: vec![],
                    resort: false,
                    depth_seq: vec![],
                })
                .collect(),
            tags: vec![Vec::new(); TAG_COUNT],
            tag_ops: vec![],
            events: vec![],
            mover: None,
            connected: [true; MAX_PLAYERS],
            versus: None,
            items: crate::pickups::ItemsState::default(),
            tower: Default::default(),
            quest: None,
            trials: None,
            vfx: Default::default(),
            dark_world: None,
        };
        w.build_level(c);
        w.update_entity_lists(c);
        player::spawn_players(&mut w, c);
        w.items.rates = crate::treasure::compute_rates(&mut w);
        w
    }

    /// Replace the scene with a fresh copy of level `level` (a new `Level`
    /// per round): every entity goes, time/slow motion/freeze reset; match
    /// state (`versus`, inputs, connections, RNG) is kept.
    pub fn load_scene(&mut self, c: &Content, level: usize) {
        self.level = level;
        self.setup.level = level;
        self.slots.clear();
        self.free.clear();
        for l in &mut self.layers {
            l.active = true;
            l.list.clear();
            l.to_add.clear();
            l.to_remove.clear();
            l.resort = false;
            l.depth_seq.clear();
        }
        for t in &mut self.tags {
            t.clear();
        }
        self.tag_ops.clear();
        self.frozen = false;
        self.shake = 0.0;
        self.collide_counter = 0.0;
        self.orb = OrbLogic::default();
        self.items.new_round();
        self.vfx = Default::default();
        let lm = self.time.time_mult;
        self.time = Time {
            last_time_mult: lm,
            time_mult: lm,
            ..Time::default()
        };
        self.build_level(c);
        self.update_entity_lists(c);
    }

    /// The docs/net.md leave rule: slot `slot`'s machine is gone; from now
    /// on it gets no input (idempotent; part of the cloned state).
    pub fn mark_disconnected(&mut self, slot: usize) {
        if slot < MAX_PLAYERS {
            self.connected[slot] = false;
        }
    }

    /// The variant flags that apply to player slot `p` (legacy struct): the
    /// typed set's per-player view OR the legacy global flags.
    pub fn variants_for(&self, p: i32) -> Variants {
        self.setup.variants.or(&self.setup.variant_set.legacy(p))
    }

    fn build_level(&mut self, c: &Content) {
        let w = self;
        let setup_level = w.level;
        // LevelTiles, then the level's platforms (file order).
        let lvl = c.level(setup_level);
        let grid = Collider::Grid {
            w: lvl.cells_x as f32 * 10.0,
            h: lvl.cells_y as f32 * 10.0,
        };
        let mut tiles = Core::new(Vec2::ZERO, grid, depth::SOLID, &[Tag::Solid]);
        tiles.active = false;
        w.add(tiles, Kind::Tiles);
        for s in &c.level(setup_level).static_solids {
            platform::add_static_solid(w, s.pos, s.w, s.h, s.jump_pad);
        }
        for jt in &c.level(setup_level).jump_thrus {
            platform::add_jump_thru(w, jt.pos, jt.width);
        }
        crate::entities::spawn_level(w, c);
    }

    // ------------------------------------------------------------ arena

    /// `Scene.Add`: the entity joins the scene at the next list update.
    pub fn add(&mut self, core: Core, kind: Kind) -> EntityId {
        let slot = Slot {
            generation: 0,
            alive: true,
            core,
            kind: Some(kind),
        };
        let id = if let Some(i) = self.free.pop() {
            let generation = self.slots[i as usize].generation + 1;
            self.slots[i as usize] = Slot { generation, ..slot };
            EntityId {
                index: i,
                generation,
            }
        } else {
            self.slots.push(slot);
            EntityId {
                index: self.slots.len() as u32 - 1,
                generation: 0,
            }
        };
        let li = self.layer_pos(core.layer);
        self.layers[li].to_add.push(id);
        id
    }

    /// `RemoveSelf` / `Scene.Remove`: takes effect at the next list update.
    pub fn remove(&mut self, id: EntityId) {
        let Some(s) = self.slot_mut(id) else { return };
        if s.core.marked_for_removal {
            return;
        }
        s.core.marked_for_removal = true;
        let layer = s.core.layer;
        let li = self.layer_pos(layer);
        self.layers[li].to_remove.push(id);
    }

    /// Deferred `Tag` / `Untag` on an entity in the scene.
    pub fn tag(&mut self, id: EntityId, t: Tag, on: bool) {
        let Some(s) = self.slot_mut(id) else { return };
        if !s.core.in_scene {
            if on {
                s.core.tags |= t.bit();
            } else {
                s.core.tags &= !t.bit();
            }
            return;
        }
        self.tag_ops.push((id, t, on));
    }

    fn layer_pos(&self, index: i8) -> usize {
        self.layers
            .iter()
            .position(|l| l.index == index)
            .unwrap_or(1)
    }

    #[inline]
    fn slot(&self, id: EntityId) -> Option<&Slot> {
        self.slots
            .get(id.index as usize)
            .filter(|s| s.alive && s.generation == id.generation)
    }
    #[inline]
    fn slot_mut(&mut self, id: EntityId) -> Option<&mut Slot> {
        self.slots
            .get_mut(id.index as usize)
            .filter(|s| s.alive && s.generation == id.generation)
    }

    /// Whether `id` still refers to a live entity (added or in the scene).
    pub fn exists(&self, id: EntityId) -> bool {
        self.slot(id).is_some()
    }

    /// The entity's core. Panics on a stale id; use [`World::try_core`].
    #[inline]
    pub fn core(&self, id: EntityId) -> &Core {
        &self.slot(id).expect("stale entity id").core
    }
    #[inline]
    pub fn core_mut(&mut self, id: EntityId) -> &mut Core {
        &mut self.slot_mut(id).expect("stale entity id").core
    }
    #[inline]
    pub fn try_core(&self, id: EntityId) -> Option<&Core> {
        self.slot(id).map(|s| &s.core)
    }
    /// The entity's type state (`None` while its own update runs).
    #[inline]
    pub fn kind(&self, id: EntityId) -> Option<&Kind> {
        self.slot(id).and_then(|s| s.kind.as_ref())
    }
    #[inline]
    pub fn kind_mut(&mut self, id: EntityId) -> Option<&mut Kind> {
        self.slot_mut(id).and_then(|s| s.kind.as_mut())
    }
    pub(crate) fn take_kind(&mut self, id: EntityId) -> Option<Kind> {
        self.slot_mut(id).and_then(|s| s.kind.take())
    }
    pub(crate) fn put_kind(&mut self, id: EntityId, k: Kind) {
        if let Some(s) = self.slot_mut(id) {
            s.kind = Some(k);
        }
    }

    pub fn player(&self, id: EntityId) -> Option<&player::Player> {
        match self.kind(id) {
            Some(Kind::Player(p)) => Some(p),
            _ => None,
        }
    }
    pub fn player_mut(&mut self, id: EntityId) -> Option<&mut player::Player> {
        match self.kind_mut(id) {
            Some(Kind::Player(p)) => Some(p),
            _ => None,
        }
    }
    pub fn arrow(&self, id: EntityId) -> Option<&arrow::Arrow> {
        match self.kind(id) {
            Some(Kind::Arrow(a)) => Some(a),
            _ => None,
        }
    }
    pub fn arrow_mut(&mut self, id: EntityId) -> Option<&mut arrow::Arrow> {
        match self.kind_mut(id) {
            Some(Kind::Arrow(a)) => Some(a),
            _ => None,
        }
    }

    /// The tag list (in-scene entities, in tagging order).
    #[inline]
    pub fn tagged(&self, t: Tag) -> &[EntityId] {
        &self.tags[t as usize]
    }

    /// Entities of a layer in update order.
    pub fn layer_entities(&self, layer: i8) -> &[EntityId] {
        &self.layers[self.layer_pos(layer)].list
    }

    /// All in-scene entities, layer by layer in update (= draw) order.
    pub fn entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.layers.iter().flat_map(|l| l.list.iter().copied())
    }

    /// Change `Depth` while in a scene: re-enter at the end of the new depth
    /// group, re-sort at the next list update.
    pub fn set_depth(&mut self, id: EntityId, d: i32) {
        let Some(s) = self.slot(id) else { return };
        if s.core.depth == d {
            return;
        }
        let (layer, in_scene) = (s.core.layer, s.core.in_scene);
        let li = self.layer_pos(layer);
        self.core_mut(id).depth = d;
        if in_scene {
            let seq = self.layers[li].next_seq(d);
            self.core_mut(id).seq = seq;
            self.layers[li].resort = true;
        }
    }

    /// `Level.Frozen` setter (engine.md §1.3).
    pub fn set_frozen(&mut self, f: bool) {
        self.frozen = f;
        for l in &mut self.layers {
            if !matches!(l.index, 3 | 4 | 10) {
                l.active = !f;
            }
        }
    }

    /// `UpdateEntityLists` (engine.md §2.3).
    pub fn update_entity_lists(&mut self, c: &Content) {
        for li in 0..self.layers.len() {
            while !self.layers[li].to_remove.is_empty() {
                let queue = std::mem::take(&mut self.layers[li].to_remove);
                for id in queue {
                    self.layers[li].list.retain(|&e| e != id);
                    let Some(s) = self.slot(id) else { continue };
                    let tags = s.core.tags;
                    for t in 0..TAG_COUNT {
                        if tags & (1 << t) != 0
                            && let Some(p) = self.tags[t].iter().position(|&e| e == id)
                        {
                            self.tags[t].remove(p);
                        }
                    }
                    self.on_removed(c, id);
                    let s = &mut self.slots[id.index as usize];
                    s.alive = false;
                    s.kind = None;
                    s.core.in_scene = false;
                    self.free.push(id.index);
                }
            }
            if !self.layers[li].to_add.is_empty() || self.layers[li].resort {
                let queue = std::mem::take(&mut self.layers[li].to_add);
                let mut added = vec![];
                for id in queue {
                    let Some(d) = self.slot(id).map(|s| s.core.depth) else {
                        continue;
                    };
                    let seq = self.layers[li].next_seq(d);
                    let s = self.slot_mut(id).unwrap();
                    s.core.seq = seq;
                    s.core.in_scene = true;
                    let tags = s.core.tags;
                    self.layers[li].list.push(id);
                    for t in 0..TAG_COUNT {
                        if tags & (1 << t) != 0 {
                            self.tags[t].push(id);
                        }
                    }
                    added.push(id);
                }
                let mut list = std::mem::take(&mut self.layers[li].list);
                list.sort_by(|a, b| {
                    let (ca, cb) = (
                        &self.slots[a.index as usize].core,
                        &self.slots[b.index as usize].core,
                    );
                    cb.depth.cmp(&ca.depth).then(ca.seq.cmp(&cb.seq))
                });
                self.layers[li].list = list;
                self.layers[li].resort = false;
                for id in added {
                    self.on_added(c, id);
                }
            }
        }
        if !self.tag_ops.is_empty() {
            let ops = std::mem::take(&mut self.tag_ops);
            for t in 0..TAG_COUNT {
                for &(id, tag, on) in &ops {
                    if tag as usize == t && !on {
                        if let Some(p) = self.tags[t].iter().position(|&e| e == id) {
                            self.tags[t].remove(p);
                        }
                        if let Some(s) = self.slot_mut(id) {
                            s.core.tags &= !tag.bit();
                        }
                    }
                }
                for &(id, tag, on) in &ops {
                    if tag as usize == t && on && self.exists(id) && !self.tags[t].contains(&id) {
                        self.tags[t].push(id);
                        self.core_mut(id).tags |= tag.bit();
                    }
                }
            }
        }
    }

    fn on_added(&mut self, c: &Content, id: EntityId) {
        match self.kind(id) {
            Some(Kind::Player(_)) => player::added(self, c, id),
            Some(Kind::Shock(_)) => crate::explosion::shock_added(self, c, id),
            Some(Kind::Bramble(_)) => crate::arrows::brambles::added(self, c, id),
            _ => {}
        }
    }

    fn on_removed(&mut self, _c: &Content, id: EntityId) {
        crate::coop::on_removed(self, id);
        // Corpses take their buried arrows with them (ArrowCushion removal).
        if let Some(Kind::Corpse(cp)) = self.kind(id).copied() {
            for a in cp.cushion.iter() {
                if self.exists(a) {
                    self.remove(a);
                }
            }
        }
    }

    // ------------------------------------------------------------ tick

    /// One 60 Hz tick. `inputs[slot]` is player slot `slot`'s input (missing
    /// slots get no input).
    pub fn step(&mut self, c: &Content, inputs: &[PlayerInput]) {
        self.events.clear();
        self.time.begin_tick();
        for i in 0..MAX_PLAYERS {
            let inp = if self.connected[i] {
                inputs.get(i).copied().unwrap_or_default()
            } else {
                PlayerInput::default()
            };
            self.inputs[i].state = InputState::derive(inp, self.inputs[i].prev_buttons);
            self.inputs[i].prev_buttons = inp.buttons;
        }
        // Level.Update, unfrozen part (engine.md §1.3).
        if !self.frozen {
            if self.time.on_interval(1) && self.shake > 0.0 {
                // Screen-shake offset draws (cosmetic generator).
                self.cosmetic_rng.next_max(2);
                self.cosmetic_rng.next_max(2);
            }
            if self.shake > 0.0 {
                self.shake -= self.time.time_mult;
            }
            // Session.OnUpdate -> RoundLogic.OnUpdate (before OrbLogic).
            crate::versus::session_update(self, c);
            crate::coop::session_update(self, c);
            let slow = self.setup.variants.slow_time
                || self
                    .setup
                    .variant_set
                    .any(crate::variants::Variant::SlowTime);
            self.orb.update(&mut self.time, slow, &mut self.rng);
            crate::pickups::orbs::update(self, c);
        }
        // Scene.Update.
        self.update_entity_lists(c);
        crate::entities::frame_start(self, c);
        for li in 0..self.layers.len() {
            if !self.layers[li].active {
                continue;
            }
            let n = self.layers[li].list.len();
            for k in 0..n {
                let id = self.layers[li].list[k];
                if self.slot(id).is_some_and(|s| s.core.active) {
                    self.update_entity(c, id);
                }
            }
        }
        crate::variant_fx::update(self, c);
        self.time.end_scene_update();
        if !self.frozen {
            player::resolve_player_collisions(self, c);
            arrow::enforce_limit(self, c);
            if let Some(n) = self.setup.respawn_frames {
                self.sandbox_respawn(c, n as f32);
            }
        }
        // Match flow between ticks (results, next round), frozen or not.
        crate::versus::end_of_tick(self, c);
        crate::coop::end_of_tick(self, c);
        self.frame += 1;
    }

    fn sandbox_respawn(&mut self, c: &Content, delay: f32) {
        for i in 0..MAX_PLAYERS {
            let Some(ps) = self.setup.players[i] else {
                continue;
            };
            let alive = self.players().any(|id| {
                self.player(id)
                    .is_some_and(|p| p.index as usize == i && !p.dead)
            });
            let pending = self.layers.iter().any(|l| {
                l.to_add.iter().any(
                    |&id| matches!(self.kind(id), Some(Kind::Player(p)) if p.index as usize == i),
                )
            });
            if alive || pending {
                self.respawn[i] = delay;
                continue;
            }
            self.respawn[i] -= self.time.time_mult;
            if self.respawn[i] > 0.0 {
                continue;
            }
            let spawns = &c.level(self.level).player_spawns;
            let pos = if spawns.is_empty() {
                ps.spawn
                    .unwrap_or(Vec2::new(c.level(self.level).width as f32 / 2.0, 0.0))
            } else {
                spawns[self.rng.choose_index(spawns.len())] + Vec2::new(0.0, 2.0)
            };
            // Sandbox housekeeping: the previous life's corpse and hat go.
            let old: Vec<EntityId> = self
                .tagged(Tag::Corpse)
                .iter()
                .chain(self.tagged(Tag::Hat))
                .copied()
                .filter(|&e| match self.kind(e) {
                    Some(Kind::Corpse(cp)) => cp.player_index as usize == i,
                    Some(Kind::Hat(h)) => h.owner as usize == i && h.hat != player::HatState::Crown,
                    _ => false,
                })
                .collect();
            for e in old {
                self.remove(e);
            }
            let id = player::create(self, c, i as u8, pos, &ps);
            self.core_mut(id)
                .flash
                .start_for(60.0, crate::entity::FlashFinish::None);
            self.respawn[i] = delay;
        }
    }

    fn update_entity(&mut self, c: &Content, id: EntityId) {
        let Some(kind) = self.take_kind(id) else {
            return;
        };
        let kind = match kind {
            Kind::Player(mut p) => {
                player::update(self, c, id, &mut p);
                Kind::Player(p)
            }
            Kind::Arrow(mut a) => {
                arrow::update(self, c, id, &mut a);
                Kind::Arrow(a)
            }
            Kind::Corpse(mut cp) => {
                corpse::update(self, c, id, &mut cp);
                Kind::Corpse(cp)
            }
            Kind::Hat(mut h) => {
                hat::update(self, c, id, &mut h);
                Kind::Hat(h)
            }
            Kind::MovingSolid(mut m) => {
                platform::update_moving(self, c, id, &mut m);
                Kind::MovingSolid(m)
            }
            Kind::Miasma(mut m) => {
                crate::versus::miasma::update(self, c, id, &mut m);
                Kind::Miasma(m)
            }
            Kind::Shock(mut e) => {
                crate::explosion::shock_update(self, c, id, &mut e);
                Kind::Shock(e)
            }
            Kind::Bramble(mut e) => {
                crate::arrows::brambles::update(self, c, id, &mut e);
                Kind::Bramble(e)
            }
            Kind::Prism(mut e) => {
                crate::arrows::prism::update(self, c, id, &mut e);
                Kind::Prism(e)
            }
            Kind::Pickup(mut e) => {
                crate::pickups::update(self, c, id, &mut e);
                Kind::Pickup(e)
            }
            Kind::Chest(mut e) => {
                crate::treasure::update(self, c, id, &mut e);
                Kind::Chest(e)
            }
            Kind::Tower(mut t) => {
                crate::entities::update(self, c, id, &mut t);
                Kind::Tower(t)
            }
            Kind::Enemy(mut e) => {
                crate::enemies::update(self, c, id, &mut e);
                Kind::Enemy(e)
            }
            Kind::Portal(mut p) => {
                crate::enemies::portal::update(self, c, id, &mut p);
                Kind::Portal(p)
            }
            Kind::EnemyAttack(mut a) => {
                crate::enemies::attack::update(self, c, id, &mut a);
                Kind::EnemyAttack(a)
            }
            Kind::FloorMiasma(mut f) => {
                crate::quest::floor::update(self, c, id, &mut f);
                Kind::FloorMiasma(f)
            }
            Kind::Dw(mut p) => {
                crate::darkworld::part::update(self, c, id, &mut p);
                Kind::Dw(p)
            }
            Kind::Hazard(mut z) => {
                crate::enemies::hazard::update(self, c, id, &mut z);
                Kind::Hazard(z)
            }
            Kind::Dummy(d) => {
                if !d.dead && crate::entities::scenery::dummy_hazards(self, c, id) {
                    self.put_kind(id, Kind::Dummy(d));
                    crate::trials::dummy_arrow_hit(self, id);
                    return;
                }
                Kind::Dummy(d)
            }
            k @ Kind::Reviver(_) => k,
            k @ (Kind::Tiles | Kind::JumpThru | Kind::StaticSolid | Kind::PrismBar(_)) => k,
        };
        self.put_kind(id, kind);
    }

    /// Events produced by the last step.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub(crate) fn emit(&mut self, e: Event) {
        self.events.push(e);
    }
    /// Entry `index` of an array effect.
    pub(crate) fn sound_indexed(&mut self, name: &'static str, index: u8, x: f32) {
        self.events.push(Event::SoundIndexed { name, index, x });
    }

    pub(crate) fn sound(&mut self, name: &'static str, x: f32) {
        self.events.push(Event::Sound { name, x });
    }
    pub(crate) fn char_sound(&mut self, sound: CharSound, player: u8, archer: u16, x: f32) {
        self.events.push(Event::CharSound {
            sound,
            player,
            archer,
            x,
        });
    }

    /// `Level.ScreenShake(frames)`.
    pub fn screen_shake(&mut self, frames: f32) {
        self.shake = crate::math::max(self.shake, frames);
        self.events.push(Event::ScreenShake {
            frames: frames as u16,
        });
    }

    // ------------------------------------------------------------ queries

    #[inline]
    pub fn world_size(&self, c: &Content) -> WorldSize {
        let l = c.level(self.level);
        WorldSize {
            w: l.width as f32,
            h: l.height as f32,
        }
    }

    #[inline]
    pub fn level_def<'a>(&self, c: &'a Content) -> &'a LevelDef {
        c.level(self.level)
    }

    /// The entity's collider at its position.
    #[inline]
    pub fn placed(&self, id: EntityId) -> Placed {
        let core = self.core(id);
        Placed {
            pos: core.pos,
            collider: core.collider,
        }
    }

    /// `Collide.Check(probe, target)` without the self test: the target must
    /// be collidable.
    #[inline]
    pub fn check(&self, c: &Content, probe: &Placed, target: EntityId) -> bool {
        let t = self.core(target);
        t.collidable
            && collide::collide(
                probe,
                &Placed {
                    pos: t.pos,
                    collider: t.collider,
                },
                c.level(self.level),
                self.world_size(c),
            )
    }

    /// `CollideFirst(tag)` with `probe` as the receiver, skipping `me`.
    pub fn first(&self, c: &Content, probe: &Placed, tag: Tag, me: EntityId) -> Option<EntityId> {
        let lvl = c.level(self.level);
        let ws = self.world_size(c);
        for &e in &self.tags[tag as usize] {
            if e == me {
                continue;
            }
            let t = &self.slots[e.index as usize].core;
            if t.collidable
                && collide::collide(
                    probe,
                    &Placed {
                        pos: t.pos,
                        collider: t.collider,
                    },
                    lvl,
                    ws,
                )
            {
                return Some(e);
            }
        }
        None
    }

    #[inline]
    pub fn any(&self, c: &Content, probe: &Placed, tag: Tag, me: EntityId) -> bool {
        self.first(c, probe, tag, me).is_some()
    }

    /// `CollideAll(tag)`.
    pub fn all(&self, c: &Content, probe: &Placed, tag: Tag, me: EntityId) -> Vec<EntityId> {
        self.tags[tag as usize]
            .iter()
            .copied()
            .filter(|&e| e != me && self.check(c, probe, e))
            .collect()
    }

    /// `CollideFirstOutside(tag, at)`: overlaps at `at` but not at the
    /// entity's current position.
    pub fn first_outside(&self, c: &Content, me: EntityId, at: Vec2, tag: Tag) -> Option<EntityId> {
        let here = self.placed(me);
        let there = Placed { pos: at, ..here };
        self.tags[tag as usize]
            .iter()
            .copied()
            .find(|&e| e != me && !self.check(c, &here, e) && self.check(c, &there, e))
    }

    /// `CollideCheck(tag, at)` for entity `me` with its own collider.
    #[inline]
    pub fn check_at(&self, c: &Content, me: EntityId, at: Vec2, tag: Tag) -> bool {
        let p = Placed {
            pos: at,
            collider: self.core(me).collider,
        };
        self.any(c, &p, tag, me)
    }

    #[inline]
    pub fn first_at(&self, c: &Content, me: EntityId, at: Vec2, tag: Tag) -> Option<EntityId> {
        let p = Placed {
            pos: at,
            collider: self.core(me).collider,
        };
        self.first(c, &p, tag, me)
    }

    /// `Scene.CollideCheck(point, tag)`.
    pub fn point_check(&self, c: &Content, p: Vec2, tag: Tag) -> bool {
        let lvl = c.level(self.level);
        let ws = self.world_size(c);
        self.tags[tag as usize].iter().any(|&e| {
            let t = &self.slots[e.index as usize].core;
            t.collidable
                && collide::collide_point(
                    &Placed {
                        pos: t.pos,
                        collider: t.collider,
                    },
                    p,
                    lvl,
                    ws,
                )
        })
    }

    /// `Scene.CollideCheck(from, to, tag)` (segment).
    pub fn line_check(&self, c: &Content, a: Vec2, b: Vec2, tag: Tag) -> bool {
        let lvl = c.level(self.level);
        let ws = self.world_size(c);
        self.tags[tag as usize].iter().any(|&e| {
            let t = &self.slots[e.index as usize].core;
            t.collidable
                && collide::collide_segment(
                    &Placed {
                        pos: t.pos,
                        collider: t.collider,
                    },
                    a,
                    b,
                    lvl,
                    ws,
                )
        })
    }

    /// Living (non-dead) archers in the `Player` tag list.
    pub fn players(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.tags[Tag::Player as usize].iter().copied()
    }

    /// The player entity with this index (first in the list), if any.
    pub fn player_by_index(&self, index: u8) -> Option<EntityId> {
        self.players()
            .find(|&id| self.player(id).is_some_and(|p| p.index == index))
    }

    // ------------------------------------------------------------ checksum

    fn hash_into(&self, h: &mut Hasher) {
        h.u32(self.frame);
        self.time.stable_hash(h);
        self.orb.stable_hash(h);
        self.rng.hash_into(h);
        h.u32(self.level as u32);
        h.bool(self.frozen);
        h.f32(self.shake);
        h.f32(self.collide_counter);
        self.inputs.stable_hash(h);
        self.respawn.stable_hash(h);
        self.connected.stable_hash(h);
        self.items.stable_hash(h);
        self.tower.stable_hash(h);
        self.setup.variant_set.stable_hash(h);
        self.vfx.stable_hash(h);
        if let Some(m) = &self.versus {
            m.stable_hash(h);
        }
        if let Some(q) = &self.quest {
            q.stable_hash(h);
        }
        if let Some(t) = &self.trials {
            t.stable_hash(h);
        }
        if let Some(d) = &self.dark_world {
            d.stable_hash(h);
        }
        for l in &self.layers {
            h.bool(l.active);
            l.list.stable_hash(h);
            l.to_add.stable_hash(h);
            l.to_remove.stable_hash(h);
        }
        for t in &self.tags {
            t.stable_hash(h);
        }
        for s in &self.slots {
            if !s.alive {
                continue;
            }
            h.u32(s.generation);
            s.core.stable_hash(h);
            s.kind.stable_hash(h);
        }
    }

    /// The checksum split by part (desync hunting): world fields one by
    /// one, then one entry per live entity (`"entity <index> <kind>"`).
    pub fn checksum_parts(&self) -> Vec<(String, u64)> {
        let mut out = vec![];
        let mut add = |name: String, f: &dyn Fn(&mut Hasher)| {
            let mut h = Hasher::new();
            f(&mut h);
            out.push((name, h.finish()));
        };
        add("frame".into(), &|h| h.u32(self.frame));
        add("time".into(), &|h| self.time.stable_hash(h));
        add("orb".into(), &|h| self.orb.stable_hash(h));
        add("rng".into(), &|h| self.rng.hash_into(h));
        add("level/frozen/shake/collide".into(), &|h| {
            h.u32(self.level as u32);
            h.bool(self.frozen);
            h.f32(self.shake);
            h.f32(self.collide_counter);
        });
        add("inputs".into(), &|h| self.inputs.stable_hash(h));
        add("respawn".into(), &|h| self.respawn.stable_hash(h));
        add("connected".into(), &|h| self.connected.stable_hash(h));
        add("items".into(), &|h| self.items.stable_hash(h));
        add("tower".into(), &|h| self.tower.stable_hash(h));
        add("variant_set".into(), &|h| {
            self.setup.variant_set.stable_hash(h)
        });
        add("vfx".into(), &|h| self.vfx.stable_hash(h));
        add("modes".into(), &|h| {
            if let Some(m) = &self.versus {
                m.stable_hash(h);
            }
            if let Some(q) = &self.quest {
                q.stable_hash(h);
            }
            if let Some(t) = &self.trials {
                t.stable_hash(h);
            }
            if let Some(d) = &self.dark_world {
                d.stable_hash(h);
            }
        });
        add("layers/tags".into(), &|h| {
            for l in &self.layers {
                h.bool(l.active);
                l.list.stable_hash(h);
                l.to_add.stable_hash(h);
                l.to_remove.stable_hash(h);
            }
            for t in &self.tags {
                t.stable_hash(h);
            }
        });
        for (i, s) in self.slots.iter().enumerate() {
            if !s.alive {
                continue;
            }
            let name = format!("{:?}", s.kind);
            let name = name.trim_start_matches("Some(");
            let name: String = name.chars().take(40).collect();
            add(format!("entity {i} {name} core"), &|h| {
                h.u32(s.generation);
                s.core.stable_hash(h);
            });
            add(format!("entity {i} {name} kind"), &|h| {
                s.kind.stable_hash(h)
            });
        }
        out
    }

    /// Hash of all gameplay state (desync detection).
    pub fn checksum(&self) -> u64 {
        let mut h = Hasher::new();
        self.hash_into(&mut h);
        h.finish()
    }

    /// 128-bit variant (what `tfp_net::NetGame::checksum` wants).
    pub fn checksum128(&self) -> u128 {
        let mut h = Hasher::new();
        self.hash_into(&mut h);
        h.finish128()
    }

    /// Bytes of entity storage a clone copies (slots × slot size).
    pub fn arena_bytes(&self) -> usize {
        self.slots.len() * std::mem::size_of::<Slot>()
    }

    /// Number of live entity slots (added or in the scene).
    pub fn entity_count(&self) -> usize {
        self.slots.iter().filter(|s| s.alive).count()
    }

    /// The hitbox an entity uses for its current collider, if it is a box.
    pub fn hitbox_of(&self, id: EntityId) -> Option<Hitbox> {
        match self.core(id).collider {
            Collider::Box(h) => Some(h),
            _ => None,
        }
    }
}
