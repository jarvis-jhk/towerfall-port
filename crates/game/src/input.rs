//! Input devices: the keyboard and any number of game controllers
//! (hot-plugged), sampled once per simulation tick into a [`RawInput`]
//! snapshot: per device, which buttons/keys are held, which went down or
//! up since the previous tick (taps shorter than a tick are latched), and
//! the analogue axes.
//!
//! [`InputHub`] holds the state and is fed by plain method calls, so it is
//! independent of SDL and unit-tested; [`SdlInput`] owns the SDL
//! controller handles and translates SDL events into hub calls. Mapping
//! raw input to player actions (the original's bindings) happens later, in
//! the game; [`PadState::direction`] and friends are conveniences.

use std::collections::BTreeMap;

use sdl2::controller::{Axis, Button, GameController};
use sdl2::event::{Event, WindowEvent};

/// Number of SDL game-controller buttons (A .. Touchpad).
pub const PAD_BUTTONS: usize = 21;
/// Number of SDL game-controller axes.
pub const PAD_AXES: usize = 6;
/// Keyboard scancodes tracked (SDL scancodes are < 512).
pub const SCANCODES: usize = 512;

/// A set of controller buttons (bit = SDL button index).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PadButtons(pub u32);

impl PadButtons {
    pub fn contains(self, b: Button) -> bool {
        self.0 & (1 << b as u32) != 0
    }

    fn set(&mut self, index: usize, on: bool) {
        if on {
            self.0 |= 1 << index;
        } else {
            self.0 &= !(1 << index);
        }
    }

    #[allow(dead_code)]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// A set of keyboard scancodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeySet(pub [u64; SCANCODES / 64]);

impl Default for KeySet {
    fn default() -> Self {
        Self([0; SCANCODES / 64])
    }
}

impl KeySet {
    pub fn contains_code(&self, code: usize) -> bool {
        code < SCANCODES && self.0[code / 64] & (1 << (code % 64)) != 0
    }

    pub fn contains(&self, sc: sdl2::keyboard::Scancode) -> bool {
        self.contains_code(sc as usize)
    }

    fn set(&mut self, code: usize, on: bool) {
        if code >= SCANCODES {
            return;
        }
        let bit = 1u64 << (code % 64);
        if on {
            self.0[code / 64] |= bit;
        } else {
            self.0[code / 64] &= !bit;
        }
    }

    fn and_not(&self, other: &KeySet) -> KeySet {
        let mut out = *self;
        for (o, b) in out.0.iter_mut().zip(other.0) {
            *o &= !b;
        }
        out
    }

    fn or(&self, other: &KeySet) -> KeySet {
        let mut out = *self;
        for (o, b) in out.0.iter_mut().zip(other.0) {
            *o |= b;
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|&w| w == 0)
    }

    /// Scancodes in the set, ascending.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        (0..SCANCODES).filter(|&c| self.contains_code(c))
    }
}

/// Keyboard state for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyboardState {
    pub held: KeySet,
    pub pressed: KeySet,
    pub released: KeySet,
}

/// One controller's state for one tick.
#[derive(Clone, Debug, PartialEq)]
pub struct PadState {
    /// SDL joystick instance id (stable while plugged in; never reused).
    pub id: u32,
    /// Connection order (0 = connected first); stays with the device.
    pub order: u64,
    pub name: String,
    pub held: PadButtons,
    pub pressed: PadButtons,
    pub released: PadButtons,
    /// LeftX, LeftY, RightX, RightY (-1..1, y down like SDL), TriggerLeft,
    /// TriggerRight (0..1). Raw: no dead zone.
    pub axes: [f32; PAD_AXES],
}

impl PadState {
    pub fn axis(&self, a: Axis) -> f32 {
        self.axes[a as usize]
    }

    /// Left stick with a radial dead zone (rescaled to 0..1 outside it),
    /// or the d-pad when it is pressed. y down.
    pub fn direction(&self, dead_zone: f32) -> (f32, f32) {
        let b = |x| self.held.contains(x) as i32 as f32;
        let (dx, dy) = (
            b(Button::DPadRight) - b(Button::DPadLeft),
            b(Button::DPadDown) - b(Button::DPadUp),
        );
        if dx != 0.0 || dy != 0.0 {
            return (dx, dy);
        }
        deadzone(self.axis(Axis::LeftX), self.axis(Axis::LeftY), dead_zone)
    }
}

