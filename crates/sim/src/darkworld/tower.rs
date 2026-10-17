//! Dark World tower data (darkworld.md §2): levels per difficulty, enemy
//! sets with their weights and the time budget, from tfp-assets'
//! `towers::DarkWorldTower`.

use crate::enemies::EnemyType;

/// Which portals may spawn an enemy (darkworld.md §2.1 `type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortalKind {
    All,
    Ground,
    Air,
    Nodes,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DwSpawn {
    pub ty: EnemyType,
    pub portal: PortalKind,
    pub delay: i32,
    pub difficulty: i32,
    pub weight: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DwLevel {
    /// Index into `Content::levels` (the arena).
    pub arena: usize,
    pub difficulty: i32,
    pub waves: i32,
    /// Index into `DwTower::sets` (normal levels).
    pub enemy_set: usize,
    pub delay_multiplier: f32,
    /// Raw treasure tokens with their `+`/`-` prefixes.
    pub treasure: Vec<String>,
    /// Boss id 0..3 for a boss level.
    pub boss: Option<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DwTower {
    /// 0 Amaranth, 1 Dreadwood, 2 Darkfang, 3 Cataclysm, 4 Dark Gauntlet.
    pub id: u8,
    pub name: String,
    pub theme: String,
    pub time_base: i32,
    pub time_add: i32,
    pub sets: Vec<(String, Vec<DwSpawn>)>,
    /// Normal, Hardcore, Legendary.
    pub levels: [Vec<DwLevel>; 3],
}

impl DwTower {
    /// Build from the asset reader's tower; `first_arena` is the index of the
    /// tower's first arena in `Content::levels` (arenas in file order).
    pub fn from_assets(
        id: u8,
        t: &tfp_assets::towers::DarkWorldTower,
        first_arena: usize,
    ) -> DwTower {
        let mut sets: Vec<(String, Vec<DwSpawn>)> = t
            .enemy_sets
            .iter()
            .map(|(name, list)| {
                let spawns = list
                    .iter()
                    .filter_map(|e| {
                        Some(DwSpawn {
                            ty: enemy_by_name(&e.enemy)?,
                            portal: match e.portal_type.as_str() {
                                "Ground" => PortalKind::Ground,
                                "Air" => PortalKind::Air,
                                "Nodes" => PortalKind::Nodes,
                                _ => PortalKind::All,
                            },
                            delay: e.delay,
                            difficulty: e.difficulty,
                            weight: e.weight,
                        })
                    })
                    .collect();
                (name.clone(), spawns)
            })
            .collect();
        if sets.is_empty() {
            sets.push(("Normal".into(), vec![]));
        }
        let set_index = |n: &str| sets.iter().position(|(s, _)| s == n).unwrap_or(0);
        let conv = |l: &tfp_assets::towers::DarkWorldLevel| DwLevel {
            arena: first_arena + l.file.max(0) as usize,
            difficulty: l.difficulty,
            waves: l.waves.max(1),
            enemy_set: set_index(&l.enemy_set),
            delay_multiplier: l.delay_multiplier,
            treasure: l.treasure.clone(),
            boss: l.boss.map(|b| b as u8),
        };
        let levels = [
            t.normal.iter().map(conv).collect(),
            t.hardcore.iter().map(conv).collect(),
            t.legendary.iter().map(conv).collect(),
        ];
        DwTower {
            id,
            name: t.dir_name.clone(),
            theme: t.theme.clone(),
            time_base: t.time_base,
            time_add: t.time_add,
            sets,
            levels,
        }
    }
}

/// Spawn-table names of the Dark World sets (`enemies.md` §2).
pub fn enemy_by_name(n: &str) -> Option<EnemyType> {
    EnemyType::from_name(n.trim())
}

/// Treasure tokens for `n` players (darkworld.md §2.1 prefixes): `p` leading
/// `+` = from `max(p, 1)` players, `m` `-` = up to `m` players (none: no
/// upper bound, also beyond 4 players).
pub fn treasure_for(tokens: &[String], n: usize) -> Vec<String> {
    tokens
        .iter()
        .filter_map(|t| {
            let t = t.trim();
            let plus = t.chars().take_while(|&c| c == '+').count();
            let rest = &t[plus..];
            let minus = rest.chars().take_while(|&c| c == '-').count();
            let name = &rest[minus..];
            let ok = n >= plus.max(1) && (minus == 0 || n <= minus);
            (ok && !name.is_empty()).then(|| name.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treasure_prefixes() {
        let t: Vec<String> = ["Shield", "++Shield", "+++Wings", "++++Arrows", "-Mirror"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(treasure_for(&t, 1), vec!["Shield", "Mirror"]);
        assert_eq!(treasure_for(&t, 2), vec!["Shield", "Shield"]);
        assert_eq!(treasure_for(&t, 3), vec!["Shield", "Shield", "Wings"]);
        assert_eq!(treasure_for(&t, 4).len(), 4);
        assert_eq!(treasure_for(&t, 8).len(), 4, "no upper bound without '-'");
    }
}
