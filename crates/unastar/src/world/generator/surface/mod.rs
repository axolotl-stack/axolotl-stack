//! Surface rules system for declarative block placement.
//!
//! This module re-exports surface types from `unastar_noise` and provides
//! the `SurfaceSystem` which applies rules to terrain chunks.
//!
//! ## Block Lookup
//!
//! Block IDs are resolved at apply time via a closure. This allows the
//! generated rules in `unastar_noise` to work without depending on the
//! block registry. When applying rules, pass a closure to convert block
//! names to IDs:
//!
//! ```rust,ignore
//! use unastar::world::generator::surface::*;
//! use unastar::world::chunk::blocks;
//!
//! let rule: Box<dyn Rule> = build_overworld_surface_rule(seed);
//!
//! // Apply with block lookup closure
//! if let Some(block_id) = rule.try_apply(&ctx, &|name| {
//!     blocks::get_block_id(name)
//! }) {
//!     chunk.set_block(x, y, z, block_id);
//! }
//! ```

// Re-export all surface types from unastar_noise
pub use unastar_noise::surface::*;

// Keep the overworld builder and system locally
mod overworld;
mod system;

pub use overworld::build_overworld_surface_rule;
pub use system::SurfaceSystem;
