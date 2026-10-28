//! Decoration driver: runs the compiled feature rules for one chunk.
//!
//! Order per chunk: [`Decorator::carve`] (the `pregeneration_pass`, cave
//! carvers, which only touch the chunk itself) runs on the proto chunk after
//! the surface builders; [`Decorator::decorate`] runs every later pass in
//! [`Pass::ALL`] order with the 3x3 neighbourhood available.
//!
//! **unmeasured: to confirm by BDS fixture**: chunk seeding, pass stream
//! separation and rule draw order. Each chunk currently has an independent
//! deterministic stream. The fixed Molang noise intrinsic is independently
//! calibrated; statistical/byte-exact decoration parity is not established.

use super::generated;
use super::molang_noise::MolangNoise;
use super::rng::BedrockRandom;
use super::world::{BlockPos, BlockWorld, Ctx};
use super::{BiomeSet, Pass};

/// Per-world decoration state.
pub struct Decorator {
    seed: i64,
    population_x: u32,
    population_z: u32,
    noise: MolangNoise,
}

impl Decorator {
    pub fn new(seed: i64) -> Self {
        let mut population = BedrockRandom::new(seed as u32 ^ (seed >> 32) as u32);
        let population_x = population.next_u32() | 1;
        let population_z = population.next_u32() | 1;
        Self {
            seed,
            population_x,
            population_z,
            noise: MolangNoise,
        }
    }

    /// Mix chunk coordinates with the world's two immutable odd population
    /// multipliers. Keep wrapping arithmetic and the signed-world high-word fold
    /// unchanged; each phase still constructs its own RNG from this seed.
    pub(super) fn chunk_seed(&self, chunk_x: i32, chunk_z: i32) -> u32 {
        (chunk_x as u32)
            .wrapping_mul(self.population_x)
            .wrapping_add((chunk_z as u32).wrapping_mul(self.population_z))
            ^ self.seed as u32
    }

    fn run<W: BlockWorld>(
        &self,
        world: &mut W,
        chunk_x: i32,
        chunk_z: i32,
        biomes: &BiomeSet,
        passes: &[Pass],
        salt: u32,
    ) {
        let mut rng = BedrockRandom::new(self.chunk_seed(chunk_x, chunk_z) ^ salt);
        let mut ctx = Ctx {
            world,
            rng: &mut rng,
            noise: &self.noise,
            seed: self.seed,
        };
        let origin = BlockPos::new(chunk_x * 16, 0, chunk_z * 16);
        for &pass in passes {
            use unastar_noise::profile::{self, Stage};
            let _stage = profile::scope(match pass {
                Pass::Pregeneration => Stage::Carvers,
                Pass::First => Stage::BedrockFirst,
                Pass::BeforeUnderground => Stage::BedrockBeforeUnderground,
                Pass::Underground => Stage::BedrockUnderground,
                Pass::AfterUnderground => Stage::BedrockAfterUnderground,
                Pass::BeforeSurface => Stage::BedrockBeforeSurface,
                Pass::Surface => Stage::BedrockSurface,
                Pass::AfterSurface => Stage::BedrockAfterSurface,
                Pass::Final => Stage::BedrockFinal,
            });
            generated::run_pass(pass, &mut ctx, origin, biomes);
        }
    }

    /// Run the `pregeneration_pass` (cave carvers) for one chunk.
    pub fn carve<W: BlockWorld>(
        &self,
        world: &mut W,
        chunk_x: i32,
        chunk_z: i32,
        biomes: &BiomeSet,
    ) {
        // A separate stream keeps carving independent of decoration draws.
        self.run(
            world,
            chunk_x,
            chunk_z,
            biomes,
            &[Pass::Pregeneration],
            0x5bd1_e995,
        );
    }

