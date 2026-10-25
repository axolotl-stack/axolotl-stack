//! Bedrock pipeline assembly on the shared noise core.
//!
//! **unmeasured: to confirm by BDS fixture**: Java climate/seeding is reused
//! unchanged until reference worlds establish the required Bedrock deviations.
//! This is not a claim of Bedrock terrain, biome or structure parity.

use super::BiomeSet;
use super::adapter::RegionWorld;
use super::decorate::{Decorator, chunk_seed};
use super::generated::{self, bb, props};
use super::rng::BedrockRandom;
use super::surface::{self, ColumnInput, SurfaceColumn, SurfaceNoises};
use crate::world::ChunkPos;
use crate::world::chunk::{Chunk, MIN_Y};
use crate::world::generator::pipeline::ChunkCache;
use crate::world::generator::proto::{DenseBlocks, QUARTS_Y, WORLD_HEIGHT};
use crate::world::generator::{
    DecorationRegion, Generator, JavaGenerator, NativeBiome, ProtoChunk, QuartBiomes, SpawnPoint,
    WorldGenRegion,
};
use unastar_noise::java_biomes::JavaBiome;

/// BDS-data-driven surfaces/carvers/features over the shared noise terrain.
pub struct BedrockGenerator {
    core: JavaGenerator,
    surface_noises: SurfaceNoises,
    decorator: Decorator,
    // Height arrays, not full raw chunks: bounded memory while allowing distant
    // structure queries. Cache eviction affects work only, never generated output.
    noise_heights: ChunkCache<[i32; 256]>,
    surface_heights: ChunkCache<[i32; 256]>,
}

impl BedrockGenerator {
    pub fn new(seed: i64) -> Self {
        Self {
            core: JavaGenerator::new(seed),
            surface_noises: SurfaceNoises::new(seed),
            decorator: Decorator::new(seed),
            noise_heights: ChunkCache::new(128),
            surface_heights: ChunkCache::new(128),
        }
    }

    fn bedrock_id(native: NativeBiome) -> u16 {
        JavaBiome::from_index(native)
            .expect("shared noise biomes store JavaBiome discriminants")
            .data()
            .bedrock_id
            .try_into()
            .expect("Bedrock biome id fits u16")
    }

    fn sample_biome(&self, x: i32, y: i32, z: i32) -> u16 {
        Self::bedrock_id(self.core.sample_biome(x, y, z))
    }

    fn raw_height(&self, x: i32, z: i32) -> i32 {
        let pos = ChunkPos::new(x >> 4, z >> 4);
        self.noise_heights
            .get_or_compute(pos, || self.core.noise_heightmap(pos))
            [((z & 15) * 16 + (x & 15)) as usize]
    }

    fn surface_height(&self, x: i32, z: i32) -> i32 {
        let pos = ChunkPos::new(x >> 4, z >> 4);
        self.surface_heights.get_or_compute(pos, || {
            let (blocks, _) = self.surface_blocks(pos);
            column_heights(&blocks, |id| id != bb::AIR)
        })[((z & 15) * 16 + (x & 15)) as usize]
    }

    fn surface_blocks(&self, pos: ChunkPos) -> (DenseBlocks, QuartBiomes) {
        let (mut blocks, mut biomes) = self.core.generate_noise_blocks(pos);
        for qy in 0..QUARTS_Y {
            for qz in 0..4 {
                for qx in 0..4 {
                    biomes.set(qx, qy, qz, Self::bedrock_id(biomes.get(qx, qy, qz)));
                }
            }
        }
        let heights = self
            .noise_heights
            .get_or_compute(pos, || column_heights(&blocks, props::blocks_motion));
        let mut rng = BedrockRandom::new(chunk_seed(self.seed(), pos.x, pos.z));
        for z in 0..16u8 {
            for x in 0..16u8 {
                let (wx, wz) = (pos.x * 16 + i32::from(x), pos.z * 16 + i32::from(z));
                let top = heights[z as usize * 16 + x as usize];
                let surface_id = biomes.at_block(x, top - 1, z);
                let biome = generated::biome(surface_id)
                    .expect("shared overworld biome has BDS surface data");
                let neighbor = |dx: i32, dz: i32| {
                    let (lx, lz) = (i32::from(x) + dx, i32::from(z) + dz);
                    if (0..16).contains(&lx) && (0..16).contains(&lz) {
                        heights[(lz * 16 + lx) as usize] - 1
                    } else {
                        self.raw_height(wx + dx, wz + dz) - 1
                    }
                };
                // Only steep-slope builders need neighboring terrain. Do not
                // generate adjacent heightmaps for ordinary surface columns.
                let steep = biome.mountain.is_some_and(|m| m.steep_material.is_some());
                let neighbor_heights = if steep {
                    [
                        neighbor(0, -1),
                        neighbor(0, 1),
                        neighbor(-1, 0),
                        neighbor(1, 0),
                    ]
                } else {
                    [top - 1; 4]
                };
                let input = ColumnInput {
                    x: wx,
                    z: wz,
                    biome,
                    sea_level: JavaGenerator::SEA_LEVEL,
                    neighbour_heights: neighbor_heights,
                };
                let mut col = Column {
                    blocks: &mut blocks,
                    x,
                    z,
                };
                surface::build_column(&mut col, &input, &self.surface_noises, &mut rng);
                // Cave biome subsurface builders operate on their own quart
                // ranges; a surface biome must not erase 3D cave identity.
                let mut qy = 0;
                while qy < QUARTS_Y && MIN_Y + (qy as i32) * 4 < top - 1 {
                    let id = biomes.get(x as usize >> 2, qy, z as usize >> 2);
                    let start = qy;
                    qy += 1;
                    while qy < QUARTS_Y && biomes.get(x as usize >> 2, qy, z as usize >> 2) == id {
                        qy += 1;
                    }
                    if id != surface_id {
                        let cave =
                            generated::biome(id).expect("shared cave biome has BDS surface data");
                        let cave_input = ColumnInput {
                            biome: cave,
                            ..input
                        };
                        surface::build_range(
                            &mut col,
                            &cave_input,
                            &self.surface_noises,
                            &mut rng,
                            MIN_Y + start as i32 * 4,
                            (MIN_Y + qy as i32 * 4).min(top - 1),
                        );
                    }
                }
            }
        }
        (blocks, biomes)
    }
}

