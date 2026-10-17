//! Monocle's timing components as plain `Copy` data (engine.md §4).
//!
//! Callbacks are not stored: each component's `update` reports what fired
//! and the owning entity runs its own reaction in the same place the
//! original's delegate would have run. All of them count in f32 by
//! `TimeMult`, so slow motion behaves like the original.

use crate::checksum::{Hasher, StableHash};
use crate::impl_hash;
use crate::math::{PI, cos, pow, sin};

// ------------------------------------------------------------------ Counter

/// `Counter` helpers on a plain `f32` (the original's `Counter` is a float
/// wrapper; owners update it by hand, often conditionally).
pub trait Counter {
    /// `Update()`: `value -= TimeMult`.
    fn tick(&mut self, tm: f32);
    /// Boolean view: `value > 0`.
    fn on(&self) -> bool;
    /// `SetMax(n)`.
    fn set_max(&mut self, n: f32);
}

impl Counter for f32 {
    #[inline]
    fn tick(&mut self, tm: f32) {
        *self -= tm;
    }
    #[inline]
    fn on(&self) -> bool {
        *self > 0.0
    }
    #[inline]
    fn set_max(&mut self, n: f32) {
        *self = crate::math::max(n, *self);
    }
}

/// `Counter.JustPassed(n)`.
pub fn counter_just_passed(value: f32, n: f32, last_time_mult: f32) -> bool {
    value <= n && value + last_time_mult > n
}

// -------------------------------------------------------------------- Alarm

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlarmMode {
    Persist,
    Oneshot,
    Looping,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Alarm {
    pub duration: i32,
    pub frames_left: f32,
    pub mode: AlarmMode,
    pub active: bool,
    /// Oneshot alarms remove themselves after firing.
    pub removed: bool,
}

impl Alarm {
    pub fn new(duration: i32, mode: AlarmMode) -> Alarm {
        Alarm {
            duration,
            frames_left: 0.0,
            mode,
            active: false,
            removed: false,
        }
    }
    pub fn start(&mut self) {
        self.active = true;
        self.frames_left = self.duration as f32;
    }
    pub fn start_with(&mut self, duration: i32) {
        self.duration = duration;
        self.start();
    }
    pub fn stop(&mut self) {
        self.active = false;
    }
    /// Decrement; returns true when the alarm fires. The owner then runs its
    /// callback and calls [`Alarm::after_fire`].
    pub fn update(&mut self, tm: f32) -> bool {
        if !self.active || self.removed {
            return false;
        }
        self.frames_left -= tm;
        if self.frames_left <= 0.0 {
            self.frames_left = 0.0;
            return true;
        }
        false
    }
    pub fn after_fire(&mut self) {
        match self.mode {
            AlarmMode::Looping => self.start(),
            AlarmMode::Oneshot => {
                self.active = false;
                self.removed = true;
            }
            AlarmMode::Persist => {
                if self.frames_left <= 0.0 {
                    self.active = false;
                }
            }
        }
    }
}

// --------------------------------------------------------------------- Ease

/// The `Ease` functions (engine.md §4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ease {
    Linear,
    QuadIn,
    QuadOut,
    QuadInOut,
    CubeIn,
    CubeOut,
    CubeInOut,
    BackIn,
    BackOut,
    BackInOut,
    BigBackIn,
    BigBackOut,
    BigBackInOut,
    ExpoIn,
    ExpoOut,
    ExpoInOut,
    SineIn,
    SineOut,
    SineInOut,
    BounceIn,
    BounceOut,
    BounceInOut,
}

const B1: f32 = 0.36363637;
const B2: f32 = 0.72727275;
const B3: f32 = 0.54545456;
const B4: f32 = 0.90909094;
const B5: f32 = 0.8181818;
const B6: f32 = 21.0 / 22.0;
const B_LAST: f32 = 63.0 / 64.0;

