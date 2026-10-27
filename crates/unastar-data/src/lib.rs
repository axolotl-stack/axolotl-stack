//! Generated game data for Minecraft Bedrock.
//!
//! Native open-data families use datagen/codegen. Pack-derived actor and biome
//! families are generated from the pinned submodules into `OUT_DIR` at build
//! time, embedded, and decoded once at startup. Only their hashes and counts
//! are committed, never EULA-derived per-entry Rust or KDL.

pub mod actor_definitions;
pub mod actor_identifiers;
pub mod actor_wire;
pub mod bds_packets;
pub mod biomes;
pub mod blocks;
pub mod creative;
pub mod item_components;
pub mod items;
pub mod loot;
pub mod pmmp;
pub mod recipes;
pub mod shapes;
pub mod source;
pub mod types;

pub use pmmp::*;
pub use source::*;
pub use types::*;
