//! Block-class flags used by worldgen heightmaps and block predicates.
//!
//! The Java-style generator's tables (`JAVA_STATE_CLASSES`, `BEDROCK_STATE_CLASSES`) are
//! generated at build time from `java_data/` (see `codegen/data/java_blocks.rs`).

/// Air (`air`, `cave_air`, `void_air`).
pub const AIR: u8 = 1 << 0;
/// Carries a fluid (water or lava, including waterlogged states).
pub const FLUID: u8 = 1 << 1;
/// Carries water (water or waterlogged).
pub const WATER: u8 = 1 << 2;
/// A leaves block.
pub const LEAVES: u8 = 1 << 3;
/// Blocks motion (Java: solid collision shape; cobweb and bamboo sapling excluded).
pub const BLOCKS_MOTION: u8 = 1 << 4;
/// Runtime id that no Java state maps to; a Java-style generator never places it.
pub const UNMAPPED: u8 = 1 << 7;
