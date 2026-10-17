//! Which level each round plays (`VersusLevelSystem`, versus.md §4.3 and
//! levels.md §9.1), with the 12-player spawn tiers of versus.md §15.3.

use crate::content::{Content, LevelDef};
use crate::math::Vec2;
use crate::rng::Rng;
use crate::versus::settings::MatchSettings;
use crate::versus::spawn;

/// Where a level's spawn points come from for this match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnTier {
    /// Authored `PlayerSpawn` (FFA) or `TeamSpawnA/B` (two teams).
    Authored,
    /// `PlayerSpawn ∪ TeamSpawnA ∪ TeamSpawnB`.
    Pool,
    /// The pool plus derived standing positions.
    Derived,
}

/// The de-duplicated authored pool of a level.
pub fn authored_pool(l: &LevelDef) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = vec![];
    for p in l
        .player_spawns
        .iter()
        .chain(&l.team_spawns_a)
        .chain(&l.team_spawns_b)
    {
        if !out.contains(p) {
            out.push(*p);
        }
    }
    out
}

/// Does level `l` qualify in `tier` for `n` players / the match's teams?
pub fn qualifies(c: &Content, l: usize, set: &MatchSettings, n: usize, tier: SpawnTier) -> bool {
    let lvl = c.level(l);
    let two_team_authored = set.team_mode() && set.teams == 2;
    match tier {
        SpawnTier::Authored => {
            if two_team_authored {
                lvl.team_spawns_a.len().min(lvl.team_spawns_b.len()) >= set.max_team_size()
            } else if set.team_mode() {
                false
            } else {
                lvl.player_spawns.len() >= n
            }
        }
        SpawnTier::Pool => authored_pool(lvl).len() >= n,
        SpawnTier::Derived => spawn::derive_spawns(c, lvl, n, &mut Rng::new(0)).len() >= n,
    }
}

/// The eligible set `E` (file order) and the tier that produced it: the
/// first non-empty tier. Falls back to every level (derived spawns, reused
/// if short) so a match never fails to start.
pub fn eligible(c: &Content, set: &MatchSettings, n: usize) -> (Vec<usize>, SpawnTier) {
    for tier in [SpawnTier::Authored, SpawnTier::Pool, SpawnTier::Derived] {
        let e: Vec<usize> = set
            .levels
            .iter()
            .copied()
            .filter(|&l| qualifies(c, l, set, n, tier))
            .collect();
        if !e.is_empty() {
            return (e, tier);
        }
    }
    (set.levels.clone(), SpawnTier::Derived)
}

/// The level queue of a match.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelSystem {
    pub queue: Vec<usize>,
    pub last_level: Option<usize>,
    pub tier: Option<SpawnTier>,
}

impl LevelSystem {
    /// Pop the next round's level (rebuilding and shuffling the queue from the
    /// eligible set when it is empty).
    pub fn next(&mut self, c: &Content, set: &MatchSettings, n: usize, rng: &mut Rng) -> usize {
        let (e, tier) = eligible(c, set, n);
        self.tier = Some(tier);
        // Players may have left: drop queued levels that no longer qualify.
        self.queue.retain(|l| e.contains(l));
        if self.queue.is_empty() {
            let mut q = e.clone();
            if set.fixed_first && self.last_level.is_none() && !q.is_empty() {
                let first = q.remove(0);
                rng.shuffle(&mut q);
                q.insert(0, first);
            } else {
                rng.shuffle(&mut q);
                if q.len() > 1 && Some(q[0]) == self.last_level {
                    let f = q.remove(0);
                    q.push(f);
                }
            }
            self.queue = q;
        }
        let l = self.queue.remove(0);
        self.last_level = Some(l);
        l
    }
}
