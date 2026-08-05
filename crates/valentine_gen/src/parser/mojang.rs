//! Frontend for Mojang's `bedrock-protocol-docs/json` schemas.
//!
//! Mojang's files are JSON Schema documents rather than the compact
//! PrismarineJS protocol graph.  This module resolves their local and external
//! references, applies the checked-in correction layer, and lowers the result
//! to the shared Valentine IR.  The generator after this point is intentionally
//! source-agnostic.

use super::ParseResult;
use crate::ir::{Container, Field, Packet, Primitive, Type, UnionVariant};
use crate::overrides;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    pub minecraft_version: String,
    pub protocol_version: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Target {
    file: String,
    pointer: String,
}

struct Lowerer {
    documents: HashMap<String, Value>,
    types: HashMap<String, Type>,
    target_names: HashMap<Target, String>,
    used_names: HashMap<String, Target>,
    building: HashSet<Target>,
    anonymous_counter: usize,
}

/// Mojang's definition IDs are global hashes, not file-local symbols.  The
/// `CompoundTag` hash is therefore referenced by many packet documents even
/// though no document owns a matching `definitions` entry.  Valentine has one
/// NBT wire primitive today; use it for this builtin rather than treating the
/// globally shared hash as a dangling external reference.
const MOJANG_BUILTIN_COMPOUND_TAG_ID: &str = "3172631924";

pub fn parse(
    source_root: &Path,
    override_dir: &Path,
) -> Result<ParseResult, Box<dyn std::error::Error>> {
    let mut documents = load_documents(source_root)?;
    if documents.is_empty() {
        return Err(format!(
            "no Mojang JSON schemas found below {}",
            source_root.display()
        )
        .into());
    }
    overrides::apply(&mut documents, override_dir)?;

    let mut packet_documents = documents
        .iter()
        .filter_map(|(file, document)| packet_metadata(document).map(|(id, _)| (file.clone(), id)))
        .collect::<Vec<_>>();
    packet_documents.sort_by(|(left_file, left_id), (right_file, right_id)| {
        left_id
            .cmp(right_id)
            .then_with(|| left_file.cmp(right_file))
    });

    let mut lowerer = Lowerer {
        documents,
        types: HashMap::new(),
        target_names: HashMap::new(),
        used_names: HashMap::new(),
        building: HashSet::new(),
        anonymous_counter: 0,
    };

    // Materialize non-packet roots as named definitions. Packet roots are
    // emitted in the packet pass so their structs remain version-local, just
    // like the existing Prismarine frontend.
    let root_targets = lowerer
        .documents
        .iter()
        .filter(|(_, document)| packet_metadata(document).is_none())
        .filter(|(_, document)| document.get("type").is_some() || document.get("oneOf").is_some())
        .map(|(file, document)| {
            let name = document
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| file.trim_end_matches(".json").to_owned());
            (
                Target {
                    file: file.clone(),
                    pointer: "#".to_string(),
                },
                name,
            )
        })
        .collect::<Vec<_>>();
    let mut root_targets = root_targets;
    root_targets.sort_by(|(left, _), (right, _)| {
        left.file
            .cmp(&right.file)
            .then_with(|| left.pointer.cmp(&right.pointer))
    });
    for (target, name) in root_targets {
        lowerer.ensure_named(target, &name)?;
    }

    let mut packets = Vec::new();
    for (file, id) in packet_documents {
        let document = lowerer
            .documents
            .get(&file)
            .cloned()
            .ok_or_else(|| format!("packet document disappeared: {file}"))?;
        let title = document
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| file.trim_end_matches(".json").to_owned());
        let packet_name = packet_ir_name(&title);
        let packet_type = lowerer.lower_schema(&document, &title, &file)?;
        let mut body = match packet_type {
            Type::Container(container) => container,
            Type::Reference(name) => match lowerer.types.get(&name) {
                Some(Type::Container(container)) => container.clone(),
                Some(other) => {
                    return Err(format!(
                        "Mojang packet {title} in {file} resolves to non-container type {other:?}"
                    )
                    .into());
                }
                None => {
                    return Err(format!(
                        "Mojang packet {title} in {file} resolves to missing type {name}"
                    )
                    .into());
                }
            },
            other => {
                return Err(format!(
                    "Mojang packet {title} in {file} has non-container body {other:?}"
                )
                .into());
            }
        };
        body.name.clone_from(&packet_name);
        packets.push(Packet {
            id,
            name: packet_name,
            body,
        });
    }

    packets.sort_by_key(|packet| packet.id);
    if packets.is_empty() {
        return Err("Mojang schemas contained no cereal:packet documents".into());
    }

    debug!(
        packets = packets.len(),
        types = lowerer.types.len(),
        "Parsed Mojang protocol schemas"
    );
    Ok(ParseResult {
        packets,
        types: lowerer.types,
    })
}

