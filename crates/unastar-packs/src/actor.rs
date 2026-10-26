//! Actor definition loader (`entities/*.json`, `minecraft:entity`).
//!
//! One loader for build time (vanilla, embedded per D2/D13) and for addon
//! packs at startup (D5). It accepts, normalises and rejects pack documents
//! according to the measured rules below (entities research §4.7). Supported
//! vanilla input normalises to a fixed point. Retained legacy unknown data may
//! still produce diagnostics when loaded again.
//!
//! # Rules
//!
//! | Concern | Rule | Evidence |
//! |---|---|---|
//! | Syntax | JSON with `//` and `/* */` comments and an optional BOM | BDS pack files ([`crate::json`]) |
//! | Shape | `format_version` plus exactly one `minecraft:entity`; other top-level keys are logged | Mojang entity documentation |
//! | No identifier | A file without `description.identifier` defines nothing and is skipped ([`Loaded::Skipped`]) | `npc.json` in the BDS 1.26.51 layers `vanilla_1.16.100` and `vanilla_1.17.20` has only `components`; the layered stack still matches bedrock-samples. Unconfirmed: recorded in `bds-quirks.md` |
//! | `events: null` | Same as no events | 23 vanilla files (bedrock-samples 1.26.50.4 = BDS 1.26.51); recorded in `bds-quirks.md` |
//! | `format_version` upgrade | Before 1.26.10, `minecraft:pushable {is_pushable, is_pushable_by_piston}` (both default `true`) becomes `minecraft:pushable_by_entity {}` when `is_pushable` and `minecraft:pushable_by_block {}` when `is_pushable_by_piston`, in base components and in every group | 25 vanilla files on 1.26.0 still use `minecraft:pushable`; 1.26.10+ files use the pair (Mojang changelog for 1.26.10) |
//! | Filters | Every filter inside a component (recognised by shape: an object with a string `test`, or an object of group keys) is parsed with every accepted spelling and written in canonical form | [`unastar_rules::filter`] |
//! | Events | Parsed and written in canonical form, malformed vanilla nodes tolerated | [`unastar_rules::event`] |
//! | Properties | Typed bool/int/float/enum definitions with retained defaults/ranges. Numeric/bool expressions bind in property-default context; a bad default disables the entire property table without rejecting the actor. Runtime consumers use `Description::active_properties()` | Mojang property metadata and isolated BDS property-scope controls |
//! | Overrides | Whole-file replacement by identifier across a pack stack, see [`resolve_stack`] | Pack layout of the BDS install |
//! | Version ceiling | 1.26.51 accepted, 1.26.52 and 99.0.0 rejected | Crafted BDS 1.26.51 probes |
//! | Unknown components | Warn/retain as inactive through 1.21.80; reject from 1.21.90. Names and patterns come from stable Mojang schemas plus its legacy form index; addon kinds register before load | Version-boundary BDS probes, pinned metadata |
//! | Unknown entity members | Unregistered members reject in strict-schema formats; other schema-declared members (such as `upgrades`), top-level and description extras remain retained | Canonical ActorDocument schema and isolated BDS probes |
//!
//! Component bodies other than their filters are kept verbatim; component
//! kinds parse their own fields when a runtime implementation exists.

use crate::error::{Error, Result};
use crate::json;
use crate::packs::{BASE_GAME_VERSION, Family, PackStack};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use unastar_rules::event::EventNode;
use unastar_rules::filter::{Filter, FilterRegistry};
use unastar_rules::molang::{self, Expr, QueryContext, QueryRegistry};
use unastar_rules::{Diagnostic, Diagnostics, ParseError};

/// Top-level schema key of an actor definition.
pub const ENTITY_KEY: &str = "minecraft:entity";

// Crafted BDS probes: unknown components warn through 1.21.80 and reject the
// definition from 1.21.90. See the recorded fidelity baseline.
const STRICT_ENTITY_SCHEMA_SINCE: [u32; 3] = [1, 21, 90];

/// Parser bindings are assembled before loading addon definitions. Component
/// names/patterns come from the pinned Mojang entity-component schema, not a
/// hand-maintained allow-list. Addon/WASM component kinds may register first.
pub struct ActorParser<'a> {
    filters: &'a FilterRegistry,
    components: BTreeSet<String>,
    patterns: Vec<regex::Regex>,
    entity_fields: BTreeSet<String>,
    max_version: [u32; 3],
}

impl<'a> ActorParser<'a> {
    pub fn from_schema(filters: &'a FilterRegistry, schema: Value) -> Result<Self> {
        Self::from_schemas(filters, [Ok(schema)])
    }