fn column_heights(blocks: &DenseBlocks, counts: impl Fn(u32) -> bool) -> [i32; 256] {
    std::array::from_fn(|i| {
        let (x, z) = ((i % 16) as u8, (i / 16) as u8);
        (MIN_Y..MIN_Y + WORLD_HEIGHT)
            .rev()
            .find(|&y| counts(blocks.get_block(x, y as i16, z)))
            .map_or(MIN_Y, |y| y + 1)
    })
}

struct Column<'a> {
    blocks: &'a mut DenseBlocks,
    x: u8,
    z: u8,
}
impl SurfaceColumn for Column<'_> {
    fn min_y(&self) -> i32 {
        MIN_Y
    }
    fn max_y(&self) -> i32 {
        MIN_Y + WORLD_HEIGHT
    }
    fn get(&self, y: i32) -> u32 {
        self.blocks.get_block(self.x, y as i16, self.z)
    }
    fn set(&mut self, y: i32, id: u32) {
        self.blocks.set_block(self.x, y as i16, self.z, id);
    }
}

impl Generator for BedrockGenerator {
    fn seed(&self) -> i64 {
        self.core.seed
    }

    fn generate_proto(&self, pos: ChunkPos) -> ProtoChunk {
        let (blocks, biomes) = self.surface_blocks(pos);
        self.surface_heights
            .get_or_compute(pos, || column_heights(&blocks, |id| id != bb::AIR));
        let chunk = blocks.to_chunk(pos, bb::AIR);
        let mut set = BiomeSet::default();
        for &id in biomes.as_slice() {
            set.insert(id);
        }
        let mut proto = ProtoChunk { pos, chunk, biomes };
        self.decorator.carve(&mut proto, pos.x, pos.z, &set);
        proto
    }

    fn decorates(&self) -> bool {
        true
    }

    fn decorate(&self, region: &mut DecorationRegion<'_>) {
        let origin = region.origin();
        let mut set = BiomeSet::default();
        for qy in 0..QUARTS_Y {
            for qz in 0..4 {
                for qx in 0..4 {
                    set.insert(region.biome_at(
                        origin.x * 4 + qx,
                        (MIN_Y >> 2) + qy as i32,
                        origin.z * 4 + qz,
                    ));
                }
            }
        }
        let mut world = RegionWorld::new(region);
        self.decorator.structures(
            &mut world,
            origin.x,
            origin.z,
            &|x, z| self.surface_height(x, z),
            &|x, y, z| self.sample_biome(x, y, z),
        );
        self.decorator
            .decorate(&mut world, origin.x, origin.z, &set);
    }

    fn block_classes(&self) -> &'static [u8] {
        &props::STATE_CLASSES
    }
    fn bedrock_biome(&self, biome: NativeBiome) -> u32 {
        u32::from(biome)
    }

    fn find_spawn(&self, chunk_at: &dyn Fn(ChunkPos) -> Chunk) -> SpawnPoint {
        // Shared safe-spawn search examines finished Bedrock chunks.
        // BDS spawn-center selection is unmeasured: to confirm by BDS fixture.
        self.core.find_spawn(chunk_at)
    }
}
