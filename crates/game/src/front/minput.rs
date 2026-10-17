//! Menu input (input.md 3, 6): per device Confirm / Back / Start / Alt /
//! Alt2 / directions with press edges, and the combined "any device"
//! `MenuInput` with the original's direction auto-repeat (fires on press,
//! again after 30 frames, then every 6).

use std::collections::BTreeMap;

use sdl2::controller::{Axis, Button};
use sdl2::keyboard::Scancode;

use crate::input::RawInput;

/// A local input device. Ordered: keyboards first, then pads by id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dev {
    /// Keyboard layout k.
    Keyboard(u8),
    /// SDL joystick instance id.
    Pad(u32),
}

/// Menu buttons as bits.
pub mod b {
    pub const CONFIRM: u16 = 1;
    pub const BACK: u16 = 2;
    pub const START: u16 = 4;
    pub const ALT: u16 = 8;
    pub const ALT2: u16 = 16;
    pub const UP: u16 = 32;
    pub const DOWN: u16 = 64;
    pub const LEFT: u16 = 128;
    pub const RIGHT: u16 = 256;
    pub const DIRS: [u16; 4] = [UP, DOWN, LEFT, RIGHT];
}

/// A keyboard layout: eleven bindings, each a list of keys (input.md 4).
#[derive(Clone, Debug, PartialEq)]
pub struct KeyLayout {
    pub left: Vec<Scancode>,
    pub right: Vec<Scancode>,
    pub up: Vec<Scancode>,
    pub down: Vec<Scancode>,
    pub jump: Vec<Scancode>,
    pub shoot: Vec<Scancode>,
    pub alt_shoot: Vec<Scancode>,
    pub dodge: Vec<Scancode>,
    pub arrows: Vec<Scancode>,
    pub menu_alt: Vec<Scancode>,
    pub start: Vec<Scancode>,
}

impl KeyLayout {
    /// Config 0 defaults: arrows, C jump, X shoot, Z alt shoot, LShift
    /// dodge, S arrow toggle, Tab menu alt, Enter start.
    pub fn default_config() -> Self {
        use Scancode::*;
        Self {
            left: vec![Left],
            right: vec![Right],
            up: vec![Up],
            down: vec![Down],
            jump: vec![C],
            shoot: vec![X],
            alt_shoot: vec![Z],
            dodge: vec![LShift],
            arrows: vec![S],
            menu_alt: vec![Tab],
            start: vec![Return],
        }
    }

    /// Our "shared keyboard" pair (input.md 10.2): left hand on WASD...
    pub fn shared_left() -> Self {
        use Scancode::*;
        Self {
            left: vec![A],
            right: vec![D],
            up: vec![W],
            down: vec![S],
            jump: vec![V],
            shoot: vec![C],
            alt_shoot: vec![X],
            dodge: vec![LShift],
            arrows: vec![Q],
            menu_alt: vec![Tab],
            start: vec![Num1],
        }
    }

    /// ...and right hand on the arrow keys.
    pub fn shared_right() -> Self {
        use Scancode::*;
        Self {
            left: vec![Left],
            right: vec![Right],
            up: vec![Up],
            down: vec![Down],
            jump: vec![Period],
            shoot: vec![Comma],
            alt_shoot: vec![M],
            dodge: vec![RShift],
            arrows: vec![L],
            menu_alt: vec![Semicolon],
            start: vec![Return],
        }
    }