fn bounce_out_raw(t: f32) -> f32 {
    if t < B1 {
        7.5625 * t * t
    } else if t < B2 {
        7.5625 * (t - B3) * (t - B3) + 0.75
    } else if t < B4 {
        7.5625 * (t - B5) * (t - B5) + 0.9375
    } else {
        7.5625 * (t - B6) * (t - B6) + B_LAST
    }
}

fn bounce_in_raw(t: f32) -> f32 {
    let t = 1.0 - t;
    if t < B1 {
        1.0 - 7.5625 * t * t
    } else if t < B2 {
        1.0 - (7.5625 * (t - B3) * (t - B3) + 0.75)
    } else if t < B4 {
        1.0 - (7.5625 * (t - B5) * (t - B5) + 0.9375)
    } else {
        1.0 - (7.5625 * (t - B6) * (t - B6) + B_LAST)
    }
}

impl Ease {
    fn base_in(self) -> Option<fn(f32) -> f32> {
        use Ease::*;
        Some(match self {
            QuadIn | QuadOut | QuadInOut => |t| t * t,
            CubeIn | CubeOut | CubeInOut => |t| t * t * t,
            BackIn | BackOut | BackInOut => |t| t * t * (2.70158 * t - 1.70158),
            BigBackIn | BigBackOut | BigBackInOut => |t| t * t * (4.0 * t - 3.0),
            ExpoIn | ExpoOut | ExpoInOut => |t| pow(2.0, 10.0 * (t - 1.0)),
            _ => return None,
        })
    }

    pub fn apply(self, t: f32) -> f32 {
        use Ease::*;
        match self {
            Linear => t,
            SineIn => 0.0 - cos(PI / 2.0 * t) + 1.0,
            SineOut => sin(PI / 2.0 * t),
            SineInOut => (0.0 - cos(PI * t)) / 2.0 + 0.5,
            BounceOut => bounce_out_raw(t),
            BounceIn => bounce_in_raw(t),
            BounceInOut => {
                if t < 0.5 {
                    // BounceIn branches on t' = 1 - 2t, each divided by 2.
                    bounce_in_raw(t * 2.0) / 2.0
                } else {
                    bounce_out_raw(t * 2.0 - 1.0) / 2.0 + 0.5
                }
            }
            QuadIn | CubeIn | BackIn | BigBackIn | ExpoIn => (self.base_in().unwrap())(t),
            QuadOut | CubeOut | BackOut | BigBackOut | ExpoOut => {
                1.0 - (self.base_in().unwrap())(1.0 - t)
            }
            QuadInOut | CubeInOut | BackInOut | BigBackInOut | ExpoInOut => {
                let f = self.base_in().unwrap();
                if t <= 0.5 {
                    f(t * 2.0) / 2.0
                } else {
                    (1.0 - f(1.0 - (t * 2.0 - 1.0))) / 2.0 + 0.5
                }
            }
        }
    }
}

/// `Ease.UpDown`.
pub fn ease_up_down(e: f32) -> f32 {
    if e <= 0.5 {
        2.0 * e
    } else {
        1.0 - (e - 0.5) * 2.0
    }
}

// -------------------------------------------------------------------- Tween

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TweenMode {
    Persist,
    Oneshot,
    Looping,
    YoyoOneshot,
    YoyoLooping,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tween {
    pub mode: TweenMode,
    pub duration: i32,
    pub frames_left: f32,
    pub percent: f32,
    pub eased: f32,
    pub reverse: bool,
    started_reversed: bool,
    pub easer: Option<Ease>,
    pub active: bool,
    pub removed: bool,
}

/// What one tween update did (the owner runs `OnUpdate` then `OnComplete`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TweenStep {
    pub updated: bool,
    pub completed: bool,
    /// `OnStart` fired again (looping / yoyo restart).
    pub restarted: bool,
}

