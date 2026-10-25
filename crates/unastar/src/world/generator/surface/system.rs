//! Java's surface pass over a proto chunk.
//!
//! Per column, from the generation surface (`WORLD_SURFACE_WG` + 1) down to the bottom:
//! air resets the stone depth, fluids set the water height, and every default-block
//! (stone) position is offered to the compiled surface rule with the column's stone depth
//! above/below, water height, surface depth and lazily sampled context. Eroded badlands
//! columns first grow stone pillars; frozen-ocean columns get icebergs afterwards.

use std::collections::HashMap;

use unastar_noise::java_biomes::JavaBiome;
use unastar_noise::surface::{BAND_COUNT, SurfaceEnv, clay_bands};
use unastar_noise::xoroshiro::PositionalRandomFactory;
use unastar_noise::{
    ColumnContext, FlatCacheGrid, FunctionContext, NoiseRef, NoiseSource, biome_climate,
    block_class, compute_preliminary_surface_level, java_blocks, overworld_settings,
    overworld_surface, random_hashes,
};

use crate::world::ChunkPos;
use crate::world::chunk::MIN_Y;
use crate::world::generator::biome_source::{sample_biome, zoom_seed, zoomed_biome};
use crate::world::generator::density::NoiseRegistry;
use crate::world::generator::proto::{DenseBlocks, QUARTS_Y, QuartBiomes, WORLD_HEIGHT};

const MAX_Y: i32 = MIN_Y + WORLD_HEIGHT - 1;

#[inline]
fn class(block: u32) -> u8 {
    java_blocks::BEDROCK_STATE_CLASSES
        .get(block as usize)
        .copied()
        .unwrap_or(block_class::UNMAPPED)
}

#[inline]
fn is_air(block: u32) -> bool {
    class(block) & block_class::AIR != 0
}

#[inline]
fn has_fluid(block: u32) -> bool {
    class(block) & block_class::FLUID != 0
}

#[inline]
fn is_water(block: u32) -> bool {
    class(block) & block_class::WATER != 0
}

/// Java's `isStone` in the surface pass: any block that is neither air nor fluid.
#[inline]
fn is_solid(block: u32) -> bool {
    class(block) & (block_class::AIR | block_class::FLUID) == 0
}

/// Seed-derived state of the surface pass.
pub struct SurfaceSystem {
    /// World base positional random (surface depth jitter, iceberg randoms).
    noise_random: PositionalRandomFactory,
    /// Positional randoms of the rule's vertical gradients, by gradient index.
    gradient_randoms: Vec<PositionalRandomFactory>,
    clay_bands: [u32; BAND_COUNT],
    zoom_seed: i64,
}

impl SurfaceSystem {
    pub fn new(seed: i64) -> Self {
        let noise_random = PositionalRandomFactory::new(seed);
        let gradient_randoms = overworld_surface::GRADIENT_HASHES
            .iter()
            .map(|&hash| noise_random.fork_from_hash(hash))
            .collect();
        let mut band_random = noise_random.from_hash(random_hashes::CLAY_BANDS);
        let clay_bands = clay_bands(&mut band_random, &overworld_surface::BAND_BLOCKS);
        Self {
            noise_random,
            gradient_randoms,
            clay_bands,
            zoom_seed: zoom_seed(seed),
        }
    }

