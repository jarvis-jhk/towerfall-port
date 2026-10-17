//! `MatchSettings` (versus.md §3, §11, §15): what all peers agree on before
//! a Versus match starts.

use crate::checksum::{Hasher, StableHash};
use crate::variants::{Variant, VariantSet};
use crate::world::MAX_PLAYERS;

/// The playable Versus modes (`Modes` 3..5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    LastManStanding,
    Headhunters,
    TeamDeathmatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchLength {
    Instant,
    Quick,
    Standard,
    Epic,
}

impl MatchLength {
    pub fn multiplier(self) -> f32 {
        match self {
            MatchLength::Instant => 0.01,
            MatchLength::Quick => 0.5,
            MatchLength::Standard => 1.0,
            MatchLength::Epic => 2.0,
        }
    }
}

/// One joined slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotSettings {
    /// Index into `Content::archers`.
    pub archer: u16,
    /// Team index (0 = Blue, 1 = Red, 2.. extra teams); only used in Team Deathmatch.
    pub team: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchSettings {
    pub mode: Mode,
    pub length: MatchLength,
    pub slots: [Option<SlotSettings>; MAX_PLAYERS],
    /// Number of teams `T` in Team Deathmatch (2..6).
    pub teams: u8,
    pub variants: VariantSet,
    /// The tower's levels as indices into `Content::levels`, in file order.
    pub levels: Vec<usize>,
    /// `tower.xml` `fixedFirst`: the first eligible level opens the match.
    pub fixed_first: bool,
    /// The match seed (all peers share it).
    pub seed: i32,
    /// Show the round-0 tower card (waits for a Confirm press).
    pub tower_card: bool,
    /// The tower's treasure mask / arrow rate and unlocks.
    pub items: crate::pickups::ItemSetup,
    /// `RandomLevelSeed`: the Cataclysm layout seed of this match (the
    /// app builds `Content` with [`MatchSettings::procedural_key`]; all
    /// peers must agree, so it is part of the checksum).
    pub procedural_seed: i32,
}

/// The layout seed when nobody picked one.
pub const DEFAULT_PROCEDURAL_SEED: i32 = 123_456_789;

impl MatchSettings {
    /// A match on `levels` with `n` players (slot i plays archer i).
    pub fn new(mode: Mode, n: usize, levels: Vec<usize>) -> MatchSettings {
        let mut slots = [None; MAX_PLAYERS];
        for (i, s) in slots.iter_mut().enumerate().take(n.min(MAX_PLAYERS)) {
            *s = Some(SlotSettings {
                archer: i as u16,
                team: (i % 2) as u8,
            });
        }
        MatchSettings {
            mode,
            length: MatchLength::Standard,
            slots,
            teams: 2,
            variants: VariantSet::default(),
            levels,
            fixed_first: false,
            seed: 0,
            tower_card: true,
            items: crate::pickups::ItemSetup::default(),
            procedural_seed: DEFAULT_PROCEDURAL_SEED,
        }
    }

    /// What the procedural (Cataclysm) layouts depend on: the seed and
    /// whether team spawns are kept free of chests (Team Deathmatch).
    pub fn procedural_key(&self) -> (i32, bool) {
        (self.procedural_seed, self.team_mode())
    }

    pub fn team_mode(&self) -> bool {
        self.mode == Mode::TeamDeathmatch
    }

    pub fn joined(&self, p: usize) -> bool {
        self.slots.get(p).is_some_and(|s| s.is_some())
    }

    /// Bit mask of joined slots.
    pub fn joined_mask(&self) -> u16 {
        (0..MAX_PLAYERS)
            .filter(|&p| self.joined(p))
            .fold(0, |m, p| m | 1 << p)
    }

