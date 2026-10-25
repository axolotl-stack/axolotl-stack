//! Java-style overworld biome source.
//!
//! Biomes are chosen per quart (4x4x4 cell) at the quart's corner block: the noise
//! router's climate functions (temperature, vegetation, continents, erosion, depth,
//! ridges) are quantized and searched in the data generator's parameter list
//! (`unastar_noise::overworld_biome_source`). Block-level lookups go through Java's
//! "fuzzy" zoom ([`zoomed_biome`]), which picks one of the 8 surrounding quarts with a
//! seed-derived jitter.

use sha2::{Digest, Sha256};
use unastar_noise::java_biomes::JavaBiome;
use unastar_noise::{
    ColumnContextGrid, FlatCacheGrid, FunctionContext, compute_continents,
    compute_continents_uncached, compute_depth, compute_depth_uncached, compute_erosion,
    compute_erosion_uncached, compute_ridges, compute_ridges_uncached, compute_temperature,
    compute_temperature_uncached, compute_vegetation, compute_vegetation_uncached,
    overworld_biome_source, overworld_settings,
};

use super::density::NoiseRegistry;
use super::proto::{NativeBiome, QUARTS_Y, QuartBiomes};
use crate::world::ChunkPos;
use crate::world::chunk::MIN_Y;

/// Biome at an absolute quart position, without chunk caches.
pub fn sample_biome(noises: &NoiseRegistry, quart_x: i32, quart_y: i32, quart_z: i32) -> JavaBiome {
    let ctx = FunctionContext::new(quart_x << 2, quart_y << 2, quart_z << 2);
    let target = overworld_biome_source::target(
        compute_temperature_uncached(&ctx, noises),
        compute_vegetation_uncached(&ctx, noises),
        compute_continents_uncached(&ctx, noises),
        compute_erosion_uncached(&ctx, noises),
        compute_depth_uncached(&ctx, noises),
        compute_ridges_uncached(&ctx, noises),
    );
    overworld_biome_source::find(&target)
}

/// The 3D quart biomes of a chunk, using the chunk's flat and column caches.
pub fn fill_chunk(
    noises: &NoiseRegistry,
    pos: ChunkPos,
    grid: &FlatCacheGrid,
    col_grid: &ColumnContextGrid,
) -> QuartBiomes {
    let mut biomes = QuartBiomes::filled(0);
    for qz in 0..4 {
        for qx in 0..4 {
            let x = pos.x * 16 + (qx as i32) * 4;
            let z = pos.z * 16 + (qz as i32) * 4;
            let col = col_grid.get(qx * 4, qz * 4).as_scalar();
            let mut previous: Option<([i64; overworld_biome_source::DIMS], JavaBiome)> = None;
            for qy in 0..QUARTS_Y {
                let ctx = FunctionContext::new(x, MIN_Y + (qy as i32) * 4, z);
                let target = overworld_biome_source::target(
                    compute_temperature(&ctx, noises, grid, &col),
                    compute_vegetation(&ctx, noises, grid, &col),
                    compute_continents(&ctx, noises, grid, &col),
                    compute_erosion(&ctx, noises, grid, &col),
                    compute_depth(&ctx, noises, grid, &col),
                    compute_ridges(&ctx, noises, grid, &col),
                );
                // The search is a pure function of the target; columns repeat targets.
                let biome = match previous {
                    Some((last, biome)) if last == target => biome,
                    _ => overworld_biome_source::find(&target),
                };
                previous = Some((target, biome));
                biomes.set(qx, qy, qz, biome as NativeBiome);
            }
        }
    }
    biomes
}

/// Block column (x, z) where Java's world-spawn search starts: the position closest to
/// the origin whose climate (at y = 0, depth forced to 0) best fits the noise settings'
/// `spawn_target`. Fitness is `distance² * 2048² + x² + z²`, minimised over the origin,
/// a coarse ring search (radius 512..=2048, step 512) and a fine one around the best
/// so far (radius 32..=512, step 32).
pub fn spawn_search_start(noises: &NoiseRegistry) -> (i32, i32) {
    let fitness = |x: i32, z: i32| -> i64 {
        let ctx = FunctionContext::new((x >> 2) << 2, 0, (z >> 2) << 2);
        let target = overworld_biome_source::target(
            compute_temperature_uncached(&ctx, noises),
            compute_vegetation_uncached(&ctx, noises),
            compute_continents_uncached(&ctx, noises),
            compute_erosion_uncached(&ctx, noises),
            0.0,
            compute_ridges_uncached(&ctx, noises),
        );
        let distance = overworld_settings::SPAWN_TARGET
            .iter()
            .map(|point| {
                point
                    .iter()
                    .zip(target)
                    .map(|(&[min, max], value)| {
                        let d = if value > max {
                            value - max
                        } else {
                            (min - value).max(0)
                        };
                        d * d
                    })
                    .sum::<i64>()
            })
            .min()
            .unwrap_or(i64::MAX);
        distance * 2048 * 2048 + (x as i64).pow(2) + (z as i64).pow(2)
    };
    let mut best = ((0, 0), fitness(0, 0));
    for (max_radius, step) in [(2048.0f32, 512.0f32), (512.0, 32.0)] {
        let (center_x, center_z) = best.0;
        let mut angle = 0.0f32;
        let mut radius = step;
        while radius <= max_radius {
            let x = center_x + ((angle as f64).sin() * radius as f64) as i32;
            let z = center_z + ((angle as f64).cos() * radius as f64) as i32;
            let f = fitness(x, z);
            if f < best.1 {
                best = ((x, z), f);
            }
            angle += step / radius;
            if angle > std::f32::consts::PI * 2.0 {
                angle = 0.0;
                radius += step;
            }
        }
    }
    best.0
}