    /// Apply the surface pass to a proto chunk with its quart biomes. `grid` is the
    /// chunk's flat cache, which also covers the +16 corners of the chunk.
    pub fn build_surface(
        &self,
        chunk: &mut DenseBlocks,
        pos: ChunkPos,
        noises: &NoiseRegistry,
        grid: &FlatCacheGrid,
        biomes: &QuartBiomes,
    ) {
        let mut heights = column_heights(chunk);
        let corners = preliminary_corners(pos, noises, grid);
        let mut border_biomes = HashMap::new();

        for local_x in 0..16u8 {
            for local_z in 0..16u8 {
                let x = pos.x * 16 + local_x as i32;
                let z = pos.z * 16 + local_z as i32;
                let top = heights[column_index(local_x, local_z)] as i32 + 1;
                let mut env = ColumnEnv {
                    system: self,
                    noises,
                    pos,
                    biomes,
                    border_biomes: &mut border_biomes,
                    heights: &heights,
                    corners: &corners,
                    local_x,
                    local_z,
                    x,
                    z,
                    y: top,
                    surface_depth: self.surface_depth(noises, x, z),
                    surface_secondary: None,
                    min_surface_level: None,
                    steep: None,
                    noise_cache: [None; NoiseRef::COUNT],
                    biome_cache: None,
                    stone_depth_above: 0,
                    stone_depth_below: 0,
                    water_height: i32::MIN,
                };
                let column_biome = env.biome_at(top);
                if column_biome == JavaBiome::ErodedBadlands {
                    self.eroded_badlands_extension(chunk, noises, local_x, local_z, x, z, top);
                }
                let top = top_non_air(chunk, local_x, local_z) + 1;

                let mut stone_depth_above = 0;
                let mut water_height = i32::MIN;
                let mut solid_below_until = i32::MAX;
                let mut y = top;
                while y >= MIN_Y {
                    let block = chunk.get_block(local_x, y as i16, local_z);
                    if is_air(block) {
                        stone_depth_above = 0;
                        water_height = i32::MIN;
                        y -= 1;
                        continue;
                    }
                    if has_fluid(block) {
                        if water_height == i32::MIN {
                            water_height = y + 1;
                        }
                        y -= 1;
                        continue;
                    }
                    if solid_below_until >= y {
                        solid_below_until = i32::MIN;
                        let mut below = y - 1;
                        while below >= MIN_Y - 1 {
                            let solid = below >= MIN_Y
                                && is_solid(chunk.get_block(local_x, below as i16, local_z));
                            if !solid {
                                solid_below_until = below + 1;
                                break;
                            }
                            below -= 1;
                        }
                    }
                    stone_depth_above += 1;
                    env.y = y;
                    env.biome_cache = None;
                    env.stone_depth_above = stone_depth_above;
                    env.stone_depth_below = y - solid_below_until + 1;
                    env.water_height = water_height;
                    if block == overworld_settings::DEFAULT_BLOCK
                        && let Some(replacement) = overworld_surface::apply(&mut env)
                    {
                        chunk.set_block(local_x, y as i16, local_z, replacement);
                    }
                    y -= 1;
                }

                if matches!(
                    column_biome,
                    JavaBiome::FrozenOcean | JavaBiome::DeepFrozenOcean
                ) {
                    let min_surface_level = env.min_surface_level();
                    self.frozen_ocean_extension(
                        chunk,
                        noises,
                        column_biome,
                        min_surface_level,
                        local_x,
                        local_z,
                        x,
                        z,
                        top,
                    );
                }
                heights[column_index(local_x, local_z)] =
                    top_non_air(chunk, local_x, local_z) as i16;
            }
        }
    }

    /// Java's carver `topMaterial`: the surface rule's block for exposed dirt at
    /// `(x, y, z)` under a carved surface block, with stone depth 1 above and below.
    #[allow(clippy::too_many_arguments)]
    pub fn top_material(
        &self,
        chunk: &DenseBlocks,
        pos: ChunkPos,
        noises: &NoiseRegistry,
        grid: &FlatCacheGrid,
        biomes: &QuartBiomes,
        x: i32,
        y: i32,
        z: i32,
        has_fluid: bool,
    ) -> Option<u32> {
        let heights = column_heights(chunk);
        let corners = preliminary_corners(pos, noises, grid);
        let mut border_biomes = HashMap::new();
        let mut env = ColumnEnv {
            system: self,
            noises,
            pos,
            biomes,
            border_biomes: &mut border_biomes,
            heights: &heights,
            corners: &corners,
            local_x: (x & 15) as u8,
            local_z: (z & 15) as u8,
            x,
            z,
            y,
            surface_depth: self.surface_depth(noises, x, z),
            surface_secondary: None,
            min_surface_level: None,
            steep: None,
            noise_cache: [None; NoiseRef::COUNT],
            biome_cache: None,
            stone_depth_above: 1,
            stone_depth_below: 1,
            water_height: if has_fluid { y + 1 } else { i32::MIN },
        };
        overworld_surface::apply(&mut env)
    }