    /// Older schemas retain legacy names still present in shipped definitions.
    /// This is a parser-name registry, not a claim that every kind has runtime
    /// behaviour. New pack names must still resolve to a registered kind.
    pub fn from_schemas(
        filters: &'a FilterRegistry,
        schemas: impl IntoIterator<Item = Result<Value>>,
    ) -> Result<Self> {
        let origin = "actor component schema";
        let mut components = BTreeSet::new();
        let mut pattern_names = BTreeSet::new();
        let mut entity_fields = BTreeSet::new();
        let mut max_version = None;
        for schema in schemas {
            let Value::Object(mut schema) = schema? else {
                return Err(Error::data(origin, "expected an object"));
            };
            let Some(Value::String(version)) = schema.remove("x-format-version") else {
                return Err(Error::data(origin, "missing x-format-version"));
            };
            let version = parse_version(&version).map_err(|e| Error::data(origin, e))?;
            max_version = Some(max_version.map_or(version, |v: [u32; 3]| v.max(version)));
            let Some(Value::Object(properties)) = schema.remove("properties") else {
                return Err(Error::data(origin, "missing component properties"));
            };
            components.extend(properties.into_iter().map(|(name, _)| name));
            match schema.remove("patternProperties") {
                None => {}
                Some(Value::Object(patterns)) => {
                    pattern_names.extend(patterns.into_iter().map(|(name, _)| name));
                }
                Some(_) => return Err(Error::data(origin, "patternProperties is not an object")),
            }
            if let Some(fields) = schema.remove("entity_fields") {
                entity_fields.extend(
                    serde_json::from_value::<BTreeSet<String>>(fields)
                        .map_err(|e| Error::data(origin, format!("entity_fields: {e}")))?,
                );
            }
        }
        let max_version = max_version.ok_or_else(|| Error::data(origin, "no schemas supplied"))?;
        let patterns = pattern_names
            .into_iter()
            .map(|pattern| {
                regex::Regex::new(&pattern).map_err(|e| Error::data(origin, e.to_string()))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            filters,
            components,
            patterns,
            entity_fields,
            max_version,
        })
    }

    pub fn register_component(&mut self, name: impl Into<String>) {
        self.components.insert(name.into());
    }

    /// Merge the legacy Mojang form index (some internal vanilla kinds are
    /// still absent from the newer public JSON-schema index).
    pub fn register_form(&mut self, form: Value) -> Result<()> {
        #[derive(serde::Deserialize)]
        struct Form {
            fields: Vec<Field>,
        }
        #[derive(serde::Deserialize)]
        struct Field {
            id: String,
        }
        let form: Form = serde_json::from_value(form)
            .map_err(|e| Error::data("legacy actor component form", e.to_string()))?;
        self.components
            .extend(form.fields.into_iter().map(|field| field.id));
        Ok(())
    }

    /// Include schema-declared root members that are retained rather than
    /// interpreted by this envelope loader (for example `upgrades`).
    pub fn register_document_schema(&mut self, document: Value) -> Result<()> {
        let Value::Object(mut document) = document else {
            return Err(Error::data("actor document schema", "expected an object"));
        };
        let Some(Value::Object(properties)) = document.remove("properties") else {
            return Err(Error::data("actor document schema", "missing properties"));
        };
        self.entity_fields
            .extend(properties.into_iter().map(|(name, _)| name));
        Ok(())
    }

    pub fn schema_version(&self) -> [u32; 3] {
        self.max_version
    }

    pub fn component_count(&self) -> usize {
        self.components.len()
    }

    /// Minimal build artefact: retain the registry, not descriptions/examples.
    pub fn to_schema(&self) -> Value {
        serde_json::json!({
            "x-format-version": format!("{}.{}.{}", self.max_version[0], self.max_version[1], self.max_version[2]),
            "properties": self.components.iter().map(|name| (name.clone(), Value::Null)).collect::<Map<_, _>>(),
            "patternProperties": self.patterns.iter().map(|pattern| (pattern.as_str().to_owned(), Value::Null)).collect::<Map<_, _>>(),
            "entity_fields": self.entity_fields,
        })
    }

    fn validate_components(
        &self,
        set: &BTreeMap<String, Value>,
        path: &str,
        version: [u32; 3],
        diagnostics: &mut Diagnostics,
        ignored: &mut BTreeSet<String>,
    ) -> Result<()> {
        for name in set.keys() {
            if !self.components.contains(name) && !self.patterns.iter().any(|p| p.is_match(name)) {
                if version >= STRICT_ENTITY_SCHEMA_SINCE {
                    return Err(Error::data(
                        path,
                        format!("component {name:?} is not registered"),
                    ));
                }
                diagnostics.push(
                    format!("{path}/{name}"),
                    "unregistered legacy component ignored",
                );
                ignored.insert(name.clone());
            }
        }
        Ok(())
    }
}

/// Stable versioned registry schemas in the pinned bedrock-samples metadata.
/// Beta schemas are deliberately excluded from the vanilla parser.
pub fn component_schema_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root).map_err(|e| Error::io(root, e))? {
        let entry = entry.map_err(|e| Error::io(root, e))?;
        if !entry
            .file_type()
            .map_err(|e| Error::io(entry.path(), e))?
            .is_dir()
            || entry.file_name() == "beta"
        {
            continue;
        }
        files.push(entry.path().join("Entity component definitions.json"));
    }
    files.sort();
    if files.is_empty() {
        return Err(Error::data(
            root.display().to_string(),
            "no stable entity component schemas",
        ));
    }
    Ok(files)
}

/// A normalised actor definition.
#[derive(Debug, Clone, PartialEq)]
pub struct ActorDefinition {
    /// `format_version` as written in the source file.
    pub format_version: String,
    pub description: Description,
    /// Base components (name → body, filters canonical).
    pub components: BTreeMap<String, Value>,
    /// Named component groups.
    pub component_groups: BTreeMap<String, BTreeMap<String, Value>>,
    /// Legacy unknown bodies survive round-trips but must not become active
    /// runtime components. Newer formats reject them instead.
    pub ignored_components: BTreeSet<String>,
    pub events: BTreeMap<String, EventNode>,
    /// Envelope and unmodelled entity fields retained for lossless round-trips.
    pub document_other: BTreeMap<String, Value>,
    pub entity_other: BTreeMap<String, Value>,
}

