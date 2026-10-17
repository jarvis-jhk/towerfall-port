//! A small, fast, platform-independent hasher for desync detection.
//!
//! Two independent FxHash-style lanes give a 128-bit value (tfp-net compares
//! `u128`); [`Hasher::finish`] folds them into the `u64` the sim exposes.
//! Floats are hashed by their bit pattern.

#[derive(Clone, Debug)]
pub struct Hasher {
    a: u64,
    b: u64,
}

impl Default for Hasher {
    fn default() -> Self {
        Hasher::new()
    }
}

impl Hasher {
    pub fn new() -> Hasher {
        Hasher {
            a: 0x243f_6a88_85a3_08d3,
            b: 0x1319_8a2e_0370_7344,
        }
    }

    #[inline]
    pub fn u64(&mut self, v: u64) {
        self.a = (self.a.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
        self.b = (self.b.rotate_left(7) ^ v).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (self.b >> 29);
    }
    #[inline]
    pub fn u32(&mut self, v: u32) {
        self.u64(v as u64)
    }
    #[inline]
    pub fn i32(&mut self, v: i32) {
        self.u64(v as u32 as u64)
    }
    #[inline]
    pub fn u8(&mut self, v: u8) {
        self.u64(v as u64)
    }
    #[inline]
    pub fn bool(&mut self, v: bool) {
        self.u64(v as u64)
    }
    #[inline]
    pub fn f32(&mut self, v: f32) {
        self.u64(v.to_bits() as u64)
    }
    #[inline]
    pub fn f64(&mut self, v: f64) {
        self.u64(v.to_bits())
    }
    #[inline]
    pub fn vec2(&mut self, v: crate::math::Vec2) {
        self.f32(v.x);
        self.f32(v.y);
    }

    pub fn finish(&self) -> u64 {
        self.a ^ self.b.rotate_left(32)
    }

    pub fn finish128(&self) -> u128 {
        ((self.a as u128) << 64) | self.b as u128
    }
}

/// Types that feed gameplay-relevant state into a [`Hasher`].
pub trait StableHash {
    fn stable_hash(&self, h: &mut Hasher);
}

macro_rules! prim {
    ($($t:ty => $m:ident),*) => {$(
        impl StableHash for $t {
            #[inline]
            fn stable_hash(&self, h: &mut Hasher) { h.$m(*self as _) }
        }
    )*};
}
prim!(u8 => u8, u32 => u32, i32 => i32, u64 => u64, bool => bool, f32 => f32, f64 => f64);

impl StableHash for i8 {
    fn stable_hash(&self, h: &mut Hasher) {
        h.i32(*self as i32)
    }
}
impl StableHash for u16 {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u32(*self as u32)
    }
}
impl StableHash for i16 {
    fn stable_hash(&self, h: &mut Hasher) {
        h.i32(*self as i32)
    }
}
impl StableHash for crate::math::Vec2 {
    fn stable_hash(&self, h: &mut Hasher) {
        h.vec2(*self)
    }
}
impl<T: StableHash> StableHash for Option<T> {
    fn stable_hash(&self, h: &mut Hasher) {
        match self {
            None => h.u8(0),
            Some(v) => {
                h.u8(1);
                v.stable_hash(h);
            }
        }
    }
}
impl<T: StableHash> StableHash for [T] {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u32(self.len() as u32);
        for v in self {
            v.stable_hash(h);
        }
    }
}
impl<T: StableHash, const N: usize> StableHash for [T; N] {
    fn stable_hash(&self, h: &mut Hasher) {
        for v in self {
            v.stable_hash(h);
        }
    }
}
impl<T: StableHash> StableHash for Vec<T> {
    fn stable_hash(&self, h: &mut Hasher) {
        self.as_slice().stable_hash(h)
    }
}

/// `impl_hash!(Type { a, b, c })` hashes the listed fields in order.
#[macro_export]
macro_rules! impl_hash {
    ($t:ty { $($f:ident),* $(,)? }) => {
        impl $crate::checksum::StableHash for $t {
            fn stable_hash(&self, h: &mut $crate::checksum::Hasher) {
                $( $crate::checksum::StableHash::stable_hash(&self.$f, h); )*
            }
        }
    };
}

/// Hash a fieldless enum by its discriminant.
#[macro_export]
macro_rules! impl_hash_enum {
    ($($t:ty),*) => {$(
        impl $crate::checksum::StableHash for $t {
            fn stable_hash(&self, h: &mut $crate::checksum::Hasher) { h.u32(*self as u32) }
        }
    )*};
}
