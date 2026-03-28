//! Vanilla Minecraft world generation.
//!
//! This module implements terrain generation matching vanilla Minecraft 1.18+
//! multi-noise biome system.

// TODO: Refactor aquifer and ore_veinifier to use AOT-compiled density functions
pub mod aquifer;
mod beardifier;
pub mod carver;
mod climate;
mod constants;
pub mod density;
mod feature_registry;
mod features;
pub mod flat;
mod jigsaw;
pub mod noise;
pub mod ore_veinifier;
mod structure_registry;
mod structures;
pub mod surface;
mod terrain;
pub mod xoroshiro;

pub use climate::BiomeNoise;
pub use constants::Biome;
pub use feature_registry::{
    FeatureRuntimeRegistry, RuntimeConfiguredFeatureDefinition, RuntimePlacedFeatureDefinition,
};
pub use structure_registry::{
    RuntimeStructureDefinition, RuntimeStructureSelection, RuntimeStructureSet,
    RuntimeTemplatePool, StructureRuntimeRegistry,
};
pub use structures::{
    StructureConfig, StructurePos, StructureSelectionEntry, StructureType, find_structures_in_area,
    get_structure_pos, structure_set_attempt_order,
};
pub use terrain::VanillaGenerator;