    /// `(int)(surface * 2.75 + 3 + random(x, 0, z) * 0.25)`.
    fn surface_depth(&self, noises: &NoiseRegistry, x: i32, z: i32) -> i32 {
        let noise = noises.sample(NoiseRef::Surface, x as f64, 0.0, z as f64);
        let jitter = self.noise_random.at(x, 0, z).next_double();
        (noise * 2.75 + 3.0 + jitter * 0.25) as i32
    }

    fn band(&self, noises: &NoiseRegistry, x: i32, y: i32, z: i32) -> u32 {
        let offset = (noises.sample(NoiseRef::ClayBandsOffset, x as f64, 0.0, z as f64) * 4.0 + 0.5)
            .floor() as i32;
        self.clay_bands[((y + offset + BAND_COUNT as i32) % BAND_COUNT as i32) as usize]
    }

    /// Stone pillars of eroded badlands, grown from y=64 up to a noise-driven height.
    #[allow(clippy::too_many_arguments)]
    fn eroded_badlands_extension(
        &self,
        chunk: &mut DenseBlocks,
        noises: &NoiseRegistry,
        local_x: u8,
        local_z: u8,
        x: i32,
        z: i32,
        height: i32,
    ) {
        let surface =
            (noises.sample(NoiseRef::BadlandsSurface, x as f64, 0.0, z as f64) * 8.25).abs();
        let pillar = noises.sample(
            NoiseRef::BadlandsPillar,
            x as f64 * 0.2,
            0.0,
            z as f64 * 0.2,
        ) * 15.0;
        let strength = surface.min(pillar);
        if strength <= 0.0 {
            return;
        }
        let roof = (noises.sample(
            NoiseRef::BadlandsPillarRoof,
            x as f64 * 0.75,
            0.0,
            z as f64 * 0.75,
        ) * 1.5)
            .abs();
        let pillar_top =
            (64.0 + (strength * strength * 2.5).min((roof * 50.0).ceil() + 24.0)).floor() as i32;
        if height > pillar_top {
            return;
        }
        let mut y = pillar_top;
        while y >= MIN_Y {
            let block = chunk.get_block(local_x, y as i16, local_z);
            if block == overworld_settings::DEFAULT_BLOCK {
                break;
            }
            if is_water(block) {
                return;
            }
            y -= 1;
        }
        let mut y = pillar_top;
        while y >= MIN_Y && is_air(chunk.get_block(local_x, y as i16, local_z)) {
            chunk.set_block(
                local_x,
                y as i16,
                local_z,
                overworld_settings::DEFAULT_BLOCK,
            );
            y -= 1;
        }
    }