/// Radial dead zone with rescaling, clamped to the unit disk.
pub fn deadzone(x: f32, y: f32, dz: f32) -> (f32, f32) {
    let len = (x * x + y * y).sqrt();
    if len <= dz || len == 0.0 {
        return (0.0, 0.0);
    }
    let k = ((len - dz) / (1.0 - dz)).min(1.0) / len;
    (x * k, y * k)
}

/// Every local device's input for one tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawInput {
    pub keyboard: KeyboardState,
    /// Connected controllers in connection order.
    pub pads: Vec<PadState>,
    /// Controllers connected / disconnected since the previous tick
    /// (instance ids), for join screens.
    pub connected: Vec<u32>,
    pub disconnected: Vec<u32>,
    /// Files / folders dropped on the window since the previous tick.
    pub dropped: Vec<std::path::PathBuf>,
    /// The window lost focus since the previous tick (matches pause,
    /// ui.md 10.1).
    pub focus_lost: bool,
}

fn axis_value(index: usize, raw: i16) -> f32 {
    let v = (raw as f32 / 32767.0).clamp(-1.0, 1.0);
    if index >= 4 { v.max(0.0) } else { v }
}

#[derive(Clone, Debug)]
struct Pad {
    order: u64,
    name: String,
    held: PadButtons,
    latched_press: PadButtons,
    latched_release: PadButtons,
    last_held: PadButtons,
    axes: [f32; PAD_AXES],
}

/// Holds the state of every local device between ticks.
#[derive(Clone, Debug, Default)]
pub struct InputHub {
    pads: BTreeMap<u32, Pad>,
    next_order: u64,
    keys: KeySet,
    key_press: KeySet,
    key_release: KeySet,
    last_keys: KeySet,
    connected: Vec<u32>,
    disconnected: Vec<u32>,
    dropped: Vec<std::path::PathBuf>,
    focus_lost: bool,
}

