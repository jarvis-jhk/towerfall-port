//! Quest mode (docs/spec/quest.md): co-op wave survival with lives,
//! auto-revive, life stealing, combos, the two gauntlets, game over and
//! completion. 1-2 archers as in the original; more behind
//! [`QuestSettings::extended`] (quest.md §7, our proposal).
//!
//! `QuestControl`'s coroutine is a small program of [`Op`]s run by
//! [`session_update`]; a wave group expands into more ops when it starts
//! (its shuffles happen then, from the quest RNG).

pub mod floor;
pub mod script;

use crate::checksum::{Hasher, StableHash};
use crate::content::Content;
use crate::enemies::{self, EnemyType, portal};
use crate::entity::{Allegiance, EntityId, Kind};
use crate::math::Vec2;
use crate::pickups::Pickups;
use crate::player::{self, DeathCause, HatState, PState};
use crate::rng::Rng;
use crate::world::{MAX_PLAYERS, MatchSetup, PlayerSetup, World};
pub use script::QuestScript;

/// Everything all peers agree on before a quest starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuestSettings {
    /// Index into `Content::levels` (the arena).
    pub level: usize,
    /// Index into `Content::quest_scripts`.
    pub script: usize,
    /// Tower id 0..13 (King's Court 7 and Ascension 11 have reaper waves).
    pub tower_id: u8,
    pub hardcore: bool,
    /// Archer per joined slot.
    pub slots: [Option<u16>; MAX_PLAYERS],
    /// Co-op crowns from the previous clear (quest.md §5.5).
    pub crowns: [bool; MAX_PLAYERS],
    /// Allow more than 2 archers (quest.md §7).
    pub extended: bool,
    pub seed: i32,
    /// Show the title card and wait for Confirm (else the restart path).
    pub start_screen: bool,
}

