//! Decoration driver: runs the compiled feature rules for one chunk.
//!
//! Order per chunk: [`Decorator::carve`] (the `pregeneration_pass`, cave
//! carvers, which only touch the chunk itself) runs on the proto chunk after
//! the surface builders; [`Decorator::decorate`] runs every later pass in
//! [`Pass::ALL`] order with the 3x3 neighbourhood available.
//!
//! Seeding (measured, see `rng::feature_rule_seed`): the population seed of
//! a chunk is `level_seed ^ (chunk_x * px + chunk_z * pz)`, where `px` and
//! `pz` are the first two non-negative 31-bit draws of a random seeded with
//! the level seed, each forced odd. Every feature rule then runs on its own
//! stream derived from that seed and its identifier, so rules never shift
//! each other's draws. **unmeasured**: how seeds beyond 32 bits fold into the
//! level seed, and the geology and structure streams salted from the
//! population seed. The fixed Molang noise intrinsic is independently
//! calibrated; statistical/byte-exact decoration parity is not established.

use super::generated;
use super::molang_noise::MolangNoise;
use super::rng::BedrockRandom;
use super::world::{BlockPos, BlockWorld, Ctx};
use super::{BiomeSet, Pass};

/// Per-world decoration state.
pub struct Decorator {
    seed: i64,
    level_seed: u32,
    population_x: u32,
    population_z: u32,
    noise: MolangNoise,
}

impl Decorator {
    pub fn new(seed: i64) -> Self {
        let level_seed = seed as u32 ^ (seed >> 32) as u32;
        let mut population = BedrockRandom::new(level_seed);
        let population_x = population.next_non_negative_int() as u32 | 1;
        let population_z = population.next_non_negative_int() as u32 | 1;
        Self {
            seed,
            level_seed,
            population_x,
            population_z,
            noise: MolangNoise,
        }
    }

    /// Population seed of a chunk: its coordinates mixed with the world's two
    /// odd population multipliers (wrapping), xored with the level seed.
    pub(super) fn chunk_seed(&self, chunk_x: i32, chunk_z: i32) -> u32 {
        (chunk_x as u32)
            .wrapping_mul(self.population_x)
            .wrapping_add((chunk_z as u32).wrapping_mul(self.population_z))
            ^ self.level_seed
    }

    fn run<W: BlockWorld>(
        &self,
        world: &mut W,
        chunk_x: i32,
        chunk_z: i32,
        biomes: &BiomeSet,
        passes: &[Pass],
    ) {
        let population_seed = self.chunk_seed(chunk_x, chunk_z);
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
            generated::run_pass(
                pass,
                world,
                &self.noise,
                self.seed,
                origin,
                biomes,
                population_seed,
            );
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
        self.run(world, chunk_x, chunk_z, biomes, &[Pass::Pregeneration]);
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
        self.run(world, chunk_x, chunk_z, biomes, &Pass::ALL[1..]);
    }
}

#[cfg(test)]
mod tests {
    use super::super::rng::feature_rule_seed;
    use super::super::world::NoiseSource;
    use super::*;

    /// Low 32 bits of the 64-bit FNV-1 hash (multiply, then xor) of `text`.
    fn fnv1_low(text: &str) -> u32 {
        text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            h.wrapping_mul(0x0000_0100_0000_01b3) ^ u64::from(b)
        }) as u32
    }

    fn rule_seed(decorator: &Decorator, rule: &str, chunk_x: i32, chunk_z: i32) -> u32 {
        feature_rule_seed(decorator.chunk_seed(chunk_x, chunk_z), fnv1_low(rule))
    }

    /// Authored single-marker probe rules in natural seed-12345 BDS 1.26.51
    /// worlds (`uniform [0,16]` x and z, `coordinate_eval_order` xzy).
    #[test]
    fn feature_rule_streams_match_native_probe_worlds() {
        let decorator = Decorator::new(12345);
        // Seeds recovered from 50-iteration probe volumes: the only 32-bit
        // MT19937 seeds whose draws reproduce all 50 native positions.
        for (rule, x, z, seed) in [
            ("wg_seed:c_probe", -5, -5, 418_089_768),
            ("wg_seed:c_probe", -5, -3, 199_148_233),
            ("wg_seed:c_probe", -4, -4, 4_248_420_555),
            ("wg_seed:c_probe", -5, -2, 1_018_405_851),
            ("wg_seed:d_probe", -5, -5, 1_688_742_135),
        ] {
            assert_eq!(
                rule_seed(&decorator, rule, x, z),
                seed,
                "{rule} in chunk {x},{z}"
            );
        }
        // One-iteration probes observed directly; first_pass and final_pass
        // rules in two separate worlds.
        for (rule, x, z, local) in [
            ("wg_seed:c_probe", -5, -5, (0, 7)),
            ("wg_seed:c_probe", 3, 3, (3, 6)),
            ("wg_seed:c_probe", 0, -2, (13, 11)),
            ("wg_seed:a_probe", -5, -5, (8, 11)),
            ("wg_seed:a_probe", 3, 3, (13, 4)),
            ("wg_seed:x_probe", -5, -5, (4, 10)),
            ("wg_seed:x_probe", 0, -2, (15, 2)),
        ] {
            let mut rng = BedrockRandom::new(rule_seed(&decorator, rule, x, z));
            let placed = (rng.next_int(16), rng.next_int(16));
            assert_eq!(placed, local, "{rule} in chunk {x},{z}");
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
