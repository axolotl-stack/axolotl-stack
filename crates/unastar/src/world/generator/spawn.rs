//! World spawn search with Java's rules, over finished chunks.
//!
//! Java picks the spawn chunk from the climate (`biome_source::spawn_search_start`),
//! then walks an 11x11 square spiral of chunks around it and takes the first column,
//! x-major then z, whose surface a player can stand on:
//!
//! - `MOTION_BLOCKING` height `m`, `WORLD_SURFACE` height `w`, `OCEAN_FLOOR` height `o`
//!   (heights are one above the counted block);
//! - the column is rejected when `w <= m && w > o` (its top is fluid or a non-solid
//!   block over the ocean floor);
//! - scanning down from `m + 1`, the first fluid block rejects the column and the first
//!   block whose collision shape has a full top face is the floor; the spawn is the block
//!   above it.
//!
//! When no chunk of the spiral qualifies, Java keeps the climate chunk's centre at its
//! default spawn height. The server has no later pass that moves players out of terrain,
//! so this fallback stands on the `WORLD_SURFACE` height of that column instead.

use unastar_noise::block_class;

use super::pipeline::SpawnPoint;
use super::region::HeightmapKind;
use crate::world::ChunkPos;
use crate::world::chunk::{Chunk, MIN_Y, SUBCHUNK_COUNT};

/// Side of the chunk spiral searched around the climate chunk.
const SPIRAL_SIDE: i32 = 11;

/// Spawn for a world whose climate search ended at block column `start`.
///
/// `chunk_at` returns finished chunks; `classes` are the generator's block-class flags
/// indexed by runtime id.
pub fn java_spawn(
    start: (i32, i32),
    chunk_at: &dyn Fn(ChunkPos) -> Chunk,
    classes: &[u8],
) -> SpawnPoint {
    let center = ChunkPos::new(start.0 >> 4, start.1 >> 4);
    let half = SPIRAL_SIDE / 2;
    let (mut dx, mut dz, mut step_x, mut step_z) = (0i32, 0i32, 0i32, -1i32);
    for _ in 0..SPIRAL_SIDE * SPIRAL_SIDE {
        if (-half..=half).contains(&dx) && (-half..=half).contains(&dz) {
            let pos = ChunkPos::new(center.x + dx, center.z + dz);
            if let Some(spawn) = spawn_in_chunk(&chunk_at(pos), pos, classes) {
                return spawn;
            }
        }
        if dx == dz || (dx < 0 && dx == -dz) || (dx > 0 && dx == 1 - dz) {
            (step_x, step_z) = (-step_z, step_x);
        }
        dx += step_x;
        dz += step_z;
    }
    let chunk = chunk_at(center);
    SpawnPoint {
        x: center.x * 16 + 8,
        y: height(&chunk, classes, HeightmapKind::WorldSurface, 8, 8),
        z: center.z * 16 + 8,
    }
}

/// First standable column of a chunk, x-major then z.
fn spawn_in_chunk(chunk: &Chunk, pos: ChunkPos, classes: &[u8]) -> Option<SpawnPoint> {
    for x in 0..16u8 {
        for z in 0..16u8 {
            if let Some(y) = spawn_in_column(chunk, classes, x, z) {
                return Some(SpawnPoint {
                    x: pos.x * 16 + x as i32,
                    y,
                    z: pos.z * 16 + z as i32,
                });
            }
        }
    }
    None
}

fn spawn_in_column(chunk: &Chunk, classes: &[u8], x: u8, z: u8) -> Option<i32> {
    let motion = height(chunk, classes, HeightmapKind::MotionBlocking, x, z);
    let surface = height(chunk, classes, HeightmapKind::WorldSurface, x, z);
    if surface <= motion && surface > height(chunk, classes, HeightmapKind::OceanFloor, x, z) {
        return None;
    }
    for y in (MIN_Y..=motion + 1).rev() {
        let flags = class_at(chunk, classes, x, y, z);
        if flags & block_class::FLUID != 0 {
            return None;
        }
        if flags & block_class::FULL_TOP_FACE != 0 {
            return Some(y + 1);
        }
    }
    None
}

