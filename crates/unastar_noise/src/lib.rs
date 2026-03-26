//! Worldgen noise and density function library.
//!
//! This crate provides generated density functions and noise parameters
//! for Minecraft worldgen. Code is generated at build time from JSON files.
//!
//! # Biome Feature Lookup
//!
//! Features are organized by biome and generation step. Use `BiomeFeatures` to query
//! which placed features should be generated for a given biome:
//!
//! ```
//! use unastar_noise::{BiomeFeatures, GenerationStep};
//!
//! // Get ore features for Plains biome
//! let plains = BiomeFeatures::Plains;
//! let ores = plains.get_features(GenerationStep::UndergroundOres);
//! assert!(ores.contains(&"ore_coal_upper"));
//! assert!(ores.contains(&"ore_diamond"));
//!
//! // Look up biome by name
//! let biome = BiomeFeatures::from_name("dark_forest").unwrap();
//! let vegetation = biome.get_features(GenerationStep::VegetalDecoration);
//! assert!(vegetation.contains(&"dark_forest_vegetation"));
//! ```

#![feature(portable_simd)]

// Core types module
mod types;
pub use types::*;

// Noise generation modules
pub mod noise;
pub mod xoroshiro;

pub use noise::{BlendedNoise, DoublePerlinNoise, OctaveNoise, PerlinNoise, SimplexNoise};
pub use xoroshiro::{JavaRandom, PositionalRandomFactory, Xoroshiro128, get_seed};

// Biome enum
pub mod biome;
pub use biome::Biome;

// Biome lookup tables
pub mod biome_tables;

// Surface rules system
pub mod surface;
pub use surface::{
    // Conditions
    AbovePreliminarySurface,
    // Rules
    BandlandsRule,
    BiomeCheck,
    BlockIdRule,
    BlockRule,
    // Context
    CaveSurface,
    Condition,
    Hole,
    LazyCondition,
    NoiseThreshold,
    Not,
    Rule,
    SequenceRule,
    Steep,
    StoneDepthCheck,
    SurfaceContext,
    Temperature,
    TestRule,
    VerticalAnchor,
    VerticalGradient,
    WaterCheck,
    YCheck,
};

// Include generated code from OUT_DIR
include!(concat!(env!("OUT_DIR"), "/mod.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_biome_source_parameter_list_json() {
        let overworld = multi_noise_biome_source_parameter_list_json("overworld")
            .expect("missing overworld biome source preset");
        assert!(overworld.contains("\"preset\""));
        assert!(overworld.contains("minecraft:overworld"));
    }

    #[test]
    fn exposes_template_pool_json() {
        let plains_town_centers = template_pool_json("village/plains/town_centers")
            .expect("missing village/plains/town_centers template pool");
        assert!(plains_town_centers.contains("\"elements\""));
    }

    #[test]
    fn exposes_structure_template_bytes() {
        let watchtower = structure_template_nbt("pillager_outpost/watchtower")
            .expect("missing pillager outpost watchtower template");
        assert!(!watchtower.is_empty());
    }
}