/// `description`.
#[derive(Debug, Clone, PartialEq)]
pub struct Description {
    pub identifier: String,
    pub is_spawnable: bool,
    pub is_summonable: bool,
    pub is_experimental: bool,
    pub spawn_category: Option<String>,
    pub runtime_identifier: Option<String>,
    pub properties: BTreeMap<String, PropertyDef>,
    /// A bad default leaves the actor loadable but disables its whole property
    /// table, including otherwise valid siblings (crafted BDS controls).
    pub properties_usable: bool,
    /// Keys kept verbatim for addons and clients (`aliases`, `animations`, `scripts`, ...).
    pub other: BTreeMap<String, Value>,
}

impl Description {
    /// Runtime consumers use this table, not the retained source properties.
    pub fn active_properties(&self) -> Option<&BTreeMap<String, PropertyDef>> {
        self.properties_usable.then_some(&self.properties)
    }
}

/// One entity property (`description.properties`).
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyDef {
    pub kind: PropertyKind,
    /// `default`: a literal, or a Molang string for numeric/boolean properties.
    pub default: Option<Value>,
    /// Parsed and context-bound default, evaluated at actor creation/load.
    /// Enum defaults are literal identifiers rather than Molang expressions.
    pub default_expression: Option<Expr>,
    pub client_sync: bool,
    pub other: BTreeMap<String, Value>,
}

/// Property type.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyKind {
    Bool,
    Int { min: i64, max: i64 },
    Float { min: f64, max: f64 },
    Enum { values: Vec<String> },
}

/// Result of loading one file.
#[derive(Debug, Clone, PartialEq)]
pub enum Loaded {
    Definition {
        definition: Box<ActorDefinition>,
        diagnostics: Vec<Diagnostic>,
    },
    /// The file defines nothing BDS would register (see the module rules).
    Skipped { reason: String },
}

/// Load one entity file. `origin` names the file in errors and diagnostics.
pub fn load(origin: &str, bytes: &[u8], parser: &ActorParser<'_>) -> Result<Loaded> {
    let value = json::parse(origin, bytes)?;
    load_value(origin, &value, parser)
}

/// Load an already-parsed document.
pub fn load_value(origin: &str, value: &Value, parser: &ActorParser<'_>) -> Result<Loaded> {
    let registry = parser.filters;
    let err = |e: ParseError| Error::data(origin, e.to_string());
    let mut diags = Diagnostics::default();
    let Value::Object(doc) = value else {
        return Err(Error::data(origin, "document is not an object"));
    };
    let format_version = match doc.get("format_version") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => {
            return Err(Error::data(
                origin,
                format!("format_version must be a string, got {other}"),
            ));
        }
        None => return Err(Error::data(origin, "missing format_version")),
    };
    for key in doc.keys() {
        if key != "format_version" && key != ENTITY_KEY {
            diags.push(format!("/{key}"), "unknown top-level key ignored");
        }
    }
    let Some(Value::Object(entity)) = doc.get(ENTITY_KEY) else {
        return Err(Error::data(
            origin,
            format!("expected a {ENTITY_KEY} object"),
        ));
    };
    let Some(identifier) = entity
        .get("description")
        .and_then(|d| d.get("identifier"))
        .and_then(Value::as_str)
    else {
        return Ok(Loaded::Skipped {
            reason: "no description.identifier".into(),
        });
    };
    let description =
        parse_description(identifier, &entity["description"], &mut diags).map_err(err)?;
    let version = parse_version(&format_version).map_err(|m| Error::data(origin, m))?;
    if version > BASE_GAME_VERSION {
        return Err(Error::data(
            origin,
            format!("unsupported format_version {format_version}"),
        ));
    }

    let mut components = parse_component_set(
        entity.get("components"),
        "/components",
        registry,
        &mut diags,
    )
    .map_err(err)?
    .unwrap_or_default();
    let mut component_groups = BTreeMap::new();
    match entity.get("component_groups") {
        None => {}
        Some(Value::Object(groups)) => {
            for (name, body) in groups {
                let path = format!("/component_groups/{name}");
                let set = parse_component_set(Some(body), &path, registry, &mut diags)
                    .map_err(err)?
                    .unwrap_or_default();
                component_groups.insert(name.clone(), set);
            }
        }
        Some(other) => {
            return Err(Error::data(
                origin,
                format!("component_groups must be an object, got {other}"),
            ));
        }
    }
    if version < [1, 26, 10] {
        upgrade_pushable(&mut components);
        component_groups.values_mut().for_each(upgrade_pushable);
    }
    let mut ignored_components = BTreeSet::new();
    parser.validate_components(
        &components,
        &format!("{origin}/components"),
        version,
        &mut diags,
        &mut ignored_components,
    )?;
    for (name, set) in &component_groups {
        parser.validate_components(
            set,
            &format!("{origin}/component_groups/{name}"),
            version,
            &mut diags,
            &mut ignored_components,
        )?;
    }

    let mut events = BTreeMap::new();
    match entity.get("events") {
        None | Some(Value::Null) => {}
        Some(Value::Object(map)) => {
            for (name, body) in map {
                let node = EventNode::parse(body, registry, &format!("/events/{name}"), &mut diags)
                    .map_err(err)?;
                events.insert(name.clone(), node);
            }
        }
        Some(other) => {
            return Err(Error::data(
                origin,
                format!("events must be an object, got {other}"),
            ));
        }
    }
    for key in entity.keys() {
        if !matches!(
            key.as_str(),
            "description" | "components" | "component_groups" | "events"
        ) && !parser.entity_fields.contains(key)
        {
            if version >= STRICT_ENTITY_SCHEMA_SINCE {
                return Err(Error::data(origin, format!("unknown entity key {key:?}")));
            }
            diags.push(format!("/{ENTITY_KEY}/{key}"), "unknown key ignored");
        }
    }
    Ok(Loaded::Definition {
        definition: Box::new(ActorDefinition {
            format_version,
            description,
            components,
            component_groups,
            ignored_components,
            events,
            document_other: doc
                .iter()
                .filter(|(k, _)| k.as_str() != "format_version" && k.as_str() != ENTITY_KEY)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            entity_other: entity
                .iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.as_str(),
                        "description" | "components" | "component_groups" | "events"
                    )
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }),
        diagnostics: diags.items,
    })
}

