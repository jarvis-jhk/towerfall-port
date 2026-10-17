//! Per-player inputs on the wire.
//!
//! GGRS wants `Copy + PartialEq + Default + Serialize + DeserializeOwned`
//! inputs. The game supplies a plain `bytemuck::Pod` struct instead; [`Wire`]
//! adapts it: the bytes of the struct are the wire format (a fixed number of
//! bytes, no length prefix), `Default` is all zeroes and equality is byte
//! equality. All-zero therefore means "no input", which is also what a
//! dropped player gets.
//!
//! [`TfInput`] is TowerFall's input: 4 bytes per player per frame.

use bytemuck::{Pod, Zeroable};
use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::marker::PhantomData;

/// The inputs of one player for one frame, as the network sees them.
///
/// Implemented for every `Pod` type. Keep it small (GGRS sends the last few
/// frames of every local player in each packet) and state-based (buttons held,
/// not pressed: the sim derives presses by comparing with the previous
/// frame): the predictor repeats the last input.
pub trait NetInput: Pod + Send + 'static {}
impl<T: Pod + Send + 'static> NetInput for T {}

/// One TowerFall player's controls for one frame.
///
/// `aim_x`/`aim_y` are the movement/aim direction (stick or d-pad/keys),
/// quantised to multiples of [`TfInput::AXIS_STEP`] in
/// `-AXIS_MAX..=AXIS_MAX`; +x = right, +y = **down** (screen space).
/// Quantising keeps mispredictions (and rollbacks) rare: a stick that wobbles
/// by a few hundredths does not change the input. Buttons are held states,
/// see the `BTN_*` constants.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Pod, Zeroable)]
pub struct TfInput {
    pub aim_x: i8,
    pub aim_y: i8,
    pub buttons: u8,
    /// Always 0 (room for more buttons without changing the size).
    pub reserved: u8,
}

impl TfInput {
    pub const BTN_JUMP: u8 = 1 << 0;
    pub const BTN_SHOOT: u8 = 1 << 1;
    pub const BTN_DODGE: u8 = 1 << 2;
    /// Alternate shoot (Dark World trigger arrows / arrow toggle).
    pub const BTN_ALT: u8 = 1 << 3;
    /// Start / pause.
    pub const BTN_START: u8 = 1 << 4;

    /// Full deflection.
    pub const AXIS_MAX: i8 = 120;
    /// Quantisation step: 15 levels per side.
    pub const AXIS_STEP: i8 = 8;

    pub fn held(&self, btn: u8) -> bool {
        self.buttons & btn != 0
    }

    /// Quantise one analog axis (`-1.0..=1.0`, values outside are clamped;
    /// `deadzone` is the radius treated as 0).
    pub fn quantise_axis(v: f32, deadzone: f32) -> i8 {
        if !v.is_finite() || v.abs() <= deadzone {
            return 0;
        }
        let v = v.clamp(-1.0, 1.0);
        // Rescale so the first level starts right after the dead zone.
        let mag = ((v.abs() - deadzone) / (1.0 - deadzone).max(1e-6)).clamp(0.0, 1.0);
        let levels = (Self::AXIS_MAX / Self::AXIS_STEP) as f32;
        let q = ((mag * levels).round() as i8).max(1) * Self::AXIS_STEP;
        if v < 0.0 { -q } else { q }
    }

    /// From an analog stick with a radial dead zone (TowerFall aims with the
    /// stick angle, so the direction must survive quantisation).
    pub fn from_stick(x: f32, y: f32, deadzone: f32, buttons: u8) -> TfInput {
        let len = (x * x + y * y).sqrt();
        let (aim_x, aim_y) = if !len.is_finite() || len <= deadzone {
            (0, 0)
        } else {
            let scale = if len > 1.0 { 1.0 / len } else { 1.0 };
            (
                Self::quantise_axis(x * scale, 0.0),
                Self::quantise_axis(y * scale, 0.0),
            )
        };
        TfInput {
            aim_x,
            aim_y,
            buttons,
            reserved: 0,
        }
    }

