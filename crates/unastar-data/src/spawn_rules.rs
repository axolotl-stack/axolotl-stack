//! Typed spawn rules (`spawn_rules/*.json`, D2).
//!
//! [`body`] is generated at build time from the pinned `server/spawn`
//! schemas (`build/spawn_rules.rs`, the actor component lowering: closed
//! structs, published defaults, filters kept as [`FilterJson`] for
//! `unastar_rules`). The vanilla documents are embedded as canonical JSON
//! and decoded once into these types.
//!
//! [`FilterJson`]: crate::actor_components::FilterJson

use serde::Deserialize;
use std::sync::LazyLock;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/spawn_rules.rs"));
}

pub use generated::body;

/// Build-generated canonical JSON rows (`{source, document}`).
pub const VANILLA_JSON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/spawn_rules.json"));

/// One `spawn_rules` document.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnRulesDocument {
    pub format_version: String,
    #[serde(rename = "minecraft:spawn_rules")]
    pub spawn_rules: body::SpawnRules,
}

/// A decoded spawn rule file and the pack entry it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnRuleFile {
    /// Pack entry, e.g. `cow.json`.
    pub source: String,
    pub document: SpawnRulesDocument,
}

impl SpawnRuleFile {
    /// `description.identifier`, e.g. `minecraft:cow`.
    pub fn identifier(&self) -> &str {
        &self.document.spawn_rules.description.identifier
    }
}

/// A spawn rule document that does not match the typed schema.
#[derive(Debug, thiserror::Error)]
#[error("spawn rules {source_name} at {path}: {message}")]
pub struct SpawnRuleDecodeError {
    pub source_name: String,
    /// Failing field path (`.` for the document itself).
    pub path: String,
    pub message: String,
}

/// Decode one spawn rule document; the error names the failing field path.
pub fn decode(
    source: &str,
    document: &serde_json::Value,
) -> Result<SpawnRuleFile, SpawnRuleDecodeError> {
    serde_path_to_error::deserialize(document)
        .map(|document| SpawnRuleFile {
            source: source.to_owned(),
            document,
        })
        .map_err(|error| SpawnRuleDecodeError {
            source_name: source.to_owned(),
            path: error.path().to_string(),
            message: error.inner().to_string(),
        })
}

#[derive(Deserialize)]
struct Row {
    source: String,
    document: serde_json::Value,
}

static VANILLA: LazyLock<Vec<SpawnRuleFile>> = LazyLock::new(|| {
    let rows: Vec<Row> = serde_json::from_slice(VANILLA_JSON).expect("embedded spawn rule JSON");
    rows.iter()
        .map(|row| decode(&row.source, &row.document).unwrap_or_else(|e| panic!("{e}")))
        .collect()
});

/// Every vanilla spawn rule file, in pack entry order.
pub fn vanilla() -> &'static [SpawnRuleFile] {
    &VANILLA
}

/// Decode every embedded vanilla document without panicking (for tests).
pub fn decode_vanilla() -> Vec<Result<SpawnRuleFile, SpawnRuleDecodeError>> {
    let rows: Vec<Row> = serde_json::from_slice(VANILLA_JSON).expect("embedded spawn rule JSON");
    rows.iter()
        .map(|row| decode(&row.source, &row.document))
        .collect()
}