impl QuestSettings {
    pub fn new(level: usize, script: usize, players: usize) -> QuestSettings {
        let mut slots = [None; MAX_PLAYERS];
        for (i, s) in slots.iter_mut().enumerate().take(players) {
            *s = Some(i as u16);
        }
        QuestSettings {
            level,
            script,
            tower_id: 0,
            hardcore: false,
            slots,
            crowns: [false; MAX_PLAYERS],
            extended: false,
            seed: 0,
            start_screen: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestPhase {
    /// Title card / skip sequence before `StartRound`.
    Intro,
    Playing,
    /// Last wave done, level ending.
    Ending,
    GameOver,
    Complete,
    /// Results acknowledged; the menu is outside the sim.
    Finished,
}

/// One instruction of the control coroutine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Wait(f32),
    WaitConfirm,
    StartRound,
    WaveStart(u16),
    WaveHud,
    WaveEffects(u16),
    Group(u16, u16),
    /// Open/close one portal; `wait(5)` if it changed.
    PortalSet(EntityId, bool),
    SpawnOn(EntityId, EnemyType),
    /// A treasure chest (spot, large, contents); appears 5 ticks later.
    Chest(Vec2, bool, [Option<Pickups>; 3]),
    /// `Wings?B` (0) / `Shield?B` (1): resolved when the step runs.
    ChestCond(Vec2, u8, Option<Pickups>),
    /// Gauntlet: wait until no chest is still closed.
    WaitChestsOpened,
    Reaper(i32, bool),
    /// The King's transformation starts (60 ticks into its sequence).
    KingTransform,
    /// The Reaper appears from the King's spot (difficulty, final wave).
    ReaperFromKing(i32, bool),
    /// Stop the music, then the intro steps below.
    Jester,
    JesterSound,
    JesterSpawn,
    /// Every FloorMiasma dies out.
    FloorsDieOut,
    ClosePortals,
    WaitNoEnemies,
    WaveEnd(u16),
    GGroupStart(u16),
    GSpawn(EntityId, EnemyType),
    WaitTotalBelow(i32),
    WaitTotalZero,
    GDarkEnd,
    LevelEnding,
    Complete,
}

impl StableHash for Op {
    fn stable_hash(&self, h: &mut Hasher) {
        let (k, a, b): (u8, f32, u32) = match *self {
            Op::Wait(n) => (0, n, 0),
            Op::WaitConfirm => (1, 0.0, 0),
            Op::StartRound => (2, 0.0, 0),
            Op::WaveStart(i) => (3, 0.0, i as u32),
            Op::WaveHud => (4, 0.0, 0),
            Op::WaveEffects(i) => (5, 0.0, i as u32),
            Op::Group(a, b) => (6, a as f32, b as u32),
            Op::PortalSet(e, o) => (7, o as u32 as f32, e.index),
            Op::SpawnOn(e, _) => (8, 0.0, e.index),
            Op::Chest(p, big, items) => (9, p.x + p.y * 1000.0, chest_code(big, &items)),
            Op::WaitChestsOpened => (22, 0.0, 0),
            Op::ChestCond(p, k, b) => (
                23,
                p.x + p.y * 1000.0,
                k as u32 | b.map_or(0, |b| b as u32 + 1) << 8,
            ),
            Op::Reaper(d, f) => (10, d as f32, f as u32),
            Op::KingTransform => (27, 0.0, 0),
            Op::ReaperFromKing(d, f) => (28, d as f32, f as u32),
            Op::Jester => (11, 0.0, 0),
            Op::JesterSound => (24, 0.0, 0),
            Op::JesterSpawn => (25, 0.0, 0),
            Op::FloorsDieOut => (26, 0.0, 0),
            Op::ClosePortals => (12, 0.0, 0),
            Op::WaitNoEnemies => (13, 0.0, 0),
            Op::WaveEnd(i) => (14, 0.0, i as u32),
            Op::GGroupStart(i) => (15, 0.0, i as u32),
            Op::GSpawn(e, _) => (16, 0.0, e.index),
            Op::WaitTotalBelow(m) => (17, m as f32, 0),
            Op::WaitTotalZero => (18, 0.0, 0),
            Op::GDarkEnd => (19, 0.0, 0),
            Op::LevelEnding => (20, 0.0, 0),
            Op::Complete => (21, 0.0, 0),
        };
        h.u8(k);
        h.f32(a);
        h.u32(b);
    }
}

/// The running quest (`QuestControl` + `QuestRoundLogic`).
#[derive(Clone, Debug, PartialEq)]
pub struct Quest {
    pub settings: QuestSettings,
    pub rng: Rng,
    pub phase: QuestPhase,
    pub phase_t: f32,
    pub prog: Vec<Op>,
    pub pc: usize,
    wait: f32,
    pub lives: [i32; MAX_PLAYERS],
    pub deaths: [u32; MAX_PLAYERS],
    pub kills: [u32; MAX_PLAYERS],
    pub best_combo: [u32; MAX_PLAYERS],
    pub combo: [u32; MAX_PLAYERS],
    combo_timer: [f32; MAX_PLAYERS],
    auto_revive: [f32; MAX_PLAYERS],
    steal_timer: [f32; MAX_PLAYERS],
    pub can_steal: [bool; MAX_PLAYERS],
    pub current_wave: i32,
    pub total_waves: i32,
    pub between_waves: bool,
    /// Monsters left in a gauntlet.
    pub gauntlet: Option<i32>,
    /// Round clock in ticks (from `StartRound`).
    pub time: u32,
    pub started: bool,
    pub ending: bool,
    /// Portals by spawner marker index.
    pub portals: Vec<EntityId>,
    /// Shuffled chest spots (normal, big) and the next index of each.
    chest_spots: Vec<Vec2>,
    big_spots: Vec<Vec2>,
    chest_next: usize,
    big_next: usize,
    wave_slow: bool,
    king_transformed: bool,
    /// Shuffled spawner names per gauntlet group.
    gauntlet_names: Vec<Vec<usize>>,
    gauntlet_lists: Vec<Vec<EnemyType>>,
    pub no_deaths: bool,
    pub ready: bool,
}

impl StableHash for Quest {
    fn stable_hash(&self, h: &mut Hasher) {
        self.rng.hash_into(h);
        h.u8(self.phase as u8);
        h.f32(self.phase_t);
        self.prog.stable_hash(h);
        h.u32(self.pc as u32);
        h.f32(self.wait);
        self.lives.stable_hash(h);
        self.deaths.stable_hash(h);
        self.kills.stable_hash(h);
        self.combo.stable_hash(h);
        self.combo_timer.stable_hash(h);
        self.auto_revive.stable_hash(h);
        self.steal_timer.stable_hash(h);
        self.can_steal.stable_hash(h);
        h.i32(self.current_wave);
        h.bool(self.between_waves);
        self.gauntlet.stable_hash(h);
        h.u32(self.time);
        h.bool(self.started);
        h.bool(self.ending);
    }
}

impl Quest {
    fn n_players(&self) -> usize {
        self.active_slots().len()
    }
    pub fn active_slots(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..MAX_PLAYERS)
            .filter(|&p| self.settings.slots[p].is_some())
            .collect();
        if !self.settings.extended {
            v.truncate(2);
        }
        v
    }
    fn script<'a>(&self, c: &'a Content) -> &'a QuestScript {
        &c.quest_scripts[self.settings.script]
    }
    fn waves<'a>(&self, c: &'a Content) -> &'a [script::Wave] {
        let s = self.script(c);
        if self.settings.hardcore {
            &s.hardcore
        } else {
            &s.normal
        }
    }
    fn gsteps<'a>(&self, c: &'a Content) -> &'a [script::GStep] {
        let s = self.script(c);
        if self.settings.hardcore {
            &s.hardcore_gauntlet
        } else {
            &s.normal_gauntlet
        }
    }
    /// Group total multiplier for n > 2 (quest.md §7).
    fn scale(&self) -> f32 {
        let n = self.n_players();
        if self.settings.extended && n > 2 {
            1.0 + 0.25 * (n - 2) as f32
        } else {
            1.0
        }
    }
}

