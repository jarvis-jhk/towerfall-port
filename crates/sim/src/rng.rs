//! The legacy .NET `System.Random` subtractive generator (engine.md §5.3),
//! plus the `Calc` extension methods gameplay uses.
//!
//! The state is a plain array, so cloning the world copies it.

use crate::math::{PI, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rng {
    seed_array: [i32; 56],
    inext: u8,
    inextp: u8,
}

const MBIG: i32 = i32::MAX;
const MSEED: i32 = 161_803_398;

impl Rng {
    /// `new Random(seed)`.
    pub fn new(seed: i32) -> Rng {
        let mut s = [0i32; 56];
        let sub = if seed == i32::MIN {
            i32::MAX
        } else {
            seed.abs()
        };
        let mut mj = MSEED - sub;
        s[55] = mj;
        let mut mk: i32 = 1;
        let mut ii = 0usize;
        for _ in 1..55 {
            ii = (ii + 21) % 55;
            s[ii] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 {
                mk = mk.wrapping_add(MBIG);
            }
            mj = s[ii];
        }
        for _ in 0..4 {
            for k in 1..56 {
                let j = (k + 30) % 55;
                s[k] = s[k].wrapping_sub(s[1 + j]);
                if s[k] < 0 {
                    s[k] = s[k].wrapping_add(MBIG);
                }
            }
        }
        Rng {
            seed_array: s,
            inext: 0,
            inextp: 21,
        }
    }

    fn internal_sample(&mut self) -> i32 {
        let mut a = self.inext as usize + 1;
        if a >= 56 {
            a = 1;
        }
        let mut b = self.inextp as usize + 1;
        if b >= 56 {
            b = 1;
        }
        let mut v = self.seed_array[a].wrapping_sub(self.seed_array[b]);
        if v == MBIG {
            v -= 1;
        }
        if v < 0 {
            v = v.wrapping_add(MBIG);
        }
        self.seed_array[a] = v;
        self.inext = a as u8;
        self.inextp = b as u8;
        v
    }

    /// `Next()`: [0, 2^31 - 2].
    pub fn next_i32(&mut self) -> i32 {
        self.internal_sample()
    }

    /// `NextDouble()` / `Sample()`.
    pub fn next_double(&mut self) -> f64 {
        self.internal_sample() as f64 * 4.656612875245797E-10
    }

    /// `Next(max)`: [0, max).
    pub fn next_max(&mut self, max: i32) -> i32 {
        (self.next_double() * max as f64) as i32
    }

    /// `Next(min, max)`: [min, max).
    pub fn next_range(&mut self, min: i32, max: i32) -> i32 {
        (self.next_double() * (max as i64 - min as i64) as f64) as i32 + min
    }

    /// `Calc.NextFloat()`: the double sample rounded to f32.
    pub fn next_float(&mut self) -> f32 {
        self.next_double() as f32
    }

    /// `Calc.NextFloat(max)`.
    pub fn next_float_max(&mut self, max: f32) -> f32 {
        self.next_float() * max
    }

    /// `Calc.NextAngle()`.
    pub fn next_angle(&mut self) -> f32 {
        self.next_float() * (PI * 2.0)
    }

    /// `Calc.Range(int min, int add)`: `min + Next(add)`.
    pub fn range_i(&mut self, min: i32, add: i32) -> i32 {
        min + self.next_max(add)
    }

    /// `Calc.Range(float min, float add)`: `min + add * NextFloat()`.
    pub fn range_f(&mut self, min: f32, add: f32) -> f32 {
        min + add * self.next_float()
    }

    /// `Calc.Range(Vector2 min, Vector2 add)` (x first).
    pub fn range_v(&mut self, min: Vec2, add: Vec2) -> Vec2 {
        let x = min.x + add.x * self.next_float();
        let y = min.y + add.y * self.next_float();
        Vec2::new(x, y)
    }

    /// `Calc.Chance(p)`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.next_float() < p
    }

    /// `Calc.Choose` index.
    pub fn choose_index(&mut self, count: usize) -> usize {
        self.next_max(count as i32) as usize
    }

    /// `Calc.Shuffle` (Fisher-Yates from the end).
    pub fn shuffle<T>(&mut self, list: &mut [T]) {
        let mut i = list.len();
        while i > 1 {
            i -= 1;
            let j = self.next_max(i as i32 + 1) as usize;
            list.swap(i, j);
        }
    }

    pub(crate) fn hash_into(&self, h: &mut crate::checksum::Hasher) {
        for v in self.seed_array {
            h.i32(v);
        }
        h.u8(self.inext);
        h.u8(self.inextp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_seed_0() {
        let mut r = Rng::new(0);
        let v: Vec<i32> = (0..5).map(|_| r.next_i32()).collect();
        assert_eq!(
            v,
            [1559595546, 1755192844, 1649316166, 1198642031, 442452829]
        );
        let mut r = Rng::new(0);
        assert_eq!(r.next_double(), 0.7262432699679598);
        assert_eq!(r.next_double(), 0.8173253595909687);
        let mut r = Rng::new(0);
        let v: Vec<i32> = (0..8).map(|_| r.next_max(100)).collect();
        assert_eq!(v, [72, 81, 76, 55, 20, 55, 90, 44]);
    }

    #[test]
    fn vectors_seed_1_and_12345() {
        let mut r = Rng::new(1);
        let v: Vec<i32> = (0..5).map(|_| r.next_i32()).collect();
        assert_eq!(
            v,
            [534011718, 237820880, 1002897798, 1657007234, 1412011072]
        );
        let mut r = Rng::new(1);
        let v: Vec<i32> = (0..8).map(|_| r.next_max(100)).collect();
        assert_eq!(v, [24, 11, 46, 77, 65, 43, 35, 94]);
        let mut r = Rng::new(12345);
        let v: Vec<i32> = (0..5).map(|_| r.next_i32()).collect();
        assert_eq!(
            v,
            [143337951, 150666398, 1663795458, 1097663221, 1712597933]
        );
        let mut r = Rng::new(12345);
        let v: Vec<i32> = (0..8).map(|_| r.next_max(100)).collect();
        assert_eq!(v, [6, 7, 77, 51, 79, 82, 16, 73]);
    }

    #[test]
    fn min_seed_does_not_panic() {
        let mut r = Rng::new(i32::MIN);
        r.next_i32();
    }
}
