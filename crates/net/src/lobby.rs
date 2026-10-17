//! Host-side slot assignment: deterministic, pure, unit-tested.
//!
//! Order: machines by id (the host, 0, first), each machine's slots in its own
//! order, until [`MAX_SLOTS`]. Slots that do not fit are left out of the match
//! (the lobby shows which, see [`accepted_slots`]).
//!
//! Archers (TowerFall gives each archer to one player): a request is honoured
//! if that archer is still free, else the slot gets the next free archer after
//! it (wrapping), like the original's archer select skipping taken archers.
//! With more players than archers, once all are taken the request is honoured
//! as a duplicate and numbered (`dup` 1, 2, ...) so the renderer can tint it.
//! Skins (normal/alt/secret) are the player's choice.
//!
//! Teams: free-for-all gives every slot its own team; with `n` teams a valid
//! request is honoured, otherwise the slot joins the smallest team (lowest
//! index on ties).

use crate::wire::{
    MAX_SLOTS, MachineId, MachineView, PlayerSlot, SKIN_SECRET, SlotRequest, TeamMode,
};
use std::collections::BTreeMap;

pub fn clean_name(s: &str, fallback: &str) -> String {
    let t: String = s.chars().filter(|c| !c.is_control()).take(16).collect();
    let t = t.trim().to_string();
    if t.is_empty() {
        fallback.to_string()
    } else {
        t
    }
}

/// Assign global slots for the machines that play (callers pass only the
/// machines that should play). `archers` = number of distinct archers
/// (0 = take requests as they are, numbering duplicates).
pub fn assign(machines: &[MachineView], mode: TeamMode, archers: u8) -> Vec<PlayerSlot> {
    let mut ms: Vec<&MachineView> = machines.iter().collect();
    ms.sort_by_key(|m| m.machine);
    let mut out: Vec<PlayerSlot> = Vec::new();
    let mut taken: BTreeMap<u8, u8> = BTreeMap::new();
    let teams = match mode {
        TeamMode::Ffa => 0,
        TeamMode::Teams(n) => n.clamp(2, MAX_SLOTS as u8),
    };
    let mut team_size = vec![0usize; teams as usize];
    for m in ms {
        for (li, req) in m.slots.iter().enumerate() {
            if out.len() >= MAX_SLOTS {
                return out;
            }
            let slot = out.len() as u8;
            let archer = if archers == 0 {
                req.archer
            } else {
                let want = req.archer % archers;
                (0..archers)
                    .map(|k| (want + k) % archers)
                    .find(|a| !taken.contains_key(a))
                    .unwrap_or(want)
            };
            let n = taken.entry(archer).or_insert(0);
            let dup = *n;
            *n = n.saturating_add(1);
            let team = if teams == 0 {
                slot
            } else {
                match req.team {
                    Some(t) if t < teams => t,
                    _ => (0..teams)
                        .min_by_key(|t| (team_size[*t as usize], *t))
                        .unwrap_or(0),
                }
            };
            if teams > 0 {
                team_size[team as usize] += 1;
            }
            out.push(PlayerSlot {
                slot,
                machine: m.machine,
                local_index: li as u8,
                name: clean_name(
                    &req.name,
                    &format!("{} {}", clean_name(&m.name, "player"), li + 1),
                ),
                archer,
                skin: if req.skin <= SKIN_SECRET { req.skin } else { 0 },
                dup,
                team,
            });
        }
    }
    out
}

/// How many of each machine's slots fit, in lobby order (for display).
pub fn accepted_slots(machines: &[MachineView]) -> Vec<(MachineId, usize)> {
    let mut ms: Vec<&MachineView> = machines.iter().collect();
    ms.sort_by_key(|m| m.machine);
    let mut left = MAX_SLOTS;
    ms.iter()
        .map(|m| {
            let n = m.slots.len().min(left);
            left -= n;
            (m.machine, n)
        })
        .collect()
}

