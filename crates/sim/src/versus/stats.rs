//! Match statistics (versus.md §10.6) and the end-of-match awards (§10.5).
//!
//! Counters that belong to features not implemented yet (bombs, lasers,
//! prisms, ghosts, ...) exist and stay 0 until those features feed them.

use crate::checksum::{Hasher, StableHash};
use crate::player::DeathCause;
use crate::rng::Rng;
use crate::versus::settings::{MatchSettings, Mode};

/// `DeathType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathType {
    Normal,
    Self_,
    Team,
}

/// `KillStats`: by type, by cause, by the other party's archer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KillStats {
    pub kills: u32,
    pub team_kills: u32,
    pub self_kills: u32,
    /// Indexed by `DeathCause`.
    pub by_cause: [u32; 13],
    /// `[0]` = environment (-1), then archers 0..8.
    pub by_archer: [u32; 10],
}

impl KillStats {
    pub fn total(&self) -> u32 {
        self.kills + self.team_kills + self.self_kills
    }
    pub fn environment(&self) -> u32 {
        self.by_archer[0]
    }
    pub fn cause(&self, c: DeathCause) -> u32 {
        self.by_cause[c as usize]
    }
    /// `Add(type, cause, archer)`.
    pub fn add(&mut self, t: DeathType, cause: DeathCause, archer: i32) {
        match t {
            DeathType::Normal => self.kills += 1,
            DeathType::Self_ => self.self_kills += 1,
            DeathType::Team => self.team_kills += 1,
        }
        self.by_cause[cause as usize] += 1;
        if (-1..9).contains(&archer) {
            self.by_archer[(archer + 1) as usize] += 1;
        }
    }
}

/// `MatchStats` (one per slot, whole match).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MatchStats {
    pub won: bool,
    pub got_win: bool,
    pub points_from_goal: i32,
    pub furthest_behind: i32,
    pub round_sweeps: u32,
    pub survived_with_no_kills: u32,
    pub kills: KillStats,
    pub deaths: KillStats,
    pub winner_kills: u32,
    pub kills_while_dead: u32,
    /// Seconds of round time of the fastest kill (0 = none).
    pub fastest_kill: f32,
    pub arrows_shot: u32,
    pub spam_shots: u32,
    pub failed_shots: u32,
    pub arrows_collected: u32,
    pub arrows_caught: u32,
    pub own_arrows_caught: u32,
    pub team_arrow_catches: u32,
    pub grace_period_catches: u32,
    pub arrows_stolen: u32,
    pub longest_shot: f32,
    pub most_bounces_laser_kill: u32,
    pub most_bolt_turns: u32,
    pub drill_hits: u32,
    pub shields_broken: u32,
    pub dodges: u32,
    pub jumps: u32,
    pub dodge_stomps: u32,
    pub deaths_during_dodge_cooldown: u32,
    pub dodges_too_late: u32,
    pub duck_dances: u32,
    pub duck_frames: f32,
    pub fast_fall_frames: f32,
    pub ledge_frames: f32,
    pub frames_alive: f32,
    pub screen_wraps: u32,
    pub arrow_screen_wraps: u32,
    pub crown_spawns: u32,
    pub crowns_picked_up: u32,
    pub hats_picked_up: u32,
    pub hats_shot_off: u32,
    pub explosions_survived: i32,
    pub bomb_multi_kill: u32,
    pub trigger_bomb_kills: u32,
    pub bomb_trap_kills: u32,
    pub bombs_defused: u32,
    pub ledge_grabbed_kills: u32,
    pub dropped_arrow_kills: u32,
    pub hyper_arrow_kills: u32,
    pub hyper_deaths: u32,
    pub hyper_self_kills: u32,
    pub hyper_stomps: u32,
    pub self_laser_kills: u32,
    pub crush_others_kills: u32,
    pub kills_during_miasma: u32,
    pub enemy_in_prism_kills: u32,
    pub self_in_prism_kills: u32,
    pub enemy_in_team_prism_kills: u32,
    pub kills_as_ghost: u32,
    pub ghost_kills: u32,
    pub revives: u32,
    pub treasures_taken: u32,
    pub trigger_bombs_lost: u32,
}

