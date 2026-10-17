//! Read-only reader for the original's save file (`tf_saveData`): options,
//! statistics, quest/Dark World/trials records, unlocks and keyboard
//! configs. The file is `XmlSerializer` output of the game's `SaveData`
//! (layout: `docs/spec/ui.md` §12.5, notes in `docs/formats/savedata.md`).
//! Nothing here ever writes it.

use crate::error::{Error, Result};
use crate::install::GameInstall;
use crate::xml::{self, XmlFile};
use roxmltree::Node;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// .NET ticks per second (times in the save are 100 ns ticks).
pub const TICKS_PER_SECOND: i64 = 10_000_000;

pub fn ticks_to_seconds(t: i64) -> f64 {
    t as f64 / TICKS_PER_SECOND as f64
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct SaveOptions {
    pub fullscreen: bool,
    pub music_volume: f32,
    pub sfx_volume: f32,
    pub gamepad_vibration: bool,
    pub gamepad_discovery: bool,
    pub allow_direct_input: bool,
    pub hold_to_pause: bool,
    pub show_tips: bool,
    /// `UseGPU`, `UseCPU` or `Off`.
    pub replay_mode: String,
    pub can_skip_replays: bool,
    pub show_input_during_replays: bool,
    pub remove_slow_motion_wave_effect: bool,
    pub remove_scroll_effects: bool,
    pub remove_screen_flash_effects: bool,
    pub vertical_sync: bool,
    pub dev_console: bool,
}

/// Kill or death counters.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct KillStats {
    pub total: u64,
    pub team: u64,
    pub own: u64,
    pub environment: u64,
    /// Green, Blue, Pink, Orange, White, Yellow, Cyan, Purple, Red.
    pub per_archer: [u64; 9],
    /// By cause: `ArrowKills`, `ShockKills`, `ExplosionKills`,
    /// `BramblesKills`, `JumpedOnKills`, `LavaKills`, `SpikeBallKills`,
    /// `OrbKills`, `SquishKills`, `CursedBowKills`, `MiasmaKills`,
    /// `EnemyKills`, `ChaliceKills` (as named in the file).
    pub by_cause: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct SaveStats {
    pub time_played_ticks: i64,
    pub kills: KillStats,
    pub deaths: KillStats,
    /// Match wins per archer.
    pub wins: Vec<u64>,
    /// `awards[award][archer]`.
    pub awards: Vec<Vec<u64>>,
    pub versus_tip_matches: i64,
    /// Five preset slots; `None` = empty slot, else variant titles.
    pub variant_presets: Vec<Option<Vec<String>>>,
    /// Times each Versus tower was played.
    pub versus_tower_plays: Vec<u64>,
    // Steam-backed counters (plain integers in the file; −1 if never synced).
    pub times_launched: i64,
    pub total_versus_kills: i64,
    pub matches_played: i64,
    pub rounds_played: i64,
    pub arrows_shot: i64,
    pub arrows_collected: i64,
    pub arrows_caught: i64,
    pub treasures_taken: i64,
    pub dodges: i64,
    pub jumps: i64,
    pub versus_random_plays: i64,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct QuestTowerStats {
    pub revealed: bool,
    pub completed_normal: bool,
    pub completed_hardcore: bool,
    pub completed_no_deaths: bool,
    /// Ticks, 0 = none.
    pub best_1p_ticks: i64,
    pub best_2p_ticks: i64,
    pub total_deaths: i64,
    pub total_attempts: i64,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct DarkWorldTowerStats {
    pub revealed: bool,
    pub completed_normal: bool,
    pub completed_hardcore: bool,
    pub completed_legendary: bool,
    pub earned_eye: bool,
    pub earned_gold_eye: bool,
    /// Best time for 1–4 players, ticks (0 = none).
    pub best_ticks: [i64; 4],
    /// Most curses for 1–4 players.
    pub most_curses: [i64; 4],
    pub deaths: i64,
    pub attempts: i64,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct TrialsLevelStats {
    pub attempts: i64,
    pub unlocked_gold: bool,
    pub unlocked_diamond: bool,
    pub unlocked_dev_time: bool,
    /// Ticks, 0 = none.
    pub best_time_ticks: i64,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct SaveUnlocks {
    pub moonstone: bool,
    pub sunken_city: bool,
    pub tower_forge: bool,
    pub ascension: bool,
    pub white_archer_revealed: bool,
    /// (tower, level) of every yellow gem collected.
    pub yellow_gems_found: Vec<(i32, i32)>,
    pub yellow_archer_revealed: bool,
    pub purple_archer_revealed: bool,
    pub white_archer_intro: bool,
    pub yellow_archer_intro: bool,
    pub cyan_archer_intro: bool,
    pub purple_archer_intro: bool,
    pub ascension_box_open: bool,
    pub gunn_style: bool,
    /// Variant titles still tagged NEW.
    pub new_variants: Vec<String>,
}

/// One keyboard layout: XNA key names per binding (several keys allowed).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct KeyboardConfig {
    pub left: Vec<String>,
    pub right: Vec<String>,
    pub up: Vec<String>,
    pub down: Vec<String>,
    pub jump: Vec<String>,
    pub shoot: Vec<String>,
    pub alt_shoot: Vec<String>,
    pub dodge: Vec<String>,
    pub arrows: Vec<String>,
    pub menu_alt: Vec<String>,
    pub start: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct SaveData {
    pub path: PathBuf,
    /// Game version that last wrote the file ("1.3.3.3").
    pub version: String,
    pub options: SaveOptions,
    pub stats: SaveStats,
    pub quest: Vec<QuestTowerStats>,
    pub dark_world: Vec<DarkWorldTowerStats>,
    /// `trials[level][tier]`.
    pub trials: Vec<Vec<TrialsLevelStats>>,
    pub unlocks: SaveUnlocks,
    /// Up to four layouts; `None` = slot not configured.
    pub keyboard: Vec<Option<KeyboardConfig>>,
}

fn is_nil(n: Node) -> bool {
    n.attribute((XSI, "nil")).is_some_and(|v| v == "true")
}

fn ch<'a, 'i>(n: Node<'a, 'i>, k: &str) -> Option<Node<'a, 'i>> {
    xml::child(n, k).filter(|c| !is_nil(*c))
}

fn bool_of(n: Node, k: &str) -> bool {
    xml::child_bool(n, k).unwrap_or(false)
}

fn int_of(n: Node, k: &str) -> i64 {
    xml::child_parse(n, k).unwrap_or(0)
}

fn uint_of(n: Node, k: &str) -> u64 {
    xml::child_parse(n, k).unwrap_or(0)
}

fn uints(n: Option<Node>) -> Vec<u64> {
    n.map(|n| {
        xml::elements(n)
            .map(|c| xml::inner_text(c).trim().parse().unwrap_or(0))
            .collect()
    })
    .unwrap_or_default()
}

fn strings(n: Option<Node>) -> Vec<String> {
    n.map(|n| {
        xml::elements(n)
            .map(|c| xml::inner_text(c).trim().to_owned())
            .collect()
    })
    .unwrap_or_default()
}

const ARCHERS: [&str; 9] = [
    "Green", "Blue", "Pink", "Orange", "White", "Yellow", "Cyan", "Purple", "Red",
];

fn kill_stats(n: Option<Node>) -> KillStats {
    let Some(n) = n else {
        return KillStats::default();
    };
    let mut k = KillStats {
        total: uint_of(n, "Kills"),
        team: uint_of(n, "TeamKills"),
        own: uint_of(n, "SelfKills"),
        environment: uint_of(n, "Environment"),
        ..Default::default()
    };
    for (i, a) in ARCHERS.iter().enumerate() {
        k.per_archer[i] = uint_of(n, a);
    }
    for c in xml::elements(n) {
        let name = c.tag_name().name();
        if name.ends_with("Kills") && !matches!(name, "Kills" | "TeamKills" | "SelfKills") {
            k.by_cause.insert(
                name.to_owned(),
                xml::inner_text(c).trim().parse().unwrap_or(0),
            );
        }
    }
    k
}

impl SaveData {
    /// Parses a save file (read-only).
    pub fn load(path: &Path) -> Result<SaveData> {
        let file = XmlFile::read(path)?;
        let doc = file.doc()?;
        let root = doc.root_element();
        if root.tag_name().name() != "SaveData" {
            return Err(Error::format(path, "root is not <SaveData>"));
        }
        let o = ch(root, "Options");
        let options = o
            .map(|o| SaveOptions {
                fullscreen: bool_of(o, "Fullscreen"),
                music_volume: xml::child_f32(o, "MusicVolume").unwrap_or(0.8),
                sfx_volume: xml::child_f32(o, "SFXVolume").unwrap_or(0.8),
                gamepad_vibration: bool_of(o, "GamepadVibration"),
                gamepad_discovery: bool_of(o, "GamepadDiscovery"),
                allow_direct_input: bool_of(o, "AllowDirectInput"),
                hold_to_pause: bool_of(o, "HoldToPause"),
                show_tips: bool_of(o, "ShowTips"),
                replay_mode: xml::child_text(o, "ReplayMode")
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
                can_skip_replays: bool_of(o, "CanSkipReplays"),
                show_input_during_replays: bool_of(o, "ShowInputDuringReplays"),
                remove_slow_motion_wave_effect: bool_of(o, "RemoveSlowMotionWaveEffect"),
                remove_scroll_effects: bool_of(o, "RemoveScrollEffects"),
                remove_screen_flash_effects: bool_of(o, "RemoveScreenFlashEffects"),
                vertical_sync: bool_of(o, "VerticalSync"),
                dev_console: bool_of(o, "DevConsole"),
            })
            .unwrap_or_default();
        let stats = ch(root, "Stats")
            .map(|s| SaveStats {
                time_played_ticks: int_of(s, "TimePlayed"),
                kills: kill_stats(ch(s, "Kills")),
                deaths: kill_stats(ch(s, "Deaths")),
                wins: uints(ch(s, "Wins")),
                awards: ch(s, "Awards")
                    .map(|a| {
                        xml::elements(a)
                            .map(|r| {
                                if is_nil(r) {
                                    Vec::new()
                                } else {
                                    uints(Some(r))
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                versus_tip_matches: int_of(s, "VersusTipMatches"),
                variant_presets: ch(s, "VariantPresets")
                    .map(|p| {
                        xml::elements(p)
                            .map(|v| (!is_nil(v)).then(|| strings(ch(v, "Variants"))))
                            .collect()
                    })
                    .unwrap_or_default(),
                versus_tower_plays: uints(ch(s, "VersusTowerPlays")),
                times_launched: int_of(s, "TimesLaunched"),
                total_versus_kills: int_of(s, "TotalVersusKills"),
                matches_played: int_of(s, "MatchesPlayed"),
                rounds_played: int_of(s, "RoundsPlayed"),
                arrows_shot: int_of(s, "ArrowsShot"),
                arrows_collected: int_of(s, "ArrowsCollected"),
                arrows_caught: int_of(s, "ArrowsCaught"),
                treasures_taken: int_of(s, "TreasuresTaken"),
                dodges: int_of(s, "Dodges"),
                jumps: int_of(s, "Jumps"),
                versus_random_plays: int_of(s, "VersusRandomPlays"),
            })
            .unwrap_or_default();
        let list = |path: &[&str]| -> Vec<Node> {
            let mut n = Some(root);
            for p in path {
                n = n.and_then(|n| ch(n, p));
            }
            n.map(|n| xml::elements(n).collect()).unwrap_or_default()
        };
        let quest = list(&["Quest", "Towers"])
            .into_iter()
            .map(|q| QuestTowerStats {
                revealed: bool_of(q, "Revealed"),
                completed_normal: bool_of(q, "CompletedNormal"),
                completed_hardcore: bool_of(q, "CompletedHardcore"),
                completed_no_deaths: bool_of(q, "CompletedNoDeaths"),
                best_1p_ticks: int_of(q, "Best1PTime"),
                best_2p_ticks: int_of(q, "Best2PTime"),
                total_deaths: int_of(q, "TotalDeaths"),
                total_attempts: int_of(q, "TotalAttempts"),
            })
            .collect();
        let dark_world = list(&["DarkWorld", "Towers"])
            .into_iter()
            .map(|d| DarkWorldTowerStats {
                revealed: bool_of(d, "Revealed"),
                completed_normal: bool_of(d, "CompletedNormal"),
                completed_hardcore: bool_of(d, "CompletedHardcore"),
                completed_legendary: bool_of(d, "CompletedLegendary"),
                earned_eye: bool_of(d, "EarnedEye"),
                earned_gold_eye: bool_of(d, "EarnedGoldEye"),
                best_ticks: [1, 2, 3, 4].map(|p| int_of(d, &format!("Best{p}PTime"))),
                most_curses: [1, 2, 3, 4].map(|p| int_of(d, &format!("Most{p}PCurses"))),
                deaths: int_of(d, "Deaths"),
                attempts: int_of(d, "Attempts"),
            })
            .collect();
        let trials = list(&["Trials", "Levels"])
            .into_iter()
            .map(|lv| {
                xml::elements(lv)
                    .map(|t| TrialsLevelStats {
                        attempts: int_of(t, "Attempts"),
                        unlocked_gold: bool_of(t, "UnlockedGold"),
                        unlocked_diamond: bool_of(t, "UnlockedDiamond"),
                        unlocked_dev_time: bool_of(t, "UnlockedDevTime"),
                        best_time_ticks: int_of(t, "BestTime"),
                    })
                    .collect()
            })
            .collect();
        let unlocks = ch(root, "Unlocks")
            .map(|u| SaveUnlocks {
                moonstone: bool_of(u, "Moonstone"),
                sunken_city: bool_of(u, "SunkenCity"),
                tower_forge: bool_of(u, "TowerForge"),
                ascension: bool_of(u, "Ascension"),
                white_archer_revealed: bool_of(u, "WhiteArcherRevealed"),
                yellow_gems_found: ch(u, "YellowGemsFound")
                    .map(|g| {
                        xml::elements(g)
                            .map(|p| (int_of(p, "X") as i32, int_of(p, "Y") as i32))
                            .collect()
                    })
                    .unwrap_or_default(),
                yellow_archer_revealed: bool_of(u, "YellowArcherRevealed"),
                purple_archer_revealed: bool_of(u, "PurpleArcherRevealed"),
                white_archer_intro: bool_of(u, "WhiteArcherIntro"),
                yellow_archer_intro: bool_of(u, "YellowArcherIntro"),
                cyan_archer_intro: bool_of(u, "CyanArcherIntro"),
                purple_archer_intro: bool_of(u, "PurpleArcherIntro"),
                ascension_box_open: bool_of(u, "AscensionBoxOpen"),
                gunn_style: bool_of(u, "GunnStyle"),
                new_variants: strings(ch(u, "NewVariants")),
            })
            .unwrap_or_default();
        let keyboard = ch(root, "Keyboard")
            .map(|k| {
                xml::elements(k)
                    .map(|c| {
                        (!is_nil(c)).then(|| {
                            let keys = |b: &str| strings(ch(c, b));
                            KeyboardConfig {
                                left: keys("Left"),
                                right: keys("Right"),
                                up: keys("Up"),
                                down: keys("Down"),
                                jump: keys("Jump"),
                                shoot: keys("Shoot"),
                                alt_shoot: keys("AltShoot"),
                                dodge: keys("Dodge"),
                                arrows: keys("Arrows"),
                                menu_alt: keys("MenuAlt"),
                                start: keys("Start"),
                            }
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(SaveData {
            path: path.to_owned(),
            version: xml::child_text(root, "Version")
                .unwrap_or_default()
                .trim()
                .to_owned(),
            options,
            stats,
            quest,
            dark_world,
            trials,
            unlocks,
            keyboard,
        })
    }

    /// The most recently written of the places the game keeps its save:
    /// the per-user data folder (`$XDG_DATA_HOME/TowerFall`, macOS
    /// `~/Library/Application Support/TowerFall`) and the install folder,
    /// under either spelling (`tf_saveData` / `tf_savedata`).
    pub fn find(inst: &GameInstall) -> Option<PathBuf> {
        let mut dirs = vec![inst.root.clone()];
        let home = std::env::var_os("HOME").map(PathBuf::from);
        if let Some(x) = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".local/share")))
        {
            dirs.push(x.join("TowerFall"));
        }
        if let Some(h) = &home {
            dirs.push(h.join("Library/Application Support/TowerFall"));
        }
        dirs.iter()
            .flat_map(|d| ["tf_saveData", "tf_savedata"].map(|n| d.join(n)))
            .filter(|p| p.is_file())
            .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{installs, require};

    #[test]
    fn reads_the_steam_save() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let path = require!(steam.savedata.as_ref(), "tf_savedata");
        let before = std::fs::metadata(path).unwrap().modified().unwrap();
        let s = SaveData::load(path).unwrap();
        assert!(!s.version.is_empty());
        assert!((0.0..=1.0).contains(&s.options.music_volume));
        assert!(["UseGPU", "UseCPU", "Off"].contains(&s.options.replay_mode.as_str()));
        assert_eq!(s.stats.kills.per_archer.len(), 9);
        assert_eq!(s.stats.wins.len(), 9);
        assert_eq!(s.stats.awards.len(), 70);
        assert!(s.stats.awards.iter().all(|a| a.len() == 9));
        assert_eq!(s.stats.variant_presets.len(), 5);
        assert_eq!(s.stats.versus_tower_plays.len(), 16);
        assert_eq!(s.quest.len(), 14);
        assert_eq!(s.dark_world.len(), 5);
        assert_eq!(s.trials.len(), 16);
        assert!(s.trials.iter().all(|t| t.len() == 3));
        assert_eq!(s.keyboard.len(), 4);
        assert!(s.stats.kills.by_cause.contains_key("ArrowKills"));
        let cause_sum: u64 = s.stats.kills.by_cause.values().sum();
        assert!(cause_sum <= s.stats.kills.total * 2 + 10_000, "{cause_sum}");
        if let Some(k) = s.keyboard.iter().flatten().next() {
            assert!(!k.jump.is_empty() && !k.start.is_empty());
        }
        // Read-only: the file is untouched.
        assert_eq!(std::fs::metadata(path).unwrap().modified().unwrap(), before);
        assert!(SaveData::find(steam).is_some());
        eprintln!(
            "save {} v{}: {:.0} h played, {} matches, {} rounds",
            path.display(),
            s.version,
            ticks_to_seconds(s.stats.time_played_ticks) / 3600.0,
            s.stats.matches_played,
            s.stats.rounds_played
        );
    }
}