    /// Place compiled structure sets before vegetation decoration.
    ///
    /// **unmeasured: to confirm by BDS fixture**: the independent structure
    /// stream salt and its ordering relative to the feature passes.
    pub fn structures<W: BlockWorld>(
        &self,
        world: &mut W,
        chunk_x: i32,
        chunk_z: i32,
        surface: &dyn Fn(i32, i32) -> i32,
        biome: &dyn Fn(i32, i32, i32) -> u16,
    ) {
        let mut rng = BedrockRandom::new(self.chunk_seed(chunk_x, chunk_z) ^ 0x7374_7275);
        let mut ctx = Ctx {
            world,
            rng: &mut rng,
            noise: &self.noise,
            seed: self.seed,
        };
        super::structures::place_in_chunk(&mut ctx, surface, biome, chunk_x, chunk_z);
    }

    /// Run every decoration pass after `pregeneration_pass` for one chunk;
    /// `world` must expose the chunk's 3x3 neighbourhood.
    pub fn decorate<W: BlockWorld>(
        &self,
        world: &mut W,
        chunk_x: i32,
        chunk_z: i32,
        biomes: &BiomeSet,
    ) {
        self.run(world, chunk_x, chunk_z, biomes, &Pass::ALL[1..], 0);
    }
}

#[cfg(test)]
mod tests {
    use super::super::world::NoiseSource;
    use super::*;

    #[test]
    fn population_streams_preserve_signed_world_and_coordinate_boundaries() {
        // Fingerprints from an independent MT19937 reference, established
        // before moving world-seed setup out of the per-chunk paths.
        let cases: [(i64, i32, i32, u64); 8] = [
            (0, 0, 0, 0x4e75_1239_6bae_afd0),
            (12345, -4, 9, 0xd05a_17f0_eeae_0866),
            (-1, i32::MIN, i32::MAX, 0x343c_39b0_a986_0cef),
            (i64::MIN, -30_000_000, 30_000_000, 0xde02_d8c8_6b1f_511e),
            (i64::MAX, i32::MAX, i32::MIN, 0xb6ce_e0ba_4d9f_cea2),
            (0x1_0000_0001, i32::MAX, -1, 0x24df_626a_f548_6861),
            (-9_876_543_210_123, 64, 66, 0x4b0d_756f_2aad_ea28),
            (54321, -128, 34, 0x94d2_7b74_e243_9f2c),
        ];
        for (world_seed, x, z, expected) in cases {
            let seed = Decorator::new(world_seed).chunk_seed(x, z);
            let mut fingerprint = 0xcbf2_9ce4_8422_2325u64;
            for salt in [0, 0x5bd1_e995, 0x7374_7275, 0x6261_7365] {
                let mut stream = BedrockRandom::new(seed ^ salt);
                for _ in 0..32 {
                    for byte in stream.next_u32().to_le_bytes() {
                        fingerprint ^= u64::from(byte);
                        fingerprint = fingerprint.wrapping_mul(0x100_0000_01b3);
                    }
                }
            }
            assert_eq!(fingerprint, expected, "world {world_seed}, chunk {x},{z}");
        }
    }

    #[test]
    fn molang_noise_matches_authored_bds_scalar_and_float_controls() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/bds_1_26_51_molang_noise.json"
        )))
        .unwrap();
        for seed in fixture["seeds"].as_array().unwrap() {
            let seed = seed.as_i64().unwrap();
            let decorator = Decorator::new(seed);
            for sample in fixture["samples"].as_array().unwrap() {
                let x = sample["x"].as_f64().unwrap() as f32;
                let z = sample["z"].as_f64().unwrap() as f32;
                let actual_y = (192.0 + 64.0 * decorator.noise.noise(x, z)).round() as i64;
                assert_eq!(
                    actual_y,
                    sample["marker_y"].as_i64().unwrap(),
                    "native scalar carrier at seed {seed}, query.noise({x}, {z})"
                );
            }
        }
        let exact = &fixture["exact_controls"];
        let seed = exact["seed"].as_i64().unwrap();
        let decorator = Decorator::new(seed);
        for sample in exact["samples"].as_array().unwrap() {
            let x = sample["x"].as_f64().unwrap() as f32;
            let z = sample["z"].as_f64().unwrap() as f32;
            let expected = u32::from_str_radix(sample["float_bits"].as_str().unwrap(), 16).unwrap();
            assert_eq!(
                decorator.noise.noise(x, z).to_bits(),
                expected,
                "calibrated native float at seed {seed}, query.noise({x}, {z})"
            );
        }
    }
}