/// Heightmap value of a column: one above the highest block the kind counts, `MIN_Y`
/// for none.
fn height(chunk: &Chunk, classes: &[u8], kind: HeightmapKind, x: u8, z: u8) -> i32 {
    let top = MIN_Y + SUBCHUNK_COUNT as i32 * 16 - 1;
    (MIN_Y..=top)
        .rev()
        .find(|&y| kind.counts(class_at(chunk, classes, x, y, z)))
        .map_or(MIN_Y, |y| y + 1)
}

fn class_at(chunk: &Chunk, classes: &[u8], x: u8, y: i32, z: u8) -> u8 {
    let block = chunk.get_block(x, y as i16, z);
    let flags = classes
        .get(block as usize)
        .copied()
        .unwrap_or(block_class::UNMAPPED);
    assert!(
        flags & block_class::UNMAPPED == 0,
        "block {block} has no class in this generator's block-class table"
    );
    flags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::blocks;

    /// The generator's class table; the test blocks must be mapped.
    fn classes() -> Vec<u8> {
        let classes = unastar_noise::java_blocks::BEDROCK_STATE_CLASSES.to_vec();
        for id in [*blocks::AIR, *blocks::STONE, *blocks::WATER] {
            assert_eq!(classes[id as usize] & block_class::UNMAPPED, 0);
        }
        classes
    }

    fn column(chunk: &mut Chunk, x: u8, z: u8, top_stone: i32, water_to: Option<i32>) {
        for y in MIN_Y..=top_stone {
            chunk.set_block(x, y as i16, z, *blocks::STONE);
        }
        if let Some(water_top) = water_to {
            for y in top_stone + 1..=water_top {
                chunk.set_block(x, y as i16, z, *blocks::WATER);
            }
        }
    }

    #[test]
    fn stands_on_first_dry_full_block_x_major() {
        let classes = classes();
        let mut chunk = Chunk::new(2, -1);
        for x in 0..16 {
            for z in 0..16 {
                column(&mut chunk, x, z, 40, Some(62));
            }
        }
        // Dry land only at x=3 z=9 and x=5 z=0: x-major order picks x=3.
        column(&mut chunk, 3, 9, 70, None);
        column(&mut chunk, 5, 0, 80, None);
        let spawn = spawn_in_chunk(&chunk, ChunkPos::new(2, -1), &classes).unwrap();
        assert_eq!(
            spawn,
            SpawnPoint {
                x: 2 * 16 + 3,
                y: 71,
                z: -16 + 9
            }
        );
    }

    #[test]
    fn submerged_columns_are_rejected() {
        let classes = classes();
        let mut chunk = Chunk::new(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                column(&mut chunk, x, z, 40, Some(62));
            }
        }
        assert_eq!(spawn_in_chunk(&chunk, ChunkPos::new(0, 0), &classes), None);
    }

    #[test]
    fn spiral_visits_the_climate_chunk_first_then_rings() {
        let classes = classes();
        let visited = std::cell::RefCell::new(Vec::new());
        let ocean = |pos: ChunkPos| {
            visited.borrow_mut().push((pos.x, pos.z));
            let mut chunk = Chunk::new(pos.x, pos.z);
            for x in 0..16 {
                for z in 0..16 {
                    column(&mut chunk, x, z, 40, Some(62));
                }
            }
            chunk
        };
        let spawn = java_spawn((100, -40), &ocean, &classes);
        let visited = visited.into_inner();
        // 121 spiral chunks, then the fallback centre chunk again.
        assert_eq!(visited.len(), 122);
        assert_eq!(&visited[..3], &[(6, -3), (7, -3), (7, -2)]);
        let unique: std::collections::HashSet<_> = visited[..121].iter().collect();
        assert_eq!(unique.len(), 121);
        assert!(
            visited[..121]
                .iter()
                .all(|&(x, z)| (x - 6).abs() <= 5 && (z + 3).abs() <= 5)
        );
        // Fallback: the centre column's world surface (water top 62 -> 63).
        assert_eq!(
            spawn,
            SpawnPoint {
                x: 6 * 16 + 8,
                y: 63,
                z: -3 * 16 + 8
            }
        );
    }
}
