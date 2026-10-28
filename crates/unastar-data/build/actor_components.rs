//! Typed actor component bodies generated from the pinned stable entity
//! schemas (D2). Component identities and bodies are Rust types, not strings
//! looked up at runtime.
//!
//! Phases:
//! 1. **Select**: the registry names come from the actor parser registry
//!    (stable schemas plus the legacy form); each name's body schema is the one
//!    listed by the highest stable registry version
//!    ([`component_defaults::select`], the same choice as the defaults table).
//! 2. **Lower**: schema nodes become an IR of named types.
//!    - Definitions reached through `$ref` are deduplicated by (file, pointer)
//!      and named after the file stem (`minecraft_` stripped, UpperCamelCase);
//!      a component's own file is named after the component.
//!    - Inline objects, string enums and `oneOf`/`anyOf` unions are named
//!      `<Parent><Field>` (`<Parent><Field>Item` for array items).
//!    - The `Filter Group`/`Filter Test`/`Filter Group Map` definitions lower to
//!      the hand-written `FilterJson` alias: the runtime parses filters with
//!      `unastar_rules`. A filter field is always `Option<FilterJson>`; its
//!      published default is an all-null group, i.e. no filter.
//!    - Primitive types follow `type` + `x-underlying-type`; arrays with equal
//!      `minItems`/`maxItems` become fixed-size arrays. List fields also
//!      accept a single element (`actor_components::de::one_or_many`).
//!    - `FloatRange`/`IntRange` are lowered but implemented by hand
//!      ([`EXTERNAL_FILES`]).
//!    - Fields an older pinned schema published but the selected one dropped,
//!      and the few vanilla fields only the legacy forms document
//!      ([`FORM_ONLY_FIELDS`]), are appended as optional legacy fields.
//! 3. **Emit**: `quote` tokens formatted by `prettyplease`. Published defaults
//!    become serde default functions whose bodies are Rust literals checked
//!    against the field type here, so a default that does not fit its schema
//!    fails the build instead of a runtime decode.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{format_ident, quote};
use serde_json::Value;

use crate::component_defaults::{self, Schemas};

/// Schema files whose nodes are filters, kept as `FilterJson`.
const FILTER_FILES: [&str; 3] = [
    "Filter Group.json",
    "Filter Test.json",
    "Filter Group Map.json",
];

/// Shared definitions whose file stems make poor Rust names.
const RENAMED_FILES: [(&str, &str); 2] = [
    ("Expression Node.json", "MolangValue"),
    (
        "Molang expression (object representation).json",
        "MolangExpressionObject",
    ),
];

/// Range definitions the engine reads leniently: besides the published
/// `{"min", "max"}` object, vanilla writes a bare number, `[min, max]` and
/// `{"range_min", "range_max"}` (`behavior.ranged_attack.attack_interval`
/// 3.5, `behavior.place_block.xz_range` 0, `behavior.celebrate.jump_interval`).
/// They are lowered for default literals but implemented by hand in
/// `actor_components::range` with the same field names.
const EXTERNAL_FILES: [&str; 2] = ["FloatRange.json", "IntRange.json"];

/// Fields vanilla definitions author that no pinned schema version publishes.
/// They are documented only by the legacy Mojang Creator forms
/// (`data/upstream/mojang/entity_forms/<component>.form.json`, whose field
/// description becomes the doc comment); the type follows the form `dataType`
/// and the vanilla values.
const FORM_ONLY_FIELDS: [(&str, &str, &str); 8] = [
    ("minecraft:addrider", "entity_type", "string"),
    ("minecraft:addrider", "spawn_event", "string"),
    ("minecraft:behavior.leap_at_target", "target_dist", "number"),
    ("minecraft:behavior.random_fly", "avoid_damage_blocks", "boolean"),
    ("minecraft:behavior.random_fly", "y_offset", "number"),
    ("minecraft:behavior.random_look_around", "look_distance", "number"),
    ("minecraft:behavior.ranged_attack", "attack_interval_max", "number"),
    ("minecraft:behavior.ranged_attack", "attack_interval_min", "number"),
];