impl InputHub {
    /// A file or folder was dropped on the window.
    pub fn drop_path(&mut self, p: std::path::PathBuf) {
        self.dropped.push(p);
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// A controller was plugged in (ignored if already known).
    pub fn connect(&mut self, id: u32, name: &str) {
        if self.pads.contains_key(&id) {
            return;
        }
        self.pads.insert(
            id,
            Pad {
                order: self.next_order,
                name: name.to_owned(),
                held: PadButtons::default(),
                latched_press: PadButtons::default(),
                latched_release: PadButtons::default(),
                last_held: PadButtons::default(),
                axes: [0.0; PAD_AXES],
            },
        );
        self.next_order += 1;
        self.connected.push(id);
    }

    pub fn disconnect(&mut self, id: u32) {
        if self.pads.remove(&id).is_some() {
            self.disconnected.push(id);
        }
    }

    pub fn pad_button(&mut self, id: u32, button: usize, down: bool) {
        let Some(p) = self.pads.get_mut(&id) else {
            return;
        };
        if button >= PAD_BUTTONS {
            return;
        }
        let was = p.held.0 & (1 << button) != 0;
        if down && !was {
            p.latched_press.set(button, true);
        }
        if !down && was {
            p.latched_release.set(button, true);
        }
        p.held.set(button, down);
    }

    pub fn pad_axis(&mut self, id: u32, axis: usize, raw: i16) {
        if let Some(p) = self.pads.get_mut(&id)
            && axis < PAD_AXES
        {
            p.axes[axis] = axis_value(axis, raw);
        }
    }

    pub fn key(&mut self, scancode: usize, down: bool) {
        let was = self.keys.contains_code(scancode);
        if down && !was {
            self.key_press.set(scancode, true);
        }
        if !down && was {
            self.key_release.set(scancode, true);
        }
        self.keys.set(scancode, down);
    }

    /// Focus lost: release every key (key-up events will not arrive).
    pub fn release_all_keys(&mut self) {
        self.focus_lost = true;
        for code in self.keys.iter().collect::<Vec<_>>() {
            self.key(code, false);
        }
    }

    #[allow(dead_code)]
    pub fn pad_count(&self) -> usize {
        self.pads.len()
    }

    /// Samples every device for one tick (consumes the edges).
    pub fn snapshot(&mut self, out: &mut RawInput) {
        let kb = &mut out.keyboard;
        kb.held = self.keys;
        kb.pressed = self.key_press.or(&self.keys.and_not(&self.last_keys));
        kb.released = self.key_release.or(&self.last_keys.and_not(&self.keys));
        self.key_press = KeySet::default();
        self.key_release = KeySet::default();
        self.last_keys = self.keys;

        out.pads.clear();
        for (&id, p) in &mut self.pads {
            let pressed = PadButtons(p.latched_press.0 | (p.held.0 & !p.last_held.0));
            let released = PadButtons(p.latched_release.0 | (!p.held.0 & p.last_held.0));
            p.latched_press = PadButtons::default();
            p.latched_release = PadButtons::default();
            p.last_held = p.held;
            out.pads.push(PadState {
                id,
                order: p.order,
                name: p.name.clone(),
                held: p.held,
                pressed,
                released,
                axes: p.axes,
            });
        }
        out.pads.sort_by_key(|p| p.order);
        out.connected = std::mem::take(&mut self.connected);
        out.disconnected = std::mem::take(&mut self.disconnected);
        out.dropped = std::mem::take(&mut self.dropped);
        out.focus_lost = std::mem::take(&mut self.focus_lost);
    }
}

/// Info about a connected controller (`--list-devices`, menus).
#[derive(Clone, Debug)]
pub struct ControllerInfo {
    pub id: u32,
    pub name: String,
    pub has_rumble: bool,
}

/// SDL front end: owns the opened controllers and feeds the hub.
pub struct SdlInput {
    subsystem: sdl2::GameControllerSubsystem,
    controllers: BTreeMap<u32, GameController>,
    pub hub: InputHub,
}

#[allow(dead_code)] // rumble/controllers: used by menus later (M4/M6).
impl SdlInput {
    pub fn new(subsystem: sdl2::GameControllerSubsystem) -> Self {
        // Optional extra mappings next to the executable or in the data
        // dir (SDL's built-in database covers common pads).
        let mut candidates = Vec::new();
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
        {
            candidates.push(dir.join("gamecontrollerdb.txt"));
        }
        // The original ships one next to its executable (read only).
        for var in ["TF_GAME_DIR", "TFP_DATA_DIR"] {
            if let Ok(d) = std::env::var(var) {
                candidates.push(std::path::PathBuf::from(d).join("gamecontrollerdb.txt"));
            }
        }
        for path in candidates.into_iter().filter(|p| p.exists()) {
            match subsystem.load_mappings(&path) {
                Ok(n) => log::info!("loaded {n} controller mappings from {}", path.display()),
                Err(e) => log::warn!("controller mappings {}: {e}", path.display()),
            }
        }
        Self {
            subsystem,
            controllers: BTreeMap::new(),
            hub: InputHub::new(),
        }
    }

    /// Feeds one SDL event. Returns true if it was an input event.
    pub fn handle_event(&mut self, event: &Event) -> bool {
        match event {
            Event::ControllerDeviceAdded { which, .. } => match self.subsystem.open(*which) {
                Ok(c) => {
                    let id = c.instance_id();
                    log::info!("controller connected: #{id} {}", c.name());
                    self.hub.connect(id, &c.name());
                    self.controllers.entry(id).or_insert(c);
                }
                Err(e) => log::warn!("cannot open controller {which}: {e}"),
            },
            Event::ControllerDeviceRemoved { which, .. } => {
                if let Some(c) = self.controllers.remove(which) {
                    log::info!("controller disconnected: #{which} {}", c.name());
                }
                self.hub.disconnect(*which);
            }
            Event::ControllerButtonDown { which, button, .. } => {
                self.hub.pad_button(*which, *button as usize, true)
            }
            Event::ControllerButtonUp { which, button, .. } => {
                self.hub.pad_button(*which, *button as usize, false)
            }
            Event::ControllerAxisMotion {
                which, axis, value, ..
            } => self.hub.pad_axis(*which, *axis as usize, *value),
            Event::KeyDown {
                scancode: Some(sc),
                repeat: false,
                ..
            } => self.hub.key(*sc as usize, true),
            Event::KeyUp {
                scancode: Some(sc), ..
            } => self.hub.key(*sc as usize, false),
            Event::Window {
                win_event: WindowEvent::FocusLost,
                ..
            } => self.hub.release_all_keys(),
            Event::DropFile { filename, .. } => self.hub.drop_path(filename.into()),
            _ => return false,
        }
        true
    }

