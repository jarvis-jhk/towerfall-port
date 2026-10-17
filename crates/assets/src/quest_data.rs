//! Quest wave scripts (`Levels/Quest/NNdata.xml`). See `docs/formats/levels.md`.

use crate::error::{Error, Result};
use crate::xml::{self, XmlFile};
use roxmltree::Node;
use std::collections::BTreeMap;
use std::path::Path;

/// `[a*b]Name` → `a` enemies solo, `b` with 2+ players; `[n]Name` → `n`
/// either way; `Name` → 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnemyCount {
    pub enemy: String,
    pub solo: u32,
    pub coop: u32,
}

impl EnemyCount {
    pub fn parse(s: &str) -> std::result::Result<EnemyCount, String> {
        let s = s.trim();
        if let Some(rest) = s.strip_prefix('[') {
            let close = rest
                .find(']')
                .ok_or_else(|| format!("unclosed [ in {s:?}"))?;
            let inner = &rest[..close];
            let enemy = rest[close + 1..].to_owned();
            let num = |t: &str| {
                t.trim()
                    .parse::<u32>()
                    .map_err(|_| format!("bad count in {s:?}"))
            };
            let (solo, coop) = match inner.split_once('*') {
                Some((a, b)) => (num(a)?, num(b)?),
                None => {
                    let n = num(inner)?;
                    (n, n)
                }
            };
            Ok(EnemyCount { enemy, solo, coop })
        } else {
            Ok(EnemyCount {
                enemy: s.to_owned(),
                solo: 1,
                coop: 1,
            })
        }
    }

    pub fn count(&self, players: u32) -> u32 {
        if players == 1 { self.solo } else { self.coop }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuestGroup {
    /// Frames after the previous group (attribute `delay`, default 0).
    pub delay: i32,
    /// Only with exactly two players.
    pub coop: bool,
    /// Only with one player.
    pub solo: bool,
    /// Spawner names (`Spawner` entities' `name`).
    pub spawns: Vec<String>,
    pub enemies: Vec<EnemyCount>,
    pub treasure: Vec<String>,
    pub big_treasure: Vec<String>,
    /// Reaper spawn index, -1 = none.
    pub reaper: i32,
    pub jester: bool,
    /// All attributes as written (some files carry unused ones like `max`).
    pub attrs: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuestWave {
    pub dark: bool,
    pub slow: bool,
    pub scroll: bool,
    /// Floor miasma groups raised for this wave.
    pub floors: Option<Vec<i32>>,
    pub groups: Vec<QuestGroup>,
}

/// A step of a gauntlet quest (`<data gauntlet="true">`): the children of
/// `<normal>`/`<hardcore>` in order.
#[derive(Clone, Debug, PartialEq)]
pub enum GauntletStep {
    /// Spawn these enemies (shuffled) from the portals, at most `max` alive
    /// at once (default 2); `dark` turns the lights off.
    Group {
        enemies: Vec<EnemyCount>,
        max: i32,
        dark: bool,
    },
    Treasure(String),
    BigTreasure(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuestData {
    /// `gauntlet="true"` on the root: `*_gauntlet` are used, not the waves.
    pub gauntlet: bool,
    pub normal: Vec<QuestWave>,
    pub hardcore: Vec<QuestWave>,
    pub normal_gauntlet: Vec<GauntletStep>,
    pub hardcore_gauntlet: Vec<GauntletStep>,
}

fn gauntlet(path: &Path, n: Option<Node>) -> Result<Vec<GauntletStep>> {
    let Some(n) = n else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for c in xml::elements(n) {
        let text = xml::inner_text(c);
        match c.tag_name().name() {
            "group" => out.push(GauntletStep::Group {
                enemies: xml::parse_csv(&text)
                    .iter()
                    .map(|e| EnemyCount::parse(e))
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|e| Error::format(path, e))?,
                max: xml::attr_parse(c, "max").unwrap_or(2),
                dark: xml::attr_bool(c, "dark").unwrap_or(false),
            }),
            "treasure" => out.push(GauntletStep::Treasure(text.trim().to_owned())),
            "bigTreasure" => out.push(GauntletStep::BigTreasure(xml::parse_csv(&text))),
            _ => {}
        }
    }
    Ok(out)
}

fn waves(path: &Path, n: Option<Node>) -> Result<Vec<QuestWave>> {
    let Some(n) = n else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for w in xml::descendants(n, "wave") {
        let mut groups = Vec::new();
        for g in xml::descendants(w, "group") {
            let csv = |k: &str| {
                xml::child_text(g, k)
                    .map(|t| xml::parse_csv(&t))
                    .unwrap_or_default()
            };
            let enemies = csv("enemies")
                .iter()
                .map(|e| EnemyCount::parse(e))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::format(path, e))?;
            groups.push(QuestGroup {
                delay: xml::attr_parse(g, "delay").unwrap_or(0),
                coop: xml::attr_bool(g, "coop").unwrap_or(false),
                solo: xml::attr_bool(g, "solo").unwrap_or(false),
                spawns: csv("spawns"),
                enemies,
                treasure: csv("treasure"),
                big_treasure: csv("bigTreasure"),
                reaper: xml::child_parse(g, "reaper").unwrap_or(-1),
                jester: xml::child_bool(g, "jester").unwrap_or(false),
                attrs: g
                    .attributes()
                    .map(|a| (a.name().to_owned(), a.value().to_owned()))
                    .collect(),
            });
        }
        let floors = match xml::child_text(w, "floors") {
            Some(t) => Some(xml::parse_csv_ints(&t).map_err(|e| Error::format(path, e))?),
            None => None,
        };
        out.push(QuestWave {
            dark: xml::attr_bool(w, "dark").unwrap_or(false),
            slow: xml::attr_bool(w, "slow").unwrap_or(false),
            scroll: xml::attr_bool(w, "scroll").unwrap_or(false),
            floors,
            groups,
        });
    }
    Ok(out)
}

impl QuestData {
    pub fn load(path: &Path) -> Result<QuestData> {
        let file = XmlFile::read(path)?;
        let doc = file.doc()?;
        let root = doc.root_element();
        let is_gauntlet = xml::attr_bool(root, "gauntlet").unwrap_or(false);
        let (normal, hardcore) = (xml::child(root, "normal"), xml::child(root, "hardcore"));
        Ok(if is_gauntlet {
            QuestData {
                gauntlet: true,
                normal: Vec::new(),
                hardcore: Vec::new(),
                normal_gauntlet: gauntlet(path, normal)?,
                hardcore_gauntlet: gauntlet(path, hardcore)?,
            }
        } else {
            QuestData {
                gauntlet: false,
                normal: waves(path, normal)?,
                hardcore: waves(path, hardcore)?,
                normal_gauntlet: Vec::new(),
                hardcore_gauntlet: Vec::new(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enemy_counts() {
        assert_eq!(
            EnemyCount::parse("[2*4]Slime").unwrap(),
            EnemyCount {
                enemy: "Slime".into(),
                solo: 2,
                coop: 4
            }
        );
        assert_eq!(EnemyCount::parse("[3]Bat").unwrap().count(2), 3);
        assert_eq!(EnemyCount::parse("Ghost").unwrap().count(1), 1);
    }
}
