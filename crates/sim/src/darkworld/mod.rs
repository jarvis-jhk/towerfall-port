//! Dark World (darkworld.md): the co-op campaign of the expansion. A
//! session is a sequence of levels of one tower at one difficulty; normal
//! levels are paced enemy waves through spawn portals ending in an exit
//! portal, boss levels are a boss fight. Lives (solo), teammate revives,
//! continues, mid-level treasure, curses and power-ups, the records of a
//! clear.
//!
//! State lives in [`World::dark_world`]; its entities (exit portal, boss
//! bodies and parts, projectiles) are [`Kind::Dw`] with a small [`Part`]
//! each, the heavy boss state stays in [`DarkWorld::boss`].

pub mod amaranth;
pub mod boss;
pub mod cataclysm;
pub mod cyclops;
pub mod dreadwood;
pub mod part;
pub mod tower;

pub use part::Part;
pub use tower::{DwLevel, DwSpawn, DwTower, PortalKind};

use crate::checksum::{Hasher, StableHash};
use crate::content::Content;
use crate::enemies::{self, EnemyType, portal};
use crate::entity::{Allegiance, EntityId, Kind, Tag};
use crate::math::Vec2;
use crate::pickups::Pickups;
use crate::player::{self, DeathCause, HatState};
use crate::quiver::{ArrowType, Quiver};
use crate::rng::Rng;
use crate::variants::{Variant, VariantSet};
use crate::world::{MAX_PLAYERS, MatchSetup, PlayerSetup, World};

pub const NORMAL: u8 = 0;
pub const HARDCORE: u8 = 1;
pub const LEGENDARY: u8 = 2;

/// Everything all peers agree on before a session starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DwSettings {
    /// Index into `Content::dark_world`.
    pub tower: usize,
    /// 0 Normal, 1 Hardcore, 2 Legendary.
    pub difficulty: u8,
    /// Archer per joined slot.
    pub slots: [Option<u16>; MAX_PLAYERS],
    /// Curses and power-ups (darkworld.md §5.6), plus anything else.
    pub variants: VariantSet,
    /// Co-op crowns from the previous clear.
    pub crowns: [bool; MAX_PLAYERS],
    /// More than 4 archers (darkworld.md §11).
    pub extended: bool,
    pub seed: i32,
    /// Level 1 shows the title card and waits for Confirm.
    pub start_screen: bool,
    /// Show the trigger-arrow tips slide before the title (`TriggerArrowScreen`:
    /// Darkfang only; the game decides "should show": not shown yet this
    /// run, tips option on, an alt-shoot button exists).
    pub trigger_tips: bool,
}

impl DwSettings {
    pub fn new(tower: usize, difficulty: u8, players: usize) -> DwSettings {
        let mut slots = [None; MAX_PLAYERS];
        for (i, s) in slots.iter_mut().enumerate().take(players) {
            *s = Some(i as u16);
        }
        DwSettings {
            tower,
            difficulty: difficulty.min(2),
            slots,
            variants: VariantSet::default(),
            crowns: [false; MAX_PLAYERS],
            extended: false,
            seed: 0,
            start_screen: false,
            trigger_tips: false,
        }
    }

    pub fn players(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

/// The power-ups and curses of the pre-game screen (§5.6).
pub const POWER_UPS: [Variant; 6] = [
    Variant::StartWithShields,
    Variant::StartWithWings,
    Variant::StartWithSpeedBoots,
    Variant::SuperSeekingArrows,
    Variant::DoubleJumping,
    Variant::SuperDodging,
];
pub const CURSES: [Variant; 7] = [
    Variant::SmallQuivers,
    Variant::CursedBows,
    Variant::NoTreasure,
    Variant::AlwaysDark,
    Variant::Encumbrance,
    Variant::SlipperyFloors,
    Variant::ExplodingCorpses,
];

/// Power-up cap per difficulty (Normal 3, Hardcore 1, Legendary 0).
pub fn power_up_cap(difficulty: u8) -> usize {
    [3, 1, 0][difficulty.min(2) as usize]
}

/// `DarkWorldVariantsSelect` start rules: solo cannot have Exploding
/// Corpses, multiplayer cannot have No Treasure, power-ups over the cap go
/// (in variant order).
pub fn enforce_variants(v: &mut VariantSet, difficulty: u8, players: usize, mask: u16) {
    if players <= 1 {
        v.set_all(Variant::ExplodingCorpses, false, mask);
    } else {
        v.set_all(Variant::NoTreasure, false, mask);
    }
    let mut on = 0;
    for p in POWER_UPS {
        if v.any(p) {
            on += 1;
            if on > power_up_cap(difficulty) {
                v.set_all(p, false, mask);
            }
        }
    }
}

/// Number of curses switched on (records).
pub fn curse_count(v: &VariantSet) -> u32 {
    CURSES.iter().filter(|&&c| v.any(c)).count() as u32
}

/// What an archer carries between levels.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Inventory {
    pub arrows: Quiver,
    pub shield: bool,
    pub wings: bool,
    pub boots: bool,
    pub invisible: bool,
}

impl StableHash for Inventory {
    fn stable_hash(&self, h: &mut Hasher) {
        self.arrows.stable_hash(h);
        h.bool(self.shield);
        h.bool(self.wings);
        h.bool(self.boots);
        h.bool(self.invisible);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DwPhase {
    /// The trigger-arrow tips slide (Darkfang, level 1).
    Tips,
    /// Level 1 title card (waits for Confirm).
    Title,
    /// "LEVEL n" / boss card before the fight.
    Intro,
    Playing,
    /// The exit portal is open.
    Exit,
    /// Zoom / fade into the next level.
    Transition,
    /// `DarkWorldGameOver` (menu: continue / restart / map / quit).
    GameOver,
    /// `DarkWorldComplete` (results, crowns).
    Complete,
}

/// The control program (`DarkWorldControl` coroutines, flattened).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Wait(f32),
    WaitConfirm,
    /// The tips slide may be closed: Confirm, Start or Back (dodge).
    WaitTipsClose,
    /// The tips slide has slid out: on to the title card.
    TipsDone,
    StartRound,
    BuildPortals,
    DarkStart,
    /// Wave `i`: mid-level treasure, then the paced spawn loop.
    Wave(u8),
    /// After a wave's spawn loop: up to `n` more `wait(10)` while not clear.
    Stragglers(u8),
    WaitClear,
    ClearTail,
    WaitBoss,
    Exit,
    WaitPortal,
    Transition,
    NextLevel,
    Complete,
}

impl StableHash for Op {
    fn stable_hash(&self, h: &mut Hasher) {
        let (k, a) = match *self {
            Op::Wait(t) => (0, t),
            Op::WaitConfirm => (1, 0.0),
            Op::StartRound => (2, 0.0),
            Op::BuildPortals => (3, 0.0),
            Op::DarkStart => (4, 0.0),
            Op::Wave(i) => (5, i as f32),
            Op::Stragglers(n) => (6, n as f32),
            Op::WaitClear => (7, 0.0),
            Op::ClearTail => (8, 0.0),
            Op::WaitBoss => (9, 0.0),
            Op::Exit => (10, 0.0),
            Op::WaitPortal => (11, 0.0),
            Op::Transition => (12, 0.0),
            Op::NextLevel => (13, 0.0),
            Op::Complete => (14, 0.0),
            Op::WaitTipsClose => (15, 0.0),
            Op::TipsDone => (16, 0.0),
        };
        h.u8(k);
        h.f32(a);
    }
}

/// The paced spawn loop of one wave (§5.2 step 4).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct WaveState {
    pub index: u8,
    pub difficulty: i32,
    pub time_left: i32,
    pub started: bool,
    /// Mid-level treasure: chests still to drop and the wait between them.
    pub chests_left: u8,
    pub chest_wait: f32,
}

/// A spawn portal of the arena and what it may spawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DwPortal {
    pub id: EntityId,
    pub ground: bool,
    pub nodes: bool,
}

