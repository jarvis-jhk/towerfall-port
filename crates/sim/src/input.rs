//! Player input: what crosses the network ([`PlayerInput`], held state
//! only) and what the archer reads each frame ([`InputState`], player.md §2),
//! derived inside the sim from this and the previous frame's buttons.

use crate::impl_hash;
use crate::math::{PI, Vec2, atan2, snap};
use bytemuck::{Pod, Zeroable};

/// One player's controls for one frame. Byte-compatible with
/// `tfp_net::TfInput`: axes are -120..=120 (+y = down), buttons held.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Pod, Zeroable)]
pub struct PlayerInput {
    pub aim_x: i8,
    pub aim_y: i8,
    pub buttons: u8,
    pub _pad: u8,
}

impl PlayerInput {
    pub const JUMP: u8 = 1 << 0;
    pub const SHOOT: u8 = 1 << 1;
    pub const DODGE: u8 = 1 << 2;
    pub const ALT_SHOOT: u8 = 1 << 3;
    pub const START: u8 = 1 << 4;
    /// Toggle arrow type (keyboard S / pad Y).
    pub const ARROWS: u8 = 1 << 5;
    pub const AXIS_MAX: i8 = 120;

    /// Keyboard / d-pad style: 8 directions at full deflection.
    pub fn dpad(x: i8, y: i8, buttons: u8) -> PlayerInput {
        PlayerInput {
            aim_x: x.signum() * Self::AXIS_MAX,
            aim_y: y.signum() * Self::AXIS_MAX,
            buttons,
            _pad: 0,
        }
    }

    pub fn held(&self, b: u8) -> bool {
        self.buttons & b != 0
    }

    /// The axis as the original's float vector.
    pub fn axis(&self) -> Vec2 {
        Vec2::new(
            self.aim_x as f32 / Self::AXIS_MAX as f32,
            self.aim_y as f32 / Self::AXIS_MAX as f32,
        )
    }
}

/// `InputState` (player.md §2.1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InputState {
    pub jump_check: bool,
    pub jump_pressed: bool,
    pub shoot_check: bool,
    pub shoot_pressed: bool,
    pub alt_shoot_check: bool,
    pub alt_shoot_pressed: bool,
    pub dodge_check: bool,
    pub dodge_pressed: bool,
    pub arrows_pressed: bool,
    /// Start went down this frame (menus / results prompts).
    pub start_pressed: bool,
    pub move_x: i32,
    pub move_y: i32,
    /// (0, 0) = no aim input.
    pub aim_axis: Vec2,
}

impl_hash!(InputState {
    jump_check,
    jump_pressed,
    shoot_check,
    shoot_pressed,
    alt_shoot_check,
    alt_shoot_pressed,
    dodge_check,
    dodge_pressed,
    arrows_pressed,
    start_pressed,
    move_x,
    move_y,
    aim_axis
});

impl InputState {
    /// The gamepad rules of player.md §2.3 applied to the transmitted axis
    /// (keyboard input arrives at full deflection, where they agree with
    /// the keyboard rules of §2.2).
    pub fn derive(now: PlayerInput, prev_buttons: u8) -> InputState {
        let a = now.axis();
        let pressed = |b: u8| now.buttons & b != 0 && prev_buttons & b == 0;
        let move_x = if a.x.abs() < 0.5 {
            0
        } else {
            a.x.signum() as i32
        };
        let move_y = if a.y.abs() < 0.8 {
            0
        } else {
            a.y.signum() as i32
        };
        let aim_axis = if a.x * a.x + a.y * a.y < 0.09 {
            Vec2::ZERO
        } else {
            a
        };
        InputState {
            jump_check: now.held(PlayerInput::JUMP),
            jump_pressed: pressed(PlayerInput::JUMP),
            shoot_check: now.held(PlayerInput::SHOOT),
            shoot_pressed: pressed(PlayerInput::SHOOT),
            alt_shoot_check: now.held(PlayerInput::ALT_SHOOT),
            alt_shoot_pressed: pressed(PlayerInput::ALT_SHOOT),
            dodge_check: now.held(PlayerInput::DODGE),
            dodge_pressed: pressed(PlayerInput::DODGE),
            arrows_pressed: pressed(PlayerInput::ARROWS),
            start_pressed: pressed(PlayerInput::START),
            move_x,
            move_y,
            aim_axis,
        }
    }

    /// `PlayerInput.GetAimDirection` (player.md §2.4).
    pub fn aim_direction(&self, facing: i32, free_aiming: bool) -> f32 {
        if self.aim_axis.is_zero() {
            return if facing > 0 { 0.0 } else { PI };
        }
        let angle = atan2(self.aim_axis.y, self.aim_axis.x);
        if free_aiming {
            angle
        } else {
            snap(angle, PI / 4.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds() {
        let s = InputState::derive(
            PlayerInput {
                aim_x: 70,
                aim_y: 90,
                buttons: 0,
                _pad: 0,
            },
            0,
        );
        assert_eq!((s.move_x, s.move_y), (1, 0)); // 0.58 / 0.75
        let s = InputState::derive(PlayerInput::dpad(0, 1, PlayerInput::JUMP), 0);
        assert_eq!(s.move_y, 1);
        assert!(s.jump_pressed);
        let s = InputState::derive(
            PlayerInput::dpad(0, 1, PlayerInput::JUMP),
            PlayerInput::JUMP,
        );
        assert!(!s.jump_pressed && s.jump_check);
        assert_eq!(s.aim_direction(1, false), PI / 2.0);
        let s = InputState::derive(PlayerInput::dpad(0, 0, 0), 0);
        assert_eq!(s.aim_direction(-1, false), PI);
    }
}
