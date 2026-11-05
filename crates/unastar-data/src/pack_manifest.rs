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
    /// For generated Rust source: the build-script generator and the crates
    /// that print it. Their versions change the hash as much as pack data
    /// does (a `cargo update` of the formatter is enough).
    pub generated_by: Option<Generator>,
}

/// What produces a generated-source family besides its pack inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Generator {
    /// The build-script module that emits the source.
    pub script: &'static str,
    /// Crates whose locked versions shape the printed text.
    pub formatters: &'static [&'static str],
}

/// Every pack-derived family, in manifest (name) order.
pub fn families() -> Vec<PackFamily> {
    let family = |name, bytes: &'static [u8], count| PackFamily {
        name,
        bytes: Cow::Borrowed(bytes),
        count,
        generated_by: None,
    };
    let mut families = vec![
        PackFamily {
            generated_by: Some(Generator {
                script: "crates/unastar-data/build/actor_components.rs",
                formatters: &["prettyplease", "syn", "quote", "proc-macro2"],
            }),
            ..family(
                "actor_component_bodies",
                crate::actor_components::GENERATED_SOURCE.as_bytes(),
                crate::actor_components::ActorComponent::ALL.len(),
            )
        },
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
            generated_by: None,
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
            generated_by: None,
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