fn parse_version(text: &str) -> std::result::Result<[u32; 3], String> {
    let mut out = [0u32; 3];
    let parts: Vec<&str> = text.split('.').collect();
    if parts.is_empty() || parts.len() > 3 {
        return Err(format!("malformed format_version {text:?}"));
    }
    for (slot, part) in out.iter_mut().zip(&parts) {
        *slot = part
            .parse()
            .map_err(|_| format!("malformed format_version {text:?}"))?;
    }
    Ok(out)
}

fn parse_description(
    identifier: &str,
    value: &Value,
    diags: &mut Diagnostics,
) -> std::result::Result<Description, ParseError> {
    let Value::Object(map) = value else {
        return Err(ParseError::new(
            "/description",
            "description must be an object",
        ));
    };
    let flag = |key: &str| -> std::result::Result<bool, ParseError> {
        match map.get(key) {
            None => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(other) => Err(ParseError::new(
                format!("/description/{key}"),
                format!("expected a boolean, got {other}"),
            )),
        }
    };
    let text = |key: &str| -> std::result::Result<Option<String>, ParseError> {
        match map.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(other) => Err(ParseError::new(
                format!("/description/{key}"),
                format!("expected a string, got {other}"),
            )),
        }
    };
    let mut properties = BTreeMap::new();
    let mut properties_usable = true;
    match map.get("properties") {
        None => {}
        Some(Value::Object(props)) => {
            for (name, body) in props {
                let path = format!("/description/properties/{name}");
                let mut property = parse_property(&path, body, diags)?;
                if !matches!(&property.kind, PropertyKind::Enum { .. })
                    && let Some(Value::String(source)) = &property.default
                {
                    let compiled = molang::parse(source).map_err(|e| e.to_string()).and_then(
                        |mut expression| {
                            expression
                                .bind(QueryRegistry::vanilla(), QueryContext::PropertyDefault)?;
                            Ok(expression)
                        },
                    );
                    match compiled {
                        Ok(expression) => property.default_expression = Some(expression),
                        Err(error) => {
                            properties_usable = false;
                            diags.push(format!("{path}/default"), error);
                        }
                    }
                }
                properties.insert(name.clone(), property);
            }
        }
        Some(other) => {
            return Err(ParseError::new(
                "/description/properties",
                format!("expected an object, got {other}"),
            ));
        }
    }
    let known = [
        "identifier",
        "is_spawnable",
        "is_summonable",
        "is_experimental",
        "spawn_category",
        "runtime_identifier",
        "properties",
    ];
    let other = map
        .iter()
        .filter(|(k, _)| !known.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Ok(Description {
        identifier: identifier.to_owned(),
        is_spawnable: flag("is_spawnable")?,
        is_summonable: flag("is_summonable")?,
        is_experimental: flag("is_experimental")?,
        spawn_category: text("spawn_category")?,
        runtime_identifier: text("runtime_identifier")?,
        properties,
        properties_usable,
        other,
    })
}

