//! The Versus match inside the sim (docs/spec/versus.md): settings, level
//! order, spawning, round phases, scoring for Last Man Standing,
//! Headhunters and Team Deathmatch, crowns, miasma, final-kill slow motion,
//! the docs/net.md leave rule, match/round statistics and awards.
//!
//! The match lives in [`World::versus`]; `World::step` calls
//! [`session_update`] (unfrozen ticks, before `OrbLogic`, like
//! `Session.OnUpdate`) and [`end_of_tick`] (every tick, after the entities:
//! intro, results and next-round transitions), and `Player::Die` calls
//! [`on_player_death`]. Everything the HUD/results screens need is in
//! [`HudView`] (via `View::hud`).

pub mod draft;
pub mod levels;
pub mod miasma;
pub mod settings;
pub mod spawn;
pub mod stats;

use crate::checksum::{Hasher, StableHash};
use crate::content::Content;
use crate::entity::EntityId;
use crate::math::Vec2;
use crate::player::{self, DeathCause};
use crate::quiver::ArrowType;
use crate::rng::Rng;
use crate::variants::Variant;
use crate::world::{MAX_PLAYERS, MatchSetup, World};
pub use levels::{LevelSystem, SpawnTier};
pub use settings::{MatchLength, MatchSettings, Mode, SlotSettings};
pub use stats::{DeathType, KillStats, MatchStats};

/// Round-end countdown (`RoundEndCounter`).
pub const END_COUNTER: f32 = 90.0;
/// Intro proper: level shown -> `StartRound`.
pub const INTRO_TICKS: f32 = 160.0;
/// Tower card: Confirm is accepted from this tick; it then slides out.
pub const CARD_CONFIRM_AT: f32 = 70.0;
pub const CARD_OUT_TICKS: f32 = 30.0;

/// The match phase (versus.md §4.2, §17.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Round 0 only: the tower card, waiting for a Confirm.
    TowerCard,
    /// "ROUND n" / "FIGHT!!": archers frozen and flashing.
    Intro,
    /// The round runs (including the end countdown when `ending`).
    Playing,
    /// Round results over the frozen level.
    Results,
    /// Match results (a winner exists, or too few sides are left).
    MatchResults,
    /// Results acknowledged; the post-match menu is outside the sim.
    Finished,
}

/// Round-results events, in the order they happened (versus.md §6.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreEvent {
    Gain(u8),
    Lose(u8),
    /// `TeamHasCrown` of every score index after the round.
    Crown([bool; MAX_PLAYERS]),
}

/// Match-flow notifications for the presentation (music, banners, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersusEvent {
    RoundIntro {
        round: u32,
        overtime: bool,
    },
    RoundStart,
    MiasmaSpawned,
    MiasmaDissipated,
    PointGained {
        index: u8,
    },
    PointLost {
        index: u8,
    },
    FinalKill {
        victor: u8,
    },
    FinalKillCancelled,
    RoundEnded,
    /// Round results shown; `decided` = the match results follow.
    Results {
        decided: bool,
    },
    MatchResults {
        winner: i8,
    },
    MatchFinished,
}

/// Per-round state (`RoundLogic`).
#[derive(Clone, Debug, PartialEq)]
pub struct RoundState {
    pub kills: [u8; MAX_PLAYERS],
    pub events: Vec<ScoreEvent>,
    /// Seconds since `StartRound` (not slowed by slow motion).
    pub time: f32,
    pub started: bool,
    pub overtime: bool,
    pub miasma_counter: f32,
    pub miasma: Option<[EntityId; 2]>,
    pub ending: bool,
    pub done: bool,
    pub was_final_kill: bool,
    pub end_counter: f32,
    /// `ghostWaitCounter`: up to 90 extra ticks while a player ghost and a
    /// living archer coexist.
    pub ghost_wait: f32,
    /// Spawned this round and not dead yet.
    pub alive: [bool; MAX_PLAYERS],
    pub spawned: [bool; MAX_PLAYERS],
    pub players_at_start: u8,
    pub random_arrow_type: ArrowType,
    end_pending: bool,
}

