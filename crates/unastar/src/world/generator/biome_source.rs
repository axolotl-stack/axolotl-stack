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
    ColumnContext, ColumnContextGrid, FlatCacheGrid, FunctionContext, compute_continents,
    compute_continents_uncached, compute_depth, compute_depth_uncached, compute_erosion,
    compute_erosion_uncached, compute_ridges, compute_ridges_uncached, compute_temperature,
    compute_temperature_uncached, compute_vegetation, compute_vegetation_uncached,
    overworld_biome_source,
};

use super::density::NoiseRegistry;
use super::proto::{NativeBiome, QUARTS_Y, QuartBiomes};
use crate::world::ChunkPos;
use crate::world::chunk::MIN_Y;

/// Biome at an absolute quart position, without chunk caches.
pub fn sample_biome(noises: &NoiseRegistry, quart_x: i32, quart_y: i32, quart_z: i32) -> JavaBiome {
    let (x, y, z) = (quart_x << 2, quart_y << 2, quart_z << 2);
    let ctx = FunctionContext::new(x, y, z);
    let col = ColumnContext::new_standalone(x, z, noises);
    let target = overworld_biome_source::target(
        compute_temperature_uncached(&ctx, noises, &col),
        compute_vegetation_uncached(&ctx, noises, &col),
        compute_continents_uncached(&ctx, noises, &col),
        compute_erosion_uncached(&ctx, noises, &col),
        compute_depth_uncached(&ctx, noises, &col),
        compute_ridges_uncached(&ctx, noises, &col),
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
