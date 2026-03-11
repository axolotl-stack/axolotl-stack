//! Component schema definitions and defaults loaded from vendored upstream metadata.
//!
//! Type definitions come from the Blockception Bedrock entity JSON Schema.
//! Default values come from Mojang creator-tools entity forms, with local KDL
//! overrides layered on top for project-specific corrections.

use heck::{ToPascalCase, ToSnakeCase};
use kdl::{KdlDocument, KdlValue};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    Float,
    Integer,
    Bool,
    String,
    Option(Box<FieldType>),
}

#[derive(Debug, Clone)]
pub struct FieldSchema {
    pub name: String,
    pub rust_name: String,
    pub field_type: FieldType,
    pub is_primary: bool,
}

#[derive(Debug, Clone)]
pub struct ComponentSchema {
    pub name: String,
    pub rust_name: String,
    pub description: Option<String>,
    pub fields: Vec<FieldSchema>,
    pub is_marker: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ComponentDefaults {
    pub values: HashMap<String, HashMap<String, String>>,
    pub primary_values: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct SchemaCatalog {
    pub components: HashMap<String, ComponentSchema>,
    pub defaults: ComponentDefaults,
}

impl SchemaCatalog {
    pub fn load(upstream_dir: &Path, defaults_override_path: &Path) -> miette::Result<Self> {
        let components = load_blockception_component_schemas(
            &upstream_dir.join("blockception/entities.schema.json"),
        )?;

        let mut defaults =
            load_mojang_component_defaults(&upstream_dir.join("mojang/entity_forms"))?;
        if defaults_override_path.exists() {
            defaults.merge_kdl(defaults_override_path)?;
        }

        Ok(Self {
            components,
            defaults,
        })
    }
}

impl ComponentDefaults {
    pub fn merge_kdl(&mut self, defaults_path: &Path) -> miette::Result<()> {
        let content = std::fs::read_to_string(defaults_path)
            .map_err(|e| miette::miette!("Failed to read defaults: {}", e))?;

        let doc: KdlDocument = content
            .parse()
            .map_err(|e| miette::miette!("Failed to parse defaults KDL: {}", e))?;

        for node in doc.nodes() {
            if node.name().value() != "defaults" {
                continue;
            }

            let Some(children) = node.children() else {
                continue;
            };

            for child in children.nodes() {
                let comp_name = child.name().value().to_string();
                let mut field_defaults = HashMap::new();

                for entry in child.entries() {
                    if entry.name().is_none() {
                        self.primary_values
                            .insert(comp_name.clone(), kdl_value_to_string(entry.value()));
                    } else if let Some(prop_name) = entry.name() {
                        field_defaults.insert(
                            prop_name.value().to_string(),
                            kdl_value_to_string(entry.value()),
                        );
                    }
                }

                if let Some(comp_children) = child.children() {
                    for prop_node in comp_children.nodes() {
                        let prop_name = prop_node.name().value();
                        if prop_name.starts_with("//") {
                            continue;
                        }
                        if let Some(entry) = prop_node.entries().first() {
                            field_defaults
                                .insert(prop_name.to_string(), kdl_value_to_string(entry.value()));
                        }
                    }
                }

                if !field_defaults.is_empty() {
                    self.values
                        .entry(comp_name)
                        .or_default()
                        .extend(field_defaults);
                }
            }
        }

        Ok(())
    }

    pub fn get_field_default(&self, component: &str, field: &str) -> Option<&str> {
        self.values
            .get(component)
            .and_then(|fields| fields.get(field))
            .map(|s| s.as_str())
    }

    pub fn get_primary_default(&self, component: &str) -> Option<&str> {
        self.primary_values.get(component).map(|s| s.as_str())
    }

    fn merge_field_default(&mut self, component: &str, field: &str, value: String) {
        self.values
            .entry(component.to_string())
            .or_default()
            .entry(field.to_string())
            .or_insert(value);
    }

    fn merge_primary_default(&mut self, component: &str, value: String) {
        self.primary_values
            .entry(component.to_string())
            .or_insert(value);
    }
}

fn load_mojang_component_defaults(forms_dir: &Path) -> miette::Result<ComponentDefaults> {
    let mut defaults = ComponentDefaults::default();

    if !forms_dir.exists() {
        return Ok(defaults);
    }

    for entry in std::fs::read_dir(forms_dir)
        .map_err(|e| miette::miette!("Failed to read Mojang forms dir: {}", e))?
    {
        let entry =
            entry.map_err(|e| miette::miette!("Failed to read Mojang form entry: {}", e))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| miette::miette!("Failed to read {}: {}", path.display(), e))?;
        let json: Value = serde_json::from_str(&content)
            .map_err(|e| miette::miette!("Failed to parse {}: {}", path.display(), e))?;

        let Some(component_id) = json.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if !component_id.starts_with("minecraft:") {
            continue;
        }

        let component_name = normalize_component_name(component_id);
        let Some(fields) = json.get("fields").and_then(|v| v.as_array()) else {
            continue;
        };

        for field in fields {
            let Some(field_name) = field.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(default_value) = field.get("defaultValue") else {
                continue;
            };
            let Some(value) = simple_json_to_string(default_value) else {
                continue;
            };

            defaults.merge_field_default(&component_name, field_name, value.clone());
            if field_name == "value" {
                defaults.merge_primary_default(&component_name, value);
            }
        }
    }

    Ok(defaults)
}

fn load_blockception_component_schemas(
    schema_path: &Path,
) -> miette::Result<HashMap<String, ComponentSchema>> {
    if !schema_path.exists() {
        return Ok(HashMap::new());
    }

    let content = std::fs::read_to_string(schema_path)
        .map_err(|e| miette::miette!("Failed to read schema {}: {}", schema_path.display(), e))?;
    let root: Value = serde_json::from_str(&content)
        .map_err(|e| miette::miette!("Failed to parse schema {}: {}", schema_path.display(), e))?;

    let definitions = root
        .get("definitions")
        .and_then(|v| v.as_object())
        .ok_or_else(|| miette::miette!("Blockception schema is missing definitions"))?;

    let component_props = definitions
        .get("F")
        .and_then(|v| v.get("properties"))
        .and_then(|v| v.as_object())
        .ok_or_else(|| miette::miette!("Blockception schema is missing component properties"))?;

    let mut schemas = HashMap::new();

    for (component_id, component_ref) in component_props {
        if !component_id.starts_with("minecraft:") {
            continue;
        }

        if let Some(schema) = build_component_schema(component_id, component_ref, definitions) {
            schemas.insert(schema.name.clone(), schema);
        }
    }

    Ok(schemas)
}

fn build_component_schema(
    component_id: &str,
    component_ref: &Value,
    definitions: &Map<String, Value>,
) -> Option<ComponentSchema> {
    let resolved = resolve_schema(component_ref, definitions);
    let name = normalize_component_name(component_id);
    let rust_name = sanitize_rust_ident(&name.to_pascal_case(), true);
    if rust_name.is_empty() {
        return None;
    }

    let description = resolved
        .get("description")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| {
            resolved
                .get("title")
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned)
        });

    let Some((properties, required)) = flatten_object_schema(resolved, definitions) else {
        return None;
    };

    if properties.is_empty() {
        return Some(ComponentSchema {
            name,
            rust_name,
            description,
            fields: Vec::new(),
            is_marker: true,
        });
    }

    let mut entries: Vec<_> = properties.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut fields = Vec::with_capacity(entries.len());
    for (field_name, field_schema) in entries {
        let rust_field_name = sanitize_rust_ident(&field_name.to_snake_case(), false);
        if rust_field_name.is_empty() {
            return None;
        }

        let Some(mut field_type) = derive_field_type(&field_schema, definitions) else {
            return None;
        };

        let is_primary = field_name == "value";
        if !required.contains(&field_name) && !is_primary {
            field_type = FieldType::Option(Box::new(field_type));
        }

        fields.push(FieldSchema {
            name: field_name,
            rust_name: rust_field_name,
            field_type,
            is_primary,
        });
    }

    Some(ComponentSchema {
        name,
        rust_name,
        description,
        fields,
        is_marker: false,
    })
}