impl RoundState {
    fn new(set: &MatchSettings, overtime: bool) -> RoundState {
        let miasma_counter = if set.variants.any(Variant::SuddenDeath) {
            60.0
        } else if set.variants.any(Variant::NoTimeLimit) {
            0.0
        } else {
            1500.0
        };
        RoundState {
            kills: [0; MAX_PLAYERS],
            events: vec![],
            time: 0.0,
            started: false,
            overtime,
            miasma_counter,
            miasma: None,
            ending: false,
            done: false,
            was_final_kill: false,
            end_counter: END_COUNTER,
            ghost_wait: END_COUNTER,
            alive: [false; MAX_PLAYERS],
            spawned: [false; MAX_PLAYERS],
            players_at_start: 0,
            random_arrow_type: ArrowType::Normal,
            end_pending: false,
        }
    }
}

/// The whole match (`Session` + `RoundLogic` + `VersusLevelSystem`).
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub settings: MatchSettings,
    /// Match RNG (level order, spawns, awards); gameplay uses `World::rng`,
    /// re-seeded per round from `(seed, round)`.
    pub rng: Rng,
    pub scores: [i32; MAX_PLAYERS],
    pub old_scores: [i32; MAX_PLAYERS],
    pub round_index: u32,
    /// Slot still in the match (false after the leave rule removed it).
    pub active: [bool; MAX_PLAYERS],
    pub stats: [MatchStats; MAX_PLAYERS],
    pub levels: LevelSystem,
    /// Team order along the screen for 3+ teams (rotated every round).
    pub team_order: Vec<u8>,
    pub round: RoundState,
    pub phase: Phase,
    pub phase_t: f32,
    card_confirmed: Option<f32>,
    pub results_ready_at: f32,
    /// The round results lead to the match results.
    pub decided: bool,
    pub winner: i32,
    pub awards: Vec<Vec<u8>>,
    /// Treasure Draft of the current round's intro.
    pub draft: Option<draft::Draft>,
    /// Base `CharacterIndex` per slot (alt skins count as their base).
    pub archer_base: [i32; MAX_PLAYERS],
}

fn round_seed(seed: i32, round: u32) -> i32 {
    seed.wrapping_mul(31).wrapping_add(round as i32 * 7919 + 17)
}

impl Match {
    fn new(settings: MatchSettings) -> Match {
        let mut rng = Rng::new(settings.seed);
        let mut team_order: Vec<u8> = (0..settings.teams).collect();
        rng.shuffle(&mut team_order);
        let mut active = [false; MAX_PLAYERS];
        for (p, a) in active.iter_mut().enumerate() {
            *a = settings.joined(p);
        }
        let round = RoundState::new(&settings, false);
        Match {
            settings,
            rng,
            scores: [0; MAX_PLAYERS],
            old_scores: [0; MAX_PLAYERS],
            round_index: 0,
            active,
            stats: [MatchStats::default(); MAX_PLAYERS],
            levels: LevelSystem::default(),
            team_order,
            round,
            phase: Phase::Intro,
            phase_t: 0.0,
            card_confirmed: None,
            results_ready_at: 0.0,
            decided: false,
            winner: -1,
            awards: vec![],
            draft: None,
            archer_base: [-1; MAX_PLAYERS],
        }
    }

    /// Score indices that take part (FFA: joined slots; teams: 0..T).
    fn participants(&self) -> Vec<usize> {
        if self.settings.team_mode() {
            (0..self.settings.teams as usize).collect()
        } else {
            (0..MAX_PLAYERS)
                .filter(|&p| self.settings.joined(p))
                .collect()
        }
    }

    /// Participants that can still win (an active member).
    fn active_participants(&self) -> Vec<usize> {
        if self.settings.team_mode() {
            (0..self.settings.teams as usize)
                .filter(|&t| {
                    (0..MAX_PLAYERS).any(|p| self.active[p] && self.settings.score_index(p) == t)
                })
                .collect()
        } else {
            (0..MAX_PLAYERS).filter(|&p| self.active[p]).collect()
        }
    }

    pub fn goal(&self) -> i32 {
        self.settings.goal_score()
    }

    /// `GetHighestScore`.
    pub fn highest(&self) -> i32 {
        self.scores[..self.settings.score_count()]
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
            .max(0)
    }

