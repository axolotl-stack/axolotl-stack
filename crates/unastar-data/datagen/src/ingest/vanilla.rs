use crate::ir::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, warn};
use walkdir::WalkDir;

use crate::json::remove_json_comments;

/// Raw JSON structure matching behavior pack format
#[derive(Deserialize)]
struct RawEntityFile {
    #[allow(dead_code)]
    format_version: Option<String>,
    #[serde(rename = "minecraft:entity")]
    entity: RawEntity,
}

#[derive(Deserialize)]
struct RawEntity {
    description: RawDescription,
    #[serde(default)]
    components: HashMap<String, serde_json::Value>,
    #[serde(default)]
    component_groups: HashMap<String, HashMap<String, serde_json::Value>>,
    #[serde(default)]
    events: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct RawDescription {
    identifier: String,
    #[serde(default)]
    spawn_category: Option<String>,
    #[serde(default)]
    is_spawnable: bool,
    #[serde(default)]
    is_summonable: bool,
    #[allow(dead_code)]
    #[serde(default)]
    runtime_identifier: Option<String>,
    #[serde(default)]
    properties: HashMap<String, serde_json::Value>,
}

pub fn parse_vanilla_entities(dir: &Path) -> miette::Result<HashMap<String, EntityDef>> {
    let mut entities = HashMap::new();

    if !dir.exists() {
        warn!(
            "Vanilla entities directory does not exist: {}",
            dir.display()
        );
        return Ok(entities);
    }

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
    {
        let path = entry.path();
        debug!("Parsing vanilla: {}", path.display());

        match parse_entity_file(path) {
            Ok(entity) => {
                let name = entity
                    .identifier
                    .strip_prefix("minecraft:")
                    .unwrap_or(&entity.identifier)
                    .to_string();
                entities.insert(name, entity);
            }
            Err(e) => {
                warn!("Failed to parse {}: {}", path.display(), e);
            }
        }
    }

    Ok(entities)
}

fn parse_entity_file(path: &Path) -> miette::Result<EntityDef> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| miette::miette!("Failed to read {}: {}", path.display(), e))?;

    // Remove JSON comments (Bedrock allows // comments)
    let content = remove_json_comments(&content);

    let raw: RawEntityFile = serde_json::from_str(&content)
        .map_err(|e| miette::miette!("Failed to parse {}: {}", path.display(), e))?;

    let mut attribution = Attribution::default();

    // Mark all components as from vanilla
    for name in raw.entity.components.keys() {
        attribution
            .component_sources
            .insert(name.clone(), Source::Vanilla);
    }

    Ok(EntityDef {
        identifier: raw.entity.description.identifier,
        spawn_category: raw.entity.description.spawn_category,
        is_spawnable: raw.entity.description.is_spawnable,
        is_summonable: raw.entity.description.is_summonable,
        runtime_id: None, // Filled in later from minecraft-data
        properties: parse_properties(raw.entity.description.properties),
        components: parse_components(raw.entity.components),
        component_groups: raw
            .entity
            .component_groups
            .into_iter()
            .map(|(k, v)| (k, parse_components(v)))
            .collect(),
        events: parse_events(raw.entity.events),
        attribution,
    })
}

fn parse_properties(raw: HashMap<String, serde_json::Value>) -> HashMap<String, PropertyDef> {
    raw.into_iter()
        .filter_map(|(name, value)| parse_property(&value).map(|p| (name, p)))
        .collect()
}

fn parse_property(value: &serde_json::Value) -> Option<PropertyDef> {
    let obj = value.as_object()?;
    let prop_type = match obj.get("type")?.as_str()? {
        "enum" => {
            let values = obj
                .get("values")?
                .as_array()?
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            PropertyType::Enum { values }
        }
        "int" => {
            let range = obj
                .get("range")
                .and_then(|r| {
                    let arr = r.as_array()?;
                    Some((arr.first()?.as_i64()? as i32, arr.get(1)?.as_i64()? as i32))
                })
                .unwrap_or((0, 100));
            PropertyType::Int { range }
        }
        "float" => {
            let range = obj
                .get("range")
                .and_then(|r| {
                    let arr = r.as_array()?;
                    Some((arr.first()?.as_f64()? as f32, arr.get(1)?.as_f64()? as f32))
                })
                .unwrap_or((0.0, 1.0));
            PropertyType::Float { range }
        }
        "bool" => PropertyType::Bool,
        _ => return None,
    };

    Some(PropertyDef {
        prop_type,
        default: obj
            .get("default")
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default(),
        client_sync: obj
            .get("client_sync")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

fn parse_components(raw: HashMap<String, serde_json::Value>) -> HashMap<String, ComponentValue> {
    raw.into_iter()
        .map(|(name, value)| {
            let comp = if value.as_object().is_some_and(|o| o.is_empty()) {
                ComponentValue::Marker
            } else {
                ComponentValue::Data(value)
            };
            (name, comp)
        })
        .collect()
}

fn parse_events(raw: HashMap<String, serde_json::Value>) -> HashMap<String, EventDef> {
    raw.into_iter()
        .filter_map(|(name, value)| parse_event(&value).map(|e| (name, e)))
        .collect()
}

fn parse_event(value: &serde_json::Value) -> Option<EventDef> {
    let obj = value.as_object()?;

    Some(EventDef {
        add_groups: extract_component_groups(obj.get("add")),
        remove_groups: extract_component_groups(obj.get("remove")),
        set_properties: obj
            .get("set_property")
            .and_then(|v| v.as_object())
            .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default(),
        trigger: obj.get("trigger").and_then(|v| {
            v.as_str().map(String::from).or_else(|| {
                v.as_object()
                    .and_then(|o| o.get("event")?.as_str().map(String::from))
            })
        }),
        sequence: obj
            .get("sequence")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(parse_event).collect())
            .unwrap_or_default(),
        randomize: obj
            .get("randomize")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|entry| {
                        let o = entry.as_object()?;
                        Some(RandomizeEntry {
                            weight: o.get("weight")?.as_i64()? as i32,
                            trigger: o.get("trigger").and_then(|v| v.as_str().map(String::from)),
                            add_groups: extract_component_groups(o.get("add")),
                            remove_groups: extract_component_groups(o.get("remove")),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        filters: obj.get("filters").cloned(),
    })
}

fn extract_component_groups(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.get("component_groups"))
        .and_then(|g| g.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}
