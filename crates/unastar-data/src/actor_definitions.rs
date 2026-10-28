//! Vanilla actor definitions, built from bedrock-samples into `OUT_DIR`,
//! embedded in the binary, and decoded once at startup (D2/D13).
//!
//! The shared pack loader preserves component bodies, groups, event trees,
//! properties and description data. No per-entity Rust is generated and no
//! unmeasured defaults are merged into pack components.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};
use unastar_packs::actor::{self, Loaded};
use unastar_rules::filter::FilterRegistry;

pub use unastar_packs::actor::{ActorDefinition, Description, PropertyDef, PropertyKind};

/// Canonical normalised JSON generated from the pinned pack at build time.
/// Exposed for the manifest writer; gameplay consumes [`vanilla`] / [`get`].
pub const VANILLA_JSON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/actor_definitions.json"));

/// Build-generated component registry from pinned stable Mojang schemas.
pub const COMPONENT_SCHEMA_JSON: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/actor_component_registry.json"));

/// Published component field defaults, resolved from the pinned stable schemas
/// at build time. This is not an invented per-actor default layer.
pub const COMPONENT_DEFAULTS_JSON: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/component_schema_defaults.json"));

type ComponentDefaults = BTreeMap<String, BTreeMap<String, serde_json::Value>>;

static COMPONENT_DEFAULTS: LazyLock<ComponentDefaults> = LazyLock::new(|| {
    serde_json::from_slice(COMPONENT_DEFAULTS_JSON).expect("embedded component schema defaults")
});

/// Startup compiler input only: resolve omitted fields before creating typed
/// prototypes. Explicit pack values, including false/zero, take precedence.
pub fn component_field_default(component: &str, field: &str) -> Option<&'static serde_json::Value> {
    COMPONENT_DEFAULTS.get(component)?.get(field)
}

pub fn component_defaults_count() -> usize {
    COMPONENT_DEFAULTS.len()
}

static PARSER: LazyLock<actor::ActorParser<'static>> = LazyLock::new(|| {
    actor::ActorParser::from_schema(
        FilterRegistry::vanilla(),
        serde_json::from_slice(COMPONENT_SCHEMA_JSON).expect("embedded actor component registry"),
    )
    .expect("embedded actor parser registry")
});

pub fn parser() -> &'static actor::ActorParser<'static> {
    &PARSER
}

static VANILLA: LazyLock<Vec<Arc<ActorDefinition>>> = LazyLock::new(|| {
    let rows: Vec<serde_json::Value> =
        serde_json::from_slice(VANILLA_JSON).expect("embedded actor JSON");
    rows.iter()
        .map(|row| {
            match actor::load_value("embedded actor definitions", row, parser())
                .expect("embedded normalised actor definition")
            {
                Loaded::Definition {
                    definition,
                    diagnostics,
                } => {
                    assert!(
                        diagnostics.is_empty(),
                        "embedded normalised actor diagnostics: {diagnostics:?}"
                    );
                    Arc::from(definition)
                }
                Loaded::Skipped { reason } => panic!("embedded actor definition skipped: {reason}"),
            }
        })
        .collect()
});

/// Decoded vanilla definitions in identifier order (shared for the process).
pub fn vanilla() -> &'static [Arc<ActorDefinition>] {
    &VANILLA
}

pub fn get(identifier: &str) -> Option<&'static Arc<ActorDefinition>> {
    let rows = vanilla();
    rows.binary_search_by(|row| row.identifier().cmp(identifier))
        .ok()
        .map(|i| &rows[i])
}
