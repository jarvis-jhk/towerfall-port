//! Sim events under rollback: what the presentation may play.
//!
//! The sim emits events every step ([`tfp_sim::World::events`]). Online,
//! frames are simulated more than once: predicted, then (after a rollback)
//! again with the real inputs. [`EventGate`] turns that stream into what the
//! player should hear and see:
//!
//! * **Immediate** events (sounds, archer voices, particles, rumble, screen
//!   shake) play when *first seen*: each `(frame, identity)` plays once, so
//!   a re-simulation that produces the same event stays silent, and one that
//!   produces a different event (the prediction was wrong) plays it. The
//!   identity leaves out positions, which differ slightly between a
//!   predicted and a corrected frame.
//! * **Confirmed** events (match flow: round start/end, points, crowns,
//!   final kill, match end; deaths for the kill feed) are only released
//!   once their frame is confirmed, from the final timeline: no "POINT!"
//!   for a kill that a rollback undoes.
//! * **Looped** sounds (wall slide, ...) follow a per-frame log of on/off
//!   transitions. A rollback drops the log of the re-simulated frames; after
//!   each batch the wanted set of loops is folded from the log and compared
//!   with what is playing: loops started in rolled-back frames and not
//!   started again are cancelled, loops stopped in rolled-back frames and
//!   not stopped again resume.

use std::collections::{BTreeMap, BTreeSet};

use tfp_sim::Event;

/// A looped sound: (player, name).
pub type LoopKey = (u8, &'static str);

/// What to present after one batch of requests.
#[derive(Debug, Default)]
pub struct Released {
    /// First-seen immediate events: `(frame, event)`.
    pub immediate: Vec<(i32, Event)>,
    /// Confirmed match-flow events and deaths, in frame order.
    pub confirmed: Vec<(i32, Event)>,
    /// Loops to start (with their latest x) / stop.
    pub loop_start: Vec<(LoopKey, f32)>,
    pub loop_stop: Vec<LoopKey>,
}

#[derive(Default)]
pub struct EventGate {
    /// Immediate events already played: (frame, identity).
    seen: BTreeSet<(i32, u64)>,
    /// Confirmed-class events per frame (latest simulation wins).
    pending: BTreeMap<i32, Vec<Event>>,
    /// Loop transitions per frame (latest simulation wins).
    loop_log: BTreeMap<i32, Vec<(LoopKey, bool, f32)>>,
    /// Loops on as of the frames folded out of the log.
    loop_base: BTreeMap<LoopKey, f32>,
    /// Loops currently audible.
    playing: BTreeMap<LoopKey, f32>,
    /// Newest frame released as confirmed.
    released_to: i32,
    out: Released,
}

fn confirmed_class(e: &Event) -> bool {
    matches!(e, Event::Versus(_) | Event::Death { .. })
}

/// Identity of an immediate event without positions/directions.
fn identity(e: &Event, nth: u32) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut eat = |s: &[u8]| {
        for b in s {
            h = (h ^ *b as u64).wrapping_mul(0x100000001b3);
        }
    };
    match e {
        Event::Sound { name, .. } => {
            eat(b"s");
            eat(name.as_bytes());
        }
        Event::CharSound { sound, player, .. } => {
            eat(b"c");
            eat(format!("{sound:?}").as_bytes());
            eat(&[*player]);
        }
        Event::Particles { kind, player, .. } => {
            eat(b"p");
            eat(format!("{kind:?}").as_bytes());
            eat(&[*player]);
        }
        Event::Rumble { player, .. } => {
            eat(b"r");
            eat(&[*player]);
        }
        Event::ScreenShake { frames } => {
            eat(b"k");
            eat(&frames.to_le_bytes());
        }
        other => eat(format!("{other:?}").as_bytes()),
    }
    eat(&nth.to_le_bytes());
    h
}

impl EventGate {
    pub fn new() -> Self {
        EventGate {
            released_to: -1,
            ..Default::default()
        }
    }

    /// The state was restored to `frame`: frames from `frame` on will be
    /// simulated again.
    pub fn on_load(&mut self, frame: i32) {
        self.pending.retain(|f, _| *f < frame);
        self.loop_log.retain(|f, _| *f < frame);
    }

    /// The step from `frame` to `frame + 1` produced `events`.
    pub fn on_advance(&mut self, frame: i32, events: &[Event]) {
        let mut nth: BTreeMap<u64, u32> = BTreeMap::new();
        let mut confirmed = Vec::new();
        let mut loops = Vec::new();
        for e in events {
            match *e {
                Event::Loop {
                    name,
                    player,
                    on,
                    x,
                } => loops.push(((player, name), on, x)),
                ref e if confirmed_class(e) => confirmed.push(*e),
                ref e => {
                    let base = identity(e, 0);
                    let n = nth.entry(base).or_insert(0);
                    let id = identity(e, *n);
                    *n += 1;
                    if self.seen.insert((frame, id)) {
                        self.out.immediate.push((frame, *e));
                    }
                }
            }
        }
        if frame > self.released_to {
            self.pending.insert(frame, confirmed);
        }
        self.loop_log.insert(frame, loops);
    }

