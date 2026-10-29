//! The `minecraft:slime` spawn rule's placement test, which the rule file
//! does not describe (matches BDS 1.26.51).
//!
//! A slime spawn block (the block the slime stands on) passes when either
//!
//! - it is at most [`SLIME_CHUNK_MAX_Y`] high in a slime chunk
//!   ([`is_slime_chunk`]: the first output of an MT19937 seeded from the
//!   chunk coordinates alone, not the world seed, is a multiple of
//!   [`SLIME_CHUNK_CHANCE`]); no light test applies; or
//! - it lies in the swamp surface band [`SLIME_SWAMP_HEIGHT`], the raw
//!   brightness above it is below [`SLIME_SWAMP_BRIGHTNESS_LIMIT`] and at most
//!   a draw in `0..SLIME_SWAMP_BRIGHTNESS_LIMIT`, a float draw is below the
//!   moon brightness ([`moon_brightness`]), and the biome of the block above
//!   carries the [`SLIME_BIOME_TAG`] tag (swamp, mangrove swamp).
//!
//! The spawn cycle runs it among the rule's conditions, after the height and
//! surface/underground tests and before the difficulty and brightness
//! filters; the draws come from the cycle's random.

use bevy_ecs::prelude::World;
use glam::IVec3;
use rand::Rng;

use super::cycle::biome_at;
use crate::entity::EntityRandom;
use crate::entity::mob::constants::{
    SLIME_BIOME_TAG, SLIME_CHUNK_CHANCE, SLIME_CHUNK_MAX_Y, SLIME_CHUNK_SEED_MULTIPLIER,
    SLIME_SWAMP_BRIGHTNESS_LIMIT, SLIME_SWAMP_HEIGHT,
};
use crate::world::regional_difficulty::moon_brightness;
use crate::world::rules::WorldTime;

/// MT19937's middle word offset (`M`).
const MT_MIDDLE: u32 = 397;
/// MT19937's twist matrix (`MATRIX_A`).
const MT_MATRIX_A: u32 = 0x9908_b0df;
const MT_UPPER_MASK: u32 = 0x8000_0000;
const MT_LOWER_MASK: u32 = 0x7fff_ffff;
/// MT19937's state initialisation multiplier (`init_genrand`).
const MT_INIT_MULTIPLIER: u32 = 1_812_433_253;

/// Whether chunk (`cx`, `cz`) is a slime chunk (module docs).
pub fn is_slime_chunk(cx: i32, cz: i32) -> bool {
    let seed = (cx.wrapping_mul(SLIME_CHUNK_SEED_MULTIPLIER) as u32) ^ (cz as u32);
    first_mt19937_output(seed).is_multiple_of(SLIME_CHUNK_CHANCE)
}

/// The first `genrand_int32` output of an MT19937 seeded by `init_genrand`
/// with `seed`. The first twist rewrites state word 0 from words 0, 1 and
/// 397 only, so the 624-word state is never built: the initialisation
/// recurrence runs up to word 397 keeping word 1 aside.
fn first_mt19937_output(seed: u32) -> u32 {
    let mut word = seed;
    let mut second = 0;
    for index in 1..=MT_MIDDLE {
        word = MT_INIT_MULTIPLIER
            .wrapping_mul(word ^ (word >> 30))
            .wrapping_add(index);
        if index == 1 {
            second = word;
        }
    }
    let y = (seed & MT_UPPER_MASK) | (second & MT_LOWER_MASK);
    let mut out = word ^ (y >> 1);
    if y & 1 != 0 {
        out ^= MT_MATRIX_A;
    }
    out ^= out >> 11;
    out ^= (out << 7) & 0x9d2c_5680;
    out ^= (out << 15) & 0xefc6_0000;
    out ^ (out >> 18)
}

/// The slime placement test with the [`SLIME_BIOME_TAG`] biomes resolved
/// once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlimeSpawnRule {
    /// Bedrock ids of the biomes tagged [`SLIME_BIOME_TAG`], sorted.
    swamp_biomes: Vec<u32>,
}

impl SlimeSpawnRule {
    /// Resolve the [`SLIME_BIOME_TAG`] biomes from the vanilla biome data.
    pub fn resolve() -> Self {
        let mut swamp_biomes: Vec<u32> = unastar_data::biomes::iter()
            .filter(|biome| biome.tags.iter().any(|tag| tag == SLIME_BIOME_TAG))
            .map(|biome| biome.id)
            .collect();
        swamp_biomes.sort_unstable();
        Self { swamp_biomes }
    }

