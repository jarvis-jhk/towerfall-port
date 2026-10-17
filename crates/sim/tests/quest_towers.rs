//! Every Quest tower of the install, Normal and Hardcore, with two shared
//! bots (`tfp_sim::bot::Bot`): the run must make progress within the tick
//! budget: wave 1 on wave towers, the first gauntlet kills on the two
//! gauntlet towers (which have no waves; the HUD shows the remaining
//! count). This catches script loading, spawner names and start
//! conditions. Bot skill is not under test: enemies the bots cannot reach
//! (an idle crow in a floor gap, a mole far away) are cleared after 30 s
//! without progress. Skipped (with a note) without an install.

use tfp_assets::install::Installs;
use tfp_sim::PlayerInput;
use tfp_sim::quest::{QuestPhase, QuestSettings};

/// Ticks a tower gets to reach its first progress.
const BUDGET: usize = 60 * 60 * 4;
/// Ticks without progress before stragglers are cleared.
const STRAGGLERS: usize = 60 * 30;

#[test]
fn every_quest_tower_reaches_wave_one() {
    let Some(inst) = Installs::discover().steam.clone() else {
        eprintln!("no Steam install: skipped");
        return;
    };
    let c = tfp_sim::loader::quest_content(&inst).expect("quest content");
    assert!(c.quest_scripts.len() >= 12, "quest towers loaded");
    let mut report = vec![];
    for t in 0..c.quest_scripts.len() {
        for hardcore in [false, true] {
            let mut s = QuestSettings::new(t, t, 2);
            s.tower_id = t as u8;
            s.hardcore = hardcore;
            s.seed = 3;
            let mut w = tfp_sim::quest::new_quest(&c, s);
            let mut bots: Vec<tfp_sim::bot::Bot> = (0..2).map(tfp_sim::bot::Bot::new).collect();
            let start = w.quest.as_ref().unwrap().gauntlet;
            let progress = |w: &tfp_sim::World| {
                let q = w.quest.as_ref().unwrap();
                match start {
                    Some(g) => q.gauntlet.is_some_and(|r| r < g),
                    None => q.current_wave >= 1,
                }
            };
            let mut at = None;
            for f in 0..BUDGET {
                if f % STRAGGLERS == STRAGGLERS - 1 {
                    let none = w.entities().next().unwrap();
                    tfp_sim::enemies::kill_all(&mut w, &c, none);
                }
                let i: Vec<PlayerInput> = (0..2).map(|k| bots[k].input(&w, &c, k as u8)).collect();
                w.step(&c, &i);
                assert_ne!(
                    w.quest.as_ref().unwrap().phase,
                    QuestPhase::Complete,
                    "tower {t}: complete before wave 1?"
                );
                if progress(&w) {
                    at = Some(f);
                    break;
                }
            }
            report.push(format!("{t}{}: {at:?}", if hardcore { "H" } else { "N" }));
            assert!(
                at.is_some(),
                "tower {t} ({}) hardcore {hardcore}: no progress in {BUDGET} ticks (wave {}, phase {:?})",
                c.quest_scripts[t].name,
                w.quest.as_ref().unwrap().current_wave,
                w.quest.as_ref().unwrap().phase
            );
        }
    }
    eprintln!("first progress (ticks): {}", report.join(", "));
}
