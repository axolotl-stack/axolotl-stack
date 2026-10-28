//! Java's surface pass over a proto chunk.
//!
//! Per column, from the generation surface (`WORLD_SURFACE_WG` + 1) down to the bottom:
//! air resets the stone depth, fluids set the water height, and every default-block
//! (stone) position is offered to the compiled surface rule with the column's stone depth
//! above/below, water height, surface depth and lazily sampled context. Eroded badlands
//! columns first grow stone pillars; frozen-ocean columns get icebergs afterwards.

use unastar_noise::java_biomes::JavaBiome;
use unastar_noise::surface::{BAND_COUNT, SurfaceEnv, clay_bands};
use unastar_noise::xoroshiro::{JavaRandom, PositionalRandomFactory, get_seed};
use unastar_noise::{
    NoiseRef, NoiseSource, biome_climate, block_class, end_settings, end_surface,
    java_blocks, nether_settings, nether_surface, overworld_settings, overworld_surface,
    random_hashes,
};

use crate::world::ChunkPos;
use crate::world::generator::JavaDimension;
use crate::world::generator::proto::DenseBlocks;

/// The rule implementation is chosen outside the voxel loop.
trait Preset {
    const MIN_Y: i32;
    const HEIGHT: i32;
    const SEA_LEVEL: i32;
    const DEFAULT_BLOCK: u32;
    const PACKED_ICE: u32;
    const SNOW_BLOCK: u32;
    const LEGACY_BIOME_INITIALIZATION: bool;
    fn apply<E: SurfaceEnv>(env: &mut E) -> Option<u32>;
}

macro_rules! preset {
    ($name:ident, $settings:ident, $rules:ident) => {
        struct $name;
        impl Preset for $name {
            const MIN_Y: i32 = $settings::MIN_Y;
            const HEIGHT: i32 = $settings::HEIGHT;
            const SEA_LEVEL: i32 = $settings::SEA_LEVEL;
            const DEFAULT_BLOCK: u32 = $settings::DEFAULT_BLOCK;
            const PACKED_ICE: u32 = $rules::PACKED_ICE;
            const SNOW_BLOCK: u32 = $rules::SNOW_BLOCK;
            const LEGACY_BIOME_INITIALIZATION: bool = $settings::LEGACY_RANDOM_SOURCE;
            #[inline]
            fn apply<E: SurfaceEnv>(env: &mut E) -> Option<u32> {
                $rules::apply(env)
            }
        }
    };
}
preset!(Overworld, overworld_settings, overworld_surface);
preset!(Nether, nether_settings, nether_surface);
preset!(End, end_settings, end_surface);

/// Surface and rule-gradient factories use the preset's random-source algorithm.
enum SurfaceRandom {
    Xoroshiro(PositionalRandomFactory),
    Legacy(i64),
}

impl SurfaceRandom {
    fn new(seed: i64, legacy: bool) -> Self {
        if legacy {
            Self::Legacy(JavaRandom::from_seed(seed).next_long())
        } else {
            Self::Xoroshiro(PositionalRandomFactory::new(seed))
        }
    }

    fn fork(&self, hash: (u64, u64), java_hash: i32) -> Self {
        match self {
            Self::Xoroshiro(factory) => Self::Xoroshiro(factory.fork_from_hash(hash)),
            Self::Legacy(seed) => {
                Self::Legacy(JavaRandom::from_seed(*seed ^ java_hash as i64).next_long())
            }
        }
    }

    fn double_at(&self, x: i32, y: i32, z: i32) -> f64 {
        match self {
            Self::Xoroshiro(factory) => factory.at(x, y, z).next_double(),
            Self::Legacy(seed) => JavaRandom::from_seed(*seed ^ get_seed(x, y, z)).next_double(),
        }
    }

    fn float_at(&self, x: i32, y: i32, z: i32) -> f32 {
        match self {
            Self::Xoroshiro(factory) => factory.at(x, y, z).next_float(),
            Self::Legacy(seed) => JavaRandom::from_seed(*seed ^ get_seed(x, y, z)).next_float(),
        }
    }
}