/// Records of a finished session (darkworld.md §9.3; written by the game).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DwResult {
    pub completed: bool,
    pub tower: u8,
    pub difficulty: u8,
    pub players: u8,
    /// Ticks of play (only Legendary clears record a best time).
    pub time: u32,
    pub continues: u32,
    pub deaths: u32,
    pub curses: u32,
    /// Legendary with 0 continues / additionally 0 deaths.
    pub eye: bool,
    pub gold_eye: bool,
    pub kills: [u32; MAX_PLAYERS],
    pub deaths_per: [u32; MAX_PLAYERS],
    pub revives: [u32; MAX_PLAYERS],
    pub boss_hits: [u32; MAX_PLAYERS],
    pub best_combo: [u32; MAX_PLAYERS],
    /// Co-op crowns for the next session (2+ players).
    pub crowns: [bool; MAX_PLAYERS],
}

/// The running session (`DarkWorldSessionState` + `DarkWorldRoundLogic` +
/// `DarkWorldControl`).
#[derive(Clone, Debug, PartialEq)]
pub struct DarkWorld {
    pub settings: DwSettings,
    pub rng: Rng,
    // ---- session
    pub round_index: usize,
    pub extra_lives: i32,
    pub continues: u32,
    pub attempts: u32,
    pub level_tries: u32,
    /// Ticks of rounds in progress.
    pub time: u32,
    pub default_inv: Inventory,
    pub start_inv: [Inventory; MAX_PLAYERS],
    pub last_inv: [Inventory; MAX_PLAYERS],
    pub next_hats: [HatState; MAX_PLAYERS],
    pub kills: [u32; MAX_PLAYERS],
    pub best_combo: [u32; MAX_PLAYERS],
    pub combo: [u32; MAX_PLAYERS],
    pub combo_timer: [f32; MAX_PLAYERS],
    pub revives: [u32; MAX_PLAYERS],
    pub deaths: [u32; MAX_PLAYERS],
    pub boss_hits: [u32; MAX_PLAYERS],
    pub old_boss_hits: [u32; MAX_PLAYERS],
    // ---- level
    pub phase: DwPhase,
    pub phase_t: f32,
    pub prog: Vec<Op>,
    pub pc: usize,
    wait: f32,
    pub started: bool,
    pub ending: bool,
    pub portals: Vec<DwPortal>,
    pub last_portal: Option<EntityId>,
    /// The pacing counter (`enemyDelay`, §5.3).
    pub enemy_delay: f32,
    pub wave: Option<WaveState>,
    pub straggle: u8,
    /// Treasure tokens for this level and player count (curse-filtered).
    pub treasure: Vec<Pickups>,
    pub chest_spots: Vec<Vec2>,
    // boss
    pub boss_mode: bool,
    pub boss_health: i32,
    pub max_boss_health: i32,
    pub boss_finished: bool,
    pub boss_treasure_signal: bool,
    boss_treasure_done: bool,
    pub boss: Option<Box<boss::BossState>>,
    /// Players may not shoot (Cataclysm intro / heart death).
    pub shoot_lock: bool,
    // deaths
    /// All dead with lives left: ticks to the auto revive (60), then the
    /// 360-tick revive timeout.
    pub auto_revive: f32,
    pub revive_timeout: f32,
    // exit
    pub exit_portal: Option<EntityId>,
    pub entered: [bool; MAX_PLAYERS],
    pub portal_timer: f32,
    pub result: Option<DwResult>,
    /// The game-over menu may be used (after its intro).
    pub menu_ready: bool,
    /// Archers that touched the open exit portal this tick.
    pub pending_enter: Vec<EntityId>,
    /// `phase_t` when the trigger tips slide was closed (it slides out for
    /// 20 ticks from then).
    pub tips_closed_at: Option<f32>,
}

impl StableHash for DarkWorld {
    fn stable_hash(&self, h: &mut Hasher) {
        self.rng.hash_into(h);
        h.u32(self.round_index as u32);
        h.i32(self.extra_lives);
        h.u32(self.continues);
        h.u32(self.time);
        self.start_inv.stable_hash(h);
        self.last_inv.stable_hash(h);
        self.kills.stable_hash(h);
        self.combo.stable_hash(h);
        self.combo_timer.stable_hash(h);
        self.deaths.stable_hash(h);
        self.boss_hits.stable_hash(h);
        h.u8(self.phase as u8);
        h.f32(self.phase_t);
        self.prog.stable_hash(h);
        h.u32(self.pc as u32);
        h.f32(self.wait);
        h.bool(self.started);
        h.bool(self.ending);
        h.f32(self.enemy_delay);
        h.bool(self.wave.is_some());
        if let Some(wv) = &self.wave {
            h.i32(wv.time_left);
            h.u8(wv.chests_left);
            h.f32(wv.chest_wait);
        }
        h.u8(self.straggle);
        h.i32(self.boss_health);
        h.bool(self.boss_finished);
        h.bool(self.boss_treasure_signal);
        if let Some(b) = &self.boss {
            b.stable_hash(h);
        }
        h.bool(self.shoot_lock);
        h.f32(self.auto_revive);
        h.f32(self.revive_timeout);
        self.exit_portal.stable_hash(h);
        self.entered.stable_hash(h);
        h.f32(self.portal_timer);
        h.bool(self.result.is_some());
    }
}

/// HUD data (`DarkWorldHUD`, intro cards, results).
#[derive(Clone, Debug, PartialEq)]
pub struct DwView {
    pub phase: DwPhase,
    pub phase_t: f32,
    pub tower: u8,
    pub difficulty: u8,
    pub level: usize,
    pub levels: usize,
    pub boss_level: Option<u8>,
    /// Gems (solo).
    pub extra_lives: i32,
    /// -1 = unlimited.
    pub continues_remaining: i32,
    /// Boss bar (health, max) while in boss mode.
    pub boss_bar: Option<(i32, i32)>,
    /// The exit portal shows "GO!".
    pub go: bool,
    pub time: u32,
    pub kills: [u32; MAX_PLAYERS],
    pub result: Option<DwResult>,
    pub menu_ready: bool,
}