/// Snapshot for HUD and result screens.
#[derive(Clone, Debug, PartialEq)]
pub struct QuestView {
    pub phase: QuestPhase,
    pub phase_t: f32,
    pub ready: bool,
    pub hardcore: bool,
    pub wave: i32,
    pub total_waves: i32,
    pub between_waves: bool,
    pub gauntlet: Option<i32>,
    pub lives: [i32; MAX_PLAYERS],
    pub deaths: [u32; MAX_PLAYERS],
    pub kills: [u32; MAX_PLAYERS],
    pub best_combo: [u32; MAX_PLAYERS],
    pub combo: [u32; MAX_PLAYERS],
    pub can_steal: [bool; MAX_PLAYERS],
    pub time_ticks: u32,
    pub no_deaths: bool,
}

impl Quest {
    /// The tower's intro King became the King Reaper (King's Court; the
    /// presentation hides the King NPC from then on).
    pub fn king_transformed(&self) -> bool {
        self.king_transformed
    }

    pub fn view(&self) -> QuestView {
        QuestView {
            phase: self.phase,
            phase_t: self.phase_t,
            ready: self.ready,
            hardcore: self.settings.hardcore,
            wave: self.current_wave,
            total_waves: self.total_waves,
            between_waves: self.between_waves,
            gauntlet: self.gauntlet,
            lives: self.lives,
            deaths: self.deaths,
            kills: self.kills,
            best_combo: self.best_combo,
            combo: self.combo,
            can_steal: self.can_steal,
            time_ticks: self.time,
            no_deaths: self.no_deaths,
        }
    }
}

// ------------------------------------------------------------ setup

/// Create the world for a quest run: arena, portals, archers, control program.
pub fn new_quest(c: &Content, settings: QuestSettings) -> World {
    let setup = MatchSetup {
        level: settings.level,
        seed: settings.seed,
        start_frozen: true,
        ..MatchSetup::default()
    };
    let mut w = World::new(c, &setup);
    let mut q = Quest {
        settings,
        rng: Rng::new(settings.seed ^ 0x51ce),
        phase: QuestPhase::Intro,
        phase_t: 0.0,
        prog: vec![],
        pc: 0,
        wait: 0.0,
        lives: [0; MAX_PLAYERS],
        deaths: [0; MAX_PLAYERS],
        kills: [0; MAX_PLAYERS],
        best_combo: [0; MAX_PLAYERS],
        combo: [0; MAX_PLAYERS],
        combo_timer: [0.0; MAX_PLAYERS],
        auto_revive: [-1.0; MAX_PLAYERS],
        steal_timer: [-1.0; MAX_PLAYERS],
        can_steal: [false; MAX_PLAYERS],
        current_wave: -1,
        total_waves: 0,
        between_waves: true,
        gauntlet: None,
        time: 0,
        started: false,
        ending: false,
        portals: vec![],
        chest_spots: vec![],
        big_spots: vec![],
        chest_next: 0,
        big_next: 0,
        wave_slow: false,
        king_transformed: false,
        gauntlet_names: vec![],
        gauntlet_lists: vec![],
        no_deaths: true,
        ready: false,
    };
    let slots = q.active_slots();
    let n = slots.len();
    for &p in &slots {
        q.lives[p] = if n == 1 { 5 } else { 4 };
    }
    q.portals = portal::add_level_portals(&mut w, c);
    floor::spawn_all(&mut w, c);
    let lvl = c.level(settings.level);
    // `TreasureChest` / `BigTreasureChest` positions (ASCII 'T' / 'G').
    q.chest_spots = lvl.treasure.clone();
    q.big_spots = lvl.big_treasure.clone();
    q.rng.shuffle(&mut q.chest_spots);
    q.rng.shuffle(&mut q.big_spots);
    spawn_players(&mut w, c, &q);
    build_program(c, &mut q);
    w.quest = Some(Box::new(q));
    crate::entities::apply_solo_seek(&mut w);
    w
}

fn spawn_players(w: &mut World, c: &Content, q: &Quest) {
    let lvl = c.level(q.settings.level);
    let mut spots = lvl.player_spawns.clone();
    spots.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let slots = q.active_slots();
    if spots.len() < slots.len() {
        let mut rng = Rng::new(q.settings.seed);
        spots = crate::versus::spawn::derive_spawns(c, lvl, slots.len(), &mut rng);
        spots.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    }
    for (k, &p) in slots.iter().enumerate() {
        let at = spots
            .get(k)
            .copied()
            .unwrap_or(Vec2::new(lvl.width as f32 / 2.0, 0.0));
        let hat = if slots.len() > 1 && q.settings.crowns[p] {
            HatState::Crown
        } else {
            HatState::Normal
        };
        let ps = PlayerSetup {
            archer: q.settings.slots[p].unwrap_or(0),
            allegiance: Allegiance::Blue,
            team_color: Allegiance::Neutral,
            hat,
            arrows: 3,
            ..Default::default()
        };
        player::create(w, c, p as u8, at + Vec2::new(0.0, 2.0), &ps);
    }
}

