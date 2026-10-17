//! Online co-op (Quest, Trials): content, settings and the starting world,
//! the same on every machine. The lobby picks the kind
//! (`OnlineSettings::kind`), the quest tower / trial (`coop_index`) and
//! hardcore; roster, archers and the seed come from `MatchStart` like in
//! Versus. Quest takes 1-4 archers; Trials is a single archer (the other
//! machines watch). The co-op presentation comes from the local Quest /
//! Trials UI once it exists; until then the online scene shows a text view.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tfp_net::{MatchStart, SKIN_ALT};
use tfp_sim::Content;
use tfp_sim::darkworld::{self, DwSettings};
use tfp_sim::quest::{self, QuestPhase, QuestSettings};
use tfp_sim::trials::{self, TrialsPhase, TrialsSettings};
use tfp_sim::variants::Variant;
use tfp_sim::world::MAX_PLAYERS;

use super::adapter::{
    KIND_DARKWORLD, KIND_QUEST, KIND_TRIALS, NetWorld, OnlineSettings, levels_hash,
};

/// Most archers in an online quest (quest.md §7: extended co-op).
pub const QUEST_MAX: usize = 4;
/// Most archers in online Dark World (the local rollcall's limit).
pub const DW_MAX: usize = 4;

/// Co-op content of one install.
#[derive(Clone)]
pub struct CoopData {
    pub kind: u8,
    pub content: Arc<Content>,
    pub data_hash: u64,
    /// Quest tower names / trial level names, by `coop_index`.
    pub names: Vec<String>,
    pub trials: Vec<TrialsSettings>,
    pub base_archers: usize,
    pub alt_index: Vec<Option<u16>>,
    /// Enemy art registered into `content` (like `coop_ui::scene`).
    pub art: crate::coop_ui::art::Art,
    /// Dark World boss art (Dark World only).
    pub dw_art: crate::coop_ui::darkworld::bosses::DwArt,
    /// Quest towers / trials of the install (themes, titles, level files).
    pub index: tfp_assets::towers::TowerIndex,
}

/// Base archer count and each one's alt costume index in `Content::archers`.
pub fn archer_map(inst: &tfp_assets::GameInstall) -> Result<(usize, Vec<Option<u16>>)> {
    let gd = tfp_assets::game_data::GameData::load(&inst.content)?;
    let base = gd.archers.archers.len();
    let mut alt = Vec::with_capacity(base);
    let mut n = 0u16;
    for i in 0..base {
        let has = gd.archers.alts.get(i).is_some_and(|a| a.is_some());
        alt.push(has.then(|| base as u16 + n));
        if has {
            n += 1;
        }
    }
    Ok((base, alt))
}

impl CoopData {
    pub fn load(inst: &tfp_assets::GameInstall, kind: u8) -> Result<CoopData> {
        let idx = tfp_assets::towers::TowerIndex::build(inst)?;
        let (base_archers, alt_index) = archer_map(inst)?;
        let archer_file = inst.content.root.join("Atlas/GameData/archerData.xml");
        let (mut content, files, names, trials) = match kind {
            KIND_QUEST => {
                let c = tfp_sim::loader::quest_content(inst)?;
                let mut files: Vec<PathBuf> = Vec::new();
                for q in &idx.quest {
                    files.push(q.level.clone());
                    files.push(q.data.clone());
                }
                let names = idx.quest.iter().map(|q| q.name.clone()).collect();
                (c, files, names, Vec::new())
            }
            KIND_TRIALS => {
                let (c, settings) = tfp_sim::loader::trials_content(inst)?;
                let files = idx.trials.iter().map(|t| t.path.clone()).collect();
                let names = idx
                    .trials
                    .iter()
                    .map(|t| format!("{} {}-{}", t.theme, t.tier + 1, t.index + 1))
                    .collect();
                (c, files, names, settings)
            }
            KIND_DARKWORLD => {
                if inst.dark_world.is_none() {
                    bail!("Dark World needs the expansion content (DarkWorldContent)");
                }
                let c = tfp_sim::loader::dark_world_content(inst)?;
                let files = idx
                    .dark_world
                    .iter()
                    .flat_map(|t| t.levels.iter().cloned())
                    .collect();
                let names = idx.dark_world.iter().map(|t| t.theme.clone()).collect();
                (c, files, names, Vec::new())
            }
            k => bail!("not a co-op kind: {k}"),
        };
        // Prepared exactly like the local Quest / Trials scene: the arenas'
        // theme flags (gameplay) and the enemy art (presentation).
        let gd = tfp_assets::game_data::GameData::load(&inst.content)?;
        let themes: Vec<String> = match kind {
            KIND_QUEST => idx.quest.iter().map(|q| q.theme.clone()).collect(),
            // The arenas in the loader's order (towers, then files).
            KIND_DARKWORLD => idx
                .dark_world
                .iter()
                .flat_map(|t| std::iter::repeat_n(t.theme.clone(), t.levels.len()))
                .collect(),
            _ => idx.trials.iter().map(|t| t.theme.clone()).collect(),
        };
        for (lvl, th) in content.levels.iter_mut().zip(&themes) {
            if let Some(t) = gd.theme(th) {
                lvl.tower.theme = tfp_sim::entities::ThemeInfo::from_theme(t);
            }
        }
        let art = crate::coop_ui::art::Art::register(&mut content, inst);
        let dw_art = if kind == KIND_DARKWORLD {
            crate::coop_ui::darkworld::bosses::DwArt::register(&mut content, inst)
        } else {
            Default::default()
        };
        let mut hashed = files;
        hashed.push(archer_file);
        let data_hash =
            levels_hash(&hashed) ^ super::adapter::geometry_hash(&content).rotate_left(1);
        Ok(CoopData {
            kind,
            content: Arc::new(content),
            data_hash,
            names,
            trials,
            base_archers,
            alt_index,
            art,
            dw_art,
            index: idx,
        })
    }