    /// After a batch: release confirmed events up to `confirmed` and
    /// reconcile loops. `confirmed` = newest frame whose inputs are final.
    pub fn end_batch(&mut self, confirmed: i32) -> Released {
        // Confirmed events, in frame order.
        let keys: Vec<i32> = self.pending.range(..=confirmed).map(|(f, _)| *f).collect();
        for f in keys {
            if let Some(v) = self.pending.remove(&f) {
                self.out.confirmed.extend(v.into_iter().map(|e| (f, e)));
            }
            self.released_to = self.released_to.max(f);
        }
        // Loops: wanted = base + log in frame order.
        let mut wanted = self.loop_base.clone();
        for v in self.loop_log.values() {
            for &(k, on, x) in v {
                if on {
                    wanted.insert(k, x);
                } else {
                    wanted.remove(&k);
                }
            }
        }
        for (k, x) in &wanted {
            if !self.playing.contains_key(k) {
                self.out.loop_start.push((*k, *x));
            }
        }
        for k in self.playing.keys() {
            if !wanted.contains_key(k) {
                self.out.loop_stop.push(*k);
            }
        }
        self.playing = wanted;
        // Fold confirmed frames of the log into the base; forget old ids.
        let old: Vec<i32> = self.loop_log.range(..=confirmed).map(|(f, _)| *f).collect();
        for f in old {
            for (k, on, x) in self.loop_log.remove(&f).unwrap_or_default() {
                if on {
                    self.loop_base.insert(k, x);
                } else {
                    self.loop_base.remove(&k);
                }
            }
        }
        self.seen.retain(|(f, _)| *f > confirmed);
        std::mem::take(&mut self.out)
    }

    /// Loops currently audible.
    #[cfg(test)]
    pub fn playing(&self) -> impl Iterator<Item = &LoopKey> {
        self.playing.keys()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tfp_sim::events::CharSound;
    use tfp_sim::versus::VersusEvent;

    fn snd(name: &'static str, x: f32) -> Event {
        Event::Sound { name, x }
    }
    fn lp(on: bool) -> Event {
        Event::Loop {
            name: "WALL_SLIDE",
            player: 1,
            on,
            x: 3.0,
        }
    }

    #[test]
    fn first_seen_once_across_resimulation() {
        let mut g = EventGate::new();
        g.on_advance(10, &[snd("JUMP", 1.0), snd("JUMP", 2.0)]);
        let r = g.end_batch(5);
        assert_eq!(r.immediate.len(), 2, "two jumps in one frame both play");
        // Rollback: frame 10 again, same sounds at slightly other positions.
        g.on_load(9);
        g.on_advance(9, &[]);
        g.on_advance(10, &[snd("JUMP", 1.5), snd("JUMP", 2.5)]);
        assert!(g.end_batch(9).immediate.is_empty(), "already heard");
        // A different event in a re-simulated frame plays.
        g.on_load(10);
        g.on_advance(
            10,
            &[Event::CharSound {
                sound: CharSound::Die,
                player: 0,
                archer: 0,
                x: 0.0,
            }],
        );
        assert_eq!(g.end_batch(9).immediate.len(), 1);
    }

    #[test]
    fn match_flow_only_when_confirmed() {
        let mut g = EventGate::new();
        let point = Event::Versus(VersusEvent::PointGained { index: 2 });
        g.on_advance(20, &[point]);
        assert!(
            g.end_batch(19).confirmed.is_empty(),
            "frame 20 still predicted"
        );
        // The prediction was wrong: the re-simulated frame 20 has no point.
        g.on_load(20);
        g.on_advance(20, &[]);
        g.on_advance(21, &[Event::Versus(VersusEvent::RoundStart)]);
        let r = g.end_batch(21);
        assert_eq!(r.confirmed.len(), 1);
        assert_eq!(r.confirmed[0], (21, Event::Versus(VersusEvent::RoundStart)));
        // Released frames are never released again.
        g.on_advance(21, &[Event::Versus(VersusEvent::RoundStart)]);
        assert!(g.end_batch(30).confirmed.is_empty());
    }

    #[test]
    fn loops_cancelled_and_resumed_on_rollback() {
        let mut g = EventGate::new();
        g.on_advance(5, &[lp(true)]);
        let r = g.end_batch(0);
        assert_eq!(r.loop_start, vec![((1, "WALL_SLIDE"), 3.0)]);
        // Rollback to 5: the re-simulation does not start the loop.
        g.on_load(5);
        g.on_advance(5, &[]);
        let r = g.end_batch(1);
        assert_eq!(r.loop_stop, vec![(1, "WALL_SLIDE")], "cancelled");
        // Started again (confirmed), then stopped in a predicted frame ...
        g.on_advance(6, &[lp(true)]);
        assert_eq!(g.end_batch(6).loop_start.len(), 1);
        g.on_advance(8, &[lp(false)]);
        assert_eq!(g.end_batch(6).loop_stop.len(), 1);
        // ... which a rollback undoes: the loop resumes.
        g.on_load(8);
        g.on_advance(8, &[]);
        let r = g.end_batch(8);
        assert_eq!(r.loop_start.len(), 1, "resumed");
        assert!(g.playing().next().is_some());
        // A rollback that repeats the same stop changes nothing.
        g.on_advance(9, &[lp(false)]);
        g.end_batch(8);
        g.on_load(9);
        g.on_advance(9, &[lp(false)]);
        let r = g.end_batch(9);
        assert!(r.loop_start.is_empty() && r.loop_stop.is_empty());
    }
}
