//! The host's match settings and the variant screen (per-player variants
//! by archer, stored by (machine, local player) so they stay with the
//! player when the slot table is renumbered).

use tfp_render::Color;

use super::{OnlineScene, Screen};
use crate::front::gfx::pal;
use crate::front::minput::b;
use crate::front::variants::{Availability, VARIANTS, linked};
use crate::online::adapter::{
    KIND_DARKWORLD, KIND_NAMES, KIND_QUEST, KIND_TRIALS, LENGTH_NAMES, MODE_NAMES, tower_title,
};

/// Variant rows shown at once.
const VARIANT_ROWS: usize = 13;

impl OnlineScene {
    pub(super) fn settings_rows(&self, creating: bool) -> Vec<&'static str> {
        let mut rows = Vec::new();
        if creating {
            rows.push("ROOM");
        }
        rows.push("GAME");
        match self.settings.kind {
            KIND_QUEST => rows.extend(["QUEST", "HARDCORE"]),
            KIND_TRIALS => rows.push("TRIAL"),
            KIND_DARKWORLD => rows.extend(["DW TOWER", "DIFFICULTY", "VARIANTS"]),
            _ => {
                rows.extend(["MODE", "LENGTH"]);
                if self.settings.mode == 2 {
                    rows.push("TEAMS");
                }
                rows.extend(["TOWER", "VARIANTS"]);
            }
        }
        rows.push(if creating { "CREATE ROOM" } else { "DONE" });
        rows
    }

    fn row_text(&self, row: &str) -> String {
        let s = &self.settings;
        match row {
            "ROOM" => format!(
                "ROOM: < {} >",
                if self.public {
                    "PUBLIC"
                } else {
                    "PRIVATE (CODE ONLY)"
                }
            ),
            "GAME" => format!("GAME: < {} >", KIND_NAMES[s.kind.min(3) as usize]),
            "DIFFICULTY" => format!(
                "DIFFICULTY: < {} >",
                crate::coop_ui::darkworld::difficulty_name(s.difficulty)
            ),
            "QUEST" | "TRIAL" | "DW TOWER" => format!(
                "{row}: < {} >",
                self.coop
                    .as_ref()
                    .and_then(|c| c.names.get(s.coop_index as usize))
                    .cloned()
                    .unwrap_or_else(|| (s.coop_index + 1).to_string())
            ),
            "HARDCORE" => format!("HARDCORE: < {} >", if s.hardcore { "ON" } else { "OFF" }),
            "MODE" => format!("MODE: < {} >", MODE_NAMES[s.mode.min(2) as usize]),
            "LENGTH" => format!("LENGTH: < {} >", LENGTH_NAMES[s.length.min(3) as usize]),
            "TEAMS" => format!("TEAMS: < {} >", s.teams.clamp(2, 6)),
            "TOWER" => {
                let t = if s.tower.is_empty() {
                    self.tower_names.first().cloned().unwrap_or_default()
                } else {
                    s.tower.clone()
                };
                format!("TOWER: < {} >", tower_title(&t))
            }
            "VARIANTS" => {
                let n = s.variants.len() + s.player_variants.len();
                if n == 0 {
                    "VARIANTS: NONE".into()
                } else {
                    format!("VARIANTS: {n} ON")
                }
            }
            r => r.to_string(),
        }
    }

    pub(super) fn tick_settings(&mut self, sel: usize, creating: bool) {
        let rows = self.settings_rows(creating);
        let n = rows.len();
        let mut sel = sel.min(n - 1);
        if self.mi.p(b::UP) {
            sel = (sel + n - 1) % n;
        }
        if self.mi.p(b::DOWN) {
            sel = (sel + 1) % n;
        }
        let d = self.mi.p(b::RIGHT) as i32 - self.mi.p(b::LEFT) as i32;
        let mut changed = false;
        if d != 0 {
            changed = true;
            let s = &mut self.settings;
            match rows[sel] {
                "ROOM" => self.public = !self.public,
                "GAME" => {
                    s.kind = ((s.kind as i32 + d).rem_euclid(4)) as u8;
                    s.coop_index = 0;
                }
                "DIFFICULTY" => s.difficulty = ((s.difficulty as i32 + d).rem_euclid(3)) as u8,
                "QUEST" | "TRIAL" | "DW TOWER" => {
                    let n = self.coop.as_ref().map_or(1, |c| c.names.len().max(1)) as i32;
                    s.coop_index = ((s.coop_index as i32 + d).rem_euclid(n)) as u16;
                }
                "HARDCORE" => s.hardcore = !s.hardcore,
                "MODE" => s.mode = ((s.mode as i32 + d).rem_euclid(3)) as u8,
                "LENGTH" => s.length = ((s.length as i32 + d).rem_euclid(4)) as u8,
                "TEAMS" => s.teams = (s.teams.clamp(2, 6) as i32 + d).clamp(2, 6) as u8,
                "TOWER" if !self.tower_names.is_empty() => {
                    let k = self
                        .tower_names
                        .iter()
                        .position(|t| *t == s.tower)
                        .unwrap_or(0) as i32;
                    let m = self.tower_names.len() as i32;
                    s.tower = self.tower_names[(k + d).rem_euclid(m) as usize].clone();
                }
                _ => changed = false,
            }
        }
        if changed && rows[sel] == "GAME" {
            self.ensure_coop();
        }
        if changed && !creating {
            self.apply_settings();
        }
        self.screen = Screen::Settings { sel, creating };
        let confirm = self.mi.p(b::CONFIRM) || self.mi.p(b::START);
        if confirm && rows[sel] == "VARIANTS" {
            self.screen = Screen::Variants {
                sel: 0,
                col: 0,
                creating,
            };
            return;
        }
        let done = sel == n - 1 && confirm;
        if done && creating {
            self.host_room();
        } else if done || self.mi.p(b::BACK) {
            self.screen = if creating {
                Screen::Menu { sel: 0 }
            } else {
                Screen::Lobby
            };
        }
    }

    pub(super) fn settings_lines(&self, sel: usize, creating: bool) -> Vec<(String, Color)> {
        let mut out = vec![
            (
                if creating {
                    "NEW ROOM"
                } else {
                    "MATCH SETTINGS"
                }
                .to_string(),
                Color::WHITE,
            ),
            (String::new(), Color::WHITE),
        ];
        for (i, r) in self.settings_rows(creating).into_iter().enumerate() {
            let col = if i == sel { pal::SELECTED } else { pal::IDLE };
            out.push((self.row_text(r), col));
        }
        out
    }

    // ------------------------------------------------------------ variants

    /// Usable variants (indices into the front end's catalogue): Dark World
    /// ones only when every machine has Dark World.
    fn variant_list(&self) -> Vec<usize> {
        let dw = self.all_dark_world();
        let avail = Availability {
            dark_world: dw,
            scroll_effects: true,
        };
        // Dark World: its power-ups and curses (darkworld.md 5.6).
        let dw: Vec<String> = tfp_sim::darkworld::POWER_UPS
            .iter()
            .chain(tfp_sim::darkworld::CURSES.iter())
            .map(|v| v.title().to_string())
            .collect();
        let darkworld = self.settings.kind == KIND_DARKWORLD;
        (0..VARIANTS.len())
            .filter(|&i| avail.usable(&VARIANTS[i]))
            .filter(|&i| !darkworld || dw.contains(&VARIANTS[i].title()))
            .collect()
    }

    /// The archers a per-player variant can be set for: the lobby preview
    /// as (machine, local index, slot).
    fn variant_players(&self) -> Vec<(u8, u8, u8)> {
        self.net
            .as_ref()
            .map(|n| {
                n.lobby()
                    .preview
                    .iter()
                    .map(|p| (p.machine.0, p.local_index, p.slot))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn global_on(&self, title: &str) -> bool {
        self.settings.variants.iter().any(|t| t == title)
    }

    fn players_on(&self, title: &str) -> Vec<(u8, u8)> {
        self.settings
            .player_variants
            .iter()
            .find(|(t, _)| t == title)
            .map(|(_, o)| o.clone())
            .unwrap_or_default()
    }

    fn set_players(&mut self, title: &str, owners: Vec<(u8, u8)>) {
        let pv = &mut self.settings.player_variants;
        pv.retain(|(t, _)| t != title);
        if !owners.is_empty() {
            pv.push((title.to_string(), owners));
        }
    }

    /// Turn variant `i` off for everyone (both lists).
    fn clear_variant(&mut self, i: usize) {
        let t = VARIANTS[i].title();
        self.settings.variants.retain(|x| *x != t);
        self.set_players(&t, Vec::new());
    }

    /// Toggle variant `i` for everyone (col 0) or one archer (col >= 1).
    fn toggle_variant(&mut self, i: usize, col: usize) {
        let title = VARIANTS[i].title();
        let players = self.variant_players();
        let turning_on;
        if col == 0 || !VARIANTS[i].per_player {
            turning_on = !self.global_on(&title);
            self.clear_variant(i);
            if turning_on {
                self.settings.variants.push(title.clone());
            }
        } else {
            let Some(&(m, li, _)) = players.get(col - 1) else {
                return;
            };
            let mut owners = if self.global_on(&title) {
                // From "everyone" to "everyone but this archer".
                players.iter().map(|p| (p.0, p.1)).collect()
            } else {
                self.players_on(&title)
            };
            self.settings.variants.retain(|x| *x != title);
            turning_on = !owners.contains(&(m, li));
            if turning_on {
                owners.push((m, li));
            } else {
                owners.retain(|o| *o != (m, li));
            }
            self.set_players(&title, owners);
        }
        if turning_on {
            for j in linked(i) {
                self.clear_variant(j);
            }
        }
    }

    pub(super) fn tick_variants(&mut self, sel: usize, col: usize, creating: bool) {
        let list = self.variant_list();
        let n = list.len().max(1);
        let mut sel = sel.min(n - 1);
        if self.mi.p(b::UP) {
            sel = (sel + n - 1) % n;
        }
        if self.mi.p(b::DOWN) {
            sel = (sel + 1) % n;
        }
        let per = list.get(sel).is_some_and(|&i| VARIANTS[i].per_player);
        let cols = if per {
            1 + self.variant_players().len()
        } else {
            1
        };
        let d = self.mi.p(b::RIGHT) as i32 - self.mi.p(b::LEFT) as i32;
        let col = ((col.min(cols - 1) as i32 + d).rem_euclid(cols as i32)) as usize;
        if self.mi.p(b::CONFIRM)
            && let Some(&i) = list.get(sel)
        {
            self.toggle_variant(i, col);
            if !creating {
                self.apply_settings();
            }
        }
        self.screen = Screen::Variants { sel, col, creating };
        if self.mi.p(b::BACK) || self.mi.p(b::START) {
            if !creating {
                self.apply_settings();
            }
            self.screen = Screen::Settings {
                sel: self
                    .settings_rows(creating)
                    .iter()
                    .position(|r| *r == "VARIANTS")
                    .unwrap_or(0),
                creating,
            };
        }
    }

    pub(super) fn variants_lines(&self, sel: usize, col: usize) -> Vec<(String, Color)> {
        let list = self.variant_list();
        let players = self.variant_players();
        let mut out = vec![(
            "VARIANTS  (CONFIRM: TOGGLE, LEFT/RIGHT: ALL / ONE ARCHER)".to_string(),
            Color::WHITE,
        )];
        let first = sel
            .saturating_sub(VARIANT_ROWS / 2)
            .min(list.len().saturating_sub(VARIANT_ROWS));
        for (k, &i) in list.iter().enumerate().skip(first).take(VARIANT_ROWS) {
            let v = &VARIANTS[i];
            let title = v.title();
            let all = self.global_on(&title);
            let owners = self.players_on(&title);
            let mut line = format!("{title}: ");
            let mark = |on: bool, at: bool, s: &str| match (on, at) {
                (true, true) => format!("[{s}*]"),
                (true, false) => format!("{s}*"),
                (false, true) => format!("[{s}]"),
                (false, false) => s.to_string(),
            };
            let cur = k == sel;
            line.push_str(&mark(
                all,
                cur && col == 0,
                if v.per_player { "ALL" } else { "ON" },
            ));
            if v.per_player {
                for (c, p) in players.iter().enumerate() {
                    line.push(' ');
                    let on = all || owners.contains(&(p.0, p.1));
                    line.push_str(&mark(on, cur && col == c + 1, &format!("P{}", p.2 + 1)));
                }
            }
            let color = if cur {
                pal::SELECTED
            } else if all || !owners.is_empty() {
                pal::YELLOW
            } else {
                pal::IDLE
            };
            out.push((line, color));
        }
        out.push((String::new(), Color::WHITE));
        out.push(("* = ON.  BACK: DONE".into(), pal::IDLE));
        out
    }
}