    /// From a d-pad or keys: 8 directions at full deflection.
    pub fn from_dpad(left: bool, right: bool, up: bool, down: bool, buttons: u8) -> TfInput {
        let axis = |neg: bool, pos: bool| match (neg, pos) {
            (true, false) => -Self::AXIS_MAX,
            (false, true) => Self::AXIS_MAX,
            _ => 0,
        };
        TfInput {
            aim_x: axis(left, right),
            aim_y: axis(up, down),
            buttons,
            reserved: 0,
        }
    }
}

/// GGRS input wrapper around a `Pod` input.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Wire<I: NetInput>(pub I);

impl<I: NetInput> Default for Wire<I> {
    fn default() -> Self {
        Wire(I::zeroed())
    }
}

impl<I: NetInput> PartialEq for Wire<I> {
    fn eq(&self, other: &Self) -> bool {
        bytemuck::bytes_of(&self.0) == bytemuck::bytes_of(&other.0)
    }
}

impl<I: NetInput> fmt::Debug for Wire<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Wire(")?;
        for b in bytemuck::bytes_of(&self.0) {
            write!(f, "{b:02x}")?;
        }
        write!(f, ")")
    }
}

impl<I: NetInput> Serialize for Wire<I> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let bytes = bytemuck::bytes_of(&self.0);
        let mut t = s.serialize_tuple(bytes.len())?;
        for b in bytes {
            t.serialize_element(b)?;
        }
        t.end()
    }
}

impl<'de, I: NetInput> Deserialize<'de> for Wire<I> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<I>(PhantomData<I>);
        impl<'de, I: NetInput> Visitor<'de> for V<I> {
            type Value = Wire<I>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{} input bytes", std::mem::size_of::<I>())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Wire<I>, A::Error> {
                let mut v = I::zeroed();
                let bytes = bytemuck::bytes_of_mut(&mut v);
                for (i, b) in bytes.iter_mut().enumerate() {
                    *b = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(i, &self))?;
                }
                Ok(Wire(v))
            }
        }
        d.deserialize_tuple(std::mem::size_of::<I>(), V(PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_size_roundtrip() {
        let i = Wire(TfInput {
            aim_x: -8,
            aim_y: 120,
            buttons: TfInput::BTN_JUMP | TfInput::BTN_DODGE,
            reserved: 0,
        });
        let b = bincode::serialize(&i).unwrap();
        assert_eq!(b.len(), 4, "no length prefix");
        let back: Wire<TfInput> = bincode::deserialize(&b).unwrap();
        assert_eq!(back, i);
        assert_eq!(
            bincode::serialized_size(&Wire::<TfInput>::default()).unwrap(),
            4
        );
        assert!(bincode::deserialize::<Wire<TfInput>>(&b[..3]).is_err());
    }

    #[test]
    fn quantisation() {
        assert_eq!(TfInput::quantise_axis(0.1, 0.2), 0);
        assert_eq!(TfInput::quantise_axis(1.0, 0.2), 120);
        assert_eq!(TfInput::quantise_axis(-3.0, 0.2), -120);
        assert_eq!(
            TfInput::quantise_axis(0.21, 0.2),
            8,
            "just outside the dead zone"
        );
        assert_eq!(TfInput::quantise_axis(f32::NAN, 0.2), 0);
        for k in 0..=100 {
            let q = TfInput::quantise_axis(k as f32 / 100.0, 0.15);
            assert_eq!(q % TfInput::AXIS_STEP, 0);
            assert!((0..=TfInput::AXIS_MAX).contains(&q));
        }
        // Small wobble does not change the input.
        assert_eq!(
            TfInput::from_stick(0.60, 0.60, 0.0, 0),
            TfInput::from_stick(0.605, 0.598, 0.0, 0)
        );
        let d = TfInput::from_stick(5.0, 0.0, 0.2, TfInput::BTN_SHOOT);
        assert_eq!((d.aim_x, d.aim_y), (120, 0));
        assert!(d.held(TfInput::BTN_SHOOT));
        let p = TfInput::from_dpad(true, false, false, true, 0);
        assert_eq!((p.aim_x, p.aim_y), (-120, 120));
        assert_eq!(TfInput::from_dpad(true, true, false, false, 0).aim_x, 0);
    }
}
