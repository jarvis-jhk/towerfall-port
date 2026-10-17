//! `sim-checksums`: run fixed bot matches in every mode and print their
//! checksum sequences (one line per 100 ticks plus the final one), for
//! cross-platform determinism checks (`scripts/dev/determinism-check.sh`
//! runs it natively and under Wine and diffs the output).
//!
//! Scenarios (each skipped with a note when its data is missing):
//! `ascii` (built-in arena, always), `versus-treasure` (Steam, Headhunters,
//! all treasure), `tdm12` (8-player build, 12-player TDM), `quest` (first
//! tower), `trials` (first level), `darkworld` (Amaranth level 1, 2 players),
//! `darkworld-boss` (Cataclysm, Hardcore, 2 players).
//!
//! Usage: `sim-checksums [frames]` (default 3000).

use tfp_assets::install::{GameInstall, Installs};
use tfp_sim::content::Content;
use tfp_sim::variants::Variant;
use tfp_sim::versus::{self, MatchSettings, Mode};
use tfp_sim::{MatchSetup, PlayerInput, PlayerSetup, World};

fn run(name: &str, c: &Content, mut w: World, players: usize, frames: usize) {
    // The shared bot (`tfp_sim::bot::Bot`) for every archer.
    let mut bots: Vec<tfp_sim::bot::Bot> = (0..players)
        .map(|i| tfp_sim::bot::Bot::new(4242 + i as i32 * 97))
        .collect();
    for f in 1..=frames {
        let i: Vec<PlayerInput> = bots
            .iter_mut()
            .enumerate()
            .map(|(k, b)| b.input(&w, c, k as u8))
            .collect();
        w.step(c, &i);
        if f % 100 == 0 || f == frames {
            println!("{name} {f:5} {:032x}", w.checksum128());
        }
    }
}

const ARENA: [&str; 24] = [
    "################################",
    "#..............................#",
    "#...P......................P...#",
    "#######....############....#####",
    "#..............................#",
    "#..............................#",
    "......P................P........",
    "#......####..........####......#",
    "##########............##########",
    "#..............................#",
    "#..............................#",
    "#.........#####.......#####....#",
    "#..............................#",
    "#..............................#",
    "#....-----..........-----......#",
    "#..............................#",
    "#..............................#",
    "#......####...........####.....#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "################################",
    "################################",
];

fn ascii(frames: usize) {
    let c = Content::from_ascii(&ARENA);
    let mut s = MatchSettings::new(Mode::LastManStanding, 4, vec![0]);
    s.tower_card = false;
    s.seed = 7;
    let w = versus::new_match(&c, s);
    run("ascii", &c, w, 4, frames);
    // A sandbox with respawns and every arrow type in the air.
    let mut m = MatchSetup {
        seed: 9,
        respawn_frames: Some(30),
        ..Default::default()
    };
    for i in 0..4 {
        m.players[i] = Some(PlayerSetup {
            archer: i as u16,
            arrows: 3,
            ..Default::default()
        });
    }
    let w = World::new(&c, &m);
    run("ascii-sandbox", &c, w, 4, frames);
}

fn versus_on(
    inst: &GameInstall,
    name: &str,
    mode: Mode,
    players: usize,
    treasure: bool,
    frames: usize,
) {
    let Ok(levels) = tfp_sim::loader::versus_levels(inst) else {
        println!("{name} skipped (no tower index)");
        return;
    };
    let Ok(c) = tfp_sim::loader::content_from_install(inst, &levels) else {
        println!("{name} skipped (content)");
        return;
    };
    // A level from the middle of the list (a busy tower).
    let li = c.levels.len() / 2;
    let mut s = MatchSettings::new(mode, players, vec![li, (li + 1) % c.levels.len()]);
    s.tower_card = false;
    s.seed = 1234;
    let mask = s.joined_mask();
    if treasure {
        for v in [Variant::MaxTreasure, Variant::IgnoreTowerItemSet] {
            s.variants.set_all(v, true, mask);
        }
    }
    if mode == Mode::TeamDeathmatch {
        s.teams = 3;
        for (i, slot) in s.slots.iter_mut().enumerate() {
            if let Some(sl) = slot.as_mut() {
                sl.team = (i % 3) as u8;
            }
        }
        s.variants.set_all(Variant::TeamRevive, true, mask);
    }
    let w = versus::new_match(&c, s);
    run(name, &c, w, players, frames);
}

fn main() {
    let frames: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);
    ascii(frames);
    let installs = Installs::discover();
    match installs.steam.as_ref() {
        Some(inst) => {
            versus_on(inst, "versus-treasure", Mode::Headhunters, 4, true, frames);
            match tfp_sim::loader::quest_content(inst) {
                Ok(c) if !c.quest_scripts.is_empty() => {
                    let mut s = tfp_sim::quest::QuestSettings::new(0, 0, 2);
                    s.seed = 3;
                    let w = tfp_sim::quest::new_quest(&c, s);
                    run("quest", &c, w, 2, frames);
                }
                _ => println!("quest skipped"),
            }
            match tfp_sim::loader::trials_content(inst) {
                Ok((c, settings)) if !settings.is_empty() => {
                    let w = tfp_sim::trials::new_trial(&c, settings[0]);
                    run("trials", &c, w, 1, frames.min(1200));
                }
                _ => println!("trials skipped"),
            }
            match tfp_sim::loader::dark_world_content(inst) {
                Ok(c) if c.dark_world.len() >= 4 => {
                    use tfp_sim::darkworld::{self, DwSettings};
                    let mut s = DwSettings::new(0, darkworld::NORMAL, 2);
                    s.seed = 5;
                    let w = darkworld::new_dark_world(&c, s);
                    run("darkworld", &c, w, 2, frames);
                    let mut s = DwSettings::new(3, darkworld::HARDCORE, 2);
                    s.seed = 6;
                    let mut w = darkworld::new_dark_world(&c, s);
                    let last = w.dark_world.as_ref().unwrap().levels(&c).len() - 1;
                    darkworld::jump_to_level(&mut w, &c, last);
                    run("darkworld-boss", &c, w, 2, frames);
                }
                _ => println!("darkworld skipped"),
            }
        }
        None => println!("steam scenarios skipped (no install)"),
    }
    match installs.eight_player.as_ref() {
        Some(inst) => versus_on(inst, "tdm12", Mode::TeamDeathmatch, 12, true, frames),
        None => println!("tdm12 skipped (no 8-player install)"),
    }
}