impl Tween {
    pub fn new(mode: TweenMode, easer: Option<Ease>, duration: i32) -> Tween {
        Tween {
            mode,
            duration: duration.max(1),
            frames_left: 0.0,
            percent: 0.0,
            eased: 0.0,
            reverse: false,
            started_reversed: false,
            easer,
            active: false,
            removed: false,
        }
    }
    pub fn start(&mut self, reverse: bool) {
        self.reverse = reverse;
        self.started_reversed = reverse;
        self.frames_left = self.duration as f32;
        let v = if reverse { 1.0 } else { 0.0 };
        self.percent = v;
        self.eased = v;
        self.active = true;
    }
    pub fn inverted(&self) -> f32 {
        1.0 - self.eased
    }
    pub fn update(&mut self, tm: f32) -> TweenStep {
        let mut out = TweenStep::default();
        if !self.active || self.removed {
            return out;
        }
        self.frames_left -= tm;
        let mut p = crate::math::max(0.0, self.frames_left) / self.duration as f32;
        if !self.reverse {
            p = 1.0 - p;
        }
        self.percent = p;
        self.eased = match self.easer {
            Some(e) => e.apply(p),
            None => p,
        };
        out.updated = true;
        if self.frames_left <= 0.0 {
            self.frames_left = 0.0;
            out.completed = true;
            match self.mode {
                TweenMode::Persist => self.active = false,
                TweenMode::Oneshot => {
                    self.active = false;
                    self.removed = true;
                }
                TweenMode::Looping => {
                    self.start(self.reverse);
                    out.restarted = true;
                }
                TweenMode::YoyoOneshot => {
                    if self.reverse == self.started_reversed {
                        let orig = self.started_reversed;
                        self.start(!self.reverse);
                        self.started_reversed = orig;
                        out.restarted = true;
                    } else {
                        self.active = false;
                        self.removed = true;
                    }
                }
                TweenMode::YoyoLooping => {
                    self.start(!self.reverse);
                    out.restarted = true;
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------- Scheduler

/// `Scheduler` with a fixed capacity; `A` is the owner's action enum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scheduler<A: Copy, const N: usize> {
    entries: [(A, f32, f32); N],
    len: u8,
}

impl<A: Copy + Default, const N: usize> Default for Scheduler<A, N> {
    fn default() -> Self {
        Scheduler {
            entries: [(A::default(), 0.0, 0.0); N],
            len: 0,
        }
    }
}

impl<A: Copy + Default + PartialEq, const N: usize> Scheduler<A, N> {
    /// `ScheduleAction(a, delay, looping)`. Silently drops the action when
    /// full (capacity is sized for every use in the game).
    pub fn schedule(&mut self, a: A, delay: f32, looping: bool) {
        if (self.len as usize) < N {
            self.entries[self.len as usize] = (a, delay, if looping { delay } else { 0.0 });
            self.len += 1;
        } else {
            debug_assert!(false, "scheduler full");
        }
    }

    /// `UnscheduleAction(a)`.
    pub fn unschedule(&mut self, a: A) {
        let mut i = 0;
        while i < self.len as usize {
            if self.entries[i].0 == a {
                self.remove_at(i);
            } else {
                i += 1;
            }
        }
    }

    fn remove_at(&mut self, i: usize) {
        let n = self.len as usize;
        self.entries.copy_within(i + 1..n, i);
        self.len -= 1;
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// One update; returns the actions to run, in order.
    pub fn update(&mut self, tm: f32) -> ([A; N], usize) {
        let mut fired = [A::default(); N];
        let mut nf = 0;
        let mut i = 0;
        while i < self.len as usize {
            let e = &mut self.entries[i];
            e.1 -= tm;
            if e.1 <= 0.0 {
                let a = e.0;
                if e.2 != 0.0 {
                    e.1 = e.2;
                    i += 1;
                } else {
                    self.remove_at(i);
                }
                fired[nf] = a;
                nf += 1;
            } else {
                i += 1;
            }
        }
        (fired, nf)
    }
}

impl<A: Copy + StableHash, const N: usize> StableHash for Scheduler<A, N> {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u8(self.len);
        for e in &self.entries[..self.len as usize] {
            e.0.stable_hash(h);
            h.f32(e.1);
            h.f32(e.2);
        }
    }
}

// ----------------------------------------------------------------- SineWave

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SineWave {
    pub rate: f32,
    pub counter: f32,
    pub value: f32,
    pub value_over_two: f32,
    pub two_value: f32,
    pub active: bool,
}

impl Default for SineWave {
    fn default() -> Self {
        SineWave {
            rate: PI / 64.0,
            counter: 0.0,
            value: 0.0,
            value_over_two: 0.0,
            two_value: 0.0,
            active: true,
        }
    }
}

impl SineWave {
    pub fn frames(n: f32) -> SineWave {
        SineWave {
            rate: PI * 2.0 / n,
            ..Default::default()
        }
    }
    pub fn update(&mut self, tm: f32) {
        if !self.active {
            return;
        }
        self.counter = (self.counter + self.rate * tm) % (PI * 8.0);
        self.recompute();
    }
    fn recompute(&mut self) {
        self.value = sin(self.counter);
        self.value_over_two = sin(self.counter / 2.0);
        self.two_value = sin(self.counter * 2.0);
    }
    pub fn start_up(&mut self) {
        self.counter = PI / 2.0;
        self.recompute();
    }
    pub fn start_down(&mut self) {
        self.counter = 4.712389;
        self.recompute();
    }
}

// ------------------------------------------------------------------ Wiggler

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wiggler {
    increment: f32,
    sine_add: f32,
    pub counter: f32,
    sine_counter: f32,
    pub value: f32,
    pub easer: Option<Ease>,
    pub active: bool,
}

impl Wiggler {
    pub fn new(duration: f32, frequency: f32, easer: Option<Ease>) -> Wiggler {
        Wiggler {
            increment: 1.0 / duration,
            sine_add: ((PI * 2.0) * frequency) / 60.0,
            counter: 0.0,
            sine_counter: 0.0,
            value: 0.0,
            easer,
            active: false,
        }
    }
    pub fn start(&mut self) {
        self.counter = 1.0;
        self.sine_counter = 0.0;
        self.value = 1.0;
        self.active = true;
    }
    pub fn update(&mut self, tm: f32) {
        if !self.active {
            return;
        }
        self.sine_counter += self.sine_add * tm;
        self.counter -= self.increment * tm;
        if self.counter <= 0.0 {
            self.counter = 0.0;
            self.active = false;
        }
        let k = match self.easer {
            Some(e) => e.apply(self.counter),
            None => self.counter,
        };
        self.value = cos(self.sine_counter) * k;
    }
    pub fn stop_and_clear(&mut self) {
        self.active = false;
        self.value = 0.0;
    }
}

// ------------------------------------------------------------- StateMachine

/// `StateMachine` bookkeeping. Owners call [`StateMachine::set`] and run
/// the leave/enter reactions it reports (enter/leave are not stored).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateMachine<S: Copy + Eq> {
    pub state: S,
    pub previous: S,
}

impl<S: Copy + Eq> StateMachine<S> {
    pub fn new(initial: S) -> Self {
        StateMachine {
            state: initial,
            previous: initial,
        }
    }
    /// Returns `Some((old, new))` when the state changed.
    pub fn set(&mut self, s: S) -> Option<(S, S)> {
        if s == self.state {
            return None;
        }
        let old = self.state;
        self.previous = old;
        self.state = s;
        Some((old, s))
    }
}

// ---------------------------------------------------------------- Coroutine

/// A coroutine as an explicit resumable state: the owner matches on `step`
/// whenever [`Coroutine::update`] says to resume, and ends each resume with
/// one of the `yield_*` calls or [`Coroutine::finish`]. Timing matches
/// engine.md §4.4: `yield return n` at tick F resumes at F + n + 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coroutine {
    pub step: u16,
    pub wait: f32,
    pub active: bool,
    pub finished: bool,
}

impl_hash!(Coroutine {
    step,
    wait,
    active,
    finished
});

impl Default for Coroutine {
    fn default() -> Self {
        Coroutine {
            step: 0,
            wait: 0.0,
            active: false,
            finished: true,
        }
    }
}

impl Coroutine {
    pub fn start(step: u16) -> Coroutine {
        Coroutine {
            step,
            wait: 0.0,
            active: true,
            finished: false,
        }
    }
    /// Returns the step to resume, if any.
    pub fn update(&mut self, tm: f32) -> Option<u16> {
        if !self.active {
            return None;
        }
        if self.wait > 0.0 {
            self.wait -= tm;
            return None;
        }
        Some(self.step)
    }
    /// `yield return n` (n frames), then continue at `next`.
    pub fn yield_frames(&mut self, n: f32, next: u16) {
        self.wait = n;
        self.step = next;
    }
    /// `yield return null`: continue at `next` on the next tick.
    pub fn yield_next(&mut self, next: u16) {
        self.wait = 0.0;
        self.step = next;
    }
    pub fn finish(&mut self) {
        self.active = false;
        self.finished = true;
    }
}

// ------------------------------------------------------------ Sprite clock

/// Animation tables the sim reads (from sprite data).
#[derive(Clone, Debug, PartialEq)]
pub struct AnimDef {
    pub id: String,
    pub delay: f32,
    pub looped: bool,
    pub frames: Vec<i32>,
}

/// The playback state of a `Sprite<T>` (engine.md §4.10). Indices refer to
/// a [`crate::content::SpriteDef`] in the content.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteAnim {
    /// Index of the playing animation in its sprite's table (`u8::MAX` = none).
    pub anim: u8,
    /// `AnimationFrame` (index within the animation).
    pub frame_index: u8,
    /// The displayed frame (index into the sprite's frame grid).
    pub frame: i16,
    pub timer: f32,
    pub rate: f32,
    pub playing: bool,
    pub finished: bool,
}

impl_hash!(SpriteAnim {
    anim,
    frame_index,
    frame,
    timer,
    rate,
    playing,
    finished
});

impl Default for SpriteAnim {
    fn default() -> Self {
        SpriteAnim {
            anim: u8::MAX,
            frame_index: 0,
            frame: 0,
            timer: 0.0,
            rate: 1.0,
            playing: false,
            finished: false,
        }
    }
}

/// Animation change notifications from [`SpriteAnim::update`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnimStep {
    pub frame_changed: bool,
    pub completed: bool,
}

