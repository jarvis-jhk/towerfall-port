//! The sim as tfp-net sees it ([`NetWorld`]), the game's part of the
//! match settings ([`OnlineSettings`], the opaque `MatchSettings::game`
//! bytes), and building the sim's Versus match from a `MatchStart`.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tfp_net::{InputStatus, MatchStart, NetGame, SKIN_ALT, TeamMode};
use tfp_sim::versus::{self, MatchLength, MatchSettings as SimSettings, Mode, SlotSettings};
use tfp_sim::world::MAX_PLAYERS;
use tfp_sim::{Content, PlayerInput, World};

/// `tfp_sim::World` plus its immutable content: cloning is a `World` clone
/// and an `Arc` bump, which is how rollback saves a frame.
#[derive(Clone)]
pub struct NetWorld {
    pub world: World,
    pub content: Arc<Content>,
    /// Tests: the Dark World proof assist (boss damage every N ticks, 0 =
    /// none), applied inside the step on every machine
    /// (`coop_ui::darkworld::assist`).
    pub assist: Option<u32>,
}

/// Menu actions of the host, carried in its first slot's reserved input
/// byte (the sim ignores it) so they happen at the same frame everywhere.
/// Dark World game over: CONTINUE.
pub const ACTION_CONTINUE: u8 = 1;
/// Dark World game over / complete: RESTART (same settings).
pub const ACTION_RESTART: u8 = 2;

impl NetGame for NetWorld {
    type Input = PlayerInput;

    fn advance(&mut self, inputs: &[(PlayerInput, InputStatus)]) {
        let mut i = [PlayerInput::default(); MAX_PLAYERS];
        for (slot, (inp, status)) in inputs.iter().enumerate().take(MAX_PLAYERS) {
            if *status == InputStatus::Disconnected {
                // docs/net.md leave rule; idempotent and part of the state,
                // so it survives rollback and is the same on every machine.
                self.world.mark_disconnected(slot);
            } else {
                i[slot] = *inp;
            }
        }
        // The host's menu choice (slot 0 is always the host's first archer).
        // Only acted on in the screen it belongs to, so a repeated
        // (predicted) action after it is harmless.
        if self.world.dark_world.is_some() {
            use tfp_sim::darkworld::{self as dw, DwPhase};
            let phase = self
                .world
                .dark_world
                .as_ref()
                .map(|d| (d.phase, d.menu_ready));
            match (i[0]._pad, phase) {
                (ACTION_CONTINUE, Some((DwPhase::GameOver, true))) => {
                    dw::continue_level(&mut self.world, &self.content);
                }
                (ACTION_RESTART, Some((DwPhase::GameOver, true) | (DwPhase::Complete, _))) => {
                    dw::restart(&mut self.world, &self.content);
                }
                _ => {}
            }
            if let Some(n) = self.assist {
                let f = self.world.frame;
                crate::coop_ui::darkworld::assist(&mut self.world, &self.content, n, f);
            }
        }
        for p in i.iter_mut() {
            p._pad = 0;
        }
        self.world.step(&self.content, &i);
    }

    fn checksum(&self) -> u128 {
        self.world.checksum128()
    }
}

impl NetWorld {
    /// The Versus phase, if this is a Versus match.
    pub fn phase(&self) -> Option<versus::Phase> {
        self.world.versus.as_ref().map(|m| m.phase)
    }

    /// The match is over and acknowledged (results confirmed).
    pub fn finished(&self) -> bool {
        self.phase() == Some(versus::Phase::Finished) || super::coop::coop_finished(&self.world)
    }
}

/// Quantise an axis to tfp-net's steps (multiples of 8): stick wobble does
/// not change the input, so it does not cause mispredictions. The sim
/// applies the spec's dead zone itself.
pub fn quantise(mut i: PlayerInput) -> PlayerInput {
    let q = |v: i8| ((v as i32 + 4).div_euclid(8) * 8).clamp(-120, 120) as i8;
    i.aim_x = q(i.aim_x);
    i.aim_y = q(i.aim_y);
    // `_pad` stays: the host's menu actions (ACTION_*), 0 otherwise.
    i
}

