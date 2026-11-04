//! The pack-derived build outputs recorded in `output/pack-manifest.json`.
//!
//! EULA-derived data is generated into `OUT_DIR` from the pinned submodules
//! and never committed (D13); the manifest commits only a SHA-256 and a count
//! per family, so a pack or generator bump shows up as a reviewable diff.
//! [`families`] is the one list both the datagen writer
//! (`unastar-data-gen --manifest-only`) and the `pack_data` test use, so a
//! family cannot be written without being checked or the other way round.

use std::borrow::Cow;

/// File name of the committed manifest under `output/`.
pub const ARTIFACT: &str = "pack-manifest.json";

/// One hashed build output.
pub struct PackFamily {
    /// Key in the manifest.
    pub name: &'static str,
    /// The exact bytes the manifest hashes.
    pub bytes: Cow<'static, [u8]>,
    /// Number of entries the family decodes to.
    pub count: usize,
}

/// Every pack-derived family, in manifest (name) order.
pub fn families() -> Vec<PackFamily> {
    let family = |name, bytes: &'static [u8], count| PackFamily {
        name,
        bytes: Cow::Borrowed(bytes),
        count,
    };
    let mut families = vec![
        family(
            "actor_component_bodies",
            crate::actor_components::GENERATED_SOURCE.as_bytes(),
            crate::actor_components::ActorComponent::ALL.len(),
        ),
        family(
            "actor_component_registry",
            crate::actor_definitions::COMPONENT_SCHEMA_JSON,
            crate::actor_definitions::parser().component_count(),
        ),
        family(
            "actor_definitions",
            crate::actor_definitions::VANILLA_JSON,
            crate::actor_definitions::vanilla().len(),
        ),
        family(
            "actor_wire",
            crate::actor_wire::CATALOG_JSON,
            crate::actor_wire::vanilla().observations.len(),
        ),
        family(
            "biomes",
            crate::biomes::VANILLA_JSON,
            crate::biomes::vanilla().len(),
        ),
        family(
            "component_schema_defaults",
            crate::actor_definitions::COMPONENT_DEFAULTS_JSON,
            crate::actor_definitions::component_defaults_count(),
        ),
        PackFamily {
            name: "item_files",
            bytes: Cow::Owned(bundle(crate::item_components::VANILLA_ITEM_FILES)),
            count: crate::item_components::VANILLA_ITEM_FILES.len(),
        },
        family(
            "loot_tables",
            crate::loot::VANILLA_JSON,
            crate::loot::vanilla().len(),
        ),
        PackFamily {
            name: "recipe_files",
            bytes: Cow::Owned(bundle(crate::recipes::VANILLA_RECIPE_FILES)),
            count: crate::recipes::VANILLA_RECIPE_FILES.len(),
        },
        family(
            "spawn_rules",
            crate::spawn_rules::VANILLA_JSON,
            crate::spawn_rules::vanilla().len(),
        ),
    ];
    families.sort_by_key(|family| family.name);
    families
}

/// The hashed form of a bundled document family: each `(origin, canonical
/// JSON)` pair as `origin\njson\n`, in bundle order.
fn bundle(files: &[(&str, &str)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (origin, json) in files {
        for part in [origin, json] {
            bytes.extend_from_slice(part.as_bytes());
            bytes.push(b'\n');
        }
    }
    bytes
}
