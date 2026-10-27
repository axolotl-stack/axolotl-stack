//! Runtime types of the compiled `OVERWORLD_CARVERS`, `NETHER_CARVERS` and `END_CARVERS`
//! tables in [`crate::carvers`], plus Java's legacy-random sampling helpers.

use std::sync::LazyLock;

use crate::xoroshiro::JavaRandom;

/// A float provider, sampled from the carver's legacy random.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatProvider {
    Constant(f32),
    Uniform { min: f32, max: f32 },
    Trapezoid { min: f32, max: f32, plateau: f32 },
}

impl FloatProvider {
    pub fn sample(&self, random: &mut JavaRandom) -> f32 {
        match *self {
            Self::Constant(value) => value,
            Self::Uniform { min, max } => random.next_float() * (max - min) + min,
            Self::Trapezoid { min, max, plateau } => {
                let range = max - min;
                let slope = (range - plateau) / 2.0;
                let top = range - slope;
                min + random.next_float() * top + random.next_float() * slope
            }
        }
    }
}

/// A height provider with its anchors resolved to absolute y.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightProvider {
    Uniform { min: i32, max: i32 },
}

impl HeightProvider {
    pub fn sample(&self, random: &mut JavaRandom) -> i32 {
        match *self {
            Self::Uniform { min, max } => {
                if min > max {
                    return min;
                }
                random.next_int((max - min + 1) as u32) + min
            }
        }
    }
}

/// `minecraft:cave` / `minecraft:nether_cave` configuration.
#[derive(Debug, Clone, Copy)]
pub struct CaveConfig {
    pub probability: f32,
    pub y: HeightProvider,
    pub y_scale: FloatProvider,
    pub lava_level: i32,
    /// Bitset over canonical Java state ids of the blocks it may carve.
    pub replaceable: &'static [u64],
    pub horizontal_radius_multiplier: FloatProvider,
    pub vertical_radius_multiplier: FloatProvider,
    pub floor_level: FloatProvider,
}

/// `minecraft:canyon` configuration.
#[derive(Debug, Clone, Copy)]
pub struct CanyonConfig {
    pub probability: f32,
    pub y: HeightProvider,
    pub y_scale: FloatProvider,
    pub lava_level: i32,
    /// Bitset over canonical Java state ids of the blocks it may carve.
    pub replaceable: &'static [u64],
    pub vertical_rotation: FloatProvider,
    pub distance_factor: FloatProvider,
    pub thickness: FloatProvider,
    pub width_smoothness: i32,
    pub horizontal_radius_factor: FloatProvider,
    pub vertical_radius_default_factor: f32,
    pub vertical_radius_center_factor: f32,
}

/// A configured carver.
#[derive(Debug, Clone, Copy)]
pub enum ConfiguredCarver {
    Cave(CaveConfig),
    NetherCave(CaveConfig),
    Canyon(CanyonConfig),
}

impl ConfiguredCarver {
    pub fn probability(&self) -> f32 {
        match self {
            Self::Cave(c) | Self::NetherCave(c) => c.probability,
            Self::Canyon(c) => c.probability,
        }
    }
}

/// Whether a canonical Java state id is in a carver's replaceable bitset.
#[inline]
pub fn is_replaceable(bits: &[u64], state_id: u32) -> bool {
    let index = state_id as usize;
    bits.get(index / 64)
        .is_some_and(|word| word & (1 << (index % 64)) != 0)
}

static SIN: LazyLock<Box<[f32; 65536]>> = LazyLock::new(|| {
    let mut table = Box::new([0f32; 65536]);
    for (i, value) in table.iter_mut().enumerate() {
        *value = (i as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32;
    }
    table
});

/// Java's table-based `Mth.sin`.
#[inline]
pub fn sin(value: f32) -> f32 {
    SIN[((value * 10430.378) as i32 as u32 & 0xFFFF) as usize]
}

/// Java's table-based `Mth.cos`.
#[inline]
pub fn cos(value: f32) -> f32 {
    SIN[(((value * 10430.378) + 16384.0) as i32 as u32 & 0xFFFF) as usize]
}

/// Java's `WorldgenRandom.setLargeFeatureSeed(seed, chunkX, chunkZ)`.
pub fn set_large_feature_seed(random: &mut JavaRandom, seed: i64, chunk_x: i32, chunk_z: i32) {
    random.set_seed(seed);
    let a = random.next_long();
    let b = random.next_long();
    let mixed = (chunk_x as i64).wrapping_mul(a) ^ (chunk_z as i64).wrapping_mul(b) ^ seed;
    random.set_seed(mixed);
}
