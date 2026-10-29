//! Small Java utility behaviours shared by structure code.

use crate::features::random::{BitSource, WorldgenRandom};

/// `Util.shuffle(list, random)` / `Util.toShuffledList`: Fisher-Yates from the
/// end, `nextInt(i)` for `i = len..2`.
pub fn shuffle<T, S: BitSource, const RETAIN_GAUSSIAN: bool>(
    list: &mut [T],
    rng: &mut WorldgenRandom<S, RETAIN_GAUSSIAN>,
) {
    let mut i = list.len();
    while i > 1 {
        let k = rng.next_int_bounded(i as i32) as usize;
        list.swap(i - 1, k);
        i -= 1;
    }
}

/// `Mth.getSeed(x, y, z)`. Java multiplies `x` in 32-bit `int` arithmetic
/// before widening (`(long)(x * 3129871)`), while `z` is multiplied as a long.
#[inline]
pub fn pos_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut l =
        (x.wrapping_mul(3_129_871) as i64) ^ (z as i64).wrapping_mul(116_129_781) ^ (y as i64);
    l = l
        .wrapping_mul(l)
        .wrapping_mul(42_317_861)
        .wrapping_add(l.wrapping_mul(11));
    l >> 16
}

/// `Mth.inverseLerp`.
#[inline]
pub fn inverse_lerp(v: f32, a: f32, b: f32) -> f32 {
    (v - a) / (b - a)
}

/// `Mth.clampedLerp(a, b, t)`.
#[inline]
pub fn clamped_lerp(a: f32, b: f32, t: f32) -> f32 {
    if t < 0.0 {
        a
    } else if t > 1.0 {
        b
    } else {
        a + t * (b - a)
    }
}
