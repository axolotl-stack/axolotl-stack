//! Build-time toolchain for unastar's Bedrock-style world generator.
//!
//! The vanilla BDS packs are read from a local BDS install at build time (the
//! Minecraft EULA forbids committing them) and compiled ahead of time into
//! straight-line Rust. Phases:
//!
//! 1. **Ingestion** ([`brarchive`], [`json`], [`packs`]): read `.brarchive`
//!    containers and loose JSON, resolve the version stack of the vanilla packs.
//! 2. **IR** ([`ir`], [`blocks`]): decode documents into typed structs, resolve
//!    pre-flattening block names and block states to canonical runtime ids.
//! 3. **Analysis**: compile the Molang subset ([`molang`]), resolve biome
//!    filters per biome, compute the features reachable from overworld rules.
//! 4. **Emission** ([`emit`]): `quote` + `prettyplease` Rust modules that the
//!    `unastar` build script writes to `OUT_DIR`.

pub mod blocks;
pub mod brarchive;
pub mod builtin;
pub mod builtin_blocks;
pub mod emit;
pub mod error;
pub mod json;
pub mod ir;
pub mod molang;
pub mod model;
pub mod nbt;
pub mod packs;
pub mod structures;

pub use error::{Error, Result};