fn flatten_object_schema(
    schema: &Value,
    definitions: &Map<String, Value>,
) -> Option<(HashMap<String, Value>, HashSet<String>)> {
    let schema = resolve_schema(schema, definitions);
    let is_object_like = schema
        .get("type")
        .and_then(|v| v.as_str())
        .is_some_and(|ty| ty == "object")
        || schema.get("properties").is_some()
        || schema.get("allOf").is_some();

    if !is_object_like {
        return None;
    }

    let mut properties = HashMap::new();
    let mut required = HashSet::new();

    if let Some(all_of) = schema.get("allOf").and_then(|v| v.as_array()) {
        for part in all_of {
            if let Some((part_props, part_required)) = flatten_object_schema(part, definitions) {
                properties.extend(part_props);
                required.extend(part_required);
            }
        }
    }

    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
        for (key, value) in props {
            properties.insert(key.clone(), value.clone());
        }
    }

    if let Some(reqs) = schema.get("required").and_then(|v| v.as_array()) {
        for req in reqs {
            if let Some(name) = req.as_str() {
                required.insert(name.to_string());
            }
        }
    }

    Some((properties, required))
}

fn derive_field_type(schema: &Value, definitions: &Map<String, Value>) -> Option<FieldType> {
    let schema = resolve_schema(schema, definitions);

    if let Some(enum_values) = schema.get("enum").and_then(|v| v.as_array()) {
        if !enum_values.is_empty() {
            return Some(FieldType::String);
        }
    }

    if let Some(variants) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(|v| v.as_array())
    {
        return derive_union_field_type(variants, definitions);
    }

    derive_field_type_non_union(schema)
}

