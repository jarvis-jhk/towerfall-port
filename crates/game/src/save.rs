//! Our save data (ui.md 12.5 "port policy"): lifetime Versus statistics and
//! award counters, Quest, Trials and Dark World records, in
//! `$TFP_DATA_DIR/records.json`; plus the session statistics (not saved).
//! The original's `tf_saveData` can be imported once, read only.
//!
//! A small global API so every mode records into one store:
//! [`record_versus`], [`record_dark_world`], [`record_quest`],
//! [`record_trials`], [`with`] (read or change anything), [`snapshot`].
//! Writes go to a temporary file and are renamed over the old one; they
//! only happen when persistence is enabled ([`init`] with a path), so tests
//! and headless runs keep everything in memory.
//!
//! Times are sim ticks (1/60 s); the original's .NET ticks are converted on
//! import.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tfp_sim::versus::HudView;

pub const FILE: &str = "records.json";
const VERSION: u32 = 1;

/// Kill or death counters (the original's `Kills` / `Deaths` objects).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Counters {
    pub total: u64,
    pub team: u64,
    pub own: u64,
    pub environment: u64,
    /// Green, Blue, Pink, Orange, White, Yellow, Cyan, Purple, Red.
    pub per_archer: [u64; 9],
    /// By cause name (`Arrow`, `Explosion`, ...).
    pub by_cause: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VersusRecords {
    /// Ticks of rounds played.
    pub time_played: u64,
    pub matches: u64,
    pub rounds: u64,
    pub kills: Counters,
    pub deaths: Counters,
    /// Match wins per archer (base archer index 0..8).
    pub wins: [u64; 9],
    /// `awards[award][archer]` (70 awards, versus.md 10.5).
    pub awards: Vec<[u64; 9]>,
    pub arrows_shot: u64,
    pub arrows_collected: u64,
    pub arrows_caught: u64,
    pub treasures_taken: u64,
    pub dodges: u64,
    pub jumps: u64,
    /// Matches per versus tower (16).
    pub tower_plays: Vec<u64>,
    pub random_plays: u64,
    pub times_launched: u64,
}

