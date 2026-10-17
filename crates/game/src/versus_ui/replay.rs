//! Instant replay (versus.md 10.2, our state-ring design): the recorder
//! keeps the last 210 world states (one per ~1/60 s of *game* time) with
//! the sound events of each; the viewer rewinds and replays them by
//! rendering those states. Local presentation only.

use std::collections::VecDeque;

use tfp_sim::{Event, World};

pub const CAPACITY: usize = 210;
/// A frame is stored once this much game time accumulated.
const MIN_FRAME: f32 = 0.015_666_667;

pub struct Frame {
    pub world: World,
    /// Game seconds this frame represents.
    pub time: f32,
    /// Sound events since the previous stored frame.
    pub sounds: Vec<Event>,
    /// The presentation at that moment (particles, explosions, skulls,
    /// lights, shake): the replay shows them as they were.
    pub fx: Option<Box<super::FxSnap>>,
}

#[derive(Default)]
pub struct Recorder {
    pub frames: VecDeque<Frame>,
    acc: f32,
    pending: Vec<Event>,
}

impl Recorder {
    pub fn clear(&mut self) {
        self.frames.clear();
        self.acc = 0.0;
        self.pending.clear();
    }

    /// After every unfrozen tick.
    pub fn record(&mut self, w: &World, time_mult: f32, fx: impl FnOnce() -> super::FxSnap) {
        self.acc += time_mult / 60.0;
        self.pending.extend(
            w.events()
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        Event::Sound { .. }
                            | Event::SoundIndexed { .. }
                            | Event::CharSound { .. }
                            | Event::Env(tfp_sim::entities::EnvEvent::Sound { .. })
                    )
                })
                .copied(),
        );
        if self.acc < MIN_FRAME && !self.frames.is_empty() {
            return;
        }
        if self.frames.len() == CAPACITY {
            self.frames.pop_front();
        }
        self.frames.push_back(Frame {
            world: w.clone(),
            time: self.acc,
            sounds: std::mem::take(&mut self.pending),
            fx: Some(Box::new(fx())),
        });
        self.acc = 0.0;
    }

    /// Hands the frames to a viewer (the recorder keeps them for REPLAY).
    pub fn viewer(&self) -> Option<Viewer> {
        (self.frames.len() > 1).then(|| Viewer::new(self.frames.len()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    /// The intro: icon up, distortion settles.
    Start(u32),
    Rewind,
    /// At frame 0: play icon, 5 ticks.
    Pause(u32),
    Forward,
    Done,
}

/// Playback state over a recorder's frames.
pub struct Viewer {
    pub f: usize,
    last: usize,
    acc: f32,
    pub rate: f32,
    step: Step,
    pub t: u32,
    /// Distortion (screen wave) amplitude, counter and speed.
    pub amp: f32,
    pub counter: f32,
    speed: f32,
    pub line_alpha: f32,
    pub icon_play: bool,
    pub icon_visible: bool,
}

impl Viewer {
    fn new(n: usize) -> Self {
        Self {
            f: n - 1,
            last: n - 1,
            acc: 0.0,
            rate: 0.0,
            step: Step::Start(0),
            t: 0,
            amp: 0.0,
            counter: 0.0,
            speed: 0.0,
            line_alpha: 0.0,
            icon_play: false,
            icon_visible: true,
        }
    }

    pub fn done(&self) -> bool {
        self.step == Step::Done
    }

    pub fn skip(&mut self) {
        self.f = self.last;
        self.step = Step::Done;
    }

    /// One tick. Returns the frames entered moving forward (their sounds
    /// are replayed) and whether the "play" cue starts now.
    pub fn update(&mut self, frames: &VecDeque<Frame>, slow: bool) -> (Vec<usize>, bool) {
        self.t += 1;
        let mut entered = Vec::new();
        let mut play_cue = false;
        match self.step {
            Step::Start(n) => {
                self.amp -= self.amp / 20.0;
                if n + 1 >= 20 {
                    self.amp = 0.0;
                    self.step = Step::Rewind;
                } else {
                    self.step = Step::Start(n + 1);
                }
            }
            Step::Rewind => {
                if self.f >= 60 {
                    self.rate = (self.rate - 0.04).max(-8.0);
                    let k = self.rate / -8.0;
                    self.amp = 3.0 / 64.0 * k;
                    self.speed = -2.5 * k;
                    self.line_alpha = (self.line_alpha + 0.05).min(1.0);
                } else {
                    self.rate = (self.rate + 0.05).min(-0.5);
                    self.amp = (self.amp - 0.1 * 3.0 / 64.0).max(0.0);
                    self.line_alpha = (self.line_alpha - 0.1).max(0.0);
                }
                if self.f == 0 {
                    self.icon_play = true;
                    play_cue = true;
                    self.step = Step::Pause(0);
                }
            }
            Step::Pause(n) => {
                if n + 1 >= 5 {
                    self.rate = 0.0;
                    self.acc = 0.0;
                    self.step = Step::Forward;
                } else {
                    self.step = Step::Pause(n + 1);
                }
            }
            Step::Forward => {
                self.rate = (self.rate + 0.02).min(1.0);
                if self.rate >= 1.0 {
                    self.icon_visible = false;
                }
                self.amp = (self.amp - 0.000_625).max(0.0);
                self.line_alpha = 0.0;
                if self.f >= self.last {
                    self.step = Step::Done;
                }
            }
            Step::Done => {}
        }
        self.counter += self.speed;
        // Move the frame marker by the accumulated game time.
        let rate = if slow && self.rate > 0.0 {
            self.rate * 0.5
        } else {
            self.rate
        };
        self.acc += rate / 60.0;
        if rate > 0.0 {
            while self.f < self.last && self.acc >= frames[self.f + 1].time {
                self.acc -= frames[self.f + 1].time;
                self.f += 1;
                entered.push(self.f);
            }
        } else if rate < 0.0 {
            while self.f > 0 && self.acc <= -frames[self.f].time {
                self.acc += frames[self.f].time;
                self.f -= 1;
            }
        }
        (entered, play_cue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tfp_sim::{Content, MatchSetup};

    #[test]
    fn records_and_plays_through() {
        let c = Content::from_ascii(&["##########", "#........#", "##########"]);
        let mut w = World::new(&c, &MatchSetup::default());
        let mut r = Recorder::default();
        for _ in 0..300 {
            w.step(&c, &[]);
            r.record(&w, 1.0, super::super::FxSnap::empty);
        }
        assert_eq!(r.frames.len(), CAPACITY);
        // Slow motion: every other tick.
        let mut r2 = Recorder::default();
        for _ in 0..100 {
            r2.record(&w, 0.5, super::super::FxSnap::empty);
        }
        assert_eq!(r2.frames.len(), 50);
        let mut v = r.viewer().unwrap();
        let mut ticks = 0;
        let mut reached_zero = false;
        while !v.done() && ticks < 5000 {
            v.update(&r.frames, false);
            reached_zero |= v.f == 0;
            ticks += 1;
        }
        assert!(v.done() && reached_zero && v.f == CAPACITY - 1, "{ticks}");
        // ~1.3 s rewind + ~4 s forward.
        assert!((200..600).contains(&ticks), "{ticks}");
        // SLOW held: the forward pass takes about twice as long; rewinding
        // is unaffected.
        let mut v = r.viewer().unwrap();
        let mut slow_ticks = 0;
        while !v.done() && slow_ticks < 5000 {
            v.update(&r.frames, true);
            slow_ticks += 1;
        }
        let forward = ticks - 100;
        assert!(slow_ticks > ticks + forward / 2, "{slow_ticks} vs {ticks}");
    }
}
