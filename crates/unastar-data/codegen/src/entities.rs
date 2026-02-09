//! Entity code generation from KDL.

use crate::component_schemas::{
    ComponentDefaults, ComponentFrequency, ComponentSchema, FieldType, get_component_schemas,
    get_sparse_components,
};
use heck::{ToPascalCase, ToSnakeCase};
use kdl::{KdlDocument, KdlNode, KdlValue};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tracing::info;

/// Parsed entity from KDL for code generation
struct ParsedEntity {
    identifier: String,
    name: String, // without minecraft: prefix
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
    /// Primary argument value (e.g., `health 20` -> Some("20"))
    primary_value: Option<String>,
    /// Named properties (e.g., `max=20` -> {"max": "20"})
    properties: HashMap<String, String>,
    /// Whether this is a marker component (no data)
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

    // Load defaults from the data directory (relative to input)
    let defaults_path = input_dir
        .parent()
        .unwrap_or(input_dir)
        .join("data/overrides/_defaults.kdl");

    let defaults = if defaults_path.exists() {
        info!("Loading defaults from {}", defaults_path.display());
        ComponentDefaults::load(&defaults_path)?
    } else {
        info!("No _defaults.kdl found, using empty defaults");
        ComponentDefaults::default()
    };

    let content = std::fs::read_to_string(&kdl_path)
        .map_err(|e| miette::miette!("Failed to read entities.kdl: {}", e))?;

    let doc: KdlDocument = content
        .parse()
        .map_err(|e| miette::miette!("KDL parse error: {}", e))?;

    // Parse entities
    let mut entities = Vec::new();
    let mut all_components = HashSet::new();

    for node in doc.nodes() {
        if node.name().value() == "entity" {
            let entity = parse_entity_node(node)?;
            for comp in &entity.components {
                all_components.insert(comp.name.clone());
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

    // Generate components module with typed structs (using defaults from KDL)
    generate_components_module(&all_components, &entities_dir, &defaults)?;

    // Generate entity definitions with spawn functions
    for entity in &entities {
        generate_entity_definition(entity, &entities_dir)?;
    }

    // Generate mod.rs files
    generate_entities_mod(&entities, &entities_dir)?;

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
                        .map(|i| i as u32);
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
                            if event.name().value() == "event" {
                                if let Some(name) =
                                    event.entries().first().and_then(|e| e.value().as_string())
                                {
                                    entity.events.push(name.to_string());
                                }
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
    let mut primary_value = None;
    let mut properties = HashMap::new();

    for entry in node.entries() {
        if let Some(prop_name) = entry.name() {
            // Named property
            properties.insert(
                prop_name.value().to_string(),
                kdl_value_to_string(entry.value()),
            );
        } else {
            // Positional argument (primary value)
            primary_value = Some(kdl_value_to_string(entry.value()));
        }
    }

    let is_marker = primary_value.is_none() && properties.is_empty() && node.children().is_none();

    ParsedComponent {
        name,
        primary_value,
        properties,
        is_marker,
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
                "default" => {
                    prop.default = kdl_value_to_string(entry.value());
                }
                "client_sync" => {
                    prop.client_sync = entry.value().as_bool().unwrap_or(false);
                }
                _ => {}
            }
        }
    }

    // Get enum values from children
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "value" {
                if let Some(entry) = child.entries().first() {
                    if let Some(s) = entry.value().as_string() {
                        prop.values.push(s.to_string());
                    }
                }
            }
        }
    }

    Some(prop)
}