enum ColumnRandom {
    Xoroshiro(unastar_noise::xoroshiro::Xoroshiro128),
    Legacy(JavaRandom),
}

impl ColumnRandom {
    fn next_int(&mut self, bound: u32) -> u32 {
        match self {
            Self::Xoroshiro(random) => random.next_int(bound),
            Self::Legacy(random) => random.next_int(bound) as u32,
        }
    }

    fn next_double(&mut self) -> f64 {
        match self {
            Self::Xoroshiro(random) => random.next_double(),
            Self::Legacy(random) => random.next_double(),
        }
    }
}

#[inline]
fn class(block: u32) -> u8 {
    java_blocks::JAVA_STATE_CLASSES
        .get(block as usize)
        .copied()
        .expect("surface pass requires canonical Java block states")
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
    dimension: JavaDimension,
    noise_random: SurfaceRandom,
    gradient_randoms: Vec<SurfaceRandom>,
    /// Only the Overworld's compiled rule consumes clay bands or iceberg randomness.
    clay_bands: Option<[u32; BAND_COUNT]>,
}

impl SurfaceSystem {
    pub fn new(seed: i64, dimension: JavaDimension) -> Self {
        let (legacy, hashes, java_hashes, bands) = match dimension {
            JavaDimension::Overworld => (
                overworld_settings::LEGACY_RANDOM_SOURCE,
                overworld_surface::GRADIENT_HASHES.as_slice(),
                overworld_surface::GRADIENT_JAVA_HASHES.as_slice(),
                &overworld_surface::BAND_BLOCKS,
            ),
            JavaDimension::Nether => (
                nether_settings::LEGACY_RANDOM_SOURCE,
                nether_surface::GRADIENT_HASHES.as_slice(),
                nether_surface::GRADIENT_JAVA_HASHES.as_slice(),
                &nether_surface::BAND_BLOCKS,
            ),
            JavaDimension::End => (
                end_settings::LEGACY_RANDOM_SOURCE,
                end_surface::GRADIENT_HASHES.as_slice(),
                end_surface::GRADIENT_JAVA_HASHES.as_slice(),
                &end_surface::BAND_BLOCKS,
            ),
        };
        let noise_random = SurfaceRandom::new(seed, legacy);
        let gradient_randoms = hashes.iter().zip(java_hashes)
            .map(|(&hash, &java_hash)| noise_random.fork(hash, java_hash))
            .collect();
        let clay_bands = if let SurfaceRandom::Xoroshiro(factory) = &noise_random {
            let mut random = factory.from_hash(random_hashes::CLAY_BANDS);
            Some(clay_bands(&mut random, bands))
        } else {
            None
        };
        Self { dimension, noise_random, gradient_randoms, clay_bands }
    }

    /// Apply the selected preset's compiled surface rules. `corners` contains floored
    /// preliminary levels at (0,0), (16,0), (0,16), (16,16). The resolver supplies the
    /// edition-owned fuzzy biome at absolute block coordinates, including chunk borders.
    pub fn build_surface<N: NoiseSource, B: FnMut(i32, i32, i32) -> JavaBiome>(
        &self,
        chunk: &mut DenseBlocks,
        pos: ChunkPos,
        noises: &N,
        corners: [f64; 4],
        biome_at: &mut B,
    ) {
        match self.dimension {
            JavaDimension::Overworld => self.build_surface_for::<Overworld, N, B>(chunk, pos, noises, corners, biome_at),
            JavaDimension::Nether => self.build_surface_for::<Nether, N, B>(chunk, pos, noises, corners, biome_at),
            JavaDimension::End => self.build_surface_for::<End, N, B>(chunk, pos, noises, corners, biome_at),
        }
    }