impl DarkWorld {
    pub fn tower<'a>(&self, c: &'a Content) -> &'a DwTower {
        &c.dark_world[self.settings.tower]
    }

    pub fn levels<'a>(&self, c: &'a Content) -> &'a [DwLevel] {
        &self.tower(c).levels[self.settings.difficulty as usize]
    }

    pub fn level<'a>(&self, c: &'a Content) -> &'a DwLevel {
        &self.levels(c)[self.round_index]
    }

    pub fn active_slots(&self) -> Vec<usize> {
        (0..MAX_PLAYERS)
            .filter(|&p| self.settings.slots[p].is_some())
            .collect()
    }

    pub fn n_players(&self) -> usize {
        self.settings.players()
    }

    /// `ContinuesRemaining`: -1 unlimited (Normal), else 8 / 5 minus used.
    pub fn continues_remaining(&self) -> i32 {
        match self.settings.difficulty {
            NORMAL => -1,
            HARDCORE => (8 - self.continues as i32).max(0),
            _ => (5 - self.continues as i32).max(0),
        }
    }

    pub fn can_continue(&self) -> bool {
        self.round_index > 0 && self.continues_remaining() != 0
    }

    fn player_multiplier(&self) -> f32 {
        1.0 + 0.1 * (self.n_players() as f32 - 1.0)
    }

    fn difficulty_delay_multiplier(&self) -> f32 {
        1.0 - 0.4 * self.settings.difficulty as f32
    }

    pub fn view(&self, c: &Content) -> DwView {
        DwView {
            phase: self.phase,
            phase_t: self.phase_t,
            tower: self.tower(c).id,
            difficulty: self.settings.difficulty,
            level: self.round_index,
            levels: self.levels(c).len(),
            boss_level: self.level(c).boss,
            extra_lives: self.extra_lives,
            continues_remaining: self.continues_remaining(),
            boss_bar: self
                .boss_mode
                .then_some((self.boss_health.max(0), self.max_boss_health)),
            go: self.exit_portal.is_some() && self.phase == DwPhase::Exit,
            time: self.time,
            kills: self.kills,
            result: self.result,
            menu_ready: self.menu_ready,
        }
    }
}

// ------------------------------------------------------------- creation

fn default_inventory(v: &VariantSet) -> Inventory {
    Inventory {
        arrows: Quiver::new(3, ArrowType::Normal),
        shield: v.any(Variant::StartWithShields),
        wings: v.any(Variant::StartWithWings),
        boots: v.any(Variant::StartWithSpeedBoots),
        invisible: false,
    }
}

/// Start a Dark World session on level 1 of the settings' tower.
pub fn new_dark_world(c: &Content, settings: DwSettings) -> World {
    let mut settings = settings;
    let mask = (0..MAX_PLAYERS)
        .filter(|&p| settings.slots[p].is_some())
        .fold(0u16, |m, p| m | 1 << p);
    let n = settings.players();
    enforce_variants(&mut settings.variants, settings.difficulty, n, mask);
    let tower = &c.dark_world[settings.tower];
    let first = &tower.levels[settings.difficulty as usize][0];
    let setup = MatchSetup {
        level: first.arena,
        seed: settings.seed,
        start_frozen: true,
        variant_set: settings.variants,
        ..MatchSetup::default()
    };
    let mut w = World::new(c, &setup);
    let inv = default_inventory(&settings.variants);
    let mut next_hats = [HatState::Normal; MAX_PLAYERS];
    for (p, h) in next_hats.iter_mut().enumerate() {
        if n > 1 && settings.crowns[p] {
            *h = HatState::Crown;
        }
    }
    let d = DarkWorld {
        settings,
        rng: Rng::new(settings.seed ^ 0x0d4c),
        round_index: 0,
        extra_lives: if n == 1 { 3 } else { 0 },
        continues: 0,
        attempts: 1,
        level_tries: 0,
        time: 0,
        default_inv: inv,
        start_inv: [inv; MAX_PLAYERS],
        last_inv: [inv; MAX_PLAYERS],
        next_hats,
        kills: [0; MAX_PLAYERS],
        best_combo: [0; MAX_PLAYERS],
        combo: [0; MAX_PLAYERS],
        combo_timer: [0.0; MAX_PLAYERS],
        revives: [0; MAX_PLAYERS],
        deaths: [0; MAX_PLAYERS],
        boss_hits: [0; MAX_PLAYERS],
        old_boss_hits: [0; MAX_PLAYERS],
        phase: DwPhase::Intro,
        phase_t: 0.0,
        prog: vec![],
        pc: 0,
        wait: 0.0,
        started: false,
        ending: false,
        portals: vec![],
        last_portal: None,
        enemy_delay: 0.0,
        wave: None,
        straggle: 0,
        treasure: vec![],
        chest_spots: vec![],
        boss_mode: false,
        boss_health: 0,
        max_boss_health: 0,
        boss_finished: false,
        boss_treasure_signal: false,
        boss_treasure_done: false,
        boss: None,
        shoot_lock: false,
        auto_revive: -1.0,
        revive_timeout: -1.0,
        exit_portal: None,
        entered: [false; MAX_PLAYERS],
        portal_timer: -1.0,
        result: None,
        menu_ready: false,
        pending_enter: vec![],
        tips_closed_at: None,
    };
    w.dark_world = Some(Box::new(d));
    begin_level(&mut w, c, true);
    w
}

/// Spawn markers sorted per §4.1 (fewer than 3 players: by y, ties by
/// distance from x = 161, farther first; 3+: by x, ties by y), extended
/// with derived spawns for more than 4 archers.
/// `players` picks the sort rule; `n` markers are needed (highest slot + 1).
fn sorted_spawns(c: &Content, level: usize, players: usize, n: usize, seed: i32) -> Vec<Vec2> {
    let lvl = c.level(level);
    let mut s = lvl.player_spawns.clone();
    if s.len() < n {
        let mut rng = Rng::new(seed);
        s = crate::versus::spawn::derive_spawns(c, lvl, n, &mut rng);
    }
    if players < 3 {
        s.sort_by(|a, b| {
            a.y.total_cmp(&b.y)
                .then((b.x - 161.0).abs().total_cmp(&(a.x - 161.0).abs()))
        });
    } else {
        s.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    }
    s
}

