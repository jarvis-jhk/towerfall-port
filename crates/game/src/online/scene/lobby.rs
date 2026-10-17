//! The lobby on screen: every archer of the next match as a rollcall
//! portrait card (local and remote alike: archer, alt costume, team, the
//! owning machine, its ready state), the machines with their ping, the
//! settings, and the host's machine list (kick).

use tfp_render::Color;
use tfp_render::glam::Vec2;

use super::{OnlineScene, Screen};
use crate::front::assets::Archer;
use crate::front::gfx::{Org, Painter, pal, rgba};
use crate::front::minput::b;
use crate::online::adapter::{LENGTH_NAMES, MODE_NAMES, OnlineSettings, TowerSize, tower_title};

/// Team colours (blue, red, then the extra teams).
const TEAMS: [Color; 6] = [
    Color::hex(0x3CBCFC),
    Color::hex(0xF83800),
    Color::hex(0x58D854),
    Color::hex(0xF8B800),
    Color::hex(0xD800CC),
    Color::hex(0xF8F8F8),
];

fn look(archers: &[Archer], archer: u8, skin: u8) -> Option<&Archer> {
    let base = archers.get(archer as usize)?;
    Some(match skin {
        1 => base.alt.as_deref().unwrap_or(base),
        2 => base.secret.as_deref().unwrap_or(base),
        _ => base,
    })
}

impl OnlineScene {
    /// Ping to a machine in ms (guests: everything goes via the host).
    fn ping_of(&self, machine: tfp_net::MachineId) -> Option<u32> {
        let n = self.net.as_ref()?;
        let s = n.stats();
        let host = tfp_net::MachineId(0);
        let link = |m| {
            s.machines
                .iter()
                .find(|x| x.machine == m)
                .and_then(|x| x.link_rtt_ms)
        };
        if n.is_host() || machine == host {
            link(machine)
        } else {
            // Guest to guest: our link to the host plus the host's to it is
            // not known here; show the link to the host.
            link(host)
        }
    }

