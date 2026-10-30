//! Coordinate distributions used by compiled feature rules and scatter
//! features (`uniform`, `triangle`, `gaussian`, `inverse_gaussian`,
//! `fixed_grid`, `jittered_grid`), over the extent `[a, b]` as written in
//! the data:
//!
//! - `uniform`: `a + next_int(b - a)`, upper-exclusive; no draw (and `a`)
//!   for an empty or reversed extent;
//! - `triangle`: `a + next_int(r / 2 + 1) + next_int(r - r / 2 + 1)` with
//!   `r = b - a`;
//! - `gaussian`: `a + h + (next_int(h) - next_int(h))` with `h = (b - a) / 2`;
//! - `inverse_gaussian`: the same two draws `g`; `a - g` when negative,
//!   `b - g` when positive, and a coin flip between `a` and `b` at zero.
//!
//! Measured on the natural seed-12345 BDS world (see the codegen README): in
//! its roofed forest the decoration chains run 20-iteration gaussian scatters
//! on the same random as the trees, and the first diverging tree moved from
//! the second to the fifth iteration of chunk (4,-5); predicted copper-vein
//! centres (triangle y) have native copper within three blocks 400 of 1,056
//! times, against 123 with the earlier formula. Inverse gaussian (unused by
//! vanilla data) and the grid formulas are **unmeasured**.

use super::rng::BedrockRandom;

#[inline]
fn ordered(a: i32, b: i32) -> (i32, i32) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Uniform integer in `[a, b)`; `a`, without a draw, when `b <= a`.
#[inline]
pub fn uniform(rng: &mut BedrockRandom, a: i32, b: i32) -> i32 {
    if b > a { a + rng.next_int(b - a) } else { a }
}

/// Sum of two uniforms over the two halves of `[a, b]`: peaked at the centre.
#[inline]
pub fn triangle(rng: &mut BedrockRandom, a: i32, b: i32) -> i32 {
    let r = b - a;
    let low = r / 2;
    let high = r - low;
    let mut v = a;
    if r >= -1 {
        v += rng.next_int(low + 1);
    }
    if high >= 0 {
        v += rng.next_int(high + 1);
    }
    v
}

/// Bedrock's integer "gaussian": the difference of two uniforms below `n`.
#[inline]
fn gaussian_int(rng: &mut BedrockRandom, n: i32) -> i32 {
    rng.next_int(n) - rng.next_int(n)
}

/// Centre of `[a, b]` plus [`gaussian_int`] of the half width.
#[inline]
pub fn gaussian(rng: &mut BedrockRandom, a: i32, b: i32) -> i32 {
    let half = (b - a) / 2;
    a + half + gaussian_int(rng, half)
}

/// [`gaussian_int`] folded outwards: most mass near `a` and `b`.
#[inline]
pub fn inverse_gaussian(rng: &mut BedrockRandom, a: i32, b: i32) -> i32 {
    let g = gaussian_int(rng, (b - a) / 2);
    match g.cmp(&0) {
        std::cmp::Ordering::Less => a - g,
        std::cmp::Ordering::Greater => b - g,
        std::cmp::Ordering::Equal => {
            if rng.next_bool() {
                b
            } else {
                a
            }
        }
    }
}

/// Number of cells of a grid over `[a, b]` with `step` and `offset`.
#[inline]
pub fn grid_cells(a: i32, b: i32, step: i32, offset: i32) -> i32 {
    let (a, b) = ordered(a, b);
    let step = step.max(1);
    ((b - a - offset) / step + 1).max(1)
}

/// Fixed grid: cell `index` (wrapping) of the grid over `[a, b]`.
#[inline]
pub fn fixed_grid(a: i32, b: i32, step: i32, offset: i32, index: i32) -> i32 {
    let (a, _) = ordered(a, b);
    a + offset + step.max(1) * index.rem_euclid(grid_cells(a, b, step, offset))
}

/// Jittered grid: cell `index` plus a uniform offset inside the cell.
#[inline]
pub fn jittered_grid(
    rng: &mut BedrockRandom,
    a: i32,
    b: i32,
    step: i32,
    offset: i32,
    index: i32,
) -> i32 {
    let (_, hi) = ordered(a, b);
    let base = fixed_grid(a, b, step, offset, index);
    (base + rng.next_int(step.max(1))).min(hi)
}

/// Molang coordinate offset: nearest integer, with half ties away from zero.
/// Conversion precedes translation by the input position (BDS marker controls).
#[inline]
pub fn coord(v: f32) -> i32 {
    v.round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distributions_stay_inside_their_extent() {
        let mut r = BedrockRandom::new(7);
        for _ in 0..5_000 {
            assert!((-3..8).contains(&uniform(&mut r, -3, 8)));
            for f in [triangle, gaussian, inverse_gaussian] {
                let v = f(&mut r, -3, 8);
                assert!((-3..=8).contains(&v), "{v}");
            }
        }
    }

    #[test]
    fn grids_step_and_wrap() {
        assert_eq!(grid_cells(-1, 1, 1, 0), 3);
        assert_eq!(grid_cells(-1, 1, 2, 1), 1);
        let cells: Vec<_> = (0..4).map(|i| fixed_grid(0, 6, 2, 0, i)).collect();
        assert_eq!(cells, [0, 2, 4, 6]);
        assert_eq!(fixed_grid(0, 6, 2, 0, 4), 0);
        assert_eq!(fixed_grid(-1, 1, 2, 1, 0), 0);
    }
}
