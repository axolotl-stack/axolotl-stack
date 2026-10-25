//! [`ChunkRegion`]: a [`BlockWorld`] over the 3x3 chunks around the chunk being
//! decorated, with cached heightmaps.
//!
//! The centre chunk's features may read and write anywhere in the 3x3 area
//! (a tree near a border spills into its neighbour). Positions outside it
//! read as [`OUT_OF_BOUNDS`] and refuse writes.

use super::generated::props;
use super::world::{BlockPos, BlockWorld, OUT_OF_BOUNDS};
use crate::world::chunk::{Chunk, MIN_Y, SUBCHUNK_COUNT};

const SPAN: usize = 48;
const UNKNOWN: i32 = i32::MIN;
const MAX_Y: i32 = MIN_Y + SUBCHUNK_COUNT as i32 * 16;

/// Biome lookup for a region (numeric Bedrock biome id at a block position).
pub trait BiomeSource {
    fn biome(&self, x: i32, y: i32, z: i32) -> u16;
}

impl<F: Fn(i32, i32, i32) -> u16> BiomeSource for F {
    fn biome(&self, x: i32, y: i32, z: i32) -> u16 {
        self(x, y, z)
    }
}

/// The 3x3 chunk neighbourhood of `centre`, row-major from the north-west.
pub struct ChunkRegion<'a, B: BiomeSource> {
    chunks: [&'a mut Chunk; 9],
    /// World x/z of the north-west corner of the region.
    origin_x: i32,
    origin_z: i32,
    biomes: &'a B,
    heightmap: Box<[i32; SPAN * SPAN]>,
    top_solid: Box<[i32; SPAN * SPAN]>,
}

/// `query.heightmap` blockers: anything but air and liquids.
#[inline]
fn blocks_heightmap(id: u32) -> bool {
    id != super::generated::bb::AIR && !props::is_liquid(id)
}

impl<'a, B: BiomeSource> ChunkRegion<'a, B> {
    /// `chunks` are the 3x3 neighbourhood in row-major order (z outer,
    /// x inner, north-west first); the centre is `chunks[4]`.
    pub fn new(chunks: [&'a mut Chunk; 9], biomes: &'a B) -> Self {
        let (cx, cz) = (chunks[4].x, chunks[4].z);
        for (i, c) in chunks.iter().enumerate() {
            let (dx, dz) = ((i % 3) as i32 - 1, (i / 3) as i32 - 1);
            assert!(
                c.x == cx + dx && c.z == cz + dz,
                "region chunk {i} is ({}, {}), expected ({}, {})",
                c.x,
                c.z,
                cx + dx,
                cz + dz
            );
        }
        Self {
            chunks,
            origin_x: (cx - 1) * 16,
            origin_z: (cz - 1) * 16,
            biomes,
            heightmap: Box::new([UNKNOWN; SPAN * SPAN]),
            top_solid: Box::new([UNKNOWN; SPAN * SPAN]),
        }
    }

    #[inline]
    fn column(&self, x: i32, z: i32) -> Option<usize> {
        let (lx, lz) = (x - self.origin_x, z - self.origin_z);
        ((0..SPAN as i32).contains(&lx) && (0..SPAN as i32).contains(&lz))
            .then(|| lz as usize * SPAN + lx as usize)
    }

    #[inline]
    fn locate(&self, pos: BlockPos) -> Option<(usize, u8, u8)> {
        let (lx, lz) = (pos.x - self.origin_x, pos.z - self.origin_z);
        if !(0..SPAN as i32).contains(&lx)
            || !(0..SPAN as i32).contains(&lz)
            || !(MIN_Y..MAX_Y).contains(&pos.y)
        {
            return None;
        }
        let chunk = (lz / 16) as usize * 3 + (lx / 16) as usize;
        Some((chunk, (lx % 16) as u8, (lz % 16) as u8))
    }

    fn scan(&self, x: i32, z: i32, blocks: impl Fn(u32) -> bool) -> i32 {
        for y in (MIN_Y..MAX_Y).rev() {
            if blocks(self.get(BlockPos::new(x, y, z))) {
                return y + 1;
            }
        }
        MIN_Y
    }
}

impl<B: BiomeSource> BlockWorld for ChunkRegion<'_, B> {
    fn min_y(&self) -> i32 {
        MIN_Y
    }

    fn max_y(&self) -> i32 {
        MAX_Y
    }

    #[inline]
    fn get(&self, pos: BlockPos) -> u32 {
        match self.locate(pos) {
            Some((chunk, x, z)) => self.chunks[chunk].get_block(x, pos.y as i16, z),
            None => OUT_OF_BOUNDS,
        }
    }

    fn set(&mut self, pos: BlockPos, id: u32) -> bool {
        let Some((chunk, x, z)) = self.locate(pos) else {
            return false;
        };
        self.chunks[chunk].set_block(x, pos.y as i16, z, id);
        let col = self.column(pos.x, pos.z).expect("located");
        // Keep the cached heights exact: raise on a new top block, forget the
        // column when its top block is removed.
        for (cache, blocks) in [
            (&mut self.heightmap[col], blocks_heightmap(id)),
            (&mut self.top_solid[col], props::is_solid(id)),
        ] {
            if *cache == UNKNOWN {
                continue;
            }
            if blocks && pos.y + 1 > *cache {
                *cache = pos.y + 1;
            } else if !blocks && pos.y + 1 == *cache {
                *cache = UNKNOWN;
            }
        }
        true
    }

    fn biome(&self, pos: BlockPos) -> u16 {
        self.biomes.biome(pos.x, pos.y, pos.z)
    }

    fn heightmap(&mut self, x: i32, z: i32) -> i32 {
        let Some(col) = self.column(x, z) else {
            return MIN_Y;
        };
        if self.heightmap[col] == UNKNOWN {
            self.heightmap[col] = self.scan(x, z, blocks_heightmap);
        }
        self.heightmap[col]
    }

    fn above_top_solid(&mut self, x: i32, z: i32) -> i32 {
        let Some(col) = self.column(x, z) else {
            return MIN_Y;
        };
        if self.top_solid[col] == UNKNOWN {
            self.top_solid[col] = self.scan(x, z, props::is_solid);
        }
        self.top_solid[col]
    }
}
