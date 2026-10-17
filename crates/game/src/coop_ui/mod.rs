//! Quest and Trials presentation (M9) over the sim's `quest` / `trials`
//! modes: enemy art (`art`), death leftovers (`debris`), the Quest and
//! Trials HUDs and flow screens (`quest_hud`, `trials_hud`), the menus
//! (`menus`: co-op menu, quest tower + difficulty select, trials grid) and
//! the play scene (`scene::CoopPlay`). Dark World (M10) lives in
//! `darkworld` (boss art, HUD, tower select). Notes in `docs/ui-notes.md`.

pub mod art;
pub mod darkworld;
pub mod debris;
pub mod intro_controls;
pub mod menus;
pub mod miasma;
pub mod quest_hud;
pub mod scene;
pub mod screen_fx;
#[cfg(test)]
mod tests;
pub mod trials_hud;

use std::collections::BTreeMap;

use crate::front::settings::Owner;

/// Which game type the rollcall and the map run for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RollcallMode {
    #[default]
    Versus,
    Quest,
    Trials,
    DarkWorld,
}

impl RollcallMode {
    /// Archers needed to start / allowed (ui.md 6).
    pub fn limits(self, capacity: usize) -> (usize, usize) {
        match self {
            RollcallMode::Versus => (2, capacity),
            // Ours: up to 4 (quest.md 7 scaling) when laid out for 4+.
            RollcallMode::Quest => (1, capacity.min(4)),
            RollcallMode::Trials => (1, 1),
            RollcallMode::DarkWorld => (1, capacity.min(4)),
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            RollcallMode::Versus => "fight",
            RollcallMode::Quest => "quest",
            RollcallMode::Trials => "trials",
            RollcallMode::DarkWorld => "darkWorld",
        }
    }

    /// Modes entered from the CO-OP menu (Back from the rollcall returns there).
    pub fn entered_from_coop_menu(self) -> bool {
        matches!(self, RollcallMode::Quest | RollcallMode::DarkWorld)
    }
}

/// What to play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoopMode {
    /// Quest tower id (0..13).
    Quest { tower: usize, hardcore: bool },
    /// Index into the trials level list (`TowerIndex::trials`).
    Trials { level: usize },
    /// Dark World tower (0..4), difficulty (0 Normal, 1 Hardcore, 2
    /// Legendary) and the curses / power-ups chosen on the map.
    DarkWorld {
        tower: usize,
        difficulty: u8,
        variants: tfp_sim::variants::VariantSet,
    },
}

/// One joined archer.
#[derive(Clone, Debug, PartialEq)]
pub struct CoopPlayer {
    pub slot: usize,
    /// Base archer index (0..8).
    pub archer: usize,
    pub alt: bool,
    /// `None`: a bot.
    pub owner: Option<Owner>,
}

/// The menus' result for a co-op game.
#[derive(Clone, Debug, PartialEq)]
pub struct CoopLaunch {
    pub mode: CoopMode,
    pub players: Vec<CoopPlayer>,
}

/// Session records (no save file yet): quest skulls and best times,
/// trials best times and medals, co-op crowns.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Records {
    pub quest: BTreeMap<usize, QuestRecord>,
    pub trials: BTreeMap<usize, TrialRecord>,
    pub dark_world: BTreeMap<usize, darkworld::DwRecord>,
    pub crowns: [bool; 12],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuestRecord {
    pub normal: bool,
    pub hardcore: bool,
    pub no_deaths: bool,
    /// Best clear times in ticks for 1 and 2+ players (0 = none).
    pub best: [u32; 2],
    pub attempts: u32,
    pub deaths: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrialRecord {
    pub best: u32,
    pub unlocked: [bool; 3],
    pub attempts: u32,
    /// The hidden yellow gem was found.
    pub gem: bool,
}

/// Where a co-op game hands control back to the menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// The mode's tower select.
    Map,
    /// The main menu.
    Quit,
}

/// Front-end state of the co-op modes (lives in `front::Session`).
#[derive(Clone, Debug, Default)]
pub struct CoopSession {
    pub mode: RollcallMode,
    /// Last quest tower / trials level and difficulty (map selection).
    pub quest_tower: usize,
    pub hardcore: bool,
    pub trials_level: usize,
    /// Dark World map selection: tower, difficulty, curses / power-ups.
    pub dw_tower: usize,
    pub dw_difficulty: u8,
    pub dw_variants: tfp_sim::variants::VariantSet,
    pub records: Records,
    /// Set by a map screen; taken by the game scene.
    pub pending: Option<CoopLaunch>,
}

/// The joined front-end slots as co-op players (rollcall order).
pub fn players_from_slots(slots: &[crate::front::settings::Slot], max: usize) -> Vec<CoopPlayer> {
    use crate::front::settings::Costume;
    slots
        .iter()
        .enumerate()
        .filter(|(_, s)| s.joined)
        .take(max)
        .map(|(i, s)| CoopPlayer {
            slot: i,
            archer: s.archer,
            alt: s.costume == Costume::Alt,
            owner: s.owner.clone(),
        })
        .collect()
}