pub fn discover_versions(
    source_root: &Path,
) -> Result<Vec<VersionInfo>, Box<dyn std::error::Error>> {
    let documents = load_documents(source_root)?;
    let mut versions = HashMap::<String, i32>::new();
    for document in documents.values() {
        let Some(version) = document.get("x-minecraft-version").and_then(Value::as_str) else {
            continue;
        };
        let Some(protocol) = document.get("x-protocol-version").and_then(value_as_i64) else {
            continue;
        };
        // Mojang may publish the release corpus with a beta metadata suffix
        // (for example 1.26.40-beta.0) even when the protocol pin is the final
        // 1.26.40 network revision. Generated Rust identifiers and crate names
        // use the numeric release triplet.
        let release_version = version.split('-').next().unwrap_or(version);
        versions.insert(release_version.to_string(), protocol as i32);
    }
    let mut result = versions
        .into_iter()
        .map(|(minecraft_version, protocol_version)| VersionInfo {
            minecraft_version,
            protocol_version,
        })
        .collect::<Vec<_>>();
    result.sort_by(|a, b| {
        version_parts(&a.minecraft_version).cmp(&version_parts(&b.minecraft_version))
    });
    Ok(result)
}

fn load_documents(
    source_root: &Path,
) -> Result<HashMap<String, Value>, Box<dyn std::error::Error>> {
    let json_root = if source_root.file_name().is_some_and(|name| name == "json") {
        source_root.to_path_buf()
    } else {
        source_root.join("json")
    };
    let mut paths = fs::read_dir(&json_root)
        .map_err(|error| {
            format!(
                "failed to read Mojang schema directory {}: {error}",
                json_root.display()
            )
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<PathBuf>>();
    paths.sort();

    let mut documents = HashMap::new();
    for path in paths {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("invalid Mojang schema filename: {}", path.display()))?
            .to_string();
        let contents = fs::read_to_string(&path)?;
        let document = serde_json::from_str::<Value>(&contents)
            .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
        documents.insert(file_name, document);
    }
    Ok(documents)
}

impl Lowerer {
    fn ensure_named(
        &mut self,
        target: Target,
        hint: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        if let Some(name) = self.target_names.get(&target) {
            return Ok(name.clone());
        }
        let name = self.allocate_name(hint, &target);
        self.target_names.insert(target.clone(), name.clone());

        // The placeholder breaks recursive reference cycles while the target
        // is being lowered. It is replaced before the parse result is returned.
        self.types
            .insert(name.clone(), Type::Primitive(Primitive::Void));
        if self.building.insert(target.clone()) {
            let schema = self.target_value(&target).ok_or_else(|| {
                format!(
                    "unable to resolve Mojang schema target {}{}",
                    target.file, target.pointer
                )
            })?;
            let lowered = self.lower_named_schema(&schema, &name, &target.file)?;
            self.types.insert(name.clone(), lowered);
            self.building.remove(&target);
        }
        Ok(name)
    }

    fn allocate_name(&mut self, hint: &str, target: &Target) -> String {
        let base = crate::generator::utils::clean_type_name(hint);
        let base = if base.is_empty() {
            "GeneratedType".to_string()
        } else {
            base
        };
        if !self.used_names.contains_key(&base) {
            self.used_names.insert(base.clone(), target.clone());
            return base;
        }
        if self.used_names.get(&base) == Some(target) {
            return base;
        }

        let file_stem = target
            .file
            .trim_end_matches(".json")
            .split('/')
            .next_back()
            .unwrap_or("Schema");
        let mut candidate = format!(
            "{}{}",
            base,
            crate::generator::utils::safe_camel_ident(file_stem)
        );
        let mut suffix = 2;
        while self.used_names.contains_key(&candidate) {
            candidate = format!("{}{}", base, suffix);
            suffix += 1;
        }
        self.used_names.insert(candidate.clone(), target.clone());
        candidate
    }

    fn allocate_anonymous_name(&mut self, hint: &str) -> String {
        self.anonymous_counter += 1;
        let target = Target {
            file: format!("<anonymous-{}>", self.anonymous_counter),
            pointer: "#".to_string(),
        };
        self.allocate_name(&format!("{}Variant", hint), &target)
    }

    fn target_value(&self, target: &Target) -> Option<Value> {
        let document = self.documents.get(&target.file)?;
        pointer_value(document, &target.pointer).cloned()
    }

    fn resolve_ref(&self, reference: &str, current_file: &str) -> Result<Target, String> {
        let (file_part, fragment) = reference.split_once('#').unwrap_or((reference, ""));
        let file = if file_part.is_empty() {
            current_file.to_string()
        } else {
            let normalized = file_part.replace('\\', "/");
            let candidate = normalized
                .split('/')
                .rfind(|part| !part.is_empty() && *part != "." && *part != "..")
                .unwrap_or(normalized.as_str());
            if self.documents.contains_key(candidate) {
                candidate.to_string()
            } else {
                return Err(format!(
                    "external Mojang $ref points to missing file {candidate}"
                ));
            }
        };
        let pointer = format!("#{fragment}");
        Ok(Target { file, pointer })
    }

    fn lower_ref(
        &mut self,
        reference: &str,
        current_file: &str,
        allow_void: bool,
    ) -> Result<Type, Box<dyn std::error::Error>> {
        if let Some(builtin) = builtin_type_for_reference(reference) {
            return Ok(builtin);
        }
        let target = self.resolve_ref(reference, current_file)?;
        let target_value = self.target_value(&target).ok_or_else(|| {
            format!("unable to resolve Mojang $ref {reference} in {current_file}")
        })?;
        let title = target_value
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| target_name_from_pointer(&target));
        if is_void_schema(&target_value) {
            if !allow_void {
                return Err(format!("Mojang $ref {reference} in {current_file} resolves to an untyped schema; add a correction or model the missing wire type explicitly").into());
            }
            return Ok(Type::Primitive(Primitive::Void));
        }
        Ok(Type::Reference(self.ensure_named(target, &title)?))
    }

    fn lower_named_schema(
        &mut self,
        schema: &Value,
        hint: &str,
        current_file: &str,
    ) -> Result<Type, Box<dyn std::error::Error>> {
        if schema.get("oneOf").is_some() {
            return self.lower_union_type(schema, hint, current_file);
        }
        self.lower_schema(schema, hint, current_file)
    }

    fn lower_schema(
        &mut self,
        schema: &Value,
        hint: &str,
        current_file: &str,
    ) -> Result<Type, Box<dyn std::error::Error>> {
        self.lower_schema_with_void(schema, hint, current_file, false)
    }

    fn lower_schema_with_void(
        &mut self,
        schema: &Value,
        hint: &str,
        current_file: &str,
        allow_void: bool,
    ) -> Result<Type, Box<dyn std::error::Error>> {
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            // Enum definitions carry the symbolic members, while packet fields
            // carry the actual wire representation (underlying type and
            // Compression). A plain reference would discard that contextual
            // metadata and encode many int32 enums as fixed I32LE values.
            if serialization_options(schema)
                .iter()
                .any(|option| option.eq_ignore_ascii_case("Enum-as-Value"))
            {
                let target = self.resolve_ref(reference, current_file)?;
                let mut enum_schema = self.target_value(&target).ok_or_else(|| {
                    format!("unable to resolve Mojang enum $ref {reference} in {current_file}")
                })?;
                if enum_schema.get("enum").is_some() {
                    let object = enum_schema
                        .as_object_mut()
                        .ok_or_else(|| format!("Mojang enum $ref {reference} is not an object"))?;
                    for key in ["x-underlying-type", "x-serialization-options"] {
                        if let Some(value) = schema.get(key) {
                            object.insert(key.to_string(), value.clone());
                        }
                    }
                    let name = self.allocate_anonymous_name(hint);
                    let lowered = self.lower_enum(&enum_schema, &name, current_file)?;
                    self.types.insert(name.clone(), lowered);
                    return Ok(Type::Reference(name));
                }
            }
            return self.lower_ref(reference, current_file, allow_void);
        }
        if schema.get("oneOf").is_some() {
            let name = self.allocate_anonymous_name(hint);
            let union = self.lower_union_type(schema, &name, current_file)?;
            self.types.insert(name.clone(), union);
            return Ok(Type::Reference(name));
        }
        if let Some(all_of) = schema.get("allOf").and_then(Value::as_array) {
            if all_of.len() == 1 {
                return self.lower_schema_with_void(&all_of[0], hint, current_file, allow_void);
            }
            return Err(format!("unsupported multi-branch allOf in {current_file}: {hint}").into());
        }
        if schema.get("enum").is_some() {
            return self.lower_enum(schema, hint, current_file);
        }

        let schema_type = schema.get("type").and_then(Value::as_str);
        match schema_type {
            Some("object") => {
                if schema.get("properties").is_some() {
                    Ok(Type::Container(self.lower_container(
                        schema,
                        hint,
                        current_file,
                    )?))
                } else if let Some(entries) = schema.get("additionalProperties") {
                    // Mojang represents dictionaries as arrays of explicit
                    // key/value entry objects. Preserve that wire layout with
                    // the existing length-prefixed array IR.
                    if entries.is_boolean() {
                        return Err(format!(
                            "untyped additionalProperties map {hint} in {current_file}"
                        )
                        .into());
                    }
                    let inner_type =
                        self.lower_schema(entries, &format!("{hint}Entry"), current_file)?;
                    Ok(self.lower_array_type(schema, inner_type))
                } else {
                    Ok(Type::Container(Container {
                        name: hint.to_string(),
                        fields: Vec::new(),
                    }))
                }
            }
            None if schema.get("properties").is_some() => Ok(Type::Container(
                self.lower_container(schema, hint, current_file)?,
            )),
            None if is_void_schema(schema) => {
                if allow_void {
                    Ok(Type::Primitive(Primitive::Void))
                } else if schema
                    .get("x-valentine-allow-void")
                    .and_then(Value::as_bool)
                    == Some(true)
                {
                    warn!(
                        file = current_file,
                        field = hint,
                        "Mojang schema explicitly allows an untyped field; lowering it to void is a documented parity gap"
                    );
                    Ok(Type::Primitive(Primitive::Void))
                } else {
                    Err(format!("untyped Mojang schema in {current_file}: {hint}; add a correction or model the missing wire type explicitly").into())
                }
            }
            None => Err(format!("schema {hint} in {current_file} has no type").into()),
            Some("array") => {
                let items = schema
                    .get("items")
                    .ok_or_else(|| format!("array {hint} in {current_file} has no items"))?;
                // Mojang puts element wire metadata on the array node rather
                // than on `items` (SetHud's Hud Element, PlayerAuthInput's
                // Input Data). Without propagating it, int32 elements carrying
                // Compression would encode as fixed I32LE instead of a zigzag
                // varint. "No size compression" is excluded deliberately: it
                // governs the count prefix, not the elements.
                let merged_items = match items.as_object() {
                    Some(object) if items.get("x-serialization-options").is_none() => {
                        let element_options = serialization_options(schema)
                            .into_iter()
                            .filter(|option| !option.eq_ignore_ascii_case("No size compression"))
                            .map(Value::String)
                            .collect::<Vec<_>>();
                        if element_options.is_empty() {
                            None
                        } else {
                            let mut object = object.clone();
                            object.insert(
                                "x-serialization-options".to_string(),
                                Value::Array(element_options),
                            );
                            Some(Value::Object(object))
                        }
                    }
                    _ => None,
                };
                let items = merged_items.as_ref().unwrap_or(items);
                let inner_type = self.lower_schema(items, &format!("{hint}Item"), current_file)?;
                Ok(self.lower_array_type(schema, inner_type))
            }
            Some("string") => Ok(Type::String {
                count_type: Box::new(Type::Primitive(Primitive::VarInt)),
                encoding: Some("utf8".to_string()),
            }),
            Some("boolean") => Ok(Type::Primitive(Primitive::Bool)),
            Some("integer") | Some("number") => Ok(Type::Primitive(self.primitive_for(schema)?)),
            Some("null") => Ok(Type::Primitive(Primitive::Void)),
            Some(other) => Err(format!(
                "unsupported Mojang JSON Schema type {other} in {current_file}: {hint}"
            )
            .into()),
        }
    }

    fn lower_array_type(&self, schema: &Value, inner_type: Type) -> Type {
        let fixed_size = schema
            .get("minItems")
            .and_then(value_as_i64)
            .zip(schema.get("maxItems").and_then(value_as_i64))
            .filter(|(min, max)| min >= &0 && min == max)
            .and_then(|(min, _)| usize::try_from(min).ok());
        if let Some(size) = fixed_size {
            return Type::FixedArray {
                size,
                inner_type: Box::new(inner_type),
            };
        }

        if let Some(count_type) = schema
            .get("x-valentine-array-count-type")
            .and_then(Value::as_str)
        {
            let primitive = primitive_from_underlying(count_type, Vec::new())
                .expect("validated override array count type");
            return Type::Array {
                count_type: Box::new(Type::Primitive(primitive)),
                inner_type: Box::new(inner_type),
            };
        }

        let no_size_compression = serialization_options(schema)
            .iter()
            .any(|option| option.eq_ignore_ascii_case("No size compression"));
        Type::Array {
            count_type: Box::new(if no_size_compression {
                Type::Primitive(Primitive::U32LE)
            } else {
                Type::Primitive(Primitive::VarInt)
            }),
            inner_type: Box::new(inner_type),
        }
    }

    fn lower_container(
        &mut self,
        schema: &Value,
        hint: &str,
        current_file: &str,
    ) -> Result<Container, Box<dyn std::error::Error>> {
        let empty_properties = serde_json::Map::new();
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .unwrap_or(&empty_properties);
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();

        let mut names = properties.keys().cloned().collect::<Vec<_>>();
        for field_name in &names {
            if properties.get(field_name).and_then(ordinal).is_none() {
                warn!(
                    file = current_file,
                    container = hint,
                    field = field_name,
                    "Mojang schema field has no x-ordinal-index; alphabetical fallback is being used"
                );
            }
        }
        names.sort_by(|a, b| {
            let a_ordinal = properties.get(a).and_then(ordinal).unwrap_or(i64::MAX);
            let b_ordinal = properties.get(b).and_then(ordinal).unwrap_or(i64::MAX);
            a_ordinal.cmp(&b_ordinal).then_with(|| a.cmp(b))
        });

        let mut fields = Vec::with_capacity(names.len());
        for field_name in names {
            let field_schema = properties
                .get(&field_name)
                .ok_or_else(|| format!("missing property {field_name} in {current_file}"))?;
            let field_hint = format!(
                "{}{}",
                hint,
                crate::generator::utils::safe_camel_ident(&field_name)
            );
            let mut field_type = self.lower_schema(field_schema, &field_hint, current_file)?;
            let allow_void = field_schema
                .get("x-valentine-allow-void")
                .and_then(Value::as_bool)
                == Some(true);
            if required.contains(field_name.as_str())
                && matches!(field_type, Type::Primitive(Primitive::Void))
                && !allow_void
            {
                return Err(format!(
                    "required typeless Mojang field {field_name} in {current_file}"
                )
                .into());
            }
            let options = serialization_options(field_schema);
            let presence_count = if options.iter().any(|option| option == "+double-optional") {
                2
            } else if required.contains(field_name.as_str()) {
                0
            } else {
                1
            };
            for _ in 0..presence_count {
                field_type = Type::Option(Box::new(field_type));
            }
            fields.push(Field {
                name: field_name,
                type_def: field_type,
            });
        }

        Ok(Container {
            name: hint.to_string(),
            fields,
        })
    }

    fn lower_enum(
        &self,
        schema: &Value,
        hint: &str,
        current_file: &str,
    ) -> Result<Type, Box<dyn std::error::Error>> {
        let values = schema
            .get("enum")
            .and_then(Value::as_array)
            .ok_or("enum schema has no enum array")?;
        let explicit_numbers = schema
            .get("x-enum-values")
            .and_then(Value::as_array)
            .filter(|numbers| numbers.len() == values.len());
        let numbers = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                value.as_i64().or_else(|| {
                    explicit_numbers
                        .and_then(|numbers| numbers.get(index))
                        .and_then(value_as_i64)
                })
            })
            .collect::<Option<Vec<_>>>();
        let numbers = numbers.ok_or_else(|| {
            format!(
                "Mojang string enum {hint} in {current_file} has no explicit wire values; \
                 add an x-enum-values correction"
            )
        })?;
        let mut variants = Vec::with_capacity(values.len());
        let mut used_values = HashSet::new();
        for (index, value) in values.iter().enumerate() {
            let name = value
                .as_str()
                .map(str::to_owned)
                .or_else(|| value.as_i64().map(|number| format!("Value{number}")))
                .unwrap_or_else(|| format!("Value{index}"));
            let number = numbers[index];
            if !used_values.insert(number) {
                return Err(format!("Mojang enum contains duplicate wire value {number}").into());
            }
            variants.push((name, number));
        }
        Ok(Type::Enum {
            underlying: self.primitive_for(schema)?,
            variants,
        })
    }

    fn lower_union_type(
        &mut self,
        schema: &Value,
        hint: &str,
        current_file: &str,
    ) -> Result<Type, Box<dyn std::error::Error>> {
        let branches = schema
            .get("oneOf")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("oneOf in {current_file}: {hint} is not an array"))?;
        if branches.is_empty() {
            return Err(format!("oneOf in {current_file}: {hint} has no variants").into());
        }
        if schema.get("x-control-value-type").is_none() {
            return Err(format!("oneOf in {current_file}: {hint} has no x-control-value-type; add a documented correction for an untagged representation").into());
        }
        let mut variants = Vec::with_capacity(branches.len());
        let mut used_names = HashSet::new();
        let mut used_values = HashSet::new();
        for (index, branch) in branches.iter().enumerate() {
            let value = branch
                .get("x-ordinal-index")
                .and_then(value_as_i64)
                .or_else(|| {
                    schema
                        .get("x-control-values")
                        .and_then(Value::as_array)
                        .and_then(|values| values.get(index))
                        .and_then(value_as_i64)
                })
                .ok_or_else(|| {
                    format!("oneOf in {current_file}: {hint} branch {index} has no wire discriminant; add x-ordinal-index or a documented override")
                })?;
            if !used_values.insert(value) {
                return Err(format!(
                    "oneOf in {current_file}: {hint} repeats wire discriminant {value}"
                )
                .into());
            }
            let branch_name = branch
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|reference| self.resolve_ref(reference, current_file).ok())
                .and_then(|target| self.target_value(&target))
                .and_then(|target| {
                    target
                        .get("title")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .or_else(|| {
                    branch
                        .get("title")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| format!("Variant{value}"));
            let mut name = crate::generator::utils::safe_camel_ident(&branch_name);
            if name.is_empty() {
                name = format!("Variant{value}");
            }
            if !used_names.insert(name.clone()) {
                name = format!("{}{}", name, value);
                used_names.insert(name.clone());
            }
            let type_def = self.lower_schema_with_void(
                branch,
                &format!("{}{}", hint, name),
                current_file,
                true,
            )?;
            variants.push(UnionVariant {
                control_value: value,
                name,
                type_def,
            });
        }
        Ok(Type::Union {
            control_type: self.control_primitive(schema)?,
            variants,
        })
    }

    fn primitive_for(&self, schema: &Value) -> Result<Primitive, Box<dyn std::error::Error>> {
        let underlying = schema
            .get("x-underlying-type")
            .and_then(Value::as_str)
            .or_else(|| schema.get("format").and_then(Value::as_str))
            .unwrap_or_else(|| match schema.get("type").and_then(Value::as_str) {
                Some("number") => "double",
                _ => "int32",
            });
        primitive_from_underlying(underlying, serialization_options(schema))
    }

    fn control_primitive(&self, schema: &Value) -> Result<Primitive, Box<dyn std::error::Error>> {
        let underlying = schema
            .get("x-control-value-type")
            .and_then(Value::as_str)
            .ok_or("Mojang oneOf is missing x-control-value-type")?;
        primitive_from_underlying(underlying, serialization_options(schema))
    }
}