impl Default for VersusRecords {
    fn default() -> Self {
        Self {
            time_played: 0,
            matches: 0,
            rounds: 0,
            kills: Counters::default(),
            deaths: Counters::default(),
            wins: [0; 9],
            awards: vec![[0; 9]; tfp_sim::versus::stats::AWARD_NAMES.len()],
            arrows_shot: 0,
            arrows_collected: 0,
            arrows_caught: 0,
            treasures_taken: 0,
            dodges: 0,
            jumps: 0,
            tower_plays: vec![0; 16],
            random_plays: 0,
            times_launched: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuestTower {
    pub revealed: bool,
    pub completed_normal: bool,
    pub completed_hardcore: bool,
    pub completed_no_deaths: bool,
    /// Best clear times (ticks, 0 = none): 1 player, 2+ players.
    pub best: [u32; 2],
    pub total_deaths: u32,
    pub total_attempts: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DarkWorldTower {
    pub revealed: bool,
    pub completed_normal: bool,
    pub completed_hardcore: bool,
    pub completed_legendary: bool,
    pub earned_eye: bool,
    pub earned_gold_eye: bool,
    /// Best Legendary clear per player count 1..4 (ticks, 0 = none).
    pub best: [u32; 4],
    /// Most curses per player count 1..4.
    pub most_curses: [u32; 4],
    pub deaths: u32,
    pub attempts: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrialsLevel {
    pub attempts: u32,
    /// Gold, diamond, dev time.
    pub unlocked: [bool; 3],
    /// Ticks, 0 = none.
    pub best: u32,
    /// The level's hidden yellow gem was found (trials.md 9).
    pub gem: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SaveData {
    pub version: u32,
    pub versus: VersusRecords,
    /// By quest tower index.
    pub quest: BTreeMap<usize, QuestTower>,
    /// By Dark World tower index.
    pub dark_world: BTreeMap<usize, DarkWorldTower>,
    /// By trials level (index of the trials content, tower-major x tier).
    pub trials: BTreeMap<usize, TrialsLevel>,
    /// Co-op crowns carried to the next session (12 slots).
    pub crowns: Vec<bool>,
    /// Where the one-time import came from (it is not repeated).
    pub imported_from: Option<String>,
}

impl Default for SaveData {
    fn default() -> Self {
        Self {
            version: VERSION,
            versus: VersusRecords::default(),
            quest: BTreeMap::new(),
            dark_world: BTreeMap::new(),
            trials: BTreeMap::new(),
            crowns: vec![false; 12],
            imported_from: None,
        }
    }
}

/// Since launch (the archives' SESSION page; not saved).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionStats {
    pub matches: u64,
    pub rounds: u64,
    pub time_played: u64,
    pub kills: [u64; 9],
    pub deaths: [u64; 9],
    pub self_kills: [u64; 9],
    pub wins: [u64; 9],
}

struct Store {
    data: SaveData,
    session: SessionStats,
    path: Option<PathBuf>,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut g = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let s = g.get_or_insert_with(|| Store {
        data: SaveData::default(),
        session: SessionStats::default(),
        path: None,
    });
    f(s)
}

/// The data directory's records file.
pub fn path() -> Option<PathBuf> {
    crate::options_store::data_dir().map(|d| d.join(FILE))
}

/// Start-up: load `path` (if it exists) and keep saving there. `None`
/// keeps everything in memory. Counts the launch.
pub fn init(path: Option<PathBuf>) -> Result<()> {
    let data = match &path {
        Some(p) if p.is_file() => load_from(p)?,
        _ => SaveData::default(),
    };
    store(|s| {
        s.data = data;
        s.data.versus.times_launched += 1;
        s.path = path;
    });
    flush()
}

pub fn load_from(p: &Path) -> Result<SaveData> {
    let text = std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", p.display()))
}

pub fn save_to(p: &Path, d: &SaveData) -> Result<()> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(d)?)
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, p).with_context(|| format!("replacing {}", p.display()))?;
    Ok(())
}

/// Writes the store (when persistent).
pub fn flush() -> Result<()> {
    let (path, data) = store(|s| (s.path.clone(), s.data.clone()));
    match path {
        Some(p) => save_to(&p, &data),
        None => Ok(()),
    }
}

fn flush_logged() {
    if let Err(e) = flush() {
        log::warn!("saving records: {e:#}");
    }
}

/// Reads or changes the records (saved afterwards when persistent).
pub fn with<R>(f: impl FnOnce(&mut SaveData) -> R) -> R {
    let r = store(|s| f(&mut s.data));
    flush_logged();
    r
}

/// A copy of the records and the session statistics (for screens).
pub fn snapshot() -> (SaveData, SessionStats) {
    store(|s| (s.data.clone(), s.session.clone()))
}

const CAUSES: [&str; 13] = [
    "Arrow",
    "Explosion",
    "Brambles",
    "JumpedOn",
    "Lava",
    "Shock",
    "SpikeBall",
    "FallingObject",
    "Squish",
    "Curse",
    "Miasma",
    "Enemy",
    "Chalice",
];

fn add_counters(c: &mut Counters, k: &tfp_sim::versus::stats::KillStats) {
    c.total += k.total() as u64;
    c.team += k.team_kills as u64;
    c.own += k.self_kills as u64;
    c.environment += k.environment() as u64;
    for a in 0..9 {
        c.per_archer[a] += k.by_archer[a + 1] as u64;
    }
    for (i, &n) in k.by_cause.iter().enumerate() {
        if n > 0 {
            *c.by_cause.entry(CAUSES[i].into()).or_default() += n as u64;
        }
    }
}

/// A finished Versus match (once, when the match results show).
/// `archer_base[slot]` = the base archer (0..8) of each slot's look,
/// `tower` = the versus tower index, `random` = the tower was RANDOM,
/// `round_ticks` = ticks of rounds played this match.
pub fn record_versus(
    hud: &HudView,
    archer_base: &[Option<usize>],
    tower: usize,
    random: bool,
    round_ticks: u64,
) {
    store(|s| {
        let v = &mut s.data.versus;
        v.matches += 1;
        v.rounds += hud.round_index as u64 + 1;
        v.time_played += round_ticks;
        if let Some(t) = v.tower_plays.get_mut(tower) {
            *t += 1;
        }
        if random {
            v.random_plays += 1;
        }
        s.session.matches += 1;
        s.session.rounds += hud.round_index as u64 + 1;
        s.session.time_played += round_ticks;
        for (slot, sh) in hud.slots.iter().enumerate() {
            if sh.is_none() {
                continue;
            }
            let st = &hud.stats[slot];
            let a = archer_base
                .get(slot)
                .copied()
                .flatten()
                .unwrap_or(slot % 9)
                .min(8);
            let v = &mut s.data.versus;
            add_counters(&mut v.kills, &st.kills);
            add_counters(&mut v.deaths, &st.deaths);
            v.arrows_shot += st.arrows_shot as u64;
            v.arrows_collected += st.arrows_collected as u64;
            v.arrows_caught += st.arrows_caught as u64;
            v.treasures_taken += st.treasures_taken as u64;
            v.dodges += st.dodges as u64;
            v.jumps += st.jumps as u64;
            if st.won {
                v.wins[a] += 1;
                s.session.wins[a] += 1;
            }
            for &aw in hud.awards.get(slot).into_iter().flatten() {
                if let Some(row) = v.awards.get_mut(aw as usize) {
                    row[a] += 1;
                }
            }
            s.session.kills[a] += st.kills.kills as u64;
            s.session.deaths[a] += st.deaths.total() as u64;
            s.session.self_kills[a] += st.kills.self_kills as u64;
        }
    });
    flush_logged();
}

/// A finished Dark World run (`DwResult`).
pub fn record_dark_world(r: &tfp_sim::darkworld::DwResult) {
    with(|d| {
        let t = d.dark_world.entry(r.tower as usize).or_default();
        t.revealed = true;
        t.attempts += 1;
        t.deaths += r.deaths;
        if r.completed {
            match r.difficulty {
                0 => t.completed_normal = true,
                1 => t.completed_hardcore = true,
                _ => t.completed_legendary = true,
            }
            t.earned_eye |= r.eye;
            t.earned_gold_eye |= r.gold_eye;
            let i = (r.players.clamp(1, 4) - 1) as usize;
            if r.difficulty >= 2 && r.time > 0 && (t.best[i] == 0 || r.time < t.best[i]) {
                t.best[i] = r.time;
            }
            t.most_curses[i] = t.most_curses[i].max(r.curses);
        }
        for (i, c) in d.crowns.iter_mut().enumerate() {
            *c = r.crowns.get(i).copied().unwrap_or(false);
        }
    });
}

/// A quest attempt ended: `cleared` with `time` ticks for `players`.
#[allow(dead_code)] // API for the Dark World / co-op screens
pub fn record_quest(
    tower: usize,
    hardcore: bool,
    players: usize,
    cleared: bool,
    no_deaths: bool,
    time: u32,
    deaths: u32,
) {
    with(|d| {
        let q = d.quest.entry(tower).or_default();
        q.revealed = true;
        q.total_attempts += 1;
        q.total_deaths += deaths;
        if cleared {
            q.completed_normal = true;
            if hardcore {
                q.completed_hardcore = true;
                q.completed_no_deaths |= no_deaths;
            }
            let k = usize::from(players > 1);
            if time > 0 && (q.best[k] == 0 || time < q.best[k]) {
                q.best[k] = time;
            }
        }
    });
}

/// A trials attempt ended (`time` > 0 when won).
#[allow(dead_code)] // API for the Dark World / co-op screens
#[allow(clippy::needless_range_loop)]
pub fn record_trials(level: usize, time: u32, unlocked: [bool; 3]) {
    with(|d| {
        let t = d.trials.entry(level).or_default();
        t.attempts += 1;
        if time > 0 && (t.best == 0 || time < t.best) {
            t.best = time;
        }
        for i in 0..3 {
            t.unlocked[i] |= unlocked[i];
        }
    });
}

/// The co-op scene's records, seeded from the store (quest, trials,
/// crowns).
pub fn coop_records() -> crate::coop_ui::Records {
    let (d, _) = snapshot();
    let mut r = crate::coop_ui::Records::default();
    for (&k, q) in &d.quest {
        r.quest.insert(
            k,
            crate::coop_ui::QuestRecord {
                normal: q.completed_normal,
                hardcore: q.completed_hardcore,
                no_deaths: q.completed_no_deaths,
                best: q.best,
                attempts: q.total_attempts,
                deaths: q.total_deaths,
            },
        );
    }
    for (&k, t) in &d.trials {
        r.trials.insert(
            k,
            crate::coop_ui::TrialRecord {
                best: t.best,
                unlocked: t.unlocked,
                attempts: t.attempts,
                gem: t.gem,
            },
        );
    }
    for (&k, t) in &d.dark_world {
        r.dark_world.insert(
            k,
            crate::coop_ui::darkworld::DwRecord {
                normal: t.completed_normal || t.completed_hardcore || t.completed_legendary,
                hardcore: t.completed_hardcore || t.completed_legendary,
                legendary: t.completed_legendary,
                eye: t.earned_eye,
                gold_eye: t.earned_gold_eye,
                best: t.best,
                curses: t.most_curses,
                deaths: t.deaths,
                attempts: t.attempts,
            },
        );
    }
    for (i, c) in r.crowns.iter_mut().enumerate() {
        *c = d.crowns.get(i).copied().unwrap_or(false);
    }
    r
}

/// Back from a co-op game: its records into the store (they started from
/// [`coop_records`], so counters are taken as they are).
#[allow(clippy::needless_range_loop)]
pub fn store_coop(r: &crate::coop_ui::Records) {
    with(|d| {
        for (&k, q) in &r.quest {
            let o = d.quest.entry(k).or_default();
            o.revealed |= q.attempts > 0;
            o.completed_normal |= q.normal;
            o.completed_hardcore |= q.hardcore;
            o.completed_no_deaths |= q.no_deaths;
            for i in 0..2 {
                if q.best[i] > 0 && (o.best[i] == 0 || q.best[i] < o.best[i]) {
                    o.best[i] = q.best[i];
                }
            }
            o.total_attempts = o.total_attempts.max(q.attempts);
            o.total_deaths = o.total_deaths.max(q.deaths);
        }
        for (&k, t) in &r.trials {
            let o = d.trials.entry(k).or_default();
            if t.best > 0 && (o.best == 0 || t.best < o.best) {
                o.best = t.best;
            }
            for i in 0..3 {
                o.unlocked[i] |= t.unlocked[i];
            }
            o.attempts = o.attempts.max(t.attempts);
            o.gem |= t.gem;
        }
        d.crowns = r.crowns.to_vec();
    });
}

/// .NET ticks (100 ns) -> sim ticks (1/60 s).
fn net_ticks(t: i64) -> u32 {
    if t <= 0 {
        0
    } else {
        (tfp_assets::savedata::ticks_to_seconds(t) * 60.0).round() as u32
    }
}

/// The original's records over `d` (counters added, flags or-ed, best
/// times kept when better). `trials_order` maps our trials level index to
/// the original's (tower, tier).
#[allow(clippy::needless_range_loop)]
pub fn import(
    d: &mut SaveData,
    src: &tfp_assets::savedata::SaveData,
    trials_order: &[(usize, usize)],
) {
    let s = &src.stats;
    let v = &mut d.versus;
    let pos = |x: i64| x.max(0) as u64;
    v.time_played +=
        (tfp_assets::savedata::ticks_to_seconds(s.time_played_ticks).max(0.0) * 60.0) as u64;
    v.matches += pos(s.matches_played);
    v.rounds += pos(s.rounds_played);
    v.arrows_shot += pos(s.arrows_shot);
    v.arrows_collected += pos(s.arrows_collected);
    v.arrows_caught += pos(s.arrows_caught);
    v.treasures_taken += pos(s.treasures_taken);
    v.dodges += pos(s.dodges);
    v.jumps += pos(s.jumps);
    v.random_plays += pos(s.versus_random_plays);
    v.times_launched += pos(s.times_launched);
    for (ours, theirs) in [(&mut v.kills, &s.kills), (&mut v.deaths, &s.deaths)] {
        ours.total += theirs.total;
        ours.team += theirs.team;
        ours.own += theirs.own;
        ours.environment += theirs.environment;
        for a in 0..9 {
            ours.per_archer[a] += theirs.per_archer[a];
        }
        for (k, n) in &theirs.by_cause {
            let name = k.trim_end_matches("Kills");
            let name = match name {
                "Squish" => "Squish",
                "Orb" => "FallingObject",
                "CursedBow" => "Curse",
                n => n,
            };
            *ours.by_cause.entry(name.to_owned()).or_default() += n;
        }
    }
    for (a, w) in s.wins.iter().enumerate().take(9) {
        v.wins[a] += w;
    }
    for (i, row) in s.awards.iter().enumerate() {
        if let Some(ours) = v.awards.get_mut(i) {
            for (a, n) in row.iter().enumerate().take(9) {
                ours[a] += n;
            }
        }
    }
    for (i, n) in s.versus_tower_plays.iter().enumerate() {
        if let Some(t) = v.tower_plays.get_mut(i) {
            *t += n;
        }
    }
    let better = |ours: &mut u32, theirs: u32| {
        if theirs > 0 && (*ours == 0 || theirs < *ours) {
            *ours = theirs;
        }
    };
    for (i, q) in src.quest.iter().enumerate() {
        let o = d.quest.entry(i).or_default();
        o.revealed |= q.revealed;
        o.completed_normal |= q.completed_normal;
        o.completed_hardcore |= q.completed_hardcore;
        o.completed_no_deaths |= q.completed_no_deaths;
        better(&mut o.best[0], net_ticks(q.best_1p_ticks));
        better(&mut o.best[1], net_ticks(q.best_2p_ticks));
        o.total_deaths += q.total_deaths.max(0) as u32;
        o.total_attempts += q.total_attempts.max(0) as u32;
    }
    for (i, t) in src.dark_world.iter().enumerate() {
        let o = d.dark_world.entry(i).or_default();
        o.revealed |= t.revealed;
        o.completed_normal |= t.completed_normal;
        o.completed_hardcore |= t.completed_hardcore;
        o.completed_legendary |= t.completed_legendary;
        o.earned_eye |= t.earned_eye;
        o.earned_gold_eye |= t.earned_gold_eye;
        for k in 0..4 {
            better(&mut o.best[k], net_ticks(t.best_ticks[k]));
            o.most_curses[k] = o.most_curses[k].max(t.most_curses[k].max(0) as u32);
        }
        o.deaths += t.deaths.max(0) as u32;
        o.attempts += t.attempts.max(0) as u32;
    }
    for (ours, &(tower, tier)) in trials_order.iter().enumerate() {
        let Some(t) = src.trials.get(tower).and_then(|r| r.get(tier)) else {
            continue;
        };
        let o = d.trials.entry(ours).or_default();
        o.attempts += t.attempts.max(0) as u32;
        o.unlocked[0] |= t.unlocked_gold;
        o.unlocked[1] |= t.unlocked_diamond;
        o.unlocked[2] |= t.unlocked_dev_time;
        better(&mut o.best, net_ticks(t.best_time_ticks));
        o.gem |= src
            .unlocks
            .yellow_gems_found
            .contains(&(tower as i32, tier as i32));
    }
    d.imported_from = Some(src.path.display().to_string());
}

/// One-time import at start-up: when nothing was imported yet and the
/// original's save is found (or `force`), add its records. Returns the
/// source path when it imported.
pub fn import_once(force: bool, save_path: Option<&Path>) -> Option<PathBuf> {
    let done = store(|s| s.data.imported_from.is_some());
    if done && !force {
        return None;
    }
    let installs = tfp_assets::Installs::discover();
    let inst = installs.steam.as_ref().or(installs.eight_player.as_ref())?;
    let p = save_path
        .map(Path::to_path_buf)
        .or_else(|| tfp_assets::savedata::SaveData::find(inst))?;
    let src = match tfp_assets::savedata::SaveData::load(&p) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("original save {}: {e:#}", p.display());
            return None;
        }
    };
    let order: Vec<(usize, usize)> = tfp_assets::towers::TowerIndex::build(inst)
        .map(|i| i.trials.iter().map(|t| (t.index, t.tier)).collect())
        .unwrap_or_default();
    with(|d| import(d, &src, &order));
    Some(p)
}