// ------------------------------------------------------------ settings

/// What the host decides for an online Versus match, in
/// `tfp_net::MatchSettings::game` (bincode). Roster, archers, teams and the
/// seed come from `MatchStart` itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OnlineSettings {
    /// 0 Last Man Standing, 1 Headhunters, 2 Team Deathmatch.
    pub mode: u8,
    /// 0 Instant, 1 Quick, 2 Standard, 3 Epic.
    pub length: u8,
    /// Versus tower directory name (`00 - Sacred Ground`).
    pub tower: String,
    /// Number of the tower's level files and a hash of their bytes: every
    /// machine must simulate the same levels.
    pub levels: u16,
    pub data_hash: u64,
    /// Variants on for everyone (titles, e.g. "NO DODGING"; per-player
    /// variants listed here apply to every archer).
    pub variants: Vec<String>,
    /// Show the round-0 tower card (waits for a Confirm press).
    pub tower_card: bool,
    /// Per-player variants for single archers: title -> the archers it is
    /// on for, as (machine, local player index), so the flags stay with the
    /// player when the slot table is renumbered.
    pub player_variants: Vec<(String, Vec<(u8, u8)>)>,
    /// The 8-player build's 420x240 towers (every machine has that build
    /// with identical data, and more than 4 archers play).
    pub wide: bool,
    /// Teams in Team Deathmatch (2-6).
    pub teams: u8,
    /// What is played: [`KIND_VERSUS`], [`KIND_QUEST`], [`KIND_TRIALS`].
    pub kind: u8,
    /// Quest tower / Trials level index (co-op kinds).
    pub coop_index: u16,
    pub hardcore: bool,
    /// Procedural (Cataclysm) towers: the layout seed of this match (the
    /// host picks one per match; part of the sim's `MatchSettings` and of
    /// the content every machine builds).
    pub procedural_seed: i32,
    /// Dark World: 0 Normal, 1 Hardcore, 2 Legendary.
    pub difficulty: u8,
    /// Tests: the Dark World proof assist (see `NetWorld::assist`).
    pub dw_assist: Option<u32>,
    /// Tests: start the Dark World run at this level (0-based).
    pub dw_level: Option<u8>,
}

pub const KIND_VERSUS: u8 = 0;
pub const KIND_QUEST: u8 = 1;
pub const KIND_TRIALS: u8 = 2;
pub const KIND_DARKWORLD: u8 = 3;
pub const KIND_NAMES: [&str; 4] = ["VERSUS", "QUEST", "TRIALS", "DARK WORLD"];

impl Default for OnlineSettings {
    fn default() -> Self {
        OnlineSettings {
            mode: 1,
            length: 2,
            tower: String::new(),
            levels: 0,
            data_hash: 0,
            variants: Vec::new(),
            tower_card: true,
            player_variants: Vec::new(),
            wide: false,
            teams: 2,
            kind: KIND_VERSUS,
            coop_index: 0,
            hardcore: false,
            procedural_seed: tfp_sim::loader::DEFAULT_PROCEDURAL_SEED,
            difficulty: 0,
            dw_assist: None,
            dw_level: None,
        }
    }
}

/// What a machine has (in `MachineView::info`): hashes over the versus
/// level files and archer data of its Steam install and of the 8-player
/// build (if installed), and whether Dark World is there. The host plays
/// wide towers only when every machine's `wide` hash equals its own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MachineInfo {
    pub steam: u64,
    pub wide: Option<u64>,
    pub dark_world: bool,
}

