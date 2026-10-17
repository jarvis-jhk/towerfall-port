//! Fixed 60 Hz simulation clock with vsync-aware frame pacing.
//!
//! The original runs its logic at 1/60 s per update. We run exactly that,
//! fixed, and draw the latest state (no render interpolation: pixel art at
//! 60 Hz). Measured frame times jitter around the display's refresh
//! period; snapping them to whole refresh periods keeps a 60 Hz display at
//! exactly one tick per frame instead of occasionally 0 or 2.

/// Simulation rate.
pub const TICK_HZ: f64 = 60.0;
/// One simulation step in seconds.
pub const TICK_DT: f64 = 1.0 / TICK_HZ;

/// Turns variable frame times into a whole number of fixed ticks.
#[derive(Clone, Debug)]
pub struct FixedStep {
    acc: f64,
    ticks: u64,
    /// Longest frame time accepted (hitches are clamped, so the sim does
    /// not spiral trying to catch up).
    max_frame: f64,
    /// Display refresh period to snap measured frame times to (0 = off).
    refresh: f64,
}

/// Snap frame times within this distance of a refresh multiple.
const SNAP_TOLERANCE: f64 = 0.0005;

impl Default for FixedStep {
    fn default() -> Self {
        Self::new()
    }
}

impl FixedStep {
    pub fn new() -> Self {
        Self {
            acc: 0.0,
            ticks: 0,
            max_frame: 0.25,
            refresh: 0.0,
        }
    }

    /// Enables snapping to the display's refresh rate (Hz; 0 disables).
    pub fn set_refresh_rate(&mut self, hz: f64) {
        self.refresh = if hz > 1.0 { 1.0 / hz } else { 0.0 };
    }

    /// Adds a frame's elapsed time; returns how many ticks to run now.
    pub fn advance(&mut self, frame_dt: f64) -> u32 {
        let mut dt = frame_dt.clamp(0.0, self.max_frame);
        if self.refresh > 0.0 {
            let k = (dt / self.refresh).round();
            if k >= 1.0 && (dt - k * self.refresh).abs() < SNAP_TOLERANCE {
                dt = k * self.refresh;
            }
        }
        self.acc += dt;
        let mut n = 0;
        // The epsilon absorbs float error of exactly-1/60 frames.
        while self.acc >= TICK_DT - 1e-9 {
            self.acc -= TICK_DT;
            n += 1;
        }
        self.acc = self.acc.max(0.0);
        self.ticks += n as u64;
        n
    }

    /// Total ticks run so far.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Seconds until the next tick is due (for sleeping without vsync).
    pub fn until_next_tick(&self) -> f64 {
        (TICK_DT - self.acc).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_hz_vsync_is_one_tick_per_frame() {
        let mut f = FixedStep::new();
        f.set_refresh_rate(60.0);
        // Jittery measurements around 16.67 ms.
        for i in 0..6000 {
            let jitter = if i % 2 == 0 { 0.0004 } else { -0.0004 };
            assert_eq!(f.advance(TICK_DT + jitter), 1, "frame {i}");
        }
        assert_eq!(f.ticks(), 6000);
    }

    #[test]
    fn other_refresh_rates() {
        let mut f = FixedStep::new();
        f.set_refresh_rate(120.0);
        let n: u32 = (0..1200).map(|_| f.advance(1.0 / 120.0 + 0.0002)).sum();
        assert_eq!(n, 600);
        let mut f = FixedStep::new();
        f.set_refresh_rate(144.0);
        let n: u32 = (0..1440).map(|_| f.advance(1.0 / 144.0)).sum();
        assert!((599..=600).contains(&n), "{n}");
        // 30 Hz: two ticks per frame.
        let mut f = FixedStep::new();
        f.set_refresh_rate(30.0);
        assert_eq!(f.advance(1.0 / 30.0 - 0.0003), 2);
    }

    #[test]
    fn hitch_is_clamped() {
        let mut f = FixedStep::new();
        assert_eq!(f.advance(5.0), 15);
        assert!(f.until_next_tick() > 0.0);
    }
}
