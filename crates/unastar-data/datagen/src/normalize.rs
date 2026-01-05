//! Normalize polymorphic types in entity data.
//!
//! This module handles Bedrock's inconsistent JSON types:
//! - `fuse_length`: can be `1.5` (fixed) or `{range_min, range_max}` (random range)
//! - `damage`: can be `3` (fixed) or `[1, 5]` (array range)
//! - `deals_damage`: can be `false` (bool) or `"no"` (string)
//!
//! Currently a placeholder - normalization happens during codegen.

// Future: Add normalization passes here if needed