fn builtin_type_for_reference(reference: &str) -> Option<Type> {
    let fragment = reference
        .split_once('#')
        .map(|(_, fragment)| fragment)
        .unwrap_or(reference);
    match fragment.strip_prefix("/definitions/") {
        Some(MOJANG_BUILTIN_COMPOUND_TAG_ID) => Some(Type::Primitive(Primitive::Nbt)),
        _ => None,
    }
}

fn packet_metadata(document: &Value) -> Option<(u32, String)> {
    let meta = document.get("$metaProperties")?.as_object()?;
    let id = meta.get("[cereal:packet]").and_then(value_as_i64)?;
    let title = document
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("UnnamedPacket")
        .to_string();
    Some((id as u32, title))
}

fn packet_ir_name(title: &str) -> String {
    let base = title
        .strip_suffix("PacketPayload")
        .or_else(|| title.strip_suffix("Packet"))
        .unwrap_or(title);
    if base.starts_with("packet_") {
        base.to_string()
    } else {
        format!("packet_{base}")
    }
}

fn pointer_value<'a>(value: &'a Value, pointer: &str) -> Option<&'a Value> {
    let pointer = pointer.strip_prefix('#').unwrap_or(pointer);
    if pointer.is_empty() {
        return Some(value);
    }
    let mut current = value;
    for segment in pointer.split('/').skip(1) {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        current = current.get(&segment)?;
    }
    Some(current)
}