pub fn write(files: &[PathBuf], forms: &Path, registry: &BTreeSet<String>, out: &Path) {
    let mut schemas = Schemas::new();
    let selected = component_defaults::select(files, &mut schemas);
    let mut extra = legacy_fields(files, &selected, &mut schemas);
    for (component, field, ty) in FORM_ONLY_FIELDS {
        let form = forms.join(format!("{}.form.json", component.replace([':', '.'], "_")));
        let form = component_defaults::load(&form, &mut schemas);
        let description = form["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|entry| entry["id"] == field)
            .unwrap_or_else(|| panic!("{component}.{field} is not in its legacy form"))
            .get("description")
            .cloned()
            .unwrap_or_else(|| "Documented by the legacy Mojang form only.".into());
        let mut node = serde_json::json!({ "type": ty, "description": description });
        if ty == "number" {
            node["x-underlying-type"] = "float".into();
        }
        let fields = extra.entry(component.to_owned()).or_default();
        assert!(
            !fields.iter().any(|known| known.json == field),
            "{component}.{field} is published by a pinned schema; drop it from FORM_ONLY_FIELDS"
        );
        fields.push(ExtraField {
            json: field.to_owned(),
            path: forms.to_owned(),
            node,
        });
    }
    let mut lower = Lower {
        schemas: &mut schemas,
        types: Vec::new(),
        by_key: BTreeMap::new(),
        names: BTreeSet::new(),
        holes: Vec::new(),
        extra: BTreeMap::new(),
    };
    // Declare every component type first so a nested reference to another
    // component's schema file resolves to that component's type.
    let mut components = Vec::new();
    for (name, (registry_path, entry)) in &selected {
        assert!(
            registry.contains(name),
            "{name} has a stable schema but is missing from the actor parser registry"
        );
        let Some(reference) = entry.get("$ref") else {
            continue;
        };
        let resolved = component_defaults::resolve(
            registry_path,
            serde_json::json!({ "$ref": reference }),
            lower.schemas,
        );
        let id = lower.declare(component_type_name(name));
        let key = (resolved.path.clone(), resolved.pointer.clone());
        assert!(
            lower.by_key.insert(key, id).is_none(),
            "{name} shares its body schema with another component"
        );
        if let Some(fields) = extra.remove(name) {
            lower.extra.insert(id, fields);
        }
        components.push((name.clone(), id, resolved));
    }
    assert!(
        extra.is_empty(),
        "extra fields for components without a generated body: {:?}",
        extra.keys().collect::<Vec<_>>()
    );
    for (_, id, resolved) in &components {
        lower.define(*id, &resolved.path, &resolved.schema);
    }
    lower.alias_enum_defaults();
    assert!(
        lower.extra.is_empty(),
        "extra fields for non-object component schemas"
    );

    let emit = Emit { types: &lower.types };
    let body_items = emit.body_items(&components);
    let component_enum = component_enum(registry, &selected);
    let dispatch = components.iter().map(|(_, id, _)| {
        let ty = format_ident!("{}", lower.types[*id].name);
        quote!(ActorComponent::#ty => crate::actor_components::decode::<body::#ty>(body).map(|_| ()),)
    });
    let external = EXTERNAL_FILES.map(|file| format_ident!("{}", shared_type_name(file, "")));
    let tokens = quote! {
        #component_enum

        pub mod body {
            use crate::actor_components::{ActorComponent, ComponentBody, FilterJson};
            pub use crate::actor_components::range::{#(#external),*};
            use serde::Deserialize;

            #body_items
        }

        /// `None` when the component has no generated body.
        pub(super) fn decode_generated(
            component: ActorComponent,
            body: &serde_json::Value,
        ) -> Option<Result<(), String>> {
            let decoded = match component {
                #(#dispatch)*
                _ => return None,
            };
            Some(decoded)
        }
    };
    let file = syn::parse2::<syn::File>(tokens).expect("generated actor components parse");
    let mut source = format!(
        "// @generated by crates/unastar-data/build/actor_components.rs from the pinned\n\
         // stable entity schemas. Do not edit.\n\
         // {} registered components, {} generated component bodies, {} generated types\n\
         // (plus the hand-written ranges {}).\n\
         // Untyped schema nodes lowered to serde_json::Value: {}\n",
        registry.len(),
        components.len(),
        lower.types.iter().filter(|definition| !definition.external).count(),
        EXTERNAL_FILES.map(|file| shared_type_name(file, "")).join(", "),
        lower.holes.len(),
    );
    for hole in &lower.holes {
        source.push_str(&format!("//   - {hole}\n"));
    }
    source.push('\n');
    source.push_str(&prettyplease::unparse(&file));
    std::fs::write(out.join("actor_components.rs"), source).expect("writing actor components");
}

/// A field outside the selected schema: dropped by the selected version but
/// published by an older pinned one (older-format definitions may still use
/// it), or documented only by a legacy form.
struct ExtraField {
    json: String,
    /// Base for resolving `$ref`s inside `node`.
    path: PathBuf,
    node: Value,
}

/// Fields published by an older pinned schema of a component but absent from
/// its selected (highest) schema. The newest version publishing a field wins.
fn legacy_fields(
    files: &[PathBuf],
    selected: &BTreeMap<String, (PathBuf, Value)>,
    schemas: &mut Schemas,
) -> BTreeMap<String, Vec<ExtraField>> {
    let mut registries: Vec<_> = files
        .iter()
        .map(|path| {
            let registry = component_defaults::load(path, schemas);
            let version = component_defaults::version(
                registry["x-format-version"].as_str().expect("schema version"),
            );
            (version, path.clone(), registry)
        })
        .collect();
    registries.sort_by_key(|registry| std::cmp::Reverse(registry.0));
    let mut extra = BTreeMap::<String, Vec<ExtraField>>::new();
    for (name, (selected_path, entry)) in selected {
        let Some(reference) = entry.get("$ref") else {
            continue;
        };
        let current = component_defaults::resolve(
            selected_path,
            serde_json::json!({ "$ref": reference }),
            schemas,
        );
        let mut known: BTreeSet<String> = current
            .schema
            .get("properties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(field, _)| field.clone())
            .collect();
        for (_, registry_path, registry) in &registries {
            let Some(reference) = registry["properties"][name].get("$ref") else {
                continue;
            };
            let older = component_defaults::resolve(
                registry_path,
                serde_json::json!({ "$ref": reference }),
                schemas,
            );
            let Some(properties) = older.schema.get("properties").and_then(Value::as_object)
            else {
                continue;
            };
            for (field, node) in properties {
                if known.insert(field.clone()) {
                    extra.entry(name.clone()).or_default().push(ExtraField {
                        json: field.clone(),
                        path: older.path.clone(),
                        node: node.clone(),
                    });
                }
            }
        }
    }
    extra
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Int {
    I16,
    U8,
    U32,
    I32,
}

impl Int {
    fn of(node: &Value) -> Self {
        match node.get("x-underlying-type").and_then(Value::as_str) {
            None | Some("int32") => Self::I32,
            Some("int16") => Self::I16,
            Some("uint8") => Self::U8,
            Some("uint32") => Self::U32,
            Some(other) => panic!("unsupported integer x-underlying-type {other}"),
        }
    }

    fn range(self) -> (i64, i64) {
        match self {
            Self::I16 => (i16::MIN.into(), i16::MAX.into()),
            Self::U8 => (0, u8::MAX.into()),
            Self::U32 => (0, u32::MAX.into()),
            Self::I32 => (i32::MIN.into(), i32::MAX.into()),
        }
    }

    fn tokens(self) -> TokenStream {
        match self {
            Self::I16 => quote!(i16),
            Self::U8 => quote!(u8),
            Self::U32 => quote!(u32),
            Self::I32 => quote!(i32),
        }
    }
}

#[derive(Clone, PartialEq)]
enum Ty {
    Bool,
    Int(Int),
    F32,
    String,
    Vec(Box<Ty>),
    Array(Box<Ty>, usize),
    Map(Box<Ty>),
    Named(usize),
    Filter,
    /// An untyped schema node (recorded in `Lower::holes`).
    Json,
}

struct TypeDef {
    name: String,
    doc: Option<String>,
    kind: Kind,
    /// Implemented by hand ([`EXTERNAL_FILES`]); lowered only for literals.
    external: bool,
}

enum Kind {
    Pending,
    Struct(Vec<Field>),
    /// `#[serde(transparent)]` wrapper for a non-object definition.
    Newtype(Ty),
    /// String enum.
    Enum(Vec<EnumVariant>),
    /// `#[serde(untagged)]` union: (variant ident, payload).
    Untagged(Vec<(String, Ty)>),
}

struct EnumVariant {
    ident: String,
    json: String,
    /// Extra accepted spellings (see `Lower::alias_enum_defaults`).
    aliases: Vec<String>,
}

impl EnumVariant {
    fn accepts(&self, value: &str) -> bool {
        self.json == value || self.aliases.iter().any(|alias| alias == value)
    }
}

struct Field {
    json: String,
    ident: Ident,
    ty: Ty,
    doc: Option<String>,
    presence: Presence,
}

enum Presence {
    Required,
    Optional,
    Default(Value),
}

struct Lower<'a> {
    schemas: &'a mut Schemas,
    types: Vec<TypeDef>,
    by_key: BTreeMap<(PathBuf, String), usize>,
    names: BTreeSet<String>,
    holes: Vec<String>,
    /// Component type id → fields appended after its selected schema's.
    extra: BTreeMap<usize, Vec<ExtraField>>,
}

impl Lower<'_> {
    fn declare(&mut self, name: String) -> usize {
        assert!(
            self.names.insert(name.clone()),
            "two generated actor component types are named {name}"
        );
        self.types.push(TypeDef {
            name,
            doc: None,
            kind: Kind::Pending,
            external: false,
        });
        self.types.len() - 1
    }

    fn lower(&mut self, path: &Path, node: &Value, hint: &str) -> Ty {
        if let Some(reference) = node.get("$ref") {
            let resolved = component_defaults::resolve(
                path,
                serde_json::json!({ "$ref": reference }),
                self.schemas,
            );
            let file = file_name(&resolved.path);
            if FILTER_FILES.contains(&file) {
                return Ty::Filter;
            }
            let key = (resolved.path.clone(), resolved.pointer.clone());
            if let Some(&id) = self.by_key.get(&key) {
                return Ty::Named(id);
            }
            if !needs_name(&resolved.schema) {
                return self.lower(&resolved.path, &resolved.schema, hint);
            }
            let id = self.declare(shared_type_name(file, &resolved.pointer));
            self.types[id].external = EXTERNAL_FILES.contains(&file);
            self.by_key.insert(key, id);
            self.define(id, &resolved.path, &resolved.schema);
            return Ty::Named(id);
        }
        if needs_name(node) {
            let id = self.declare(hint.to_owned());
            self.define(id, path, node);
            return Ty::Named(id);
        }
        match node.get("type").and_then(Value::as_str) {
            Some("boolean") => Ty::Bool,
            Some("integer") => Ty::Int(Int::of(node)),
            Some("number") => Ty::F32,
            Some("string") => Ty::String,
            Some("array") => {
                let Some(items) = node.get("items") else {
                    self.holes.push(format!("{hint} (array without items)"));
                    return Ty::Vec(Box::new(Ty::Json));
                };
                let item = Box::new(self.lower(path, items, &format!("{hint}Item")));
                let min = node.get("minItems").and_then(Value::as_u64);
                let max = node.get("maxItems").and_then(Value::as_u64);
                match (min, max) {
                    (Some(min), Some(max)) if min == max => {
                        Ty::Array(item, usize::try_from(min).expect("array length"))
                    }
                    _ => Ty::Vec(item),
                }
            }
            Some("object") => {
                let values = node
                    .get("additionalProperties")
                    .filter(|values| values.is_object())
                    .expect("needs_name covers objects without additionalProperties");
                Ty::Map(Box::new(self.lower(path, values, &format!("{hint}Value"))))
            }
            _ => {
                self.holes.push(format!("{hint} (untyped)"));
                Ty::Json
            }
        }
    }

    fn define(&mut self, id: usize, path: &Path, node: &Value) {
        let name = self.types[id].name.clone();
        let kind = if let Some(variants) = node
            .get("oneOf")
            .or_else(|| node.get("anyOf"))
            .and_then(Value::as_array)
        {
            let mut lowered = Vec::<Ty>::new();
            for (index, variant) in variants.iter().enumerate() {
                let inline_object = variant.get("$ref").is_none()
                    && variant.get("type").and_then(Value::as_str) == Some("object")
                    && variant.get("properties").is_some();
                let hint = if inline_object {
                    format!("{name}Object")
                } else {
                    format!("{name}Variant{index}")
                };
                let ty = self.lower(path, variant, &hint);
                if !lowered.contains(&ty) {
                    lowered.push(ty);
                }
            }
            if lowered.len() == 1 {
                Kind::Newtype(lowered.pop().expect("one variant"))
            } else {
                let mut used = BTreeSet::new();
                Kind::Untagged(
                    lowered
                        .into_iter()
                        .map(|ty| (unique(&mut used, self.variant_label(&ty)), ty))
                        .collect(),
                )
            }
        } else if let Some(values) = node.get("enum").and_then(Value::as_array) {
            let mut used = BTreeSet::new();
            Kind::Enum(
                values
                    .iter()
                    .map(|value| {
                        let value = value
                            .as_str()
                            .unwrap_or_else(|| panic!("{name}: non-string enum value {value}"));
                        EnumVariant {
                            ident: unique(&mut used, variant_ident(value)),
                            json: value.to_owned(),
                            aliases: Vec::new(),
                        }
                    })
                    .collect(),
            )
        } else if node.get("type").and_then(Value::as_str) == Some("object") {
            let required: BTreeSet<&str> = node
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|field| field.as_str().expect("required field name"))
                .collect();
            let mut fields = Vec::new();
            let mut idents = BTreeSet::new();
            let properties = node.get("properties").and_then(Value::as_object);
            let mut properties: Vec<_> = properties
                .into_iter()
                .flatten()
                .map(|(json, property)| (json.clone(), path.to_owned(), property.clone(), false))
                .collect();
            properties.sort_by(|a, b| a.0.cmp(&b.0));
            let mut extra: Vec<_> = self
                .extra
                .remove(&id)
                .into_iter()
                .flatten()
                .map(|field| (field.json, field.path, field.node, true))
                .collect();
            extra.sort_by(|a, b| a.0.cmp(&b.0));
            properties.extend(extra);
            for (json, path, property, legacy) in properties {
                let ty = self.lower(&path, &property, &format!("{name}{}", camel(&json)));
                let annotations = component_defaults::resolve(&path, property, self.schemas).schema;
                // Legacy fields stay optional: absence must stay observable.
                let presence = if ty == Ty::Filter || legacy {
                    Presence::Optional
                } else if required.contains(json.as_str()) {
                    Presence::Required
                } else {
                    match annotations.get("default") {
                        Some(value) if !value.is_null() => Presence::Default(value.clone()),
                        _ => Presence::Optional,
                    }
                };
                let ident = field_ident(&json);
                assert!(
                    idents.insert(ident.to_string()),
                    "{name}: two fields map to {ident}"
                );
                let doc = description(&annotations);
                let doc = if legacy {
                    Some(match doc {
                        Some(doc) => format!("Legacy field, absent from the selected schema.\n{doc}"),
                        None => "Legacy field, absent from the selected schema.".to_owned(),
                    })
                } else {
                    doc
                };
                fields.push(Field {
                    json,
                    ident,
                    ty,
                    doc,
                    presence,
                });
            }
            Kind::Struct(fields)
        } else {
            let ty = self.lower(path, node, &format!("{name}Value"));
            Kind::Newtype(ty)
        };
        let definition = &mut self.types[id];
        definition.doc = description(node);
        definition.kind = kind;
    }

    /// Some schemas publish a default whose spelling differs in case from the
    /// enum it belongs to (`liquid_material_type` lists `Any`, the move_to_liquid
    /// default is `any`). The engine accepts its own default, so that spelling
    /// becomes a serde alias of the matching variant.
    fn alias_enum_defaults(&mut self) {
        let mut aliases = Vec::new();
        for definition in &self.types {
            let Kind::Struct(fields) = &definition.kind else {
                continue;
            };
            for field in fields {
                let (Ty::Named(id), Presence::Default(Value::String(value))) =
                    (&field.ty, &field.presence)
                else {
                    continue;
                };
                let Kind::Enum(variants) = &self.types[*id].kind else {
                    continue;
                };
                if variants.iter().any(|variant| variant.accepts(value)) {
                    continue;
                }
                let index = variants
                    .iter()
                    .position(|variant| variant.json.eq_ignore_ascii_case(value))
                    .unwrap_or_else(|| {
                        panic!(
                            "{}.{}: default {value:?} is not a {} value",
                            definition.name, field.json, self.types[*id].name
                        )
                    });
                aliases.push((*id, index, value.clone()));
            }
        }
        for (id, index, alias) in aliases {
            let Kind::Enum(variants) = &mut self.types[id].kind else {
                unreachable!("collected from an enum");
            };
            variants[index].aliases.push(alias);
        }
    }

    fn variant_label(&self, ty: &Ty) -> String {
        match ty {
            Ty::Bool => "Bool".into(),
            Ty::Int(_) => "Int".into(),
            Ty::F32 => "Float".into(),
            Ty::String => "String".into(),
            Ty::Vec(_) => "List".into(),
            Ty::Array(_, _) => "Array".into(),
            Ty::Map(_) => "Map".into(),
            Ty::Named(id) => self.types[*id].name.clone(),
            Ty::Filter => "Filter".into(),
            Ty::Json => "Json".into(),
        }
    }
}

/// Whether a (non-reference) node lowers to its own named Rust type.
fn needs_name(node: &Value) -> bool {
    node.get("oneOf").is_some()
        || node.get("anyOf").is_some()
        || node.get("enum").is_some()
        || (node.get("type").and_then(Value::as_str) == Some("object")
            && (node.get("properties").is_some()
                || !node
                    .get("additionalProperties")
                    .is_some_and(Value::is_object)))
}

fn description(node: &Value) -> Option<String> {
    node.get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty() && *text != "Dynamic value")
        .map(str::to_owned)
}

/// One `#[doc]` line per description line, rendered as `/// text`. Some
/// descriptions carry literal `\n` escapes and tab indentation; lines are
/// split there and trimmed so indentation never turns into a code block, and
/// prose directly after a list gets a blank line so it does not continue the
/// last item.
fn doc_lines(text: &Option<String>) -> Vec<String> {
    let mut result = Vec::new();
    let Some(text) = text else {
        return result;
    };
    let mut in_list = false;
    for line in text.replace("\\n", "\n").lines().map(str::trim) {
        let item = line.starts_with("- ") || line.starts_with("* ");
        if in_list && !item && !line.is_empty() {
            result.push(String::new());
        }
        in_list = item;
        result.push(format!(" {line}"));
    }
    result
}

struct Emit<'a> {
    types: &'a [TypeDef],
}

