//! What the menus produce: the front end's persistent `MatchSettings`
//! (versus.md 3) plus the rollcall roster, and its conversion to the sim's
//! [`tfp_sim::world::MatchSetup`].

use super::variants::{SLOTS, VariantSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    LastManStanding,
    HeadHunters,
    TeamDeathmatch,
}

impl Mode {
    pub const ALL: [Mode; 3] = [
        Mode::LastManStanding,
        Mode::HeadHunters,
        Mode::TeamDeathmatch,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Mode::LastManStanding => "LAST MAN STANDING",
            Mode::HeadHunters => "HEADHUNTERS",
            Mode::TeamDeathmatch => "TEAM DEATHMATCH",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Mode::LastManStanding => "gameModes/lastManStanding",
            Mode::HeadHunters => "gameModes/headhunters",
            Mode::TeamDeathmatch => "gameModes/teamDeathmatch",
        }
    }

    pub fn is_team(self) -> bool {
        self == Mode::TeamDeathmatch
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Length {
    Instant,
    Quick,
    Standard,
    Epic,
}

impl Length {
    pub const ALL: [Length; 4] = [
        Length::Instant,
        Length::Quick,
        Length::Standard,
        Length::Epic,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Length::Instant => "INSTANT MATCH",
            Length::Quick => "QUICK MATCH",
            Length::Standard => "STANDARD MATCH",
            Length::Epic => "EPIC MATCH",
        }
    }

    fn multiplier(self) -> f32 {
        match self {
            Length::Instant => 0.01,
            Length::Quick => 0.5,
            Length::Standard => 1.0,
            Length::Epic => 2.0,
        }
    }
}

/// Goal score (versus.md 3.2, extended to 12 per 15.5).
pub fn goal_score(mode: Mode, length: Length, players: usize) -> u32 {
    let base = match mode {
        Mode::LastManStanding => match players {
            0..=2 => 5,
            3 => 4,
            _ => 3,
        },
        Mode::HeadHunters => match players {
            0..=2 => 5,
            3 => 8,
            4..=6 => 10,
            7..=8 => 12,
            _ => 14,
        },
        Mode::TeamDeathmatch => 5,
    };
    (base as f32 * length.multiplier()).ceil() as u32
}

/// Team of a slot (sim order: Blue 0, Red 1, then Green, Yellow, Purple,
/// Orange for 3-6 teams, versus.md 15.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Team {
    #[default]
    Neutral,
    Blue,
    Red,
    Green,
    Yellow,
    Purple,
    Orange,
}

impl Team {
    pub const ALL: [Team; 6] = [
        Team::Blue,
        Team::Red,
        Team::Green,
        Team::Yellow,
        Team::Purple,
        Team::Orange,
    ];

    pub fn index(self) -> Option<u8> {
        Team::ALL.iter().position(|&t| t == self).map(|i| i as u8)
    }

    pub fn from_index(i: u8) -> Team {
        Team::ALL.get(i as usize).copied().unwrap_or(Team::Neutral)
    }

    /// Colours A / B (versus.md 15.4).
    pub fn colors(self) -> (u32, u32) {
        match self {
            Team::Blue => (0x3CBCFC, 0xA4E4FC),
            Team::Red => (0xAD4126, 0xFF6338),
            Team::Green => (0x00B800, 0x45E045),
            Team::Yellow => (0xF2FF00, 0xFFFF66),
            Team::Purple => (0x7A42FF, 0xA77FFF),
            Team::Orange => (0xEF8C21, 0xFFB15E),
            Team::Neutral => (0xFFFFFF, 0xFFFFFF),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Team::Blue => "BLUE",
            Team::Red => "RED",
            Team::Green => "GREEN",
            Team::Yellow => "YELLOW",
            Team::Purple => "PURPLE",
            Team::Orange => "ORANGE",
            Team::Neutral => "",
        }
    }
}

/// Archer costume of a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Costume {
    #[default]
    Normal,
    Alt,
    Secret,
}

/// Who controls a slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owner {
    /// A local device (see `front::minput::Dev`).
    Local(super::minput::Dev),
    /// A remote peer's player (filled by `tfp-net` later; the lobby hook).
    #[allow(dead_code)] // The online lobby hook (tfp-net).
    Remote { peer: u64, name: String },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub owner: Option<Owner>,
    pub joined: bool,
    pub archer: usize,
    pub costume: Costume,
    /// Owner display: device name and icon family.
    pub dev_name: String,
    pub icons: Option<super::minput::IconSet>,
}

/// Tower choice on the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TowerChoice {
    /// Index into the versus tower list.
    pub tower: usize,
    /// Picked by the RANDOM button.
    pub random: bool,
    /// Procedural towers: 9-digit seed.
    pub seed: u32,
}

