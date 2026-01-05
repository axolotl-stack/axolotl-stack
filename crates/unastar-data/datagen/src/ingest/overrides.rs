use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, warn};
use walkdir::WalkDir;

/// Parsed override data to apply on top of vanilla
#[derive(Debug, Default)]
pub struct Overrides {
    pub defaults: DefaultsOverride,
    pub entities: HashMap<String, EntityOverride>,
}

#[derive(Debug, Default)]
pub struct DefaultsOverride {
    pub components: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Default)]
pub struct EntityOverride {
    pub components: HashMap<String, serde_json::Value>,
    pub meta: OverrideMeta,
}

#[derive(Debug, Default)]
pub struct OverrideMeta {
    pub verified_version: Option<String>,
    pub verified_by: Option<String>,
    pub notes: Option<String>,
}

pub fn parse_overrides(dir: &Path) -> miette::Result<Overrides> {
    let mut overrides = Overrides::default();

    if !dir.exists() {
        debug!("Overrides directory does not exist: {}", dir.display());
        return Ok(overrides);
    }

    // Parse _defaults.kdl
    let defaults_path = dir.join("_defaults.kdl");
    if defaults_path.exists() {
        debug!("Parsing defaults: {}", defaults_path.display());
        match parse_defaults_kdl(&defaults_path) {
            Ok(defaults) => overrides.defaults = defaults,
            Err(e) => warn!("Failed to parse defaults: {}", e),
        }
    }

    // Parse entity overrides
    let entities_dir = dir.join("entities");
    if entities_dir.exists() {
        for entry in WalkDir::new(&entities_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "kdl"))
        {
            let path = entry.path();
            debug!("Parsing override: {}", path.display());

            match parse_entity_override_kdl(path) {
                Ok((name, entity_override)) => {
                    overrides.entities.insert(name, entity_override);
                }
                Err(e) => {
                    warn!("Failed to parse {}: {}", path.display(), e);
                }
            }
        }
    }

    Ok(overrides)
}

fn parse_defaults_kdl(path: &Path) -> miette::Result<DefaultsOverride> {
    let content =
        std::fs::read_to_string(path).map_err(|e| miette::miette!("Failed to read: {}", e))?;

    let doc: kdl::KdlDocument = content
        .parse()
        .map_err(|e| miette::miette!("KDL parse error: {:?}", e))?;

    let mut defaults = DefaultsOverride::default();

    for node in doc.nodes() {
        if node.name().value() == "defaults" {
            // Parse children as component defaults
            if let Some(children) = node.children() {
                for child in children.nodes() {
                    let comp_name = format!("minecraft:{}", child.name().value());
                    let value = kdl_node_to_json(child);
                    defaults.components.insert(comp_name, value);
                }
            }
        }
    }

    Ok(defaults)
}

fn parse_entity_override_kdl(path: &Path) -> miette::Result<(String, EntityOverride)> {
    let content =
        std::fs::read_to_string(path).map_err(|e| miette::miette!("Failed to read: {}", e))?;

    let doc: kdl::KdlDocument = content
        .parse()
        .map_err(|e| miette::miette!("KDL parse error: {}", e))?;

    let mut entity_override = EntityOverride::default();
    let mut entity_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    for node in doc.nodes() {
        if node.name().value() == "entity" {
            // Get entity identifier from first argument
            if let Some(arg) = node.entries().first() {
                if let Some(s) = arg.value().as_string() {
                    entity_name = s
                        .strip_prefix("minecraft:")
                        .unwrap_or(s)
                        .to_string();
                }
            }

            // Parse children
            if let Some(children) = node.children() {
                for child in children.nodes() {
                    let name = child.name().value();

                    if name == "_meta" {
                        // Parse metadata
                        if let Some(meta_children) = child.children() {
                            for meta_node in meta_children.nodes() {
                                match meta_node.name().value() {
                                    "verified_version" => {
                                        entity_override.meta.verified_version = meta_node
                                            .entries()
                                            .first()
                                            .and_then(|e| e.value().as_string().map(String::from));
                                    }
                                    "verified_by" => {
                                        entity_override.meta.verified_by = meta_node
                                            .entries()
                                            .first()
                                            .and_then(|e| e.value().as_string().map(String::from));
                                    }
                                    "notes" => {
                                        entity_override.meta.notes = meta_node
                                            .entries()
                                            .first()
                                            .and_then(|e| e.value().as_string().map(String::from));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    } else {
                        // Component override
                        let comp_name = if name.starts_with("minecraft:") {
                            name.to_string()
                        } else {
                            format!("minecraft:{}", name)
                        };
                        let value = kdl_node_to_json(child);
                        entity_override.components.insert(comp_name, value);
                    }
                }
            }
        }
    }

    Ok((entity_name, entity_override))
}

/// Convert a KDL node to a JSON value
fn kdl_node_to_json(node: &kdl::KdlNode) -> serde_json::Value {
    let mut obj = serde_json::Map::new();

    // Add properties from entries
    for entry in node.entries() {
        if let Some(name) = entry.name() {
            obj.insert(name.value().to_string(), kdl_value_to_json(entry.value()));
        }
    }

    // If node has children, recurse
    if let Some(children) = node.children() {
        for child in children.nodes() {
            let child_value = kdl_node_to_json(child);
            obj.insert(child.name().value().to_string(), child_value);
        }
    }

    // If no properties and no children, return the first argument as value
    if obj.is_empty() {
        if let Some(arg) = node.entries().first() {
            if arg.name().is_none() {
                return kdl_value_to_json(arg.value());
            }
        }
        return serde_json::Value::Object(serde_json::Map::new());
    }

    serde_json::Value::Object(obj)
}

fn kdl_value_to_json(value: &kdl::KdlValue) -> serde_json::Value {
    match value {
        kdl::KdlValue::String(s) => serde_json::Value::String(s.clone()),
        kdl::KdlValue::Integer(n) => {
            serde_json::Value::Number(serde_json::Number::from(*n as i64))
        }
        kdl::KdlValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        kdl::KdlValue::Bool(b) => serde_json::Value::Bool(*b),
        kdl::KdlValue::Null => serde_json::Value::Null,
    }
}
