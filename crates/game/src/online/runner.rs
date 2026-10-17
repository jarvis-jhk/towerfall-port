//! One online Versus match on this machine, without graphics or audio:
//! fulfils tfp-net's requests on [`NetWorld`], feeds the [`EventGate`],
//! and reports the end of the match once it is on the confirmed timeline.
//! Used by the online scene and by the headless test harness.

use std::collections::BTreeMap;

use anyhow::Result;
use tfp_net::{MatchStart, NetGame, NetSession, Request};
use tfp_sim::PlayerInput;

use super::adapter::{NetWorld, TowerData, new_world, quantise};
use super::gate::{EventGate, Released};

pub struct OnlineMatch {
    pub start: MatchStart,
    pub game: NetWorld,
    pub gate: EventGate,
    /// Sim frame of `game` (state after `frame` steps).
    pub frame: i32,
    /// State frames whose state is "match finished", with their checksum
    /// (re-simulation corrects them).
    finished: BTreeMap<i32, u128>,
    /// The confirmed end: (state frame, checksum), reported to tfp-net.
    pub done: Option<(i32, u128)>,
    /// Frames simulated for the first time / again after rollbacks.
    pub advanced: u64,
    pub resimulated: u64,
    /// Checksums of the states at multiples of [`CHECKPOINT`] frames
    /// (re-simulation overwrites; final once confirmed).
    pub checkpoints: BTreeMap<i32, u128>,
    /// Keep a copy of the world after every step until its frame is
    /// confirmed ([`OnlineMatch::drain_confirmed`]), for the presentation.
    pub keep_snapshots: bool,
    /// Step frame -> world after that step (re-simulation overwrites).
    snapshots: BTreeMap<i32, tfp_sim::World>,
    /// Newest confirmed frame after the last step.
    pub confirmed: i32,
    /// Tests: a quest also counts as over once this wave has started (or
    /// the quest ended), so runs can stop after N waves.
    pub until_wave: Option<i32>,
    /// Tests: a Dark World run counts as over once this level (0-based
    /// round index) has been reached (or the run ended).
    pub until_level: Option<usize>,
    /// Tests: the state of this frame counts as the end if the match has
    /// not ended before (the same frame on every machine: deterministic).
    pub stop_at: Option<i32>,
}

/// Checkpoint interval (5 s).
pub const CHECKPOINT: i32 = 300;

impl OnlineMatch {
    pub fn new(start: MatchStart, tower: &TowerData) -> Result<OnlineMatch> {
        let game = new_world(&start, tower)?;
        Ok(OnlineMatch::from_world(start, game))
    }

    /// A Quest / Trials run.
    pub fn new_coop(start: MatchStart, data: &super::coop::CoopData) -> Result<OnlineMatch> {
        let os = super::adapter::OnlineSettings::decode(&start.settings.game)
            .ok_or_else(|| anyhow::anyhow!("bad match settings from the host"))?;
        let game = super::coop::new_coop_world(&start, &os, data)?;
        Ok(OnlineMatch::from_world(start, game))
    }

    /// Wrap an already built world.
    pub fn from_world(start: MatchStart, game: NetWorld) -> OnlineMatch {
        OnlineMatch {
            start,
            game,
            gate: EventGate::new(),
            frame: 0,
            finished: BTreeMap::new(),
            done: None,
            advanced: 0,
            resimulated: 0,
            checkpoints: BTreeMap::new(),
            keep_snapshots: false,
            snapshots: BTreeMap::new(),
            confirmed: -1,
            until_wave: None,
            until_level: None,
            stop_at: None,
        }
    }

    /// The end was the frame cap, not the match.
    pub fn capped(&self) -> bool {
        self.done.is_some_and(|(f, _)| Some(f) == self.stop_at) && !self.game.finished()
    }

    fn is_over(&self) -> bool {
        if self.game.finished() || self.stop_at.is_some_and(|s| self.frame >= s) {
            return true;
        }
        if let (Some(n), Some(d)) = (self.until_level, &self.game.world.dark_world)
            && (d.round_index >= n || d.phase == tfp_sim::darkworld::DwPhase::Complete)
        {
            return true;
        }
        match (self.until_wave, &self.game.world.quest) {
            (Some(n), Some(q)) => {
                // Gauntlet towers (no waves; `gauntlet` = enemies left):
                // the run counts as over once the gauntlet is cleared.
                let progressed = if q.total_waves == 0 {
                    q.gauntlet == Some(0)
                } else {
                    q.current_wave >= n
                };
                progressed
                    || matches!(
                        q.phase,
                        tfp_sim::quest::QuestPhase::GameOver | tfp_sim::quest::QuestPhase::Complete
                    )
            }
            _ => false,
        }
    }