fn build_program(c: &Content, q: &mut Quest) {
    let mut p = vec![];
    if q.settings.start_screen {
        p.extend([
            Op::Wait(30.0),
            Op::Wait(30.0),
            Op::Wait(10.0),
            Op::WaitConfirm,
            Op::Wait(30.0),
        ]);
    }
    p.extend([Op::Wait(10.0), Op::StartRound]);
    let gauntlet = q.script(c).gauntlet;
    if gauntlet {
        let steps = q.gsteps(c).to_vec();
        let n = q.n_players();
        let names_n = c.level(q.settings.level).markers.spawners.len();
        let mut total = 0;
        for (i, s) in steps.iter().enumerate() {
            match s {
                script::GStep::Group { enemies, .. } => {
                    let mut list = vec![];
                    for t in enemies {
                        for _ in 0..t.count(n, q.settings.extended) {
                            list.push(t.ty);
                        }
                    }
                    q.rng.shuffle(&mut list);
                    total += list.len() as i32;
                    let mut names: Vec<usize> = (0..names_n).collect();
                    q.rng.shuffle(&mut names);
                    q.gauntlet_lists.push(list);
                    q.gauntlet_names.push(names);
                    p.push(Op::GGroupStart(i as u16));
                }
                script::GStep::Treasure(_) | script::GStep::BigTreasure(_) => {
                    q.gauntlet_lists.push(vec![]);
                    q.gauntlet_names.push(vec![]);
                    let (big, names): (bool, Vec<&str>) = match s {
                        script::GStep::Treasure(t) => (false, vec![t.as_str()]),
                        script::GStep::BigTreasure(l) => {
                            (true, l.iter().map(|s| s.as_str()).collect())
                        }
                        _ => unreachable!(),
                    };
                    let spots = if big { &q.big_spots } else { &q.chest_spots };
                    let spot = spots
                        .get(if big { q.big_next } else { q.chest_next })
                        .copied()
                        .unwrap_or(Vec2::new(160.0, 120.0));
                    if big {
                        q.big_next += 1;
                    } else {
                        q.chest_next += 1;
                    }
                    // Event mode: created, then triggered 5 ticks later.
                    p.extend([
                        Op::Chest(spot, big, items(&names)),
                        Op::Wait(5.0),
                        Op::WaitChestsOpened,
                        Op::Wait(60.0),
                    ]);
                }
            }
        }
        q.gauntlet = Some(total);
        q.total_waves = 0;
        p.insert(
            p.iter().position(|o| *o == Op::StartRound).unwrap() + 1,
            Op::Wait(60.0),
        );
    } else {
        let waves = q.waves(c).len();
        q.total_waves = waves as i32;
        p.push(Op::Wait(60.0));
        for i in 0..waves {
            let groups = q.waves(c)[i].groups.len();
            p.extend([
                Op::WaveStart(i as u16),
                Op::WaveHud,
                Op::WaveEffects(i as u16),
            ]);
            for g in 0..groups {
                p.push(Op::Group(i as u16, g as u16));
            }
            p.extend([Op::ClosePortals, Op::WaitNoEnemies, Op::WaveEnd(i as u16)]);
        }
    }
    p.extend([Op::LevelEnding, Op::Wait(60.0), Op::Complete]);
    q.prog = p;
}

// ------------------------------------------------------------ the program

fn confirm_pressed(w: &World, q: &Quest) -> bool {
    q.active_slots().iter().any(|&p| {
        w.connected[p] && (w.inputs[p].state.jump_pressed || w.inputs[p].state.start_pressed)
    })
}

fn included(g: &script::Group, n: usize, extended: bool) -> bool {
    if g.coop {
        n == 2 || (extended && n >= 2)
    } else if g.solo {
        n == 1
    } else {
        true
    }
}