/// Set up the current level (`DarkWorldRoundLogic` setup + the control's
/// start): players, program, boss.
fn begin_level(w: &mut World, c: &Content, first_time: bool) {
    crate::entities::apply_solo_seek(w);
    let Some(mut d) = w.dark_world.take() else {
        return;
    };
    let lvl = d.level(c).clone();
    let slots = d.active_slots();
    // §4.1 players: marker [slot] (slot index, not compact index) + (0, 2).
    let spawns = sorted_spawns(
        c,
        lvl.arena,
        slots.len(),
        slots.iter().max().map_or(1, |&m| m + 1),
        d.settings.seed,
    );
    for &p in &slots {
        let at = spawns
            .get(p)
            .copied()
            .unwrap_or(Vec2::new(c.level(lvl.arena).width as f32 / 2.0, 0.0));
        let inv = d.start_inv[p];
        let ps = PlayerSetup {
            archer: d.settings.slots[p].unwrap_or(0),
            allegiance: Allegiance::Blue,
            team_color: Allegiance::Neutral,
            hat: d.next_hats[p],
            arrows: 0,
            shield: inv.shield,
            wings: inv.wings,
            speed_boots: inv.boots,
            invisible: inv.invisible,
            ..Default::default()
        };
        let id = player::create(w, c, p as u8, at + Vec2::new(0.0, 2.0), &ps);
        if let Some(pl) = w.player_mut(id) {
            pl.arrows = inv.arrows;
        }
    }
    // Level state.
    d.phase = if first_time && d.settings.start_screen {
        DwPhase::Title
    } else {
        DwPhase::Intro
    };
    d.phase_t = 0.0;
    d.started = false;
    d.ending = false;
    d.portals.clear();
    d.last_portal = None;
    d.enemy_delay = 0.0;
    d.wave = None;
    d.straggle = 0;
    d.boss_mode = false;
    d.boss_health = 0;
    d.max_boss_health = 0;
    d.boss_finished = false;
    d.boss_treasure_signal = false;
    d.boss_treasure_done = false;
    d.boss = None;
    d.shoot_lock = false;
    d.auto_revive = -1.0;
    d.revive_timeout = -1.0;
    d.exit_portal = None;
    d.entered = [false; MAX_PLAYERS];
    d.portal_timer = -1.0;
    d.wait = 0.0;
    d.pc = 0;
    let n = d.n_players();
    let mut items = vec![];
    if !d.settings.variants.any(Variant::NoTreasure) {
        for t in tower::treasure_for(&lvl.treasure, n) {
            if let Some(p) = Pickups::from_name(&t) {
                items.push(p);
            }
        }
    }
    d.treasure = items;
    d.chest_spots = c.level(lvl.arena).treasure.clone();
    // The program (§5.1 - §5.5).
    let mut p = vec![];
    // The tips slide: 10 + 20 ticks in, 60 ticks deaf, then a press closes
    // it (20 ticks out).
    if first_time && d.settings.trigger_tips && d.tower(c).id == 2 {
        d.phase = DwPhase::Tips;
        p.extend([
            Op::Wait(30.0),
            Op::Wait(60.0),
            Op::WaitTipsClose,
            Op::Wait(20.0),
            Op::TipsDone,
        ]);
    }
    if first_time && d.settings.start_screen {
        p.extend([
            Op::Wait(30.0),
            Op::Wait(30.0),
            Op::WaitConfirm,
            Op::Wait(30.0),
            Op::Wait(10.0),
        ]);
    } else {
        p.extend([Op::Wait(30.0), Op::Wait(30.0)]);
    }
    p.push(Op::StartRound);
    if lvl.boss.is_some() {
        // Boss card: emblem in/out around the title.
        p.extend([Op::Wait(40.0), Op::Wait(60.0), Op::Wait(80.0)]);
    } else {
        // "LEVEL n" / "FIGHT!!" card.
        p.extend([Op::Wait(
            31.0 + 21.0 + 31.0 + 11.0 + 21.0 + 61.0 + 21.0 + 21.0,
        )]);
    }
    p.extend([
        Op::BuildPortals,
        Op::Wait(30.0),
        Op::DarkStart,
        Op::Wait(30.0),
    ]);
    if lvl.boss.is_some() {
        p.push(Op::WaitBoss);
    } else {
        for i in 0..lvl.waves.max(1) as u8 {
            p.extend([Op::Wave(i), Op::Stragglers(24)]);
        }
        p.extend([Op::WaitClear, Op::ClearTail, Op::Wait(60.0)]);
    }
    p.push(Op::Exit);
    d.prog = p;
    let boss = lvl.boss;
    let difficulty = d.settings.difficulty;
    w.dark_world = Some(d);
    if let Some(b) = boss {
        boss::spawn(w, c, b, difficulty);
    }
}

// ----------------------------------------------------------------- ticks

/// Before the entities (from `coop::session_update`).
pub fn session_update(w: &mut World, c: &Content) {
    let Some(mut d) = w.dark_world.take() else {
        return;
    };
    let tm = w.time.time_mult;
    d.phase_t += 1.0;
    if d.started && !d.ending {
        d.time += 1;
    }
    if d.shoot_lock {
        for s in w.inputs.iter_mut() {
            s.state.shoot_check = false;
            s.state.shoot_pressed = false;
            s.state.alt_shoot_check = false;
            s.state.alt_shoot_pressed = false;
        }
    }
    // Combos (airborne combos persist, §4.3 / quest.md §5.2).
    for p in d.active_slots() {
        if d.combo[p] > 0 {
            if d.combo_timer[p] > 0.0 {
                d.combo_timer[p] -= tm;
            } else {
                let grounded = w
                    .player_by_index(p as u8)
                    .and_then(|id| w.player(id))
                    .is_none_or(|pl| pl.on_ground);
                if grounded {
                    d.combo[p] = 0;
                }
            }
        }
    }
    // Solo auto revive (§4.2).
    if d.auto_revive > 0.0 {
        d.auto_revive -= tm;
        if d.auto_revive <= 0.0 {
            d.auto_revive = -1.0;
            match crate::revive::revivers(w).first().copied() {
                Some(r) => {
                    d.extra_lives -= 1;
                    if let Some(Kind::Reviver(t)) = w.kind(r) {
                        let pi = t.player_index;
                        crate::revive::set_auto(w, pi);
                    }
                    d.revive_timeout = 360.0;
                }
                None => game_over(w, &mut d, false),
            }
        }
    }
    if d.revive_timeout > 0.0 {
        d.revive_timeout -= tm;
        let alive = w.players().any(|id| w.player(id).is_some_and(|p| !p.dead));
        if alive {
            d.revive_timeout = -1.0;
        } else if d.revive_timeout <= 0.0 {
            // Too slow: the revive is cancelled and the run ends.
            d.revive_timeout = -1.0;
            crate::revive::clear_auto_all(w);
            game_over(w, &mut d, false);
        }
    }
    // Boss bar mirror / midway treasure.
    if d.boss_treasure_signal && !d.boss_treasure_done {
        d.boss_treasure_done = true;
        spawn_boss_treasure(w, &mut d);
    }
    if !matches!(d.phase, DwPhase::GameOver | DwPhase::Complete) {
        run(w, c, &mut d);
    } else if d.phase == DwPhase::GameOver && d.phase_t >= 385.0 {
        d.menu_ready = true;
    }
    w.dark_world = Some(d);
    crate::arrows::brambles::tick_free_growths(w, c);
}

