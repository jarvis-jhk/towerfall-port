//! The time model (engine.md §1): fixed 60 Hz ticks, `TimeRate`/`TimeMult`,
//! `FrameCounter`/`OnInterval`, and `OrbLogic`'s slow-motion state.

use crate::impl_hash;
use crate::math::approach;

/// `1f / 60f` as an f32 (0.016666668).
pub const FRAME_SECONDS: f32 = 1.0 / 60.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Time {
    /// `Engine.TimeRate` (1 normally; slow motion lowers it).
    pub time_rate: f32,
    /// `Engine.TimeMult` of the current tick (= the rate at the tick start).
    pub time_mult: f32,
    /// `TimeMult` of the previous tick.
    pub last_time_mult: f32,
    /// `Engine.DeltaTime` (seconds) of the current tick.
    pub delta_time: f32,
    /// `Scene.FrameCounter` (f32, += TimeMult at the end of every scene update).
    pub frame_counter: f32,
}

impl_hash!(Time {
    time_rate,
    time_mult,
    last_time_mult,
    delta_time,
    frame_counter
});

impl Default for Time {
    fn default() -> Self {
        Time {
            time_rate: 1.0,
            time_mult: 1.0,
            last_time_mult: 1.0,
            delta_time: FRAME_SECONDS,
            frame_counter: 0.0,
        }
    }
}

impl Time {
    /// Start of a tick: the fixed-step branch of `Engine.Update`.
    pub fn begin_tick(&mut self) {
        self.last_time_mult = self.time_mult;
        self.time_mult = self.time_rate;
        self.delta_time = FRAME_SECONDS * self.time_rate;
    }

    /// `Scene.OnInterval(n)`: true iff an integer `i` in
    /// `[(int)(FrameCounter - LastTimeMult) + 1 ..= (int)FrameCounter]` has `i % n == 0`.
    pub fn on_interval(&self, n: i32) -> bool {
        let lo = (self.frame_counter - self.last_time_mult) as i32 + 1;
        let hi = self.frame_counter as i32;
        (lo..=hi).any(|i| i % n == 0)
    }

    /// End of `Scene.Update`.
    pub fn end_scene_update(&mut self) {
        self.frame_counter += self.time_mult;
    }
}

/// The slow-motion part of `OrbLogic` (engine.md §1.4). Darkness and the
/// space orb are cosmetic and not modelled here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbLogic {
    pub slow_time_start: f32,
    pub slow_time_end: f32,
    pub game_rate_target: f32,
    pub game_rate_ease: bool,
}

impl_hash!(OrbLogic {
    slow_time_start,
    slow_time_end,
    game_rate_target,
    game_rate_ease
});

impl Default for OrbLogic {
    fn default() -> Self {
        OrbLogic {
            slow_time_start: 0.0,
            slow_time_end: 0.0,
            game_rate_target: 1.0,
            game_rate_ease: false,
        }
    }
}

impl OrbLogic {
    /// One unfrozen tick (`OrbLogic.Update`, slow-time part and the rate approach).
    pub fn update(&mut self, time: &mut Time, slow_time_variant: bool, rng: &mut crate::rng::Rng) {
        let tm = time.time_mult;
        if self.slow_time_end > 0.0 {
            self.slow_time_end -= tm;
            if self.slow_time_end <= 0.0 {
                self.end_slow(time);
            }
        } else if self.slow_time_start > 0.0 {
            self.slow_time_start -= tm;
            if self.slow_time_start <= 0.0 {
                if !slow_time_variant {
                    self.slow_time_end = rng.range_i(300, 60) as f32;
                }
                self.start_slow();
            }
        }
        time.time_rate = approach(time.time_rate, self.game_rate_target, 0.01 * tm);
    }

    /// Final-kill slow motion (`DoSlowMoKill`).
    pub fn do_slow_mo_kill(&mut self, time: &mut Time) {
        time.time_rate = 0.5;
        self.game_rate_ease = false;
        self.slow_time_end = 60.0;
        self.slow_time_start = 0.0;
    }

    /// Time orb pickup (`DoTimeOrb`).
    pub fn do_time_orb(&mut self, delay: bool, ending: bool, rng: &mut crate::rng::Rng) {
        if ending {
            return;
        }
        if self.slow_time_end > 0.0 {
            let r = rng.range_i(300, 60) as f32;
            self.slow_time_end = crate::math::max(self.slow_time_end, r);
        } else {
            self.slow_time_start = if delay { 60.0 } else { 10.0 };
        }
    }

    /// `StartSlow()` (Quest slow waves).
    pub fn start_slow(&mut self) {
        self.game_rate_target = 0.5;
        self.game_rate_ease = true;
    }

    /// `EndSlow()`.
    pub fn end_slow(&mut self, time: &mut Time) {
        if !self.game_rate_ease {
            time.time_rate = 1.0;
        }
        self.game_rate_target = 1.0;
    }

    /// `CancelSlowMo`.
    pub fn cancel_slow_mo(&mut self, time: &mut Time) {
        time.time_rate = 1.0;
        self.game_rate_target = 1.0;
        self.slow_time_start = 0.0;
        self.slow_time_end = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_interval_at_time_mult_1() {
        let mut t = Time::default();
        for n_done in 0..20 {
            t.begin_tick();
            assert_eq!(t.on_interval(4), n_done % 4 == 0, "tick {n_done}");
            assert!(t.on_interval(1));
            t.end_scene_update();
        }
    }

    #[test]
    fn on_interval_at_half_rate() {
        let mut t = Time {
            time_rate: 0.5,
            ..Time::default()
        };
        let mut hits = vec![];
        for k in 0..12 {
            t.begin_tick();
            hits.push(t.on_interval(2));
            t.end_scene_update();
            let _ = k;
        }
        // FrameCounter 0, 0.5, 1.0, ... with LastTimeMult 1 on the first tick.
        // tick0: range [0..=0] -> true; tick1 fc 0.5 lmt .5: [1..=0] empty;
        // tick2 fc 1.0: [1..=1] -> false; tick3 fc 1.5: [2..=1] empty; tick4 fc 2: [2..=2] true
        assert_eq!(&hits[..6], &[true, false, false, false, true, false]);
    }

    #[test]
    fn slow_mo_kill_ramp() {
        let mut t = Time::default();
        let mut o = OrbLogic::default();
        let mut rng = crate::rng::Rng::new(0);
        o.do_slow_mo_kill(&mut t);
        let mut mults = vec![];
        let mut reached_one = None;
        for tick in 1..=90 {
            t.begin_tick();
            mults.push(t.time_mult);
            if reached_one.is_none() && t.time_mult >= 1.0 {
                reached_one = Some(tick);
            }
            o.update(&mut t, false, &mut rng);
        }
        assert_eq!(mults[0], 0.5);
        assert_eq!(mults[1], 0.505);
        assert!((mults[2] - 0.51005).abs() < 1e-6);
        let r = reached_one.unwrap();
        assert!((69..=73).contains(&r), "reached 1.0 at tick {r}");
        assert!(o.slow_time_end <= 0.0);
        assert_eq!(t.time_rate, 1.0);
    }
}