    pub(super) fn draw_lobby(&self, p: &mut Painter) {
        let Some(n) = &self.net else { return };
        let lobby = n.lobby();
        let s: OnlineSettings = self.settings_in_lobby();
        let cx = p.w / 2.0;
        let head = match &self.room_code {
            Some(c) => format!("ROOM CODE {c}"),
            None if n.is_host() => "PUBLIC ROOM".into(),
            None => "LOBBY".into(),
        };
        p.text_outlined(&head, Vec2::new(cx, 34.0), (0.5, 0.0), 1.0, pal::SUBTITLE);
        let mut line = if s.kind != crate::online::adapter::KIND_VERSUS {
            let name = self
                .coop
                .as_ref()
                .and_then(|c| c.names.get(s.coop_index as usize))
                .cloned()
                .unwrap_or_default();
            format!(
                "{} {}{}",
                crate::online::adapter::KIND_NAMES[s.kind.min(3) as usize],
                name.to_ascii_uppercase(),
                if s.kind == crate::online::adapter::KIND_DARKWORLD {
                    format!(
                        "  {}",
                        crate::coop_ui::darkworld::difficulty_name(s.difficulty)
                    )
                } else if s.hardcore {
                    "  HARDCORE".to_string()
                } else {
                    String::new()
                }
            )
        } else {
            format!(
                "{}  {}  {}",
                MODE_NAMES[s.mode.min(2) as usize],
                LENGTH_NAMES[s.length.min(3) as usize],
                tower_title(&s.tower)
            )
        };
        if s.mode == 2 {
            line.push_str(&format!("  {} TEAMS", s.teams.clamp(2, 6)));
        }
        let nv = s.variants.len() + s.player_variants.len();
        if nv > 0 {
            line.push_str(&format!("  {nv} VARIANTS"));
        }
        p.text(&line, Vec2::new(cx, 44.0), (0.5, 0.0), 1.0, Color::WHITE);
        let size = if n.is_host() {
            self.tower_size.label()
        } else if s.wide {
            TowerSize::Wide.label()
        } else {
            "STANDARD TOWERS"
        };
        p.text(
            size,
            Vec2::new(cx, 54.0),
            (0.5, 0.0),
            1.0,
            if s.wide { pal::GOLD } else { pal::IDLE },
        );
        // Archer cards: six per row.
        let a = p.a;
        let me = n.me();
        let cards = &lobby.preview;
        let per_row = 6usize;
        let (cw, ch) = (50.0, 62.0);
        for (k, pl) in cards.iter().enumerate() {
            let row = k / per_row;
            let in_row = cards.len().saturating_sub(row * per_row).min(per_row);
            let col = k % per_row;
            let x0 = cx - in_row as f32 * cw / 2.0 + col as f32 * cw + cw / 2.0;
            let o = Vec2::new(x0, 92.0 + row as f32 * (ch + 8.0));
            let mv = lobby.machines.iter().find(|m| m.machine == pl.machine);
            let arch = look(&a.archers, pl.archer, pl.skin);
            let ca = arch.map(|x| rgba(x.color_a)).unwrap_or(Color::WHITE);
            let ready = mv.is_some_and(|m| m.ready || m.machine.0 == 0);
            // The frame: the archer's colour, or the team's in Team Deathmatch.
            let frame = if s.mode == 2 {
                TEAMS[pl.team as usize % 6]
            } else {
                ca
            };
            p.rect(o.x - 24.0, o.y - 24.0, 48.0, 48.0, frame);
            match arch.and_then(|x| a.short_portrait(&x.joined)) {
                Some(pic) => p.tex(pic, o, Org::Center, 0.7, Color::WHITE),
                None => p.rect(o.x - 21.0, o.y - 21.0, 42.0, 42.0, ca * 0.5),
            }
            p.text_outlined(
                &format!("P{}", pl.slot + 1),
                o + Vec2::new(-21.0, -21.0),
                (0.0, 0.0),
                1.0,
                ca,
            );
            if pl.skin == 1 {
                p.text_outlined(
                    "ALT",
                    o + Vec2::new(21.0, -21.0),
                    (1.0, 0.0),
                    1.0,
                    pal::GOLD,
                );
            }
            if ready {
                p.text_outlined(
                    "OK",
                    o + Vec2::new(21.0, 13.0),
                    (1.0, 0.0),
                    1.0,
                    pal::SELECTED,
                );
            }
            let owner: String = mv
                .map(|m| m.name.to_ascii_uppercase())
                .unwrap_or_default()
                .chars()
                .take(8)
                .collect();
            let col = if Some(pl.machine) == me {
                pal::SELECTED
            } else {
                pal::IDLE
            };
            p.text(&owner, o + Vec2::new(0.0, 28.0), (0.5, 0.0), 1.0, col);
        }
        if cards.is_empty() {
            p.text(
                "NO ARCHERS YET: PRESS CONFIRM ON A CONTROLLER OR THE KEYBOARD",
                Vec2::new(cx, 100.0),
                (0.5, 0.5),
                1.0,
                pal::IDLE,
            );
        }
        // Machines: name, ping, ready.
        let mut x = 6.0;
        let y = 196.0;
        for mv in &lobby.machines {
            let ping = self
                .ping_of(mv.machine)
                .map(|ms| format!(" {ms}MS"))
                .unwrap_or_default();
            let t = format!(
                "{}{}{}{}",
                mv.name
                    .to_ascii_uppercase()
                    .chars()
                    .take(10)
                    .collect::<String>(),
                if mv.machine.0 == 0 { "*" } else { "" },
                if Some(mv.machine) == me { "" } else { &ping },
                if mv.machine.0 == 0 || mv.ready {
                    ""
                } else {
                    " ..."
                }
            );
            let col = if mv.ready || mv.machine.0 == 0 {
                pal::SELECTED
            } else {
                pal::YELLOW
            };
            p.text(&t, Vec2::new(x, y), (0.0, 0.0), 1.0, col);
            x += p.measure(&t).x + 10.0;
            if x > p.w - 40.0 {
                break;
            }
        }
        if let tfp_net::Phase::InMatch { .. } = lobby.phase {
            p.text(
                "A MATCH IS RUNNING: YOU PLAY IN THE NEXT ONE",
                Vec2::new(cx, 184.0),
                (0.5, 0.0),
                1.0,
                pal::YELLOW,
            );
        }
        p.text(
            "CONFIRM JOIN  LEFT/RIGHT ARCHER  UP ALT  DOWN TEAM",
            Vec2::new(cx, 208.0),
            (0.5, 0.0),
            1.0,
            pal::IDLE,
        );
        let hint = if n.is_host() {
            "START: START  F2: SETTINGS  F3: MACHINES  BACK: LEAVE"
        } else {
            "START: READY  BACK: LEAVE"
        };
        p.text(hint, Vec2::new(cx, 217.0), (0.5, 0.0), 1.0, pal::IDLE);
    }

