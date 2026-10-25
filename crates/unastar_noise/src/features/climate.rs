//! Java biome climate as decoration reads it: height-adjusted temperature,
//! `shouldFreeze` and `shouldSnow` (used by `freeze_top_layer`, ice and snow
//! placement).
//!
//! Block light is taken as 0 during decoration (the light engine has not run
//! yet), so the Java `getBrightness(BLOCK, pos) < 10` checks always pass.

use super::blocks::{BlockPos, FluidKind, JavaState};
use super::eval::FeatureWorld;
use super::java_blocks::{blocks as b, states};
use super::random::LegacyRandom;
use crate::noise::SimplexNoise;
use std::sync::LazyLock;

/// Climate settings of one biome (`worldgen/biome/*.json`).
#[derive(Debug, Clone, Copy)]
pub struct BiomeClimate {
    pub has_precipitation: bool,
    pub temperature: f32,
    /// `temperature_modifier: "frozen"`.
    pub frozen: bool,
    pub downfall: f32,
}

/// `PerlinSimplexNoise` over consecutive octaves `[first, 0]` built from a
/// legacy random, as Java's `Biome` noises are.
struct PerlinSimplex {
    /// Highest-frequency level first.
    levels: Vec<SimplexNoise>,
}

impl PerlinSimplex {
    fn new(seed: i64, octave_count: usize) -> Self {
        let mut random = LegacyRandom::new(seed);
        let levels = (0..octave_count)
            .map(|_| SimplexNoise::new(&mut random))
            .collect();
        Self { levels }
    }

    fn value(&self, x: f64, z: f64) -> f64 {
        let n = self.levels.len() as i32;
        let mut input = 1.0;
        let mut factor = 1.0 / (2f64.powi(n) - 1.0);
        let mut sum = 0.0;
        for level in &self.levels {
            sum += level.get_value_2d(x * input, z * input) * factor;
            input /= 2.0;
            factor *= 2.0;
        }
        sum
    }
}

static TEMPERATURE_NOISE: LazyLock<PerlinSimplex> = LazyLock::new(|| PerlinSimplex::new(1234, 1));
static FROZEN_TEMPERATURE_NOISE: LazyLock<PerlinSimplex> =
    LazyLock::new(|| PerlinSimplex::new(3456, 3));

impl BiomeClimate {
    /// Base temperature after the `frozen` modifier.
    fn modified_temperature(&self, pos: BlockPos) -> f32 {
        if !self.frozen {
            return self.temperature;
        }
        let d0 = FROZEN_TEMPERATURE_NOISE.value(pos.x as f64 * 0.05, pos.z as f64 * 0.05) * 7.0;
        let d1 = super::noise::biome_info_noise(pos.x as f64 * 0.2, pos.z as f64 * 0.2);
        if d0 + d1 < 0.3 {
            let d3 = super::noise::biome_info_noise(pos.x as f64 * 0.09, pos.z as f64 * 0.09);
            if d3 < 0.8 {
                return 0.2;
            }
        }
        self.temperature
    }

    /// `getHeightAdjustedTemperature(pos, seaLevel)`.
    pub fn temperature_at(&self, pos: BlockPos, sea_level: i32) -> f32 {
        let t = self.modified_temperature(pos);
        let snow_line = sea_level + 17;
        if pos.y > snow_line {
            let n = (TEMPERATURE_NOISE
                .value((pos.x as f32 / 8.0) as f64, (pos.z as f32 / 8.0) as f64)
                * 8.0) as f32;
            t - (n + pos.y as f32 - snow_line as f32) * 0.05 / 40.0
        } else {
            t
        }
    }

    /// `warmEnoughToRain`.
    #[inline]
    pub fn warm_enough_to_rain(&self, pos: BlockPos, sea_level: i32) -> bool {
        self.temperature_at(pos, sea_level) >= 0.15
    }

    /// `shouldFreeze(level, pos, mustBeAtEdge)`.
    pub fn should_freeze<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        pos: BlockPos,
        must_be_at_edge: bool,
    ) -> bool {
        if self.warm_enough_to_rain(pos, world.sea_level()) || world.is_outside_build_height(pos.y)
        {
            return false;
        }
        let state = world.get_state(pos);
        let fluid = state.fluid();
        if !(fluid.kind == FluidKind::Water && fluid.source && state.is(b::WATER)) {
            return false;
        }
        if !must_be_at_edge {
            return true;
        }
        let water = |w: &mut W, p: BlockPos| w.fluid(p).is_water();
        !(water(world, pos.offset(-1, 0, 0))
            && water(world, pos.offset(1, 0, 0))
            && water(world, pos.offset(0, 0, -1))
            && water(world, pos.offset(0, 0, 1)))
    }

    /// `shouldSnow(level, pos)`.
    pub fn should_snow<W: FeatureWorld + ?Sized>(&self, world: &mut W, pos: BlockPos) -> bool {
        if self.warm_enough_to_rain(pos, world.sea_level()) || world.is_outside_build_height(pos.y)
        {
            return false;
        }
        let state: JavaState = world.get_state(pos);
        (state.is_air() || state.is(b::SNOW))
            && super::survival::can_survive(world, states::SNOW, pos)
    }
}