impl StableHash for MatchStats {
    fn stable_hash(&self, h: &mut Hasher) {
        // The rule-relevant part; the rest is display-only.
        h.bool(self.won);
        h.bool(self.got_win);
        h.u32(self.kills.total());
        h.u32(self.deaths.total());
        h.u32(self.arrows_shot);
        h.u32(self.dodges);
        h.u32(self.jumps);
        h.f32(self.fastest_kill);
    }
}

/// The 70 awards in table order (versus.md §10.5).
pub const AWARD_NAMES: [&str; 70] = [
    "Most Lethal",
    "Bubble Breaker Award",
    "Quick Draw Award",
    "Most Devious",
    "Most Vindictive",
    "Greediest",
    "Late To The Party Award",
    "Mostly Harmless",
    "Most Prepared",
    "Least Prepared",
    "Most Trigger-Happy",
    "Worst Accounting",
    "Who Needs Arrows?",
    "Marksmanship Award",
    "Longest Shot",
    "Pickpocket Award",
    "Happy Accident Award",
    "Most Regal",
    "Regicide Award",
    "Fetal Position Award",
    "Cannonball Award",
    "Koala Award",
    "Koala Hunter Award",
    "Most Evasive",
    "Most Zen",
    "Least Composed",
    "Artless Dodger Award",
    "Most Twitchy",
    "Most Takebacks",
    "Super Booper",
    "Speed of Light Award",
    "Hyper Booper",
    "Hyper Martyr Award",
    "Too Fast Too Furious",
    "Turncloak Award",
    "Damage Control Award",
    "Reanimator Award",
    "Most Reckless",
    "Most Compressed",
    "Gravedigger Award",
    "Special Delivery Award",
    "Nerves Of Steel Award",
    "Most Fashionable",
    "Usurper Award",
    "William Tell Award",
    "Bottle Rocket Award",
    "Double Kill",
    "Triple Kill",
    "Bomb Squad Award",
    "Cause And Effect Award",
    "Bank Shot Award",
    "Green Thumb Award",
    "X-Ray Vision Award",
    "Most Conductive",
    "Right Angle Award",
    "Mastermind Award",
    "Web of Lies Award",
    "Prism Warden Award",
    "Caged Animal Award",
    "Ally Oop Award",
    "From The Grave Award",
    "Exorcist Award",
    "Eagle Eye Award",
    "Vendetta Award",
    "Most Thorough",
    "Comeback King",
    "Steak In Mouth Award",
    "Consolation Prize",
    "Overachiever Award",
    "Ironman Award",
];

pub const CONSOLATION: u8 = 67;
const HIGH_PRIORITY: [u8; 8] = [46, 47, 62, 63, 65, 66, 68, 69];
const CAN_TIE: [u8; 5] = [46, 47, 65, 66, 69];

fn at_least(v: u64, min: u64) -> u64 {
    if v >= min { v } else { 0 }
}