impl MachineInfo {
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("serialisable")
    }

    pub fn decode(b: &[u8]) -> Option<MachineInfo> {
        bincode::deserialize(b).ok()
    }

    /// Hash this machine's installs (reads every versus level file once).
    pub fn of(installs: &tfp_assets::Installs) -> MachineInfo {
        let hash = |inst: &tfp_assets::GameInstall| -> u64 {
            let mut files = tfp_sim::loader::versus_levels(inst).unwrap_or_default();
            files.push(inst.content.root.join("Atlas/GameData/archerData.xml"));
            levels_hash(&files)
        };
        let steam = installs.steam.as_ref().or(installs.eight_player.as_ref());
        MachineInfo {
            steam: steam.map(hash).unwrap_or(0),
            wide: installs.eight_player.as_ref().map(hash),
            dark_world: steam.is_some_and(|i| i.dark_world.is_some()),
        }
    }
}

/// Why a room plays standard or wide towers (shown in the lobby).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TowerSize {
    Wide,
    /// 4 or fewer archers.
    FewArchers,
    /// Some machine lacks the 8-player build (or has other data).
    NotEveryMachine,
    /// The tower does not exist in the 8-player build.
    NotInBuild,
}

impl TowerSize {
    pub fn label(self) -> &'static str {
        match self {
            TowerSize::Wide => "WIDE TOWERS (8-PLAYER BUILD, ALL MACHINES)",
            TowerSize::FewArchers => "STANDARD TOWERS (4 OR FEWER ARCHERS)",
            TowerSize::NotEveryMachine => {
                "STANDARD TOWERS (NOT EVERY MACHINE HAS THE 8-PLAYER BUILD)"
            }
            TowerSize::NotInBuild => "STANDARD TOWERS (TOWER NOT IN THE 8-PLAYER BUILD)",
        }
    }
}

/// Host: wide or standard towers for these machines and archers.
pub fn tower_size(
    infos: &[Option<MachineInfo>],
    archers: usize,
    tower_in_wide_build: bool,
) -> TowerSize {
    let first = infos.first().cloned().flatten().and_then(|i| i.wide);
    if archers <= 4 {
        TowerSize::FewArchers
    } else if first.is_none()
        || infos
            .iter()
            .any(|i| i.as_ref().and_then(|i| i.wide) != first)
    {
        TowerSize::NotEveryMachine
    } else if !tower_in_wide_build {
        TowerSize::NotInBuild
    } else {
        TowerSize::Wide
    }
}

pub const MODE_NAMES: [&str; 3] = ["LAST MAN STANDING", "HEADHUNTERS", "TEAM DEATHMATCH"];
pub const LENGTH_NAMES: [&str; 4] = ["INSTANT", "QUICK", "STANDARD", "EPIC"];

impl OnlineSettings {
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("serialisable")
    }

    pub fn decode(b: &[u8]) -> Option<OnlineSettings> {
        bincode::deserialize(b).ok()
    }

    pub fn sim_mode(&self) -> Mode {
        match self.mode {
            0 => Mode::LastManStanding,
            2 => Mode::TeamDeathmatch,
            _ => Mode::Headhunters,
        }
    }

    pub fn sim_length(&self) -> MatchLength {
        match self.length {
            0 => MatchLength::Instant,
            1 => MatchLength::Quick,
            3 => MatchLength::Epic,
            _ => MatchLength::Standard,
        }
    }

    pub fn team_mode(&self) -> TeamMode {
        if self.mode == 2 {
            TeamMode::Teams(self.teams.clamp(2, 6))
        } else {
            TeamMode::Ffa
        }
    }

    /// The lobby's `tfp_net::MatchSettings` for these settings.
    pub fn net_settings(&self, archers: u8) -> tfp_net::MatchSettings {
        tfp_net::MatchSettings {
            team_mode: self.team_mode(),
            archers,
            mode: MODE_NAMES[self.mode.min(2) as usize].to_string(),
            map: tower_title(&self.tower),
            game: self.encode(),
        }
    }
}

