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

/// Seed of a chunk's decoration random: two odd multipliers drawn from the
/// world seed mix the chunk coordinates (the classic per-chunk population
/// seed shape), folded to MT19937's 32-bit seed.
pub fn chunk_seed(world_seed: i64, chunk_x: i32, chunk_z: i32) -> u32 {
    let mut r = BedrockRandom::new(world_seed as u32 ^ (world_seed >> 32) as u32);
    let a = r.next_u32() | 1;
    let b = r.next_u32() | 1;
    (chunk_x as u32)
        .wrapping_mul(a)
        .wrapping_add((chunk_z as u32).wrapping_mul(b))
        ^ world_seed as u32
}

/// Per-world decoration state.
pub struct Decorator {
    seed: i64,
    noise: MolangNoise,
}

impl Decorator {
    pub fn new(seed: i64) -> Self {
        Self {
            seed,
            noise: MolangNoise,
        }
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
        let mut rng = BedrockRandom::new(chunk_seed(self.seed, chunk_x, chunk_z) ^ salt);
        let mut ctx = Ctx {
            world,
            rng: &mut rng,
            noise: &self.noise,
            seed: self.seed,
        };
        let origin = BlockPos::new(chunk_x * 16, 0, chunk_z * 16);
        for &pass in passes {
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
        let mut rng = BedrockRandom::new(chunk_seed(self.seed, chunk_x, chunk_z) ^ 0x7374_7275);
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
    fn chunk_seeds_differ_per_chunk_and_world() {
        let s = |seed, x, z| chunk_seed(seed, x, z);
        assert_ne!(s(12345, 0, 0), s(12345, 1, 0));
        assert_ne!(s(12345, 1, 0), s(12345, 0, 1));
        assert_ne!(s(12345, 3, 7), s(54321, 3, 7));
        assert_eq!(s(12345, -4, 9), s(12345, -4, 9));
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
