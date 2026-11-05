//! Reviewable hash/count ratchet for EULA-derived build outputs (D13).

use sha2::{Digest, Sha256};
use unastar_data::actor_definitions;
use unastar_data::pack_manifest::{PackFamily, families};

const REFRESH: &str =
    "refresh with `cargo run -p unastar-data-gen -- --manifest-only` after reviewing the change";

/// Locked versions of `krate` in the workspace `Cargo.lock` (`syn 2.0.112`).
fn locked_versions(krate: &str) -> Vec<String> {
    let lock = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock"),
    )
    .unwrap_or_default();
    let mut versions = Vec::new();
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line == format!("name = \"{krate}\"")
            && let Some(version) = lines
                .next()
                .and_then(|v| v.strip_prefix("version = \""))
                .and_then(|v| v.strip_suffix('"'))
        {
            versions.push(format!("{krate} {version}"));
        }
    }
    versions
}

/// What can change `family`'s hash besides the pinned pack: for generated
/// source, its generator script and the locked formatter versions.
fn refresh_hint(family: &PackFamily) -> String {
    let mut hint = format!("{}: changed pack data", family.name);
    if let Some(generator) = family.generated_by {
        let versions: Vec<String> = generator
            .formatters
            .iter()
            .flat_map(|krate| locked_versions(krate))
            .collect();
        hint.push_str(&format!(
            ", a change to {}, or a `cargo update` of its formatter ({})",
            generator.script,
            versions.join(", ")
        ));
    }
    format!("{hint}; {REFRESH}")
}

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
        let hint = refresh_hint(family);
        assert_eq!(manifest[name]["count"], family.count, "{hint}");
        assert_eq!(
            manifest[name]["sha256"],
            format!("{:x}", Sha256::digest(&family.bytes)),
            "{hint}"
        );
    }
}

#[test]
fn generated_source_failures_name_the_generator_and_formatter_versions() {
    let families = families();
    let bodies = families
        .iter()
        .find(|family| family.name == "actor_component_bodies")
        .unwrap();
    let hint = refresh_hint(bodies);
    assert!(hint.contains("build/actor_components.rs"), "{hint}");
    for krate in ["prettyplease", "syn", "quote"] {
        assert!(hint.contains(&format!("{krate} ")), "{hint}");
    }
    assert!(
        hint.chars().any(|c| c.is_ascii_digit()),
        "no locked versions: {hint}"
    );
    let loot = families.iter().find(|f| f.name == "loot_tables").unwrap();
    assert!(!refresh_hint(loot).contains("cargo update"));
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
