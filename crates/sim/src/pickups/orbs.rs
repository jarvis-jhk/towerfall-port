//! `OrbLogic` (pickups.md §4): the dark, time, lava, space and chaos orbs.
//! The slow-motion counters are [`crate::time::OrbLogic`]; this module adds
//! darkness, the space-orb camera scroll and the entry points, and runs
//! once per unfrozen tick right after the slow-motion update (before the
//! entities).

use crate::components::Ease;
use crate::content::Content;
use crate::events::Event;
use crate::impl_hash;
use crate::math::{Vec2, lerp, round_he};
use crate::variants::Variant;
use crate::world::World;

pub const SPACE_FRAMES: f32 = 360.0;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Orbs {
    pub dark_start: f32,
    pub dark_end: f32,
    /// Darkness is on (the lighting layer's alpha is 1).
    pub dark: bool,
    /// Last observed slow-motion target (for the time-orb sounds).
    pub slow_target: f32,
    /// Space orb: camera offset tween.
    pub camera: Vec2,
    pub space_from: Vec2,
    pub space_to: Vec2,
    pub space_t: f32,
    pub space_on: bool,
    /// Chaos orb's delayed scroll (frames left, 0 = none).
    pub space_delay: f32,
    /// Length of the running camera tween (360 space orb, 90 offset world /
    /// end of scroll) and its easing (true = CubeOut, else CubeInOut).
    pub space_len: f32,
    pub space_out: bool,
    /// Always Scrolling / Quest scroll waves: speed and its target.
    pub scroll_speed: Vec2,
    pub scroll_target: Vec2,
}

impl_hash!(Orbs {
    dark_start,
    dark_end,
    dark,
    slow_target,
    camera,
    space_from,
    space_to,
    space_t,
    space_on,
    space_delay,
    space_len,
    space_out,
    scroll_speed,
    scroll_target
});

/// One tick of the orb logic (after `OrbLogic.update`).
pub fn update(w: &mut World, c: &Content) {
    let tm = w.time.time_mult;
    // Time orb sounds follow the slow-motion target.
    let target = w.orb.game_rate_target;
    let last = w.items.orbs.slow_target;
    if target != last {
        if target < 1.0 {
            w.sound("pu_timeOrb", 160.0);
        } else if last != 0.0 {
            w.sound("pu_timeReturn", 160.0);
        }
        w.items.orbs.slow_target = target;
    }
    // Darkness.
    let o = &mut w.items.orbs;
    if o.dark_end > 0.0 {
        o.dark_end -= tm;
        if o.dark_end <= 0.0 {
            end_dark(w);
        }
    } else if o.dark_start > 0.0 {
        o.dark_start -= tm;
        if o.dark_start <= 0.0 {
            if !crate::pickups::variant_any(w, Variant::AlwaysDark) {
                w.items.orbs.dark_end = w.rng.range_i(600, 300) as f32;
            }
            start_dark(w);
        }
    }
    // Space orb.
    if w.items.orbs.space_delay > 0.0 {
        w.items.orbs.space_delay -= tm;
        if w.items.orbs.space_delay <= 0.0 && w.items.orbs.scroll_target.is_zero() {
            do_space_orb(w, c);
        }
    }
    let o = &mut w.items.orbs;
    if o.space_on {
        o.space_t += tm;
        let len = if o.space_len > 0.0 {
            o.space_len
        } else {
            SPACE_FRAMES
        };
        let ease = if o.space_out {
            Ease::CubeOut
        } else {
            Ease::CubeInOut
        };
        let e = ease.apply((o.space_t / len).min(1.0));
        o.camera = Vec2::new(
            lerp(o.space_from.x, o.space_to.x, e),
            lerp(o.space_from.y, o.space_to.y, e),
        );
        if o.space_t >= len {
            o.space_on = false;
        }
    } else if !o.scroll_target.is_zero() {
        o.scroll_speed = approach_vec(o.scroll_speed, o.scroll_target, 0.02 * tm);
        o.camera += o.scroll_speed * tm;
    }
}

fn approach_vec(v: Vec2, t: Vec2, d: f32) -> Vec2 {
    Vec2::new(
        crate::math::approach(v.x, t.x, d),
        crate::math::approach(v.y, t.y, d),
    )
}

fn tween_camera(w: &mut World, to: Vec2, len: f32, out: bool) {
    let o = &mut w.items.orbs;
    o.space_from = o.camera;
    o.space_to = to;
    o.space_t = 0.0;
    o.space_len = len;
    o.space_out = out;
    o.space_on = true;
}

/// `StartScroll()` (Always Scrolling at round start, Quest scroll waves):
/// the camera drifts at 0.7 px/tick in a random cardinal direction.
pub fn start_scroll(w: &mut World) {
    let dirs = [
        Vec2::new(0.7, 0.0),
        Vec2::new(-0.7, 0.0),
        Vec2::new(0.0, -0.7),
        Vec2::new(0.0, 0.7),
    ];
    w.items.orbs.scroll_target = dirs[w.rng.choose_index(4)];
}

