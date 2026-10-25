//! Bedrock block queries over the shared proto/decorating pipeline.
//!
//! Unlike `WorldGenRegion`, Bedrock features treat horizontal out-of-region
//! reads as a nonmatching sentinel. Keep that boundary check before delegating.

use super::generated::{bb, props};
use super::world::{BlockPos, BlockWorld, OUT_OF_BOUNDS};
use crate::world::chunk::MIN_Y;
use crate::world::generator::proto::WORLD_HEIGHT;
use crate::world::generator::{ProtoChunk, WorldGenRegion};

const SPAN: usize = 48;
const UNKNOWN: i16 = i16::MIN;

/// Mutable region adapter; cached query heights are invalidated by every write.
pub struct RegionWorld<'a, R: WorldGenRegion> {
    region: &'a mut R,
    min_x: i32,
    min_z: i32,
    heightmap: Box<[i16; SPAN * SPAN]>,
    top_solid: Box<[i16; SPAN * SPAN]>,
}

impl<'a, R: WorldGenRegion> RegionWorld<'a, R> {
    pub fn new(region: &'a mut R) -> Self {
        let origin = region.origin();
        Self {
            region,
            min_x: (origin.x - 1) * 16,
            min_z: (origin.z - 1) * 16,
            heightmap: Box::new([UNKNOWN; SPAN * SPAN]),
            top_solid: Box::new([UNKNOWN; SPAN * SPAN]),
        }
    }

    fn column(&self, x: i32, z: i32) -> Option<usize> {
        let (x, z) = (x - self.min_x, z - self.min_z);
        ((0..SPAN as i32).contains(&x) && (0..SPAN as i32).contains(&z))
            .then(|| z as usize * SPAN + x as usize)
    }

    fn scan(&self, x: i32, z: i32, solid: bool) -> i32 {
        for y in (self.min_y()..self.max_y()).rev() {
            let id = self.region.get_block(x, y, z);
            if if solid {
                props::is_solid(id)
            } else {
                id != bb::AIR && !props::is_liquid(id)
            } {
                return y + 1;
            }
        }
        self.min_y()
    }
}

impl<R: WorldGenRegion> BlockWorld for RegionWorld<'_, R> {
    fn min_y(&self) -> i32 {
        self.region.min_y()
    }
    fn max_y(&self) -> i32 {
        self.region.min_y() + self.region.height()
    }

    fn get(&self, p: BlockPos) -> u32 {
        if self.column(p.x, p.z).is_none() || !(self.min_y()..self.max_y()).contains(&p.y) {
            OUT_OF_BOUNDS
        } else {
            self.region.get_block(p.x, p.y, p.z)
        }
    }

    fn set(&mut self, p: BlockPos, id: u32) -> bool {
        let Some(col) = self.column(p.x, p.z) else {
            return false;
        };
        if !(self.min_y()..self.max_y()).contains(&p.y) || !self.region.set_block(p.x, p.y, p.z, id)
        {
            return false;
        }
        self.heightmap[col] = UNKNOWN;
        self.top_solid[col] = UNKNOWN;
        true
    }

    fn biome(&self, p: BlockPos) -> u16 {
        if self.column(p.x, p.z).is_none() {
            return u16::MAX;
        }
        self.region.biome_at(p.x >> 2, p.y >> 2, p.z >> 2)
    }

    fn heightmap(&mut self, x: i32, z: i32) -> i32 {
        let Some(col) = self.column(x, z) else {
            return self.min_y();
        };
        if self.heightmap[col] == UNKNOWN {
            self.heightmap[col] = self.scan(x, z, false) as i16;
        }
        i32::from(self.heightmap[col])
    }

    fn above_top_solid(&mut self, x: i32, z: i32) -> i32 {
        let Some(col) = self.column(x, z) else {
            return self.min_y();
        };
        if self.top_solid[col] == UNKNOWN {
            self.top_solid[col] = self.scan(x, z, true) as i16;
        }
        i32::from(self.top_solid[col])
    }
}

/// Pregeneration carvers may write only their own proto, not its neighbours.
impl BlockWorld for ProtoChunk {
    fn min_y(&self) -> i32 {
        MIN_Y
    }
    fn max_y(&self) -> i32 {
        MIN_Y + WORLD_HEIGHT
    }