    /// Packed-ice icebergs with snow caps in frozen oceans.
    #[allow(clippy::too_many_arguments)]
    fn frozen_ocean_extension(
        &self,
        chunk: &mut DenseBlocks,
        noises: &NoiseRegistry,
        biome: JavaBiome,
        min_surface_level: i32,
        local_x: u8,
        local_z: u8,
        x: i32,
        z: i32,
        height: i32,
    ) {
        let sea_level = overworld_settings::SEA_LEVEL;
        let surface =
            (noises.sample(NoiseRef::IcebergSurface, x as f64, 0.0, z as f64) * 8.25).abs();
        let pillar = noises.sample(
            NoiseRef::IcebergPillar,
            x as f64 * 1.28,
            0.0,
            z as f64 * 1.28,
        ) * 15.0;
        let strength = surface.min(pillar);
        if strength <= 1.8 {
            return;
        }
        let roof = (noises.sample(
            NoiseRef::IcebergPillarRoof,
            x as f64 * 1.17,
            0.0,
            z as f64 * 1.17,
        ) * 1.5)
            .abs();
        let mut top = (strength * strength * 1.2).min((roof * 40.0).ceil() + 14.0);
        if biome_climate::should_melt_frozen_ocean_iceberg_slightly(
            biome, x, sea_level, z, sea_level,
        ) {
            top -= 2.0;
        }
        let bottom;
        if top > 2.0 {
            bottom = sea_level as f64 - top - 7.0;
            top += sea_level as f64;
        } else {
            top = 0.0;
            bottom = 0.0;
        }
        let mut random = self.noise_random.at(x, 0, z);
        let snow_depth = 2 + random.next_int(4) as i32;
        let snow_min_y = sea_level + 18 + random.next_int(10) as i32;
        let mut snow_placed = 0;
        let mut y = height.max(top as i32 + 1);
        while y >= min_surface_level {
            let block = chunk.get_block(local_x, y as i16, local_z);
            let place = (is_air(block) && y < top as i32 && random.next_double() > 0.01)
                || (is_water(block)
                    && y > bottom as i32
                    && y < sea_level
                    && bottom != 0.0
                    && random.next_double() > 0.15);
            if place {
                if snow_placed <= snow_depth && y > snow_min_y {
                    chunk.set_block(local_x, y as i16, local_z, overworld_surface::SNOW_BLOCK);
                    snow_placed += 1;
                } else {
                    chunk.set_block(local_x, y as i16, local_z, overworld_surface::PACKED_ICE);
                }
            }
            y -= 1;
        }
    }
}

#[inline]
fn column_index(x: u8, z: u8) -> usize {
    (z as usize) << 4 | x as usize
}

/// Top non-air block of every column (`z * 16 + x`), the generation surface heightmap.
fn column_heights(chunk: &DenseBlocks) -> [i16; 256] {
    let mut heights = [MIN_Y as i16; 256];
    for z in 0..16u8 {
        for x in 0..16u8 {
            heights[column_index(x, z)] = top_non_air(chunk, x, z) as i16;
        }
    }
    heights
}

/// Preliminary surface at the chunk's section corners (x, x+16) x (z, z+16); the chunk's
/// flat cache covers the +16 corners.
fn preliminary_corners(pos: ChunkPos, noises: &NoiseRegistry, grid: &FlatCacheGrid) -> [f64; 4] {
    [(0, 0), (16, 0), (0, 16), (16, 16)].map(|(dx, dz)| {
        let (x, z) = (pos.x * 16 + dx, pos.z * 16 + dz);
        let ctx = FunctionContext::new(x, 0, z);
        let col = ColumnContext::new(x, z, noises, grid);
        compute_preliminary_surface_level(&ctx, noises, grid, &col).floor()
    })
}

/// y of the highest non-air block (`MIN_Y - 1` if none).
fn top_non_air(chunk: &DenseBlocks, x: u8, z: u8) -> i32 {
    let mut y = MAX_Y;
    while y >= MIN_Y {
        if !is_air(chunk.get_block(x, y as i16, z)) {
            return y;
        }
        y -= 1;
    }
    MIN_Y - 1
}

/// [`SurfaceEnv`] for one column of the pass.
struct ColumnEnv<'a> {
    system: &'a SurfaceSystem,
    noises: &'a NoiseRegistry,
    pos: ChunkPos,
    biomes: &'a QuartBiomes,
    /// Quart biomes outside the chunk, sampled on demand.
    border_biomes: &'a mut HashMap<(i32, i32, i32), JavaBiome>,
    /// `WORLD_SURFACE_WG` top block per column (`z * 16 + x`).
    heights: &'a [i16; 256],
    /// Preliminary surface level at the chunk's four corners (x, x+16) x (z, z+16).
    corners: &'a [f64; 4],
    local_x: u8,
    local_z: u8,
    x: i32,
    z: i32,
    y: i32,
    surface_depth: i32,
    surface_secondary: Option<f64>,
    min_surface_level: Option<i32>,
    steep: Option<bool>,
    noise_cache: [Option<f64>; NoiseRef::COUNT],
    biome_cache: Option<JavaBiome>,
    stone_depth_above: i32,
    stone_depth_below: i32,
    water_height: i32,
}