/// Rating, `WinBy` and eligibility of award `a` for slot `p`.
fn rate(a: u8, p: usize, s: &MatchStats, set: &MatchSettings, archer: u16) -> (bool, u64, u64) {
    let n = set.player_amount();
    let team = set.team_mode();
    let max = u64::MAX;
    let k = &s.kills;
    let d = &s.deaths;
    use DeathCause as C;
    let r = |v: u32| v as u64;
    let (ok, rating, win_by) = match a {
        0 => (set.mode != Mode::Headhunters && n > 2, r(k.kills), 2),
        1 => (true, r(s.shields_broken), 1),
        2 => (
            true,
            if s.fastest_kill > 0.0 && s.fastest_kill <= 3.0 {
                max - 1 - (s.fastest_kill * 1000.0) as u64
            } else {
                0
            },
            0,
        ),
        3 => (
            true,
            at_least(r(s.screen_wraps + 2 * s.arrow_screen_wraps), 10),
            0,
        ),
        4 => (true, r(s.kills_while_dead), 3),
        5 => (true, r(s.treasures_taken), 2),
        6 => (
            n > 2 && set.mode != Mode::LastManStanding,
            r(s.survived_with_no_kills),
            0,
        ),
        7 => (
            n > 2,
            if k.kills >= 3 {
                0
            } else {
                max - 1 - r(k.kills)
            },
            1,
        ),
        8 => (true, r(s.arrows_collected), 3),
        9 => (
            true,
            if s.arrows_collected >= 8 {
                0
            } else {
                max - 1 - r(s.arrows_collected)
            },
            0,
        ),
        10 => (true, at_least(r(s.spam_shots), 5), 3),
        11 => (true, r(s.failed_shots), 0),
        12 => (true, at_least(r(k.cause(C::JumpedOn)), 4), 2),
        13 => (
            true,
            if s.arrows_shot < 5 {
                0
            } else {
                1000 * r(k.cause(C::Arrow)) / r(s.arrows_shot)
            },
            0,
        ),
        14 => (true, at_least(s.longest_shot as u64, 30), 0),
        15 => (true, r(s.arrows_stolen), 2),
        16 => (true, r(s.dropped_arrow_kills), 0),
        17 => (!team && n > 2, r(s.crown_spawns), 1),
        18 => (n > 2, r(s.winner_kills), 0),
        19 => (true, at_least(s.duck_frames as u64, 300), 0),
        20 => (true, at_least(s.fast_fall_frames as u64, 180), 0),
        21 => (true, at_least(s.ledge_frames as u64, 300), 0),
        22 => (true, at_least(r(s.ledge_grabbed_kills), 2), 0),
        23 => (true, r(s.dodges), 0),
        24 => (true, r(s.arrows_caught), 0),
        25 => (true, at_least(r(s.deaths_during_dodge_cooldown), 3), 0),
        26 => (true, at_least(r(s.dodges_too_late), 3), 0),
        27 => (true, at_least(r(s.grace_period_catches), 2), 0),
        28 => (true, r(s.own_arrows_caught), 0),
        29 => (true, at_least(r(s.dodge_stomps), 3), 0),
        30 => (true, r(s.hyper_arrow_kills), 0),
        31 => (true, r(s.hyper_stomps), 0),
        32 => (true, r(s.hyper_deaths), 0),
        33 => (true, r(s.hyper_self_kills), 0),
        34 => (team, r(k.team_kills), 0),
        35 => (team, r(s.team_arrow_catches), 0),
        36 => (team, at_least(r(s.revives), 3), 0),
        37 => (true, at_least(r(k.self_kills), 2), 1),
        38 => (true, r(d.cause(C::Squish)), 0),
        39 => (true, r(s.crush_others_kills), 0),
        40 => (true, r(s.bomb_trap_kills), 0),
        41 => (true, r(s.kills_during_miasma), 0),
        42 => (true, r(s.hats_picked_up), 0),
        43 => (true, r(s.crowns_picked_up), 0),
        44 => (true, at_least(r(s.hats_shot_off), 3), 0),
        45 => (true, at_least(s.explosions_survived.max(0) as u64, 3), 0),
        46 => (n > 2, (s.bomb_multi_kill == 2) as u64, 0),
        47 => (n > 2, (s.bomb_multi_kill == 3) as u64, 0),
        48 => (true, r(s.bombs_defused), 0),
        49 => (true, r(s.self_laser_kills), 0),
        50 => (
            true,
            if s.most_bounces_laser_kill > 1 {
                r(s.most_bounces_laser_kill)
            } else {
                0
            },
            0,
        ),
        51 => (true, r(k.cause(C::Brambles)), 2),
        52 => (true, at_least(r(s.drill_hits), 3), 0),
        53 => (true, r(k.cause(C::Shock)), 0),
        54 => (true, at_least(r(s.most_bolt_turns), 2), 0),
        55 => (true, at_least(r(s.trigger_bomb_kills), 2), 0),
        56 => (true, r(s.trigger_bombs_lost), 0),
        57 => (true, at_least(r(s.enemy_in_prism_kills), 2), 0),
        58 => (true, at_least(r(s.self_in_prism_kills), 2), 0),
        59 => (team, r(s.enemy_in_team_prism_kills), 0),
        60 => (true, at_least(r(s.kills_as_ghost), 2), 0),
        61 => (true, at_least(r(s.ghost_kills), 3), 0),
        62 => {
            let hits = k.cause(C::Arrow) + k.cause(C::Explosion);
            let ok = s.arrows_shot >= 5 && hits as f32 / s.arrows_shot as f32 >= 0.5;
            (true, if ok { r(s.arrows_shot) } else { 0 }, 0)
        }
        63 => {
            let mut v = 0;
            if k.kills >= 4 {
                for j in 0..9u16 {
                    if j != archer && k.by_archer[j as usize + 1] as f32 >= 0.8 * k.kills as f32 {
                        v = r(k.by_archer[j as usize + 1]);
                        break;
                    }
                }
            }
            (set.player_team_size(p) < n - 1, v, 0)
        }
        64 => (n > 2, r(s.round_sweeps), 0),
        65 => (
            true,
            if s.furthest_behind > 3 && s.won {
                s.furthest_behind as u64
            } else {
                0
            },
            0,
        ),
        66 => (true, (s.got_win && !s.won) as u64, 0),
        67 => (false, 0, 0),
        68 => (
            set.mode == Mode::Headhunters,
            if s.points_from_goal >= 2 {
                s.points_from_goal as u64
            } else {
                0
            },
            0,
        ),
        69 => (true, (d.total() == 0 && k.total() > 1) as u64, 0),
        _ => (false, 0, 0),
    };
    (ok, rating, win_by)
}

