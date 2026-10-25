//! Base rocks measured from BDS 1.26.51 seed 12345, chunks (6,-4)..(7,-3).
//!
//! `tests/fixtures/bds_1_26_51_seed_12345_geology.json` records contiguous
//! bedrock columns of thickness 2..=5, stone absent through y=0, mixed stone
//! and deepslate at y=1..=7, and no deepslate from y=8 upward. This differs
//! from the shared Java surface pass, which must not run for Bedrock.
//!
//! **unmeasured: to confirm by BDS fixture**: the per-column thickness draw
//! order/seeding and exact deepslate threshold RNG. Uniform thickness and a
//! linear 0..8 transition fit the observed layer counts; they are not yet
//! established across seeds or matched block-for-block.

use super::generated::bb;
use super::rng::BedrockRandom;
use crate::world::chunk::MIN_Y;
use crate::world::generator::proto::DenseBlocks;

pub(super) fn apply(blocks: &mut DenseBlocks, rng: &mut BedrockRandom) {
    for z in 0..16 {
        for x in 0..16 {
            let thickness = 2 + rng.next_int(4);
            for y in MIN_Y..=0 {
                if y < MIN_Y + thickness {
                    blocks.set_block(x, y as i16, z, bb::BEDROCK);
                } else if blocks.get_block(x, y as i16, z) == bb::STONE {
                    blocks.set_block(x, y as i16, z, bb::DEEPSLATE);
                }
            }
            for y in 1..8 {
                if blocks.get_block(x, y as i16, z) == bb::STONE && rng.next_int(8) >= y {
                    blocks.set_block(x, y as i16, z, bb::DEEPSLATE);
                }
            }
        }
    }
}