/// `00 - Sacred Ground` -> `SACRED GROUND`.
pub fn tower_title(dir: &str) -> String {
    dir.split_once(" - ")
        .map_or(dir, |(_, n)| n)
        .to_ascii_uppercase()
}

// ------------------------------------------------------------ towers

/// One versus tower's simulation content, loaded the same way on every
/// machine (cloning shares the content).
#[derive(Clone)]
pub struct TowerData {
    pub dir_name: String,
    pub levels: Vec<PathBuf>,
    pub fixed_first: bool,
    pub content: Arc<Content>,
    pub data_hash: u64,
    /// Base archers in the archer data (`MatchSettings::archers`).
    pub base_archers: usize,
    /// Per base archer: its alt costume's index in `Content::archers`.
    pub alt_index: Vec<Option<u16>>,
    /// From the 8-player build (420x240 towers).
    pub wide: bool,
    /// Enemy / ghost art registered into `content` (as the offline play
    /// mode does; appended sprites only, the sim is unaffected).
    pub art: crate::coop_ui::art::Art,
    /// A Cataclysm tower (layouts generated from `key`).
    pub procedural: bool,
    /// The procedural key the content was built with (`NO_KEY` for fixed
    /// towers).
    pub key: (i32, bool),
}

/// The key of fixed towers (their content does not depend on it).
pub const NO_KEY: (i32, bool) = (tfp_sim::loader::DEFAULT_PROCEDURAL_SEED, false);