    /// `GetScoreLead(idx)`.
    pub fn score_lead(&self, idx: usize) -> i32 {
        let others = (0..self.settings.score_count())
            .filter(|&i| i != idx)
            .map(|i| self.scores[i])
            .max()
            .unwrap_or(0);
        self.scores[idx] - others.max(0)
    }

    /// `TeamHasCrown(idx)` on `scores`.
    pub fn team_has_crown(&self, idx: usize, scores: &[i32; MAX_PLAYERS]) -> bool {
        let parts = self.participants();
        !parts.iter().any(|&i| scores[i] > scores[idx])
            && parts.iter().any(|&i| scores[i] < scores[idx])
    }

    fn crown_flags(&self, scores: &[i32; MAX_PLAYERS]) -> [bool; MAX_PLAYERS] {
        let mut f = [false; MAX_PLAYERS];
        for i in self.participants() {
            f[i] = self.team_has_crown(i, scores);
        }
        f
    }

    /// `GetWinner()`: the unique highest score >= goal among sides still in
    /// the match, else -1.
    pub fn get_winner(&self) -> i32 {
        let parts = self.active_participants();
        let Some(best) = parts.iter().map(|&i| self.scores[i]).max() else {
            return -1;
        };
        let n = parts.iter().filter(|&&i| self.scores[i] == best).count();
        if n == 1 && best >= self.goal() {
            parts.into_iter().find(|&i| self.scores[i] == best).unwrap() as i32
        } else {
            -1
        }
    }

    /// The round ended and the results are about to freeze the level.
    pub fn round_end_pending(&self) -> bool {
        self.round.end_pending || self.round.done
    }

    /// `IsInOvertime` (Headhunters).
    pub fn is_in_overtime(&self) -> bool {
        self.settings.mode == Mode::Headhunters
            && self.scores[..self.settings.score_count()]
                .iter()
                .any(|&s| s >= self.goal())
    }

    /// `ShouldSpawn(p)`.
    pub fn should_spawn(&self, p: usize) -> bool {
        self.active[p]
            && (!self.round.overtime || self.score_lead(self.settings.score_index(p)) == 0)
    }

    fn was_winning_at_start(&self, p: usize) -> bool {
        let idx = self.settings.score_index(p);
        let parts = self.participants();
        let s = self.old_scores[idx];
        !parts.iter().any(|&i| i != idx && self.old_scores[i] > s)
            && parts.iter().any(|&i| i != idx && self.old_scores[i] < s)
    }

    /// Alive and still connected (inert archers never block a round end).
    fn living(&self, w: &World) -> Vec<usize> {
        (0..MAX_PLAYERS)
            .filter(|&p| self.round.alive[p] && w.connected[p])
            .collect()
    }

    /// `AddScore(index, delta)`.
    fn add_score(&mut self, w: &mut World, idx: usize, delta: i32) {
        let old = self.scores[idx];
        let new = (old + delta).max(0);
        self.scores[idx] = new;
        if new > old {
            self.round.events.push(ScoreEvent::Gain(idx as u8));
            w.emit(crate::Event::Versus(VersusEvent::PointGained {
                index: idx as u8,
            }));
        } else if new < old {
            self.round.events.push(ScoreEvent::Lose(idx as u8));
            w.emit(crate::Event::Versus(VersusEvent::PointLost {
                index: idx as u8,
            }));
        }
    }

    fn final_kill(&mut self, w: &mut World, victor_idx: usize) {
        for p in 0..MAX_PLAYERS {
            if self.settings.joined(p) && self.settings.score_index(p) == victor_idx {
                self.stats[p].got_win = true;
            }
        }
        w.orb.do_slow_mo_kill(&mut w.time);
        w.emit(crate::Event::Versus(VersusEvent::FinalKill {
            victor: victor_idx as u8,
        }));
    }

    fn cancel_final_kill(&mut self, w: &mut World) {
        w.emit(crate::Event::Versus(VersusEvent::FinalKillCancelled));
    }

