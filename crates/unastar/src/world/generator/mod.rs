//! Vanilla Minecraft world generation.
//!
//! This module implements terrain generation matching vanilla Minecraft 1.18+
//! multi-noise biome system.

mod climate;
mod constants;
pub mod flat;
mod noise;
mod structures;
mod terrain;
mod xoroshiro;

pub use climate::BiomeNoise;
pub use constants::Biome;
pub use structures::{
    StructureConfig, StructurePos, StructureType, find_structures_in_area, get_structure_pos,
};
pub use terrain::VanillaGenerator;