    fn sim_archer(&self, archer: u8, skin: u8) -> u16 {
        let a = (archer as usize).min(self.base_archers.saturating_sub(1));
        match self.alt_index.get(a) {
            Some(Some(alt)) if skin == SKIN_ALT => *alt,
            _ => a as u16,
        }
    }

    /// Put this content into `s` (kind, data hash, level count).
    pub fn apply_to(&self, s: &mut OnlineSettings) {
        s.kind = self.kind;
        s.tower = match self.kind {
            KIND_QUEST => "QUEST".into(),
            KIND_DARKWORLD => "DARKWORLD".into(),
            _ => "TRIALS".into(),
        };
        s.levels = self.content.levels.len() as u16;
        s.data_hash = self.data_hash;
        s.wide = false;
        s.coop_index = s.coop_index.min(self.names.len().saturating_sub(1) as u16);
    }
}

/// The starting world of an online Quest / Trials run.
pub fn new_coop_world(ms: &MatchStart, os: &OnlineSettings, data: &CoopData) -> Result<NetWorld> {
    if os.kind != data.kind || os.data_hash != data.data_hash {
        bail!("this machine's co-op data differs from the host's (different game version?)");
    }
    let seed = (ms.seed ^ (ms.seed >> 32)) as i32;
    let i = os.coop_index as usize;
    let world = match data.kind {
        KIND_QUEST => {
            if ms.players.len() > QUEST_MAX {
                bail!("quest takes at most {QUEST_MAX} archers");
            }
            let mut s = QuestSettings::new(i, i, 0);
            for p in &ms.players {
                if (p.slot as usize) < MAX_PLAYERS {
                    s.slots[p.slot as usize] = Some(data.sim_archer(p.archer, p.skin));
                }
            }
            s.tower_id = i as u8;
            s.hardcore = os.hardcore;
            s.extended = ms.players.len() > 2;
            s.seed = seed;
            s.start_screen = true;
            quest::new_quest(&data.content, s)
        }
        KIND_DARKWORLD => {
            if ms.players.len() > DW_MAX {
                bail!("Dark World takes at most {DW_MAX} archers");
            }
            let mut s = DwSettings::new(i, os.difficulty.min(2), 0);
            for p in &ms.players {
                if (p.slot as usize) < MAX_PLAYERS {
                    s.slots[p.slot as usize] = Some(data.sim_archer(p.archer, p.skin));
                }
            }
            let joined = ms.players.iter().fold(0u16, |m, p| m | 1 << p.slot.min(15));
            for title in &os.variants {
                if let Some(v) = Variant::ALL.iter().find(|v| v.title() == title) {
                    s.variants.set_all(*v, true, joined);
                }
            }
            for (title, owners) in &os.player_variants {
                if let Some(v) = Variant::ALL.iter().find(|v| v.title() == title) {
                    for p in &ms.players {
                        if owners.contains(&(p.machine.0, p.local_index)) {
                            s.variants.set(*v, p.slot as usize, true);
                        }
                    }
                }
            }
            s.extended = ms.players.len() > 4;
            s.seed = seed;
            s.start_screen = true;
            s.trigger_tips = false;
            let mut w = darkworld::new_dark_world(&data.content, s);
            if let Some(l) = os.dw_level {
                darkworld::jump_to_level(&mut w, &data.content, l as usize);
            }
            w
        }
        _ => {
            let p = ms.players.first().context("trials needs an archer")?;
            let mut s = *data.trials.get(i).context("no such trial")?;
            s.archer = data.sim_archer(p.archer, p.skin);
            s.start_screen = true;
            s.seed ^= seed;
            trials::new_trial(&data.content, s)
        }
    };
    Ok(NetWorld {
        world,
        content: data.content.clone(),
        assist: if data.kind == KIND_DARKWORLD {
            os.dw_assist
        } else {
            None
        },
    })
}