    /// Alive teams (score indices) among living connected archers.
    fn teams_alive(&self, w: &World) -> Vec<usize> {
        let mut t: Vec<usize> = self
            .living(w)
            .iter()
            .map(|&p| self.settings.score_index(p))
            .collect();
        t.sort_unstable();
        t.dedup();
        t
    }

    /// `OtherPlayerCouldWin(w)` (Headhunters).
    fn other_player_could_win(&self, w: &World, win: usize) -> bool {
        if self.scores[win] < self.goal() || self.score_lead(win) <= 0 {
            return true;
        }
        let living = self.living(w);
        let threshold = self.highest() - (living.len() as i32 - 1);
        living
            .iter()
            .any(|&i| i != win && self.scores[i] >= threshold)
    }

    /// The results timeline length (versus.md §10.3): events start at 40,
    /// prompts 10 ticks after the last.
    fn results_duration(&self) -> f32 {
        let mut t = 40.0;
        let mut shown = self.crown_flags(&self.old_scores);
        for e in &self.round.events {
            match e {
                ScoreEvent::Gain(_) | ScoreEvent::Lose(_) => t += 20.0,
                ScoreEvent::Crown(f) => {
                    let changed = (0..MAX_PLAYERS).filter(|&i| f[i] != shown[i]).count();
                    if changed > 0 {
                        t += 10.0 * changed as f32 + 30.0;
                    }
                    shown = *f;
                }
            }
        }
        t + 10.0
    }

    /// HUD/results snapshot.
    pub fn hud(&self, w: &World) -> HudView {
        let mut slots = [None; MAX_PLAYERS];
        for (p, s) in slots.iter_mut().enumerate() {
            if let Some(ss) = self.settings.slots[p] {
                *s = Some(SlotHud {
                    archer: ss.archer,
                    team: ss.team,
                    score_index: self.settings.score_index(p) as u8,
                    active: self.active[p],
                    connected: w.connected[p],
                    alive: self.round.alive[p],
                    spawned: self.round.spawned[p],
                });
            }
        }
        HudView {
            draft: self.draft.as_ref().map(|d| d.view()),
            phase: self.phase,
            phase_t: self.phase_t,
            card_confirmed: self.card_confirmed,
            ready: matches!(self.phase, Phase::Results | Phase::MatchResults)
                && self.phase_t >= self.results_ready_at,
            mode: self.settings.mode,
            round_index: self.round_index,
            overtime: self.round.overtime,
            goal: self.goal(),
            score_count: self.settings.score_count(),
            teams: if self.settings.team_mode() {
                self.settings.teams
            } else {
                0
            },
            scores: self.scores,
            old_scores: self.old_scores,
            crowns: self.crown_flags(&self.scores),
            had_crowns: self.crown_flags(&self.old_scores),
            events: self.round.events.clone(),
            decided: self.decided,
            winner: self.winner,
            slots,
            round_time: self.round.time,
            ending: self.round.ending,
            awards: self.awards.clone(),
            stats: Box::new(self.stats),
        }
    }
}

impl StableHash for Match {
    fn stable_hash(&self, h: &mut Hasher) {
        self.settings.stable_hash(h);
        self.rng.hash_into(h);
        self.scores.stable_hash(h);
        self.old_scores.stable_hash(h);
        h.u32(self.round_index);
        self.active.stable_hash(h);
        for s in &self.stats {
            s.stable_hash(h);
        }
        self.levels.queue.iter().for_each(|&l| h.u32(l as u32));
        self.draft.stable_hash(h);
        h.u8(self.phase as u8);
        h.f32(self.phase_t);
        self.card_confirmed.stable_hash(h);
        h.bool(self.decided);
        h.i32(self.winner);
        let r = &self.round;
        r.kills.stable_hash(h);
        h.u32(r.events.len() as u32);
        h.f32(r.time);
        h.bool(r.started);
        h.bool(r.overtime);
        h.f32(r.miasma_counter);
        h.bool(r.ending);
        h.bool(r.done);
        h.bool(r.was_final_kill);
        h.f32(r.end_counter);
        h.f32(r.ghost_wait);
        r.alive.stable_hash(h);
    }
}

/// One slot as the HUD sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotHud {
    pub archer: u16,
    pub team: u8,
    pub score_index: u8,
    /// Still in the match (false: removed by the leave rule; show greyed).
    pub active: bool,
    pub connected: bool,
    pub alive: bool,
    pub spawned: bool,
}