/// Resets the store (tests).
#[cfg(test)]
pub fn reset_for_test() {
    store(|s| {
        s.data = SaveData::default();
        s.session = SessionStats::default();
        s.path = None;
    });
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_and_defaults() {
        let mut d = SaveData::default();
        d.versus.matches = 3;
        d.versus.kills.by_cause.insert("Arrow".into(), 7);
        d.quest.insert(
            2,
            QuestTower {
                best: [100, 0],
                ..Default::default()
            },
        );
        d.trials.insert(
            5,
            TrialsLevel {
                best: 321,
                unlocked: [true, false, false],
                attempts: 2,
                gem: true,
            },
        );
        let t = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<SaveData>(&t).unwrap(), d);
        // Old / partial files load with defaults for the rest.
        let p: SaveData = serde_json::from_str(r#"{"versus":{"matches":9}}"#).unwrap();
        assert_eq!(p.versus.matches, 9);
        assert_eq!(p.versus.awards.len(), 70);
        let dir = std::env::var_os("TFP_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| ".".into())
            .join("scratch/test-records");
        save_to(&dir.join(FILE), &d).unwrap();
        assert_eq!(load_from(&dir.join(FILE)).unwrap(), d);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn records_and_import() {
        reset_for_test();
        record_trials(4, 600, [true, false, false]);
        record_trials(4, 500, [false, true, false]);
        record_trials(4, 0, [false; 3]);
        record_quest(1, true, 2, true, true, 9000, 3);
        let (d, _) = snapshot();
        let t = d.trials[&4];
        assert_eq!(
            (t.best, t.unlocked, t.attempts),
            (500, [true, true, false], 3)
        );
        let q = d.quest[&1];
        assert!(q.completed_hardcore && q.completed_no_deaths && q.best == [0, 9000]);
        // Import from a synthetic original save.
        let mut src = tfp_assets::savedata::SaveData::default();
        src.stats.matches_played = 10;
        src.stats.wins = vec![1, 2, 0, 0, 0, 0, 0, 0, 0];
        src.stats.kills.by_cause.insert("ArrowKills".into(), 4);
        let mut qt = tfp_assets::savedata::QuestTowerStats::default();
        qt.best_1p_ticks = 28_523_471;
        qt.completed_normal = true;
        src.quest = vec![qt];
        let mut tl = tfp_assets::savedata::TrialsLevelStats::default();
        tl.best_time_ticks = 20_000_000;
        tl.unlocked_gold = true;
        src.trials = vec![vec![tl]];
        let mut d = SaveData::default();
        import(&mut d, &src, &[(0, 0)]);
        assert_eq!(d.versus.matches, 10);
        assert_eq!(d.versus.wins[1], 2);
        assert_eq!(d.versus.kills.by_cause["Arrow"], 4);
        assert_eq!(d.quest[&0].best[0], 171, "2.85 s = 171 ticks");
        assert_eq!((d.trials[&0].best, d.trials[&0].unlocked[0]), (120, true));
        assert!(d.imported_from.is_some());
        // A real save, when this machine has one.
        if let Some(inst) = tfp_assets::Installs::discover().steam
            && let Some(p) = tfp_assets::savedata::SaveData::find(&inst)
        {
            let src = tfp_assets::savedata::SaveData::load(&p).unwrap();
            let mut d = SaveData::default();
            import(&mut d, &src, &[]);
            assert!(d.versus.matches > 0 || d.versus.times_launched > 0);
        }
        reset_for_test();
    }
}