/// A co-op run is over and acknowledged (Dark World: the complete
/// sequence has played; deterministic: it only reads the sim's state).
pub fn coop_finished(w: &tfp_sim::World) -> bool {
    if let Some(d) = &w.dark_world {
        return crate::coop_ui::darkworld::hud::DwHud::complete_done(d);
    }
    w.quest
        .as_ref()
        .is_some_and(|q| q.phase == QuestPhase::Finished)
        || w.trials
            .as_ref()
            .is_some_and(|t| t.phase == TrialsPhase::Finished)
}

/// One line for logs and the placeholder view.
pub fn coop_summary(w: &tfp_sim::World, players: usize) -> String {
    if let Some(q) = &w.quest {
        let v = q.view();
        // Gauntlet towers have no waves: their progress is the enemies left.
        let progress = if v.total_waves == 0 {
            match v.gauntlet {
                Some(n) => format!("gauntlet {n} left"),
                None => "gauntlet not started".into(),
            }
        } else {
            format!("wave {}/{}", v.wave + 1, v.total_waves)
        };
        format!(
            "quest phase {:?} {progress} kills {:?} lives {:?}",
            v.phase,
            &v.kills[..players.min(MAX_PLAYERS)],
            &v.lives[..players.min(MAX_PLAYERS)]
        )
    } else if let Some(d) = &w.dark_world {
        format!(
            "darkworld phase {:?} level {} boss {}/{} deaths {:?} continues {}",
            d.phase,
            d.round_index + 1,
            d.boss_health,
            d.max_boss_health,
            &d.deaths[..players.min(MAX_PLAYERS)],
            d.continues
        )
    } else if let Some(t) = &w.trials {
        format!(
            "trials phase {:?} time {} dummies {}",
            t.phase,
            trials::time_string(t.time),
            t.drawing
        )
    } else {
        "no co-op".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tfp_net::NetGame;
    use tfp_sim::PlayerInput;
    use tfp_sim::quest::QuestScript;

    const MAP: [&str; 24] = [
        "################################",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#.....S..................S.....#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#..............................#",
        "#.T..........P...P..........T..#",
        "################################",
        "################################",
    ];

    fn data() -> CoopData {
        let mut c = Content::from_ascii(&MAP);
        c.quest_scripts = vec![QuestScript::simple(&[(&["S0", "S1"], &[("Slime", 2)])])];
        CoopData {
            kind: KIND_QUEST,
            content: Arc::new(c),
            data_hash: 77,
            names: vec!["TEST".into()],
            trials: vec![],
            base_archers: 4,
            alt_index: vec![None; 4],
            art: Default::default(),
            dw_art: Default::default(),
            index: Default::default(),
        }
    }

    fn start(n: usize, settings: &OnlineSettings) -> MatchStart {
        MatchStart {
            match_id: 1,
            seed: 0x1234_5678_9abc,
            settings: tfp_net::MatchSettings {
                game: settings.encode(),
                ..Default::default()
            },
            players: (0..n)
                .map(|slot| tfp_net::PlayerSlot {
                    slot: slot as u8,
                    machine: tfp_net::MachineId(slot as u8),
                    local_index: 0,
                    name: String::new(),
                    archer: slot as u8,
                    skin: 0,
                    dup: 0,
                    team: 0,
                })
                .collect(),
            ggrs: Default::default(),
            spectators: vec![],
        }
    }

    #[test]
    fn quest_world_from_the_lobby() {
        let d = data();
        let mut os = OnlineSettings::default();
        d.apply_to(&mut os);
        assert_eq!(
            (os.kind, os.data_hash, os.tower.as_str()),
            (KIND_QUEST, 77, "QUEST")
        );
        let ms = start(2, &os);
        let mut a = new_coop_world(&ms, &os, &d).unwrap();
        let q = a.world.quest.as_ref().unwrap();
        assert!(q.settings.slots[0].is_some() && q.settings.slots[1].is_some());
        assert!(q.settings.slots[2].is_none());
        // Same lobby, same world; rollback from a clone is exact.
        let b = new_coop_world(&ms, &os, &d).unwrap();
        assert_eq!(a.checksum(), b.checksum());
        let inp = |f: u32| {
            vec![
                (
                    PlayerInput::dpad((f % 3) as i8 - 1, 0, f.is_multiple_of(7) as u8),
                    tfp_net::InputStatus::Confirmed
                );
                2
            ]
        };
        for f in 0..300 {
            a.advance(&inp(f));
        }
        let mut c = a.clone();
        for f in 300..400 {
            a.advance(&inp(f));
            c.advance(&inp(f));
        }
        assert_eq!(a.checksum(), c.checksum());
        // Different data or too many archers are refused.
        let mut bad = os.clone();
        bad.data_hash = 1;
        assert!(new_coop_world(&ms, &bad, &d).is_err());
        assert!(new_coop_world(&start(QUEST_MAX + 1, &os), &os, &d).is_err());
    }
}

#[cfg(test)]
mod dw_tests {
    use super::*;
    use crate::online::adapter::{ACTION_CONTINUE, ACTION_RESTART};
    use tfp_net::{InputStatus, NetGame};
    use tfp_sim::PlayerInput;

    /// The host's menu choice in slot 0's input continues exactly once,
    /// identically on every copy of the world (no install: skipped).
    #[test]
    fn host_menu_actions_are_deterministic() {
        let installs = tfp_assets::Installs::discover();
        let Some(inst) = installs.steam.clone().filter(|i| i.dark_world.is_some()) else {
            eprintln!("skipping: no Dark World data");
            return;
        };
        let data = CoopData::load(&inst, KIND_DARKWORLD).unwrap();
        let mut os = OnlineSettings::default();
        data.apply_to(&mut os);
        os.dw_level = Some(1);
        let ms = tfp_net::MatchStart {
            match_id: 1,
            seed: 77,
            settings: tfp_net::MatchSettings {
                game: os.encode(),
                ..Default::default()
            },
            players: (0..2)
                .map(|slot| tfp_net::PlayerSlot {
                    slot,
                    machine: tfp_net::MachineId(slot),
                    local_index: 0,
                    name: String::new(),
                    archer: slot,
                    skin: 0,
                    dup: 0,
                    team: 0,
                })
                .collect(),
            ggrs: Default::default(),
            spectators: vec![],
        };
        let mut a = new_coop_world(&ms, &os, &data).unwrap();
        let idle = |action: u8| {
            let mut i = [(PlayerInput::default(), InputStatus::Confirmed); 2];
            i[0].0._pad = action;
            i
        };
        // Past the cards into play (tapping Jump), then a game over (the
        // local proof's helper).
        for f in 0..3000 {
            let mut i = idle(0);
            if f % 2 == 0 {
                i[0].0.buttons = PlayerInput::JUMP;
            }
            a.advance(&i);
            if a.world
                .dark_world
                .as_ref()
                .is_some_and(|d| d.phase == tfp_sim::darkworld::DwPhase::Playing)
            {
                break;
            }
        }
        crate::coop_ui::darkworld::force_end(&mut a.world, &a.content, false);
        let mut ready = false;
        for _ in 0..1200 {
            a.advance(&idle(0));
            if a.world.dark_world.as_ref().is_some_and(|d| d.menu_ready) {
                ready = true;
                break;
            }
        }
        assert!(ready, "the game-over menu comes up");
        let mut b = a.clone();
        for _ in 0..5 {
            a.advance(&idle(ACTION_CONTINUE));
            b.advance(&idle(ACTION_CONTINUE));
        }
        let d = a.world.dark_world.as_ref().unwrap();
        assert_eq!(
            d.continues, 1,
            "continued once despite five frames of the action"
        );
        assert_eq!(a.checksum(), b.checksum());
        // Restart from a fresh game over.
        for f in 0..3000 {
            let mut i = idle(0);
            if f % 2 == 0 {
                i[0].0.buttons = PlayerInput::JUMP;
            }
            a.advance(&i);
            if a.world
                .dark_world
                .as_ref()
                .is_some_and(|d| d.phase == tfp_sim::darkworld::DwPhase::Playing)
            {
                break;
            }
        }
        crate::coop_ui::darkworld::force_end(&mut a.world, &a.content, false);
        for _ in 0..1200 {
            a.advance(&idle(0));
            if a.world.dark_world.as_ref().is_some_and(|d| d.menu_ready) {
                break;
            }
        }
        a.advance(&idle(ACTION_RESTART));
        let d = a.world.dark_world.as_ref().unwrap();
        assert_eq!(
            (d.round_index, d.continues),
            (0, 0),
            "a new run on the same settings"
        );
    }
}