fn generate_components_module(
    components: &HashSet<String>,
    output_dir: &Path,
    defaults: &ComponentDefaults,
) -> miette::Result<()> {
    let components_dir = output_dir.join("components");
    let schemas = get_component_schemas();
    let sparse_components = get_sparse_components();

    let mut mod_items = Vec::new();

    for name in components {
        let module_name = name.to_snake_case();
        let struct_name = name.to_pascal_case();

        // Skip empty or invalid names
        if module_name.is_empty() || struct_name.is_empty() {
            tracing::warn!("Skipping component with invalid name: {:?}", name);
            continue;
        }

        // Check if we have a schema for this component
        let code = if let Some(schema) = schemas.get(name.as_str()) {
            generate_typed_component(schema, &sparse_components, defaults)
        } else {
            // Fallback to generic component
            let is_sparse = sparse_components.contains(&name.as_str());
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

    // Sort for deterministic output
    mod_items.sort_by(|a, b| a.0.cmp(&b.0));

    // Generate mod.rs
    let mods: Vec<TokenStream> = mod_items
        .iter()
        .map(|(m, _)| {
            let mod_ident = format_ident!("{}", m);
            quote! { pub mod #mod_ident; }
        })
        .collect();

    let uses: Vec<TokenStream> = mod_items
        .iter()
        .map(|(m, s)| {
            let mod_ident = format_ident!("{}", m);
            let struct_ident = format_ident!("{}", s);
            quote! { pub use #mod_ident::#struct_ident; }
        })
        .collect();

    let code = quote! {
        //! Generated component DTOs from Bedrock behavior packs.
        //!
        //! Components marked with `#[component(storage = "SparseSet")]` are:
        //! - Rarely present across entities, or
        //! - Frequently added/removed at runtime
        //!
        //! All other components use the default Table storage for cache-friendly iteration.

        #(#mods)*

        #(#uses)*
    };

    std::fs::write(components_dir.join("mod.rs"), format_code(code)?)
        .map_err(|e| miette::miette!("Failed to write components mod.rs: {}", e))?;

    Ok(())
}

fn generate_typed_component(
    schema: &ComponentSchema,
    sparse_list: &[&str],
    defaults: &ComponentDefaults,
) -> TokenStream {
    let struct_ident = format_ident!("{}", schema.rust_name);
    let doc = format!(" Bedrock component `minecraft:{}`", schema.name);

    let is_sparse = matches!(schema.frequency, ComponentFrequency::Sparse)
        || sparse_list.contains(&schema.name);

    let storage_attr = if is_sparse {
        quote! { #[component(storage = "SparseSet")] }
    } else {
        quote! {}
    };

    if schema.is_marker {
        // Marker component - no fields
        quote! {
            use bevy_ecs::prelude::*;

            #[doc = #doc]
            #[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
            #storage_attr
            pub struct #struct_ident;
        }
    } else {
        // Component with fields
        let fields: Vec<TokenStream> = schema
            .fields
            .iter()
            .map(|f| {
                let field_ident = format_ident!("{}", f.rust_name);
                let field_type = field_type_to_tokens(&f.field_type);
                let doc = format!(" {}", f.name);
                quote! {
                    #[doc = #doc]
                    pub #field_ident: #field_type
                }
            })
            .collect();

        let default_impl = generate_default_impl(schema, defaults);

        quote! {
            use bevy_ecs::prelude::*;

            #[doc = #doc]
            #[derive(Component, Debug, Clone, PartialEq)]
            #storage_attr
            pub struct #struct_ident {
                #(#fields),*
            }

            #default_impl
        }
    }
}

fn generate_default_impl(schema: &ComponentSchema, defaults: &ComponentDefaults) -> TokenStream {
    let struct_ident = format_ident!("{}", schema.rust_name);

    let field_defaults: Vec<TokenStream> = schema
        .fields
        .iter()
        .map(|f| {
            let field_ident = format_ident!("{}", f.rust_name);

            // Look up default from KDL: primary value or field property
            let kdl_default = if f.is_primary {
                defaults.get_primary_default(schema.name)
            } else {
                defaults.get_field_default(schema.name, f.name)
            };

            let default_value = field_default_value(&f.field_type, kdl_default);
            quote! { #field_ident: #default_value }
        })
        .collect();

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

fn field_type_to_tokens(field_type: &FieldType) -> TokenStream {
    match field_type {
        FieldType::Float => quote! { f32 },
        FieldType::Integer => quote! { i32 },
        FieldType::Bool => quote! { bool },
        FieldType::String => quote! { String },
        FieldType::Option(inner) => {
            let inner_tokens = field_type_to_tokens(inner);
            quote! { Option<#inner_tokens> }
        }
    }
}

fn field_default_value(field_type: &FieldType, default: Option<&str>) -> TokenStream {
    match field_type {
        FieldType::Float => {
            if let Some(d) = default {
                if let Ok(v) = d.parse::<f32>() {
                    return quote! { #v };
                }
            }
            quote! { 0.0 }
        }
        FieldType::Integer => {
            if let Some(d) = default {
                if let Ok(v) = d.parse::<i32>() {
                    return quote! { #v };
                }
            }
            quote! { 0 }
        }
        FieldType::Bool => {
            if let Some(d) = default {
                if d == "true" {
                    return quote! { true };
                }
            }
            quote! { false }
        }
        FieldType::String => {
            if let Some(d) = default {
                return quote! { #d.to_string() };
            }
            quote! { String::new() }
        }
        FieldType::Option(_) => quote! { None },
    }
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
            /// Raw data - schema not yet defined
            pub data: Option<serde_json::Value>,
        }
    }
}

fn generate_entity_definition(entity: &ParsedEntity, output_dir: &Path) -> miette::Result<()> {
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

    // Generate spawn function
    let spawn_fn = generate_spawn_function(entity, &struct_name);

    // Generate bundle struct
    let bundle_struct = generate_bundle_struct(entity, &struct_name);

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

        use bevy_ecs::prelude::*;
        use super::super::components::*;

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

fn generate_bundle_struct(entity: &ParsedEntity, struct_name: &str) -> TokenStream {
    let bundle_name = format_ident!("{}Bundle", struct_name);
    let schemas = get_component_schemas();

    // Only include components we have schemas for (typed components)
    let bundle_fields: Vec<TokenStream> = entity
        .components
        .iter()
        .filter(|c| schemas.contains_key(c.name.as_str()))
        .map(|c| {
            let schema = schemas.get(c.name.as_str()).unwrap();
            let field_name = format_ident!("{}", c.name.to_snake_case());
            let type_name = format_ident!("{}", schema.rust_name);
            quote! { pub #field_name: #type_name }
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

fn generate_spawn_function(entity: &ParsedEntity, struct_name: &str) -> TokenStream {
    let fn_name = format_ident!("spawn_{}", entity.name.to_snake_case());
    let bundle_name = format_ident!("{}Bundle", struct_name);
    let schemas = get_component_schemas();

    // Generate component initializers
    let component_inits: Vec<TokenStream> = entity
        .components
        .iter()
        .filter(|c| schemas.contains_key(c.name.as_str()))
        .map(|c| {
            let schema = schemas.get(c.name.as_str()).unwrap();
            let field_name = format_ident!("{}", c.name.to_snake_case());
            let init = generate_component_init(c, schema);
            quote! { #field_name: #init }
        })
        .collect();

    if component_inits.is_empty() {
        // No typed components, return empty
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

fn generate_component_init(comp: &ParsedComponent, schema: &ComponentSchema) -> TokenStream {
    let type_name = format_ident!("{}", schema.rust_name);

    if schema.is_marker {
        return quote! { #type_name };
    }

    // Generate field values from parsed data
    let field_values: Vec<TokenStream> = schema
        .fields
        .iter()
        .map(|f| {
            let field_ident = format_ident!("{}", f.rust_name);

            // Check if we have a value for this field
            let value = if f.is_primary {
                comp.primary_value.as_deref()
            } else {
                comp.properties.get(f.name).map(|s| s.as_str())
            };

            let value_tokens = if let Some(v) = value {
                parse_field_value(&f.field_type, v)
            } else {
                // No value provided - use type default (zero/false/empty)
                field_default_value(&f.field_type, None)
            };

            quote! { #field_ident: #value_tokens }
        })
        .collect();

    quote! {
        #type_name {
            #(#field_values),*
        }
    }
}

fn parse_field_value(field_type: &FieldType, value: &str) -> TokenStream {
    match field_type {
        FieldType::Float => {
            if let Ok(v) = value.parse::<f32>() {
                quote! { #v }
            } else {
                quote! { 0.0 }
            }
        }
        FieldType::Integer => {
            if let Ok(v) = value.parse::<i32>() {
                quote! { #v }
            } else {
                quote! { 0 }
            }
        }
        FieldType::Bool => {
            if value == "true" {
                quote! { true }
            } else {
                quote! { false }
            }
        }
        FieldType::String => {
            quote! { #value.to_string() }
        }
        FieldType::Option(inner) => {
            let inner_value = parse_field_value(inner, value);
            quote! { Some(#inner_value) }
        }
    }
}

fn generate_entities_mod(entities: &[ParsedEntity], output_dir: &Path) -> miette::Result<()> {
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
        .filter(|e| {
            // Only export if entity has typed components
            let schemas = get_component_schemas();
            e.components
                .iter()
                .any(|c| schemas.contains_key(c.name.as_str()))
        })
        .map(|e| {
            let mod_ident = format_ident!("{}", e.name.to_snake_case());
            let fn_name = format_ident!("spawn_{}", e.name.to_snake_case());
            quote! { pub use #mod_ident::#fn_name; }
        })
        .collect();

    let def_code = quote! {
        //! Generated entity definitions.

        #(#def_mods)*

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
        //! Generated entity definitions from Bedrock behavior packs.
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

fn format_code(code: TokenStream) -> miette::Result<String> {
    let file =
        syn::parse2(code).map_err(|e| miette::miette!("Failed to parse generated code: {}", e))?;
    Ok(prettyplease::unparse(&file))
}
