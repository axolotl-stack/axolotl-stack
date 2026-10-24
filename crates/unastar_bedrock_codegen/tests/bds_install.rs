//! Ingestion checks against the local BDS 1.26.51 install.
//!
//! The pack files are under the Minecraft EULA and are never committed, so
//! these tests read the install named by `UNASTAR_BDS_DIR` and skip (with a
//! message) when it is not set.

use std::path::PathBuf;
use unastar_bedrock_codegen::packs::{Family, PackStack};

fn bds_dir() -> Option<PathBuf> {
    match std::env::var_os("UNASTAR_BDS_DIR") {
        Some(dir) => Some(PathBuf::from(dir)),
        None => {
            eprintln!("skipping: UNASTAR_BDS_DIR is not set (point it at a BDS 1.26.51 server directory)");
            None
        }
    }
}

#[test]
fn resolves_the_1_26_51_stack() {
    let Some(dir) = bds_dir() else { return };
    let stack = PackStack::open(&dir).unwrap();
    assert_eq!(stack.layers[0].name, "definitions");
    assert_eq!(stack.layers[1].name, "vanilla");
    assert_eq!(stack.layers.last().unwrap().name, "vanilla_1.26.51");

    let rules = stack.resolve(Family::FeatureRules).unwrap();
    let features = stack.resolve(Family::Features).unwrap();
    let biomes = stack.resolve(Family::Biomes).unwrap();
    assert_eq!(rules.len(), 267);
    assert_eq!(features.len(), 290);
    assert_eq!(biomes.len(), 89);
    // Since the 1.18 ore rework the old coal rule is disabled by the newest pack.
    let coal = &rules["minecraft:overworld_underground_coal_ore_feature"];
    assert!(coal.source.starts_with("vanilla_"), "{}", coal.source);
    assert_eq!(coal.body["distribution"]["iterations"], 0);
}