    fn build_surface_for<P: Preset, N: NoiseSource, B: FnMut(i32, i32, i32) -> JavaBiome>(
        &self,
        chunk: &mut DenseBlocks,
        pos: ChunkPos,
        noises: &N,
        corners: [f64; 4],
        biome_at: &mut B,
    ) {
        let mut heights = column_heights::<P>(chunk);

        for local_x in 0..16u8 {
            for local_z in 0..16u8 {
                let x = pos.x * 16 + local_x as i32;
                let z = pos.z * 16 + local_z as i32;
                let top = heights[column_index(local_x, local_z)] as i32 + 1;
                let mut env = ColumnEnv {
                    system: self,
                    noises,
                    biome_at: &mut *biome_at,
                    sea_level: P::SEA_LEVEL,
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
                let column_biome = env.biome_at(if P::LEGACY_BIOME_INITIALIZATION { 0 } else { top });
                if column_biome == JavaBiome::ErodedBadlands {
                    self.eroded_badlands_extension::<P, N>(chunk, noises, local_x, local_z, x, z, top);
                }
                let top = top_non_air::<P>(chunk, local_x, local_z) + 1;

                let mut stone_depth_above = 0;
                let mut water_height = i32::MIN;
                let mut solid_below_until = i32::MAX;
                let mut y = top;
                while y >= P::MIN_Y {
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
                        while below >= P::MIN_Y - 1 {
                            let solid = below >= P::MIN_Y
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
                    if block == P::DEFAULT_BLOCK
                        && let Some(replacement) = P::apply(&mut env)
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
                    self.frozen_ocean_extension::<P, N>(
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
                    top_non_air::<P>(chunk, local_x, local_z) as i16;
            }
        }
    }

    /// Java's carver `topMaterial`: the surface rule's block for exposed dirt at
    /// `(x, y, z)` under a carved surface block, with stone depth 1 above and below.
    #[allow(clippy::too_many_arguments)]
    pub fn top_material<N: NoiseSource, B: FnMut(i32, i32, i32) -> JavaBiome>(
        &self,
        chunk: &DenseBlocks,
        pos: ChunkPos,
        noises: &N,
        corners: [f64; 4],
        biome_at: &mut B,
        x: i32,
        y: i32,
        z: i32,
        has_fluid: bool,
    ) -> Option<u32> {
        match self.dimension {
            JavaDimension::Overworld => self.top_material_for::<Overworld, N, B>(chunk, pos, noises, corners, biome_at, x, y, z, has_fluid),
            JavaDimension::Nether => self.top_material_for::<Nether, N, B>(chunk, pos, noises, corners, biome_at, x, y, z, has_fluid),
            JavaDimension::End => self.top_material_for::<End, N, B>(chunk, pos, noises, corners, biome_at, x, y, z, has_fluid),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn top_material_for<P: Preset, N: NoiseSource, B: FnMut(i32, i32, i32) -> JavaBiome>(
        &self,
        chunk: &DenseBlocks,
        _pos: ChunkPos,
        noises: &N,
        corners: [f64; 4],
        biome_at: &mut B,
        x: i32,
        y: i32,
        z: i32,
        has_fluid: bool,
    ) -> Option<u32> {
        let heights = column_heights::<P>(chunk);
        let mut env = ColumnEnv {
            system: self,
            noises,
            biome_at: &mut *biome_at,
            sea_level: P::SEA_LEVEL,
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
        P::apply(&mut env)
    }

    /// `(int)(surface * 2.75 + 3 + random(x, 0, z) * 0.25)`.
    fn surface_depth<N: NoiseSource>(&self, noises: &N, x: i32, z: i32) -> i32 {
        let noise = noises.sample(NoiseRef::Surface, x as f64, 0.0, z as f64);
        let jitter = self.noise_random.double_at(x, 0, z);
        (noise * 2.75 + 3.0 + jitter * 0.25) as i32
    }

    fn band<N: NoiseSource>(&self, noises: &N, x: i32, y: i32, z: i32) -> u32 {
        let offset = (noises.sample(NoiseRef::ClayBandsOffset, x as f64, 0.0, z as f64) * 4.0 + 0.5)
            .floor() as i32;
        self.clay_bands.as_ref().expect("preset surface rule has no clay bands")
            [(y + offset).rem_euclid(BAND_COUNT as i32) as usize]
    }

    /// Stone pillars of eroded badlands, grown from y=64 up to a noise-driven height.
    #[allow(clippy::too_many_arguments)]
    fn eroded_badlands_extension<P: Preset, N: NoiseSource>(
        &self,
        chunk: &mut DenseBlocks,
        noises: &N,
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
        while y >= P::MIN_Y {
            let block = chunk.get_block(local_x, y as i16, local_z);
            if block == P::DEFAULT_BLOCK {
                break;
            }
            if is_water(block) {
                return;
            }
            y -= 1;
        }
        let mut y = pillar_top;
        while y >= P::MIN_Y && is_air(chunk.get_block(local_x, y as i16, local_z)) {
            chunk.set_block(
                local_x,
                y as i16,
                local_z,
                P::DEFAULT_BLOCK,
            );
            y -= 1;
        }
    }

    /// Packed-ice icebergs with snow caps in frozen oceans.
    #[allow(clippy::too_many_arguments)]
    fn frozen_ocean_extension<P: Preset, N: NoiseSource>(
        &self,
        chunk: &mut DenseBlocks,
        noises: &N,
        biome: JavaBiome,
        min_surface_level: i32,
        local_x: u8,
        local_z: u8,
        x: i32,
        z: i32,
        height: i32,
    ) {
        let sea_level = P::SEA_LEVEL;
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
        let mut random = match &self.noise_random {
            SurfaceRandom::Xoroshiro(factory) => ColumnRandom::Xoroshiro(factory.at(x, 0, z)),
            SurfaceRandom::Legacy(seed) => ColumnRandom::Legacy(JavaRandom::from_seed(*seed ^ get_seed(x, 0, z))),
        };
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
                    chunk.set_block(local_x, y as i16, local_z, P::SNOW_BLOCK);
                    snow_placed += 1;
                } else {
                    chunk.set_block(local_x, y as i16, local_z, P::PACKED_ICE);
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
fn column_heights<P: Preset>(chunk: &DenseBlocks) -> [i16; 256] {
    let mut heights = [P::MIN_Y as i16; 256];
    for z in 0..16u8 {
        for x in 0..16u8 {
            heights[column_index(x, z)] = top_non_air::<P>(chunk, x, z) as i16;
        }
    }
    heights
}

/// y of the highest non-air block, or one below the preset's generation volume.
fn top_non_air<P: Preset>(chunk: &DenseBlocks, x: u8, z: u8) -> i32 {
    let mut y = P::MIN_Y + P::HEIGHT - 1;
    while y >= P::MIN_Y {
        if !is_air(chunk.get_block(x, y as i16, z)) {
            return y;
        }
        y -= 1;
    }
    P::MIN_Y - 1
}

/// [`SurfaceEnv`] for one column of the pass.
struct ColumnEnv<'a, N, B> {
    system: &'a SurfaceSystem,
    noises: &'a N,
    biome_at: &'a mut B,
    sea_level: i32,
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

impl<N: NoiseSource, B: FnMut(i32, i32, i32) -> JavaBiome> ColumnEnv<'_, N, B> {
    fn biome_at(&mut self, y: i32) -> JavaBiome {
        (self.biome_at)(self.x, y, self.z)
    }

    fn height_at(&self, x: u8, z: u8) -> i32 {
        // Java's heightmap stores the first free y above the top block.
        self.heights[column_index(x, z)] as i32 + 1
    }
}

impl<N: NoiseSource, B: FnMut(i32, i32, i32) -> JavaBiome> SurfaceEnv for ColumnEnv<'_, N, B> {
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
        self.system.gradient_randoms[gradient].float_at(self.x, self.y, self.z)
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
            self.sea_level,
        )
    }

    fn band(&mut self) -> u32 {
        self.system.band(self.noises, self.x, self.y, self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::BlockStateSpace;
    use crate::world::generator::{JavaGenerationOptions, JavaGenerator, JavaTerrainStage};

    #[test]
    fn native_badlands_bands_reach_surface_consumer() {
        // Immutable native full-stage SURFACE capture, seed -734287144591,
        // chunk (100,-100), structures=false; canonical Java state properties.
        // This exercises real noise, biome lookup and compiled rule dispatch,
        // not just a separately constructed colour table.
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                let generated = JavaGenerator::with_options(
                    -734287144591,
                    JavaGenerationOptions {
                        dimension: JavaDimension::Overworld,
                        generate_structures: false,
                        biome_features: false,
                    },
                )
                .generate_stage(ChunkPos::new(100, -100), JavaTerrainStage::Surface);
                assert_eq!(generated.chunk.state_space(), BlockStateSpace::Java);
                for (x, y, z, expected) in [
                    (0, 75, 0, "minecraft:light_gray_terracotta"),
                    (0, 76, 0, "minecraft:brown_terracotta"),
                    (0, 77, 0, "minecraft:brown_terracotta"),
                    (0, 80, 0, "minecraft:terracotta"),
                    (0, 83, 0, "minecraft:yellow_terracotta"),
                    (0, 93, 0, "minecraft:white_terracotta"),
                    (0, 98, 0, "minecraft:air"),
                    (15, 75, 15, "minecraft:light_gray_terracotta"),
                    (15, 82, 15, "minecraft:terracotta"),
                    (15, 98, 15, "minecraft:air"),
                    (7, 89, 8, "minecraft:terracotta"),
                    (7, 95, 8, "minecraft:terracotta"),
                ] {
                    assert_eq!(
                        generated.chunk.get_block(x, y, z),
                        java_blocks::default_state(expected).unwrap(),
                        "native badlands SURFACE block=({},{y},{})",
                        1600 + i32::from(x),
                        -1600 + i32::from(z),
                    );
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn native_dimension_surface_boundaries() {
        // Pinned 1.21.11 actual NOISE -> SURFACE captures, seed 0 / chunk (0,0):
        // native-surface-corpus-20261027/surface-{nether,end}-seed0-smoke.json.
        // Native generator, registries, tags and nine-chunk biome neighborhood;
        // unblended, no structures, carvers, features or ticks. These are native
        // consumer-visible block states, not hashes of this implementation.
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                let options = |dimension| JavaGenerationOptions {
                    dimension,
                    generate_structures: false,
                    biome_features: false,
                };
                let bedrock = java_blocks::default_state("minecraft:bedrock").unwrap();
                let air = java_blocks::default_state("minecraft:air").unwrap();
                let nether = JavaGenerator::with_options(0, options(JavaDimension::Nether))
                    .generate_stage(ChunkPos::new(0, 0), JavaTerrainStage::Surface);
                assert_eq!(nether.chunk.state_space(), BlockStateSpace::Java);
                let ys = [0, 1, 2, 3, 4, 5, 122, 123, 124, 125, 126, 127];
                for (x, z, expected_bedrock) in [
                    (0, 0, [true, true, true, true, false, false, false, false, true, true, false, true]),
                    (15, 15, [true, true, false, false, false, false, false, false, true, false, true, true]),
                ] {
                    for (y, is_bedrock) in ys.into_iter().zip(expected_bedrock) {
                        assert_eq!(
                            nether.chunk.get_block(x, y, z),
                            if is_bedrock { bedrock } else { nether_settings::DEFAULT_BLOCK },
                            "native Nether SURFACE seed=0 block=({x},{y},{z})"
                        );
                    }
                    assert_eq!(nether.chunk.get_block(x, 128, z), air);
                    assert_eq!(nether.chunk.get_block(x, 255, z), air);
                }

                let end = JavaGenerator::with_options(0, options(JavaDimension::End))
                    .generate_stage(ChunkPos::new(0, 0), JavaTerrainStage::Surface);
                assert_eq!(end.chunk.state_space(), BlockStateSpace::Java);
                for (x, z, top) in [(0, 0, 62), (15, 15, 64)] {
                    assert_eq!(end.chunk.get_block(x, 0, z), air);
                    for y in [32, 48, 60, top] {
                        assert_eq!(
                            end.chunk.get_block(x, y, z),
                            end_settings::DEFAULT_BLOCK,
                            "native End SURFACE seed=0 block=({x},{y},{z})"
                        );
                    }
                    for y in [top + 1, 127, 128, 255] {
                        assert_eq!(end.chunk.get_block(x, y, z), air);
                    }
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
