//! Component schema definitions and defaults loaded from vendored upstream metadata.
//!
//! Type definitions come from the Blockception Bedrock entity JSON Schema.
//! Default values come from Mojang creator-tools entity forms, with local KDL
//! overrides layered on top for project-specific corrections.

use heck::{ToPascalCase, ToSnakeCase};
use kdl::{KdlDocument, KdlValue};
use serde_json::{Map, Number, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimitiveType {
    Float,
    Integer,
    Bool,
    String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SchemaType {
    Primitive(PrimitiveType),
    Option(Box<SchemaType>),
    Vec(Box<SchemaType>),
    Map(Box<SchemaType>),
    Object(ObjectSchema),
    RangeOrVal(Box<SchemaType>),
    MolangOr(Box<SchemaType>),
    BoolOrString,
    DynamicValue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldSchema {
    pub name: String,
    pub rust_name: String,
    pub description: Option<String>,
    pub field_type: SchemaType,
    pub is_primary: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ObjectSchema {
    pub fields: Vec<FieldSchema>,
    pub additional: Option<Box<SchemaType>>,
}

#[derive(Debug, Clone)]
pub struct ComponentSchema {
    pub name: String,
    pub rust_name: String,
    pub description: Option<String>,
    pub root: ObjectSchema,
    pub is_marker: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ComponentDefaults {
    pub values: HashMap<String, HashMap<String, Value>>,
    pub primary_values: HashMap<String, Value>,
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
                            .insert(comp_name.clone(), kdl_value_to_json(entry.value()));
                    } else if let Some(prop_name) = entry.name() {
                        field_defaults.insert(
                            prop_name.value().to_string(),
                            kdl_value_to_json(entry.value()),
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
                                .insert(prop_name.to_string(), kdl_value_to_json(entry.value()));
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

    pub fn get_field_default(&self, component: &str, field: &str) -> Option<&Value> {
        let fields = self.values.get(component)?;
        if let Some(value) = fields.get(field) {
            return Some(value);
        }

        let wanted = normalize_lookup_key(field);
        fields
            .iter()
            .find(|(name, _)| normalize_lookup_key(name) == wanted)
            .map(|(_, value)| value)
    }

    pub fn get_primary_default(&self, component: &str) -> Option<&Value> {
        self.primary_values.get(component)
    }

    fn merge_field_default(&mut self, component: &str, field: &str, value: Value) {
        self.values
            .entry(component.to_string())
            .or_default()
            .entry(field.to_string())
            .or_insert(value);
    }

    fn merge_primary_default(&mut self, component: &str, value: Value) {
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

            defaults.merge_field_default(&component_name, field_name, default_value.clone());
            if field_name == "value" {
                defaults.merge_primary_default(&component_name, default_value.clone());
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

        let schema = build_component_schema(component_id, component_ref, definitions);
        schemas.insert(schema.name.clone(), schema);
    }

    Ok(schemas)
}

fn build_component_schema(
    component_id: &str,
    component_ref: &Value,
    definitions: &Map<String, Value>,
) -> ComponentSchema {
    let resolved = resolve_schema(component_ref, definitions);
    let name = normalize_component_name(component_id);
    let rust_name = sanitize_rust_ident(&name.to_pascal_case(), true);

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

    let mut ref_stack = Vec::new();
    let derived = derive_schema_type(component_ref, definitions, &mut ref_stack);
    let root = match derived {
        SchemaType::Object(object) => object,
        other => ObjectSchema {
            fields: vec![FieldSchema {
                name: "value".to_string(),
                rust_name: "value".to_string(),
                description: None,
                field_type: other,
                is_primary: true,
            }],
            additional: None,
        },
    };

    let is_marker = root.fields.is_empty() && root.additional.is_none();

    ComponentSchema {
        name,
        rust_name,
        description,
        root,
        is_marker,
    }
}

fn derive_schema_type(
    schema: &Value,
    definitions: &Map<String, Value>,
    ref_stack: &mut Vec<String>,
) -> SchemaType {
    if let Some(reference) = schema.get("$ref").and_then(|v| v.as_str())
        && let Some(name) = reference.strip_prefix("#/definitions/")
    {
        if ref_stack.iter().any(|existing| existing == name) {
            return SchemaType::DynamicValue;
        }

        let Some(resolved) = definitions.get(name) else {
            return SchemaType::DynamicValue;
        };

        ref_stack.push(name.to_string());
        let derived = derive_schema_type(resolved, definitions, ref_stack);
        ref_stack.pop();
        return derived;
    }

    if let Some(union) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(|v| v.as_array())
    {
        return derive_union_type(union, definitions, ref_stack);
    }

    if let Some(enum_values) = schema.get("enum").and_then(|v| v.as_array())
        && !enum_values.is_empty()
    {
        return SchemaType::Primitive(PrimitiveType::String);
    }

    if is_object_like(schema) {
        return SchemaType::Object(derive_object_schema(schema, definitions, ref_stack));
    }

    if let Some(type_name) = schema.get("type").and_then(|v| v.as_str()) {
        return match type_name {
            "number" => SchemaType::Primitive(PrimitiveType::Float),
            "integer" => SchemaType::Primitive(PrimitiveType::Integer),
            "boolean" => SchemaType::Primitive(PrimitiveType::Bool),
            "string" => SchemaType::Primitive(PrimitiveType::String),
            "array" => {
                let item_type = derive_array_item_type(schema, definitions, ref_stack);
                SchemaType::Vec(Box::new(item_type))
            }
            "object" => SchemaType::Object(derive_object_schema(schema, definitions, ref_stack)),
            _ => SchemaType::DynamicValue,
        };
    }

    if let Some(type_names) = schema.get("type").and_then(|v| v.as_array()) {
        let mut variants = Vec::new();
        let mut saw_null = false;
        for type_name in type_names.iter().filter_map(|v| v.as_str()) {
            match type_name {
                "null" => saw_null = true,
                "number" => variants.push(SchemaType::Primitive(PrimitiveType::Float)),
                "integer" => variants.push(SchemaType::Primitive(PrimitiveType::Integer)),
                "boolean" => variants.push(SchemaType::Primitive(PrimitiveType::Bool)),
                "string" => variants.push(SchemaType::Primitive(PrimitiveType::String)),
                "array" => variants.push(SchemaType::Vec(Box::new(SchemaType::DynamicValue))),
                "object" => variants.push(SchemaType::Object(ObjectSchema::default())),
                _ => variants.push(SchemaType::DynamicValue),
            }
        }

        let merged = merge_scalar_like_types(variants).unwrap_or(SchemaType::DynamicValue);
        return if saw_null {
            SchemaType::Option(Box::new(merged))
        } else {
            merged
        };
    }

    if let Some(const_value) = schema.get("const") {
        return match const_value {
            Value::Number(n) if n.is_i64() || n.is_u64() => {
                SchemaType::Primitive(PrimitiveType::Integer)
            }
            Value::Number(_) => SchemaType::Primitive(PrimitiveType::Float),
            Value::Bool(_) => SchemaType::Primitive(PrimitiveType::Bool),
            Value::String(_) => SchemaType::Primitive(PrimitiveType::String),
            _ => SchemaType::DynamicValue,
        };
    }

    if let Some(additional) = schema.get("additionalProperties") {
        return SchemaType::Map(Box::new(derive_additional_type(
            additional,
            definitions,
            ref_stack,
        )));
    }

    SchemaType::DynamicValue
}

fn derive_union_type(
    variants: &[Value],
    definitions: &Map<String, Value>,
    ref_stack: &mut Vec<String>,
) -> SchemaType {
    let mut non_null = Vec::new();
    let mut saw_null = false;

    for variant in variants {
        if is_null_schema(variant) {
            saw_null = true;
        } else {
            non_null.push(variant);
        }
    }

    if non_null.is_empty() {
        return SchemaType::Option(Box::new(SchemaType::DynamicValue));
    }

    let mut derived = if let Some(primitive) = detect_range_or_val(non_null.as_slice()) {
        SchemaType::RangeOrVal(Box::new(SchemaType::Primitive(primitive)))
    } else if let Some(one_or_many) =
        detect_one_or_many(non_null.as_slice(), definitions, ref_stack)
    {
        SchemaType::Vec(Box::new(one_or_many))
    } else {
        let resolved: Vec<SchemaType> = non_null
            .iter()
            .map(|variant| derive_schema_type(variant, definitions, ref_stack))
            .collect();

        if let Some(merged) = merge_object_variants(&resolved) {
            SchemaType::Object(merged)
        } else if let Some(merged) = merge_scalar_like_types(resolved.clone()) {
            merged
        } else {
            SchemaType::DynamicValue
        }
    };

    if saw_null {
        derived = SchemaType::Option(Box::new(derived));
    }

    derived
}

fn derive_object_schema(
    schema: &Value,
    definitions: &Map<String, Value>,
    ref_stack: &mut Vec<String>,
) -> ObjectSchema {
    let resolved = resolve_schema(schema, definitions);
    let mut properties = HashMap::new();
    let mut required = HashSet::new();
    let mut additional = None;

    collect_object_parts(
        resolved,
        definitions,
        ref_stack,
        &mut properties,
        &mut required,
        &mut additional,
    );

    let mut entries: Vec<_> = properties.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut fields = Vec::with_capacity(entries.len());
    for (field_name, field_schema) in entries {
        let rust_field_name = sanitize_rust_ident(&field_name.to_snake_case(), false);
        if rust_field_name.is_empty() {
            continue;
        }

        let description = field_schema
            .get("description")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned)
            .or_else(|| {
                field_schema
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned)
            });

        let is_primary = field_name == "value";
        let mut field_type = derive_schema_type(&field_schema, definitions, ref_stack);
        if !required.contains(&field_name) && !is_primary {
            field_type = wrap_optional(field_type);
        }

        fields.push(FieldSchema {
            name: field_name,
            rust_name: rust_field_name,
            description,
            field_type,
            is_primary,
        });
    }

    ObjectSchema { fields, additional }
}

fn collect_object_parts(
    schema: &Value,
    definitions: &Map<String, Value>,
    ref_stack: &mut Vec<String>,
    properties: &mut HashMap<String, Value>,
    required: &mut HashSet<String>,
    additional: &mut Option<Box<SchemaType>>,
) {
    let schema = resolve_schema(schema, definitions);

    if let Some(all_of) = schema.get("allOf").and_then(|v| v.as_array()) {
        for part in all_of {
            if let Some(then_schema) = part.get("then") {
                collect_object_parts(
                    then_schema,
                    definitions,
                    ref_stack,
                    properties,
                    required,
                    additional,
                );
            }
            collect_object_parts(
                part,
                definitions,
                ref_stack,
                properties,
                required,
                additional,
            );
        }
    }

    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
        for (key, value) in props {
            properties
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }

    if let Some(reqs) = schema.get("required").and_then(|v| v.as_array()) {
        for req in reqs {
            if let Some(name) = req.as_str() {
                required.insert(name.to_string());
            }
        }
    }

    if additional.is_none()
        && let Some(extra_schema) = schema.get("additionalProperties")
        && !matches!(extra_schema, Value::Bool(false))
    {
        *additional = Some(Box::new(derive_additional_type(
            extra_schema,
            definitions,
            ref_stack,
        )));
    }
}

fn derive_additional_type(
    additional: &Value,
    definitions: &Map<String, Value>,
    ref_stack: &mut Vec<String>,
) -> SchemaType {
    match additional {
        Value::Bool(false) => SchemaType::DynamicValue,
        Value::Bool(true) => SchemaType::DynamicValue,
        Value::Object(obj) => {
            if obj.values().all(Value::is_object)
                && !obj.contains_key("type")
                && !obj.contains_key("oneOf")
            {
                SchemaType::DynamicValue
            } else {
                derive_schema_type(additional, definitions, ref_stack)
            }
        }
        _ => SchemaType::DynamicValue,
    }
}

fn derive_array_item_type(
    schema: &Value,
    definitions: &Map<String, Value>,
    ref_stack: &mut Vec<String>,
) -> SchemaType {
    if let Some(items) = schema.get("items") {
        if let Some(arr) = items.as_array() {
            let variants: Vec<SchemaType> = arr
                .iter()
                .map(|item| derive_schema_type(item, definitions, ref_stack))
                .collect();
            return merge_scalar_like_types(variants).unwrap_or(SchemaType::DynamicValue);
        }

        return derive_schema_type(items, definitions, ref_stack);
    }

    SchemaType::DynamicValue
}

fn detect_one_or_many(
    variants: &[&Value],
    definitions: &Map<String, Value>,
    ref_stack: &mut Vec<String>,
) -> Option<SchemaType> {
    if variants.len() != 2 {
        return None;
    }

    let first = resolve_schema(variants[0], definitions);
    let second = resolve_schema(variants[1], definitions);

    let (array_variant, single_variant) =
        if first.get("type").and_then(|v| v.as_str()) == Some("array") {
            (first, second)
        } else if second.get("type").and_then(|v| v.as_str()) == Some("array") {
            (second, first)
        } else {
            return None;
        };

    let array_item = derive_array_item_type(array_variant, definitions, ref_stack);
    let single_item = derive_schema_type(single_variant, definitions, ref_stack);
    if array_item == single_item {
        Some(array_item)
    } else {
        None
    }
}

fn detect_range_or_val(variants: &[&Value]) -> Option<PrimitiveType> {
    if variants.len() < 2 {
        return None;
    }

    let mut primitive = None;
    let mut saw_scalar = false;
    let mut saw_range = false;

    for variant in variants {
        let current = if is_number_schema(variant) {
            saw_scalar = true;
            if variant.get("type").and_then(|v| v.as_str()) == Some("integer") {
                PrimitiveType::Integer
            } else {
                PrimitiveType::Float
            }
        } else if is_number_range_array_schema(variant) || is_number_range_object_schema(variant) {
            saw_range = true;
            PrimitiveType::Float
        } else if is_integer_range_object_schema(variant) {
            saw_range = true;
            PrimitiveType::Integer
        } else {
            return None;
        };

        match &primitive {
            Some(existing) if existing != &current => {
                if matches!(existing, PrimitiveType::Integer)
                    && matches!(current, PrimitiveType::Float)
                    || matches!(existing, PrimitiveType::Float)
                        && matches!(current, PrimitiveType::Integer)
                {
                    primitive = Some(PrimitiveType::Float);
                } else {
                    return None;
                }
            }
            None => primitive = Some(current),
            _ => {}
        }
    }

    if saw_scalar && saw_range {
        primitive
    } else {
        None
    }
}

fn merge_object_variants(variants: &[SchemaType]) -> Option<ObjectSchema> {
    if variants.is_empty() {
        return None;
    }

    let mut merged = ObjectSchema::default();
    let mut any_object = false;

    for variant in variants {
        let SchemaType::Object(object) = variant else {
            return None;
        };
        any_object = true;

        for field in &object.fields {
            if merged
                .fields
                .iter()
                .any(|existing| existing.name == field.name)
            {
                continue;
            }

            let mut field = field.clone();
            field.field_type = wrap_optional(field.field_type);
            field.is_primary = field.name == "value";
            merged.fields.push(field);
        }

        if merged.additional.is_none() {
            merged.additional = object.additional.clone();
        }
    }

    if any_object {
        merged.fields.sort_by(|a, b| a.name.cmp(&b.name));
        Some(merged)
    } else {
        None
    }
}

fn merge_scalar_like_types(variants: Vec<SchemaType>) -> Option<SchemaType> {
    let mut saw_float = false;
    let mut saw_integer = false;
    let mut saw_bool = false;
    let mut saw_string = false;
    let mut saw_option = false;

    for variant in variants {
        match variant {
            SchemaType::Primitive(PrimitiveType::Float) => saw_float = true,
            SchemaType::Primitive(PrimitiveType::Integer) => saw_integer = true,
            SchemaType::Primitive(PrimitiveType::Bool) => saw_bool = true,
            SchemaType::Primitive(PrimitiveType::String) => saw_string = true,
            SchemaType::Option(inner) => {
                saw_option = true;
                match *inner {
                    SchemaType::Primitive(PrimitiveType::Float) => saw_float = true,
                    SchemaType::Primitive(PrimitiveType::Integer) => saw_integer = true,
                    SchemaType::Primitive(PrimitiveType::Bool) => saw_bool = true,
                    SchemaType::Primitive(PrimitiveType::String) => saw_string = true,
                    _ => return None,
                }
            }
            SchemaType::BoolOrString => {
                saw_bool = true;
                saw_string = true;
            }
            SchemaType::DynamicValue => return None,
            _ => return None,
        }
    }

    let primitive_count = [saw_float, saw_integer, saw_bool, saw_string]
        .into_iter()
        .filter(|v| *v)
        .count();
    if primitive_count == 0 {
        return None;
    }

    let mut base = if saw_bool && saw_string && !saw_float && !saw_integer {
        SchemaType::BoolOrString
    } else if saw_string && (saw_float || saw_integer) && !saw_bool {
        let inner = if saw_float {
            SchemaType::Primitive(PrimitiveType::Float)
        } else {
            SchemaType::Primitive(PrimitiveType::Integer)
        };
        SchemaType::MolangOr(Box::new(inner))
    } else if saw_string && !saw_float && !saw_integer && !saw_bool {
        SchemaType::Primitive(PrimitiveType::String)
    } else if saw_float {
        SchemaType::Primitive(PrimitiveType::Float)
    } else if saw_integer {
        SchemaType::Primitive(PrimitiveType::Integer)
    } else if saw_bool {
        SchemaType::Primitive(PrimitiveType::Bool)
    } else {
        return None;
    };

    if saw_option {
        base = SchemaType::Option(Box::new(base));
    }

    Some(base)
}

fn wrap_optional(schema_type: SchemaType) -> SchemaType {
    if matches!(schema_type, SchemaType::Option(_)) {
        schema_type
    } else {
        SchemaType::Option(Box::new(schema_type))
    }
}

fn is_object_like(schema: &Value) -> bool {
    schema
        .get("type")
        .and_then(|v| v.as_str())
        .is_some_and(|ty| ty == "object")
        || schema.get("properties").is_some()
        || schema.get("allOf").is_some()
        || schema.get("additionalProperties").is_some()
}

fn is_null_schema(schema: &Value) -> bool {
    schema
        .get("type")
        .and_then(|v| v.as_str())
        .is_some_and(|ty| ty == "null")
        || schema.get("const").is_some_and(Value::is_null)
}

fn is_number_schema(schema: &Value) -> bool {
    matches!(
        schema.get("type").and_then(|v| v.as_str()),
        Some("number" | "integer")
    )
}

fn is_number_range_array_schema(schema: &Value) -> bool {
    let Some(items) = schema.get("items").and_then(|v| v.as_array()) else {
        return false;
    };
    schema.get("type").and_then(|v| v.as_str()) == Some("array")
        && !items.is_empty()
        && items.iter().all(is_number_schema)
}

fn is_number_range_object_schema(schema: &Value) -> bool {
    let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) else {
        return false;
    };
    let keys: HashSet<_> = properties.keys().map(String::as_str).collect();
    (keys.contains("min") && keys.contains("max")
        || keys.contains("range_min") && keys.contains("range_max"))
        && properties.values().all(is_number_schema)
}

fn is_integer_range_object_schema(schema: &Value) -> bool {
    let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) else {
        return false;
    };
    let keys: HashSet<_> = properties.keys().map(String::as_str).collect();
    (keys.contains("min") && keys.contains("max")
        || keys.contains("range_min") && keys.contains("range_max"))
        && properties
            .values()
            .all(|value| value.get("type").and_then(|v| v.as_str()) == Some("integer"))
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

fn normalize_lookup_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn kdl_value_to_json(value: &KdlValue) -> Value {
    match value {
        KdlValue::String(s) => Value::String(s.clone()),
        KdlValue::Integer(i) => Value::Number(Number::from(*i as i64)),
        KdlValue::Float(f) => Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        KdlValue::Bool(b) => Value::Bool(*b),
        KdlValue::Null => Value::Null,
    }
}

fn sanitize_rust_ident(name: &str, pascal: bool) -> String {
    let mut candidate = name.replace(['.', '-'], "_");
    if !pascal {
        candidate = candidate.to_snake_case();
    }

    if candidate.is_empty() {
        return candidate;
    }

    if is_rust_keyword(&candidate) {
        candidate.push('_');
    }

    if candidate.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        candidate.insert(0, '_');
    }

    candidate
}

fn is_rust_keyword(s: &str) -> bool {
    matches!(
        s,
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
    )
}
