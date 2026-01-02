//! Surface rules system for declarative block placement.
//!
//! This module implements Java Edition's surface rule system, which replaces
//! procedural block placement with a declarative, condition-based approach.
//!
//! ## Overview
//!
//! Surface rules determine what blocks appear at the terrain surface and
//! underground based on conditions like:
//! - Stone depth (distance from surface)
//! - Y coordinate
//! - Biome
//! - Noise values
//! - Water level
//!
//! ## Block Lookup
//!
//! Block IDs are resolved at apply time via a closure. This allows:
//! - Generated code to live in `unastar_noise` (no dependency on block registry)
//! - Custom blocks for modded generation
//! - Late binding of block names to IDs
//!
//! ```rust,ignore
//! use unastar_noise::surface::*;
//!
//! // Create a rule
//! let rule = BlockRule::new("minecraft:grass_block");
//!
//! // Apply with your own block lookup
//! let block_id = rule.try_apply(&ctx, &|name| {
//!     my_registry.get(name).unwrap_or(0)
//! });
//! ```
//!
//! ## Key Components
//!
//! - [`Condition`] - Trait for conditions that determine when rules apply
//! - [`Rule`] - Trait for rules that produce block states
//! - [`SurfaceContext`] - Context for rule evaluation

pub mod condition;
pub mod context;
pub mod rule;

// Re-export conditions
pub use condition::{
    AbovePreliminarySurface, BiomeCheck, Condition, Hole, LazyCondition, NoiseThreshold, Not,
    Steep, StoneDepthCheck, Temperature, VerticalGradient, WaterCheck, YCheck,
};

// Re-export context
pub use context::{CaveSurface, SurfaceContext, VerticalAnchor};

// Re-export rules
pub use rule::{BandlandsRule, BlockIdRule, BlockRule, Rule, SequenceRule, TestRule};
