//! Emitter for Java decoration: biome table, placed features compiled to
//! straight-line Rust, per-step feature order and biome membership.

use crate::codegen::java_blocks::{JavaBlocks, Result, write_pretty};
use crate::codegen::parser::biome::BiomeJson;
use crate::codegen::parser::feature::FeatureData;
use quote::quote;
use std::collections::HashMap;
use std::path::Path;

/// Emit `features.rs` into `out_dir`.
pub fn emit_features(
    out_dir: &Path,
    biomes: &HashMap<String, BiomeJson>,
    _features: &FeatureData,
    _blocks: &mut JavaBlocks,
) -> Result<()> {
    let mut names: Vec<&String> = biomes.keys().collect();
    names.sort();
    let n = names.len();
    let biome_names = names.iter().map(|s| format!("minecraft:{s}"));
    let climates = names.iter().map(|name| {
        let b = &biomes[*name];
        let has_precipitation = b.has_precipitation;
        let temperature = b.temperature;
        let downfall = b.downfall;
        let frozen = match b.temperature_modifier.as_deref() {
            None | Some("none") => false,
            Some("frozen") => true,
            Some(other) => panic!("biome {name}: unknown temperature_modifier {other}"),
        };
        quote! {
            BiomeClimate {
                has_precipitation: #has_precipitation,
                temperature: #temperature,
                frozen: #frozen,
                downfall: #downfall,
            }
        }
    });
    let tokens = quote! {
        use super::climate::BiomeClimate;

        /// Number of Java biomes (all dimensions).
        pub const BIOME_COUNT: usize = #n;
        /// Java biome ids in index order (sorted by name); `FeatureWorld::biome`
        /// returns indices into this list.
        pub static BIOME_NAMES: [&str; #n] = [#(#biome_names),*];
        /// Climate settings per biome index.
        pub static BIOME_CLIMATE: [BiomeClimate; #n] = [#(#climates),*];

        /// Biome index by id (`minecraft:plains`).
        pub fn biome_index(name: &str) -> Option<u8> {
            BIOME_NAMES.binary_search(&name).ok().map(|i| i as u8)
        }
    };
    write_pretty(&out_dir.join("features.rs"), tokens)
}