/// Expand a wave group into ops (quest.md §4.4).
fn expand_group(w: &mut World, c: &Content, q: &mut Quest, wi: usize, gi: usize) -> Vec<Op> {
    let g = q.waves(c)[wi].groups[gi].clone();
    let n = q.n_players();
    let mut out = vec![];
    if !included(&g, n, q.settings.extended) {
        return out;
    }
    if g.delay > 0 {
        out.push(Op::Wait(g.delay as f32));
    }
    let mut enemies = vec![];
    for t in &g.enemies {
        for _ in 0..t.count(n, q.settings.extended) {
            enemies.push(t.ty);
        }
    }
    let base = enemies.len();
    let target = (base as f32 * q.scale()).round() as usize;
    for k in base..target.max(base) {
        enemies.push(enemies[k % base]);
    }
    q.rng.shuffle(&mut enemies);
    let mut spawns = g.spawns.clone();
    q.rng.shuffle(&mut spawns);
    let names: Vec<(EntityId, String)> = portal::portals(w, c);
    for (pid, name) in &names {
        let open = spawns
            .iter()
            .position(|s| s == name)
            .is_some_and(|i| i < enemies.len());
        out.push(Op::PortalSet(*pid, open));
    }
    if !enemies.is_empty() || !names.is_empty() {
        out.push(Op::Wait(60.0));
    }
    if g.reaper >= 0 {
        let fin = wi + 1 == q.waves(c).len();
        out.push(Op::Reaper(g.reaper, fin));
    }
    if g.jester {
        out.push(Op::Jester);
    }
    let mut k = 0;
    for ty in enemies {
        if spawns.is_empty() {
            break;
        }
        if k >= spawns.len() {
            q.rng.shuffle(&mut spawns);
            k = 0;
        }
        if let Some((pid, _)) = names.iter().find(|(_, nm)| *nm == spawns[k]) {
            out.push(Op::SpawnOn(*pid, ty));
            out.push(Op::Wait(20.0));
        }
        k += 1;
    }
    for t in &g.treasure {
        if q.chest_next >= q.chest_spots.len() {
            break;
        }
        let spot = q.chest_spots[q.chest_next];
        q.chest_next += 1;
        let op = match t.split_once('?') {
            Some(("Wings", b)) => Op::ChestCond(spot, 0, Pickups::from_name(b)),
            Some(("Shield", b)) => Op::ChestCond(spot, 1, Pickups::from_name(b)),
            Some((a, _)) => Op::Chest(spot, false, items(&[a])),
            None => Op::Chest(spot, false, items(&[t.as_str()])),
        };
        out.push(op);
        out.push(Op::Wait(10.0));
    }
    if !g.big_treasure.is_empty() && q.big_next < q.big_spots.len() {
        let spot = q.big_spots[q.big_next];
        q.big_next += 1;
        let names: Vec<&str> = g.big_treasure.iter().map(|s| s.as_str()).collect();
        out.push(Op::Chest(spot, true, items(&names)));
        out.push(Op::Wait(10.0));
    }
    out.push(Op::Wait(30.0));
    out
}

/// `A?B` (quest.md §4.4.7): Wings/Shield fall back to `B` when the
/// living archers that already have it number at least the match's
/// player count.
fn resolve_conditional(w: &World, q: &Quest, which: u8, b: Option<Pickups>) -> Option<Pickups> {
    let have = w
        .players()
        .filter(|&p| {
            w.player(p).is_some_and(|pl| {
                !pl.dead
                    && if which == 0 {
                        pl.has_wings
                    } else {
                        pl.has_shield
                    }
            })
        })
        .count();
    if have >= q.active_slots().len() {
        b
    } else if which == 0 {
        Some(Pickups::Wings)
    } else {
        Some(Pickups::Shield)
    }
}

/// Pickups by name (unknown names are skipped; at most 3).
fn items(names: &[&str]) -> [Option<Pickups>; 3] {
    let mut out = [None; 3];
    for (slot, p) in out
        .iter_mut()
        .zip(names.iter().filter_map(|n| Pickups::from_name(n)))
    {
        *slot = Some(p);
    }
    out
}

fn chest_code(big: bool, items: &[Option<Pickups>; 3]) -> u32 {
    let mut b = big as u32;
    for (k, p) in items.iter().enumerate() {
        b |= p.map_or(0, |p| p as u32 + 1) << (1 + 8 * k);
    }
    b
}

/// A Quest chest (appears after 5 ticks, flashing).
fn spawn_chest(w: &mut World, at: Vec2, big: bool, contents: [Option<Pickups>; 3]) {
    use crate::treasure::{self, Appear, ChestSpawn, ChestType};
    let ty = if big {
        ChestType::Large
    } else {
        ChestType::Normal
    };
    treasure::create(
        w,
        &ChestSpawn {
            pos: at,
            ty,
            timer: 5.0,
            contents,
        },
        Appear::Time,
    );
}

/// No chest is waiting, appearing or closed.
fn chests_opened(w: &World) -> bool {
    use crate::treasure::ChestState;
    !w.entities()
        .any(|e| matches!(w.kind(e), Some(Kind::Chest(ch)) if ch.state < ChestState::Opening))
}

/// Create the King Reaper (`from` = the King's spot after a transformation).
fn summon_reaper(w: &mut World, c: &Content, q: &Quest, d: i32, fin: bool, from: Option<Vec2>) {
    let n = q.n_players();
    let extra = if q.settings.extended && n > 2 {
        ((n - 2) / 2) as i8
    } else {
        0
    };
    let wing = q.settings.tower_id == 11;
    enemies::reaper::summon(w, c, d, fin, from, wing, extra);
}

