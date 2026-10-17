//! A bit-exact replica of the seeded `System.Random` of .NET / Mono (Knuth's
//! subtractive generator, 55-entry lagged table, lags 55 and 34 → index
//! offsets 1 and 22 in the 56-slot table). The original seeds one per level
//! load (`LoadSeed`) and uses it for autotile picks, so matching it makes our
//! tiles identical to the game's.
//!
//! Only the seeded constructor is meaningful; there is no clock seeding.

const MBIG: i32 = i32::MAX;
const MSEED: i32 = 161_803_398;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DotNetRandom {
    table: [i32; 56],
    inext: usize,
    inextp: usize,
}

impl DotNetRandom {
    pub fn new(seed: i32) -> Self {
        let mut table = [0i32; 56];
        let subtraction = if seed == i32::MIN {
            i32::MAX
        } else {
            seed.abs()
        };
        let mut mj = MSEED - subtraction;
        table[55] = mj;
        let mut mk = 1i32;
        for i in 1..55 {
            let ii = (21 * i) % 55;
            table[ii] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 {
                mk = mk.wrapping_add(MBIG);
            }
            mj = table[ii];
        }
        for _ in 1..5 {
            for i in 1..56 {
                let v = table[i].wrapping_sub(table[1 + (i + 30) % 55]);
                table[i] = if v < 0 { v.wrapping_add(MBIG) } else { v };
            }
        }
        DotNetRandom {
            table,
            inext: 0,
            inextp: 21,
        }
    }

    fn internal_sample(&mut self) -> i32 {
        let mut a = self.inext + 1;
        if a >= 56 {
            a = 1;
        }
        let mut b = self.inextp + 1;
        if b >= 56 {
            b = 1;
        }
        let mut v = self.table[a].wrapping_sub(self.table[b]);
        if v == MBIG {
            v -= 1;
        }
        if v < 0 {
            v = v.wrapping_add(MBIG);
        }
        self.table[a] = v;
        self.inext = a;
        self.inextp = b;
        v
    }

    /// `NextDouble()`: [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        self.internal_sample() as f64 * (1.0 / MBIG as f64)
    }

    /// `Next()`: [0, i32::MAX).
    pub fn next_i32(&mut self) -> i32 {
        self.internal_sample()
    }

    /// `Next(max)`: [0, max).
    pub fn next_max(&mut self, max: i32) -> i32 {
        (self.next_f64() * max as f64) as i32
    }

    /// `Next(min, max)`: [min, max).
    pub fn next_range(&mut self, min: i32, max: i32) -> i32 {
        let range = max as i64 - min as i64;
        if range <= i32::MAX as i64 {
            return (self.next_f64() * range as f64) as i32 + min;
        }
        // Large range: two samples combined, as .NET does.
        let mut r = self.internal_sample();
        if self.internal_sample() % 2 == 0 {
            r = -r;
        }
        let d = (r as f64 + (i32::MAX as f64 - 1.0)) / (2.0 * i32::MAX as f64 - 1.0);
        (d * range as f64) as i64 as i32 + min
    }

    /// The `(float)NextDouble()` the game uses as `NextFloat`.
    pub fn next_f32(&mut self) -> f32 {
        self.next_f64() as f32
    }

    // ---- the engine's `Calc` helpers on top of the generator ----

    /// `Chance(float p)`: `NextFloat() < p`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }

    /// `Chance(double p)`: the float sample widened, compared in f64.
    pub fn chance_f64(&mut self, p: f64) -> bool {
        (self.next_f32() as f64) < p
    }

    /// `Choose(a, b, …)`: `list[Next(len)]`.
    pub fn choose<T: Copy>(&mut self, list: &[T]) -> T {
        list[self.next_max(list.len() as i32) as usize]
    }

    /// `Range(int min, int add)`: `min + Next(add)` (`add` is a count).
    pub fn range_i32(&mut self, min: i32, add: i32) -> i32 {
        min + self.next_max(add)
    }

    /// `Range(float min, float add)`: `min + add * NextFloat()`.
    pub fn range_f32(&mut self, min: f32, add: f32) -> f32 {
        min + add * self.next_f32()
    }

    /// `NextAngle()`: `NextFloat() * 2π` (f32).
    pub fn next_angle(&mut self) -> f32 {
        self.next_f32() * (std::f32::consts::PI * 2.0)
    }

    /// `List.Shuffle(random)`: Fisher–Yates from the end, `j = Next(i + 1)`.
    pub fn shuffle<T>(&mut self, list: &mut [T]) {
        let mut i = list.len();
        while i > 1 {
            i -= 1;
            let j = self.next_max(i as i32 + 1) as usize;
            list.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_sequences() {
        // Reference values printed by .NET 8 for seeded instances.
        type Case = (i32, [i32; 8], i32, i32, i32, f64, i32);
        let cases: [Case; 5] = [
            (
                0,
                [72, 81, 76, 55, 20, 55, 90, 44],
                1559595546,
                1755192844,
                5,
                0.5581611914365372,
                -3,
            ),
            (
                42,
                [66, 14, 12, 52, 16, 26, 72, 51],
                1434747710,
                302596119,
                0,
                0.5227642760252413,
                -3,
            ),
            (
                1234,
                [39, 89, 31, 94, 33, 94, 80, 52],
                857019877,
                1923929452,
                2,
                0.9467375338760845,
                -1,
            ),
            (
                -5,
                [33, 28, 26, 62, 46, 92, 14, 95],
                726643700,
                610783965,
                1,
                0.6253758443637638,
                1,
            ),
            (
                2043104004,
                [93, 26, 86, 66, 86, 47, 76, 57],
                2012232425,
                566337385,
                6,
                0.6627786260390555,
                7,
            ),
        ];
        for (seed, hundred, a, b, seven, d, range) in cases {
            let mut r = DotNetRandom::new(seed);
            let got: Vec<i32> = (0..8).map(|_| r.next_max(100)).collect();
            assert_eq!(got, hundred, "seed {seed}");
            let mut r = DotNetRandom::new(seed);
            assert_eq!(
                (r.next_i32(), r.next_i32(), r.next_max(7)),
                (a, b, seven),
                "seed {seed}"
            );
            assert_eq!(r.next_f64(), d, "seed {seed}");
            assert_eq!(r.next_range(-5, 9), range, "seed {seed}");
        }
    }
}
