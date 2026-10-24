//! Bedrock-style world generation (BDS 1.26.51 parity), `kind = "bedrock"`.
//!
//! The vanilla data (feature rules, features, biome surface parameters) is
//! read from a local BDS install at build time and compiled to Rust by
//! `unastar_bedrock_codegen` (see `crates/unastar/build.rs`); `generated`
//! includes that output. This module holds the runtime the compiled code calls:
//!
//! - [`world`]: block access ([`world::BlockWorld`]) and the decoration
//!   context ([`world::Ctx`]);
//! - [`rng`]: Bedrock's MT19937 random;
//! - [`scatter`]: coordinate distributions of rules and scatter features;
//! - [`features`]: algorithms of the JSON feature types (ore, tree, ...);
//! - [`builtin`]: BDS built-in features (no JSON definition), registered under
//!   their identifiers;
//! - [`surface`]: Bedrock surface builders parameterised by biome data.

pub mod builtin;
pub mod features;
pub mod rng;
pub mod scatter;
pub mod surface;
pub mod world;

/// Output of `unastar_bedrock_codegen` for the local BDS install.
#[allow(
    clippy::all,
    clippy::pedantic,
    unused_parens,
    unused_braces,
    unreachable_code,
    non_upper_case_globals
)]
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/bedrock_worldgen.rs"));
}

/// BDS placement passes, in execution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pass {
    Pregeneration,
    First,
    BeforeUnderground,
    Underground,
    AfterUnderground,
    BeforeSurface,
    Surface,
    AfterSurface,
    Final,
}

impl Pass {
    /// Every pass in execution order.
    pub const ALL: [Pass; 9] = [
        Pass::Pregeneration,
        Pass::First,
        Pass::BeforeUnderground,
        Pass::Underground,
        Pass::AfterUnderground,
        Pass::BeforeSurface,
        Pass::Surface,
        Pass::AfterSurface,
        Pass::Final,
    ];
}

/// Set of numeric biome ids (`< 256`) present in a chunk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BiomeSet([u64; 4]);

impl BiomeSet {
    #[inline]
    pub fn insert(&mut self, id: u16) {
        let id = id as usize;
        debug_assert!(id < 256, "biome id {id} outside the set");
        self.0[(id / 64) & 3] |= 1 << (id % 64);
    }

    #[inline]
    pub fn contains(&self, id: u16) -> bool {
        let id = id as usize;
        id < 256 && self.0[id / 64] >> (id % 64) & 1 == 1
    }

    /// `true` when any id is in both sets (compiled rule masks use this).
    #[inline]
    pub fn intersects(&self, mask: &[u64; 4]) -> bool {
        (self.0[0] & mask[0]) | (self.0[1] & mask[1]) | (self.0[2] & mask[2]) | (self.0[3] & mask[3]) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.0 == [0; 4]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biome_set_masks() {
        let mut set = BiomeSet::default();
        assert!(set.is_empty());
        set.insert(1);
        set.insert(192);
        assert!(set.contains(192) && !set.contains(2));
        assert!(set.intersects(&[0b10, 0, 0, 0]));
        assert!(set.intersects(&[0, 0, 0, 1]));
        assert!(!set.intersects(&[0b100, 0, 0, 0]));
    }
}