    /// The bindings in the configuration-screen prompt order with labels.
    pub fn bindings_mut(&mut self) -> [(&'static str, &mut Vec<Scancode>); 11] {
        [
            ("JUMP + MENU CONFIRM", &mut self.jump),
            ("SHOOT + MENU CANCEL", &mut self.shoot),
            ("DODGE", &mut self.dodge),
            ("LEFT", &mut self.left),
            ("RIGHT", &mut self.right),
            ("UP", &mut self.up),
            ("DOWN", &mut self.down),
            ("START", &mut self.start),
            ("DODGE + MENU ALT FUNCTIONS", &mut self.menu_alt),
            ("ALT SHOOT", &mut self.alt_shoot),
            ("ARROW TOGGLE", &mut self.arrows),
        ]
    }

    pub fn empty() -> Self {
        Self {
            left: vec![],
            right: vec![],
            up: vec![],
            down: vec![],
            jump: vec![],
            shoot: vec![],
            alt_shoot: vec![],
            dodge: vec![],
            arrows: vec![],
            menu_alt: vec![],
            start: vec![],
        }
    }

    /// Keys of the essential bindings (no duplicates allowed among them).
    pub fn essential_keys(&self) -> Vec<Scancode> {
        [
            &self.left,
            &self.right,
            &self.up,
            &self.down,
            &self.jump,
            &self.shoot,
            &self.dodge,
            &self.menu_alt,
            &self.start,
        ]
        .into_iter()
        .flatten()
        .copied()
        .collect()
    }

    fn held(&self, raw: &RawInput) -> u16 {
        let k = |keys: &[Scancode]| keys.iter().any(|&s| raw.keyboard.held.contains(s));
        let mut h = 0;
        let mut set = |on: bool, bit: u16| {
            if on {
                h |= bit
            }
        };
        set(k(&self.jump), b::CONFIRM);
        set(k(&self.shoot), b::BACK);
        set(k(&self.start), b::START);
        set(k(&self.dodge), b::ALT);
        set(k(&self.menu_alt), b::ALT2);
        // Right wins over left, down over up (input.md 2.2).
        let (l, r) = (k(&self.left), k(&self.right));
        set(r, b::RIGHT);
        set(l && !r, b::LEFT);
        let (u, d) = (k(&self.up), k(&self.down));
        set(d, b::DOWN);
        set(u && !d, b::UP);
        h
    }

    fn pressed(&self, raw: &RawInput) -> u16 {
        let k = |keys: &[Scancode]| keys.iter().any(|&s| raw.keyboard.pressed.contains(s));
        let mut p = 0;
        for (keys, bit) in [
            (&self.jump, b::CONFIRM),
            (&self.shoot, b::BACK),
            (&self.start, b::START),
            (&self.dodge, b::ALT),
            (&self.menu_alt, b::ALT2),
            (&self.left, b::LEFT),
            (&self.right, b::RIGHT),
            (&self.up, b::UP),
            (&self.down, b::DOWN),
        ] {
            if k(keys) {
                p |= bit;
            }
        }
        p
    }
}

/// Which keyboard layouts exist.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyboardSetup {
    pub layouts: Vec<KeyLayout>,
}

impl Default for KeyboardSetup {
    fn default() -> Self {
        Self {
            layouts: vec![KeyLayout::default_config()],
        }
    }
}

impl KeyboardSetup {
    pub fn shared() -> Self {
        Self {
            layouts: vec![KeyLayout::shared_left(), KeyLayout::shared_right()],
        }
    }
}

/// One device's menu state for this tick.
#[derive(Clone, Debug, PartialEq)]
pub struct DevState {
    pub dev: Dev,
    pub held: u16,
    pub pressed: u16,
    /// Display name (`KEYBOARD`, `XBOX 360`, `DUALSHOCK 4`, `???`).
    pub name: String,
    pub icons: IconSet,
}

impl DevState {
    pub fn p(&self, bit: u16) -> bool {
        self.pressed & bit != 0
    }

    pub fn h(&self, bit: u16) -> bool {
        self.held & bit != 0
    }
}

/// Button-icon family (input.md 3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSet {
    Keyboard(u8),
    Xb360,
    DualShock,
}

/// Classifies a controller by its SDL name.
pub fn classify_pad(name: &str) -> (String, IconSet) {
    let n = name.to_ascii_lowercase();
    if [
        "ps4",
        "ps5",
        "dualshock",
        "dualsense",
        "wireless controller",
    ]
    .iter()
    .any(|k| n.contains(k))
    {
        ("DUALSHOCK 4".into(), IconSet::DualShock)
    } else if ["xbox", "x-box", "xinput", "360"]
        .iter()
        .any(|k| n.contains(k))
    {
        ("XBOX 360".into(), IconSet::Xb360)
    } else {
        ("???".into(), IconSet::Xb360)
    }
}