fn parse_property(
    path: &str,
    value: &Value,
    diags: &mut Diagnostics,
) -> std::result::Result<PropertyDef, ParseError> {
    let Value::Object(map) = value else {
        return Err(ParseError::new(path, "property must be an object"));
    };
    let range = || -> std::result::Result<(f64, f64), ParseError> {
        match map
            .get("range")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
        {
            Some([lo, hi]) => match (lo.as_f64(), hi.as_f64()) {
                (Some(lo), Some(hi)) => Ok((lo, hi)),
                _ => Err(ParseError::new(
                    format!("{path}/range"),
                    "range bounds must be numbers",
                )),
            },
            _ => Err(ParseError::new(
                format!("{path}/range"),
                "int and float properties need a two-element range",
            )),
        }
    };
    let kind = match map.get("type").and_then(Value::as_str) {
        Some("bool") => PropertyKind::Bool,
        Some("int") => {
            let (min, max) = range()?;
            PropertyKind::Int {
                min: min as i64,
                max: max as i64,
            }
        }
        Some("float") => {
            let (min, max) = range()?;
            PropertyKind::Float { min, max }
        }
        Some("enum") => {
            let values = map
                .get("values")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    ParseError::new(format!("{path}/values"), "enum properties need values")
                })?
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    ParseError::new(format!("{path}/values"), "enum values must be strings")
                })?;
            PropertyKind::Enum { values }
        }
        other => {
            return Err(ParseError::new(
                format!("{path}/type"),
                format!("unknown property type {other:?}"),
            ));
        }
    };
    let client_sync = match map.get("client_sync") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(other) => {
            return Err(ParseError::new(
                format!("{path}/client_sync"),
                format!("expected a boolean, got {other}"),
            ));
        }
    };
    for key in map.keys() {
        if !matches!(
            key.as_str(),
            "type" | "range" | "values" | "default" | "client_sync"
        ) {
            diags.push(format!("{path}/{key}"), "unknown property key ignored");
        }
    }
    Ok(PropertyDef {
        kind,
        default: map.get("default").cloned(),
        default_expression: None,
        client_sync,
        other: map
            .iter()
            .filter(|(k, _)| {
                !matches!(
                    k.as_str(),
                    "type" | "range" | "values" | "default" | "client_sync"
                )
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    })
}

fn parse_component_set(
    value: Option<&Value>,
    path: &str,
    registry: &FilterRegistry,
    diags: &mut Diagnostics,
) -> std::result::Result<Option<BTreeMap<String, Value>>, ParseError> {
    match value {
        None => Ok(None),
        Some(Value::Object(map)) => {
            let mut out = BTreeMap::new();
            for (name, body) in map {
                let mut body = body.clone();
                normalise_filters(&mut body, &format!("{path}/{name}"), registry, diags)?;
                out.insert(name.clone(), body);
            }
            Ok(Some(out))
        }
        Some(other) => Err(ParseError::new(
            path,
            format!("component set must be an object, got {other}"),
        )),
    }
}

const GROUP_KEYS: [&str; 7] = ["all_of", "all", "AND", "any_of", "any", "OR", "none_of"];

/// A filter by shape: a string `test`, or an object whose keys are all
/// filter group keys (`NOT` included).
fn is_filter_shaped(map: &Map<String, Value>) -> bool {
    if map.get("test").is_some_and(Value::is_string) {
        return true;
    }
    !map.is_empty()
        && map
            .keys()
            .all(|k| GROUP_KEYS.contains(&k.as_str()) || k == "NOT")
}

fn normalise_filters(
    value: &mut Value,
    path: &str,
    registry: &FilterRegistry,
    diags: &mut Diagnostics,
) -> std::result::Result<(), ParseError> {
    match value {
        Value::Object(map) if is_filter_shaped(map) => {
            *value = Filter::parse(value, registry, path, diags)?.to_json();
        }
        Value::Object(map) => {
            for (key, member) in map.iter_mut() {
                normalise_filters(member, &format!("{path}/{key}"), registry, diags)?;
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter_mut().enumerate() {
                normalise_filters(item, &format!("{path}/{i}"), registry, diags)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn upgrade_pushable(set: &mut BTreeMap<String, Value>) {
    let Some(old) = set.remove("minecraft:pushable") else {
        return;
    };
    let flag = |key: &str| old.get(key).and_then(Value::as_bool).unwrap_or(true);
    if flag("is_pushable") {
        set.entry("minecraft:pushable_by_entity".into())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    if flag("is_pushable_by_piston") {
        set.entry("minecraft:pushable_by_block".into())
            .or_insert_with(|| Value::Object(Map::new()));
    }
}

impl ActorDefinition {
    pub fn identifier(&self) -> &str {
        &self.description.identifier
    }

    /// The normalised document.
    pub fn to_json(&self) -> Value {
        let mut description = Map::new();
        let d = &self.description;
        description.insert("identifier".into(), Value::String(d.identifier.clone()));
        description.insert("is_spawnable".into(), Value::Bool(d.is_spawnable));
        description.insert("is_summonable".into(), Value::Bool(d.is_summonable));
        description.insert("is_experimental".into(), Value::Bool(d.is_experimental));
        if let Some(c) = &d.spawn_category {
            description.insert("spawn_category".into(), Value::String(c.clone()));
        }
        if let Some(r) = &d.runtime_identifier {
            description.insert("runtime_identifier".into(), Value::String(r.clone()));
        }
        if !d.properties.is_empty() {
            let props = d
                .properties
                .iter()
                .map(|(k, p)| (k.clone(), p.to_json()))
                .collect();
            description.insert("properties".into(), Value::Object(props));
        }
        description.extend(d.other.iter().map(|(k, v)| (k.clone(), v.clone())));

        let mut entity = Map::new();
        entity.insert("description".into(), Value::Object(description));
        entity.insert(
            "components".into(),
            Value::Object(self.components.clone().into_iter().collect()),
        );
        if !self.component_groups.is_empty() {
            let groups = self
                .component_groups
                .iter()
                .map(|(k, set)| (k.clone(), Value::Object(set.clone().into_iter().collect())))
                .collect();
            entity.insert("component_groups".into(), Value::Object(groups));
        }
        if !self.events.is_empty() {
            let events = self
                .events
                .iter()
                .map(|(k, e)| (k.clone(), e.to_json()))
                .collect();
            entity.insert("events".into(), Value::Object(events));
        }
        entity.extend(
            self.entity_other
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        let mut doc = Map::new();
        doc.insert(
            "format_version".into(),
            Value::String(self.format_version.clone()),
        );
        doc.insert(ENTITY_KEY.into(), Value::Object(entity));
        doc.extend(
            self.document_other
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        Value::Object(doc)
    }

    /// Every filter in components, groups and events (for coverage checks).
    pub fn filters(&self, registry: &FilterRegistry) -> Vec<Filter> {
        let mut out = Vec::new();
        let mut diags = Diagnostics::default();
        let sets = std::iter::once(&self.components).chain(self.component_groups.values());
        for body in sets.flat_map(|s| s.values()) {
            collect_filters(body, registry, &mut diags, &mut out);
        }
        for event in self.events.values() {
            event.walk(&mut |node| {
                out.extend(node.filters.clone());
                if let Some(t) = &node.trigger {
                    out.extend(t.filters.clone());
                }
            });
        }
        out
    }
}

fn collect_filters(
    value: &Value,
    registry: &FilterRegistry,
    diags: &mut Diagnostics,
    out: &mut Vec<Filter>,
) {
    match value {
        Value::Object(map) if is_filter_shaped(map) => {
            if let Ok(f) = Filter::parse(value, registry, "", diags) {
                out.push(f);
            }
        }
        Value::Object(map) => map
            .values()
            .for_each(|v| collect_filters(v, registry, diags, out)),
        Value::Array(items) => items
            .iter()
            .for_each(|v| collect_filters(v, registry, diags, out)),
        _ => {}
    }
}

impl PropertyDef {
    fn to_json(&self) -> Value {
        let mut map = Map::new();
        match &self.kind {
            PropertyKind::Bool => {
                map.insert("type".into(), "bool".into());
            }
            PropertyKind::Int { min, max } => {
                map.insert("type".into(), "int".into());
                map.insert(
                    "range".into(),
                    Value::Array(vec![(*min).into(), (*max).into()]),
                );
            }
            PropertyKind::Float { min, max } => {
                map.insert("type".into(), "float".into());
                map.insert("range".into(), Value::Array(vec![float(*min), float(*max)]));
            }
            PropertyKind::Enum { values } => {
                map.insert("type".into(), "enum".into());
                map.insert(
                    "values".into(),
                    Value::Array(values.iter().cloned().map(Value::String).collect()),
                );
            }
        }
        if let Some(default) = &self.default {
            map.insert("default".into(), default.clone());
        }
        map.insert("client_sync".into(), Value::Bool(self.client_sync));
        map.extend(self.other.iter().map(|(k, v)| (k.clone(), v.clone())));
        Value::Object(map)
    }
}

fn float(v: f64) -> Value {
    serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
}

/// Resolve the `entities` family over a stack: the last layer defining an
/// identifier replaces the whole definition; two files with the same
/// identifier in one layer are an error.
pub fn resolve_stack(stack: &PackStack, parser: &ActorParser<'_>) -> Result<Resolved> {
    let mut out = Resolved::default();
    let mut layer_seen: BTreeMap<String, String> = BTreeMap::new();
    let mut current_layer = String::new();
    let mut selected = BTreeMap::new();
    for file in stack.files(Family::Entities)? {
        if file.layer != current_layer {
            current_layer = file.layer.clone();
            layer_seen.clear();
        }
        let document = json::parse(&file.origin, &file.bytes)?;
        let identifier = document
            .get(ENTITY_KEY)
            .and_then(|v| v.get("description"))
            .and_then(|v| v.get("identifier"))
            .and_then(Value::as_str);
        let Some(identifier) = identifier else {
            if let Loaded::Skipped { reason } = load_value(&file.origin, &document, parser)? {
                out.skipped.push((file.origin, reason));
            }
            continue;
        };
        if let Some(previous) = layer_seen.insert(identifier.to_owned(), file.origin.clone()) {
            return Err(Error::data(
                file.origin,
                format!("{identifier} already defined in the same layer by {previous}"),
            ));
        }
        selected.insert(identifier.to_owned(), (file.origin, document));
    }
    // Whole-file selection precedes component parsing: an overridden legacy
    // body is not combined with or validated as part of its replacement.
    for (identifier, (origin, document)) in selected {
        if let Loaded::Definition {
            definition,
            diagnostics,
        } = load_value(&origin, &document, parser)?
        {
            out.definitions.insert(
                identifier,
                ResolvedDefinition {
                    source: origin,
                    definition: *definition,
                    diagnostics,
                },
            );
        }
    }
    Ok(out)
}

/// Definitions resolved over a stack.
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    pub definitions: BTreeMap<String, ResolvedDefinition>,
    /// Files that defined nothing, with the reason.
    pub skipped: Vec<(String, String)>,
}

/// The winning copy of one identifier.
#[derive(Debug, Clone)]
pub struct ResolvedDefinition {
    /// `<layer>/entities:<entry>`.
    pub source: String,
    pub definition: ActorDefinition,
    pub diagnostics: Vec<Diagnostic>,
}

/// Difference between two resolved sets (D9: bedrock-samples vs BDS install).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StackDiff {
    pub only_left: Vec<String>,
    pub only_right: Vec<String>,
    /// Identifiers whose normalised definitions differ.
    pub differing: Vec<String>,
}

impl StackDiff {
    pub fn is_empty(&self) -> bool {
        self.only_left.is_empty() && self.only_right.is_empty() && self.differing.is_empty()
    }
}

/// Compare two resolved sets by normalised definition.
pub fn diff(left: &Resolved, right: &Resolved) -> StackDiff {
    let mut out = StackDiff::default();
    for (id, l) in &left.definitions {
        match right.definitions.get(id) {
            None => out.only_left.push(id.clone()),
            Some(r) if r.definition != l.definition => out.differing.push(id.clone()),
            Some(_) => {}
        }
    }
    out.only_right = right
        .definitions
        .keys()
        .filter(|id| !left.definitions.contains_key(*id))
        .cloned()
        .collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(super) fn parser() -> ActorParser<'static> {
        ActorParser::from_schema(
            FilterRegistry::vanilla(),
            json!({
                "x-format-version": "1.26.50",
                "properties": {
                    "minecraft:pushable": null,
                    "minecraft:pushable_by_entity": null,
                    "minecraft:pushable_by_block": null,
                    "minecraft:environment_sensor": null
                },
                "patternProperties": {
                    "^minecraft:memory_behavior.move_to_position\\.[A-Za-z0-9_]{1,32}$": null
                }
            }),
        )
        .unwrap()
    }

    #[test]
    fn rejects_unregistered_components_in_base_and_inactive_groups() {
        for body in [
            json!({"components": {"addon:custom": {"values": [1, 2]}}}),
            json!({"component_groups": {"addon:unused": {"addon:custom": {"values": [1, 2]}}}}),
        ] {
            let mut body = body;
            body["description"] = json!({"identifier": "addon:actor"});
            let document = entity("1.26.50", body);
            let mut parser = parser();
            assert!(matches!(
                load_value("addon.json", &document, &parser),
                Err(Error::Data { .. })
            ));
            parser.register_component("addon:custom");
            let Loaded::Definition { definition, .. } =
                load_value("addon.json", &document, &parser).unwrap()
            else {
                panic!("registered addon definition skipped")
            };
            let normal = definition.to_json();
            for key in ["components", "component_groups"] {
                if let Some(expected) = document[ENTITY_KEY].get(key) {
                    assert_eq!(&normal[ENTITY_KEY][key], expected);
                }
            }
        }
    }

    #[test]
    fn rejects_future_format_and_accepts_registered_component_patterns() {
        let accepted = entity("1.26.51", json!({"description": {"identifier": "t:x"}}));
        let Loaded::Definition { definition, .. } =
            load_value("current.json", &accepted, &parser()).unwrap()
        else {
            panic!("current-format actor skipped")
        };
        assert_eq!(definition.format_version, "1.26.51");
        for version in ["1.26.52", "99.0.0"] {
            let future = entity(version, json!({"description": {"identifier": "t:x"}}));
            assert!(matches!(
                load_value("future.json", &future, &parser()),
                Err(Error::Data { .. })
            ));
        }
        let body = |name: &str| {
            entity(
                "1.26.50",
                json!({
                    "description": {"identifier": "t:x"}, "components": {name: {}}
                }),
            )
        };
        let Loaded::Definition { definition, .. } = load_value(
            "pattern.json",
            &body("minecraft:memory_behavior.move_to_position.target_1"),
            &parser(),
        )
        .unwrap() else {
            panic!("pattern component skipped")
        };
        assert!(
            definition
                .components
                .contains_key("minecraft:memory_behavior.move_to_position.target_1")
        );
        let bad = body("minecraft:memory_behavior.move_to_position.");
        assert!(matches!(
            load_value("pattern.json", &bad, &parser()),
            Err(Error::Data { .. })
        ));
    }

    #[test]
    fn legacy_unknown_components_warn_but_modern_ones_reject() {
        let body = json!({
            "description": {"identifier": "t:x"},
            "components": {"addon:unknown": {"array": [1, 2]}}
        });
        let Loaded::Definition {
            definition,
            diagnostics,
        } = load_value("legacy.json", &entity("1.21.80", body.clone()), &parser()).unwrap()
        else {
            panic!("legacy actor skipped")
        };
        assert!(definition.ignored_components.contains("addon:unknown"));
        assert!(
            diagnostics
                .iter()
                .any(|d| d.path.ends_with("/addon:unknown"))
        );
        assert_eq!(
            definition.to_json()[ENTITY_KEY]["components"],
            body["components"]
        );
        assert!(matches!(
            load_value("modern.json", &entity("1.21.90", body), &parser()),
            Err(Error::Data { .. })
        ));
    }

    #[test]
    fn modern_unknown_entity_member_rejects_but_envelope_and_description_survive() {
        let document = entity(
            "1.26.50",
            json!({
                "description": {"identifier": "t:x"},
                "addon:entity": {"nested": [1, 2]}
            }),
        );
        assert!(matches!(
            load_value("entity.json", &document, &parser()),
            Err(Error::Data { .. })
        ));
        let mut allowed = entity(
            "1.26.50",
            json!({
                "description": {"identifier": "t:x", "addon:description": {"nested": [3, 4]}}
            }),
        );
        allowed["addon:envelope"] = json!({"nested": [5, 6]});
        let Loaded::Definition { definition, .. } =
            load_value("envelope.json", &allowed, &parser()).unwrap()
        else {
            panic!("allowed actor skipped")
        };
        let output = definition.to_json();
        assert_eq!(output["addon:envelope"], allowed["addon:envelope"]);
        assert_eq!(
            output[ENTITY_KEY]["description"]["addon:description"],
            allowed[ENTITY_KEY]["description"]["addon:description"]
        );
    }

    fn load_ok(v: Value) -> (ActorDefinition, Vec<Diagnostic>) {
        match load_value("t.json", &v, &parser()).unwrap() {
            Loaded::Definition {
                definition,
                diagnostics,
            } => (*definition, diagnostics),
            Loaded::Skipped { reason } => panic!("skipped: {reason}"),
        }
    }

    fn entity(format_version: &str, body: Value) -> Value {
        json!({"format_version": format_version, "minecraft:entity": body})
    }

    #[test]
    fn normalised_form_is_a_fixed_point() {
        let (def, diags) = load_ok(entity(
            "1.26.0",
            json!({
                "description": {"identifier": "t:x", "is_spawnable": true, "properties": {
                    "t:p": {"type": "enum", "values": ["a", "b"], "default": "a", "client_sync": true}
                }},
                "components": {
                    "minecraft:pushable": {"is_pushable": true, "is_pushable_by_piston": false},
                    "minecraft:environment_sensor": {"triggers": [{"filters": {"AND": [{"test": "is_family", "subject": 1, "operator": 1, "value": "player"}]}, "event": "e"}]}
                },
                "events": {"e": {"remove": {"t:g": {}}, "trigger": "f"}}
            }),
        ));
        assert_eq!(diags.len(), 1, "{diags:?}");
        let normal = def.to_json();
        let (again, again_diags) = load_ok(normal.clone());
        assert!(again_diags.is_empty(), "{again_diags:?}");
        assert_eq!(again, def);
        assert_eq!(again.to_json(), normal);
        assert_eq!(
            normal["minecraft:entity"]["components"]["minecraft:environment_sensor"]["triggers"][0]
                ["filters"],
            json!({"all_of": [{"test": "is_family", "subject": "other", "operator": "!=", "value": "player"}]})
        );
    }

    #[test]
    fn pushable_is_upgraded_before_1_26_10_only() {
        let body = |v: Value| json!({"description": {"identifier": "t:x"}, "components": {"minecraft:pushable": v}});
        let (old, _) = load_ok(entity(
            "1.26.0",
            body(json!({"is_pushable": false, "is_pushable_by_piston": true})),
        ));
        assert!(!old.components.contains_key("minecraft:pushable"));
        assert!(!old.components.contains_key("minecraft:pushable_by_entity"));
        assert!(old.components.contains_key("minecraft:pushable_by_block"));
        let (defaults, _) = load_ok(entity("1.21.90", body(json!({}))));
        assert!(
            defaults
                .components
                .contains_key("minecraft:pushable_by_entity")
        );
        assert!(
            defaults
                .components
                .contains_key("minecraft:pushable_by_block")
        );
        let (new, _) = load_ok(entity("1.26.10", body(json!({}))));
        assert!(new.components.contains_key("minecraft:pushable"));
    }

    #[test]
    fn invalid_property_default_diagnoses_without_rejecting_actor() {
        let (definition, diagnostics) = load_ok(entity(
            "1.26.50",
            json!({
                "description": {"identifier": "t:x", "properties": {
                    "t:bad": {"type": "bool", "default": "query.is_baby"},
                    "t:n": {"type": "int", "range": [-10, 10], "default": 3}
                }}
            }),
        ));
        assert_eq!(definition.description.identifier, "t:x");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.path.ends_with("/t:bad/default"))
        );
        assert!(definition.description.active_properties().is_none());
        assert_eq!(
            definition.to_json()[ENTITY_KEY]["description"]["properties"]["t:n"]["default"],
            3
        );
    }

    #[test]
    fn enum_defaults_are_literal_even_when_they_look_like_queries() {
        let (definition, _) = load_ok(entity(
            "1.26.50",
            json!({
                "description": {"identifier": "t:x", "properties": {
                    "t:mode": {"type": "enum", "values": ["query.is_baby", "plain"], "default": "query.is_baby"}
                }}
            }),
        ));
        let property = &definition.description.active_properties().unwrap()["t:mode"];
        assert_eq!(property.default.as_ref().unwrap(), "query.is_baby");
        assert!(property.default_expression.is_none());
    }

    #[test]
    fn missing_identifier_and_null_events() {
        let skipped = load_value(
            "npc.json",
            &entity("1.16.100", json!({"components": {}})),
            &parser(),
        )
        .unwrap();
        assert!(matches!(skipped, Loaded::Skipped { .. }));
        let (def, diags) = load_ok(entity(
            "1.26.0",
            json!({"description": {"identifier": "t:x"}, "components": {}, "events": null}),
        ));
        assert!(def.events.is_empty() && diags.is_empty());
    }

    #[test]
    fn rejects_malformed_documents() {
        for bad in [
            json!({"minecraft:entity": {"description": {"identifier": "t:x"}}}),
            entity(
                "1.26.0",
                json!({"description": {"identifier": "t:x", "is_spawnable": "yes"}}),
            ),
            entity(
                "1.26.0",
                json!({"description": {"identifier": "t:x", "properties": {"p": {"type": "int"}}}}),
            ),
            entity("one.two", json!({"description": {"identifier": "t:x"}})),
        ] {
            assert!(load_value("t.json", &bad, &parser()).is_err(), "{bad}");
        }
    }
}

#[cfg(test)]
mod preservation_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_envelope_and_property_fields_survive_roundtrip() {
        let input = json!({
            "format_version": "1.26.50",
            "addon:envelope": {"values": [1, 2]},
            "minecraft:entity": {
                "description": {
                    "identifier": "addon:actor",
                    "scripts": {"initialize": ["variable.x = 1;"]},
                    "properties": {"addon:p": {"type": "bool", "default": true, "addon:metadata": [3, 4]}}
                },
                "components": {"addon:component": {"array": [{"x": 1}, {"x": 2}]}}
            }
        });
        let mut parser = tests::parser();
        parser.register_component("addon:component");
        let Loaded::Definition { definition, .. } =
            load_value("addon.json", &input, &parser).unwrap()
        else {
            panic!("actor skipped")
        };
        let output = definition.to_json();
        assert_eq!(output["addon:envelope"], input["addon:envelope"]);
        assert_eq!(
            output[ENTITY_KEY]["description"]["scripts"],
            input[ENTITY_KEY]["description"]["scripts"]
        );
        assert_eq!(
            output[ENTITY_KEY]["description"]["properties"]["addon:p"]["addon:metadata"],
            json!([3, 4])
        );
        assert_eq!(
            output[ENTITY_KEY]["components"],
            input[ENTITY_KEY]["components"]
        );
        let Loaded::Definition {
            definition: again, ..
        } = load_value("normalised.json", &output, &parser).unwrap()
        else {
            panic!("normalised actor skipped")
        };
        assert_eq!(again, definition);
    }
}
