//! The tower index: every Versus, Quest, Dark World and Trials tower of an
//! install with its theme and level files, plus Workshop towers. Mirrors how
//! the game assembles them: tower descriptions live in `Content/Levels`, and
//! Dark World levels of the same tower directory in `DarkWorldContent/Levels`
//! are appended after the base ones. See `docs/formats/levels.md`.

use crate::error::{Error, Result};
use crate::install::GameInstall;
use crate::xml::{self, XmlFile};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mode {
    Versus,
    Quest,
    DarkWorld,
    Trials,
    Workshop,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VersusTower {
    /// Directory name, e.g. `00 - Sacred Ground`.
    pub dir_name: String,
    /// Theme id (`tower.xml` `<theme>`).
    pub theme: String,
    pub fixed_first: bool,
    pub procedural: bool,
    /// Treasure names as listed (each entry adds one lottery ticket).
    pub treasure: Option<Vec<String>>,
    pub special_arrow_rate: f32,
    pub arrow_shuffle: bool,
    /// Level files: base directory sorted, then the Dark World directory.
    pub levels: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuestTower {
    /// `name` attribute in `quest.xml` (`00`…).
    pub name: String,
    pub theme: String,
    pub level: PathBuf,
    pub data: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DarkWorldEnemy {
    pub enemy: String,
    /// All, Ground, Air, Nodes.
    pub portal_type: String,
    pub delay: i32,
    pub difficulty: i32,
    pub weight: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DarkWorldLevel {
    /// Index into the tower's level list.
    pub file: i32,
    pub difficulty: i32,
    pub waves: i32,
    pub enemy_set: String,
    pub delay_multiplier: f32,
    /// Raw treasure entries (`+`/`-` prefixes select player counts).
    pub treasure: Vec<String>,
    pub boss: Option<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DarkWorldTower {
    pub dir_name: String,
    pub theme: String,
    pub levels: Vec<PathBuf>,
    pub time_base: i32,
    pub time_add: i32,
    pub enemy_sets: BTreeMap<String, Vec<DarkWorldEnemy>>,
    pub normal: Vec<DarkWorldLevel>,
    pub hardcore: Vec<DarkWorldLevel>,
    pub legendary: Vec<DarkWorldLevel>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrialsLevel {
    pub tier: usize,
    pub index: usize,
    pub theme: String,
    pub path: PathBuf,
    pub arrows: i32,
    pub switch_timer: i32,
    /// Seconds: gold, diamond, dev.
    pub goals: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct TowerIndex {
    pub versus: Vec<VersusTower>,
    pub quest: Vec<QuestTower>,
    pub dark_world: Vec<DarkWorldTower>,
    pub trials: Vec<TrialsLevel>,
    pub workshop: Vec<PathBuf>,
}

fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn sorted_oels(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "oel"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn dir_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl TowerIndex {
    pub fn build(inst: &GameInstall) -> Result<TowerIndex> {
        let levels = inst.content.levels_dir();
        let dw_levels = inst.dark_world.as_ref().map(|d| d.levels_dir());
        let dw_dir = |sub: &str, name: &str| {
            dw_levels
                .as_ref()
                .map(|d| d.join(sub).join(name))
                .filter(|d| d.is_dir())
        };
        let mut idx = TowerIndex::default();

        for dir in sorted_dirs(&levels.join("Versus")) {
            let name = dir_name(&dir);
            let path = dir.join("tower.xml");
            let file = XmlFile::read(&path)?;
            let doc = file.doc()?;
            let root = doc.root_element();
            let treasure_el = xml::child(root, "treasure");
            let mut lv = sorted_oels(&dir);
            if let Some(d) = dw_dir("Versus", &name) {
                lv.extend(sorted_oels(&d));
            }
            idx.versus.push(VersusTower {
                theme: xml::child_text(root, "theme")
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
                fixed_first: xml::child(root, "fixedFirst").is_some(),
                procedural: xml::child(root, "procedural").is_some(),
                treasure: treasure_el.map(|t| xml::parse_csv(&xml::inner_text(t))),
                special_arrow_rate: treasure_el
                    .and_then(|t| xml::attr_f32(t, "arrows"))
                    .unwrap_or(0.6),
                arrow_shuffle: treasure_el
                    .and_then(|t| xml::attr_bool(t, "arrowShuffle"))
                    .unwrap_or(false),
                levels: lv,
                dir_name: name,
            });
        }

        let quest_xml = levels.join("Quest").join("quest.xml");
        if quest_xml.is_file() {
            let file = XmlFile::read(&quest_xml)?;
            let doc = file.doc()?;
            for l in xml::descendants(doc.root_element(), "level") {
                let name = l.attribute("name").unwrap_or("").to_owned();
                idx.quest.push(QuestTower {
                    theme: xml::child_text(l, "theme")
                        .unwrap_or_default()
                        .trim()
                        .to_owned(),
                    level: levels.join("Quest").join(format!("{name}.oel")),
                    data: levels.join("Quest").join(format!("{name}data.xml")),
                    name,
                });
            }
        }

        if inst.dark_world.is_some() {
            for dir in sorted_dirs(&levels.join("DarkWorld")) {
                idx.dark_world
                    .push(load_dark_world(&dir, dw_dir("DarkWorld", &dir_name(&dir)))?);
            }
        }

        let trials_xml = levels.join("Trials").join("trials.xml");
        if trials_xml.is_file() {
            let file = XmlFile::read(&trials_xml)?;
            let doc = file.doc()?;
            for (tier, t) in xml::descendants(doc.root_element(), "tier").enumerate() {
                for (index, l) in xml::descendants(t, "level").enumerate() {
                    let rel = l.attribute("path").unwrap_or("");
                    let mut path = levels.join("Trials").join(rel);
                    if !path.is_file()
                        && let Some(d) = &dw_levels
                    {
                        path = d.join("Trials").join(rel);
                    }
                    idx.trials.push(TrialsLevel {
                        tier,
                        index,
                        theme: xml::child_text(l, "theme")
                            .unwrap_or_default()
                            .trim()
                            .to_owned(),
                        arrows: xml::child_parse(l, "arrows").unwrap_or(3),
                        switch_timer: xml::child_parse(l, "switchTimer").unwrap_or(200),
                        goals: [
                            xml::child_f32(l, "gold").unwrap_or(0.0),
                            xml::child_f32(l, "diamond").unwrap_or(0.0),
                            xml::child_f32(l, "dev").unwrap_or(0.1),
                        ],
                        path,
                    });
                }
            }
        }

        if let Some(ws) = &inst.workshop {
            idx.workshop = crate::workshop::find_tower_files(ws);
        }
        Ok(idx)
    }

    /// Every level file referenced by the index (no Workshop).
    pub fn all_level_files(&self) -> Vec<(Mode, PathBuf)> {
        let mut v = Vec::new();
        for t in &self.versus {
            v.extend(t.levels.iter().map(|p| (Mode::Versus, p.clone())));
        }
        v.extend(self.quest.iter().map(|q| (Mode::Quest, q.level.clone())));
        for t in &self.dark_world {
            v.extend(t.levels.iter().map(|p| (Mode::DarkWorld, p.clone())));
        }
        v.extend(self.trials.iter().map(|t| (Mode::Trials, t.path.clone())));
        v
    }
}

fn load_dark_world(dir: &Path, dw: Option<PathBuf>) -> Result<DarkWorldTower> {
    let path = dir.join("tower.xml");
    let file = XmlFile::read(&path)?;
    let doc = file.doc()?;
    let root = doc.root_element();
    let mut levels = sorted_oels(dir);
    if let Some(d) = dw {
        levels.extend(sorted_oels(&d));
    }
    let time = xml::child(root, "time").ok_or_else(|| Error::format(&path, "no <time>"))?;
    let mut enemy_sets = BTreeMap::new();
    if let Some(e) = xml::child(root, "enemies") {
        for set in xml::descendants(e, "set") {
            let list = xml::descendants(set, "spawn")
                .map(|s| DarkWorldEnemy {
                    enemy: xml::child_text(s, "enemy")
                        .unwrap_or_default()
                        .trim()
                        .to_owned(),
                    portal_type: xml::child_text(s, "type")
                        .unwrap_or_default()
                        .trim()
                        .to_owned(),
                    delay: xml::child_parse(s, "delay").unwrap_or(0),
                    difficulty: xml::child_parse(s, "difficulty").unwrap_or(0),
                    weight: xml::child_f32(s, "weight").unwrap_or(0.0),
                })
                .collect();
            enemy_sets.insert(set.attribute("id").unwrap_or("").to_owned(), list);
        }
    }
    let set = |n: &str| -> Vec<DarkWorldLevel> {
        let Some(s) = xml::child(root, n) else {
            return Vec::new();
        };
        xml::descendants(s, "level")
            .map(|l| DarkWorldLevel {
                file: xml::child_parse(l, "file").unwrap_or(0),
                difficulty: xml::child_parse(l, "difficulty").unwrap_or(0),
                waves: xml::child_parse(l, "waves").unwrap_or(3),
                enemy_set: xml::child_text(l, "enemySet")
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
                delay_multiplier: xml::child_f32(l, "delayMultiplier").unwrap_or(1.0),
                treasure: xml::child_text(l, "treasure")
                    .map(|t| xml::parse_csv(&t))
                    .unwrap_or_default(),
                boss: xml::child_parse(l, "boss"),
            })
            .collect()
    };
    Ok(DarkWorldTower {
        dir_name: dir_name(dir),
        theme: xml::child_text(root, "theme")
            .unwrap_or_default()
            .trim()
            .to_owned(),
        levels,
        time_base: xml::child_parse(time, "base").unwrap_or(0),
        time_add: xml::child_parse(time, "add").unwrap_or(0),
        enemy_sets,
        normal: set("normal"),
        hardcore: set("hardcore"),
        legendary: set("legendary"),
    })
}

/// The seed the game uses for a Versus level: the sum of the UTF-16 code
/// units of its path as the game builds it (`Content/Levels/Versus/<dir>/<file>`
/// with the platform's separator — `/` on Linux/macOS, `\` on Windows — and
/// `DarkWorldContent/…` for expansion levels).
pub fn versus_load_seed(game_relative_path: &str) -> i32 {
    game_relative_path
        .encode_utf16()
        .fold(0i32, |a, c| a.wrapping_add(c as i32))
}

/// `Content/Levels/...` / `DarkWorldContent/Levels/...` path of `file`
/// relative to the install root, with `/` separators.
pub fn game_relative_path(inst: &GameInstall, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(&inst.root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_data::GameData;
    use crate::level::Level;
    use crate::quest_data::QuestData;
    use crate::testutil::installs;

    #[test]
    fn index_every_install() {
        for inst in installs().iter() {
            let idx = TowerIndex::build(inst).unwrap();
            let gd = GameData::load(&inst.content).unwrap();
            assert_eq!(idx.versus.len(), 16, "{}", inst.summary());
            for t in &idx.versus {
                assert!(
                    gd.theme(&t.theme).is_some(),
                    "{} theme {}",
                    t.dir_name,
                    t.theme
                );
                assert!(
                    !t.levels.is_empty() || inst.dark_world.is_none(),
                    "{}",
                    t.dir_name
                );
            }
            for q in &idx.quest {
                assert!(q.level.is_file() && q.data.is_file(), "{q:?}");
                let qd = QuestData::load(&q.data).unwrap();
                assert!(
                    !qd.normal.is_empty() || !qd.normal_gauntlet.is_empty(),
                    "{}",
                    q.name
                );
                assert!(
                    !qd.hardcore.is_empty() || !qd.hardcore_gauntlet.is_empty(),
                    "{}",
                    q.name
                );
            }
            for d in &idx.dark_world {
                for l in d.normal.iter().chain(&d.hardcore).chain(&d.legendary) {
                    assert!(
                        (l.file as usize) < d.levels.len(),
                        "{} file {}",
                        d.dir_name,
                        l.file
                    );
                    if l.boss.is_none() {
                        assert!(
                            d.enemy_sets.contains_key(&l.enemy_set),
                            "{} set {}",
                            d.dir_name,
                            l.enemy_set
                        );
                    }
                }
            }
            for t in &idx.trials {
                assert!(t.path.is_file(), "{t:?}");
            }
            let files = idx.all_level_files();
            for (_, f) in &files {
                Level::load(f).unwrap();
            }
            let versus: usize = idx.versus.iter().map(|t| t.levels.len()).sum();
            eprintln!(
                "{}: {} versus towers / {versus} levels, {} quest, {} dark world, {} trials, {} workshop; {} level files",
                inst.summary(),
                idx.versus.len(),
                idx.quest.len(),
                idx.dark_world.len(),
                idx.trials.len(),
                idx.workshop.len(),
                files.len()
            );
        }
    }

    #[test]
    fn seed_is_char_sum() {
        assert_eq!(versus_load_seed("ab"), 97 + 98);
    }
}
