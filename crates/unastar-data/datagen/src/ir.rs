//! Intermediate representation for entity data.
//!
//! This IR is format-agnostic - it represents the merged, normalized data
//! before emission to KDL.

use std::collections::HashMap;

/// Complete entity definition after merging all layers
#[derive(Debug, Clone)]
pub struct EntityDef {
    pub identifier: String,
    pub spawn_category: Option<String>,
    pub is_spawnable: bool,
    pub is_summonable: bool,
    pub runtime_id: Option<u32>,
    pub properties: HashMap<String, PropertyDef>,
    pub components: HashMap<String, ComponentValue>,
    pub component_groups: HashMap<String, HashMap<String, ComponentValue>>,
    pub events: HashMap<String, EventDef>,
    /// Track where each field came from for debugging
    pub attribution: Attribution,
}

#[derive(Debug, Clone, Default)]
pub struct Attribution {
    /// Which source contributed each component
    pub component_sources: HashMap<String, Source>,
}

#[derive(Debug, Clone)]
pub enum Source {
    Defaults,
    Vanilla,
    Override(#[allow(dead_code)] String), // filename
}

/// Entity property (synced enum or value)
#[derive(Debug, Clone)]
pub struct PropertyDef {
    pub prop_type: PropertyType,
    pub default: String,
    pub client_sync: bool,
}

#[derive(Debug, Clone)]
pub enum PropertyType {
    Enum { values: Vec<String> },
    Int { range: (i32, i32) },
    Float { range: (f32, f32) },
    Bool,
}

/// Component value from JSON
#[derive(Debug, Clone)]
pub enum ComponentValue {
    /// Empty component like `"minecraft:physics": {}`
    Marker,
    /// Structured component data
    Data(serde_json::Value),
}

/// Event definition
#[derive(Debug, Clone, Default)]
pub struct EventDef {
    pub add_groups: Vec<String>,
    pub remove_groups: Vec<String>,
    #[allow(dead_code)]
    pub set_properties: HashMap<String, serde_json::Value>,
    pub trigger: Option<String>,
    #[allow(dead_code)]
    pub sequence: Vec<EventDef>,
    pub randomize: Vec<RandomizeEntry>,
    #[allow(dead_code)]
    pub filters: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct RandomizeEntry {
    pub weight: i32,
    pub trigger: Option<String>,
    #[allow(dead_code)]
    pub add_groups: Vec<String>,
    #[allow(dead_code)]
    pub remove_groups: Vec<String>,
}