/// Run the control program until it waits.
fn run(w: &mut World, c: &Content, q: &mut Quest) {
    let tm = w.time.time_mult;
    if q.wait > 0.0 {
        q.wait -= tm;
        return;
    }
    for _ in 0..512 {
        let Some(&op) = q.prog.get(q.pc) else { return };
        match op {
            Op::Wait(n) => {
                q.pc += 1;
                q.wait = n;
                return;
            }
            Op::WaitConfirm => {
                if !confirm_pressed(w, q) {
                    return;
                }
                q.pc += 1;
            }
            Op::StartRound => {
                q.pc += 1;
                q.started = true;
                q.phase = QuestPhase::Playing;
                q.phase_t = 0.0;
                player::unfreeze_all(w, c);
                crate::entities::round_started(w, c);
            }
            Op::WaveStart(i) => {
                q.pc += 1;
                q.between_waves = false;
                q.current_wave = i as i32;
                if !q.ending {
                    for p in q.active_slots() {
                        if w.player_by_index(p as u8)
                            .is_none_or(|id| w.player(id).is_none_or(|pl| pl.dead))
                        {
                            crate::revive::set_auto(w, p as u8);
                        }
                    }
                }
            }
            Op::WaveHud => {
                q.pc += 1;
                q.wait = 12.0 * q.total_waves as f32 + 103.0;
                return;
            }
            Op::WaveEffects(i) => {
                q.pc += 1;
                let wave = &q.waves(c)[i as usize];
                let (dark, slow, scroll) = (wave.dark, wave.slow, wave.scroll);
                let floors = wave.floors.clone();
                floor::start_groups(w, &floors);
                // `StartDark` / `StartSlow` / `StartScroll` (quest.md §4).
                if dark {
                    crate::pickups::orbs::start_dark(w);
                }
                if slow {
                    w.orb.start_slow();
                    q.wave_slow = true;
                }
                if scroll {
                    crate::pickups::orbs::start_scroll(w);
                }
                crate::entities::king_get_mad(w);
            }
            Op::Group(wi, gi) => {
                q.pc += 1;
                let ops = expand_group(w, c, q, wi as usize, gi as usize);
                let at = q.pc;
                q.prog.splice(at..at, ops);
            }
            Op::PortalSet(pid, open) => {
                q.pc += 1;
                let changed = if open {
                    portal::appear(w, pid)
                } else {
                    portal::force_disappear(w, pid)
                };
                if changed {
                    q.wait = 5.0;
                    return;
                }
            }
            Op::SpawnOn(pid, ty) => {
                q.pc += 1;
                portal::spawn_enemy(w, pid, ty);
            }
            Op::Chest(at, big, contents) => {
                q.pc += 1;
                spawn_chest(w, at, big, contents);
            }
            Op::ChestCond(at, which, b) => {
                q.pc += 1;
                let item = resolve_conditional(w, q, which, b);
                spawn_chest(w, at, false, [item, None, None]);
            }
            Op::WaitChestsOpened => {
                if !chests_opened(w) {
                    return;
                }
                q.pc += 1;
            }
            // quest.md §4.4 step 4 / §6: an arena with the King NPC sends
            // its first Reaper through the King's transformation (wait(60),
            // the transform animation up to frame 16, then the Reaper, all
            // inside a wait(360)); otherwise the Reaper appears at once.
            Op::Reaper(d, fin) => {
                q.pc += 1;
                let king = c.level(q.settings.level).markers.king_intro;
                if king.is_some() && !q.king_transformed {
                    q.king_transformed = true;
                    w.sound("pu_darkOrbCollect", 160.0);
                    let t = crate::entities::scenery::KING_TRANSFORM_REAPER;
                    let ops = [
                        Op::Wait(60.0),
                        Op::KingTransform,
                        Op::Wait(t),
                        Op::ReaperFromKing(d, fin),
                        Op::Wait(360.0 - 60.0 - t),
                    ];
                    let pc = q.pc;
                    q.prog.splice(pc..pc, ops);
                    return;
                }
                summon_reaper(w, c, q, d, fin, None);
                q.wait = 60.0;
                return;
            }
            Op::KingTransform => {
                q.pc += 1;
                crate::entities::king_start_transform(w);
                w.sound("boss_headOff", 160.0);
            }
            Op::ReaperFromKing(d, fin) => {
                q.pc += 1;
                let from = c.level(q.settings.level).markers.king_intro;
                summon_reaper(w, c, q, d, fin, from);
            }
            Op::Jester => {
                // `wait(30)`, intro sound, `wait(160)`, spawn, `wait(60)`.
                q.pc += 1;
                let ops = [
                    Op::Wait(30.0),
                    Op::JesterSound,
                    Op::Wait(160.0),
                    Op::JesterSpawn,
                    Op::Wait(60.0),
                ];
                let pc = q.pc;
                q.prog.splice(pc..pc, ops);
            }
            Op::JesterSound => {
                q.pc += 1;
                w.sound("sfx_cyanIntro", 160.0);
            }
            Op::FloorsDieOut => {
                q.pc += 1;
                floor::die_out_all(w);
            }
            Op::JesterSpawn => {
                q.pc += 1;
                let top = portal::portals(w, c)
                    .into_iter()
                    .find(|(_, n)| n == "TopMid")
                    .map(|(id, _)| w.core(id).pos);
                let at = top.unwrap_or(Vec2::new(160.0, 40.0));
                let jester = EnemyType::from_name("Jester").unwrap();
                enemies::spawn(w, c, jester, at + jester.spawn_offset(), -1, &[]);
            }
            Op::ClosePortals => {
                q.pc += 1;
                let ids: Vec<EntityId> = q.portals.clone();
                let mut ops = vec![];
                for id in ids {
                    if portal::is_open(w, id) {
                        ops.push(Op::PortalSet(id, false));
                    }
                }
                let pc = q.pc;
                q.prog.splice(pc..pc, ops);
            }
            Op::WaitNoEnemies => {
                if enemies::enemy_count(w) > 0 {
                    return;
                }
                q.pc += 1;
            }
            Op::WaveEnd(i) => {
                q.pc += 1;
                q.between_waves = true;
                let wave = &q.waves(c)[i as usize];
                let (dark, slow, scroll) = (wave.dark, wave.slow, wave.scroll);
                if dark {
                    crate::pickups::orbs::end_dark(w);
                }
                if slow {
                    w.orb.end_slow(&mut w.time);
                    q.wave_slow = false;
                }
                if scroll {
                    crate::pickups::orbs::end_scroll(w, c);
                }
                // Effects ended: `wait(30)` before the floors die out.
                if dark || slow || scroll {
                    let pc = q.pc;
                    q.prog.splice(pc..pc, [Op::FloorsDieOut, Op::Wait(60.0)]);
                    q.wait = 31.0;
                } else {
                    floor::die_out_all(w);
                    q.wait = 60.0;
                }
                return;
            }
            Op::GGroupStart(i) => {
                q.pc += 1;
                let list = q.gauntlet_lists[i as usize].clone();
                let names = q.gauntlet_names[i as usize].clone();
                let (max, dark) = match &q.gsteps(c)[i as usize] {
                    script::GStep::Group { max, dark, .. } => (*max, *dark),
                    _ => (2, false),
                };
                if dark {
                    crate::pickups::orbs::start_dark(w);
                }
                let max = (max as f32 * q.scale()).ceil() as i32;
                let mut ops = vec![Op::Wait(30.0)];
                for (k, ty) in list.iter().enumerate() {
                    if names.is_empty() {
                        break;
                    }
                    let pid = q.portals[names[k % names.len()]];
                    ops.extend([
                        Op::GSpawn(pid, *ty),
                        Op::Wait(30.0),
                        Op::WaitTotalBelow(max),
                    ]);
                }
                ops.extend([Op::WaitTotalZero, Op::Wait(60.0)]);
                if dark {
                    ops.push(Op::GDarkEnd);
                }
                let pc = q.pc;
                q.prog.splice(pc..pc, ops);
            }
            Op::GSpawn(pid, ty) => {
                q.pc += 1;
                portal::appear_and_spawn(w, pid, ty);
            }
            Op::WaitTotalBelow(m) => {
                if (enemies::enemy_count(w) + portal::queued_total(w)) as i32 >= m {
                    return;
                }
                q.pc += 1;
            }
            Op::WaitTotalZero => {
                if enemies::enemy_count(w) + portal::queued_total(w) > 0 {
                    return;
                }
                q.pc += 1;
            }
            Op::GDarkEnd => {
                q.pc += 1;
                crate::pickups::orbs::end_dark(w);
            }
            Op::LevelEnding => {
                q.pc += 1;
                if let Some(g) = q.gauntlet.as_mut() {
                    *g = 0;
                }
                q.ending = true;
                q.phase = QuestPhase::Ending;
                w.orb.cancel_slow_mo(&mut w.time);
            }
            Op::Complete => {
                q.pc += 1;
                q.phase = QuestPhase::Complete;
                q.phase_t = 0.0;
                let total: i32 = q.active_slots().iter().map(|&p| q.lives[p]).sum();
                q.no_deaths = total
                    >= if q.n_players() == 1 {
                        5
                    } else {
                        4 * q.n_players() as i32
                    };
            }
        }
    }
}