/// `VersusAwards.GetAwards`: award ids per slot (only joined slots get any).
pub fn compute_awards(
    set: &MatchSettings,
    stats: &[MatchStats; crate::world::MAX_PLAYERS],
    bases: &[i32; crate::world::MAX_PLAYERS],
    rng: &mut Rng,
) -> Vec<Vec<u8>> {
    let slots: Vec<usize> = (0..crate::world::MAX_PLAYERS)
        .filter(|&p| set.joined(p))
        .collect();
    let mut held: Vec<Vec<u8>> = vec![vec![]; crate::world::MAX_PLAYERS];
    for a in 0..70u8 {
        let mut best = 1u64;
        let mut second = 0u64;
        let mut ratings = vec![];
        for &p in &slots {
            let archer = bases[p].max(0) as u16;
            let (ok, r, win_by) = rate(a, p, &stats[p], set, archer);
            if !ok {
                continue;
            }
            best = best.max(r);
            if r < best && r > second {
                second = r;
            }
            ratings.push((p, r, win_by));
        }
        let mut holders: Vec<usize> = ratings
            .iter()
            .filter(|&&(_, r, wb)| r == best && r >= second.saturating_add(wb))
            .map(|&(p, _, _)| p)
            .collect();
        if holders.len() > 1 && !CAN_TIE.contains(&a) {
            let fewest = holders.iter().map(|&p| held[p].len()).min().unwrap_or(0);
            holders.retain(|&p| held[p].len() == fewest);
            if holders.len() > 1 {
                let k = rng.choose_index(holders.len());
                holders = vec![holders[k]];
            }
        }
        for p in holders {
            held[p].push(a);
        }
    }
    let cap = if slots.len() > 5 { 1 } else { 3 };
    for &p in &slots {
        let list = &mut held[p];
        if list.len() > cap {
            let mut keep: Vec<u8> = list
                .iter()
                .copied()
                .filter(|a| HIGH_PRIORITY.contains(a))
                .collect();
            let mut rest: Vec<u8> = list
                .iter()
                .copied()
                .filter(|a| !HIGH_PRIORITY.contains(a))
                .collect();
            rng.shuffle(&mut rest);
            for a in rest {
                if keep.len() >= cap {
                    break;
                }
                keep.push(a);
            }
            *list = keep;
        }
        if list.is_empty() {
            list.push(CONSOLATION);
        }
    }
    held
}
