//! Slime chunks: the chunks in which the slime's native spawn check lets a
//! slime spawn underground.
//!
//! A chunk is a slime chunk when the first output of a 32-bit Mersenne
//! Twister seeded with `chunk_x × 0x1f1f1f1f ^ chunk_z` (wrapping, as
//! unsigned 32-bit numbers) is a multiple of 10: one chunk in ten, the same
//! in every world, since the world seed plays no part. Every slime BDS
//! 1.26.51 spawned on the superflat floor of `sweep.py slime-spawn` stood in
//! such a chunk (`mob::tests::slime::every_slime_bds_spawned_stood_in_a_slime_chunk`).

use crate::entity::mob::constants::{SLIME_CHUNK_CHANCE, SLIME_CHUNK_SEED_MULTIPLIER};
use crate::world::bedrock_random::BedrockRandom;

/// Whether the chunk at `(chunk_x, chunk_z)` is a slime chunk.
pub fn is_slime_chunk(chunk_x: i32, chunk_z: i32) -> bool {
    let seed = (chunk_x as u32).wrapping_mul(SLIME_CHUNK_SEED_MULTIPLIER) ^ chunk_z as u32;
    BedrockRandom::new(seed)
        .next_u32()
        .is_multiple_of(SLIME_CHUNK_CHANCE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_one_chunk_in_ten_is_a_slime_chunk() {
        let slime = (-50..50)
            .flat_map(|x| (-50..50).map(move |z| (x, z)))
            .filter(|&(x, z)| is_slime_chunk(x, z))
            .count();
        assert!((900..1100).contains(&slime), "{slime} of 10000");
    }
}