/// After the entities: archers that touched the open exit portal leave.
pub fn end_of_tick(w: &mut World, _c: &Content) {
    let Some(d) = w.dark_world.as_mut() else {
        return;
    };
    let list = std::mem::take(&mut d.pending_enter);
    for pid in list {
        if w.player(pid).is_some_and(|p| !p.dead) {
            player_entered(w, pid);
        }
    }
}

/// The exit portal saw `pid` (from its collision callback).
pub fn queue_enter(w: &mut World, pid: EntityId) {
    if let Some(d) = w.dark_world.as_mut()
        && !d.pending_enter.contains(&pid)
    {
        d.pending_enter.push(pid);
    }
}

fn living(w: &World) -> usize {
    w.players()
        .filter(|&id| w.player(id).is_some_and(|p| !p.dead))
        .count()
}

/// The field is clear: no enemy and no portal with queued spawns.
pub fn field_clear(w: &World) -> bool {
    enemies::enemy_count(w) == 0 && portal::queued_total(w) == 0
}

/// No portal queue and at most `n` enemies (the boss counts).
pub fn field_clear_but(w: &World, n: usize) -> bool {
    enemies::enemy_count(w) <= n && portal::queued_total(w) == 0
}

/// `DarkWorldDifficulty` of the living enemies (0.5 each, 1 per skeleton;
/// boss parts count 0).
fn enemy_weight(w: &World) -> f32 {
    w.tagged(Tag::Enemy)
        .iter()
        .map(|&id| match w.kind(id) {
            Some(Kind::Enemy(e)) if e.alive() && !e.untagged => {
                if matches!(e.ty, EnemyType::Skeleton(_)) {
                    1.0
                } else {
                    0.5
                }
            }
            _ => 0.0,
        })
        .sum()
}

/// Dead-player penalty of the pacing counter (§5.3; OURS beyond 4).
pub fn dead_penalty(n: usize, alive: usize) -> i32 {
    const T: [[i32; 3]; 4] = [[0, 0, 0], [6, 0, 0], [8, 4, 0], [10, 6, 3]];
    if alive >= n || alive == 0 {
        return 0;
    }
    if n <= 4 {
        T[n - 1][(alive - 1).min(2)]
    } else {
        crate::math::round_he(12.0 * (n - alive) as f32 / n as f32) as i32
    }
}

fn pace(w: &World, d: &mut DarkWorld) {
    if field_clear(w) {
        d.enemy_delay = 0.0;
    } else if d.entered.iter().any(|&e| e) {
        d.enemy_delay -= 1.0;
    } else {
        let add = dead_penalty(d.n_players(), living(w)) + enemy_weight(w) as i32;
        d.enemy_delay -= 1.0 / (1.0 + 0.1 * add as f32);
    }
}

/// Weighted enemy choice for difficulty `diff` (§2.2).
fn draw_enemy(d: &mut DarkWorld, set: &[DwSpawn], diff: i32) -> Option<DwSpawn> {
    let weights: Vec<f32> = set
        .iter()
        .map(|e| {
            if diff < e.difficulty {
                0.0
            } else if diff > e.difficulty {
                e.weight * libm::powf(0.9, (diff - e.difficulty) as f32)
            } else {
                e.weight
            }
        })
        .collect();
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let mut u = d.rng.next_float() * total;
    for (e, &wt) in set.iter().zip(&weights) {
        if wt <= 0.0 {
            continue;
        }
        if u < wt {
            return Some(e.clone());
        }
        u -= wt;
    }
    // Float edge: the last entry of the list.
    set.last().cloned()
}

fn pick_portal(d: &mut DarkWorld, kind: PortalKind) -> Option<EntityId> {
    let list: Vec<EntityId> = d
        .portals
        .iter()
        .filter(|p| match kind {
            PortalKind::All => true,
            PortalKind::Ground => p.ground,
            PortalKind::Air => !p.ground,
            PortalKind::Nodes => p.nodes,
        })
        .map(|p| p.id)
        .collect();
    let list = if list.is_empty() {
        d.portals.iter().map(|p| p.id).collect()
    } else {
        list
    };
    if list.is_empty() {
        return None;
    }
    let pool: Vec<EntityId> = if list.len() > 1 {
        list.iter()
            .copied()
            .filter(|&p| Some(p) != d.last_portal)
            .collect()
    } else {
        list
    };
    let p = pool[d.rng.choose_index(pool.len())];
    d.last_portal = Some(p);
    Some(p)
}

fn build_portals(w: &mut World, c: &Content, d: &mut DarkWorld) {
    let ids = portal::add_level_portals(w, c);
    let lvl = c.level(w.level);
    for (k, id) in ids.into_iter().enumerate() {
        let m = &lvl.markers.spawners[k];
        let probe = crate::collide::Placed {
            pos: m.pos + Vec2::new(-2.0, 13.0),
            collider: crate::collide::Collider::Box(crate::collide::Hitbox::new(
                4.0, 4.0, 0.0, 0.0,
            )),
        };
        let ground = w.any(c, &probe, Tag::Solid, id)
            || w.point_check(c, m.pos + Vec2::new(0.0, 16.0), Tag::JumpThru);
        d.portals.push(DwPortal {
            id,
            ground,
            nodes: !m.nodes.is_empty(),
        });
    }
}

fn chest(w: &mut World, at: Vec2, kind: Pickups, timer: f32) {
    use crate::treasure::{self, Appear, ChestSpawn, ChestType};
    treasure::create(
        w,
        &ChestSpawn {
            pos: at,
            ty: ChestType::Normal,
            timer,
            contents: [Some(kind), None, None],
        },
        Appear::Time,
    );
}

/// `BossMidwayTreasureSignal`: chest `k` appears 20 * (k + 1) ticks later.
fn spawn_boss_treasure(w: &mut World, d: &mut DarkWorld) {
    let mut spots = d.chest_spots.clone();
    d.rng.shuffle(&mut spots);
    for (k, (&at, &t)) in spots.iter().zip(d.treasure.iter()).enumerate() {
        chest(w, at, t, 20.0 * (k as f32 + 1.0));
    }
}

