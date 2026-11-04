//! Reviewable hash/count ratchet for EULA-derived build outputs (D13).

use sha2::{Digest, Sha256};
use unastar_data::actor_definitions;
use unastar_data::pack_manifest::families;

const REFRESH: &str =
    "refresh with `cargo run -p unastar-data-gen -- --manifest-only` after reviewing the pack bump";

#[test]
fn build_generated_pack_data_matches_committed_manifest() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../output/pack-manifest.json")).unwrap();
    let families = families();
    let recorded: Vec<&str> = manifest
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let built: Vec<&str> = families.iter().map(|family| family.name).collect();
    assert_eq!(recorded, built, "manifest families: {REFRESH}");
    for family in &families {
        let name = family.name;
        assert_eq!(manifest[name]["count"], family.count, "{name}: {REFRESH}");
        assert_eq!(
            manifest[name]["sha256"],
            format!("{:x}", Sha256::digest(&family.bytes)),
            "{name}: {REFRESH}"
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