fn target_name_from_pointer(target: &Target) -> String {
    target
        .pointer
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| target.file.trim_end_matches(".json"))
        .replace("~1", "/")
        .replace("~0", "~")
}

fn ordinal(schema: &Value) -> Option<i64> {
    schema.get("x-ordinal-index").and_then(value_as_i64)
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
        .or_else(|| value.as_str().and_then(|number| number.parse().ok()))
}

fn serialization_options(schema: &Value) -> Vec<String> {
    schema
        .get("x-serialization-options")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn is_void_schema(schema: &Value) -> bool {
    schema.get("$ref").is_none()
        && schema.get("type").is_none()
        && schema.get("properties").is_none()
        && schema.get("additionalProperties").is_none()
        && schema.get("items").is_none()
        && schema.get("enum").is_none()
        && schema.get("oneOf").is_none()
        && schema.get("allOf").is_none()
}

fn primitive_from_underlying(
    underlying: &str,
    options: Vec<String>,
) -> Result<Primitive, Box<dyn std::error::Error>> {
    let normalized = underlying.to_ascii_lowercase().replace(['_', '-', ' '], "");
    let compressed = options
        .iter()
        .any(|option| option.eq_ignore_ascii_case("Compression"));
    for option in &options {
        let lower = option.to_ascii_lowercase();
        if lower.contains("varlong") || lower.contains("varint64") {
            return Ok(Primitive::VarLong);
        }
        if lower.contains("varint") {
            return Ok(Primitive::VarInt);
        }
    }
    let zigzag = options.iter().find_map(|option| {
        let lower = option.to_ascii_lowercase();
        if lower.contains("zigzag64") {
            Some(Primitive::ZigZag64)
        } else if lower.contains("zigzag") {
            Some(Primitive::ZigZag32)
        } else {
            None
        }
    });
    if let Some(zigzag) = zigzag {
        return Ok(zigzag);
    }
    if compressed {
        // Mojang's `Compression` means a signed value is zig-zag encoded
        // before the variable-length representation.  Treating every
        // compressed integer as an unsigned VarInt loses negative values and
        // was the source of a large wire-level mismatch with the protocol
        // 2168 gophertunnel and Cloudburst serializers.
        return Ok(match normalized.as_str() {
            "long" | "int64" => Primitive::ZigZag64,
            "byte" | "int8" | "sbyte" | "short" | "int16" | "int" | "int32" => Primitive::ZigZag32,
            "ulong" | "uint64" => Primitive::VarLong,
            "ubyte" | "uint8" | "ushort" | "uint16" | "uint" | "uint32" => Primitive::VarInt,
            // A few upstream schemas attach Compression to boolean and
            // collection nodes as a broad metadata marker.  Their wire type
            // is not an integer varint, so preserve the actual primitive.
            "bool" | "boolean" => Primitive::Bool,
            other => {
                return Err(format!(
                    "Compression is unsupported for Mojang underlying type {other}"
                )
                .into());
            }
        });
    }

    let mut primitive = match normalized.as_str() {
        "bool" | "boolean" => Primitive::Bool,
        "byte" | "int8" | "sbyte" => Primitive::I8,
        "ubyte" | "uint8" => Primitive::U8,
        "short" | "int16" => Primitive::I16,
        "ushort" | "uint16" => Primitive::U16,
        "int" | "int32" => Primitive::I32,
        "uint" | "uint32" => Primitive::U32,
        "long" | "int64" => Primitive::I64,
        "ulong" | "uint64" => Primitive::U64,
        "float" | "float32" => Primitive::F32,
        "double" | "float64" | "number" => Primitive::F64,
        "varint" | "varint32" => Primitive::VarInt,
        "varlong" | "varint64" => Primitive::VarLong,
        "zigzag32" => Primitive::ZigZag32,
        "zigzag64" => Primitive::ZigZag64,
        "uuid" => Primitive::Uuid,
        "nbt" | "lnbt" => Primitive::Nbt,
        "bytearray" | "buffer" => Primitive::ByteArray,
        "string" => Primitive::ByteArray,
        other => {
            return Err(format!("unsupported Mojang x-underlying-type {other}").into());
        }
    };
    let big_endian = options.iter().any(|option| {
        option
            .to_ascii_lowercase()
            .replace(['_', '-', ' '], "")
            .contains("bigendian")
    });
    // Bedrock fixed-width numerics are little-endian by default.  Mojang's
    // schemas spell the exception as `Big Endian`; older code looked for a
    // literal `little endian` option that never occurs in this corpus and
    // consequently emitted big-endian primitives for almost every field.
    if !big_endian {
        primitive = match primitive {
            Primitive::U16 => Primitive::U16LE,
            Primitive::I16 => Primitive::I16LE,
            Primitive::U32 => Primitive::U32LE,
            Primitive::I32 => Primitive::I32LE,
            Primitive::U64 => Primitive::U64LE,
            Primitive::I64 => Primitive::I64LE,
            Primitive::F32 => Primitive::F32LE,
            Primitive::F64 => Primitive::F64LE,
            other => other,
        };
    }
    Ok(primitive)
}

fn version_parts(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse, primitive_from_underlying};
    use crate::ir::Primitive;
    use serde_json::json;
    use std::fs;

    #[test]
    fn lowers_refs_ordinals_unions_and_presence_headers() {
        let root =
            std::env::temp_dir().join(format!("valentine-gen-mojang-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("json")).expect("create fixture");
        fs::write(
            root.join("json").join("Value.json"),
            serde_json::to_vec(&json!({
                "title":"Value",
                "type":"object",
                "properties": {
                    "Number": {"type":"integer", "x-underlying-type":"uint32", "x-ordinal-index":0}
                },
                "required":["Number"]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("json").join("ExamplePacket.json"),
            serde_json::to_vec(&json!({
                "title":"ExamplePacket",
                "type":"object",
                "properties": {
                    "Choice": {
                        "oneOf": [
                            {"$ref":"Value.json", "x-ordinal-index":3},
                            {"type":"boolean", "x-ordinal-index":4}
                        ],
                        "x-control-value-type":"uint32",
                        "x-ordinal-index":1
                    },
                    "Compound": {
                        "$ref":"#/definitions/3172631924",
                        "x-ordinal-index":2
                    },
                    "Optional": {"type":"integer", "x-underlying-type":"int32", "x-ordinal-index":0}
                    ,"DoubleOptional": {"type":"integer", "x-underlying-type":"int32", "x-serialization-options":["+double-optional"], "x-ordinal-index":5}
                },
                "required":["Choice","Compound"],
                "$metaProperties":{"[cereal:packet]":99}
            }))
            .unwrap(),
        )
        .unwrap();

        let result = parse(&root, &root.join("missing-overrides")).expect("parse Mojang fixture");
        assert_eq!(result.packets.len(), 1);
        assert_eq!(result.packets[0].id, 99);
        assert_eq!(result.packets[0].body.fields[0].name, "Optional");
        assert!(matches!(
            &result.packets[0].body.fields[0].type_def,
            crate::ir::Type::Option(_)
        ));
        assert!(matches!(
            &result.packets[0].body.fields[1].type_def,
            crate::ir::Type::Reference(_)
        ));
        assert!(matches!(
            &result.packets[0].body.fields[2].type_def,
            crate::ir::Type::Primitive(Primitive::Nbt)
        ));
        assert!(matches!(
            &result.packets[0].body.fields[3].type_def,
            crate::ir::Type::Option(inner) if matches!(inner.as_ref(), crate::ir::Type::Option(_))
        ));
        let union = result
            .types
            .values()
            .find_map(|ty| match ty {
                crate::ir::Type::Union { variants, .. } => Some(variants),
                _ => None,
            })
            .expect("fixture union lowered");
        assert_eq!(
            union
                .iter()
                .map(|variant| variant.control_value)
                .collect::<Vec<_>>(),
            vec![3, 4]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn packet_root_reference_uses_the_referenced_payload_fields() {
        let root = std::env::temp_dir().join(format!(
            "valentine-gen-mojang-packet-ref-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("json")).expect("create fixture");
        fs::write(
            root.join("json").join("Payload.json"),
            serde_json::to_vec(&json!({
                "title":"Payload",
                "type":"object",
                "properties": {
                    "Value": {"type":"integer", "x-underlying-type":"uint32", "x-ordinal-index":0}
                },
                "required":["Value"]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("json").join("WrappedPacket.json"),
            serde_json::to_vec(&json!({
                "title":"WrappedPacket",
                "$ref":"Payload.json",
                "$metaProperties":{"[cereal:packet]":7}
            }))
            .unwrap(),
        )
        .unwrap();

        let result = parse(&root, &root.join("missing-overrides")).expect("parse packet wrapper");
        assert_eq!(result.packets.len(), 1);
        assert_eq!(result.packets[0].body.fields.len(), 1);
        assert_eq!(result.packets[0].body.fields[0].name, "Value");
        assert!(matches!(
            result.packets[0].body.fields[0].type_def,
            crate::ir::Type::Primitive(Primitive::U32LE)
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn referenced_enum_uses_the_packet_fields_wire_metadata() {
        let root = std::env::temp_dir().join(format!(
            "valentine-gen-mojang-enum-wire-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("json")).expect("create fixture");
        fs::write(
            root.join("json").join("Mode.json"),
            serde_json::to_vec(&json!({
                "title":"Mode",
                "type":"string",
                "enum":["First", "Second"],
                "x-enum-values":[0, 1]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("json").join("EnumPacket.json"),
            serde_json::to_vec(&json!({
                "title":"EnumPacket",
                "type":"object",
                "properties": {
                    "Mode": {
                        "$ref":"Mode.json",
                        "x-underlying-type":"int32",
                        "x-serialization-options":["Compression", "Enum-as-Value"],
                        "x-ordinal-index":0
                    }
                },
                "required":["Mode"],
                "$metaProperties":{"[cereal:packet]":8}
            }))
            .unwrap(),
        )
        .unwrap();

        let result = parse(&root, &root.join("missing-overrides")).expect("parse enum packet");
        let crate::ir::Type::Reference(name) = &result.packets[0].body.fields[0].type_def else {
            panic!("enum field should reference its contextual enum type");
        };
        assert!(matches!(
            result.types.get(name),
            Some(crate::ir::Type::Enum {
                underlying: Primitive::ZigZag32,
                ..
            })
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn primitive_options_preserve_bedrock_wire_endianness_and_signed_compression() {
        let cases = [
            ("uint32", vec![], Primitive::U32LE),
            ("int32", vec![], Primitive::I32LE),
            ("uint16", vec![], Primitive::U16LE),
            ("int16", vec![], Primitive::I16LE),
            ("uint64", vec![], Primitive::U64LE),
            ("int64", vec![], Primitive::I64LE),
            ("float", vec![], Primitive::F32LE),
            ("double", vec![], Primitive::F64LE),
            ("uint32", vec!["Big Endian".to_string()], Primitive::U32),
            ("int32", vec!["Big Endian".to_string()], Primitive::I32),
            ("double", vec!["Big Endian".to_string()], Primitive::F64),
            (
                "int32",
                vec!["Compression".to_string()],
                Primitive::ZigZag32,
            ),
            (
                "int64",
                vec!["Compression".to_string()],
                Primitive::ZigZag64,
            ),
            ("uint32", vec!["Compression".to_string()], Primitive::VarInt),
            (
                "uint64",
                vec!["Compression".to_string()],
                Primitive::VarLong,
            ),
        ];

        for (underlying, options, expected) in cases {
            assert_eq!(
                primitive_from_underlying(underlying, options).expect("primitive mapping"),
                expected,
                "unexpected mapping for {underlying}"
            );
        }
    }
}