fn run(w: &mut World, c: &Content, d: &mut DarkWorld) {
    let tm = w.time.time_mult;
    if d.wait > 0.0 {
        d.wait -= tm;
        return;
    }
    for _ in 0..64 {
        let Some(&op) = d.prog.get(d.pc) else {
            return;
        };
        match op {
            Op::Wait(t) => {
                d.pc += 1;
                d.wait = t;
                return;
            }
            Op::WaitTipsClose => {
                let ok = d.active_slots().iter().any(|&p| {
                    let s = w.inputs[p].state;
                    s.jump_pressed || s.start_pressed || s.dodge_pressed
                });
                if !ok {
                    return;
                }
                w.sound("ui_click", 160.0);
                d.tips_closed_at = Some(d.phase_t);
                d.pc += 1;
            }
            Op::TipsDone => {
                d.pc += 1;
                d.phase = if d.settings.start_screen {
                    DwPhase::Title
                } else {
                    DwPhase::Intro
                };
                d.phase_t = 0.0;
            }
            Op::WaitConfirm => {
                let ok = d
                    .active_slots()
                    .iter()
                    .any(|&p| w.inputs[p].state.jump_pressed || w.inputs[p].state.start_pressed);
                if !ok {
                    return;
                }
                d.pc += 1;
            }
            Op::StartRound => {
                d.pc += 1;
                d.started = true;
                d.phase = DwPhase::Playing;
                d.phase_t = 0.0;
                player::unfreeze_all(w, c);
                crate::entities::round_started(w, c);
            }
            Op::BuildPortals => {
                d.pc += 1;
                build_portals(w, c, d);
            }
            Op::DarkStart => {
                d.pc += 1;
                if d.settings.variants.any(Variant::AlwaysDark) {
                    crate::pickups::orbs::start_dark(w);
                }
            }
            Op::Wave(i) => {
                if !wave(w, c, d, i) {
                    return;
                }
                d.pc += 1;
                d.straggle = 0;
            }
            Op::Stragglers(n) => {
                if d.straggle < n && !field_clear(w) {
                    d.straggle += 1;
                    d.wait = 10.0;
                    return;
                }
                d.pc += 1;
            }
            Op::WaitClear => {
                if !field_clear(w) {
                    return;
                }
                d.pc += 1;
            }
            Op::ClearTail => {
                d.pc += 1;
                w.emit(crate::Event::Particles {
                    kind: crate::events::ParticleKind::LightFade,
                    player: u8::MAX,
                    pos: Vec2::new(160.0, 120.0),
                    count: 1,
                    dir: 0.0,
                });
                w.sound("sfx_darkWorldGo", 160.0);
                disable_dangers(w);
            }
            Op::WaitBoss => {
                if !d.boss_finished {
                    return;
                }
                d.pc += 1;
            }
            Op::Exit => {
                d.pc += 1;
                if d.round_index + 1 >= d.levels(c).len() {
                    if !d.ending {
                        complete(w, d);
                    }
                    return;
                }
                if d.settings.variants.any(Variant::AlwaysDark) {
                    crate::pickups::orbs::end_dark(w);
                }
                let at = c
                    .level(w.level)
                    .markers
                    .endless_portal
                    .unwrap_or(Vec2::new(160.0, 120.0));
                d.exit_portal = Some(part::spawn_exit_portal(w, at));
                d.boss_mode = false;
                d.phase = DwPhase::Exit;
                d.phase_t = 0.0;
                d.prog.push(Op::WaitPortal);
            }
            Op::WaitPortal => {
                let active = d.active_slots();
                let remaining = active
                    .iter()
                    .filter(|&&p| !d.entered[p])
                    .filter(|&&p| {
                        w.player_by_index(p as u8)
                            .and_then(|id| w.player(id))
                            .is_some_and(|pl| !pl.dead)
                    })
                    .count();
                let any = d.entered.iter().any(|&e| e);
                if any && d.portal_timer < 0.0 {
                    d.portal_timer = 360.0;
                }
                if d.portal_timer > 0.0 {
                    d.portal_timer -= tm;
                }
                let go = any && (remaining == 0 || d.portal_timer <= 0.0);
                if !go {
                    return;
                }
                d.pc += 1;
                d.ending = true;
                d.phase = DwPhase::Transition;
                d.phase_t = 0.0;
                w.sound("sfx_darkWorldPortalEnter01", 160.0);
                d.prog.extend([
                    Op::Wait(31.0 + 31.0 + 41.0 + 31.0 + 31.0 + 31.0),
                    Op::NextLevel,
                ]);
            }
            Op::Transition => d.pc += 1,
            Op::NextLevel => {
                d.pc += 1;
                next_level(w, c, d);
                return;
            }
            Op::Complete => d.pc += 1,
        }
    }
}

/// One tick of wave `i` (§5.2). Returns true when the spawn loop is over.
fn wave(w: &mut World, c: &Content, d: &mut DarkWorld, i: u8) -> bool {
    let lvl = d.level(c).clone();
    let tower = d.tower(c).clone();
    if d.wave.is_none_or(|wv| wv.index != i) {
        let difficulty = lvl.difficulty + i as i32;
        let time_left =
            ((tower.time_base + tower.time_add * difficulty) as f32 * d.player_multiplier()) as i32;
        let mid = (i as i32 == lvl.waves / 2) && !d.treasure.is_empty();
        if mid {
            d.rng.shuffle(&mut d.chest_spots);
        }
        d.wave = Some(WaveState {
            index: i,
            difficulty,
            time_left,
            started: true,
            chests_left: if mid {
                d.treasure.len().min(d.chest_spots.len()) as u8
            } else {
                0
            },
            chest_wait: 0.0,
        });
    }
    let mut wv = d.wave.unwrap();
    // Mid-level treasure: one chest per item, wait(20) between.
    if wv.chests_left > 0 {
        if wv.chest_wait > 0.0 {
            wv.chest_wait -= w.time.time_mult;
        } else {
            let k = d.treasure.len().min(d.chest_spots.len()) - wv.chests_left as usize;
            chest(w, d.chest_spots[k], d.treasure[k], 0.0);
            wv.chests_left -= 1;
            // wait(20): the next chest 21 ticks later.
            wv.chest_wait = 20.0;
        }
        d.wave = Some(wv);
        return false;
    }
    if d.ending {
        return true;
    }
    // Pacing counter.
    if d.enemy_delay > 0.0 {
        pace(w, d);
        d.wave = Some(wv);
        return false;
    }
    if wv.time_left <= 0 {
        return true;
    }
    let set = &tower.sets[lvl.enemy_set.min(tower.sets.len() - 1)].1;
    let Some(e) = draw_enemy(d, set, wv.difficulty) else {
        return true;
    };
    wv.time_left -= e.delay;
    if let Some(p) = pick_portal(d, e.portal) {
        portal::appear_and_spawn(w, p, e.ty);
    }
    d.enemy_delay = e.delay as f32 / d.player_multiplier()
        * lvl.delay_multiplier
        * d.difficulty_delay_multiplier();
    d.wave = Some(wv);
    false
}

/// "Disable dangers" after the clear: sensor blocks stop, brambles retract.
pub(crate) fn disable_dangers(w: &mut World) {
    crate::entities::platforms::disable_sensors(w);
    let tiles: Vec<EntityId> = w.tagged(Tag::Brambles).to_vec();
    for b in tiles {
        crate::arrows::brambles::shrink(w, b, true);
    }
}