fn derive_union_field_type(
    variants: &[Value],
    definitions: &Map<String, Value>,
) -> Option<FieldType> {
    let mut saw_null = false;
    let mut derived = Vec::new();

    for variant in variants {
        let resolved = resolve_schema(variant, definitions);
        if is_null_schema(resolved) {
            saw_null = true;
            continue;
        }

        let field_type = if let Some(nested) = resolved
            .get("oneOf")
            .or_else(|| resolved.get("anyOf"))
            .and_then(|v| v.as_array())
        {
            derive_union_field_type(nested, definitions)?
        } else {
            derive_field_type_non_union(resolved)?
        };

        derived.push(field_type);
    }

    let merged = merge_field_types(derived)?;
    if saw_null {
        Some(FieldType::Option(Box::new(merged)))
    } else {
        Some(merged)
    }
}

fn derive_field_type_non_union(schema: &Value) -> Option<FieldType> {
    if let Some(type_name) = schema.get("type").and_then(|v| v.as_str()) {
        return match type_name {
            "number" => Some(FieldType::Float),
            "integer" => Some(FieldType::Integer),
            "boolean" => Some(FieldType::Bool),
            "string" => Some(FieldType::String),
            _ => None,
        };
    }

    if let Some(type_names) = schema.get("type").and_then(|v| v.as_array()) {
        let mut saw_null = false;
        let mut derived = Vec::new();
        for type_name in type_names.iter().filter_map(|v| v.as_str()) {
            match type_name {
                "null" => saw_null = true,
                "number" => derived.push(FieldType::Float),
                "integer" => derived.push(FieldType::Integer),
                "boolean" => derived.push(FieldType::Bool),
                "string" => derived.push(FieldType::String),
                _ => return None,
            }
        }
        let merged = merge_field_types(derived)?;
        if saw_null {
            return Some(FieldType::Option(Box::new(merged)));
        }
        return Some(merged);
    }

    schema.get("const").and_then(const_field_type)
}