    /// Wrap an already built world (tests, synthetic content).
    #[cfg(test)]
    pub fn with_world(start: MatchStart, game: NetWorld) -> OnlineMatch {
        OnlineMatch::from_world(start, game)
    }

    /// One 60 Hz step: local inputs in, requests carried out, events gated.
    pub fn step(
        &mut self,
        net: &mut NetSession<NetWorld>,
        local: &[(usize, PlayerInput)],
    ) -> Released {
        for &(slot, i) in local {
            let _ = net.add_local_input(slot, quantise(i));
        }
        // Spectators only ever get final frames (and their session may end
        // inside this very advance, once the last frames are handed out).
        let spectating = net.is_spectator();
        let reqs = net.advance();
        self.apply(reqs);
        // Not `confirmed_frame`: inputs received while stalled at the
        // prediction limit are confirmed but not yet simulated.
        let confirmed = if spectating {
            self.frame - 1
        } else {
            net.settled_frame()
        };
        self.confirmed = confirmed;
        if self.done.is_none()
            && let Some((&f, &c)) = self.finished.iter().next()
            && f - 1 <= confirmed
        {
            // State `f` is the first finished one and its inputs are final:
            // every machine reports the same frame and checksum.
            self.done = Some((f, c));
            let _ = net.finish_match(f, c);
        }
        self.gate.end_batch(confirmed)
    }

    fn apply(&mut self, reqs: Vec<Request<NetWorld>>) {
        for r in reqs {
            match r {
                Request::Save { cell, frame } => {
                    cell.save(frame, Some(self.game.clone()), Some(self.game.checksum()));
                }
                Request::Load { cell, frame } => {
                    self.game = cell.load().expect("tfp-net hands out saved cells");
                    self.frame = frame;
                    self.gate.on_load(frame);
                    self.finished.retain(|f, _| *f <= frame);
                    self.snapshots.retain(|f, _| *f < frame);
                }
                Request::Advance {
                    frame,
                    inputs,
                    resim,
                } => {
                    self.game.advance(&inputs);
                    self.frame = frame + 1;
                    if resim {
                        self.resimulated += 1;
                    } else {
                        self.advanced += 1;
                    }
                    self.gate.on_advance(frame, self.game.world.events());
                    if self.keep_snapshots {
                        self.snapshots.insert(frame, self.game.world.clone());
                    }
                    if (frame + 1) % CHECKPOINT == 0 {
                        self.checkpoints.insert(frame + 1, self.game.checksum());
                    }
                    if self.is_over() {
                        self.finished
                            .entry(frame + 1)
                            .or_insert_with(|| self.game.checksum());
                    }
                }
            }
        }
    }

    /// Worlds after the steps that are confirmed now, in frame order (each
    /// handed out once): the presentation's rollback-free timeline.
    pub fn drain_confirmed(&mut self) -> Vec<tfp_sim::World> {
        self.drain_confirmed_frames()
            .into_iter()
            .map(|(_, w)| w)
            .collect()
    }

    /// [`drain_confirmed`](Self::drain_confirmed) with each world's step
    /// frame (the world is the state after that step).
    pub fn drain_confirmed_frames(&mut self) -> Vec<(i32, tfp_sim::World)> {
        let keys: Vec<i32> = self
            .snapshots
            .range(..=self.confirmed)
            .map(|(f, _)| *f)
            .collect();
        keys.into_iter()
            .filter_map(|f| self.snapshots.remove(&f).map(|w| (f, w)))
            .collect()
    }