// ------------------------------------------------------------ hooks

/// Unfrozen tick, before the entities: clock, control program, combos,
/// revive and steal timers.
pub fn session_update(w: &mut World, c: &Content) {
    let Some(mut q) = w.quest.take() else { return };
    let tm = w.time.time_mult;
    if q.started && !q.ending && q.phase == QuestPhase::Playing {
        q.time += 1;
    }
    if !matches!(
        q.phase,
        QuestPhase::GameOver | QuestPhase::Complete | QuestPhase::Finished
    ) {
        run(w, c, &mut q);
    }
    for p in q.active_slots() {
        // Combo decay.
        if q.combo[p] > 0 {
            if q.combo_timer[p] > 0.0 {
                q.combo_timer[p] -= tm;
            } else {
                let grounded = w
                    .player_by_index(p as u8)
                    .and_then(|id| w.player(id))
                    .is_none_or(|pl| pl.on_ground || pl.state() == PState::LedgeGrab || pl.dead);
                if grounded {
                    q.combo[p] = 0;
                }
            }
        }
        if q.auto_revive[p] > 0.0 {
            q.auto_revive[p] -= tm;
            if q.auto_revive[p] <= 0.0 {
                q.auto_revive[p] = -1.0;
                crate::revive::set_auto(w, p as u8);
            }
        }
        if q.steal_timer[p] > 0.0 {
            q.steal_timer[p] -= tm;
            if q.steal_timer[p] <= 0.0 {
                q.steal_timer[p] = -1.0;
                q.can_steal[p] = true;
            }
        }
        // Only a dead archer steals; a revived one loses the offer.
        let alive = w
            .player_by_index(p as u8)
            .is_some_and(|id| w.player(id).is_some_and(|pl| !pl.dead));
        if alive {
            q.can_steal[p] = false;
            q.steal_timer[p] = -1.0;
        }
        if q.can_steal[p] && !q.ending {
            let s = w.inputs[p].state;
            if (s.jump_pressed || s.alt_shoot_pressed)
                && let Some(donor) = q.active_slots().into_iter().find(|&d| q.lives[d] > 0)
            {
                q.lives[donor] -= 1;
                q.can_steal[p] = false;
                crate::revive::set_auto(w, p as u8);
            }
        }
    }
    w.quest = Some(q);
}

