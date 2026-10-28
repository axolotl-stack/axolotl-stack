//! Bedrock-style world generation targeting BDS 1.26.51.
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
//! - [`surface`]: Bedrock surface builders parameterised by biome data;
//! - [`structures`]: data-driven jigsaw structures (assembly, placement,
//!   terrain adaptation, processors) and the template tables shared with the
//!   template-based features;
//! - [`adapter`]: Bedrock block queries over the shared 3x3 decoration region;
//! - [`decorate`]: per-chunk seeding and the pass driver.
//!
//! # Measurement boundary
//!
//! **unmeasured: to confirm by BDS fixture** applies to per-chunk/feature RNG
//! seeding and draw order, coordinate/distribution semantics, built-in feature
//! geometry and selectors, JSON feature placement details, surface noise and
//! builder formulas, carver paths, jigsaw placement and processors. The data
//! schemas and emitted numeric identities are checked against local install
//! inputs; that does not establish runtime BDS parity. The codegen README lists
//! the measurement categories and the reference controls needed for each.

pub mod adapter;
pub mod builtin;
pub mod decorate;
pub mod features;
mod generator;
mod geology;
pub mod molang_noise;
pub mod rng;
pub mod scatter;
mod simplex;
pub mod structures;
pub mod surface;
pub mod world;

pub use generator::BedrockGenerator;

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
        (self.0[0] & mask[0])
            | (self.0[1] & mask[1])
            | (self.0[2] & mask[2])
            | (self.0[3] & mask[3])
            != 0
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
