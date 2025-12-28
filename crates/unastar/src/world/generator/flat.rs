//! Superflat generator caching.

use crate::world::chunk::{Chunk, MIN_Y, SUBCHUNK_COUNT, blocks};
use std::sync::LazyLock;

/// Generate the template chunk for superflat world.
fn generate_superflat_template() -> Chunk {
    let mut chunk = Chunk::new(0, 0);
    // Standard Minecraft superflat: bedrock, 2 dirt, 1 grass
    // Y=0: Bedrock, Y=1-2: Dirt, Y=3: Grass

    // First fill with grass (will be at Y=3 when done)
    chunk.fill_floor(4, *blocks::GRASS_BLOCK); // Y=0-3: all grass initially
    // Then overwrite Y=0-2 with dirt
    chunk.fill_floor(3, *blocks::DIRT); // Y=0-2: now dirt
    // Finally overwrite Y=0 with bedrock
    chunk.fill_floor(1, *blocks::BEDROCK); // Y=0: now bedrock

    chunk
}

/// Cache of encoded subchunks for the superflat template.
/// Index = subchunk index (0..SUBCHUNK_COUNT).
///
/// We map absolute Y to index relative to MIN_Y.
static SUPERFLAT_CACHE: LazyLock<Vec<Vec<u8>>> = LazyLock::new(|| {
    let chunk = generate_superflat_template();
    (0..SUBCHUNK_COUNT)
        .map(|i| {
            // Convert array index (0..24) to world Y-index for encoding
            let y_index = (crate::world::chunk::MIN_Y >> 4) + i as i32;
            chunk.encode_subchunk(y_index).unwrap_or_default()
        })
        .collect()
});

/// Get cached encoded subchunk data for a given Y index.
pub fn get_cached_superflat_subchunk(y_index: i32) -> Option<&'static [u8]> {
    let array_idx = (y_index - (MIN_Y >> 4)) as usize;
    if array_idx < SUBCHUNK_COUNT as usize {
        let cache = &*SUPERFLAT_CACHE;
        Some(&cache[array_idx])
    } else {
        None
    }
}