pub fn sanitize_slots(v: Vec<SlotRequest>) -> Vec<SlotRequest> {
    v.into_iter()
        .take(MAX_SLOTS)
        .map(|s| SlotRequest {
            name: clean_name(&s.name, ""),
            ..s
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::SKIN_ALT;

    fn mach(id: u8, n: usize, archer: u8, team: Option<u8>) -> MachineView {
        MachineView {
            machine: MachineId(id),
            account: id as i64,
            name: format!("m{id}"),
            ready: true,
            waiting: false,
            spectate: false,
            rejoined_from: None,
            info: Vec::new(),
            slots: (0..n)
                .map(|i| SlotRequest {
                    name: format!("p{id}.{i}"),
                    archer,
                    skin: (i % 2) as u8,
                    team,
                })
                .collect(),
        }
    }

    #[test]
    fn order_cap_and_twelve_archers() {
        let ms = vec![
            mach(2, 5, 0, None),
            mach(0, 4, 0, None),
            mach(1, 5, 0, None),
        ];
        let s = assign(&ms, TeamMode::Ffa, 12);
        assert_eq!(s.len(), 12);
        assert_eq!(s[0].machine, MachineId(0));
        assert_eq!(s[4].machine, MachineId(1));
        assert_eq!(s[9].machine, MachineId(2));
        assert_eq!(s[11].local_index, 2);
        assert!(
            s.iter()
                .enumerate()
                .all(|(i, p)| p.slot as usize == i && p.team as usize == i)
        );
        // Everybody asked for archer 0: they get 0, 1, 2, ... in order.
        let a: Vec<u8> = s.iter().map(|p| p.archer).collect();
        assert_eq!(a, (0..12).collect::<Vec<u8>>());
        assert!(s.iter().all(|p| p.dup == 0));
        assert_eq!(s[1].skin, SKIN_ALT);
        assert_eq!(
            accepted_slots(&ms),
            vec![(MachineId(0), 4), (MachineId(1), 5), (MachineId(2), 3)]
        );
    }

    #[test]
    fn more_players_than_archers() {
        // 8 archers, 12 players all wanting archer 6.
        let ms = vec![mach(0, 6, 6, None), mach(1, 6, 6, None)];
        let s = assign(&ms, TeamMode::Ffa, 8);
        let a: Vec<(u8, u8)> = s.iter().map(|p| (p.archer, p.dup)).collect();
        assert_eq!(
            a,
            vec![
                (6, 0),
                (7, 0),
                (0, 0),
                (1, 0),
                (2, 0),
                (3, 0),
                (4, 0),
                (5, 0),
                (6, 1),
                (6, 2),
                (6, 3),
                (6, 4)
            ]
        );
        // No archer rule: requests kept, duplicates numbered.
        let s = assign(&ms, TeamMode::Ffa, 0);
        assert!(s.iter().all(|p| p.archer == 6));
        assert_eq!(s[11].dup, 11);
    }

    #[test]
    fn teams_requests_and_balance() {
        let ms = vec![
            mach(0, 3, 1, Some(1)),
            mach(1, 3, 1, None),
            mach(2, 2, 5, Some(9)),
        ];
        let s = assign(&ms, TeamMode::Teams(2), 12);
        let t: Vec<u8> = s.iter().map(|p| p.team).collect();
        // three requested team 1, then balance: 0,0,0, then invalid request -> smallest
        assert_eq!(t, vec![1, 1, 1, 0, 0, 0, 0, 1]);
        assert_eq!(s[0].archer, 1);
        assert_eq!(s[1].archer, 2);
        assert_eq!(s[6].archer, 7, "5 and 6 were taken by machine 1");
    }

    #[test]
    fn names_and_skins() {
        assert_eq!(clean_name("  a\u{7}b  ", "x"), "ab");
        assert_eq!(clean_name("", "x"), "x");
        assert_eq!(clean_name("12345678901234567890", "x").len(), 16);
        let mut m = mach(0, 1, 0, None);
        m.slots[0].skin = 7;
        m.slots[0].name = String::new();
        let s = assign(&[m], TeamMode::Ffa, 4);
        assert_eq!(s[0].skin, 0, "invalid skin -> normal");
        assert_eq!(s[0].name, "m0 1");
    }
}
