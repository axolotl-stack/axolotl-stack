//! Block access for decoration: the [`BlockWorld`] trait and the [`Ctx`]
//! every compiled feature receives.
//!
//! Features see absolute world coordinates. The world implementation decides
//! which region is readable and writable (the pipeline decorates a chunk with
//! its neighbours available); reads outside it return [`OUT_OF_BOUNDS`], which
//! matches no block predicate, and writes outside it are dropped and reported
//! as `false`, so a feature near the edge of the region fails cleanly.

use super::rng::BedrockRandom;

/// Runtime id returned for positions outside the readable region. It is not
/// a palette ordinal, so no compiled block predicate ever matches it.
pub const OUT_OF_BOUNDS: u32 = u32::MAX;

/// A block position in world coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    #[inline]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    #[inline]
    pub const fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }

    #[inline]
    pub const fn above(self) -> Self {
        self.offset(0, 1, 0)
    }

    #[inline]
    pub const fn below(self) -> Self {
        self.offset(0, -1, 0)
    }

    /// Neighbour in a horizontal [`Direction`].
    #[inline]
    pub const fn side(self, dir: Direction) -> Self {
        let (dx, dz) = dir.delta();
        self.offset(dx, 0, dz)
    }
}

/// Horizontal directions, in Bedrock's legacy `direction` order
/// (0 south, 1 west, 2 north, 3 east).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    South,
    West,
    North,
    East,
}

impl Direction {
    pub const ALL: [Direction; 4] = [
        Direction::South,
        Direction::West,
        Direction::North,
        Direction::East,
    ];

    #[inline]
    pub const fn delta(self) -> (i32, i32) {
        match self {
            Direction::South => (0, 1),
            Direction::West => (-1, 0),
            Direction::North => (0, -1),
            Direction::East => (1, 0),
        }
    }

    #[inline]
    pub const fn opposite(self) -> Direction {
        match self {
            Direction::South => Direction::North,
            Direction::West => Direction::East,
            Direction::North => Direction::South,
            Direction::East => Direction::West,
        }
    }
}

/// Block storage a decoration pass reads and writes.
pub trait BlockWorld {
    /// Lowest buildable y (inclusive).
    fn min_y(&self) -> i32;
    /// Highest buildable y (exclusive).
    fn max_y(&self) -> i32;
    /// Runtime id at `pos`, or [`OUT_OF_BOUNDS`].
    fn get(&self, pos: BlockPos) -> u32;
    /// Set `pos`; `false` (and no change) outside the writable region.
    fn set(&mut self, pos: BlockPos, id: u32) -> bool;
    /// Retain a generated actor at its absolute position; reject unwritable positions.
    fn set_block_entity(&mut self, entity: crate::world::block_entity::BlockEntity) -> bool;
    /// Numeric Bedrock biome id at `pos`.
    fn biome(&self, pos: BlockPos) -> u16;
    /// `query.heightmap`: y of the first block above the highest non-air block,
    /// including liquids, at column `(x, z)` (BDS water/stone marker controls).
    fn heightmap(&mut self, x: i32, z: i32) -> i32;
    /// `query.above_top_solid`: y of the first block above the highest solid
    /// block (palette `solid` flag) at column `(x, z)`.
    fn above_top_solid(&mut self, x: i32, z: i32) -> i32;
}

/// World-seeded noise for Molang `query.noise(x, z)`.
pub trait NoiseSource {
    fn noise(&self, x: f32, z: f32) -> f32;
}

/// What every compiled feature and rule receives.
pub struct Ctx<'a, W: BlockWorld> {
    pub world: &'a mut W,
    pub rng: &'a mut BedrockRandom,
    pub noise: &'a dyn NoiseSource,
    /// World seed, for features that seed their own random (carvers).
    pub seed: i64,
}

impl<W: BlockWorld> Ctx<'_, W> {
    #[inline]
    pub fn get(&self, pos: BlockPos) -> u32 {
        self.world.get(pos)
    }

    #[inline]
    pub fn set(&mut self, pos: BlockPos, id: u32) -> bool {
        self.world.set(pos, id)
    }

    #[inline]
    pub fn heightmap(&mut self, x: i32, z: i32) -> i32 {
        self.world.heightmap(x, z)
    }

    #[inline]
    pub fn above_top_solid(&mut self, x: i32, z: i32) -> i32 {
        self.world.above_top_solid(x, z)
    }

    #[inline]
    pub fn noise(&self, x: f32, z: f32) -> f32 {
        self.noise.noise(x, z)
    }

    #[inline]
    pub fn biome(&self, pos: BlockPos) -> u16 {
        self.world.biome(pos)
    }

    #[inline]
    pub fn in_height(&self, y: i32) -> bool {
        y >= self.world.min_y() && y < self.world.max_y()
    }
}
