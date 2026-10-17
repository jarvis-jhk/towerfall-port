//! Quest wave scripts (quest.md §4.1) as validated sim data, converted once
//! from `tfp_assets::quest_data::QuestData` and kept in `Content`.

use crate::enemies::EnemyType;
use tfp_assets::quest_data::{EnemyCount, GauntletStep, QuestData, QuestGroup, QuestWave};

/// `Name`, `[n]Name` or `[a*b]Name`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnemyToken {
    pub ty: EnemyType,
    pub solo: u32,
    pub coop: u32,
}

impl EnemyToken {
    /// Copies for `n` players (quest.md §4.2; §7 extrapolation for n > 2).
    pub fn count(&self, n: usize, extended: bool) -> u32 {
        match n {
            0 | 1 => self.solo,
            2 => self.coop,
            _ if extended && self.coop > self.solo => {
                self.coop + (n as u32 - 2) * (self.coop - self.solo)
            }
            _ => self.coop,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub delay: i32,
    pub coop: bool,
    pub solo: bool,
    pub spawns: Vec<String>,
    pub enemies: Vec<EnemyToken>,
    pub treasure: Vec<String>,
    pub big_treasure: Vec<String>,
    pub reaper: i32,
    pub jester: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Wave {
    pub dark: bool,
    pub slow: bool,
    pub scroll: bool,
    pub floors: Vec<i32>,
    pub groups: Vec<Group>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GStep {
    Group {
        enemies: Vec<EnemyToken>,
        max: i32,
        dark: bool,
    },
    Treasure(String),
    BigTreasure(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct QuestScript {
    pub name: String,
    pub gauntlet: bool,
    pub normal: Vec<Wave>,
    pub hardcore: Vec<Wave>,
    pub normal_gauntlet: Vec<GStep>,
    pub hardcore_gauntlet: Vec<GStep>,
}

fn tokens(list: &[EnemyCount]) -> Result<Vec<EnemyToken>, String> {
    list.iter()
        .map(|e| {
            let ty = EnemyType::from_name(&e.enemy)
                .ok_or_else(|| format!("unknown enemy {:?}", e.enemy))?;
            Ok(EnemyToken {
                ty,
                solo: e.solo,
                coop: e.coop,
            })
        })
        .collect()
}

fn group(g: &QuestGroup) -> Result<Group, String> {
    Ok(Group {
        delay: g.delay,
        coop: g.coop,
        solo: g.solo,
        spawns: g
            .spawns
            .iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect(),
        enemies: tokens(&g.enemies)?,
        treasure: g.treasure.clone(),
        big_treasure: g.big_treasure.clone(),
        reaper: g.reaper,
        jester: g.jester,
    })
}

fn waves(list: &[QuestWave]) -> Result<Vec<Wave>, String> {
    list.iter()
        .map(|w| {
            Ok(Wave {
                dark: w.dark,
                slow: w.slow,
                scroll: w.scroll,
                floors: w.floors.clone().unwrap_or_default(),
                groups: w.groups.iter().map(group).collect::<Result<_, _>>()?,
            })
        })
        .collect()
}

fn gauntlet(list: &[GauntletStep]) -> Result<Vec<GStep>, String> {
    list.iter()
        .map(|s| {
            Ok(match s {
                GauntletStep::Group { enemies, max, dark } => GStep::Group {
                    enemies: tokens(enemies)?,
                    max: *max,
                    dark: *dark,
                },
                GauntletStep::Treasure(t) => GStep::Treasure(t.clone()),
                GauntletStep::BigTreasure(t) => GStep::BigTreasure(t.clone()),
            })
        })
        .collect()
}

/// Test-script wave: portal names and (enemy name, count) pairs.
pub type SimpleWave<'a> = (&'a [&'a str], &'a [(&'a str, u32)]);

impl QuestScript {
    /// Convert and validate (unknown enemy names are rejected).
    pub fn from_data(name: &str, d: &QuestData) -> Result<QuestScript, String> {
        Ok(QuestScript {
            name: name.to_owned(),
            gauntlet: d.gauntlet,
            normal: waves(&d.normal)?,
            hardcore: waves(&d.hardcore)?,
            normal_gauntlet: gauntlet(&d.normal_gauntlet)?,
            hardcore_gauntlet: gauntlet(&d.hardcore_gauntlet)?,
        })
    }

    /// A one-wave script for tests: `enemies` from portals `spawns`.
    pub fn simple(waves: &[SimpleWave]) -> QuestScript {
        let w = waves
            .iter()
            .map(|(spawns, enemies)| Wave {
                dark: false,
                slow: false,
                scroll: false,
                floors: vec![],
                groups: vec![Group {
                    delay: 0,
                    coop: false,
                    solo: false,
                    spawns: spawns.iter().map(|s| s.to_string()).collect(),
                    enemies: enemies
                        .iter()
                        .map(|(n, k)| EnemyToken {
                            ty: EnemyType::from_name(n).expect("enemy name"),
                            solo: *k,
                            coop: *k,
                        })
                        .collect(),
                    treasure: vec![],
                    big_treasure: vec![],
                    reaper: -1,
                    jester: false,
                }],
            })
            .collect::<Vec<_>>();
        QuestScript {
            name: "test".into(),
            gauntlet: false,
            normal: w.clone(),
            hardcore: w,
            normal_gauntlet: vec![],
            hardcore_gauntlet: vec![],
        }
    }
}
