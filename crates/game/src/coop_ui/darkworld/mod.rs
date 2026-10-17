//! Dark World presentation (M10) over the sim's `darkworld` mode
//! (darkworld.md, ui.md 9.2 / 10.4): the four bosses' art with their moving
//! parts, projectiles, phase changes, hit flashes and death sequences
//! (`bosses`), the exit portal, the HUD and flow screens (`hud`: lives
//! gems, boss bar, title / level / boss cards, GO!, the trigger-arrow
//! slide, game over, complete), and the tower select with the difficulty
//! and variants overlays (`map`). The play scene is `coop_ui::scene::CoopPlay`
//! in its Dark World mode. Records are kept for the session only.

pub mod bosses;
pub mod hud;
pub mod map;
#[cfg(test)]
mod tests;

use tfp_render::Color;
use tfp_sim::variants::Variant;

/// Difficulty colours (`QuestDifficultySelect` / `DarkWorldDifficultySelect`).
pub const NORMAL: Color = Color::hex(0x96DDFF);
pub const HARDCORE: Color = Color::hex(0xFF5432);
pub const LEGENDARY: Color = Color::hex(0xFFE22B);

pub fn difficulty_color(d: u8) -> Color {
    match d {
        0 => NORMAL,
        1 => HARDCORE,
        _ => LEGENDARY,
    }
}

pub fn difficulty_name(d: u8) -> &'static str {
    match d {
        0 => "NORMAL",
        1 => "HARDCORE",
        _ => "LEGENDARY",
    }
}

/// The level-card shield of a difficulty (main atlas).
pub fn shield_image(d: u8) -> &'static str {
    match d {
        0 => "quest/shield",
        1 => "quest/shieldHardcore",
        _ => "quest/shieldLegendary",
    }
}

/// `DarkWorldBossTitle` lines per boss id.
pub fn boss_title(id: u8) -> (&'static str, &'static str, &'static str) {
    match id {
        0 => ("WRETCHED SEER", "RELENTLESS GAZE OF THE ABYSS", ""),
        1 => ("LADY ABIGAIL", "TYRANT QUEEN OF THE FOREST", ""),
        2 => ("THE BLIND LICH", "TORMENTOR OF THE FORSAKEN", ""),
        _ => (
            "CATACLYSM",
            "IMPOSSIBLE HORROR",
            "AT THE CENTER OF ALL CREATION",
        ),
    }
}

/// Session records of one Dark World tower (`DarkWorldTowerStats`,
/// darkworld.md 9.3; no save file yet).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DwRecord {
    pub normal: bool,
    pub hardcore: bool,
    pub legendary: bool,
    pub eye: bool,
    pub gold_eye: bool,
    /// Legendary best times in ticks for 1..4 players (0 = none).
    pub best: [u32; 4],
    /// Legendary: most curses for 1..4 players.
    pub curses: [u32; 4],
    pub deaths: u32,
    pub attempts: u32,
}

impl DwRecord {
    /// `DarkWorldTowerStats.Complete`: a clear on `difficulty` also sets the
    /// easier flags; only Legendary clears keep times and curse counts.
    pub fn complete(&mut self, r: &tfp_sim::darkworld::DwResult) {
        self.normal = true;
        self.hardcore |= r.difficulty >= 1;
        self.legendary |= r.difficulty >= 2;
        self.eye |= r.eye;
        self.gold_eye |= r.gold_eye;
        if r.difficulty >= 2 {
            let k = (r.players.clamp(1, 4) - 1) as usize;
            if self.best[k] == 0 || r.time < self.best[k] {
                self.best[k] = r.time;
            }
            self.curses[k] = self.curses[k].max(r.curses);
        }
    }

    /// Map badge (menu atlas): gold eye, eye, gold / red / white skull.
    pub fn badge(&self) -> Option<&'static str> {
        Some(if self.gold_eye {
            "questResults/goldEye"
        } else if self.eye {
            "questResults/eye"
        } else if self.legendary {
            "questResults/goldSkull"
        } else if self.hardcore {
            "questResults/redSkull"
        } else if self.normal {
            "questResults/whiteSkull"
        } else {
            return None;
        })
    }
}