impl SpriteAnim {
    /// `Play(id, restart)`: (re)starts only if `restart`, or the sprite is
    /// stopped (neither playing nor finished), or `anim` differs. A finished
    /// non-looping animation therefore holds its last frame.
    pub fn play(&mut self, anims: &[AnimDef], anim: u8, restart: bool) {
        if anim as usize >= anims.len() {
            return;
        }
        if restart || (!self.playing && !self.finished) || anim != self.anim {
            self.anim = anim;
            self.frame_index = 0;
            self.timer = 0.0;
            self.finished = false;
            self.playing = true;
            self.frame = anims[anim as usize].frames.first().copied().unwrap_or(0) as i16;
        }
    }

    /// One tick of the sprite clock.
    pub fn update(&mut self, anims: &[AnimDef], delta_time: f32) -> AnimStep {
        let mut out = AnimStep::default();
        if !self.playing || self.anim as usize >= anims.len() {
            return out;
        }
        let a = &anims[self.anim as usize];
        if a.delay <= 0.0 || a.frames.is_empty() {
            return out;
        }
        self.timer += delta_time * self.rate.abs();
        let len = a.frames.len() as i32;
        let dir: i32 = if self.rate >= 0.0 { 1 } else { -1 };
        while self.timer >= a.delay {
            self.timer -= a.delay;
            let mut fi = self.frame_index as i32 + dir;
            if fi == len {
                fi = 0;
                if a.looped {
                    let f = a.frames[0] as i16;
                    out.frame_changed |= f != self.frame;
                    self.frame = f;
                } else {
                    self.finished = true;
                    self.playing = false;
                }
                self.frame_index = fi as u8;
                out.completed = true;
                if !a.looped {
                    break;
                }
            } else if fi == -1 {
                fi = len - 1;
                if a.looped {
                    self.frame = a.frames[0] as i16;
                } else {
                    self.finished = true;
                    self.playing = false;
                }
                self.frame_index = fi as u8;
                out.completed = true;
                if !a.looped {
                    break;
                }
            } else {
                self.frame_index = fi as u8;
                let f = a.frames[fi as usize] as i16;
                out.frame_changed |= f != self.frame;
                self.frame = f;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::FRAME_SECONDS;

    fn advance_moments(delay: f32, n: usize) -> Vec<usize> {
        let anims = vec![AnimDef {
            id: "a".into(),
            delay,
            looped: true,
            frames: (0..100).collect(),
        }];
        let mut s = SpriteAnim::default();
        s.play(&anims, 0, true);
        let mut out = vec![];
        let mut k = 0;
        while out.len() < n {
            k += 1;
            let before = s.frame_index;
            s.update(&anims, FRAME_SECONDS);
            if s.frame_index != before {
                out.push(k);
            }
        }
        out
    }

    #[test]
    fn sprite_timer_table() {
        // engine.md §4.10
        assert_eq!(advance_moments(0.1, 4), [6, 12, 18, 24]);
        assert_eq!(advance_moments(0.08, 6), [5, 10, 15, 20, 24, 29]);
        assert_eq!(advance_moments(0.06, 6), [4, 8, 11, 15, 18, 22]);
        assert_eq!(advance_moments(0.05, 4), [3, 6, 9, 12]);
        assert_eq!(advance_moments(0.07, 6), [5, 9, 13, 17, 21, 26]);
        assert_eq!(advance_moments(0.04, 6), [3, 5, 8, 10, 12, 15]);
        assert_eq!(advance_moments(0.09, 6), [6, 11, 17, 22, 28, 33]);
        assert_eq!(advance_moments(1.0, 3), [61, 121, 181]);
    }

    #[test]
    fn alarm_fires_on_duration_th_update() {
        let mut a = Alarm::new(3, AlarmMode::Oneshot);
        a.start();
        assert!(!a.update(1.0));
        assert!(!a.update(1.0));
        assert!(a.update(1.0));
        a.after_fire();
        assert!(a.removed);
    }

    #[test]
    fn tween_percent_sequence() {
        let mut t = Tween::new(TweenMode::Oneshot, None, 4);
        t.start(false);
        let mut ps = vec![];
        for _ in 0..4 {
            let s = t.update(1.0);
            ps.push(t.percent);
            if ps.len() < 4 {
                assert!(!s.completed);
            } else {
                assert!(s.completed);
            }
        }
        assert_eq!(ps, [0.25, 0.5, 0.75, 1.0]);
    }

    #[test]
    fn scheduler_order() {
        let mut s: Scheduler<u8, 4> = Scheduler::default();
        s.schedule(1, 2.0, false);
        s.schedule(2, 1.0, false);
        let (f, n) = s.update(1.0);
        assert_eq!(&f[..n], &[2]);
        let (f, n) = s.update(1.0);
        assert_eq!(&f[..n], &[1]);
        assert!(s.is_empty());
    }

    #[test]
    fn coroutine_wait_timing() {
        // yield return 2 at tick 0 resumes at tick 3.
        let mut c = Coroutine::start(0);
        let mut resumed = vec![];
        for tick in 0..6 {
            if let Some(step) = c.update(1.0) {
                resumed.push(tick);
                if step == 0 {
                    c.yield_frames(2.0, 1);
                } else {
                    c.finish();
                }
            }
        }
        assert_eq!(resumed, [0, 3]);
    }

    #[test]
    fn ease_endpoints() {
        for e in [
            Ease::QuadOut,
            Ease::CubeInOut,
            Ease::BackOut,
            Ease::SineInOut,
            Ease::BounceOut,
        ] {
            assert!((e.apply(1.0) - 1.0).abs() < 1e-5, "{e:?}");
            assert!(e.apply(0.0).abs() < 1e-5, "{e:?}");
        }
    }
}
