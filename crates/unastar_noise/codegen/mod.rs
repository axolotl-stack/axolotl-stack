//! AOT worldgen toolchain (build-time only).
//!
//! Phases: `parser` (worldgen JSON -> IR), `data` (other data families -> tables),
//! `analyzer` (density-function graph analysis), `emitter` (IR -> Rust modules).

pub mod analyzer;
pub mod data;
pub mod emitter;
pub mod jar;
pub mod java_blocks;
pub mod parser;
pub mod templates;