    /// `PlayerAmount`.
    pub fn player_amount(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// Score index of slot `p`: the slot (FFA) or the team (team mode).
    pub fn score_index(&self, p: usize) -> usize {
        if self.team_mode() {
            self.slots[p].map(|s| s.team as usize).unwrap_or(0)
        } else {
            p
        }
    }

    /// Number of score entries.
    pub fn score_count(&self) -> usize {
        if self.team_mode() {
            self.teams as usize
        } else {
            MAX_PLAYERS
        }
    }

    /// Joined slots on team `t`.
    pub fn team_size(&self, t: u8) -> usize {
        self.slots.iter().flatten().filter(|s| s.team == t).count()
    }

    /// `GetMaxTeamSize`.
    pub fn max_team_size(&self) -> usize {
        if !self.team_mode() {
            return 1;
        }
        (0..self.teams)
            .map(|t| self.team_size(t))
            .max()
            .unwrap_or(0)
    }

    /// `GetPlayerTeamSize(p)`.
    pub fn player_team_size(&self, p: usize) -> usize {
        match (self.team_mode(), self.slots[p]) {
            (true, Some(s)) => self.team_size(s.team),
            _ => 1,
        }
    }

    /// `IsFriendly(a, b)`.
    pub fn is_friendly(&self, a: i32, b: i32) -> bool {
        if a < 0 || b < 0 {
            return false;
        }
        if self.team_mode() {
            self.score_index(a as usize) == self.score_index(b as usize)
        } else {
            a == b
        }
    }

    /// `GoalScore` (versus.md §3.2 + §15.5 for 9-12 players).
    pub fn goal_score(&self) -> i32 {
        let n = self.player_amount();
        let base = match self.mode {
            Mode::LastManStanding => match n {
                0..=2 => 5,
                3 => 4,
                _ => 3,
            },
            Mode::Headhunters => match n {
                0..=2 => 5,
                3 => 8,
                4..=6 => 10,
                7..=8 => 12,
                _ => 14,
            },
            Mode::TeamDeathmatch => 5,
        };
        crate::math::ceil(base as f32 * self.length.multiplier()) as i32
    }

    /// `AutobalanceLosingAmount`.
    pub fn autobalance_losing_amount(&self) -> i32 {
        if self.mode == Mode::LastManStanding && self.player_amount() > 2 {
            2
        } else {
            3
        }
    }

    pub fn variant(&self, v: Variant, p: i32) -> bool {
        self.variants.get(v, p)
    }

    /// Start-with-arrows type for slot `p` (versus.md §4.6), `None` = the
    /// round's random type.
    pub fn start_arrow_type(&self, p: i32) -> Option<crate::quiver::ArrowType> {
        use crate::quiver::ArrowType as A;
        let order = [
            (Variant::StartWithBombArrows, A::Bomb),
            (Variant::StartWithLaserArrows, A::Laser),
            (Variant::StartWithBrambleArrows, A::Bramble),
            (Variant::StartWithDrillArrows, A::Drill),
            (Variant::StartWithBoltArrows, A::Bolt),
            (Variant::StartWithSuperBombArrows, A::SuperBomb),
            (Variant::StartWithToyArrows, A::Toy),
            (Variant::StartWithFeatherArrows, A::Feather),
            (Variant::StartWithTriggerArrows, A::Trigger),
            (Variant::StartWithPrismArrows, A::Prism),
        ];
        for (v, t) in order {
            if self.variant(v, p) {
                return Some(t);
            }
        }
        if self.variant(Variant::StartWithRandomArrows, p) {
            return None;
        }
        Some(A::Normal)
    }
}

impl StableHash for MatchSettings {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u8(self.mode as u8);
        h.u8(self.length as u8);
        for s in &self.slots {
            match s {
                None => h.u8(0),
                Some(s) => {
                    h.u8(1);
                    h.u32(s.archer as u32);
                    h.u8(s.team);
                }
            }
        }
        h.u8(self.teams);
        self.variants.stable_hash(h);
        h.u32(self.levels.len() as u32);
        h.i32(self.seed);
        h.i32(self.procedural_seed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_scores() {
        let g = |mode, n, len| {
            let mut s = MatchSettings::new(mode, n, vec![0]);
            s.length = len;
            s.goal_score()
        };
        use MatchLength::*;
        use Mode::*;
        assert_eq!(g(LastManStanding, 2, Standard), 5);
        assert_eq!(g(LastManStanding, 4, Epic), 6);
        assert_eq!(g(Headhunters, 3, Quick), 4);
        assert_eq!(g(Headhunters, 4, Standard), 10);
        assert_eq!(g(Headhunters, 8, Standard), 12);
        assert_eq!(g(Headhunters, 12, Standard), 14);
        assert_eq!(g(TeamDeathmatch, 6, Epic), 10);
        assert_eq!(g(Headhunters, 4, Instant), 1);
    }
}