/// `EndScroll()`: stop and ease onto the screen grid in 90 ticks.
pub fn end_scroll(w: &mut World, c: &Content) {
    let ws = w.world_size(c);
    let offset = super::variant_any(w, Variant::OffsetWorld);
    let o = &mut w.items.orbs;
    o.scroll_target = Vec2::ZERO;
    o.scroll_speed = Vec2::ZERO;
    o.space_delay = 0.0;
    let snap = |v: f32, size: f32| {
        let half = if offset { size / 2.0 } else { 0.0 };
        round_he((v - half) / size) * size + half
    };
    let to = Vec2::new(snap(o.camera.x, ws.w), snap(o.camera.y, ws.h));
    tween_camera(w, to, 90.0, true);
}

/// `DoOffsetWorldVariant()` (round intro): ease the camera by half a
/// screen diagonally in 90 ticks.
pub fn do_offset_world(w: &mut World, c: &Content) {
    let ws = w.world_size(c);
    let (hx, hy) = (ws.w / 2.0, ws.h / 2.0);
    let corners = [
        Vec2::new(hx, hy),
        Vec2::new(-hx, hy),
        Vec2::new(hx, -hy),
        Vec2::new(-hx, -hy),
    ];
    let to = corners[w.rng.choose_index(4)];
    tween_camera(w, to, 90.0, false);
}

/// `StartDark()` (Quest dark waves, Always Dark).
pub fn start_dark(w: &mut World) {
    if w.items.orbs.dark {
        return;
    }
    w.items.orbs.dark = true;
    w.sound("pu_shadowFalls", 160.0);
    w.emit(Event::Loop {
        name: "pu_darkOrbCrickets",
        player: u8::MAX,
        on: true,
        x: 160.0,
    });
}

/// `EndDark()`.
pub fn end_dark(w: &mut World) {
    if !w.items.orbs.dark {
        return;
    }
    w.items.orbs.dark = false;
    w.sound("pu_shadowRecedes", 160.0);
    w.emit(Event::Loop {
        name: "pu_darkOrbCrickets",
        player: u8::MAX,
        on: false,
        x: 160.0,
    });
}

/// `DoDarkOrb()`: darkness after 30 frames for 600–899 frames (an active
/// darkness is only ever extended).
pub fn do_dark_orb(w: &mut World) {
    if super::level_ending(w) {
        return;
    }
    if w.items.orbs.dark_end > 0.0 {
        let r = w.rng.range_i(600, 300) as f32;
        w.items.orbs.dark_end = crate::math::max(w.items.orbs.dark_end, r);
    } else {
        w.items.orbs.dark_start = 30.0;
    }
}

/// `DoTimeOrb(delay)`.
pub fn do_time_orb(w: &mut World, delay: bool) {
    let ending = super::level_ending(w);
    w.orb.do_time_orb(delay, ending, &mut w.rng);
}

/// `DoLavaOrb(owner)`.
pub fn do_lava_orb(w: &mut World, c: &Content, owner: i8) {
    if super::level_ending(w) {
        return;
    }
    crate::entities::start_lava(w, c, owner, false);
}

/// `DoSpaceOrb()`: scroll the camera one screen in a random direction
/// (presentation only; the draw is gameplay randomness).
pub fn do_space_orb(w: &mut World, c: &Content) {
    if super::level_ending(w) {
        return;
    }
    let ws = w.world_size(c);
    let shifts = [
        Vec2::new(-ws.w, 0.0),
        Vec2::new(ws.w, 0.0),
        Vec2::new(0.0, -ws.h),
        Vec2::new(0.0, ws.h),
    ];
    let shift = shifts[w.rng.choose_index(4)];
    let offset = super::variant_any(w, Variant::OffsetWorld);
    let o = &mut w.items.orbs;
    let snap = |v: f32, size: f32| {
        let half = if offset { size / 2.0 } else { 0.0 };
        round_he((v - half) / size) * size + half
    };
    let to = Vec2::new(snap(o.camera.x, ws.w), snap(o.camera.y, ws.h)) + shift;
    tween_camera(w, to, SPACE_FRAMES, false);
}

/// `DoSpaceOrbDelayed()` (chaos orb: 60 frames later).
pub fn do_space_orb_delayed(w: &mut World) {
    if super::level_ending(w) {
        return;
    }
    w.items.orbs.space_delay = 60.0;
}

/// Round start: AlwaysDark (SlowTime and AlwaysLava are started by the
/// round logic and the tower entities).
pub fn round_start(w: &mut World, _c: &Content) {
    if super::variant_any(w, Variant::AlwaysDark) {
        do_dark_orb(w);
    }
}
