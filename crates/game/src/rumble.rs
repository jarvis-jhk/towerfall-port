//! Controller rumble (input.md 2.4): per pad, a request is taken if it is
//! stronger than the running one (restarting the timer) or equally strong
//! (extending it to the longer of the two); weaker ones are ignored while
//! one runs. Scenes queue requests on `HostRequests::rumble`; the window
//! loop feeds them through [`Rumbles`] and starts the accepted ones.

use std::collections::BTreeMap;

/// The running rumble per pad: (strength 0..1, frames left).
#[derive(Default, Debug)]
pub struct Rumbles {
    on: BTreeMap<u32, (f32, u32)>,
}

impl Rumbles {
    /// Offers a request; `Some((strength, frames))` when it is taken and
    /// the motor must be (re)started for that many frames.
    pub fn request(&mut self, pad: u32, strength: f32, frames: u32) -> Option<(f32, u32)> {
        let strength = strength.clamp(0.0, 1.0);
        if strength <= 0.0 || frames == 0 {
            return None;
        }
        let next = match self.on.get(&pad) {
            Some(&(s, _)) if strength > s => (strength, frames),
            Some(&(s, left)) if strength == s => (s, left.max(frames)),
            Some(_) => return None,
            None => (strength, frames),
        };
        self.on.insert(pad, next);
        Some(next)
    }

    /// One frame passes.
    pub fn tick(&mut self) {
        self.on.retain(|_, (_, left)| {
            *left = left.saturating_sub(1);
            *left > 0
        });
    }
}

/// Frames at 60 Hz to milliseconds.
pub fn frames_ms(frames: u32) -> u32 {
    (frames * 1000).div_ceil(60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stronger_replaces_equal_extends_weaker_ignored() {
        let mut r = Rumbles::default();
        assert_eq!(r.request(1, 0.5, 10), Some((0.5, 10)));
        assert_eq!(r.request(1, 0.2, 30), None);
        assert_eq!(r.request(1, 0.5, 4), Some((0.5, 10)));
        assert_eq!(r.request(1, 0.5, 20), Some((0.5, 20)));
        assert_eq!(r.request(1, 1.0, 5), Some((1.0, 5)));
        for _ in 0..5 {
            r.tick();
        }
        // Expired: anything is taken again.
        assert_eq!(r.request(1, 0.1, 3), Some((0.1, 3)));
        // Pads are independent.
        assert_eq!(r.request(2, 0.05, 3), Some((0.05, 3)));
        assert_eq!(frames_ms(20), 334);
    }
}