    fn get(&self, p: BlockPos) -> u32 {
        if p.x >> 4 != self.pos.x
            || p.z >> 4 != self.pos.z
            || !(self.min_y()..self.max_y()).contains(&p.y)
        {
            OUT_OF_BOUNDS
        } else {
            self.chunk
                .get_block((p.x & 15) as u8, p.y as i16, (p.z & 15) as u8)
        }
    }

    fn set(&mut self, p: BlockPos, id: u32) -> bool {
        if p.x >> 4 != self.pos.x
            || p.z >> 4 != self.pos.z
            || !(self.min_y()..self.max_y()).contains(&p.y)
        {
            return false;
        }
        self.chunk
            .set_block((p.x & 15) as u8, p.y as i16, (p.z & 15) as u8, id);
        true
    }

    fn biome(&self, p: BlockPos) -> u16 {
        if p.x >> 4 != self.pos.x || p.z >> 4 != self.pos.z {
            return u16::MAX;
        }
        self.biomes
            .at_block((p.x & 15) as u8, p.y, (p.z & 15) as u8)
    }

    fn heightmap(&mut self, x: i32, z: i32) -> i32 {
        if x >> 4 != self.pos.x || z >> 4 != self.pos.z {
            return self.min_y();
        }
        (self.min_y()..self.max_y())
            .rev()
            .find(|&y| {
                let id = self.get(BlockPos::new(x, y, z));
                id != bb::AIR && !props::is_liquid(id)
            })
            .map_or(self.min_y(), |y| y + 1)
    }

    fn above_top_solid(&mut self, x: i32, z: i32) -> i32 {
        if x >> 4 != self.pos.x || z >> 4 != self.pos.z {
            return self.min_y();
        }
        (self.min_y()..self.max_y())
            .rev()
            .find(|&y| props::is_solid(self.get(BlockPos::new(x, y, z))))
            .map_or(self.min_y(), |y| y + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ChunkPos;
    use crate::world::generator::DecorationRegion;
    use crate::world::generator::region::SLOT_OFFSETS;

    #[test]
    fn negative_region_edges_and_height_invalidation() {
        let origin = ChunkPos::new(-1, -1);
        let protos = SLOT_OFFSETS
            .map(|(x, z)| ProtoChunk::empty(ChunkPos::new(origin.x + x, origin.z + z), 1));
        let mut region = DecorationRegion::new(
            12345,
            origin,
            std::array::from_fn(|i| &protos[i]),
            &unastar_noise::java_blocks::BEDROCK_STATE_CLASSES,
        );
        let mut world = RegionWorld::new(&mut region);
        assert_eq!(world.get(BlockPos::new(-33, 63, -16)), OUT_OF_BOUNDS);
        assert!(!world.set(BlockPos::new(16, 63, -16), bb::STONE));
        assert_eq!(world.heightmap(-16, -16), MIN_Y);
        assert!(world.set(BlockPos::new(-16, 63, -16), bb::STONE));
        assert_eq!(world.heightmap(-16, -16), 64);
        assert_eq!(world.above_top_solid(-16, -16), 64);
        assert!(world.set(BlockPos::new(-16, 80, -16), bb::WATER));
        assert_eq!(world.heightmap(-16, -16), 64);
        assert!(world.set(BlockPos::new(-16, 63, -16), bb::AIR));
        assert_eq!(world.heightmap(-16, -16), MIN_Y);
        assert_eq!(world.above_top_solid(-16, -16), MIN_Y);
        assert_eq!(world.biome(BlockPos::new(-32, 0, -32)), 1);
        assert_eq!(world.biome(BlockPos::new(-33, 0, -32)), u16::MAX);
    }

    #[test]
    fn proto_carving_cannot_escape_its_chunk() {
        let mut proto = ProtoChunk::empty(ChunkPos::new(-1, 0), 1);
        assert!(proto.set(BlockPos::new(-1, MIN_Y, 15), bb::STONE));
        assert!(!proto.set(BlockPos::new(0, MIN_Y, 15), bb::STONE));
        assert!(!proto.set(BlockPos::new(-1, MIN_Y - 1, 15), bb::STONE));
        assert_eq!(proto.get(BlockPos::new(-1, MIN_Y, 15)), bb::STONE);
        assert_eq!(proto.get(BlockPos::new(0, MIN_Y, 15)), OUT_OF_BOUNDS);
        assert_eq!(proto.heightmap(-1, 15), MIN_Y + 1);
    }
}