    /// Whether a slime may spawn standing on `block` with raw brightness
    /// `brightness` above it (module docs). The swamp test draws from the
    /// world's [`EntityRandom`] and reads the moon phase from its
    /// [`WorldTime`].
    pub fn allows(&self, world: &mut World, block: IVec3, brightness: i32) -> bool {
        if block.y <= SLIME_CHUNK_MAX_Y && is_slime_chunk(block.x >> 4, block.z >> 4) {
            return true;
        }
        if !SLIME_SWAMP_HEIGHT.contains(&block.y) || brightness >= SLIME_SWAMP_BRIGHTNESS_LIMIT {
            return false;
        }
        let mut random = world.resource_mut::<EntityRandom>();
        if brightness > random.0.gen_range(0..SLIME_SWAMP_BRIGHTNESS_LIMIT) {
            return false;
        }
        let draw: f32 = random.0.r#gen();
        // The level time is a 32-bit tick count; the clock wraps the same way.
        let time = world.resource::<WorldTime>().ticks as i32;
        if draw >= moon_brightness(time) {
            return false;
        }
        biome_at(world, block + IVec3::Y)
            .is_some_and(|biome| self.swamp_biomes.binary_search(&biome).is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straightforward MT19937 (`mt19937ar.c`): full state, full twist.
    struct ReferenceMt {
        state: Vec<u32>,
        index: usize,
    }

    impl ReferenceMt {
        fn new(seed: u32) -> Self {
            let mut state = vec![seed];
            for i in 1..624u32 {
                let prev = state[i as usize - 1];
                state.push(
                    1_812_433_253u32
                        .wrapping_mul(prev ^ (prev >> 30))
                        .wrapping_add(i),
                );
            }
            Self { state, index: 624 }
        }

        fn next_u32(&mut self) -> u32 {
            if self.index >= 624 {
                for i in 0..624 {
                    let y =
                        (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % 624] & 0x7fff_ffff);
                    let mag = if y & 1 == 0 { 0 } else { 0x9908_b0df };
                    self.state[i] = self.state[(i + 397) % 624] ^ (y >> 1) ^ mag;
                }
                self.index = 0;
            }
            let mut y = self.state[self.index];
            self.index += 1;
            y ^= y >> 11;
            y ^= (y << 7) & 0x9d2c_5680;
            y ^= (y << 15) & 0xefc6_0000;
            y ^ (y >> 18)
        }
    }

    fn seed(cx: i32, cz: i32) -> u32 {
        (cx.wrapping_mul(0x1f1f_1f1f) as u32) ^ (cz as u32)
    }

    #[test]
    fn first_output_matches_the_reference_vector() {
        // init_genrand(5489): the reference implementation's first output.
        assert_eq!(first_mt19937_output(5489), 3_499_211_612);
        assert_eq!(ReferenceMt::new(5489).next_u32(), 3_499_211_612);
    }

    #[test]
    fn first_output_matches_a_full_mt19937_on_chunk_seeds() {
        let extremes = [i32::MIN, i32::MIN + 1, -1, 0, 1, i32::MAX];
        let grid = (-40..40).flat_map(|cx| (-40..40).map(move |cz| (cx, cz)));
        let edges = extremes
            .iter()
            .flat_map(|&cx| extremes.iter().map(move |&cz| (cx, cz)));
        for (cx, cz) in grid.chain(edges) {
            let seed = seed(cx, cz);
            let expected = ReferenceMt::new(seed).next_u32();
            assert_eq!(first_mt19937_output(seed), expected, "chunk ({cx}, {cz})");
            assert_eq!(
                is_slime_chunk(cx, cz),
                expected.is_multiple_of(10),
                "chunk ({cx}, {cz})"
            );
        }
    }

    #[test]
    fn known_slime_chunks() {
        for (cx, cz) in [
            (-3, -2),
            (-3, -1),
            (-1, 0),
            (0, -2),
            (1, -2),
            (3, 0),
            (3, 1),
        ] {
            assert!(is_slime_chunk(cx, cz), "({cx}, {cz}) is a slime chunk");
        }
        for (cx, cz) in [(0, 0), (0, 1), (2, 0), (-2, -2)] {
            assert!(!is_slime_chunk(cx, cz), "({cx}, {cz}) is no slime chunk");
        }
    }

    #[test]
    fn about_one_chunk_in_ten_is_a_slime_chunk() {
        let slime = (-100..100)
            .flat_map(|cx| (-100..100).map(move |cz| (cx, cz)))
            .filter(|&(cx, cz)| is_slime_chunk(cx, cz))
            .count();
        assert!((3_600..4_400).contains(&slime), "{slime} of 40000");
    }

    #[test]
    fn the_slime_biome_tag_resolves_to_the_swamps() {
        let rule = SlimeSpawnRule::resolve();
        for swamp in [
            "minecraft:swampland",
            "minecraft:swampland_mutated",
            "minecraft:mangrove_swamp",
        ] {
            let id = unastar_data::biomes::get(swamp).expect("vanilla biome").id;
            assert!(rule.swamp_biomes.contains(&id), "{swamp}");
        }
        let plains = unastar_data::biomes::get("minecraft:plains")
            .expect("plains")
            .id;
        assert!(!rule.swamp_biomes.contains(&plains));
    }
}
