//! Coordinate distributions used by compiled feature rules and scatter
//! features (`uniform`, `triangle`, `gaussian`, `inverse_gaussian`,
//! `fixed_grid`, `jittered_grid`). Extents are inclusive.
//!
//! The shapes follow Mojang's feature documentation (uniform = flat,
//! triangle = peaked at the centre, gaussian = bell, inverse_gaussian = mass
//! at the edges, grids = regular steps). BDS's exact draw sequence is not
//! observable from data; these keep the documented shape and range, which is
//! what statistical decoration parity (research WD4) relies on.

use super::rng::BedrockRandom;

#[inline]
fn ordered(a: i32, b: i32) -> (i32, i32) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Uniform integer in `[a, b]`.
#[inline]
pub fn uniform(rng: &mut BedrockRandom, a: i32, b: i32) -> i32 {
    let (a, b) = ordered(a, b);
    rng.next_int_inclusive(a, b)
}

/// Sum of two uniforms: peaked at the centre of `[a, b]`.
#[inline]
pub fn triangle(rng: &mut BedrockRandom, a: i32, b: i32) -> i32 {
    let (a, b) = ordered(a, b);
    let span = b - a + 1;
    a + (rng.next_int(span) + rng.next_int(span)) / 2
}

/// Mean of three uniforms: bell-shaped over `[a, b]`.
#[inline]
pub fn gaussian(rng: &mut BedrockRandom, a: i32, b: i32) -> i32 {
    let (a, b) = ordered(a, b);
    let span = b - a + 1;
    a + (rng.next_int(span) + rng.next_int(span) + rng.next_int(span)) / 3
}

/// Gaussian folded outwards: most mass near `a` and `b`.
#[inline]
pub fn inverse_gaussian(rng: &mut BedrockRandom, a: i32, b: i32) -> i32 {
    let (a, b) = ordered(a, b);
    let g = gaussian(rng, a, b);
    let mid = (a + b) / 2;
    if g <= mid { a + (mid - g) } else { b - (g - mid) }
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
pub fn jittered_grid(rng: &mut BedrockRandom, a: i32, b: i32, step: i32, offset: i32, index: i32) -> i32 {
    let (_, hi) = ordered(a, b);
    let base = fixed_grid(a, b, step, offset, index);
    (base + rng.next_int(step.max(1))).min(hi)
}

/// Molang result to a block coordinate (floor, like the scattered positions
/// of Bedrock's integer block grid).
#[inline]
pub fn coord(v: f32) -> i32 {
    v.floor() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distributions_stay_inside_their_extent() {
        let mut r = BedrockRandom::new(7);
        for _ in 0..5_000 {
            for f in [uniform, triangle, gaussian, inverse_gaussian] {
                let v = f(&mut r, -3, 8);
                assert!((-3..=8).contains(&v));
                assert!((-3..=8).contains(&f(&mut r, 8, -3)));
            }
        }
    }

    #[test]
    fn shapes_concentrate_where_documented() {
        let mut r = BedrockRandom::new(99);
        let (mut centre_tri, mut edge_inv, mut centre_uni) = (0, 0, 0);
        for _ in 0..20_000 {
            centre_tri += i32::from((-1..=1).contains(&triangle(&mut r, -8, 8)));
            centre_uni += i32::from((-1..=1).contains(&uniform(&mut r, -8, 8)));
            edge_inv += i32::from(inverse_gaussian(&mut r, -8, 8).abs() >= 6);
        }
        assert!(centre_tri > centre_uni * 3 / 2, "{centre_tri} vs {centre_uni}");
        assert!(edge_inv > 20_000 * 3 / 17, "{edge_inv}");
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

    #[test]
    fn coords_floor_negative_values() {
        assert_eq!(coord(-0.5), -1);
        assert_eq!(coord(3.99), 3);
    }
}