/// Everything HUD, round results and match results screens need.
#[derive(Clone, Debug, PartialEq)]
pub struct HudView {
    pub phase: Phase,
    /// Ticks since the phase started (drives intro/results animations).
    pub phase_t: f32,
    /// Tower card: the `phase_t` at which a player confirmed it (the card
    /// slides out from there).
    pub card_confirmed: Option<f32>,
    /// Results: "Continue" is accepted now.
    pub ready: bool,
    pub mode: Mode,
    pub round_index: u32,
    pub overtime: bool,
    pub goal: i32,
    /// Rows: 12 (FFA, by slot) or `teams`.
    pub score_count: usize,
    pub teams: u8,
    pub scores: [i32; MAX_PLAYERS],
    /// Scores at the start of the round (the results animate from these).
    pub old_scores: [i32; MAX_PLAYERS],
    /// Crowned at the start of the next round.
    pub crowns: [bool; MAX_PLAYERS],
    pub had_crowns: [bool; MAX_PLAYERS],
    pub events: Vec<ScoreEvent>,
    pub decided: bool,
    pub winner: i32,
    pub slots: [Option<SlotHud>; MAX_PLAYERS],
    pub round_time: f32,
    pub ending: bool,
    /// Award ids per slot (`stats::AWARD_NAMES`), filled at match end.
    pub awards: Vec<Vec<u8>>,
    pub stats: Box<[MatchStats; MAX_PLAYERS]>,
    /// The Treasure Draft of this round's intro.
    pub draft: Option<draft::DraftView>,
}

// ------------------------------------------------------------ entry points

/// Create the world for a Versus match: first level, spawns, intro.
pub fn new_match(c: &Content, settings: MatchSettings) -> World {
    let mut m = Match::new(settings);
    for p in 0..MAX_PLAYERS {
        if let Some(sl) = m.settings.slots[p] {
            m.archer_base[p] = c
                .archers
                .get(sl.archer as usize)
                .map_or(sl.archer as i32, |a| a.base as i32);
        }
    }
    let n = m.active.iter().filter(|&&a| a).count();
    let first = m.levels.next(c, &m.settings, n, &mut m.rng);
    let setup = MatchSetup {
        level: first,
        seed: round_seed(m.settings.seed, 0),
        start_frozen: true,
        variant_set: m.settings.variants,
        items: m.settings.items,
        ..MatchSetup::default()
    };
    let mut w = World::new(c, &setup);
    begin_round(&mut w, c, &mut m);
    w.versus = Some(Box::new(m));
    w
}

fn begin_round(w: &mut World, c: &Content, m: &mut Match) {
    m.round = RoundState::new(&m.settings, m.is_in_overtime());
    // `LevelLoadStart`: the round's random arrow type honours the
    // treasure exclusions and unlocks (pickups.md §7.7).
    m.round.random_arrow_type = crate::treasure::get_random_arrow_type(w, true);
    spawn::spawn_round(w, c, m);
    m.draft = draft::start(w, m);
    if m.settings.variants.any(Variant::OffsetWorld) {
        crate::pickups::orbs::do_offset_world(w, c);
    }
    m.phase = if m.round_index == 0 && m.settings.tower_card {
        Phase::TowerCard
    } else {
        Phase::Intro
    };
    m.phase_t = 0.0;
    m.card_confirmed = None;
    w.emit(crate::Event::Versus(VersusEvent::RoundIntro {
        round: m.round_index,
        overtime: m.round.overtime,
    }));
}

/// `Session.StartRound`.
fn start_round(w: &mut World, c: &Content, m: &mut Match) {
    m.round.started = true;
    m.phase = Phase::Playing;
    m.phase_t = 0.0;
    player::unfreeze_all(w, c);
    crate::entities::round_started(w, c);
    crate::entities::king_get_mad(w);
    // Treasure chests and the AlwaysDark / AlwaysLava variants.
    let n = m.settings.player_amount();
    crate::treasure::round_start(w, c, n, m.round.overtime, true);
    if m.settings.variants.any(Variant::SlowTime) {
        w.orb.do_time_orb(true, false, &mut w.rng);
    }
    if m.settings.variants.any(Variant::AlwaysScrolling) {
        crate::pickups::orbs::start_scroll(w);
    }
    w.emit(crate::Event::Versus(VersusEvent::RoundStart));
}