/// `DistributeArrows`: loose collectible arrows go to the archer with the
/// fewest arrows who can hold the type (ties: lowest index).
fn distribute_arrows(w: &mut World, d: &mut DarkWorld) {
    let arrows: Vec<EntityId> = w.tagged(Tag::Arrow).to_vec();
    for a in arrows {
        let Some(ar) = w.arrow(a).copied() else {
            continue;
        };
        if !ar.collectible() || ar.arrow_type == ArrowType::Laser {
            continue;
        }
        let best = d
            .active_slots()
            .into_iter()
            .filter(|&p| d.last_inv[p].arrows.can_add_arrow(ar.arrow_type))
            .min_by_key(|&p| (d.last_inv[p].arrows.len(), p));
        if let Some(p) = best {
            d.last_inv[p].arrows.add_arrows(&[ar.arrow_type]);
            w.remove(a);
        }
    }
}

/// `DarkWorldSessionState.OnPlayerDeath`: the dying archer's inventory
/// (wings already lost, boots and invisibility dropped) becomes its
/// last known one. Called by the death sequence while the archer's state
/// is still at hand.
pub fn record_death_inventory(w: &mut World, p: &player::Player) {
    if let Some(d) = w.dark_world.as_mut() {
        let mut inv = inventory_of(p);
        inv.boots = false;
        inv.invisible = false;
        d.last_inv[p.index as usize] = inv;
    }
}

fn inventory_of(p: &player::Player) -> Inventory {
    Inventory {
        arrows: p.arrows,
        shield: p.has_shield,
        wings: p.has_wings,
        boots: p.speed_boots,
        invisible: p.invisible,
    }
}

/// A player went through the exit portal (from the portal part).
pub fn player_entered(w: &mut World, pid: EntityId) {
    let Some(p) = w.player(pid).copied() else {
        return;
    };
    let Some(d) = w.dark_world.as_mut() else {
        return;
    };
    let i = p.index as usize;
    let mut inv = inventory_of(&p);
    inv.shield |= d.settings.variants.any(Variant::StartWithShields);
    inv.wings |= d.settings.variants.any(Variant::StartWithWings);
    d.last_inv[i] = inv;
    d.next_hats[i] = p.hat;
    d.entered[i] = true;
    w.emit(crate::Event::Particles {
        kind: crate::events::ParticleKind::LightFade,
        player: p.index,
        pos: w.core(pid).pos,
        count: 1,
        dir: 0.0,
    });
    w.remove(pid);
}

fn next_level(w: &mut World, c: &Content, d: &mut DarkWorld) {
    // Players still on the field keep what they carry.
    for id in w.players().collect::<Vec<_>>() {
        if let Some(p) = w.player(id).copied()
            && !p.dead
        {
            let i = p.index as usize;
            d.last_inv[i] = inventory_of(&p);
            d.next_hats[i] = p.hat;
        }
    }
    distribute_arrows(w, d);
    // OnLevelComplete.
    d.start_inv = d.last_inv;
    for inv in d.start_inv.iter_mut() {
        if inv.arrows.is_empty() {
            inv.arrows.add_arrows(&[ArrowType::Normal]);
        }
    }
    d.old_boss_hits = d.boss_hits;
    d.level_tries = 0;
    d.round_index += 1;
    let arena = d.level(c).arena;
    w.load_scene(c, arena);
    let taken = std::mem::replace(d, placeholder(d));
    w.dark_world = Some(Box::new(taken));
    begin_level(w, c, false);
    if let Some(nd) = w.dark_world.take() {
        *d = *nd;
    }
}

/// A throwaway copy used while the real state is back in the world.
fn placeholder(d: &DarkWorld) -> DarkWorld {
    d.clone()
}

fn complete(w: &mut World, d: &mut DarkWorld) {
    d.ending = true;
    d.phase = DwPhase::Complete;
    d.phase_t = 0.0;
    let n = d.n_players();
    let slots = d.active_slots();
    let deaths: u32 = d.deaths.iter().sum();
    // Player scores for the crown (2+ players): max kills, min deaths, max
    // revives, max boss hits are worth one point each.
    let mut score = [0u32; MAX_PLAYERS];
    if n > 1 {
        let mut award = |vals: &[u32; MAX_PLAYERS], max: bool| {
            let best = slots.iter().map(|&p| vals[p]);
            let b = if max { best.max() } else { best.min() };
            if let Some(b) = b {
                for &p in &slots {
                    if vals[p] == b {
                        score[p] += 1;
                    }
                }
            }
        };
        award(&d.kills, true);
        award(&d.deaths, false);
        award(&d.revives, true);
        award(&d.boss_hits, true);
    }
    let top = slots.iter().map(|&p| score[p]).max().unwrap_or(0);
    let mut crowns = [false; MAX_PLAYERS];
    if n > 1 {
        for &p in &slots {
            crowns[p] = score[p] == top;
        }
    }
    let legendary = d.settings.difficulty == LEGENDARY;
    d.result = Some(DwResult {
        completed: true,
        tower: d.settings.tower as u8,
        difficulty: d.settings.difficulty,
        players: n as u8,
        time: d.time,
        continues: d.continues,
        deaths,
        curses: curse_count(&d.settings.variants),
        eye: legendary && d.continues == 0,
        gold_eye: legendary && d.continues == 0 && deaths == 0,
        kills: d.kills,
        deaths_per: d.deaths,
        revives: d.revives,
        boss_hits: d.boss_hits,
        best_combo: d.best_combo,
        crowns,
    });
    w.sound("sfx_complete1flyin", 160.0);
}

fn game_over(w: &mut World, d: &mut DarkWorld, effects: bool) {
    if d.ending {
        return;
    }
    if effects {
        w.orb.do_slow_mo_kill(&mut w.time);
        w.sound("sfx_finalKill", 160.0);
    }
    d.ending = true;
    d.phase = DwPhase::GameOver;
    d.phase_t = 0.0;
    d.menu_ready = false;
    let n = d.n_players();
    d.result = Some(DwResult {
        completed: false,
        tower: d.settings.tower as u8,
        difficulty: d.settings.difficulty,
        players: n as u8,
        time: d.time,
        continues: d.continues,
        deaths: d.deaths.iter().sum(),
        curses: curse_count(&d.settings.variants),
        eye: false,
        gold_eye: false,
        kills: d.kills,
        deaths_per: d.deaths,
        revives: d.revives,
        boss_hits: d.boss_hits,
        best_combo: d.best_combo,
        crowns: [false; MAX_PLAYERS],
    });
    w.sound("sfx_gameOver", 160.0);
}

// --------------------------------------------------------- menu actions