impl ColumnEnv<'_> {
    fn biome_at(&mut self, y: i32) -> JavaBiome {
        let (pos, biomes, noises) = (self.pos, self.biomes, self.noises);
        let border = &mut *self.border_biomes;
        zoomed_biome(self.system.zoom_seed, self.x, y, self.z, |qx, qy, qz| {
            let qy = qy.clamp(
                MIN_Y >> 2,
                (MAX_Y >> 2).min((MIN_Y >> 2) + QUARTS_Y as i32 - 1),
            );
            let (lx, lz) = (qx - pos.x * 4, qz - pos.z * 4);
            if (0..4).contains(&lx) && (0..4).contains(&lz) {
                let native = biomes.get(lx as usize, (qy - (MIN_Y >> 2)) as usize, lz as usize);
                return JavaBiome::from_index(native).expect("Java-style quart biome");
            }
            *border
                .entry((qx, qy, qz))
                .or_insert_with(|| sample_biome(noises, qx, qy, qz))
        })
    }

    fn height_at(&self, x: u8, z: u8) -> i32 {
        // Java's heightmap stores the first free y above the top block.
        self.heights[column_index(x, z)] as i32 + 1
    }
}

impl SurfaceEnv for ColumnEnv<'_> {
    fn block_y(&self) -> i32 {
        self.y
    }

    fn surface_depth(&self) -> i32 {
        self.surface_depth
    }

    fn surface_secondary(&mut self) -> f64 {
        let (noises, x, z) = (self.noises, self.x, self.z);
        *self.surface_secondary.get_or_insert_with(|| {
            noises.sample(NoiseRef::SurfaceSecondary, x as f64, 0.0, z as f64)
        })
    }

    fn stone_depth_above(&self) -> i32 {
        self.stone_depth_above
    }

    fn stone_depth_below(&self) -> i32 {
        self.stone_depth_below
    }

    fn water_height(&self) -> i32 {
        self.water_height
    }

    fn biome(&mut self) -> JavaBiome {
        if let Some(biome) = self.biome_cache {
            return biome;
        }
        let biome = self.biome_at(self.y);
        self.biome_cache = Some(biome);
        biome
    }

    fn noise(&mut self, noise: NoiseRef) -> f64 {
        let (noises, x, z) = (self.noises, self.x, self.z);
        *self.noise_cache[noise as usize]
            .get_or_insert_with(|| noises.sample(noise, x as f64, 0.0, z as f64))
    }

    fn gradient_float(&mut self, gradient: usize) -> f32 {
        self.system.gradient_randoms[gradient]
            .at(self.x, self.y, self.z)
            .next_float()
    }

    fn steep(&mut self) -> bool {
        if let Some(steep) = self.steep {
            return steep;
        }
        let (x, z) = (self.local_x, self.local_z);
        let north = self.height_at(x, z.saturating_sub(1));
        let south = self.height_at(x, (z + 1).min(15));
        let steep = if south >= north + 4 {
            true
        } else {
            let west = self.height_at(x.saturating_sub(1), z);
            let east = self.height_at((x + 1).min(15), z);
            west >= east + 4
        };
        self.steep = Some(steep);
        steep
    }

    fn min_surface_level(&mut self) -> i32 {
        if let Some(level) = self.min_surface_level {
            return level;
        }
        let fx = (self.x & 15) as f32 / 16.0;
        let fz = (self.z & 15) as f32 / 16.0;
        let [c00, c10, c01, c11] = *self.corners;
        let lerp = |t: f64, a: f64, b: f64| a + t * (b - a);
        let level = lerp(
            fz as f64,
            lerp(fx as f64, c00, c10),
            lerp(fx as f64, c01, c11),
        )
        .floor() as i32
            + self.surface_depth
            - 8;
        self.min_surface_level = Some(level);
        level
    }

    fn cold_enough_to_snow(&mut self) -> bool {
        let biome = self.biome();
        biome_climate::cold_enough_to_snow(
            biome,
            self.x,
            self.y,
            self.z,
            overworld_settings::SEA_LEVEL,
        )
    }

    fn band(&mut self) -> u32 {
        self.system.band(self.noises, self.x, self.y, self.z)
    }
}