/// `GotoNextRound`: next level, spawns, intro.
fn next_round(w: &mut World, c: &Content, m: &mut Match) {
    m.old_scores = m.scores;
    m.round_index += 1;
    let n = m.active.iter().filter(|&&a| a).count();
    let level = m.levels.next(c, &m.settings, n, &mut m.rng);
    w.load_scene(c, level);
    w.rng = Rng::new(round_seed(m.settings.seed, m.round_index));
    begin_round(w, c, m);
}

fn confirm_pressed(w: &World) -> bool {
    (0..MAX_PLAYERS).any(|p| {
        w.connected[p] && (w.inputs[p].state.jump_pressed || w.inputs[p].state.start_pressed)
    })
}

/// `Session.OnUpdate` (unfrozen ticks, before `OrbLogic` and the entities).
pub fn session_update(w: &mut World, _c: &Content) {
    let Some(mut m) = w.versus.take() else { return };
    if m.phase == Phase::Playing && m.round.started && !m.round.done {
        m.round.time += crate::time::FRAME_SECONDS;
        if m.round.miasma_counter > 0.0 {
            m.round.miasma_counter -= w.time.time_mult;
            if m.round.miasma_counter <= 0.0 {
                m.round.miasma = Some(miasma::spawn(w));
                w.emit(crate::Event::Versus(VersusEvent::MiasmaSpawned));
            }
        }
        // Leave rule: departed archers never hold a round open.
        if !m.round.ending && (0..MAX_PLAYERS).any(|p| m.round.alive[p] && !w.connected[p]) {
            let over = if m.settings.team_mode() {
                m.teams_alive(w).len() <= 1
            } else {
                m.living(w).len() <= 1
            };
            if over {
                m.round.ending = true;
            }
        }
        if m.round.ending {
            let ghosts = !w.tagged(crate::entity::Tag::PlayerGhost).is_empty();
            if m.round.ghost_wait > 0.0 && ghosts && !m.living(w).is_empty() {
                m.round.ghost_wait -= w.time.time_mult;
            } else {
                m.round.end_counter -= w.time.time_mult;
            }
            if m.round.end_counter <= 0.0 {
                m.round.done = true;
                let living = m.living(w);
                match m.settings.mode {
                    Mode::LastManStanding => {
                        if living.len() == 1 {
                            m.add_score(w, living[0], 1);
                        }
                    }
                    Mode::TeamDeathmatch => {
                        if let Some(&p) = living.first() {
                            let t = m.settings.score_index(p);
                            m.add_score(w, t, 1);
                        }
                    }
                    Mode::Headhunters => {}
                }
                let flags = m.crown_flags(&m.scores);
                m.round.events.push(ScoreEvent::Crown(flags));
                end_round(w, &mut m);
            }
        }
    }
    w.versus = Some(m);
}

/// `Session.EndRound` (stats, leave-rule removals, decides the match).
fn end_round(w: &mut World, m: &mut Match) {
    let highest = m.highest();
    let goal = m.goal();
    let n = m.settings.player_amount();
    for p in 0..MAX_PLAYERS {
        if !m.settings.joined(p) {
            continue;
        }
        let idx = m.settings.score_index(p);
        let s = &mut m.stats[p];
        s.furthest_behind = s.furthest_behind.max(highest - m.scores[idx]);
        s.points_from_goal = m.scores[idx] - goal;
        let opponents = n - m.settings.player_team_size(p);
        if m.round.kills[p] as usize == opponents {
            s.round_sweeps += 1;
        }
        if m.round.kills[p] == 0 && m.round.alive[p] {
            s.survived_with_no_kills += 1;
        }
    }
    // Leave rule: departed slots are out of the match from now on.
    for p in 0..MAX_PLAYERS {
        if m.active[p] && !w.connected[p] {
            m.active[p] = false;
        }
    }
    let sides = m.active_participants().len();
    m.winner = m.get_winner();
    m.decided = m.winner != -1 || sides < 2;
    if m.decided {
        for p in 0..MAX_PLAYERS {
            if m.settings.joined(p) {
                m.stats[p].won = m.winner >= 0 && m.settings.score_index(p) == m.winner as usize;
            }
        }
        m.awards = stats::compute_awards(&m.settings, &m.stats, &m.archer_base, &mut m.rng);
    }
    m.round.end_pending = true;
    w.emit(crate::Event::Versus(VersusEvent::RoundEnded));
}