/// Seed of the biome zoom: the first 8 bytes (little-endian) of SHA-256 of the world
/// seed's 8 little-endian bytes.
pub fn zoom_seed(seed: i64) -> i64 {
    let digest = Sha256::digest(seed.to_le_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_le_bytes(bytes)
}

#[inline]
fn lcg_next(state: i64, add: i64) -> i64 {
    let next = state.wrapping_mul(
        state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407),
    );
    next.wrapping_add(add)
}

#[inline]
fn fiddle(state: i64) -> f64 {
    let d = ((state >> 24).rem_euclid(1024)) as f64 / 1024.0;
    (d - 0.5) * 0.9
}

#[inline]
fn fiddled_distance(seed: i64, x: i32, y: i32, z: i32, dx: f64, dy: f64, dz: f64) -> f64 {
    let mut m = lcg_next(seed, x as i64);
    m = lcg_next(m, y as i64);
    m = lcg_next(m, z as i64);
    m = lcg_next(m, x as i64);
    m = lcg_next(m, y as i64);
    m = lcg_next(m, z as i64);
    let fx = fiddle(m);
    m = lcg_next(m, seed);
    let fy = fiddle(m);
    m = lcg_next(m, seed);
    let fz = fiddle(m);
    (dz + fz) * (dz + fz) + (dy + fy) * (dy + fy) + (dx + fx) * (dx + fx)
}

/// Java's block-level biome lookup: the quart among the 8 around `(x, y, z)` (offset by
/// -2 blocks) with the smallest seed-jittered distance; `quart_biome` gives the biome of
/// an absolute quart.
pub fn zoomed_biome<B: Copy>(
    zoom_seed: i64,
    x: i32,
    y: i32,
    z: i32,
    mut quart_biome: impl FnMut(i32, i32, i32) -> B,
) -> B {
    let (i, j, k) = (x - 2, y - 2, z - 2);
    let (l, m, n) = (i >> 2, j >> 2, k >> 2);
    let (d, e, f) = (
        (i & 3) as f64 / 4.0,
        (j & 3) as f64 / 4.0,
        (k & 3) as f64 / 4.0,
    );
    let mut best = 0;
    let mut best_distance = f64::INFINITY;
    for p in 0..8 {
        let (bx, by, bz) = (p & 4 == 0, p & 2 == 0, p & 1 == 0);
        let distance = fiddled_distance(
            zoom_seed,
            if bx { l } else { l + 1 },
            if by { m } else { m + 1 },
            if bz { n } else { n + 1 },
            if bx { d } else { d - 1.0 },
            if by { e } else { e - 1.0 },
            if bz { f } else { f - 1.0 },
        );
        if best_distance > distance {
            best = p;
            best_distance = distance;
        }
    }
    quart_biome(
        if best & 4 == 0 { l } else { l + 1 },
        if best & 2 == 0 { m } else { m + 1 },
        if best & 1 == 0 { n } else { n + 1 },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use unastar_noise::{ColumnContext, compute_preliminary_surface_level};
    use unastar_noise::{FunctionContext as Ctx, compute_preliminary_surface_level_uncached};

    /// The cache-free roots (used outside a chunk's caches) must agree with the
    /// chunk-cached ones; they once dropped the climate shift noise and returned the
    /// preliminary surface's lower bound everywhere.
    #[test]
    fn uncached_roots_match_chunk_caches() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                let noises = NoiseRegistry::new(12345);
                for pos in [ChunkPos::new(0, 0), ChunkPos::new(-128, 33)] {
                    let grid = FlatCacheGrid::new(pos.x, pos.z, &noises);
                    let cols = ColumnContextGrid::new(pos.x, pos.z, &noises, &grid);
                    let biomes = fill_chunk(&noises, pos, &grid, &cols);
                    for (qx, qy, qz) in [(0, 30, 0), (3, 20, 1), (2, 5, 3)] {
                        let sampled = sample_biome(
                            &noises,
                            pos.x * 4 + qx as i32,
                            (MIN_Y >> 2) + qy as i32,
                            pos.z * 4 + qz as i32,
                        );
                        assert_eq!(sampled as NativeBiome, biomes.get(qx, qy, qz));
                    }
                    let (x, z) = (pos.x * 16 + 16, pos.z * 16);
                    let ctx = Ctx::new(x, 0, z);
                    let col = ColumnContext::new(x, z, &noises, &grid);
                    assert_eq!(
                        compute_preliminary_surface_level_uncached(&ctx, &noises),
                        compute_preliminary_surface_level(&ctx, &noises, &grid, &col)
                    );
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
