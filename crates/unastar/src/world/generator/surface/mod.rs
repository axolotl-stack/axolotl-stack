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
//! ## Key Components
//!
//! - [`Condition`] - Trait for conditions that determine when rules apply
//! - [`Rule`] - Trait for rules that produce block states
//! - [`SurfaceContext`] - Context for rule evaluation
//! - [`SurfaceSystem`] - Main system that applies rules to terrain
//!
//! ## Example
//!
//! ```rust,ignore
//! use unastar::world::generator::surface::*;
//!
//! // Build a simple surface rule
//! let rule = SequenceRule {
//!     rules: vec![
//!         // Desert biome -> sand
//!         Box::new(TestRule {
//!             condition: Box::new(BiomeCheck { biomes: vec![Biome::Desert] }),
//!             then_run: Box::new(BlockRule { block: SAND }),
//!         }),
//!         // Default -> grass
//!         Box::new(BlockRule { block: GRASS_BLOCK }),
//!     ],
//! };
//! ```

mod condition;
mod context;
mod overworld;
mod rule;
mod system;

// Re-export conditions
pub use condition::{
    AbovePreliminarySurface, BiomeCheck, Condition, Hole, LazyCondition, NoiseThreshold, Not,
    Steep, StoneDepthCheck, Temperature, VerticalGradient, WaterCheck, YCheck,
};

// Re-export context
pub use context::{CaveSurface, SurfaceContext, VerticalAnchor};

// Re-export rules
pub use rule::{BandlandsRule, BlockRule, Rule, SequenceRule, TestRule};

// Re-export system
pub use system::SurfaceSystem;

// Re-export overworld builder
pub use overworld::build_overworld_surface_rule;