/// Between ticks: intro timeline, results, next round (frozen or not).
pub fn end_of_tick(w: &mut World, c: &Content) {
    let Some(mut m) = w.versus.take() else { return };
    match m.phase {
        Phase::TowerCard => {
            m.phase_t += 1.0;
            if m.card_confirmed.is_none() && m.phase_t >= CARD_CONFIRM_AT && confirm_pressed(w) {
                m.card_confirmed = Some(m.phase_t);
            }
            if let Some(t0) = m.card_confirmed
                && m.phase_t >= t0 + CARD_OUT_TICKS
            {
                m.phase = Phase::Intro;
                m.phase_t = 0.0;
            }
        }
        Phase::Intro => {
            // The Treasure Draft runs first; the intro waits for it.
            if let Some(d) = m.draft.as_mut().filter(|d| !d.finished()) {
                draft::update(w, c, d);
            } else {
                m.phase_t += 1.0;
                if m.phase_t >= INTRO_TICKS {
                    start_round(w, c, &mut m);
                }
            }
        }
        Phase::Playing => {
            if m.round.end_pending {
                m.round.end_pending = false;
                w.set_frozen(true);
                w.orb.cancel_slow_mo(&mut w.time);
                m.phase = Phase::Results;
                m.phase_t = 0.0;
                m.results_ready_at = m.results_duration();
                w.emit(crate::Event::Versus(VersusEvent::Results {
                    decided: m.decided,
                }));
            }
        }
        Phase::Results => {
            m.phase_t += 1.0;
            if m.phase_t >= m.results_ready_at && confirm_pressed(w) {
                if m.decided {
                    m.phase = Phase::MatchResults;
                    m.phase_t = 0.0;
                    let most = m.awards.iter().map(|a| a.len()).max().unwrap_or(1) as f32;
                    m.results_ready_at = crate::math::max(120.0, 150.0 + 40.0 * most);
                    w.emit(crate::Event::Versus(VersusEvent::MatchResults {
                        winner: m.winner as i8,
                    }));
                } else {
                    next_round(w, c, &mut m);
                }
            }
        }
        Phase::MatchResults => {
            m.phase_t += 1.0;
            if m.phase_t >= m.results_ready_at && confirm_pressed(w) {
                m.phase = Phase::Finished;
                m.phase_t = 0.0;
                w.emit(crate::Event::Versus(VersusEvent::MatchFinished));
            }
        }
        Phase::Finished => m.phase_t += 1.0,
    }
    w.versus = Some(m);
}

