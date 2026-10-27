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
pub mod end_islands;
pub mod noise;
pub mod xoroshiro;

pub use noise::{BlendedNoise, DoublePerlinNoise, OctaveNoise, PerlinNoise, SimplexNoise};
pub use xoroshiro::{JavaRandom, PositionalRandomFactory, Xoroshiro128, get_seed};

// Biome temperature (surface rules, snow/ice, icebergs)
pub mod biome_climate;

// Block-class flags (heightmaps, block predicates)
pub mod block_class;

// Java-style decoration (features)
pub mod features;

// Java-style structures
pub mod structures;

// Runtime types of the compiled carvers (`carvers`)
pub mod carver;

// Runtime side of the compiled surface rules (`overworld_surface`)
pub mod surface;

// Include generated code from OUT_DIR
include!(concat!(env!("OUT_DIR"), "/mod.rs"));
