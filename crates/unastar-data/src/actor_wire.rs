//! Retained BDS wire observations, compiled into `OUT_DIR` and decoded once.
//!
//! These are sampled native packet facts, not behavior-pack defaults. In
//! particular, a sampled current attribute may reflect spawn events or native
//! state at capture time; it must not be treated as an authored or universal
//! default. Attribute order and summon-to-emitted actor aliases are preserved.
//! Property type codes contain only types actually matched between native
//! descriptors and canonical pack declarations; absent codes are unmeasured.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Canonical observations generated from hash-verified retained captures.
/// Manifest generation uses these bytes; startup compilation uses [`vanilla`].
pub const CATALOG_JSON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/actor_wire.json"));

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ActorWireCatalog {
    pub server_version: String,
    pub protocol_version: u32,
    /// Hashes of the retained capture manifests, which are the input trust roots.
    pub operator_manifest_sha256: String,
    pub member_manifest_sha256: String,
    /// Canonical pack declarations used to identify observed property type codes.
    pub actor_definitions_sha256: String,
    /// Every consumed manifest-listed file, in relative path order.
    pub sources: Vec<ActorWireSource>,
    /// Native summon observations in summon identifier order.
    pub observations: Vec<ActorObservation>,
    /// Emitted descriptor identifiers in actual member join packet order.
    pub property_descriptor_order: Vec<String>,
    /// Declared pack type name to observed native numeric code. Missing types
    /// have no measured code; callers must not invent one.
    pub property_types: BTreeMap<String, i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ActorWireSource {
    /// Relative to the retained version's capture directory (including role).
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ActorObservation {
    /// Identifier supplied to the native summon command.
    pub identifier: String,
    /// Actual actor identifier emitted by BDS, including native aliases.
    pub actor_type: String,
    /// Attribute order as emitted in the native AddActor packet.
    pub attributes: Vec<ObservedAttribute>,
    /// SHA256 of the exact retained [Header][Body] packet bytes.
    pub packet_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ObservedAttribute {
    pub name: String,
    pub min: f32,
    /// Sampled packet state, NOT a universal or authored default. Effective
    /// authored values and spawn-event composition remain separate inputs.
    pub observed_current: f32,
    pub max: f32,
}

static VANILLA: LazyLock<ActorWireCatalog> = LazyLock::new(|| {
    serde_json::from_slice(CATALOG_JSON).expect("embedded actor wire observations")
});

/// Shared typed catalog, decoded once for startup compilation.
pub fn vanilla() -> &'static ActorWireCatalog {
    &VANILLA
}

/// Cold/startup lookup by the identifier supplied to summon, not emitted alias.
/// Runtime callers should resolve names once into their own scoped handles.
pub fn get(identifier: &str) -> Option<&'static ActorObservation> {
    let observations = &vanilla().observations;
    observations
        .binary_search_by(|row| row.identifier.as_str().cmp(identifier))
        .ok()
        .map(|index| &observations[index])
}

#[cfg(test)]
#[path = "../build/actor_wire.rs"]
mod compiler;