/// `Session.OnPlayerDeath` -> `RoundLogic.OnPlayerDeath` (versus.md §6.3, §7).
/// Called from `Player::Die` after the corpse exists.
pub fn on_player_death(
    w: &mut World,
    _c: &Content,
    victim: u8,
    killer: i8,
    cause: DeathCause,
    _at: Vec2,
) {
    let Some(mut m) = w.versus.take() else { return };
    let v = victim as usize;
    m.round.alive[v] = false;
    let living = m.living(w).len();
    // Base part.
    if let Some(ids) = m.round.miasma
        && killer != -1
        && living <= 1
    {
        miasma::dissipate(w, ids);
        w.emit(crate::Event::Versus(VersusEvent::MiasmaDissipated));
    }
    let k = killer as i32;
    if k >= 0 && k != v as i32 {
        m.round.kills[k as usize] = m.round.kills[k as usize].saturating_add(1);
        let t = m.round.time;
        let fk = &mut m.stats[k as usize].fastest_kill;
        if *fk == 0.0 || t < *fk {
            *fk = t;
        }
    }
    // The victim is already dead, so a suicide counts too (§6.3 step 5).
    if k >= 0 && !m.round.alive[k as usize] {
        m.stats[k as usize].kills_while_dead += 1;
    }
    let dt = if k == v as i32 {
        DeathType::Self_
    } else if k != -1
        && m.settings.team_mode()
        && m.settings.score_index(k as usize) == m.settings.score_index(v)
    {
        DeathType::Team
    } else {
        DeathType::Normal
    };
    let archer = |p: i32| -> i32 { if p < 0 { -1 } else { m.archer_base[p as usize] } };
    if k >= 0 {
        if dt == DeathType::Normal && m.was_winning_at_start(v) {
            m.stats[k as usize].winner_kills += 1;
        }
        let va = archer(v as i32);
        m.stats[k as usize].kills.add(dt, cause, va);
        if let Some(ids) = m.round.miasma
            && miasma::age(w, ids).is_some_and(|t| t >= 720.0)
        {
            m.stats[k as usize].kills_during_miasma += 1;
        }
    }
    let ka = archer(k);
    m.stats[v].deaths.add(dt, cause, ka);
    // Mode part.
    let goal = m.goal();
    match m.settings.mode {
        Mode::LastManStanding => {
            let living = m.living(w);
            if m.round.was_final_kill && living.is_empty() {
                m.cancel_final_kill(w);
            } else if living.len() <= 1 {
                m.round.ending = true;
                if let Some(&s) = living.first()
                    && m.scores[s] >= goal - 1
                    && !m.round.was_final_kill
                {
                    m.round.was_final_kill = true;
                    m.final_kill(w, s);
                }
            }
        }
        Mode::Headhunters => {
            if k == v as i32 || k == -1 {
                m.add_score(w, v, -1);
            } else {
                m.add_score(w, k as usize, 1);
            }
            let winner = m.get_winner();
            if m.round.was_final_kill && winner == -1 {
                m.round.was_final_kill = false;
                m.round.ending = false;
                m.round.end_counter = END_COUNTER;
                m.round.ghost_wait = END_COUNTER;
                m.cancel_final_kill(w);
            }
            if m.living(w).len() <= 1 {
                m.round.ending = true;
                if winner != -1 && !m.round.was_final_kill {
                    m.round.was_final_kill = true;
                    m.final_kill(w, winner as usize);
                }
            } else if !m.round.was_final_kill
                && winner != -1
                && !m.other_player_could_win(w, winner as usize)
            {
                m.round.ending = true;
                m.round.was_final_kill = true;
                m.final_kill(w, winner as usize);
            }
        }
        Mode::TeamDeathmatch => {
            let alive_teams = m.teams_alive(w);
            if m.round.was_final_kill && m.living(w).is_empty() {
                m.round.was_final_kill = false;
                m.cancel_final_kill(w);
            } else if alive_teams.len() <= 1 {
                m.round.ending = true;
                if let Some(&t) = alive_teams.first()
                    && m.scores[t] >= goal - 1
                {
                    m.round.was_final_kill = true;
                    m.final_kill(w, t);
                }
            }
        }
    }
    w.versus = Some(m);
}

/// A Team Revive could not place the archer (versus.md §7.3): the slot is
/// dead again and the round re-checks for a win.
pub fn revive_failed(w: &mut World, p: u8) {
    let Some(mut m) = w.versus.take() else { return };
    m.round.alive[p as usize] = false;
    if m.settings.mode == Mode::TeamDeathmatch && !m.round.ending {
        let goal = m.goal();
        let alive_teams = m.teams_alive(w);
        if alive_teams.len() <= 1 {
            m.round.ending = true;
            if let Some(&t) = alive_teams.first()
                && m.scores[t] >= goal - 1
            {
                m.round.was_final_kill = true;
                m.final_kill(w, t);
            }
        }
    }
    w.versus = Some(m);
}

/// Bump a match statistic of slot `p` (no-op outside a match).
pub fn stat(w: &mut World, p: u8, f: impl FnOnce(&mut MatchStats)) {
    if let Some(m) = w.versus.as_mut()
        && (p as usize) < MAX_PLAYERS
    {
        f(&mut m.stats[p as usize]);
    }
}