/// Menu title and menu-atlas icon of a sim variant (`front::variants`).
pub fn variant_ui(v: Variant) -> (String, String) {
    let field = format!("{v:?}");
    match crate::front::variants::index_of(&field) {
        Some(i) => {
            let d = &crate::front::variants::VARIANTS[i];
            (d.title(), d.icon())
        }
        None => (v.title().to_owned(), String::new()),
    }
}

/// The curses of the pre-game screen for a player count (Exploding
/// Corpses replaces No Treasure with 2+ archers, darkworld.md 5.6).
pub fn curses_for(players: usize) -> [Variant; 6] {
    use Variant::*;
    let third = if players <= 1 {
        NoTreasure
    } else {
        ExplodingCorpses
    };
    [
        SmallQuivers,
        CursedBows,
        third,
        AlwaysDark,
        Encumbrance,
        SlipperyFloors,
    ]
}

/// Power-ups switched on.
pub fn power_ups_on(v: &tfp_sim::variants::VariantSet) -> usize {
    tfp_sim::darkworld::POWER_UPS
        .iter()
        .filter(|&&p| v.any(p))
        .count()
}

// ------------------------------------------------------------ play aids

/// Headless proof assistance (`--dw-assist N`; the sim test's scaffolding):
/// archers invulnerable and never out of arrows, a solo archer never out
/// of lives, leftovers cleared every 10 s, archers that cannot find the
/// open exit carried into it, and one point of boss damage every `n`
/// ticks while the boss can be hurt (shield orbs go after 5 s).
pub fn assist(w: &mut tfp_sim::World, c: &tfp_sim::Content, n: u32, frame: u32) {
    use tfp_sim::darkworld::{self as dw, DwPhase, Part};
    use tfp_sim::entity::Kind;
    let Some(d) = w.dark_world.as_deref() else {
        return;
    };
    let players = d.n_players() as u8;
    let solo = players == 1;
    let phase = d.phase;
    let phase_t = d.phase_t;
    let boss_mode = d.boss_mode;
    let portal = d.exit_portal;
    if solo && let Some(d) = w.dark_world.as_deref_mut() {
        d.extra_lives = d.extra_lives.max(2);
    }
    // Cataclysm's intro drains the quivers: no refills then.
    let draining = matches!(dw::boss_phase(w), Some((3, dw::cataclysm::INTRO)));
    // Darkfang's fists crush through invulnerability: like the sim test,
    // the archers leave the fight (removed, not killed) once the boss is up.
    let crush = w
        .dark_world
        .as_deref()
        .and_then(|d| d.boss.as_deref())
        .is_some_and(|b| {
            matches!(&b.kind, dw::boss::BossKind::Cyclops(s)
                if s.state != dw::cyclops::INTRO || s.intro.step >= 3)
        });
    if crush {
        let ids: Vec<_> = w.players().collect();
        for id in ids {
            w.remove(id);
        }
    }
    for k in 0..12u8 {
        if let Some(id) = w.player_by_index(k) {
            w.core_mut(id)
                .flash
                .start_for(5.0, tfp_sim::entity::FlashFinish::None);
            if let Some(p) = w.player_mut(id)
                && p.arrows.is_empty()
                && !draining
            {
                p.arrows
                    .add_arrows(&[tfp_sim::quiver::ArrowType::Normal; 3]);
            }
        }
    }
    let first = w.entities().next();
    if frame % 600 == 599
        && let Some(none) = first
    {
        tfp_sim::enemies::kill_all(w, c, none);
    }
    // Intermission waves and old shield orbs go quickly in boss fights.
    if dw::boss_phase(w).is_some() && frame.is_multiple_of(30) {
        let inter = matches!(
            dw::boss_phase(w),
            Some((1, dw::dreadwood::INTERMISSION)) | Some((2, dw::cyclops::MAD))
        );
        if inter && let Some(none) = first {
            tfp_sim::enemies::kill_all(w, c, none);
        }
        let orbs: Vec<_> = w
            .entities()
            .filter(
                |&e| matches!(w.kind(e), Some(Kind::Dw(Part::ShieldOrb { t, .. })) if *t > 300.0),
            )
            .collect();
        for o in orbs {
            w.remove(o);
        }
    }
    if phase == DwPhase::Exit
        && phase_t > 240.0
        && let Some(p) = portal.filter(|&e| w.exists(e))
    {
        let at = w.core(p).pos;
        for k in 0..players {
            if let Some(id) = w.player_by_index(k) {
                w.core_mut(id).pos = at;
            }
        }
    }
    if n > 0 && frame.is_multiple_of(n) {
        let ok = match dw::boss_phase(w) {
            Some((0, s)) => boss_mode && s == dw::amaranth::NORMAL,
            Some((1, s)) => boss_mode && s == dw::dreadwood::FIGHT,
            Some((2, s)) => boss_mode && s == dw::cyclops::IDLE,
            Some((3, s)) => (boss_mode && s == dw::cataclysm::FREAKOUT)
                || (s == dw::cataclysm::DEAD
                    && w.dark_world
                        .as_deref()
                        .and_then(|d| d.boss.as_deref())
                        .is_some_and(
                            |b| matches!(&b.kind, dw::boss::BossKind::Cataclysm(st) if st.heart),
                        )),
            _ => false,
        };
        if ok {
            dw::debug_damage(w, 1, 0);
        }
    }
}