    pub fn snapshot(&mut self, out: &mut RawInput) {
        self.hub.snapshot(out);
    }

    pub fn controllers(&self) -> Vec<ControllerInfo> {
        self.controllers
            .iter()
            .map(|(&id, c)| ControllerInfo {
                id,
                name: c.name(),
                has_rumble: c.has_rumble(),
            })
            .collect()
    }

    /// Rumble (0..1 strengths); ignored when unsupported.
    pub fn rumble(&mut self, id: u32, low: f32, high: f32, ms: u32) {
        if let Some(c) = self.controllers.get_mut(&id) {
            let q = |x: f32| (x.clamp(0.0, 1.0) * 65535.0) as u16;
            let _ = c.set_rumble(q(low), q(high), ms);
        }
    }
}

/// SDL virtual game controllers (no hardware) for headless tests of the
/// hot-plug path and of many-player input: they produce real SDL device
/// and button events.
pub struct VirtualPads {
    /// (device index at attach time, opened joystick for setting state)
    pads: Vec<*mut sdl2::sys::SDL_Joystick>,
}

impl VirtualPads {
    /// Attaches `n` virtual controllers (SDL needs the joystick subsystem).
    pub fn attach(n: usize) -> anyhow::Result<Self> {
        use sdl2::sys;
        let mut pads = Vec::new();
        for _ in 0..n {
            // SAFETY: plain SDL calls; the joystick subsystem is initialised
            // by the caller (game controller subsystem).
            unsafe {
                let index = sys::SDL_JoystickAttachVirtual(
                    sys::SDL_JoystickType::SDL_JOYSTICK_TYPE_GAMECONTROLLER,
                    PAD_AXES as i32,
                    PAD_BUTTONS as i32,
                    0,
                );
                if index < 0 {
                    anyhow::bail!("SDL_JoystickAttachVirtual: {}", sdl2::get_error());
                }
                let joy = sys::SDL_JoystickOpen(index);
                if joy.is_null() {
                    anyhow::bail!("SDL_JoystickOpen(virtual): {}", sdl2::get_error());
                }
                pads.push(joy);
            }
        }
        Ok(Self { pads })
    }

    pub fn len(&self) -> usize {
        self.pads.len()
    }

    /// Sets a button of virtual pad `i` (events follow on the next pump).
    pub fn set_button(&self, i: usize, button: Button, down: bool) {
        if let Some(&j) = self.pads.get(i) {
            // SAFETY: `j` is an open virtual joystick.
            unsafe {
                sdl2::sys::SDL_JoystickSetVirtualButton(j, button as i32, down as u8);
            }
        }
    }

    pub fn set_axis(&self, i: usize, axis: Axis, value: i16) {
        if let Some(&j) = self.pads.get(i) {
            // SAFETY: `j` is an open virtual joystick.
            unsafe {
                sdl2::sys::SDL_JoystickSetVirtualAxis(j, axis as i32, value);
            }
        }
    }