/// Between ticks: result-screen timelines (game over / complete).
pub fn end_of_tick(w: &mut World, c: &Content) {
    let Some(mut q) = w.quest.take() else { return };
    let _ = c;
    match q.phase {
        QuestPhase::GameOver => {
            q.phase_t += 1.0;
            // wait(60) + 90 title + 60 + 60 + 30 + 10 + 5 + 20 + 20 + 30 + 60 status (+60 tip) + 30.
            let ready_at =
                60.0 + 90.0 + 61.0 + 61.0 + 31.0 + 10.0 + 6.0 + 21.0 + 21.0 + 31.0 + 60.0 + 31.0;
            q.ready = q.phase_t >= ready_at;
            if q.ready && confirm_pressed(w, &q) {
                q.phase = QuestPhase::Finished;
            }
        }
        QuestPhase::Complete => {
            q.phase_t += 1.0;
            if q.phase_t >= 129.0 && !w.frozen {
                w.set_frozen(true);
            }
            let lives: i32 = q.active_slots().iter().map(|&p| q.lives[p].max(0)).sum();
            let ready_at =
                129.0 + 21.0 + 61.0 + 61.0 + 90.0 + 151.0 + 22.0 * lives as f32 + 31.0 + 181.0;
            q.ready = q.phase_t >= ready_at;
            if q.ready && confirm_pressed(w, &q) {
                q.phase = QuestPhase::Finished;
            }
        }
        QuestPhase::Intro | QuestPhase::Playing | QuestPhase::Ending => q.phase_t += 1.0,
        QuestPhase::Finished => {}
    }
    w.quest = Some(q);
}

/// `RegisterEnemyKill` (quest.md §5.2).
pub fn register_kill(w: &mut World, pos: Vec2, killer: i8, bounty: u16) {
    let Some(q) = w.quest.as_mut() else { return };
    if let Some(g) = q.gauntlet.as_mut() {
        *g = (*g - 1).max(0);
    }
    if bounty > 0 && killer >= 0 && q.active_slots().contains(&(killer as usize)) {
        let k = killer as usize;
        q.combo[k] += 1;
        q.combo_timer[k] = 120.0;
        q.kills[k] += 1;
        q.best_combo[k] = q.best_combo[k].max(q.combo[k]);
        let n = q.combo[k].min(6);
        w.sound_indexed("sfx_enemyKill", n as u8 - 1, pos.x);
    }
}

/// `CoOpCheckForAllDead`.
fn all_dead(w: &World, q: &Quest) -> bool {
    let slots = q.active_slots();
    if slots
        .iter()
        .any(|&p| q.lives[p] > 0 || q.auto_revive[p] > 0.0)
    {
        return false;
    }
    if crate::revive::any_pending(w) {
        return false;
    }
    !w.players().any(|id| w.player(id).is_some_and(|p| !p.dead))
}

/// `QuestRoundLogic.OnPlayerDeath` (quest.md §5.3).
pub fn on_player_death(
    w: &mut World,
    _c: &Content,
    victim: u8,
    _killer: i8,
    _cause: DeathCause,
    _at: Vec2,
) {
    let Some(mut q) = w.quest.take() else { return };
    let v = victim as usize;
    q.deaths[v] += 1;
    if !q.ending {
        if q.lives[v] > 0 {
            q.lives[v] -= 1;
            q.no_deaths = false;
            q.auto_revive[v] = 60.0;
        } else if all_dead(w, &q) {
            game_over(w, &mut q);
        } else {
            q.steal_timer[v] = 90.0;
        }
    }
    w.quest = Some(q);
}

fn game_over(w: &mut World, q: &mut Quest) {
    w.orb.do_slow_mo_kill(&mut w.time);
    q.ending = true;
    q.phase = QuestPhase::GameOver;
    q.phase_t = 0.0;
    w.sound("sfx_finalKill", 160.0);
}

/// A revive could not place the archer: re-check for game over (no
/// soft-lock when everyone is dead).
pub fn check_game_over(w: &mut World) {
    let Some(mut q) = w.quest.take() else { return };
    if !q.ending && all_dead(w, &q) {
        game_over(w, &mut q);
    }
    w.quest = Some(q);
}