/// Everything the front end decides for a versus match.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchSettings {
    pub mode: Mode,
    pub length: Length,
    pub variants: VariantSet,
    pub teams: [Team; SLOTS],
    pub tower: TowerChoice,
    /// `lastPlayers` of `CleanSettingsVersus`.
    pub last_players: usize,
    /// Team Deathmatch: number of teams (2..6, versus.md 15.4).
    pub team_count: u8,
}

impl Default for MatchSettings {
    fn default() -> Self {
        Self {
            mode: Mode::HeadHunters,
            length: Length::Standard,
            variants: VariantSet::default(),
            teams: [Team::Neutral; SLOTS],
            tower: TowerChoice::default(),
            last_players: 0,
            team_count: 2,
        }
    }
}

/// FFA limit: none up to 12 in our design (versus.md 15.2).
pub const FFA_MAX: usize = 12;

impl MatchSettings {
    /// `CanPlayThisMode`; `Err` = the red two-line warning.
    pub fn can_play(&self, players: usize) -> Result<(), [String; 2]> {
        if self.mode.is_team() && players < 3 {
            return Err([
                "TEAM DEATHMATCH MODE".into(),
                "REQUIRES 3 OR MORE ARCHERS".into(),
            ]);
        }
        if !self.mode.is_team() && players > FFA_MAX {
            return Err([
                format!("{} MODE", self.mode.name()),
                format!("REQUIRES {FFA_MAX} OR FEWER ARCHERS"),
            ]);
        }
        if players < 2 {
            return Err(["VERSUS".into(), "REQUIRES 2 OR MORE ARCHERS".into()]);
        }
        Ok(())
    }

    /// `CleanSettingsVersus` (versus.md 2.2 / 15.2).
    pub fn clean(&mut self, joined: &[bool]) {
        let n = joined.iter().filter(|j| **j).count();
        let last = self.last_players;
        match n {
            2 => {
                if self.mode == Mode::TeamDeathmatch
                    || (self.mode == Mode::HeadHunters && last != 2)
                {
                    self.mode = Mode::LastManStanding;
                }
            }
            3.. if self.mode == Mode::LastManStanding && last == 2 => {
                self.mode = Mode::HeadHunters;
            }
            _ => {}
        }
        if n != last {
            self.variants.clean(last, joined);
            for (i, t) in self.teams.iter_mut().enumerate() {
                if !joined[i] {
                    *t = Team::Neutral;
                }
            }
        }
        self.last_players = n;
    }

    /// Team select is complete: every joined slot assigned and every one
    /// of the `team_count` teams non-empty.
    pub fn teams_valid(&self, joined: &[bool]) -> bool {
        let mut count = [0usize; 6];
        for (i, &j) in joined.iter().enumerate() {
            if !j {
                continue;
            }
            match self.teams[i].index() {
                Some(t) if t < self.team_count => count[t as usize] += 1,
                _ => return false,
            }
        }
        count
            .iter()
            .take(self.team_count.clamp(2, 6) as usize)
            .all(|&c| c > 0)
    }

    /// The "balance" action: everybody into `team_count` teams as evenly as
    /// possible (sizes differ by at most 1), in slot order.
    pub fn balance_teams(&mut self, joined: &[bool]) {
        let t = self.team_count.clamp(2, 6);
        let mut k = 0u8;
        for (i, &j) in joined.iter().enumerate() {
            if j {
                self.teams[i] = Team::from_index(k % t);
                k += 1;
            } else {
                self.teams[i] = Team::Neutral;
            }
        }
    }
}

/// The finished front-end result: roster + settings.
#[derive(Clone, Debug, PartialEq)]
pub struct FrontEndSetup {
    pub slots: Vec<Slot>,
    pub settings: MatchSettings,
    /// Name of the chosen tower (theme name).
    pub tower_name: String,
}

impl FrontEndSetup {
    #[allow(dead_code)]
    pub fn joined(&self) -> Vec<bool> {
        self.slots.iter().map(|s| s.joined).collect()
    }

