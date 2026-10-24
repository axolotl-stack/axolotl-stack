//! Java-style decoration: data model, evaluators and feature implementations
//! the generated placed-feature code calls.
//!
//! Layout:
//! - [`java_blocks`] (generated): the pinned Java 1.21.11 block registry, block
//!   tags as bitsets, per-state solid/sturdy flags;
//! - [`blocks`], [`values`], [`random`], [`noise`]: Java value types;
//! - [`eval`]: the [`FeatureWorld`] trait the generator runtime implements
//!   over its 3x3 decoration region, plus predicate/provider evaluators;
//! - [`config`]: static feature configurations;
//! - [`kinds`]: one module per Java feature type, generic over `FeatureWorld`;
//! - [`survival`]: `BlockState.canSurvive` for the blocks features place.

pub mod blocks;
pub mod climate;
pub mod config;
pub mod eval;
pub mod kinds;
pub mod noise;
pub mod placement;
pub mod random;
pub mod survival;
pub mod values;
#[cfg(test)]
pub mod test_world;

/// Generated Java block registry (`java_data/blocks.json` + block tags).
#[allow(clippy::all, missing_docs)]
pub mod java_blocks {
    include!(concat!(env!("OUT_DIR"), "/java_blocks.rs"));
}

pub use blocks::{BlockPos, BlockSet, Direction, Fluid, Heightmap, JavaBlock, JavaState};
/// Generated decoration data and placed-feature code (`features.rs`).
#[allow(clippy::all, missing_docs)]
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/features.rs"));
}

pub use eval::FeatureWorld;
pub use random::{FeatureRandom, LegacyRandom, WorldgenRandom};