impl IconSet {
    /// Menu-atlas icon for a function: confirm, back, start, alt, alt2.
    pub fn icon(self, bit: u16, layouts: &KeyboardSetup) -> String {
        match self {
            IconSet::Keyboard(k) => {
                let Some(l) = layouts.layouts.get(k as usize) else {
                    return "controls/keyboard/undefined".into();
                };
                let key = match bit {
                    b::CONFIRM => l.jump.first(),
                    b::BACK => l.shoot.first(),
                    b::START => l.start.first(),
                    b::ALT => l.dodge.first(),
                    b::ALT2 => l.menu_alt.first(),
                    _ => None,
                };
                format!(
                    "controls/keyboard/{}",
                    key.and_then(|&s| key_icon(s)).unwrap_or("undefined")
                )
            }
            IconSet::Xb360 => match bit {
                b::CONFIRM => "controls/xb360/a",
                b::BACK => "controls/xb360/b",
                b::START => "controls/xb360/start",
                b::ALT => "controls/xb360/rt",
                b::ALT2 => "controls/xb360/lt",
                _ => "controls/xb360/stick",
            }
            .into(),
            IconSet::DualShock => match bit {
                b::CONFIRM => "controls/dualshock/x",
                b::BACK => "controls/dualshock/circle",
                b::START => "controls/dualshock/options",
                b::ALT => "controls/dualshock/r2",
                b::ALT2 => "controls/dualshock/l2",
                _ => "controls/dualshock/stick",
            }
            .into(),
        }
    }

    /// The rollcall device badge art for player `n` (0-based; only 1-4
    /// exist: 5-12 reuse them, `Painter::badge` adds the number).
    pub fn badge(self, n: usize) -> String {
        match self {
            IconSet::Keyboard(_) => "controls/keyboard/icon".into(),
            IconSet::Xb360 => format!("controls/xb360/player{}", n % 4 + 1),
            IconSet::DualShock => format!("controls/dualshock/player{}", n % 4 + 1),
        }
    }
}

/// XNA `Keys` name of a scancode, if the menu atlas has art for it.
pub fn key_icon(s: Scancode) -> Option<&'static str> {
    use Scancode::*;
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    const DIGITS: [&str; 10] = ["D1", "D2", "D3", "D4", "D5", "D6", "D7", "D8", "D9", "D0"];
    const PAD: [&str; 10] = [
        "NumPad1", "NumPad2", "NumPad3", "NumPad4", "NumPad5", "NumPad6", "NumPad7", "NumPad8",
        "NumPad9", "NumPad0",
    ];
    let c = s as i32;
    if (A as i32..=Z as i32).contains(&c) {
        return Some(LETTERS[(c - A as i32) as usize]);
    }
    if (Num1 as i32..=Num0 as i32).contains(&c) {
        return Some(DIGITS[(c - Num1 as i32) as usize]);
    }
    if (Kp1 as i32..=Kp0 as i32).contains(&c) {
        return Some(PAD[(c - Kp1 as i32) as usize]);
    }
    Some(match s {
        Left => "Left",
        Right => "Right",
        Up => "Up",
        Down => "Down",
        Return | KpEnter => "Enter",
        Tab => "Tab",
        Space => "Space",
        LShift => "LeftShift",
        RShift => "RightShift",
        LCtrl => "LeftControl",
        RCtrl => "RightControl",
        LAlt => "LeftAlt",
        RAlt => "RightAlt",
        Comma => "OemComma",
        Period => "OemPeriod",
        Minus => "OemMinus",
        Equals => "OemPlus",
        Slash => "OemQuestion",
        Semicolon => "OemSemicolon",
        Apostrophe => "OemQuotes",
        LeftBracket => "OemOpenBrackets",
        RightBracket => "OemCloseBrackets",
        Backslash => "OemPipe",
        _ => return None,
    })
}