    // ------------------------------------------------------------ machines

    pub(super) fn tick_machines(&mut self, sel: usize, armed: bool) {
        let Some(n) = &self.net else { return };
        let ms: Vec<(tfp_net::MachineId, String)> = n
            .lobby()
            .machines
            .iter()
            .filter(|m| m.machine.0 != 0)
            .map(|m| (m.machine, m.name.clone()))
            .collect();
        let cnt = ms.len().max(1);
        let mut sel = sel.min(cnt - 1);
        let mut armed = armed;
        if self.mi.p(b::UP) {
            sel = (sel + cnt - 1) % cnt;
            armed = false;
        }
        if self.mi.p(b::DOWN) {
            sel = (sel + 1) % cnt;
            armed = false;
        }
        if self.mi.p(b::CONFIRM)
            && let Some((m, name)) = ms.get(sel)
        {
            if armed {
                if let Some(n) = &mut self.net {
                    n.kick(*m, false);
                }
                self.status = format!("KICKED {}", name.to_ascii_uppercase());
                armed = false;
            } else {
                armed = true;
            }
        }
        self.screen = Screen::Machines { sel, armed };
        if self.mi.p(b::BACK) || self.mi.p(b::START) {
            self.screen = Screen::Lobby;
        }
    }

    pub(super) fn machines_lines(&self, sel: usize, armed: bool) -> Vec<(String, Color)> {
        let mut out = vec![("MACHINES IN THE ROOM".to_string(), Color::WHITE)];
        let Some(n) = &self.net else { return out };
        out.push((String::new(), Color::WHITE));
        let mut k = 0;
        for mv in &n.lobby().machines {
            let ping = self
                .ping_of(mv.machine)
                .map(|ms| format!("{ms} MS"))
                .unwrap_or_else(|| "-".into());
            let info = crate::online::adapter::MachineInfo::decode(&mv.info);
            let line = format!(
                "{}  {} ARCHERS  {}  {}{}",
                mv.name.to_ascii_uppercase(),
                mv.slots.len(),
                if mv.machine.0 == 0 {
                    "HOST".to_string()
                } else {
                    ping
                },
                if mv.ready || mv.machine.0 == 0 {
                    "READY"
                } else {
                    "NOT READY"
                },
                if info.is_some_and(|i| i.wide.is_some()) {
                    "  8P"
                } else {
                    ""
                }
            );
            if mv.machine.0 == 0 {
                out.push((line, pal::IDLE));
                continue;
            }
            let cur = k == sel;
            k += 1;
            let col = if cur && armed {
                pal::RED
            } else if cur {
                pal::SELECTED
            } else {
                pal::IDLE
            };
            out.push((line, col));
        }
        out.push((String::new(), Color::WHITE));
        out.push((
            if armed {
                "CONFIRM AGAIN TO KICK"
            } else {
                "CONFIRM: KICK  BACK: LOBBY"
            }
            .into(),
            if armed { pal::RED } else { pal::IDLE },
        ));
        out
    }
}