impl Emit<'_> {
    fn body_items(&self, components: &[(String, usize, component_defaults::Resolved)]) -> TokenStream {
        let mut items = TokenStream::new();
        let mut default_fns = TokenStream::new();
        let mut default_names = BTreeSet::new();
        for definition in self.types.iter().filter(|definition| !definition.external) {
            let name = format_ident!("{}", definition.name);
            let doc = doc_lines(&definition.doc);
            let item = match &definition.kind {
                Kind::Pending => panic!("{} was declared but never defined", definition.name),
                Kind::Struct(fields) => {
                    let fields = fields.iter().map(|field| {
                        let ident = &field.ident;
                        let doc = doc_lines(&field.doc);
                        let ty = self.ty(&field.ty);
                        let unraw = ident.to_string();
                        let unraw = unraw.trim_start_matches("r#");
                        let rename = (unraw != field.json).then(|| {
                            let json = &field.json;
                            quote!(#[serde(rename = #json)])
                        });
                        // Bedrock's loader accepts a single element where a
                        // list is declared (vanilla `interact.interactions`
                        // object, `play_sounds` string).
                        let list = matches!(field.ty, Ty::Vec(_));
                        let (ty, default) = match &field.presence {
                            Presence::Required => {
                                let with = list.then(
                                    || quote!(#[serde(deserialize_with = "crate::actor_components::de::one_or_many")]),
                                );
                                (ty, with)
                            }
                            Presence::Optional => {
                                let with = list.then(
                                    || quote!(#[serde(default, deserialize_with = "crate::actor_components::de::optional_one_or_many")]),
                                );
                                (quote!(Option<#ty>), with)
                            }
                            Presence::Default(value) => {
                                let default = if trivial_default(&field.ty, value) {
                                    quote!(default)
                                } else {
                                    let function = default_fn(&definition.name, field);
                                    assert!(
                                        default_names.insert(function.to_string()),
                                        "duplicate default function {function}"
                                    );
                                    let path = function.to_string();
                                    let expr = self.expr(&field.ty, value, &definition.name);
                                    default_fns.extend(quote! {
                                        fn #function() -> #ty { #expr }
                                    });
                                    quote!(default = #path)
                                };
                                let with = list.then(
                                    || quote!(, deserialize_with = "crate::actor_components::de::one_or_many"),
                                );
                                (ty, Some(quote!(#[serde(#default #with)])))
                            }
                        };
                        quote! {
                            #(#[doc = #doc])*
                            #rename
                            #default
                            pub #ident: #ty,
                        }
                    });
                    quote! {
                        #(#[doc = #doc])*
                        #[derive(Debug, Clone, PartialEq, Deserialize)]
                        #[serde(deny_unknown_fields)]
                        pub struct #name { #(#fields)* }
                    }
                }
                Kind::Newtype(ty) => {
                    let ty = self.ty(ty);
                    quote! {
                        #(#[doc = #doc])*
                        #[derive(Debug, Clone, PartialEq, Deserialize)]
                        #[serde(transparent)]
                        pub struct #name(pub #ty);
                    }
                }
                Kind::Enum(variants) => {
                    let variants = variants.iter().map(|variant| {
                        let ident = format_ident!("{}", variant.ident);
                        let json = &variant.json;
                        let aliases = &variant.aliases;
                        quote!(#[serde(rename = #json #(, alias = #aliases)*)] #ident,)
                    });
                    quote! {
                        #(#[doc = #doc])*
                        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
                        pub enum #name { #(#variants)* }
                    }
                }
                Kind::Untagged(variants) => {
                    let variants = variants.iter().map(|(variant, ty)| {
                        let variant = format_ident!("{variant}");
                        let ty = self.ty(ty);
                        quote!(#variant(#ty),)
                    });
                    quote! {
                        #(#[doc = #doc])*
                        #[derive(Debug, Clone, PartialEq, Deserialize)]
                        #[serde(untagged)]
                        pub enum #name { #(#variants)* }
                    }
                }
            };
            items.extend(item);
        }
        for (_, id, _) in components {
            let name = format_ident!("{}", self.types[*id].name);
            items.extend(quote! {
                impl ComponentBody for #name {
                    const COMPONENT: ActorComponent = ActorComponent::#name;
                }
            });
        }
        items.extend(default_fns);
        items
    }

    fn ty(&self, ty: &Ty) -> TokenStream {
        match ty {
            Ty::Bool => quote!(bool),
            Ty::Int(int) => int.tokens(),
            Ty::F32 => quote!(f32),
            Ty::String => quote!(String),
            Ty::Vec(item) => {
                let item = self.ty(item);
                quote!(Vec<#item>)
            }
            Ty::Array(item, len) => {
                let item = self.ty(item);
                let len = Literal::usize_unsuffixed(*len);
                quote!([#item; #len])
            }
            Ty::Map(value) => {
                let value = self.ty(value);
                quote!(std::collections::BTreeMap<String, #value>)
            }
            Ty::Named(id) => {
                let name = format_ident!("{}", self.types[*id].name);
                quote!(#name)
            }
            Ty::Filter => quote!(FilterJson),
            Ty::Json => quote!(serde_json::Value),
        }
    }

    /// A Rust expression for a published default, checked against `ty`.
    fn expr(&self, ty: &Ty, value: &Value, context: &str) -> TokenStream {
        let fail = || -> ! { panic!("{context}: default {value} does not fit its schema type") };
        match ty {
            Ty::Bool => {
                let value = value.as_bool().unwrap_or_else(|| fail());
                quote!(#value)
            }
            Ty::Int(int) => {
                let value = integer(value, *int).unwrap_or_else(|| fail());
                let literal = Literal::u64_unsuffixed(value.unsigned_abs());
                if value < 0 {
                    quote!(-#literal)
                } else {
                    quote!(#literal)
                }
            }
            Ty::F32 => {
                let value = value.as_f64().unwrap_or_else(|| fail()) as f32;
                assert!(value.is_finite(), "{context}: non-finite default");
                let literal = Literal::f32_unsuffixed(value.abs());
                if value < 0.0 {
                    quote!(-#literal)
                } else {
                    quote!(#literal)
                }
            }
            Ty::String => {
                let value = value.as_str().unwrap_or_else(|| fail());
                quote!(String::from(#value))
            }
            Ty::Vec(item) => {
                let values = value.as_array().unwrap_or_else(|| fail());
                if values.is_empty() {
                    quote!(Vec::new())
                } else {
                    let values = values.iter().map(|value| self.expr(item, value, context));
                    quote!(vec![#(#values),*])
                }
            }
            Ty::Array(item, len) => {
                let values = value.as_array().filter(|values| values.len() == *len);
                let values = values.unwrap_or_else(|| fail());
                let values = values.iter().map(|value| self.expr(item, value, context));
                quote!([#(#values),*])
            }
            Ty::Map(item) => {
                let entries = value.as_object().unwrap_or_else(|| fail());
                if entries.is_empty() {
                    quote!(std::collections::BTreeMap::new())
                } else {
                    let entries = entries.iter().map(|(key, value)| {
                        let value = self.expr(item, value, context);
                        quote!((String::from(#key), #value))
                    });
                    quote!(std::collections::BTreeMap::from([#(#entries),*]))
                }
            }
            Ty::Filter | Ty::Json => json_expr(value),
            Ty::Named(id) => {
                let definition = &self.types[*id];
                let name = format_ident!("{}", definition.name);
                match &definition.kind {
                    Kind::Pending => unreachable!("types are defined before emission"),
                    Kind::Enum(variants) => {
                        let json = value.as_str().unwrap_or_else(|| fail());
                        let variant = variants
                            .iter()
                            .find(|variant| variant.accepts(json))
                            .unwrap_or_else(|| fail());
                        let variant = format_ident!("{}", variant.ident);
                        quote!(#name::#variant)
                    }
                    Kind::Newtype(inner) => {
                        let inner = self.expr(inner, value, context);
                        quote!(#name(#inner))
                    }
                    Kind::Untagged(variants) => {
                        let (variant, ty) = variants
                            .iter()
                            .find(|(_, ty)| self.matches(ty, value))
                            .unwrap_or_else(|| fail());
                        let variant = format_ident!("{variant}");
                        let inner = self.expr(ty, value, context);
                        quote!(#name::#variant(#inner))
                    }
                    Kind::Struct(fields) => {
                        let object = value.as_object().unwrap_or_else(|| fail());
                        for key in object.keys() {
                            if !fields.iter().any(|field| &field.json == key) {
                                fail();
                            }
                        }
                        let fields = fields.iter().map(|field| {
                            let ident = &field.ident;
                            let given = object
                                .get(&field.json)
                                .filter(|value| !value.is_null())
                                .filter(|value| field.ty != Ty::Filter || !empty_filter(value));
                            let expr = match (&field.presence, given) {
                                (Presence::Optional, Some(value)) => {
                                    let value = self.expr(&field.ty, value, context);
                                    quote!(Some(#value))
                                }
                                (_, Some(value)) => self.expr(&field.ty, value, context),
                                (Presence::Optional, None) => quote!(None),
                                (Presence::Default(value), None)
                                    if trivial_default(&field.ty, value) =>
                                {
                                    quote!(Default::default())
                                }
                                (Presence::Default(_), None) => {
                                    let function = default_fn(&definition.name, field);
                                    quote!(#function())
                                }
                                (Presence::Required, None) => fail(),
                            };
                            quote!(#ident: #expr)
                        });
                        quote!(#name { #(#fields),* })
                    }
                }
            }
        }
    }

    /// Mirrors the generated serde types closely enough to pick the untagged
    /// variant a default decodes as.
    fn matches(&self, ty: &Ty, value: &Value) -> bool {
        match ty {
            Ty::Bool => value.is_boolean(),
            Ty::Int(int) => integer(value, *int).is_some(),
            Ty::F32 => value.is_number(),
            Ty::String => value.is_string(),
            Ty::Vec(item) => value
                .as_array()
                .is_some_and(|values| values.iter().all(|value| self.matches(item, value))),
            Ty::Array(item, len) => value.as_array().is_some_and(|values| {
                values.len() == *len && values.iter().all(|value| self.matches(item, value))
            }),
            Ty::Map(item) => value
                .as_object()
                .is_some_and(|values| values.values().all(|value| self.matches(item, value))),
            Ty::Filter | Ty::Json => true,
            Ty::Named(id) => match &self.types[*id].kind {
                Kind::Pending => false,
                Kind::Enum(variants) => value
                    .as_str()
                    .is_some_and(|value| variants.iter().any(|variant| variant.accepts(value))),
                Kind::Newtype(inner) => self.matches(inner, value),
                Kind::Untagged(variants) => variants.iter().any(|(_, ty)| self.matches(ty, value)),
                Kind::Struct(fields) => value.as_object().is_some_and(|object| {
                    object
                        .keys()
                        .all(|key| fields.iter().any(|field| &field.json == key))
                        && fields.iter().all(|field| match object.get(&field.json) {
                            Some(Value::Null) | None => {
                                !matches!(field.presence, Presence::Required)
                            }
                            Some(value) => self.matches(&field.ty, value),
                        })
                }),
            },
        }
    }
}

fn integer(value: &Value, int: Int) -> Option<i64> {
    let value = value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|value| value.fract() == 0.0)
            .map(|value| value as i64)
    })?;
    let (min, max) = int.range();
    (min..=max).contains(&value).then_some(value)
}

/// Whether the published default equals the Rust type's `Default`, so a plain
/// `#[serde(default)]` is exact.
fn trivial_default(ty: &Ty, value: &Value) -> bool {
    match ty {
        Ty::Bool => value == &Value::Bool(false),
        Ty::Int(_) => value.as_f64() == Some(0.0),
        Ty::F32 => value.as_f64().is_some_and(|value| value == 0.0 && value.is_sign_positive()),
        Ty::String => value.as_str() == Some(""),
        Ty::Vec(_) => value.as_array().is_some_and(Vec::is_empty),
        Ty::Map(_) => value.as_object().is_some_and(serde_json::Map::is_empty),
        Ty::Array(_, _) | Ty::Named(_) | Ty::Filter | Ty::Json => false,
    }
}

/// The published "no filter" default is a group with every key null.
fn empty_filter(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.values().all(Value::is_null))
}

fn json_expr(value: &Value) -> TokenStream {
    match value {
        Value::Null => quote!(serde_json::Value::Null),
        Value::Bool(value) => quote!(serde_json::Value::Bool(#value)),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                let literal = Literal::i64_suffixed(value.abs());
                if value < 0 {
                    quote!(serde_json::Value::from(-#literal))
                } else {
                    quote!(serde_json::Value::from(#literal))
                }
            } else {
                let value = number.as_f64().expect("finite JSON number");
                let literal = Literal::f64_suffixed(value.abs());
                if value < 0.0 {
                    quote!(serde_json::Value::from(-#literal))
                } else {
                    quote!(serde_json::Value::from(#literal))
                }
            }
        }
        Value::String(value) => quote!(serde_json::Value::from(#value)),
        Value::Array(values) => {
            let values = values.iter().map(json_expr);
            quote!(serde_json::Value::Array(vec![#(#values),*]))
        }
        Value::Object(entries) => {
            let entries = entries.iter().map(|(key, value)| {
                let value = json_expr(value);
                quote!((String::from(#key), #value))
            });
            quote!(serde_json::Value::Object(serde_json::Map::from_iter([#(#entries),*])))
        }
    }
}

fn component_enum(
    registry: &BTreeSet<String>,
    selected: &BTreeMap<String, (PathBuf, Value)>,
) -> TokenStream {
    let variants: Vec<_> = registry
        .iter()
        .map(|name| format_ident!("{}", component_type_name(name)))
        .collect();
    let docs = registry.iter().map(|name| {
        let doc = doc_lines(&selected.get(name).and_then(|(_, entry)| description(entry)));
        quote!(#(#[doc = #doc])*)
    });
    let names = registry.iter();
    let from_names = registry.iter();
    quote! {
        /// Every actor component name in the pinned registry (stable schemas
        /// plus the legacy form), in name order.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum ActorComponent {
            #(#docs #variants,)*
        }

        impl ActorComponent {
            pub const ALL: &'static [ActorComponent] = &[#(Self::#variants),*];

            /// The namespaced component name, e.g. `minecraft:behavior.random_stroll`.
            pub const fn name(self) -> &'static str {
                match self {
                    #(Self::#variants => #names,)*
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                Some(match name {
                    #(#from_names => Self::#variants,)*
                    _ => return None,
                })
            }
        }
    }
}

fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .expect("UTF-8 schema file name")
}

/// `minecraft:behavior.random_stroll` → `BehaviorRandomStroll`.
fn component_type_name(name: &str) -> String {
    camel(
        name.strip_prefix("minecraft:")
            .unwrap_or_else(|| panic!("component {name} is not in the minecraft namespace")),
    )
}

fn shared_type_name(file: &str, pointer: &str) -> String {
    if let Some((_, name)) = RENAMED_FILES.iter().find(|(renamed, _)| *renamed == file) {
        assert!(pointer.is_empty(), "renamed schema {file} is referenced by pointer");
        return (*name).to_owned();
    }
    let stem = file.strip_suffix(".json").expect("JSON schema file");
    let stem = stem.strip_prefix("minecraft_").unwrap_or(stem);
    format!("{}{}", camel(stem), camel(pointer))
}

/// UpperCamelCase over every non-alphanumeric boundary; apostrophes vanish
/// (`Goal's control flags` → `GoalsControlFlags`).
fn camel(source: &str) -> String {
    let mut result = String::new();
    for part in source
        .replace('\'', "")
        .split(|c: char| !c.is_ascii_alphanumeric())
    {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            result.push(first.to_ascii_uppercase());
            result.extend(chars);
        }
    }
    if result.starts_with(|c: char| c.is_ascii_digit()) {
        result.insert(0, 'V');
    }
    result
}

fn variant_ident(value: &str) -> String {
    match camel(value).as_str() {
        "" => "Empty".to_owned(),
        "Self" => "SelfValue".to_owned(),
        other => other.to_owned(),
    }
}

fn unique(used: &mut BTreeSet<String>, base: String) -> String {
    let mut candidate = base.clone();
    let mut index = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{base}{index}");
        index += 1;
    }
    candidate
}

fn snake(source: &str) -> String {
    let mut result = String::new();
    let mut previous: Option<char> = None;
    for c in source.chars() {
        if c.is_ascii_uppercase() {
            if previous.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit()) {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else if c.is_ascii_alphanumeric() {
            result.push(c);
        } else if !result.ends_with('_') {
            result.push('_');
        }
        previous = Some(c);
    }
    let result = result.trim_matches('_').to_owned();
    if result.starts_with(|c: char| c.is_ascii_digit()) {
        format!("n{result}")
    } else {
        result
    }
}

fn field_ident(json: &str) -> Ident {
    const RAW: [&str; 47] = [
        "as", "break", "const", "continue", "else", "enum", "extern", "false", "fn", "for", "if",
        "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
        "static", "struct", "trait", "true", "type", "unsafe", "use", "where", "while", "async",
        "await", "dyn", "abstract", "become", "box", "do", "final", "macro", "override", "priv",
        "typeof", "unsized", "virtual", "yield", "try",
    ];
    let ident = snake(json);
    assert!(!ident.is_empty(), "field {json:?} has no identifier characters");
    if matches!(ident.as_str(), "self" | "super" | "crate" | "gen") {
        Ident::new(&format!("{ident}_"), Span::call_site())
    } else if RAW.contains(&ident.as_str()) {
        Ident::new_raw(&ident, Span::call_site())
    } else {
        Ident::new(&ident, Span::call_site())
    }
}

fn default_fn(type_name: &str, field: &Field) -> Ident {
    let field = field.ident.to_string();
    format_ident!(
        "default_{}_{}",
        snake(type_name),
        field.trim_start_matches("r#")
    )
}