fn pad_held(p: &crate::input::PadState) -> u16 {
    let mut h = 0;
    let btn = |x| p.held.contains(x);
    let mut set = |on: bool, bit: u16| {
        if on {
            h |= bit
        }
    };
    set(btn(Button::A), b::CONFIRM);
    set(btn(Button::X) || btn(Button::B), b::BACK);
    set(btn(Button::Start), b::START);
    set(
        btn(Button::RightShoulder) || p.axis(Axis::TriggerRight) >= 0.1,
        b::ALT,
    );
    set(
        btn(Button::LeftShoulder) || p.axis(Axis::TriggerLeft) >= 0.1,
        b::ALT2,
    );
    let (x, y) = (p.axis(Axis::LeftX), p.axis(Axis::LeftY));
    set(btn(Button::DPadUp) || y <= -0.5, b::UP);
    set(btn(Button::DPadDown) || y >= 0.5, b::DOWN);
    set(btn(Button::DPadLeft) || x <= -0.5, b::LEFT);
    set(btn(Button::DPadRight) || x >= 0.5, b::RIGHT);
    h
}

fn pad_pressed(p: &crate::input::PadState) -> u16 {
    let mut out = 0;
    for (btns, bit) in [
        (&[Button::A][..], b::CONFIRM),
        (&[Button::X, Button::B][..], b::BACK),
        (&[Button::Start][..], b::START),
        (&[Button::RightShoulder][..], b::ALT),
        (&[Button::LeftShoulder][..], b::ALT2),
        (&[Button::DPadUp][..], b::UP),
        (&[Button::DPadDown][..], b::DOWN),
        (&[Button::DPadLeft][..], b::LEFT),
        (&[Button::DPadRight][..], b::RIGHT),
    ] {
        if btns.iter().any(|&x| p.pressed.contains(x)) {
            out |= bit;
        }
    }
    out
}

/// Every device plus the combined input with auto-repeat.
#[derive(Default)]
pub struct MenuInput {
    pub devs: Vec<DevState>,
    prev: BTreeMap<Dev, u16>,
    repeat: [u32; 4],
    /// Combined (any device) held / pressed; directions with repeat.
    held: u16,
    pressed: u16,
    /// Frames to ignore all input (after scene changes).
    pub lockout: u32,
    /// Keys newly pressed this tick (scancodes, ascending), for key binding.
    pub keys_pressed: Vec<Scancode>,
    /// The controllers' raw state this tick (controller remapping).
    pub raw_pads: Vec<crate::input::PadState>,
}

impl MenuInput {
    /// Samples all devices for one tick.
    pub fn update(&mut self, raw: &RawInput, keys: &KeyboardSetup) {
        self.keys_pressed = raw
            .keyboard
            .pressed
            .iter()
            .filter_map(|c| Scancode::from_i32(c as i32))
            .collect();
        self.raw_pads.clone_from(&raw.pads);
        self.devs.clear();
        for (i, l) in keys.layouts.iter().enumerate() {
            let held = l.held(raw);
            self.devs.push(DevState {
                dev: Dev::Keyboard(i as u8),
                held,
                pressed: l.pressed(raw),
                name: if keys.layouts.len() == 1 {
                    "KEYBOARD".into()
                } else {
                    format!("KEYBOARD {}", i + 1)
                },
                icons: IconSet::Keyboard(i as u8),
            });
        }
        for p in &raw.pads {
            let (name, icons) = classify_pad(&p.name);
            self.devs.push(DevState {
                dev: Dev::Pad(p.id),
                held: pad_held(p),
                pressed: pad_pressed(p),
                name,
                icons,
            });
        }
        // Edges from held changes too (stick and trigger thresholds).
        let mut prev = BTreeMap::new();
        for d in &mut self.devs {
            let was = self.prev.get(&d.dev).copied().unwrap_or(d.held);
            d.pressed |= d.held & !was;
            prev.insert(d.dev, d.held);
        }
        self.prev = prev;
        if self.lockout > 0 {
            self.lockout -= 1;
            for d in &mut self.devs {
                d.pressed = 0;
            }
        }
        self.held = self.devs.iter().fold(0, |a, d| a | d.held);
        let raw_pressed = self.devs.iter().fold(0, |a, d| a | d.pressed);
        self.pressed = raw_pressed & !(b::UP | b::DOWN | b::LEFT | b::RIGHT);
        // Direction repeat on the combined held state.
        for (k, &bit) in b::DIRS.iter().enumerate() {
            let opposite = b::DIRS[k ^ 1];
            let held = self.held & bit != 0 && self.held & opposite == 0;
            if held {
                self.repeat[k] += 1;
                let n = self.repeat[k];
                if n == 1 || (n > 30 && (n - 31).is_multiple_of(6)) {
                    self.pressed |= bit;
                }
            } else {
                self.repeat[k] = 0;
                if raw_pressed & bit != 0 && self.held & opposite == 0 {
                    // A tap shorter than a tick.
                    self.pressed |= bit;
                }
            }
        }
    }