/// FNV-1a over the level files' bytes.
pub fn levels_hash(levels: &[PathBuf]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in levels {
        let bytes = std::fs::read(p).unwrap_or_default();
        for b in (bytes.len() as u64).to_le_bytes().iter().chain(&bytes) {
            h = (h ^ *b as u64).wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// The procedural content key of these settings: (layout seed, Team
/// Deathmatch), as `tfp_sim::versus::MatchSettings::procedural_key`.
pub fn procedural_key(s: &OnlineSettings) -> (i32, bool) {
    (s.procedural_seed, s.mode == 2)
}

/// A fresh 9-digit layout seed (the original's seed range) for the host.
pub fn random_procedural_seed() -> i32 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    100_000_000 + (h.finish() % 900_000_000) as i32
}

/// FNV over the levels' geometry and placements as built (procedural
/// layouts included): every machine must have generated the same.
pub fn geometry_hash(c: &Content) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for l in &c.levels {
        for b in format!("{l:?}").bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// Versus towers that can be played online (procedural ones included: the
/// host picks the layout seed per match).
pub fn online_towers(
    inst: &tfp_assets::GameInstall,
) -> Result<Vec<tfp_assets::towers::VersusTower>> {
    let idx = tfp_assets::towers::TowerIndex::build(inst)?;
    Ok(idx
        .versus
        .into_iter()
        .filter(|t| !t.levels.is_empty())
        .collect())
}

impl TowerData {
    /// Load tower `dir_name` (or the first one if empty); `key` = the
    /// procedural layout key (ignored by fixed towers).
    pub fn load(
        inst: &tfp_assets::GameInstall,
        dir_name: &str,
        key: (i32, bool),
    ) -> Result<TowerData> {
        let towers = online_towers(inst)?;
        let t = towers
            .iter()
            .find(|t| dir_name.is_empty() || t.dir_name == dir_name)
            .with_context(|| format!("no versus tower {dir_name:?}"))?;
        let mut content = tfp_sim::loader::content_for_match(inst, &t.levels, key)?;
        let data_hash = levels_hash(&t.levels) ^ geometry_hash(&content).rotate_left(1);
        let art = crate::coop_ui::art::Art::register(&mut content, inst);
        let gd = tfp_assets::game_data::GameData::load(&inst.content)?;
        let base = gd.archers.archers.len();
        let mut alt_index = Vec::with_capacity(base);
        let mut n = 0u16;
        for i in 0..base {
            let has = gd.archers.alts.get(i).is_some_and(|a| a.is_some());
            alt_index.push(has.then(|| base as u16 + n));
            if has {
                n += 1;
            }
        }
        Ok(TowerData {
            dir_name: t.dir_name.clone(),
            levels: t.levels.clone(),
            fixed_first: t.fixed_first,
            content: Arc::new(content),
            data_hash,
            base_archers: base,
            alt_index,
            wide: false,
            art,
            procedural: t.procedural,
            key: if t.procedural { key } else { NO_KEY },
        })
    }

    /// Tower `dir_name` from the Steam install or (`wide`) the 8-player build.
    pub fn load_from(
        installs: &tfp_assets::Installs,
        dir_name: &str,
        wide: bool,
        key: (i32, bool),
    ) -> Result<TowerData> {
        let inst = install_for(installs, wide).with_context(|| {
            if wide {
                "the 8-player build is not installed"
            } else {
                "game data not found (TF_GAME_DIR)"
            }
        })?;
        let mut t = TowerData::load(&inst, dir_name, key)?;
        t.wide = wide;
        Ok(t)
    }

    /// Sim archer index of a lobby (archer, skin) choice.
    pub fn sim_archer(&self, archer: u8, skin: u8) -> u16 {
        let a = (archer as usize).min(self.base_archers.saturating_sub(1));
        match self.alt_index.get(a) {
            Some(Some(alt)) if skin == SKIN_ALT => *alt,
            _ => a as u16,
        }
    }

    /// Settings for this tower (the host fills mode, length, variants).
    pub fn settings(&self) -> OnlineSettings {
        let mut s = OnlineSettings::default();
        self.apply_to(&mut s);
        s
    }

    /// Put this tower into `s` (keeps mode, length, variants, ...).
    pub fn apply_to(&self, s: &mut OnlineSettings) {
        s.tower = self.dir_name.clone();
        s.levels = self.levels.len() as u16;
        s.data_hash = self.data_hash;
        s.wide = self.wide;
        if self.procedural {
            s.procedural_seed = self.key.0;
        }
    }
}

/// The sim's Versus settings for a started match.
pub fn sim_settings(
    ms: &MatchStart,
    os: &OnlineSettings,
    tower: &TowerData,
) -> Result<SimSettings> {
    if os.tower != tower.dir_name || os.levels as usize != tower.levels.len() {
        bail!(
            "the host plays {:?}; this machine loaded {:?}",
            os.tower,
            tower.dir_name
        );
    }
    if tower.procedural && tower.key != procedural_key(os) {
        bail!("tower {:?}: built for another layout seed", os.tower);
    }
    if os.data_hash != tower.data_hash {
        bail!(
            "tower {:?}: this machine's level files differ from the host's (different game version?)",
            os.tower
        );
    }
    let mut s = SimSettings::new(os.sim_mode(), 0, (0..tower.levels.len()).collect());
    for p in &ms.players {
        let slot = p.slot as usize;
        if slot < MAX_PLAYERS {
            s.slots[slot] = Some(SlotSettings {
                archer: tower.sim_archer(p.archer, p.skin),
                team: if os.mode == 2 {
                    p.team.min(os.teams.clamp(2, 6) - 1)
                } else {
                    0
                },
            });
        }
    }
    s.teams = os.teams.clamp(2, 6);
    s.length = os.sim_length();
    s.fixed_first = tower.fixed_first;
    s.seed = (ms.seed ^ (ms.seed >> 32)) as i32;
    s.tower_card = os.tower_card;
    s.procedural_seed = if tower.procedural {
        os.procedural_seed
    } else {
        tfp_sim::loader::DEFAULT_PROCEDURAL_SEED
    };
    let joined = s.joined_mask();
    for title in &os.variants {
        if let Some(v) = tfp_sim::variants::Variant::ALL
            .iter()
            .find(|v| v.title() == title)
        {
            s.variants.set_all(*v, true, joined);
        }
    }
    for (title, owners) in &os.player_variants {
        let Some(v) = tfp_sim::variants::Variant::ALL
            .iter()
            .find(|v| v.title() == title)
        else {
            continue;
        };
        for p in &ms.players {
            if owners.contains(&(p.machine.0, p.local_index)) {
                s.variants.set(*v, p.slot as usize, true);
            }
        }
    }
    Ok(s)
}

/// The towers loaded so far (switching between standard and wide, or
/// between towers, loads once).
#[derive(Default)]
pub struct TowerCache {
    towers: Vec<TowerData>,
}

impl TowerCache {
    pub fn get(
        &mut self,
        installs: &tfp_assets::Installs,
        dir: &str,
        wide: bool,
        key: (i32, bool),
    ) -> Result<&TowerData> {
        let pos = self.towers.iter().position(|t| {
            t.wide == wide
                && (dir.is_empty() || t.dir_name == dir)
                && (!t.procedural || t.key == key)
        });
        let i = match pos {
            Some(i) => i,
            None => {
                // Procedural layouts of other seeds are not kept.
                self.towers.retain(|t| !t.procedural);
                self.towers
                    .push(TowerData::load_from(installs, dir, wide, key)?);
                self.towers.len() - 1
            }
        };
        Ok(&self.towers[i])
    }
}

/// Versus tower directory names of the 8-player build (empty if absent).
pub fn wide_tower_names(installs: &tfp_assets::Installs) -> Vec<String> {
    installs
        .eight_player
        .as_ref()
        .and_then(|i| online_towers(i).ok())
        .map(|v| v.into_iter().map(|t| t.dir_name).collect())
        .unwrap_or_default()
}

/// Pick the install a match's towers come from.
pub fn install_for(installs: &tfp_assets::Installs, wide: bool) -> Option<tfp_assets::GameInstall> {
    if wide {
        installs.eight_player.clone()
    } else {
        installs.steam.clone().or(installs.eight_player.clone())
    }
}

/// The starting state of an online Versus match.
pub fn new_world(ms: &MatchStart, tower: &TowerData) -> Result<NetWorld> {
    let os =
        OnlineSettings::decode(&ms.settings.game).context("bad match settings from the host")?;
    let s = sim_settings(ms, &os, tower)?;
    Ok(NetWorld {
        world: versus::new_match(&tower.content, s),
        content: tower.content.clone(),
        assist: None,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tfp_sim::content::LevelDef;

    /// A small arena with 4 spawns and team spawns (no game data needed).
    pub fn arena() -> Content {
        let mut g = vec![vec!['.'; 32]; 24];
        for r in [0, 22, 23] {
            g[r].fill('#');
        }
        for row in g.iter_mut().take(22) {
            row[0] = '#';
            row[31] = '#';
        }
        for row in g.iter_mut().take(14).skip(11) {
            row[0] = '.';
            row[31] = '.';
        }
        for x in 3..10 {
            g[16][x] = '#';
            g[16][31 - x] = '#';
        }
        for (r, c, ch) in [(21, 5, 'P'), (15, 5, 'P'), (21, 9, 'A'), (15, 8, 'A')] {
            g[r][c] = ch;
            g[r][31 - c] = if ch == 'A' { 'B' } else { ch };
        }
        let rows: Vec<String> = g.iter().map(|r| r.iter().collect()).collect();
        let refs: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
        Content::with_levels(vec![
            LevelDef::from_ascii("a", &refs),
            LevelDef::from_ascii("b", &refs),
        ])
    }

    pub fn net_world(n: usize, seed: i32) -> NetWorld {
        let c = arena();
        let mut s = SimSettings::new(Mode::Headhunters, n, vec![0, 1]);
        s.seed = seed;
        s.length = MatchLength::Instant;
        let world = versus::new_match(&c, s);
        NetWorld {
            world,
            content: Arc::new(c),
            assist: None,
        }
    }

    fn inputs(f: u32, n: usize) -> Vec<(PlayerInput, InputStatus)> {
        (0..n)
            .map(|p| {
                let k = (f / 7 + p as u32 * 13) % 9;
                let b = if (f + p as u32).is_multiple_of(5) {
                    PlayerInput::JUMP
                } else if k == 3 {
                    PlayerInput::SHOOT
                } else {
                    0
                };
                (
                    PlayerInput::dpad((k % 3) as i8 - 1, 0, b),
                    InputStatus::Confirmed,
                )
            })
            .collect()
    }

    #[test]
    fn clone_rollback_is_exact() {
        let mut a = net_world(4, 3);
        for f in 0..200 {
            a.advance(&inputs(f, 4));
        }
        let saved = a.clone();
        let sum = a.checksum();
        for f in 200..260 {
            a.advance(&inputs(f, 4));
        }
        let ahead = a.checksum();
        let mut b = saved.clone();
        assert_eq!(b.checksum(), sum);
        for f in 200..260 {
            b.advance(&inputs(f, 4));
        }
        assert_eq!(b.checksum(), ahead, "re-simulation is bit-identical");
    }

    #[test]
    fn disconnect_marks_the_world() {
        let mut a = net_world(3, 1);
        let mut i = inputs(0, 3);
        i[2] = (
            PlayerInput::dpad(1, 0, PlayerInput::JUMP),
            InputStatus::Disconnected,
        );
        a.advance(&i);
        assert!(!a.world.connected[2] && a.world.connected[1]);
        let mut b = net_world(3, 1);
        b.advance(&inputs(0, 3));
        assert_ne!(a.checksum(), b.checksum(), "the leave is part of the state");
    }

    #[test]
    fn settings_roundtrip_and_quantise() {
        let s = OnlineSettings {
            mode: 2,
            length: 1,
            tower: "00 - Sacred Ground".into(),
            levels: 12,
            data_hash: 99,
            variants: vec!["NO DODGING".into()],
            tower_card: false,
            player_variants: vec![("MAX ARROWS".into(), vec![(1, 0)])],
            wide: true,
            ..Default::default()
        };
        assert_eq!(OnlineSettings::decode(&s.encode()), Some(s.clone()));
        let n = s.net_settings(8);
        assert_eq!(n.team_mode, TeamMode::Teams(2));
        assert_eq!(n.map, "SACRED GROUND");
        assert_eq!(n.mode, "TEAM DEATHMATCH");
        let q = quantise(PlayerInput {
            aim_x: 59,
            aim_y: -3,
            buttons: 1,
            _pad: 7,
        });
        // The reserved byte carries the host's menu actions: kept.
        assert_eq!((q.aim_x, q.aim_y, q._pad), (56, 0, 7));
        assert_eq!(quantise(PlayerInput::dpad(1, -1, 0)).aim_x, 120);
    }

    fn fake_tower() -> TowerData {
        TowerData {
            dir_name: "00 - Test".into(),
            levels: vec![PathBuf::from("a"), PathBuf::from("b")],
            fixed_first: false,
            content: Arc::new(arena()),
            data_hash: 42,
            base_archers: 4,
            alt_index: vec![None; 4],
            wide: false,
            art: Default::default(),
            procedural: false,
            key: NO_KEY,
        }
    }

    fn start(players: &[(u8, u8)]) -> MatchStart {
        MatchStart {
            match_id: 1,
            seed: 9,
            settings: tfp_net::MatchSettings::default(),
            players: players
                .iter()
                .enumerate()
                .map(|(slot, &(m, li))| tfp_net::PlayerSlot {
                    slot: slot as u8,
                    machine: tfp_net::MachineId(m),
                    local_index: li,
                    name: String::new(),
                    archer: slot as u8,
                    skin: 0,
                    dup: 0,
                    team: (slot % 3) as u8,
                })
                .collect(),
            ggrs: Default::default(),
            spectators: vec![],
        }
    }

    #[test]
    fn per_player_variants_follow_the_archer() {
        use tfp_sim::variants::Variant;
        let t = fake_tower();
        let mut os = t.settings();
        os.variants = vec![Variant::NoDodging.title().to_string()];
        // Max arrows for machine 2's second player and machine 0's first.
        os.player_variants = vec![(Variant::MaxArrows.title().to_string(), vec![(2, 1), (0, 0)])];
        os.mode = 2;
        os.teams = 3;
        // Machine 2's second player sits in slot 3 in this match.
        let ms = start(&[(0, 0), (1, 0), (2, 0), (2, 1)]);
        let s = sim_settings(&ms, &os, &t).unwrap();
        for slot in 0..4 {
            assert!(s.variants.get(Variant::NoDodging, slot));
        }
        let on: Vec<bool> = (0..4)
            .map(|p| s.variants.get(Variant::MaxArrows, p))
            .collect();
        assert_eq!(on, vec![true, false, false, true]);
        assert_eq!(s.teams, 3);
        assert_eq!(s.slots[2].unwrap().team, 2);
        // Wrong data is refused.
        os.data_hash = 7;
        assert!(sim_settings(&ms, &os, &t).is_err());
    }

    #[test]
    fn wide_towers_need_every_machine() {
        let has = |h| {
            Some(MachineInfo {
                steam: 1,
                wide: h,
                dark_world: true,
            })
        };
        let all = vec![has(Some(5)), has(Some(5)), has(Some(5))];
        assert_eq!(tower_size(&all, 6, true), TowerSize::Wide);
        assert_eq!(tower_size(&all, 4, true), TowerSize::FewArchers);
        assert_eq!(tower_size(&all, 6, false), TowerSize::NotInBuild);
        let one_missing = vec![has(Some(5)), has(None)];
        assert_eq!(
            tower_size(&one_missing, 8, true),
            TowerSize::NotEveryMachine
        );
        let other_data = vec![has(Some(5)), has(Some(6))];
        assert_eq!(tower_size(&other_data, 8, true), TowerSize::NotEveryMachine);
        assert_eq!(
            tower_size(&[has(Some(5)), None], 8, true),
            TowerSize::NotEveryMachine
        );
        let i = has(Some(3)).unwrap();
        assert_eq!(MachineInfo::decode(&i.encode()), Some(i));
    }

    #[test]
    fn procedural_layouts_follow_the_seed() {
        let installs = tfp_assets::Installs::discover();
        let Some(inst) = installs.steam.clone() else {
            eprintln!("skipping: no game data");
            return;
        };
        let Some(t) = online_towers(&inst)
            .unwrap()
            .into_iter()
            .find(|t| t.procedural)
        else {
            eprintln!("skipping: no procedural tower");
            return;
        };
        let a = TowerData::load(&inst, &t.dir_name, (111_111_111, false)).unwrap();
        let b = TowerData::load(&inst, &t.dir_name, (111_111_111, false)).unwrap();
        let c = TowerData::load(&inst, &t.dir_name, (222_222_222, false)).unwrap();
        let d = TowerData::load(&inst, &t.dir_name, (111_111_111, true)).unwrap();
        assert!(a.procedural && a.key == (111_111_111, false));
        assert_eq!(a.data_hash, b.data_hash, "same seed, same layout");
        assert_ne!(a.data_hash, c.data_hash, "another seed, another layout");
        assert_ne!(a.data_hash, d.data_hash, "team spawns change the layout");
        let mut os = a.settings();
        assert_eq!(os.procedural_seed, 111_111_111);
        // The settings must name the seed the content was built with.
        os.procedural_seed = 5;
        let ms = start(&[(0, 0), (1, 0)]);
        assert!(sim_settings(&ms, &os, &a).is_err());
        os.procedural_seed = 111_111_111;
        assert_eq!(
            sim_settings(&ms, &os, &a).unwrap().procedural_seed,
            111_111_111
        );
    }
}
