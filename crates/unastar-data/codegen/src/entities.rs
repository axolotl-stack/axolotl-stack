//! Entity code generation from KDL.

use crate::component_schemas::{
    ComponentDefaults, ComponentSchema, ObjectSchema, PrimitiveType, SchemaCatalog, SchemaType,
};
use heck::{ToPascalCase, ToSnakeCase};
use kdl::{KdlDocument, KdlNode, KdlValue};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde_json::{Map, Number, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tracing::info;

/// Parsed entity from KDL for code generation
struct ParsedEntity {
    identifier: String,
    name: String, // without minecraft: prefix
    spawn_category: Option<String>,
    is_spawnable: bool,
    is_summonable: bool,
    runtime_id: Option<u32>,
    components: Vec<ParsedComponent>,
    component_groups: Vec<String>,
    events: Vec<String>,
    properties: Vec<ParsedProperty>,
}

/// Parsed component with actual data
struct ParsedComponent {
    name: String,
    data: Option<Value>,
    /// Whether this is a marker component (no data)
    #[allow(dead_code)]
    is_marker: bool,
}

struct ParsedProperty {
    name: String,
    prop_type: String,
    values: Vec<String>, // for enums
    default: String,
    #[allow(dead_code)]
    client_sync: bool,
}

pub fn generate_entities(input_dir: &Path, output_dir: &Path) -> miette::Result<()> {
    let kdl_path = input_dir.join("entities.kdl");

    if !kdl_path.exists() {
        info!("No entities.kdl found at {}, skipping", kdl_path.display());
        return Ok(());
    }

    let data_dir = input_dir.parent().unwrap_or(input_dir).join("data");
    let defaults_path = data_dir.join("overrides/_defaults.kdl");
    let upstream_dir = data_dir.join("upstream");
    let catalog = SchemaCatalog::load(&upstream_dir, &defaults_path)?;

    info!(
        "Loaded {} upstream component schemas",
        catalog.components.len()
    );

    let content = std::fs::read_to_string(&kdl_path)
        .map_err(|e| miette::miette!("Failed to read entities.kdl: {}", e))?;

    let doc: KdlDocument = content
        .parse()
        .map_err(|e| miette::miette!("KDL parse error: {}", e))?;

    // Parse entities
    let mut entities = Vec::new();
    let mut all_components = HashSet::new();
    let mut component_counts = HashMap::new();

    for node in doc.nodes() {
        if node.name().value() == "entity" {
            let entity = parse_entity_node(node)?;
            for comp in &entity.components {
                all_components.insert(comp.name.clone());
                *component_counts.entry(comp.name.clone()).or_insert(0usize) += 1;
            }
            entities.push(entity);
        }
    }

    info!(
        "Parsed {} entities, {} unique components",
        entities.len(),
        all_components.len()
    );

    // Generate code
    let entities_dir = output_dir.join("entities");
    std::fs::create_dir_all(&entities_dir)
        .map_err(|e| miette::miette!("Failed to create entities dir: {}", e))?;
    std::fs::create_dir_all(entities_dir.join("components"))
        .map_err(|e| miette::miette!("Failed to create components dir: {}", e))?;
    std::fs::create_dir_all(entities_dir.join("definitions"))
        .map_err(|e| miette::miette!("Failed to create definitions dir: {}", e))?;

    generate_components_module(
        &all_components,
        &component_counts,
        entities.len(),
        &entities_dir,
        &catalog,
    )?;

    for entity in &entities {
        generate_entity_definition(
            entity,
            &entities_dir,
            &catalog.components,
            &catalog.defaults,
        )?;
    }

    generate_entities_mod(&entities, &entities_dir, &catalog.components)?;

    Ok(())
}

fn parse_entity_node(node: &KdlNode) -> miette::Result<ParsedEntity> {
    let identifier = node
        .entries()
        .first()
        .and_then(|e| e.value().as_string())
        .unwrap_or("unknown")
        .to_string();

    let name = identifier
        .strip_prefix("minecraft:")
        .unwrap_or(&identifier)
        .to_string();

    let mut entity = ParsedEntity {
        identifier,
        name,
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        components: Vec::new(),
        component_groups: Vec::new(),
        events: Vec::new(),
        properties: Vec::new(),
    };

    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "is_spawnable" => {
                    entity.is_spawnable = child
                        .entries()
                        .first()
                        .and_then(|e| e.value().as_bool())
                        .unwrap_or(false);
                }
                "is_summonable" => {
                    entity.is_summonable = child
                        .entries()
                        .first()
                        .and_then(|e| e.value().as_bool())
                        .unwrap_or(false);
                }
                "runtime_id" => {
                    entity.runtime_id = child
                        .entries()
                        .first()
                        .and_then(|e| e.value().as_integer())
                        .map(|value| {
                            u32::try_from(value).map_err(|_| {
                                miette::miette!(
                                    "entity {} runtime_id {} is out of u32 range",
                                    entity.identifier,
                                    value
                                )
                            })
                        })
                        .transpose()?;
                }
                "spawn_category" => {
                    entity.spawn_category = child
                        .entries()
                        .first()
                        .and_then(|e| e.value().as_string())
                        .map(ToString::to_string);
                }
                "components" => {
                    if let Some(comp_children) = child.children() {
                        for comp in comp_children.nodes() {
                            entity.components.push(parse_component_node(comp));
                        }
                    }
                }
                "component_groups" => {
                    if let Some(group_children) = child.children() {
                        for group in group_children.nodes() {
                            if group.name().value() == "group"
                                && let Some(name) =
                                    group.entries().first().and_then(|e| e.value().as_string())
                            {
                                entity.component_groups.push(name.to_string());
                            }
                        }
                    }
                }
                "events" => {
                    if let Some(event_children) = child.children() {
                        for event in event_children.nodes() {
                            if event.name().value() == "event"
                                && let Some(name) =
                                    event.entries().first().and_then(|e| e.value().as_string())
                            {
                                entity.events.push(name.to_string());
                            }
                        }
                    }
                }
                "properties" => {
                    if let Some(prop_children) = child.children() {
                        for prop in prop_children.nodes() {
                            if let Some(parsed) = parse_property_node(prop) {
                                entity.properties.push(parsed);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(entity)
}

fn parse_component_node(node: &KdlNode) -> ParsedComponent {
    let name = node.name().value().to_string();
    let data = kdl_node_payload_to_json(node);
    let is_marker = data.is_none();

    ParsedComponent {
        name,
        data,
        is_marker,
    }
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

fn kdl_node_payload_to_json(node: &KdlNode) -> Option<Value> {
    let positional: Vec<Value> = node
        .entries()
        .iter()
        .filter(|entry| entry.name().is_none())
        .map(|entry| kdl_value_to_json(entry.value()))
        .collect();

    let mut named = Map::new();
    for entry in node.entries().iter().filter(|entry| entry.name().is_some()) {
        if let Some(name) = entry.name() {
            named.insert(name.value().to_string(), kdl_value_to_json(entry.value()));
        }
    }

    let has_children = node
        .children()
        .is_some_and(|children| !children.nodes().is_empty());
    if positional.is_empty() && named.is_empty() && !has_children {
        return None;
    }

    if !has_children && named.is_empty() {
        return match positional.len() {
            0 => None,
            1 => positional.into_iter().next(),
            _ => Some(Value::Array(positional)),
        };
    }

    let mut object = Map::new();
    if !positional.is_empty() {
        let value = if positional.len() == 1 {
            positional[0].clone()
        } else {
            Value::Array(positional)
        };
        object.insert("value".to_string(), value);
    }
    object.extend(named);

    if let Some(children) = node.children() {
        let mut grouped: HashMap<String, Vec<Value>> = HashMap::new();
        for child in children.nodes() {
            let child_name = child.name().value().to_string();
            let child_value = match child_name.as_str() {
                "any_of" | "none_of" => parse_filter_group_node(child),
                _ => kdl_node_payload_to_json(child).unwrap_or_else(|| Value::Object(Map::new())),
            };
            grouped.entry(child_name).or_default().push(child_value);
        }

        let mut child_names: Vec<_> = grouped.keys().cloned().collect();
        child_names.sort();

        for child_name in child_names {
            let values = grouped.remove(&child_name).unwrap_or_default();
            if node.name().value() == "filters" && child_name == "filter" {
                object.insert("all_of".to_string(), Value::Array(values));
                continue;
            }

            let value = if values.len() == 1 {
                values.into_iter().next().unwrap()
            } else {
                Value::Array(values)
            };
            object.insert(child_name, value);
        }
    }

    Some(Value::Object(object))
}

fn parse_filter_group_node(node: &KdlNode) -> Value {
    let mut filters = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if let Some(value) = kdl_node_payload_to_json(child) {
                filters.push(value);
            }
        }
    }
    Value::Array(filters)
}

fn parse_property_node(node: &KdlNode) -> Option<ParsedProperty> {
    let name = node.name().value().to_string();
    let mut prop = ParsedProperty {
        name,
        prop_type: "unknown".to_string(),
        values: Vec::new(),
        default: String::new(),
        client_sync: false,
    };

    for entry in node.entries() {
        if let Some(name) = entry.name() {
            match name.value() {
                "type" => {
                    prop.prop_type = entry.value().as_string().unwrap_or("unknown").to_string();
                }
                "default" => match entry.value() {
                    KdlValue::String(s) => prop.default = s.clone(),
                    KdlValue::Integer(i) => prop.default = i.to_string(),
                    KdlValue::Float(f) => prop.default = f.to_string(),
                    KdlValue::Bool(b) => prop.default = b.to_string(),
                    KdlValue::Null => prop.default = "null".to_string(),
                },
                "client_sync" => {
                    prop.client_sync = entry.value().as_bool().unwrap_or(false);
                }
                _ => {}
            }
        }
    }

    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "value"
                && let Some(entry) = child.entries().first()
                && let Some(s) = entry.value().as_string()
            {
                prop.values.push(s.to_string());
            }
        }
    }

    Some(prop)
}

fn generate_components_module(
    components: &HashSet<String>,
    component_counts: &HashMap<String, usize>,
    entity_count: usize,
    output_dir: &Path,
    catalog: &SchemaCatalog,
) -> miette::Result<()> {
    let components_dir = output_dir.join("components");
    let schemas = &catalog.components;
    let defaults = &catalog.defaults;

    let mut mod_items = Vec::new();

    for name in components {
        let module_name = name.to_snake_case();
        let struct_name = name.to_pascal_case();

        if module_name.is_empty() || struct_name.is_empty() {
            tracing::warn!("Skipping component with invalid name: {:?}", name);
            continue;
        }

        let is_sparse = component_is_sparse(name, component_counts, entity_count);
        let code = if let Some(schema) = schemas.get(name) {
            generate_typed_component(schema, is_sparse, defaults)
        } else {
            generate_generic_component(name, &struct_name, is_sparse)
        };

        let formatted = format_code(code)?;

        std::fs::write(
            components_dir.join(format!("{}.rs", module_name)),
            formatted,
        )
        .map_err(|e| miette::miette!("Failed to write component file: {}", e))?;

        mod_items.push((module_name, struct_name));
    }

    mod_items.sort_by(|a, b| a.0.cmp(&b.0));

    let mods: Vec<TokenStream> = mod_items
        .iter()
        .map(|(m, _)| {
            let mod_ident = format_ident!("{}", m);
            quote! { pub mod #mod_ident; }
        })
        .collect();

    let uses: Vec<TokenStream> = mod_items
        .iter()
        .map(|(m, _)| {
            let mod_ident = format_ident!("{}", m);
            quote! { pub use #mod_ident::*; }
        })
        .collect();

    let code = quote! {
        //! Generated component DTOs from Bedrock entity metadata.
        //!
        //! Components marked with `#[component(storage = "SparseSet")]` are:
        //! - Rare across vanilla entities, or
        //! - Frequently added and removed at runtime.

        #(#mods)*

        #(#uses)*
    };

    std::fs::write(components_dir.join("mod.rs"), format_code(code)?)
        .map_err(|e| miette::miette!("Failed to write components mod.rs: {}", e))?;

    Ok(())
}

fn component_is_sparse(
    component: &str,
    component_counts: &HashMap<String, usize>,
    entity_count: usize,
) -> bool {
    const MANUAL_SPARSE_COMPONENTS: &[&str] = &[
        "is_baby",
        "is_tamed",
        "is_saddled",
        "is_sheared",
        "is_ignited",
        "is_charged",
        "is_stunned",
        "is_illager_captain",
        "can_fly",
        "can_power_jump",
        "fire_immune",
        "burns_in_daylight",
        "floats_in_liquid",
        "flying_speed",
        "glide",
        "boss",
        "angry",
        "on_fire",
        "strength",
    ];

    if MANUAL_SPARSE_COMPONENTS.contains(&component) {
        return true;
    }

    let Some(count) = component_counts.get(component) else {
        return false;
    };

    entity_count > 0 && (*count as f32 / entity_count as f32) < 0.10
}

fn generate_typed_component(
    schema: &ComponentSchema,
    is_sparse: bool,
    defaults: &ComponentDefaults,
) -> TokenStream {
    let struct_ident = format_ident!("{}", schema.rust_name);
    let doc = if let Some(description) = &schema.description {
        format!(
            " Bedrock component `minecraft:{}`. {}",
            schema.name, description
        )
    } else {
        format!(" Bedrock component `minecraft:{}`", schema.name)
    };

    let storage_attr = if is_sparse {
        quote! { #[component(storage = "SparseSet")] }
    } else {
        quote! {}
    };

    if schema.is_marker {
        return quote! {
            use bevy_ecs::prelude::*;

            #[doc = #doc]
            #[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
            #storage_attr
            pub struct #struct_ident;
        };
    }

    let mut nested_defs = Vec::new();
    let mut emitted = HashSet::new();
    for field in &schema.root.fields {
        let mut path = vec![schema.rust_name.clone(), field.name.to_pascal_case()];
        collect_nested_type_defs(&field.field_type, &mut path, &mut emitted, &mut nested_defs);
    }
    if let Some(additional) = &schema.root.additional {
        let mut path = vec![schema.rust_name.clone(), "Additional".to_string()];
        collect_nested_type_defs(additional, &mut path, &mut emitted, &mut nested_defs);
    }

    let fields = object_fields_to_tokens(&schema.root, std::slice::from_ref(&schema.rust_name));
    let default_impl = generate_default_impl(schema, defaults);

    quote! {
        use bevy_ecs::prelude::*;

        #(#nested_defs)*

        #[doc = #doc]
        #[derive(Component, Debug, Clone, PartialEq)]
        #storage_attr
        pub struct #struct_ident {
            #(#fields),*
        }

        #default_impl
    }
}

fn collect_nested_type_defs(
    schema_type: &SchemaType,
    path: &mut Vec<String>,
    emitted: &mut HashSet<String>,
    out: &mut Vec<TokenStream>,
) {
    match schema_type {
        SchemaType::Option(inner)
        | SchemaType::Vec(inner)
        | SchemaType::Map(inner)
        | SchemaType::RangeOrVal(inner)
        | SchemaType::MolangOr(inner) => collect_nested_type_defs(inner, path, emitted, out),
        SchemaType::Object(object) => {
            let type_name = type_name_from_path(path);
            if !emitted.insert(type_name.clone()) {
                return;
            }

            for field in &object.fields {
                path.push(field.name.to_pascal_case());
                collect_nested_type_defs(&field.field_type, path, emitted, out);
                path.pop();
            }
            if let Some(additional) = &object.additional {
                path.push("Additional".to_string());
                collect_nested_type_defs(additional, path, emitted, out);
                path.pop();
            }

            let struct_ident = format_ident!("{}", type_name);
            let fields = object_fields_to_tokens(object, path);
            let defaults = object_default_fields(object, path);
            out.push(quote! {
                #[derive(Debug, Clone, PartialEq)]
                pub struct #struct_ident {
                    #(#fields),*
                }

                impl Default for #struct_ident {
                    fn default() -> Self {
                        Self {
                            #(#defaults),*
                        }
                    }
                }
            });
        }
        _ => {}
    }
}

fn object_fields_to_tokens(object: &ObjectSchema, path: &[String]) -> Vec<TokenStream> {
    let mut fields: Vec<TokenStream> = object
        .fields
        .iter()
        .map(|field| {
            let field_ident = format_ident!("{}", field.rust_name);
            let mut field_path = path.to_vec();
            field_path.push(field.name.to_pascal_case());
            let field_type = schema_type_to_tokens(&field.field_type, &field_path);
            let doc = field
                .description
                .clone()
                .unwrap_or_else(|| field.name.clone());
            quote! {
                #[doc = #doc]
                pub #field_ident: #field_type
            }
        })
        .collect();

    if let Some(additional) = &object.additional {
        let mut field_path = path.to_vec();
        field_path.push("Additional".to_string());
        let inner_tokens = schema_type_to_tokens(additional, &field_path);
        fields.push(quote! {
            /// Additional dynamic entries not captured by the upstream schema.
            pub additional: std::collections::HashMap<String, #inner_tokens>
        });
    }

    fields
}

fn object_default_fields(object: &ObjectSchema, path: &[String]) -> Vec<TokenStream> {
    let mut defaults: Vec<TokenStream> = object
        .fields
        .iter()
        .map(|field| {
            let field_ident = format_ident!("{}", field.rust_name);
            let mut field_path = path.to_vec();
            field_path.push(field.name.to_pascal_case());
            let value = default_value_tokens(&field.field_type, None, &field_path);
            quote! { #field_ident: #value }
        })
        .collect();

    if object.additional.is_some() {
        defaults.push(quote! { additional: std::collections::HashMap::new() });
    }

    defaults
}

fn generate_default_impl(schema: &ComponentSchema, defaults: &ComponentDefaults) -> TokenStream {
    let struct_ident = format_ident!("{}", schema.rust_name);

    let mut field_defaults: Vec<TokenStream> = schema
        .root
        .fields
        .iter()
        .map(|field| {
            let field_ident = format_ident!("{}", field.rust_name);
            let default_value = if field.is_primary {
                defaults.get_primary_default(&schema.name)
            } else {
                defaults.get_field_default(&schema.name, &field.name)
            };
            let path = vec![schema.rust_name.clone(), field.name.to_pascal_case()];
            let value = default_value_tokens(&field.field_type, default_value, &path);
            quote! { #field_ident: #value }
        })
        .collect();

    if schema.root.additional.is_some() {
        field_defaults.push(quote! { additional: std::collections::HashMap::new() });
    }

    quote! {
        impl Default for #struct_ident {
            fn default() -> Self {
                Self {
                    #(#field_defaults),*
                }
            }
        }
    }
}

fn schema_type_to_tokens(schema_type: &SchemaType, path: &[String]) -> TokenStream {
    match schema_type {
        SchemaType::Primitive(PrimitiveType::Float) => quote! { f32 },
        SchemaType::Primitive(PrimitiveType::Integer) => quote! { i32 },
        SchemaType::Primitive(PrimitiveType::Bool) => quote! { bool },
        SchemaType::Primitive(PrimitiveType::String) => quote! { String },
        SchemaType::Option(inner) => {
            let inner_tokens = schema_type_to_tokens(inner, path);
            quote! { Option<#inner_tokens> }
        }
        SchemaType::Vec(inner) => {
            let inner_tokens = schema_type_to_tokens(inner, path);
            quote! { Vec<#inner_tokens> }
        }
        SchemaType::Map(inner) => {
            let inner_tokens = schema_type_to_tokens(inner, path);
            quote! { std::collections::HashMap<String, #inner_tokens> }
        }
        SchemaType::Object(_) => {
            let ident = format_ident!("{}", type_name_from_path(path));
            quote! { #ident }
        }
        SchemaType::RangeOrVal(inner) => {
            let inner_tokens = schema_type_to_tokens(inner, path);
            quote! { crate::types::RangeOrVal<#inner_tokens> }
        }
        SchemaType::MolangOr(inner) => {
            let inner_tokens = schema_type_to_tokens(inner, path);
            quote! { crate::types::MolangOr<#inner_tokens> }
        }
        SchemaType::BoolOrString => quote! { crate::types::BoolOrString },
        SchemaType::DynamicValue => quote! { crate::types::BedrockValue },
    }
}

fn default_value_tokens(
    schema_type: &SchemaType,
    value: Option<&Value>,
    path: &[String],
) -> TokenStream {
    match schema_type {
        SchemaType::Primitive(PrimitiveType::Float) => {
            let parsed = value.and_then(value_as_f32).unwrap_or(0.0f32);
            quote! { #parsed }
        }
        SchemaType::Primitive(PrimitiveType::Integer) => {
            let parsed = value.and_then(value_as_i32).unwrap_or(0i32);
            quote! { #parsed }
        }
        SchemaType::Primitive(PrimitiveType::Bool) => {
            let parsed = value.and_then(Value::as_bool).unwrap_or(false);
            quote! { #parsed }
        }
        SchemaType::Primitive(PrimitiveType::String) => {
            let parsed = value.and_then(value_as_string).unwrap_or_default();
            quote! { #parsed.to_string() }
        }
        SchemaType::Option(inner) => {
            if let Some(value) = value {
                if value.is_null() {
                    quote! { None }
                } else {
                    let inner_tokens = default_value_tokens(inner, Some(value), path);
                    quote! { Some(#inner_tokens) }
                }
            } else {
                quote! { None }
            }
        }
        SchemaType::Vec(inner) => {
            if let Some(Value::Array(values)) = value {
                let items: Vec<_> = values
                    .iter()
                    .map(|item| default_value_tokens(inner, Some(item), path))
                    .collect();
                quote! { vec![#(#items),*] }
            } else if let Some(value) = value {
                let single = default_value_tokens(inner, Some(value), path);
                quote! { vec![#single] }
            } else {
                quote! { Vec::new() }
            }
        }
        SchemaType::Map(inner) => {
            if let Some(Value::Object(values)) = value {
                let entries: Vec<_> = values
                    .iter()
                    .map(|(key, value)| {
                        let value_tokens = default_value_tokens(inner, Some(value), path);
                        quote! { (#key.to_string(), #value_tokens) }
                    })
                    .collect();
                quote! { std::collections::HashMap::from([#(#entries),*]) }
            } else {
                quote! { std::collections::HashMap::new() }
            }
        }
        SchemaType::Object(object) => object_value_tokens(object, value, path),
        SchemaType::RangeOrVal(inner) => range_value_tokens(inner, value, path),
        SchemaType::MolangOr(inner) => {
            if let Some(Value::String(expr)) = value {
                quote! { crate::types::MolangOr::Expr(#expr.to_string()) }
            } else {
                let inner_tokens = default_value_tokens(inner, value, path);
                quote! { crate::types::MolangOr::Value(#inner_tokens) }
            }
        }
        SchemaType::BoolOrString => match value {
            Some(Value::Bool(value)) => quote! { crate::types::BoolOrString::Bool(#value) },
            Some(Value::String(value)) => {
                quote! { crate::types::BoolOrString::String(#value.to_string()) }
            }
            _ => quote! { crate::types::BoolOrString::Bool(false) },
        },
        SchemaType::DynamicValue => dynamic_value_tokens(value.unwrap_or(&Value::Null)),
    }
}

fn object_value_tokens(
    object: &ObjectSchema,
    value: Option<&Value>,
    path: &[String],
) -> TokenStream {
    let ident = format_ident!("{}", type_name_from_path(path));
    let object_value = value.and_then(Value::as_object);

    let mut fields: Vec<TokenStream> = object
        .fields
        .iter()
        .map(|field| {
            let field_ident = format_ident!("{}", field.rust_name);
            let field_value = object_value.and_then(|map| lookup_object_value(map, &field.name));
            let mut field_path = path.to_vec();
            field_path.push(field.name.to_pascal_case());
            let value_tokens = default_value_tokens(&field.field_type, field_value, &field_path);
            quote! { #field_ident: #value_tokens }
        })
        .collect();

    if let Some(additional_schema) = &object.additional {
        let mut additional_entries = Vec::new();
        if let Some(map) = object_value {
            for (key, entry) in map {
                if object
                    .fields
                    .iter()
                    .any(|field| field_matches_lookup(key, &field.name))
                {
                    continue;
                }
                let value_tokens = default_value_tokens(additional_schema, Some(entry), path);
                additional_entries.push(quote! { (#key.to_string(), #value_tokens) });
            }
        }

        if additional_entries.is_empty() {
            fields.push(quote! { additional: std::collections::HashMap::new() });
        } else {
            fields.push(quote! {
                additional: std::collections::HashMap::from([#(#additional_entries),*])
            });
        }
    }

    quote! {
        #ident {
            #(#fields),*
        }
    }
}

fn range_value_tokens(inner: &SchemaType, value: Option<&Value>, path: &[String]) -> TokenStream {
    if let Some(value) = value {
        match value {
            Value::Array(values) if values.len() >= 2 => {
                let min = default_value_tokens(inner, Some(&values[0]), path);
                let max = default_value_tokens(inner, Some(&values[1]), path);
                return quote! { crate::types::RangeOrVal::Range { min: #min, max: #max } };
            }
            Value::Object(map) => {
                let min = map.get("min").or_else(|| map.get("range_min"));
                let max = map.get("max").or_else(|| map.get("range_max"));
                if let (Some(min), Some(max)) = (min, max) {
                    let min_tokens = default_value_tokens(inner, Some(min), path);
                    let max_tokens = default_value_tokens(inner, Some(max), path);
                    return quote! { crate::types::RangeOrVal::Range { min: #min_tokens, max: #max_tokens } };
                }
            }
            _ => {
                let fixed = default_value_tokens(inner, Some(value), path);
                return quote! { crate::types::RangeOrVal::Fixed(#fixed) };
            }
        }
    }

    let fixed = default_value_tokens(inner, None, path);
    quote! { crate::types::RangeOrVal::Fixed(#fixed) }
}

fn dynamic_value_tokens(value: &Value) -> TokenStream {
    match value {
        Value::Null => quote! { crate::types::BedrockValue::Null },
        Value::Bool(value) => quote! { crate::types::BedrockValue::Bool(#value) },
        Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                quote! { crate::types::BedrockValue::Integer(#integer) }
            } else if let Some(float) = value.as_f64() {
                quote! { crate::types::BedrockValue::Float(#float) }
            } else {
                quote! { crate::types::BedrockValue::Null }
            }
        }
        Value::String(value) => quote! { crate::types::BedrockValue::String(#value.to_string()) },
        Value::Array(values) => {
            let entries: Vec<_> = values.iter().map(dynamic_value_tokens).collect();
            quote! { crate::types::BedrockValue::Array(vec![#(#entries),*]) }
        }
        Value::Object(values) => {
            let entries: Vec<_> = values
                .iter()
                .map(|(key, value)| {
                    let value_tokens = dynamic_value_tokens(value);
                    quote! { (#key.to_string(), #value_tokens) }
                })
                .collect();
            quote! { crate::types::BedrockValue::Object(std::collections::HashMap::from([#(#entries),*])) }
        }
    }
}

fn value_as_f32(value: &Value) -> Option<f32> {
    match value {
        Value::Number(number) => number.as_f64().map(|value| value as f32),
        Value::String(text) => text.parse::<f32>().ok(),
        _ => None,
    }
}

fn value_as_i32(value: &Value) -> Option<i32> {
    match value {
        Value::Number(number) => number.as_i64().map(|value| value as i32),
        Value::String(text) => text.parse::<i32>().ok(),
        _ => None,
    }
}

fn value_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Null => Some("null".to_string()),
        _ => None,
    }
}

fn type_name_from_path(path: &[String]) -> String {
    path.iter()
        .map(|segment| segment.to_pascal_case())
        .collect::<String>()
}

fn normalize_lookup_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn candidate_lookup_keys(field_name: &str) -> Vec<String> {
    let mut keys = vec![field_name.to_string()];
    match field_name {
        "triggers" => keys.push("trigger".to_string()),
        "interactions" => keys.push("interaction".to_string()),
        "entries" => keys.push("entry".to_string()),
        "conditions" => keys.push("condition".to_string()),
        "event_names" => keys.push("event".to_string()),
        "items" => keys.push("item".to_string()),
        "functions" => keys.push("function".to_string()),
        _ => {
            if field_name.ends_with('s') && field_name.len() > 1 {
                keys.push(field_name[..field_name.len() - 1].to_string());
            }
        }
    }
    keys
}

fn field_matches_lookup(actual: &str, field_name: &str) -> bool {
    let normalized_actual = normalize_lookup_key(actual);
    candidate_lookup_keys(field_name)
        .into_iter()
        .any(|candidate| normalize_lookup_key(&candidate) == normalized_actual)
}

fn lookup_object_value<'a>(map: &'a Map<String, Value>, field_name: &str) -> Option<&'a Value> {
    if let Some(value) = map.get(field_name) {
        return Some(value);
    }

    map.iter()
        .find(|(name, _)| field_matches_lookup(name, field_name))
        .map(|(_, value)| value)
}

fn generate_generic_component(name: &str, struct_name: &str, is_sparse: bool) -> TokenStream {
    let struct_ident = format_ident!("{}", struct_name);
    let doc = format!(" Component DTO for `minecraft:{}`", name);

    let storage_attr = if is_sparse {
        quote! { #[component(storage = "SparseSet")] }
    } else {
        quote! {}
    };

    quote! {
        use bevy_ecs::prelude::*;

        #[doc = #doc]
        #[derive(Component, Debug, Clone, Default, PartialEq)]
        #storage_attr
        pub struct #struct_ident {
            /// Dynamic Bedrock payload for components that are not yet covered by the vendored schema.
            pub data: Option<crate::types::BedrockValue>,
        }
    }
}

fn generate_entity_definition(
    entity: &ParsedEntity,
    output_dir: &Path,
    schemas: &HashMap<String, ComponentSchema>,
    defaults: &ComponentDefaults,
) -> miette::Result<()> {
    let definitions_dir = output_dir.join("definitions");
    let module_name = entity.name.to_snake_case();
    let struct_name = entity.name.to_pascal_case();

    let struct_ident = format_ident!("{}", struct_name);
    let identifier = &entity.identifier;
    let is_spawnable = entity.is_spawnable;
    let is_summonable = entity.is_summonable;

    // Component group enum
    let group_enum = generate_component_group_enum(entity, &struct_name);

    // Event enum
    let event_enum = generate_event_enum(entity, &struct_name);

    // Property enums
    let property_enums = generate_property_enums(entity, &struct_name);

    let spawn_fn = generate_spawn_function(entity, &struct_name, schemas, defaults);
    let bundle_struct = generate_bundle_struct(entity, &struct_name, schemas);
    let components_import = if entity.components.is_empty() {
        quote! {}
    } else {
        quote! { #[allow(unused_imports)] use super::super::components::*; }
    };

    let doc = format!(" Entity definition for `{}`", identifier);

    let runtime_id_const = if let Some(id) = entity.runtime_id {
        quote! {
            /// The runtime numeric ID for this entity
            pub const RUNTIME_ID: u32 = #id;
        }
    } else {
        quote! {}
    };

    let code = quote! {
        //! Generated definition for entity.

        use bevy_ecs::prelude::{Bundle, Commands, Entity};
        #components_import

        #[doc = #doc]
        pub struct #struct_ident;

        impl #struct_ident {
            /// The entity identifier
            pub const IDENTIFIER: &'static str = #identifier;

            /// Whether this entity can spawn naturally
            pub const IS_SPAWNABLE: bool = #is_spawnable;

            /// Whether this entity can be summoned via commands
            pub const IS_SUMMONABLE: bool = #is_summonable;

            #runtime_id_const
        }

        #bundle_struct

        #spawn_fn

        #group_enum

        #event_enum

        #(#property_enums)*
    };

    std::fs::write(
        definitions_dir.join(format!("{}.rs", module_name)),
        format_code(code)?,
    )
    .map_err(|e| miette::miette!("Failed to write entity definition: {}", e))?;

    Ok(())
}

fn generate_component_group_enum(entity: &ParsedEntity, struct_name: &str) -> TokenStream {
    let variants: Vec<TokenStream> = entity
        .component_groups
        .iter()
        .filter(|g| !g.to_pascal_case().is_empty())
        .map(|g| {
            let variant = format_ident!("{}", g.to_pascal_case());
            quote! { #variant }
        })
        .collect();

    if !variants.is_empty() {
        let enum_name = format_ident!("{}ComponentGroup", struct_name);
        quote! {
            /// Component groups for this entity
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub enum #enum_name {
                #(#variants),*
            }
        }
    } else {
        quote! {}
    }
}

fn generate_event_enum(entity: &ParsedEntity, struct_name: &str) -> TokenStream {
    let variants: Vec<TokenStream> = entity
        .events
        .iter()
        .filter(|e| !e.to_pascal_case().is_empty())
        .map(|e| {
            let variant = format_ident!("{}", e.to_pascal_case());
            quote! { #variant }
        })
        .collect();

    if !variants.is_empty() {
        let enum_name = format_ident!("{}Event", struct_name);
        quote! {
            /// Events for this entity
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub enum #enum_name {
                #(#variants),*
            }
        }
    } else {
        quote! {}
    }
}

fn generate_property_enums(entity: &ParsedEntity, struct_name: &str) -> Vec<TokenStream> {
    entity
        .properties
        .iter()
        .filter(|p| {
            p.prop_type == "enum"
                && !p.values.is_empty()
                && !p.default.is_empty()
                && !p.default.to_pascal_case().is_empty()
        })
        .filter_map(|p| {
            let prop_name = p
                .name
                .strip_prefix("minecraft:")
                .unwrap_or(&p.name)
                .to_pascal_case();
            if prop_name.is_empty() {
                return None;
            }
            let enum_name = format_ident!("{}{}", struct_name, prop_name);

            let variants: Vec<TokenStream> = p
                .values
                .iter()
                .filter(|v| !v.to_pascal_case().is_empty())
                .map(|v| {
                    let variant = format_ident!("{}", v.to_pascal_case());
                    quote! { #variant }
                })
                .collect();

            if variants.is_empty() {
                return None;
            }

            let default_pascal = p.default.to_pascal_case();
            if default_pascal.is_empty() {
                return None;
            }
            let default_variant = format_ident!("{}", default_pascal);

            Some(quote! {
                /// Synced property enum
                #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
                pub enum #enum_name {
                    #[default]
                    #default_variant,
                    #(#variants),*
                }
            })
        })
        .collect()
}

fn generate_bundle_struct(
    entity: &ParsedEntity,
    struct_name: &str,
    schemas: &HashMap<String, ComponentSchema>,
) -> TokenStream {
    let bundle_name = format_ident!("{}Bundle", struct_name);

    let bundle_fields: Vec<TokenStream> = entity
        .components
        .iter()
        .filter_map(|component| {
            let field_name = format_ident!("{}", component.name.to_snake_case());
            let type_name = schemas
                .get(&component.name)
                .map(|schema| schema.rust_name.clone())
                .unwrap_or_else(|| component.name.to_pascal_case());
            if type_name.is_empty() {
                return None;
            }
            let type_ident = format_ident!("{}", type_name);
            Some(quote! { pub #field_name: super::super::components::#type_ident })
        })
        .collect();

    if bundle_fields.is_empty() {
        return quote! {};
    }

    let doc = format!(" Component bundle for spawning a `{}`", entity.identifier);

    quote! {
        #[doc = #doc]
        #[derive(Bundle, Clone)]
        pub struct #bundle_name {
            #(#bundle_fields),*
        }
    }
}

fn generate_spawn_function(
    entity: &ParsedEntity,
    struct_name: &str,
    schemas: &HashMap<String, ComponentSchema>,
    defaults: &ComponentDefaults,
) -> TokenStream {
    let fn_name = format_ident!("spawn_{}", entity.name.to_snake_case());
    let bundle_name = format_ident!("{}Bundle", struct_name);

    let component_inits: Vec<TokenStream> = entity
        .components
        .iter()
        .map(|component| {
            let field_name = format_ident!("{}", component.name.to_snake_case());
            let init = generate_component_init(component, schemas.get(&component.name), defaults);
            quote! { #field_name: #init }
        })
        .collect();

    if component_inits.is_empty() {
        return quote! {};
    }

    let doc = format!(
        " Spawn a new `{}` entity with default Bedrock components",
        entity.identifier
    );

    quote! {
        #[doc = #doc]
        pub fn #fn_name(commands: &mut Commands) -> Entity {
            commands.spawn(#bundle_name {
                #(#component_inits),*
            }).id()
        }
    }
}

fn generate_component_init(
    comp: &ParsedComponent,
    schema: Option<&ComponentSchema>,
    defaults: &ComponentDefaults,
) -> TokenStream {
    if let Some(schema) = schema {
        let type_name = format_ident!("{}", schema.rust_name);

        if schema.is_marker {
            return quote! { super::super::components::#type_name };
        }

        let component_value = comp.data.as_ref();
        let object_value = component_value.and_then(Value::as_object);
        let mut field_values: Vec<TokenStream> = schema
            .root
            .fields
            .iter()
            .map(|field| {
                let field_ident = format_ident!("{}", field.rust_name);
                let value = if field.is_primary {
                    match component_value {
                        Some(Value::Object(map)) => lookup_object_value(map, &field.name)
                            .or_else(|| defaults.get_primary_default(&schema.name)),
                        Some(value) => Some(value),
                        None => defaults.get_primary_default(&schema.name),
                    }
                } else {
                    object_value
                        .and_then(|map| lookup_object_value(map, &field.name))
                        .or_else(|| defaults.get_field_default(&schema.name, &field.name))
                };
                let path = vec![schema.rust_name.clone(), field.name.to_pascal_case()];
                let value_tokens = default_value_tokens(&field.field_type, value, &path);
                quote! { #field_ident: #value_tokens }
            })
            .collect();

        if let Some(additional_schema) = &schema.root.additional {
            let mut entries = Vec::new();
            if let Some(map) = object_value {
                for (key, value) in map {
                    if schema
                        .root
                        .fields
                        .iter()
                        .any(|field| field_matches_lookup(key, &field.name))
                    {
                        continue;
                    }
                    let value_tokens = default_value_tokens(
                        additional_schema,
                        Some(value),
                        &[schema.rust_name.clone(), "Additional".to_string()],
                    );
                    entries.push(quote! { (#key.to_string(), #value_tokens) });
                }
            }

            if entries.is_empty() {
                field_values.push(quote! { additional: std::collections::HashMap::new() });
            } else {
                field_values
                    .push(quote! { additional: std::collections::HashMap::from([#(#entries),*]) });
            }
        }

        return quote! {
            super::super::components::#type_name {
                #(#field_values),*
            }
        };
    }

    let type_name = format_ident!("{}", comp.name.to_pascal_case());
    let data_tokens = if let Some(value) = comp.data.as_ref() {
        let value_tokens = dynamic_value_tokens(value);
        quote! { Some(#value_tokens) }
    } else {
        quote! { None }
    };

    quote! {
        super::super::components::#type_name {
            data: #data_tokens,
        }
    }
}

fn generate_entities_mod(
    entities: &[ParsedEntity],
    output_dir: &Path,
    _schemas: &HashMap<String, ComponentSchema>,
) -> miette::Result<()> {
    // Sort entities for deterministic output
    let mut sorted_entities: Vec<_> = entities.iter().collect();
    sorted_entities.sort_by(|a, b| a.name.cmp(&b.name));

    // definitions/mod.rs
    let def_mods: Vec<TokenStream> = sorted_entities
        .iter()
        .map(|e| {
            let mod_ident = format_ident!("{}", e.name.to_snake_case());
            quote! { pub mod #mod_ident; }
        })
        .collect();

    // Re-export spawn functions
    let spawn_uses: Vec<TokenStream> = sorted_entities
        .iter()
        .filter(|e| !e.components.is_empty())
        .map(|e| {
            let mod_ident = format_ident!("{}", e.name.to_snake_case());
            let fn_name = format_ident!("spawn_{}", e.name.to_snake_case());
            quote! { pub use #mod_ident::#fn_name; }
        })
        .collect();

    let entity_definitions: Vec<TokenStream> = sorted_entities
        .iter()
        .enumerate()
        .map(|(id, entity)| {
            let id = id as u32;
            let identifier = &entity.identifier;
            let spawn_category = option_str_tokens(entity.spawn_category.as_deref());
            let runtime_id = option_u32_tokens(entity.runtime_id);
            let is_spawnable = entity.is_spawnable;
            let is_summonable = entity.is_summonable;
            let (height, width) = entity_collision_box(entity);
            let height = option_f32_tokens(height);
            let width = option_f32_tokens(width);
            quote! {
                EntityDefinitionData {
                    id: #id,
                    identifier: #identifier,
                    spawn_category: #spawn_category,
                    is_spawnable: #is_spawnable,
                    is_summonable: #is_summonable,
                    runtime_id: #runtime_id,
                    height: #height,
                    width: #width,
                }
            }
        })
        .collect();
    let entity_lookup_arms: Vec<TokenStream> = sorted_entities
        .iter()
        .enumerate()
        .map(|(index, entity)| {
            let identifier = &entity.identifier;
            let index = syn::Index::from(index);
            quote! { #identifier => Some(&ALL_ENTITIES[#index]) }
        })
        .collect();
    let entity_count = entity_definitions.len();

    let def_code = quote! {
        //! Generated entity definitions.

        #(#def_mods)*

        /// Generated behavior-pack entity metadata.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct EntityDefinitionData {
            /// Internal registry ID assigned by deterministic codegen order.
            ///
            /// This is not a Bedrock packet/runtime entity ID.
            pub id: u32,
            /// Namespaced entity identifier.
            pub identifier: &'static str,
            /// Vanilla spawn category, when present.
            pub spawn_category: Option<&'static str>,
            /// Whether this entity can spawn naturally.
            pub is_spawnable: bool,
            /// Whether this entity can be summoned by commands.
            pub is_summonable: bool,
            /// Sourced Bedrock runtime entity ID, when present in the input data.
            pub runtime_id: Option<u32>,
            /// Collision box height from `minecraft:collision_box`, when present.
            pub height: Option<f32>,
            /// Collision box width from `minecraft:collision_box`, when present.
            pub width: Option<f32>,
        }

        /// All generated entity metadata, sorted by identifier-derived module name.
        pub const ALL_ENTITIES: [EntityDefinitionData; #entity_count] = [
            #(#entity_definitions),*
        ];

        /// Look up generated entity metadata by namespaced identifier.
        pub fn get(identifier: &str) -> Option<&'static EntityDefinitionData> {
            match identifier {
                #(#entity_lookup_arms,)*
                _ => None,
            }
        }

        // Re-export spawn functions
        #(#spawn_uses)*
    };

    std::fs::write(
        output_dir.join("definitions/mod.rs"),
        format_code(def_code)?,
    )
    .map_err(|e| miette::miette!("Failed to write definitions mod.rs: {}", e))?;

    // entities/mod.rs
    let entities_code = quote! {
        //! Generated entity definitions from Bedrock entity metadata.
        //!
        //! This module is auto-generated by `unastar-data-codegen`.
        //! Do not edit manually.
        //!
        //! ## Usage
        //!
        //! ```rust,ignore
        //! use unastar_data::entities::{components::*, definitions::spawn_zombie};
        //!
        //! fn spawn_system(mut commands: Commands) {
        //!     let entity = spawn_zombie(&mut commands);
        //! }
        //! ```

        pub mod components;
        pub mod definitions;
    };

    std::fs::write(output_dir.join("mod.rs"), format_code(entities_code)?)
        .map_err(|e| miette::miette!("Failed to write entities mod.rs: {}", e))?;

    Ok(())
}

fn option_str_tokens(value: Option<&str>) -> TokenStream {
    match value {
        Some(value) => quote! { Some(#value) },
        None => quote! { None },
    }
}

fn option_u32_tokens(value: Option<u32>) -> TokenStream {
    match value {
        Some(value) => quote! { Some(#value) },
        None => quote! { None },
    }
}

fn option_f32_tokens(value: Option<f32>) -> TokenStream {
    match value {
        Some(value) => quote! { Some(#value) },
        None => quote! { None },
    }
}

fn entity_collision_box(entity: &ParsedEntity) -> (Option<f32>, Option<f32>) {
    let Some(component) = entity
        .components
        .iter()
        .find(|component| component.name == "collision_box")
    else {
        return (None, None);
    };

    let Some(Value::Object(data)) = component.data.as_ref() else {
        return (None, None);
    };

    (
        data.get("height")
            .and_then(Value::as_f64)
            .map(|value| value as f32),
        data.get("width")
            .and_then(Value::as_f64)
            .map(|value| value as f32),
    )
}

fn format_code(code: TokenStream) -> miette::Result<String> {
    let file =
        syn::parse2(code).map_err(|e| miette::miette!("Failed to parse generated code: {}", e))?;
    Ok(prettyplease::unparse(&file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_index_metadata_extracts_spawn_and_collision_data() {
        let doc: KdlDocument = r#"
            entity "minecraft:test_mob" {
                spawn_category "monster"
                is_spawnable #true
                is_summonable #false
                components {
                    collision_box height=1.8 width=0.6
                }
            }
        "#
        .parse()
        .expect("parse entity kdl");

        let entity = parse_entity_node(&doc.nodes()[0]).expect("parse entity");

        assert_eq!(entity.spawn_category.as_deref(), Some("monster"));
        assert!(entity.is_spawnable);
        assert!(!entity.is_summonable);
        assert_eq!(entity_collision_box(&entity), (Some(1.8), Some(0.6)));
    }

    #[test]
    fn entity_runtime_id_rejects_negative_values() {
        let doc: KdlDocument = r#"
            entity "minecraft:test_mob" {
                runtime_id -1
            }
        "#
        .parse()
        .expect("parse entity kdl");

        let error = match parse_entity_node(&doc.nodes()[0]) {
            Ok(_) => panic!("negative runtime_id is invalid"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("runtime_id -1 is out of u32 range")
        );
    }

    #[test]
    fn generated_entity_lookup_covers_each_entity_with_direct_match() {
        let output_dir =
            std::env::temp_dir().join(format!("unastar-entities-test-{}", std::process::id()));
        let definitions_dir = output_dir.join("definitions");
        let _ = std::fs::remove_dir_all(&output_dir);
        std::fs::create_dir_all(&definitions_dir).expect("create temp definitions dir");

        let entities = vec![
            ParsedEntity {
                identifier: "minecraft:zombie".to_string(),
                name: "zombie".to_string(),
                spawn_category: Some("monster".to_string()),
                is_spawnable: true,
                is_summonable: true,
                runtime_id: Some(32),
                components: Vec::new(),
                component_groups: Vec::new(),
                events: Vec::new(),
                properties: Vec::new(),
            },
            ParsedEntity {
                identifier: "minecraft:allay".to_string(),
                name: "allay".to_string(),
                spawn_category: Some("creature".to_string()),
                is_spawnable: true,
                is_summonable: true,
                runtime_id: Some(134),
                components: Vec::new(),
                component_groups: Vec::new(),
                events: Vec::new(),
                properties: Vec::new(),
            },
        ];
        let schemas = HashMap::new();

        generate_entities_mod(&entities, &output_dir, &schemas).expect("generate entities module");
        let generated =
            std::fs::read_to_string(definitions_dir.join("mod.rs")).expect("read definitions mod");
        let _ = std::fs::remove_dir_all(&output_dir);

        assert_eq!(
            generated.matches("=> Some(&ALL_ENTITIES[").count(),
            entities.len()
        );
        assert!(generated.contains("\"minecraft:allay\" => Some(&ALL_ENTITIES[0])"));
        assert!(generated.contains("\"minecraft:zombie\" => Some(&ALL_ENTITIES[1])"));
        assert!(!generated.contains("ALL_ENTITIES.iter().find"));
    }
}
