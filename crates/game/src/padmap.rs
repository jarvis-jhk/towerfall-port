//! Controller button mapping for gameplay (ours: the original has no pad
//! rebinding, input.md 2.1). One map per controller model (SDL name); the
//! default is the original's layout. Menus keep the fixed menu buttons.

use sdl2::controller::{Axis, Button};

use crate::input::PadState;

/// One physical input: a button or a trigger (pressed at >= 0.1, input.md
/// 2.1; capture uses 0.5 so a resting trigger is not taken).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadInput {
    Button(Button),
    TriggerLeft,
    TriggerRight,
}

impl PadInput {
    pub fn held(self, p: &PadState) -> bool {
        match self {
            PadInput::Button(b) => p.held.contains(b),
            PadInput::TriggerLeft => p.axis(Axis::TriggerLeft) >= 0.1,
            PadInput::TriggerRight => p.axis(Axis::TriggerRight) >= 0.1,
        }
    }

    /// Name for files and the remap screen (SDL's button names).
    pub fn name(self) -> String {
        match self {
            PadInput::Button(b) => b.string(),
            PadInput::TriggerLeft => "lefttrigger".into(),
            PadInput::TriggerRight => "righttrigger".into(),
        }
    }

    pub fn parse(s: &str) -> Option<PadInput> {
        match s {
            "lefttrigger" => Some(PadInput::TriggerLeft),
            "righttrigger" => Some(PadInput::TriggerRight),
            _ => Button::from_string(s).map(PadInput::Button),
        }
    }

    /// Short label for the remap screen.
    pub fn label(self) -> String {
        match self {
            PadInput::Button(Button::A) => "A".into(),
            PadInput::Button(Button::B) => "B".into(),
            PadInput::Button(Button::X) => "X".into(),
            PadInput::Button(Button::Y) => "Y".into(),
            PadInput::Button(Button::LeftShoulder) => "LB".into(),
            PadInput::Button(Button::RightShoulder) => "RB".into(),
            PadInput::Button(Button::LeftStick) => "LS".into(),
            PadInput::Button(Button::RightStick) => "RS".into(),
            PadInput::TriggerLeft => "LT".into(),
            PadInput::TriggerRight => "RT".into(),
            other => other.name().to_uppercase(),
        }
    }
}

/// Buttons that can be bound (not the d-pad: it moves; not Guide; not
/// Back/View, which cancels in the remap screen).
pub const BINDABLE: [Button; 13] = [
    Button::A,
    Button::B,
    Button::X,
    Button::Y,
    Button::Start,
    Button::LeftStick,
    Button::RightStick,
    Button::LeftShoulder,
    Button::RightShoulder,
    Button::Misc1,
    Button::Paddle1,
    Button::Paddle2,
    Button::Touchpad,
];

/// The gameplay actions in the remap screen's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadAction {
    Jump,
    Shoot,
    Dodge,
    AltShoot,
    Arrows,
    Start,
}

impl PadAction {
    pub const ALL: [PadAction; 6] = [
        PadAction::Jump,
        PadAction::Shoot,
        PadAction::Dodge,
        PadAction::AltShoot,
        PadAction::Arrows,
        PadAction::Start,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PadAction::Jump => "JUMP",
            PadAction::Shoot => "SHOOT",
            PadAction::Dodge => "DODGE",
            PadAction::AltShoot => "ALT SHOOT",
            PadAction::Arrows => "ARROW TOGGLE",
            PadAction::Start => "START",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            PadAction::Jump => "jump",
            PadAction::Shoot => "shoot",
            PadAction::Dodge => "dodge",
            PadAction::AltShoot => "alt_shoot",
            PadAction::Arrows => "arrows",
            PadAction::Start => "start",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PadMap {
    /// Inputs per action, in `PadAction::ALL` order.
    pub binds: [Vec<PadInput>; 6],
}

impl Default for PadMap {
    /// The original layout (input.md 2.1).
    fn default() -> Self {
        use PadInput::*;
        Self {
            binds: [
                vec![Button(sdl2::controller::Button::A)],
                vec![Button(sdl2::controller::Button::X)],
                vec![
                    Button(sdl2::controller::Button::LeftShoulder),
                    Button(sdl2::controller::Button::RightShoulder),
                    TriggerLeft,
                    TriggerRight,
                ],
                vec![Button(sdl2::controller::Button::B)],
                vec![Button(sdl2::controller::Button::Y)],
                vec![Button(sdl2::controller::Button::Start)],
            ],
        }
    }
}

impl PadMap {
    pub fn held(&self, p: &PadState, a: PadAction) -> bool {
        let i = PadAction::ALL.iter().position(|&x| x == a).unwrap_or(0);
        self.binds[i].iter().any(|b| b.held(p))
    }

    /// `jump=a dodge=leftshoulder,rightshoulder ...` (one line per map).
    pub fn to_line(&self) -> String {
        PadAction::ALL
            .iter()
            .zip(&self.binds)
            .map(|(a, v)| {
                let names: Vec<String> = v.iter().map(|b| b.name()).collect();
                format!("{}={}", a.key(), names.join(","))
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Parses `to_line`; actions not given keep their defaults.
    pub fn from_line(s: &str) -> PadMap {
        let mut m = PadMap::default();
        for part in s.split_whitespace() {
            let Some((k, v)) = part.split_once('=') else {
                continue;
            };
            if let Some(i) = PadAction::ALL.iter().position(|a| a.key() == k) {
                m.binds[i] = v.split(',').filter_map(PadInput::parse).collect();
            }
        }
        m
    }

    /// The input newly pressed this tick on `p` (capture for remapping).
    pub fn captured(p: &PadState, prev_triggers: (f32, f32)) -> Option<PadInput> {
        if let Some(&b) = BINDABLE.iter().find(|&&b| p.pressed.contains(b)) {
            return Some(PadInput::Button(b));
        }
        let (l, r) = (p.axis(Axis::TriggerLeft), p.axis(Axis::TriggerRight));
        if l >= 0.5 && prev_triggers.0 < 0.5 {
            return Some(PadInput::TriggerLeft);
        }
        if r >= 0.5 && prev_triggers.1 < 0.5 {
            return Some(PadInput::TriggerRight);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_round_trip() {
        let mut m = PadMap::default();
        assert_eq!(PadMap::from_line(&m.to_line()), m);
        m.binds[0] = vec![PadInput::Button(Button::B)];
        m.binds[2] = vec![PadInput::TriggerRight];
        let back = PadMap::from_line(&m.to_line());
        assert_eq!(back, m);
        assert!(m.to_line().contains("jump=b"), "{}", m.to_line());
        // Unknown parts are ignored; missing actions keep defaults.
        let p = PadMap::from_line("shoot=y nonsense dodge=");
        assert_eq!(p.binds[1], vec![PadInput::Button(Button::Y)]);
        assert!(p.binds[2].is_empty());
        assert_eq!(p.binds[0], PadMap::default().binds[0]);
    }
}