    /// Unplugs virtual pad `i` (a device-removed event follows).
    pub fn detach(&mut self, i: usize) {
        if i < self.pads.len() {
            let j = self.pads.remove(i);
            // SAFETY: `j` is an open virtual joystick; detach by its
            // instance id via the current device index.
            unsafe {
                let id = sdl2::sys::SDL_JoystickInstanceID(j);
                sdl2::sys::SDL_JoystickClose(j);
                let n = sdl2::sys::SDL_NumJoysticks();
                for d in 0..n {
                    if sdl2::sys::SDL_JoystickGetDeviceInstanceID(d) == id {
                        sdl2::sys::SDL_JoystickDetachVirtual(d);
                        break;
                    }
                }
            }
        }
    }
}

impl Drop for VirtualPads {
    fn drop(&mut self) {
        while !self.pads.is_empty() {
            self.detach(self.pads.len() - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdl2::keyboard::Scancode;

    #[test]
    fn deadzone_rescales() {
        assert_eq!(deadzone(0.1, 0.1, 0.2), (0.0, 0.0));
        let (x, _) = deadzone(0.6, 0.0, 0.2);
        assert!((x - 0.5).abs() < 1e-6);
        let (x, y) = deadzone(1.0, 1.0, 0.2);
        assert!(((x * x + y * y).sqrt() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sixteen_pads_hotplug_and_order() {
        let mut hub = InputHub::new();
        let mut snap = RawInput::default();
        // 16 controllers with non-monotonic instance ids.
        let ids: Vec<u32> = (0..16).map(|i| 100 - i * 3).collect();
        for (n, &id) in ids.iter().enumerate() {
            hub.connect(id, &format!("pad {n}"));
        }
        hub.connect(ids[0], "duplicate add is ignored");
        assert_eq!(hub.pad_count(), 16);
        for (n, &id) in ids.iter().enumerate() {
            hub.pad_button(id, Button::A as usize, n % 2 == 0);
            hub.pad_axis(id, Axis::LeftX as usize, if n < 8 { 32767 } else { -32768 });
            hub.pad_axis(id, Axis::TriggerRight as usize, -32768);
        }
        hub.snapshot(&mut snap);
        assert_eq!(snap.pads.len(), 16);
        assert_eq!(snap.connected, ids);
        // Connection order, not id order.
        assert!(snap.pads.iter().map(|p| p.id).eq(ids.iter().copied()));
        for (n, p) in snap.pads.iter().enumerate() {
            assert_eq!(p.held.contains(Button::A), n % 2 == 0, "pad {n}");
            assert_eq!(p.pressed.contains(Button::A), n % 2 == 0);
            assert_eq!(p.axis(Axis::LeftX), if n < 8 { 1.0 } else { -1.0 });
            assert_eq!(p.axis(Axis::TriggerRight), 0.0, "triggers are 0..1");
        }
        // Unplug pad 3, plug a new one: it goes last, others keep order.
        hub.disconnect(ids[3]);
        hub.connect(7, "late");
        hub.snapshot(&mut snap);
        assert_eq!(snap.pads.len(), 16);
        assert_eq!(snap.disconnected, vec![ids[3]]);
        assert_eq!(snap.connected, vec![7]);
        assert_eq!(snap.pads.last().unwrap().id, 7);
        assert_eq!(snap.pads[3].id, ids[4]);
        assert!(
            snap.pads.iter().all(|p| p.pressed.is_empty()),
            "held is not re-pressed"
        );
        // Events for unknown pads are ignored.
        hub.pad_button(9999, 0, true);
        hub.pad_axis(9999, 0, 5);
    }

    #[test]
    fn taps_are_latched() {
        let mut hub = InputHub::new();
        let mut snap = RawInput::default();
        hub.connect(1, "pad");
        hub.pad_button(1, Button::X as usize, true);
        hub.pad_button(1, Button::X as usize, false);
        let k = Scancode::Space as usize;
        hub.key(k, true);
        hub.key(k, false);
        hub.snapshot(&mut snap);
        let p = &snap.pads[0];
        assert!(p.pressed.contains(Button::X) && p.released.contains(Button::X));
        assert!(!p.held.contains(Button::X));
        assert!(snap.keyboard.pressed.contains(Scancode::Space));
        assert!(snap.keyboard.released.contains(Scancode::Space));
        hub.snapshot(&mut snap);
        assert!(snap.pads[0].pressed.is_empty() && snap.keyboard.pressed.is_empty());
        // Focus loss releases held keys.
        hub.key(k, true);
        hub.snapshot(&mut snap);
        hub.release_all_keys();
        hub.snapshot(&mut snap);
        assert!(snap.keyboard.released.contains(Scancode::Space));
        assert!(snap.keyboard.held.is_empty());
    }

    #[test]
    fn dpad_overrides_stick() {
        let mut hub = InputHub::new();
        let mut snap = RawInput::default();
        hub.connect(1, "pad");
        hub.pad_axis(1, 0, 32767);
        hub.snapshot(&mut snap);
        assert_eq!(snap.pads[0].direction(0.2), (1.0, 0.0));
        hub.pad_button(1, Button::DPadUp as usize, true);
        hub.snapshot(&mut snap);
        assert_eq!(snap.pads[0].direction(0.2), (0.0, -1.0));
    }
}
