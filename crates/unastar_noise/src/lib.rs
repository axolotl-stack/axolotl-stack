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
pub use xoroshiro::{get_seed, JavaRandom, PositionalRandomFactory, Xoroshiro128};

// Biome enum
pub mod biome;
pub use biome::Biome;

// Biome lookup tables
pub mod biome_tables;

// Surface rules system
pub mod surface;
pub use surface::{
    // Conditions
    AbovePreliminarySurface, BiomeCheck, Condition, Hole, LazyCondition, NoiseThreshold, Not,
    Steep, StoneDepthCheck, Temperature, VerticalGradient, WaterCheck, YCheck,
    // Context
    CaveSurface, SurfaceContext, VerticalAnchor,
    // Rules
    BandlandsRule, BlockIdRule, BlockRule, Rule, SequenceRule, TestRule,
};

// Include generated code from OUT_DIR
include!(concat!(env!("OUT_DIR"), "/mod.rs"));