    /// Versus summary for logs: round, scores of the joined slots, winner.
    pub fn summary(&self) -> String {
        match &self.game.world.versus {
            Some(m) => {
                let scores: Vec<String> = (0..self.start.players.len())
                    .map(|p| m.scores[m.settings.score_index(p).min(11)].to_string())
                    .collect();
                format!(
                    "round {} phase {:?} scores [{}] winner {}",
                    m.round_index,
                    m.phase,
                    scores.join(","),
                    m.winner
                )
            }
            None => super::coop::coop_summary(&self.game.world, self.start.players.len()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::adapter::tests::net_world;
    use super::*;
    use tfp_net::toy::Bot;
    use tfp_net::{LinkSim, MemHub, NetConfig, NetEvent, SlotRequest};

    /// Three machines (2 + 1 + 1 slots) and a spectator over an in-memory
    /// room with latency and loss, the real sim on a synthetic arena: the
    /// checksums of the confirmed state at frame 600 agree (compared by the
    /// host through `finish_match`; the spectator's equals them too), no
    /// desync, rollbacks happened, and confirmed events are only released
    /// for confirmed frames.
    #[test]
    fn three_machines_play_the_sim() {
        let hub = MemHub::new();
        const N: usize = 4;
        let mut nets: Vec<NetSession<NetWorld>> = (0..N)
            .map(|i| {
                let mut cfg = NetConfig::new("test");
                cfg.link_sim = LinkSim {
                    delay_ms: 30,
                    jitter_ms: 10,
                    loss: 0.03,
                };
                let mut n = NetSession::memory(hub.join(&format!("m{i}")), cfg);
                if i == N - 1 {
                    n.set_spectate(true);
                } else {
                    n.set_local_slots(vec![SlotRequest::default(); 1 + (i == 0) as usize]);
                }
                n
            })
            .collect();
        let mut matches: Vec<Option<OnlineMatch>> = (0..N).map(|_| None).collect();
        let mut bots: Vec<Bot> = (0..4).map(|i| Bot::new(i + 3)).collect();
        let mut ended: Vec<Option<bool>> = vec![None; N];
        // Machine 1's presentation timeline: confirmed worlds in order.
        let mut drained_last: i64 = -1;
        let mut drained_target: Option<u128> = None;
        let t0 = std::time::Instant::now();
        let mut started = false;
        let mut next = std::time::Instant::now();
        let target = 2 * CHECKPOINT;
        while ended.iter().any(|e| e.is_none()) {
            assert!(t0.elapsed().as_secs() < 60, "timeout");
            for (i, n) in nets.iter_mut().enumerate() {
                n.poll();
                while let Some(e) = n.next_event() {
                    match e {
                        NetEvent::MatchStarted(ms) => {
                            let game = net_world(ms.players.len(), ms.seed as i32);
                            let mut m = OnlineMatch::with_world(ms, game);
                            m.keep_snapshots = i == 1;
                            matches[i] = Some(m);
                        }
                        NetEvent::MatchEnded { agreed, .. } => ended[i] = Some(agreed),
                        NetEvent::Desync { .. } => panic!("desync"),
                        _ => {}
                    }
                }
            }
            if !started
                && nets[0].lobby().machines.len() == N
                && nets.iter().all(|n| n.me().is_some())
            {
                nets[0].start_match(Some(5)).unwrap();
                started = true;
            }
            if std::time::Instant::now() >= next {
                next += std::time::Duration::from_micros(16_667);
                for i in 0..N {
                    let Some(m) = matches[i].as_mut() else {
                        continue;
                    };
                    if ended[i].is_some() {
                        continue;
                    }
                    let local: Vec<(usize, PlayerInput)> = nets[i]
                        .local_slots()
                        .to_vec()
                        .into_iter()
                        .map(|s| {
                            let b = bots[s].input();
                            (
                                s,
                                PlayerInput::dpad(b.aim_x.signum(), b.aim_y.signum(), b.buttons),
                            )
                        })
                        .collect();
                    let r = m.step(&mut nets[i], &local);
                    for (f, w) in m.drain_confirmed_frames() {
                        assert!(f as i64 > drained_last, "in order, once");
                        assert!(f <= m.confirmed, "only confirmed frames");
                        drained_last = f as i64;
                        // The world after step f is the state of frame f + 1.
                        if f + 1 == target {
                            drained_target = Some(w.checksum128());
                        }
                    }
                    for (f, _) in r.confirmed {
                        // Against the frame the gate was given (a
                        // spectator's session can end inside this step).
                        assert!(f <= m.confirmed);
                    }
                    if nets[i].settled_frame() >= target
                        && let Some(&c) = m.checkpoints.get(&target)
                    {
                        let _ = nets[i].finish_match(target, c);
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            ended.iter().all(|e| *e == Some(true)),
            "checksums agree: {ended:?}"
        );
        let sum = |i: usize| {
            matches[i]
                .as_ref()
                .unwrap()
                .checkpoints
                .get(&target)
                .copied()
        };
        assert!(sum(0).is_some());
        assert_eq!(sum(N - 1), sum(0), "the spectator simulated the same match");
        assert_eq!(
            drained_target,
            sum(0),
            "confirmed snapshots are the final timeline"
        );
        let rollbacks: u64 = nets
            .iter()
            .filter_map(|n| n.last_match_stats())
            .map(|s| s.rollbacks)
            .sum();
        assert!(rollbacks > 0, "latency causes rollbacks");
    }
}
