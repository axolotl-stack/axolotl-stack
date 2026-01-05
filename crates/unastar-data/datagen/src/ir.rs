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
    Override(String), // filename
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
#[derive(Debug, Clone)]
pub struct EventDef {
    pub add_groups: Vec<String>,
    pub remove_groups: Vec<String>,
    pub set_properties: HashMap<String, serde_json::Value>,
    pub trigger: Option<String>,
    pub sequence: Vec<EventDef>,
    pub randomize: Vec<RandomizeEntry>,
    pub filters: Option<serde_json::Value>,
}

impl Default for EventDef {
    fn default() -> Self {
        Self {
            add_groups: Vec::new(),
            remove_groups: Vec::new(),
            set_properties: HashMap::new(),
            trigger: None,
            sequence: Vec::new(),
            randomize: Vec::new(),
            filters: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RandomizeEntry {
    pub weight: i32,
    pub trigger: Option<String>,
    pub add_groups: Vec<String>,
    pub remove_groups: Vec<String>,
}