/// Game-over / pause "CONTINUE" (or "RESTART LEVEL"): reload the level with
/// default inventories. False when not allowed.
pub fn continue_level(w: &mut World, c: &Content) -> bool {
    let Some(mut d) = w.dark_world.take() else {
        return false;
    };
    if !d.can_continue() {
        w.dark_world = Some(d);
        return false;
    }
    d.extra_lives = if d.n_players() == 1 { 3 } else { 0 };
    d.level_tries += 1;
    d.continues += 1;
    d.start_inv = [d.default_inv; MAX_PLAYERS];
    d.last_inv = d.start_inv;
    d.next_hats = [HatState::Normal; MAX_PLAYERS];
    d.boss_hits = d.old_boss_hits;
    d.result = None;
    d.menu_ready = false;
    let arena = d.level(c).arena;
    w.dark_world = Some(d);
    w.load_scene(c, arena);
    w.time.time_rate = 1.0;
    begin_level(w, c, false);
    true
}

/// "RESTART": a brand-new session on the same settings.
pub fn restart(w: &mut World, c: &Content) {
    let Some(d) = w.dark_world.as_ref() else {
        return;
    };
    let mut s = d.settings;
    // A restart skips the intro screens (title and trigger tips).
    s.start_screen = false;
    s.trigger_tips = false;
    let attempts = d.attempts + 1;
    *w = new_dark_world(c, s);
    if let Some(nd) = w.dark_world.as_mut() {
        nd.attempts = attempts;
    }
}

// ------------------------------------------------------------- hooks

/// `RegisterEnemyKill` (§4.3).
pub fn register_kill(w: &mut World, at: Vec2, killer: i8, bounty: u16) {
    let Some(d) = w.dark_world.as_mut() else {
        return;
    };
    if bounty == 0 || killer < 0 || d.settings.slots[killer as usize].is_none() {
        return;
    }
    let k = killer as usize;
    d.kills[k] += 1;
    d.combo[k] += 1;
    d.combo_timer[k] = 120.0;
    d.best_combo[k] = d.best_combo[k].max(d.combo[k]);
    let n = d.combo[k].min(6) as u8;
    w.sound_indexed("sfx_enemyKill", n - 1, at.x);
}

/// `DarkWorldRoundLogic.OnPlayerDeath` (§3, §4.2).
pub fn on_player_death(
    w: &mut World,
    _c: &Content,
    victim: u8,
    _killer: i8,
    _cause: DeathCause,
    _at: Vec2,
) {
    let Some(mut d) = w.dark_world.take() else {
        return;
    };
    let v = victim as usize;
    d.next_hats[v] = HatState::Normal;
    d.deaths[v] += 1;
    let someone_in = d.entered.iter().any(|&e| e);
    if !someone_in && !d.ending && all_dead(w, &d, victim) {
        if d.extra_lives > 0 {
            d.auto_revive = 60.0;
        } else {
            game_over(w, &mut d, true);
        }
    }
    w.dark_world = Some(d);
}

/// `CoOpCheckForAllDead`: nobody alive (the victim counts as dead), no
/// revive in progress, no auto revive running.
fn all_dead(w: &World, d: &DarkWorld, victim: u8) -> bool {
    if d.auto_revive > 0.0 || d.revive_timeout > 0.0 || crate::revive::any_pending(w) {
        return false;
    }
    !w.players()
        .any(|id| w.player(id).is_some_and(|p| !p.dead && p.index != victim))
}

/// A revive finished (stats).
pub fn on_revived(w: &mut World, reviver: Option<u8>) {
    if let (Some(d), Some(r)) = (w.dark_world.as_mut(), reviver) {
        d.revives[r as usize] += 1;
    }
}

/// `CheckForGameOver` (a revive failed: no free spot).
pub fn check_game_over(w: &mut World) {
    let Some(mut d) = w.dark_world.take() else {
        return;
    };
    if !d.ending && !d.entered.iter().any(|&e| e) && all_dead(w, &d, u8::MAX) && d.extra_lives <= 0
    {
        game_over(w, &mut d, false);
    }
    w.dark_world = Some(d);
}

/// Boss damage bookkeeping (`DarkWorldBoss.Hurt`): `lost` health off the
/// pool, a boss hit for a real killer.
pub fn boss_damaged(w: &mut World, lost: i32, killer: i8) {
    if let Some(d) = w.dark_world.as_mut() {
        d.boss_health -= lost;
        if killer >= 0 && (killer as usize) < MAX_PLAYERS {
            d.boss_hits[killer as usize] += 1;
        }
    }
}

/// `EnterBossMode`: the pool is the sum of the bosses' max health.
pub fn enter_boss_mode(w: &mut World, max: i32) {
    if let Some(d) = w.dark_world.as_mut() {
        d.boss_mode = true;
        d.max_boss_health = max;
        d.boss_health = max;
        w.sound("sfx_bossHealthbar", 160.0);
    }
}

pub fn boss_finished(w: &mut World) {
    if let Some(d) = w.dark_world.as_mut() {
        d.boss_finished = true;
    }
}

pub fn signal_boss_treasure(w: &mut World) {
    if let Some(d) = w.dark_world.as_mut() {
        d.boss_treasure_signal = true;
    }
}

/// Lock / unlock the archers' shooting.
pub fn set_shoot_lock(w: &mut World, on: bool) {
    if let Some(d) = w.dark_world.as_mut() {
        d.shoot_lock = on;
    }
}

/// The round has started (boss intros wait for it).
pub fn round_started(w: &World) -> bool {
    w.dark_world.as_ref().is_some_and(|d| d.started)
}

/// Difficulty index and player count (boss formulas).
pub fn params(w: &World) -> (i32, i32) {
    w.dark_world.as_ref().map_or((0, 1), |d| {
        (d.settings.difficulty as i32, d.n_players() as i32)
    })
}

// ----------------------------------------------------------- tools/tests

/// Level select (level test / tests): load level `index` of the session
/// with the current inventories.
pub fn jump_to_level(w: &mut World, c: &Content, index: usize) {
    let Some(d) = w.dark_world.as_mut() else {
        return;
    };
    d.round_index = index.min(d.levels(c).len() - 1);
    let arena = d.level(c).arena;
    w.load_scene(c, arena);
    begin_level(w, c, false);
}

/// The running boss: (boss id, its state number).
pub fn boss_phase(w: &World) -> Option<(u8, u8)> {
    let b = w.dark_world.as_ref()?.boss.as_ref()?;
    Some(match &b.kind {
        boss::BossKind::Amaranth(s) => (0, s.state),
        boss::BossKind::Dreadwood(s) => (1, s.phase),
        boss::BossKind::Cyclops(s) => (2, s.state),
        boss::BossKind::Cataclysm(s) => (3, s.state),
    })
}

/// Damage the boss as a hit would (Cataclysm's heart counts hits).
pub fn debug_damage(w: &mut World, n: i32, killer: i8) {
    boss::with_boss(w, |w, b| match &mut b.kind {
        boss::BossKind::Cataclysm(s) if s.heart => s.heart_health -= n,
        _ => {
            boss::hurt(w, &mut b.base, n, killer);
        }
    });
}