    /// Any device pressed (directions with repeat).
    pub fn p(&self, bit: u16) -> bool {
        self.pressed & bit != 0
    }

    /// Any device holds.
    pub fn h(&self, bit: u16) -> bool {
        self.held & bit != 0
    }

    pub fn dev(&self, d: Dev) -> Option<&DevState> {
        self.devs.iter().find(|s| s.dev == d)
    }

    /// The first device (whose icons menus draw): the first pad, else the
    /// first keyboard.
    pub fn first(&self) -> Option<&DevState> {
        self.devs
            .iter()
            .find(|d| matches!(d.dev, Dev::Pad(_)))
            .or_else(|| self.devs.first())
    }

    /// Clears repeat state (scene changes).
    pub fn clear(&mut self) {
        self.repeat = [0; 4];
        self.lockout = 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::InputHub;

    #[test]
    fn repeat_and_edges() {
        let mut hub = InputHub::new();
        let mut raw = RawInput::default();
        let keys = KeyboardSetup::default();
        let mut mi = MenuInput::default();
        hub.key(Scancode::Down as usize, true);
        let mut fired = Vec::new();
        for t in 0..50 {
            hub.snapshot(&mut raw);
            mi.update(&raw, &keys);
            if mi.p(b::DOWN) {
                fired.push(t);
            }
        }
        assert_eq!(fired, vec![0, 30, 36, 42, 48]);
        hub.key(Scancode::Down as usize, false);
        hub.key(Scancode::C as usize, true);
        hub.snapshot(&mut raw);
        mi.update(&raw, &keys);
        assert!(mi.p(b::CONFIRM) && !mi.p(b::DOWN));
        hub.snapshot(&mut raw);
        mi.update(&raw, &keys);
        assert!(!mi.p(b::CONFIRM) && mi.h(b::CONFIRM));
    }

    #[test]
    fn pads_and_sticks() {
        let mut hub = InputHub::new();
        let mut raw = RawInput::default();
        let keys = KeyboardSetup::default();
        let mut mi = MenuInput::default();
        hub.connect(7, "Xbox 360 Controller");
        hub.connect(9, "PS4 Controller");
        hub.snapshot(&mut raw);
        mi.update(&raw, &keys);
        hub.pad_axis(9, Axis::LeftX as usize, -30000);
        hub.pad_button(7, Button::B as usize, true);
        hub.snapshot(&mut raw);
        mi.update(&raw, &keys);
        let p9 = mi.dev(Dev::Pad(9)).unwrap();
        assert!(p9.p(b::LEFT) && p9.name == "DUALSHOCK 4");
        assert!(mi.dev(Dev::Pad(7)).unwrap().p(b::BACK));
        assert!(mi.p(b::LEFT) && mi.p(b::BACK));
        assert_eq!(mi.first().unwrap().dev, Dev::Pad(7));
        assert_eq!(key_icon(Scancode::C), Some("C"));
        assert_eq!(key_icon(Scancode::Num0), Some("D0"));
        assert_eq!(key_icon(Scancode::F5), None);
    }
}
