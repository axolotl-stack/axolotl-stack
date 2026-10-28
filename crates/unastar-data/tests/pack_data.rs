//! Reviewable hash/count ratchet for EULA-derived build outputs (D13).

use sha2::{Digest, Sha256};
use unastar_data::{actor_definitions, biomes, loot};

#[test]
fn build_generated_pack_data_matches_committed_manifest() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../output/pack-manifest.json")).unwrap();
    for (name, bytes, count) in [
        (
            "actor_component_registry",
            actor_definitions::COMPONENT_SCHEMA_JSON,
            actor_definitions::parser().component_count(),
        ),
        (
            "actor_definitions",
            actor_definitions::VANILLA_JSON,
            actor_definitions::vanilla().len(),
        ),
        ("biomes", biomes::VANILLA_JSON, biomes::vanilla().len()),
        ("loot_tables", loot::VANILLA_JSON, loot::vanilla().len()),
    ] {
        assert_eq!(
            manifest[name]["count"], count,
            "{name}: refresh --manifest-only after reviewing the pack bump"
        );
        assert_eq!(
            manifest[name]["sha256"],
            format!("{:x}", Sha256::digest(bytes)),
            "{name}: refresh --manifest-only after reviewing the pack bump"
        );
    }
}

#[test]
fn actor_components_have_no_invented_defaults() {
    let arrow = actor_definitions::get("minecraft:arrow").unwrap();
    assert!(!arrow.components.contains_key("minecraft:health"));
    assert!(!arrow.components.contains_key("minecraft:inventory"));
    // Arrays and goal priorities were dropped by the previous generator.
    let cow = actor_definitions::get("minecraft:cow").unwrap();
    assert_eq!(cow.components["minecraft:behavior.panic"]["priority"], 1);
    assert_eq!(
        cow.components["minecraft:type_family"]["family"],
        serde_json::json!(["cow", "mob"])
    );
}
