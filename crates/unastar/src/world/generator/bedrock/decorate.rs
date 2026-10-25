//! Decoration driver: runs the compiled feature rules for one chunk.
//!
//! Order per chunk: [`Decorator::carve`] (the `pregeneration_pass`, cave
//! carvers, which only touch the chunk itself) runs on the proto chunk after
//! the surface builders; [`Decorator::decorate`] runs every later pass in
//! [`Pass::ALL`] order with the 3x3 neighbourhood available.
//!
//! **unmeasured: to confirm by BDS fixture**: chunk seeding, pass stream
//! separation, rule draw order and Molang noise. Each chunk currently has an
//! independent deterministic stream. Neither statistical nor byte-exact
//! decoration parity has been established.

use super::generated;
use super::rng::BedrockRandom;
use super::world::{BlockPos, BlockWorld, Ctx, NoiseSource};
use super::{BiomeSet, Pass};
use unastar_noise::{SimplexNoise, Xoroshiro128};

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

/// Molang `query.noise(x, z)`: 2D simplex noise seeded from the world seed,
/// in roughly `[-1, 1]` (vanilla rules compare it against thresholds such as
/// `-0.8`, which only needs that range).
pub struct MolangNoise {
    simplex: SimplexNoise,
}

impl MolangNoise {
    pub fn new(world_seed: i64) -> Self {
        let mut rng = Xoroshiro128::from_seed(world_seed ^ 0x6d6f_6c61_6e67);
        Self {
            simplex: SimplexNoise::new(&mut rng),
        }
    }
}

impl NoiseSource for MolangNoise {
    fn noise(&self, x: f32, z: f32) -> f32 {
        self.simplex.get_value_2d(f64::from(x), f64::from(z)) as f32
    }
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
            noise: MolangNoise::new(seed),
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
    fn molang_noise_is_bounded_and_seeded() {
        let a = MolangNoise::new(1);
        let b = MolangNoise::new(2);
        let mut differs = false;
        for i in 0..200 {
            let (x, z) = (i as f32 * 0.37, i as f32 * -0.11);
            let v = a.noise(x, z);
            assert!((-1.0..=1.0).contains(&v), "{v}");
            differs |= v != b.noise(x, z);
        }
        assert!(differs);
    }
}