/// Proof aid: drive the session to its game over (lives gone, archers
/// die) or its completion (the last level loads and its boss is removed).
pub fn force_end(w: &mut tfp_sim::World, c: &tfp_sim::Content, complete: bool) {
    use tfp_sim::darkworld::{self as dw, DwPhase};
    use tfp_sim::entity::Kind;
    let Some(d) = w.dark_world.as_deref() else {
        return;
    };
    if matches!(d.phase, DwPhase::GameOver | DwPhase::Complete) {
        return;
    }
    if !complete {
        if !matches!(d.phase, DwPhase::Playing | DwPhase::Exit) {
            return;
        }
        if let Some(d) = w.dark_world.as_deref_mut() {
            d.extra_lives = 0;
        }
        let ids: Vec<_> = w.players().collect();
        for id in ids {
            tfp_sim::hurt::with_player(w, id, |w, p| {
                if !p.dead {
                    tfp_sim::player::die(w, c, id, p, tfp_sim::player::DeathCause::Enemy, -1);
                }
            });
        }
        return;
    }
    let last = d.levels(c).len() - 1;
    if d.round_index != last {
        dw::jump_to_level(w, c, last);
        return;
    }
    if d.phase != DwPhase::Playing || d.boss_finished {
        return;
    }
    let parts: Vec<_> = w
        .entities()
        .filter(|&e| matches!(w.kind(e), Some(Kind::Dw(_))))
        .collect();
    for e in parts {
        w.remove(e);
    }
    let first = w.entities().next();
    if let Some(none) = first {
        tfp_sim::enemies::kill_all(w, c, none);
    }
    if let Some(d) = w.dark_world.as_deref_mut() {
        d.boss = None;
        d.boss_mode = false;
    }
    dw::boss_finished(w);
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn records_and_ui_names() {
        let mut r = DwRecord::default();
        assert_eq!(r.badge(), None);
        let res = tfp_sim::darkworld::DwResult {
            completed: true,
            tower: 0,
            difficulty: 2,
            players: 2,
            time: 5000,
            continues: 0,
            deaths: 1,
            curses: 2,
            eye: true,
            gold_eye: false,
            kills: [0; 12],
            deaths_per: [0; 12],
            revives: [0; 12],
            boss_hits: [0; 12],
            best_combo: [0; 12],
            crowns: [false; 12],
        };
        r.complete(&res);
        assert!(r.normal && r.hardcore && r.legendary && r.eye && !r.gold_eye);
        assert_eq!((r.best[1], r.curses[1]), (5000, 2));
        assert_eq!(r.badge(), Some("questResults/eye"));
        let (title, icon) = variant_ui(Variant::StartWithShields);
        assert_eq!(title, "START WITH SHIELDS");
        assert!(icon.starts_with("variants/"), "{icon}");
        assert_eq!(curses_for(1)[2], Variant::NoTreasure);
        assert_eq!(curses_for(3)[2], Variant::ExplodingCorpses);
    }
}
