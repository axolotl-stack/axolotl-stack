//! Vanilla Minecraft world generation.
//!
//! This module implements terrain generation matching vanilla Minecraft 1.18+
//! multi-noise biome system.

pub mod aquifer;
pub mod carver;
mod climate;
mod constants;
pub mod density;
pub mod flat;
pub mod noise;
pub mod ore_veinifier;
mod structures;
pub mod surface;
mod terrain;
pub mod xoroshiro;

pub use climate::BiomeNoise;
pub use constants::Biome;
pub use structures::{
    StructureConfig, StructurePos, StructureType, find_structures_in_area, get_structure_pos,
};
pub use terrain::VanillaGenerator;
