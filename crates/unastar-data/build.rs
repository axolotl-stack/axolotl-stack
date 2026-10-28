//! EULA pack data is read from the pinned submodule, never from committed
//! derived data (D13). Only its hashes/counts are checked in.

#[path = "build/actor_components.rs"]
mod actor_components;
#[path = "build/actor_types.rs"]
mod actor_types;
#[path = "build/actor_wire.rs"]
mod actor_wire;
#[path = "build/actors.rs"]
mod actors;
#[path = "build/biomes.rs"]
mod biomes;
#[path = "build/component_defaults.rs"]
mod component_defaults;
#[path = "build/items_recipes.rs"]
mod items_recipes;
#[path = "build/loot.rs"]
mod loot;
#[path = "build/spawn_rules.rs"]
mod spawn_rules;

use std::path::PathBuf;

fn main() {
    let root =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"));
    let pack = root.join("data/vanilla_bp/behavior_pack");
    assert!(
        pack.join("entities").is_dir() && pack.join("biomes").is_dir(),
        "bedrock-samples is missing: run `git submodule update --init crates/unastar-data/data/vanilla_bp`"
    );
    let native = root.join("../valentine_gen/protocolgen/data/source/inputs/1.26.51/native");
    assert!(
        native.join("biome_id_map.json").is_file(),
        "protocolgen native data is missing: run `git submodule update --init crates/valentine_gen/protocolgen`"
    );
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    actors::write(&pack, &out);
    actor_types::write(&root.join("output/actor_identifiers.json"), &out);
    actor_wire::write(&root.join("../bds-extractor/captures/1.26.51"), &out);
    biomes::write(&pack, &native, &out);
    items_recipes::write(&pack, &out);
    loot::write(
        &pack,
        &root.join("data/overrides/loot_tables/blocks.json"),
        &out,
    );
    spawn_rules::write(&pack, &out);
}
