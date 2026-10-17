//! f32 math with the original's exact semantics (engine.md §5).
//!
//! Every gameplay expression is f32 arithmetic. Transcendentals go through
//! `libm` in f64 on the widened f32 argument and are rounded back to f32,
//! like `System.Math` called from C# float code. `Math.Round` is
//! round-half-to-even.

use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

/// `MathF.PI` (f32 3.14159274).
pub const PI: f32 = std::f32::consts::PI;
pub const TAU: f32 = PI * 2.0;

/// Angle constants of `Calc` (radians, y down: positive = clockwise on screen).
pub const RIGHT: f32 = 0.0;
pub const DOWN: f32 = PI / 2.0;
pub const LEFT: f32 = PI;
pub const UP: f32 = -PI / 2.0;
pub const DEG_TO_RAD: f32 = PI / 180.0;

/// A 2D f32 vector with FNA `Vector2` semantics.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    pub const ONE: Vec2 = Vec2 { x: 1.0, y: 1.0 };

    #[inline]
    pub const fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }
    #[inline]
    pub fn length_sq(self) -> f32 {
        self.x * self.x + self.y * self.y
    }
    #[inline]
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
    /// FNA `Normalize`: reciprocal multiply (a zero vector gives NaN).
    #[inline]
    pub fn normalize(self) -> Vec2 {
        let s = 1.0 / (self.x * self.x + self.y * self.y).sqrt();
        Vec2::new(self.x * s, self.y * s)
    }
    /// `Calc.SafeNormalize(v)`: `(1, 0)` for the exact zero vector.
    pub fn safe_normalize(self) -> Vec2 {
        if self == Vec2::ZERO {
            Vec2::new(1.0, 0.0)
        } else {
            self.normalize()
        }
    }
    /// `Calc.SafeNormalize(v, length)`.
    pub fn safe_normalize_len(self, len: f32) -> Vec2 {
        if self == Vec2::ZERO {
            Vec2::new(len, 0.0)
        } else {
            self.normalize() * len
        }
    }
    /// `Vector2.Angle()` = atan2(y, x).
    #[inline]
    pub fn angle(self) -> f32 {
        atan2(self.y, self.x)
    }
    #[inline]
    pub fn dot(self, o: Vec2) -> f32 {
        self.x * o.x + self.y * o.y
    }
    #[inline]
    pub fn dist_sq(self, o: Vec2) -> f32 {
        (o - self).length_sq()
    }
    #[inline]
    pub fn is_zero(self) -> bool {
        self.x == 0.0 && self.y == 0.0
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    #[inline]
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    #[inline]
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f32> for Vec2 {
    type Output = Vec2;
    #[inline]
    fn mul(self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}
impl Mul<Vec2> for Vec2 {
    type Output = Vec2;
    #[inline]
    fn mul(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x * o.x, self.y * o.y)
    }
}
impl Div<f32> for Vec2 {
    type Output = Vec2;
    #[inline]
    fn div(self, s: f32) -> Vec2 {
        Vec2::new(self.x / s, self.y / s)
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    #[inline]
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}
impl AddAssign for Vec2 {
    #[inline]
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}
impl SubAssign for Vec2 {
    #[inline]
    fn sub_assign(&mut self, o: Vec2) {
        *self = *self - o;
    }
}
impl MulAssign<f32> for Vec2 {
    #[inline]
    fn mul_assign(&mut self, s: f32) {
        *self = *self * s;
    }
}

// ------------------------------------------------------------ transcendentals

#[inline]
pub fn sin(a: f32) -> f32 {
    libm::sin(a as f64) as f32
}
#[inline]
pub fn cos(a: f32) -> f32 {
    libm::cos(a as f64) as f32
}
#[inline]
pub fn atan2(y: f32, x: f32) -> f32 {
    libm::atan2(y as f64, x as f64) as f32
}
#[inline]
pub fn pow(a: f32, b: f32) -> f32 {
    libm::pow(a as f64, b as f64) as f32
}
#[inline]
pub fn ceil(a: f32) -> f32 {
    libm::ceil(a as f64) as f32
}
#[inline]
pub fn floor(a: f32) -> f32 {
    libm::floor(a as f64) as f32
}

/// `Math.Round` (banker's rounding) of the f32 widened to f64.
#[inline]
pub fn round_he(x: f32) -> f32 {
    (x as f64).round_ties_even() as f32
}

/// `(int)Math.Round(x)`.
#[inline]
pub fn round_he_i(x: f32) -> i32 {
    (x as f64).round_ties_even() as i32
}

/// `Math.Sign(float)`: -1, 0, +1.
#[inline]
pub fn sign(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

#[inline]
pub fn sign_i(x: f32) -> i32 {
    sign(x) as i32
}

/// `Calc.Approach` (scalar).
#[inline]
pub fn approach(val: f32, target: f32, max_move: f32) -> f32 {
    if val > target {
        max(val - max_move, target)
    } else {
        min(val + max_move, target)
    }
}

/// `Calc.Approach` (vector).
pub fn approach_vec(val: Vec2, target: Vec2, max_move: f32) -> Vec2 {
    if max_move == 0.0 || val == target {
        return val;
    }
    let d = target - val;
    if d.length() < max_move {
        target
    } else {
        val + d.normalize() * max_move
    }
}

/// `MathHelper.Max`: `(a > b) ? a : b`.
#[inline]
pub fn max(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}
/// `MathHelper.Min`: `(a < b) ? a : b`.
#[inline]
pub fn min(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}
/// `MathHelper.Clamp`: hi applied first.
#[inline]
pub fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    let v = if v > hi { hi } else { v };
    if v < lo { lo } else { v }
}
/// `MathHelper.Lerp`.
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// `Calc.AngleToVector`.
#[inline]
pub fn angle_to_vector(angle: f32, len: f32) -> Vec2 {
    Vec2::new(cos(angle) * len, sin(angle) * len)
}

/// `Calc.AngleDiff`: `b - a` folded into (-PI, PI] by repeated f32 steps.
pub fn angle_diff(a: f32, b: f32) -> f32 {
    let mut d = b - a;
    while d > PI {
        d -= TAU;
    }
    while d <= -PI {
        d += TAU;
    }
    d
}

#[inline]
pub fn abs_angle_diff(a: f32, b: f32) -> f32 {
    angle_diff(a, b).abs()
}

/// `Calc.AngleApproach`.
pub fn angle_approach(val: f32, target: f32, max_move: f32) -> f32 {
    val + clamp(angle_diff(val, target), -max_move, max_move)
}

/// `Calc.Angle(from, to)`.
#[inline]
pub fn angle_between(from: Vec2, to: Vec2) -> f32 {
    atan2(to.y - from.y, to.x - from.x)
}

/// `Calc.Snap(v, inc)`: `round(v / inc) * inc` (banker's).
#[inline]
pub fn snap(v: f32, inc: f32) -> f32 {
    round_he(v / inc) * inc
}

/// `Calc.EightWayNormal`.
pub fn eight_way_normal(v: Vec2) -> Vec2 {
    let a = v.angle();
    let a = floor((a + PI / 8.0) / (PI / 4.0)) * (PI / 4.0);
    angle_to_vector(a, 1.0)
}

/// `Calc.ShorterAngleDifference(cur, a, b)`.
pub fn shorter_angle(cur: f32, a: f32, b: f32) -> f32 {
    if abs_angle_diff(cur, a) < abs_angle_diff(cur, b) {
        a
    } else {
        b
    }
}

/// `Calc.ClosestPointOnLine`.
pub fn closest_point_on_line(a: Vec2, b: Vec2, p: Vec2) -> Vec2 {
    let v = b - a;
    let t = (p - a).dot(v) / v.dot(v);
    let t = clamp(t, 0.0, 1.0);
    a + v * t
}

/// `(int)x` for f32 (truncation toward zero).
#[inline]
pub fn trunc_i(x: f32) -> i32 {
    x as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bankers_rounding() {
        assert_eq!(round_he_i(0.5), 0);
        assert_eq!(round_he_i(1.5), 2);
        assert_eq!(round_he_i(2.5), 2);
        assert_eq!(round_he_i(-0.5), 0);
        assert_eq!(round_he_i(-1.5), -2);
        assert_eq!(round_he_i(0.51), 1);
    }

    #[test]
    fn approach_semantics() {
        assert_eq!(approach(1.0, 0.0, 0.3), 0.7);
        assert_eq!(approach(0.0, 1.0, 5.0), 1.0);
        assert_eq!(approach(2.0, 2.0, 0.1), 2.0);
    }

    #[test]
    fn angle_helpers() {
        assert!((angle_diff(0.0, PI * 1.5) - (-PI / 2.0)).abs() < 1e-6);
        assert_eq!(snap(0.3, PI / 4.0), 0.0);
        assert_eq!(snap(0.5, PI / 4.0), PI / 4.0);
        let n = eight_way_normal(Vec2::new(1.0, 1.0));
        assert!((n.x - 0.70710677).abs() < 1e-6 && (n.y - 0.70710677).abs() < 1e-6);
        assert_eq!(sign(-0.0), 0.0);
    }
}
