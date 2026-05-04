//! Generated game data for Minecraft Bedrock.
//!
//! This crate provides strongly-typed entity definitions, components,
//! and game data generated from Bedrock behavior packs.
//!
//! ## Architecture
//!
//! Data flows through two stages:
//!
//! 1. **datagen**: Merges vanilla JSON + community overrides → `output/*.kdl`
//! 2. **codegen**: Generates Rust code from KDL → `src/**/*.rs`
//!
//! The KDL output is the "golden artifact" - human-readable, versionable,
//! and consumable by non-Rust tooling.

// Most entity component modules are generated from schemas that currently emit
// explicit `Default` impls. Keep this scoped lint allowance here until the
// generator learns to derive equivalent defaults safely.
#![allow(clippy::derivable_impls)]

pub mod biomes;
pub mod entities;
pub mod pmmp;
pub mod source;
pub mod types;

pub use pmmp::*;
pub use source::*;
pub use types::*;