fn merge_field_types(types: Vec<FieldType>) -> Option<FieldType> {
    let mut saw_float = false;
    let mut saw_integer = false;
    let mut saw_bool = false;
    let mut saw_string = false;
    let mut saw_option = false;

    for field_type in types {
        match field_type {
            FieldType::Float => saw_float = true,
            FieldType::Integer => saw_integer = true,
            FieldType::Bool => saw_bool = true,
            FieldType::String => saw_string = true,
            FieldType::Option(inner) => {
                saw_option = true;
                match *inner {
                    FieldType::Float => saw_float = true,
                    FieldType::Integer => saw_integer = true,
                    FieldType::Bool => saw_bool = true,
                    FieldType::String => saw_string = true,
                    FieldType::Option(_) => return None,
                }
            }
        }
    }

    let primitive_count = [saw_float, saw_integer, saw_bool, saw_string]
        .into_iter()
        .filter(|v| *v)
        .count();
    if primitive_count == 0 {
        return None;
    }

    let base = if saw_string {
        FieldType::String
    } else if saw_float && saw_integer {
        FieldType::Float
    } else if saw_float {
        FieldType::Float
    } else if saw_integer {
        FieldType::Integer
    } else if saw_bool {
        FieldType::Bool
    } else {
        return None;
    };

    if saw_option {
        Some(FieldType::Option(Box::new(base)))
    } else {
        Some(base)
    }
}

fn const_field_type(value: &Value) -> Option<FieldType> {
    match value {
        Value::Number(n) if n.is_i64() || n.is_u64() => Some(FieldType::Integer),
        Value::Number(_) => Some(FieldType::Float),
        Value::Bool(_) => Some(FieldType::Bool),
        Value::String(_) => Some(FieldType::String),
        _ => None,
    }
}

fn is_null_schema(schema: &Value) -> bool {
    schema
        .get("type")
        .and_then(|v| v.as_str())
        .is_some_and(|ty| ty == "null")
        || schema.get("const").is_some_and(Value::is_null)
}

fn resolve_schema<'a>(schema: &'a Value, definitions: &'a Map<String, Value>) -> &'a Value {
    let mut current = schema;
    while let Some(reference) = current.get("$ref").and_then(|v| v.as_str()) {
        let Some(name) = reference.strip_prefix("#/definitions/") else {
            break;
        };
        let Some(resolved) = definitions.get(name) else {
            break;
        };
        current = resolved;
    }
    current
}

fn normalize_component_name(component_id: &str) -> String {
    component_id
        .strip_prefix("minecraft:")
        .unwrap_or(component_id)
        .to_string()
}

fn simple_json_to_string(value: &Value) -> Option<String> {
    match value {
        Value::Null | Value::Array(_) | Value::Object(_) => None,
        Value::Bool(v) => Some(v.to_string()),
        Value::Number(v) => Some(v.to_string()),
        Value::String(v) => Some(v.clone()),
    }
}

fn kdl_value_to_string(value: &KdlValue) -> String {
    match value {
        KdlValue::String(s) => s.clone(),
        KdlValue::Integer(i) => i.to_string(),
        KdlValue::Float(f) => f.to_string(),
        KdlValue::Bool(b) => b.to_string(),
        KdlValue::Null => "null".to_string(),
    }
}

fn sanitize_rust_ident(name: &str, pascal: bool) -> String {
    let mut candidate = name.replace('.', "_").replace('-', "_");
    if !pascal {
        candidate = candidate.to_snake_case();
    }

    if candidate.is_empty() {
        return candidate;
    }

    if is_rust_keyword(&candidate) {
        candidate.push('_');
    }

    candidate
}

fn is_rust_keyword(value: &str) -> bool {
    matches!(
        value,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
    )
}