    /// The sim's setup for the first round. `level` = index into the sim
    /// content's level list (the caller maps tower -> level), `seed` the
    /// match seed.
    #[allow(dead_code)] // Used when the menus start a match (next step).
    pub fn to_sim(&self, level: usize, seed: i32) -> tfp_sim::world::MatchSetup {
        use tfp_sim::entity::Allegiance;
        use tfp_sim::world::{MatchSetup, PlayerSetup};
        let mut s = MatchSetup {
            level,
            seed,
            start_frozen: true,
            ..Default::default()
        };
        let team_mode = self.settings.mode.is_team();
        let v = &self.settings.variants;
        for (i, slot) in self.slots.iter().enumerate().take(s.players.len()) {
            if !slot.joined {
                continue;
            }
            let allegiance = match (team_mode, self.settings.teams[i]) {
                (true, Team::Blue) => Allegiance::Blue,
                (true, Team::Red) => Allegiance::Red,
                _ => Allegiance::Neutral,
            };
            // Teams beyond Blue/Red exist only in the sim's versus flow.
            let has = |f: &str| super::variants::index_of(f).is_some_and(|k| v.slot(k, i));
            let arrows = if has("NoArrows") {
                0
            } else if has("SingleArrow") {
                1
            } else if has("MaxArrows") {
                6
            } else {
                3
            };
            s.players[i] = Some(PlayerSetup {
                archer: slot.archer as u16,
                allegiance,
                team_color: allegiance,
                arrows,
                shield: has("StartWithShields"),
                wings: has("StartWithWings"),
                speed_boots: has("StartWithSpeedBoots"),
                invisible: has("StartInvisible"),
                ..Default::default()
            });
        }
        let g = |f: &str| v.by_field(f);
        let sv = &mut s.variants;
        sv.free_aiming = g("FreeAiming");
        sv.double_jumping = g("DoubleJumping");
        sv.no_dodging = g("NoDodging");
        sv.super_dodging = g("SuperDodging");
        sv.cursed_bows = g("CursedBows");
        sv.cursed_dodges = g("CursedDodges");
        sv.slippery_floors = g("SlipperyFloors");
        sv.no_slipping = g("NoSlipping");
        sv.encumbrance = g("Encumbrance");
        sv.regenerating_arrows = g("RegeneratingArrows");
        sv.regenerating_shields = g("RegeneratingShields");
        sv.anti_gravity_arrows = g("AntiGravityArrows");
        sv.no_seeking_arrows = g("NoSeekingArrows");
        sv.super_seeking_arrows = g("SuperSeekingArrows");
        sv.no_friendly_fire = g("NoFriendlyFire");
        sv.slow_time = g("SlowTime");
        sv.clumsy_archers = g("ClumsyArchers");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goals() {
        assert_eq!(goal_score(Mode::HeadHunters, Length::Standard, 4), 10);
        assert_eq!(goal_score(Mode::HeadHunters, Length::Epic, 3), 16);
        assert_eq!(goal_score(Mode::LastManStanding, Length::Quick, 2), 3);
        assert_eq!(goal_score(Mode::TeamDeathmatch, Length::Instant, 12), 1);
        assert_eq!(goal_score(Mode::HeadHunters, Length::Standard, 12), 14);
    }

    #[test]
    fn clean_settings() {
        let mut j = [false; SLOTS];
        j[0] = true;
        j[1] = true;
        let mut s = MatchSettings::default();
        s.clean(&j);
        assert_eq!(s.mode, Mode::LastManStanding, "2 archers: HH -> LMS");
        j[2] = true;
        s.clean(&j);
        assert_eq!(s.mode, Mode::HeadHunters, "LMS with previous 2 -> HH");
        s.mode = Mode::TeamDeathmatch;
        assert!(s.can_play(2).is_err());
        assert!(s.can_play(3).is_ok());
        s.teams[0] = Team::Blue;
        s.teams[1] = Team::Red;
        assert!(!s.teams_valid(&j));
        s.teams[2] = Team::Red;
        assert!(s.teams_valid(&j));
    }

    #[test]
    fn sim_conversion() {
        let mut slots = vec![Slot::default(); SLOTS];
        for (i, s) in slots.iter_mut().enumerate().take(3) {
            s.joined = true;
            s.archer = i * 2;
        }
        let mut settings = MatchSettings::default();
        let joined: Vec<bool> = slots.iter().map(|s| s.joined).collect();
        let fa = super::super::variants::index_of("FreeAiming").unwrap();
        let ma = super::super::variants::index_of("MaxArrows").unwrap();
        settings.variants.set(fa, true, &joined);
        settings.variants.set_slot(ma, 1, true);
        let fe = FrontEndSetup {
            slots,
            settings,
            tower_name: "SACRED GROUND".into(),
        };
        let s = fe.to_sim(7, 42);
        assert_eq!(s.level, 7);
        assert!(s.variants.free_aiming);
        assert_eq!(s.players.iter().flatten().count(), 3);
        assert_eq!(s.players[2].unwrap().archer, 4);
        assert_eq!(s.players[1].unwrap().arrows, 6);
        assert_eq!(s.players[0].unwrap().arrows, 3);
    }
}
