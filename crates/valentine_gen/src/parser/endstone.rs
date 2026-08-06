//! Frontend for [endstonemc/protocol-docs].
//!
//! These schemas are extracted from a running Bedrock Dedicated Server by
//! `endstonemc/protocol-dumper`, so they describe what the server actually
//! serialises rather than what documentation says it serialises. That matters:
//! the Mojang frontend needs ~147 corrections to reach the same wire, because
//! the published schemas mark fields `required` that carry a presence byte,
//! omit whole fields, and annotate 8-bit fields with a `Compression` flag that
//! means a raw byte in one packet and a varint in another. This corpus states
//! optionality, element encodings and array prefixes directly.
//!
//! The layout is three flat directories of JSON keyed by a `name` field
//! (`::` becomes `__` in filenames, so names are read from the documents, never
//! from paths):
//!
//! - `packets/` — `{id, name, fields}`
//! - `types/`   — `{name, fields}`; every named type is a struct, even
//!   single-field wrappers such as `ActorRuntimeID`
//! - `enums/`   — `{name, values: [{name, value}]}`; the wire width comes from
//!   the *referring field*, not the enum, so enums are lowered per use site
//!
//! [endstonemc/protocol-docs]: https://github.com/endstonemc/protocol-docs

use crate::ir::{Container, Field, Packet, Primitive, Type, UnionVariant};
use crate::overrides;
use crate::parser::ParseResult;
use crate::parser::mojang::VersionInfo;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use tracing::warn;

/// Types whose wire form Valentine models as a primitive. Preferred over any
/// same-named document so an NBT blob or UUID never lowers to a struct.
fn builtin(name: &str) -> Option<Primitive> {
    match name {
        "CompoundTag" => Some(Primitive::Nbt),
        "mce::UUID" => Some(Primitive::Uuid),
        _ => None,
    }
}

/// The only two names the corpus references without defining. Everything else
/// resolves, which is the headline difference from the Mojang frontend. Both
/// are handled here explicitly rather than guessed at silently.
fn undefined_reference(name: &str) -> Option<Type> {
    // A `brstd::bitset<N>` is Bedrock's actor-flag bitset. Mojang models the
    // same field as a length-prefixed uint8 array and the checked-in crate
    // already encodes it that way, so keep the two frontends agreeing rather
    // than inventing a third representation. gophertunnel writes it through
    // its own `Bitset` helper (7 bits per byte with a continuation flag), which
    // the conformance comparison cannot read, so this stays unverified.
    if name.starts_with("brstd::bitset<") {
        return Some(Type::Array {
            count_type: Box::new(Type::Primitive(Primitive::VarInt)),
            inner_type: Box::new(Type::Primitive(Primitive::U8)),
        });
    }
    // `cereal::DynamicValue` is the recursive DataStore property value. The
    // Mojang frontend cannot express it either and lowers it to a zero-byte
    // unit with a warning; matching that keeps the corpus inspectable and
    // keeps the gap in one place. Implementing the dependent
    // DataStorePropertyValue family is a wire-correctness TODO for both.
    if name == "cereal::DynamicValue" {
        return Some(Type::Primitive(Primitive::Void));
    }
    None
}

/// Scalar spellings used by the dumper. `varint*` is zigzag-signed and
/// `uvarint*` is unsigned; conflating them silently corrupts the wire, which is
/// exactly the class of bug this frontend exists to avoid.
fn scalar(name: &str) -> Option<Primitive> {
    Some(match name {
        "bool" => Primitive::Bool,
        "uint8" => Primitive::U8,
        "int8" => Primitive::I8,
        "uint16" => Primitive::U16LE,
        "int16" => Primitive::I16LE,
        "uint32" => Primitive::U32LE,
        "int32" => Primitive::I32LE,
        "uint64" => Primitive::U64LE,
        "int64" => Primitive::I64LE,
        "int32_be" => Primitive::I32,
        "float" => Primitive::F32LE,
        "double" => Primitive::F64LE,
        "varint32" => Primitive::ZigZag32,
        "varint64" => Primitive::ZigZag64,
        "uvarint32" => Primitive::VarInt,
        "uvarint64" => Primitive::VarLong,
        _ => return None,
    })
}

fn string_type() -> Type {
    Type::String {
        count_type: Box::new(Type::Primitive(Primitive::VarInt)),
        encoding: Some("utf8".to_string()),
    }
}

fn load_dir(root: &Path, sub: &str) -> Result<HashMap<String, Value>, Box<dyn std::error::Error>> {
    let dir = root.join(sub);
    let mut out = HashMap::new();
    for entry in
        fs::read_dir(&dir).map_err(|error| format!("unable to read {}: {error}", dir.display()))?
    {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let value: Value =
            serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{} has no name", path.display()))?
            .to_string();
        out.insert(name, value);
    }
    Ok(out)
}

/// `repeat` is either `{prefix}` — a length-prefixed array — or `{count}`, a
/// fixed-length one with no prefix on the wire. Both occur, sometimes nested:
/// the subchunk heightmap is a `count: 16` of a `count: 16`, i.e. the 2D
/// `[z][x]` array Mojang documents.
fn lower_repeat(
    repeat: &Value,
    inner: Type,
    ctx: &str,
) -> Result<Type, Box<dyn std::error::Error>> {
    if let Some(prefix) = repeat.get("prefix").and_then(Value::as_str) {
        let count =
            scalar(prefix).ok_or_else(|| format!("{ctx}: unknown repeat prefix {prefix}"))?;
        return Ok(Type::Array {
            count_type: Box::new(Type::Primitive(count)),
            inner_type: Box::new(inner),
        });
    }
    if let Some(count) = repeat.get("count").and_then(Value::as_u64) {
        return Ok(Type::FixedArray {
            size: count as usize,
            inner_type: Box::new(inner),
        });
    }
    Err(format!(
        "{ctx}: repeat has neither prefix nor count (keys: {})",
        repeat
            .as_object()
            .map(|object| object.keys().cloned().collect::<Vec<_>>().join(", "))
            .unwrap_or_default()
    )
    .into())
}

struct Lowerer {
    types_src: HashMap<String, Value>,
    enums_src: HashMap<String, Value>,
    out: HashMap<String, Type>,
    /// Names currently being lowered. Bedrock has genuinely recursive payloads
    /// (NBT-bearing data stores), so a reference back into an in-progress type
    /// resolves to a plain reference instead of recursing forever.
    active: HashSet<String>,
}

/// Valentine identifiers cannot contain `::`.
fn safe_name(name: &str) -> String {
    name.replace("::", "_")
        .replace(['<', '>', ' ', ',', '*'], "_")
        .replace("__", "_")
}

impl Lowerer {
    fn lower_named(&mut self, name: &str, ctx: &str) -> Result<Type, Box<dyn std::error::Error>> {
        if let Some(primitive) = builtin(name) {
            return Ok(Type::Primitive(primitive));
        }
        let key = safe_name(name);
        if self.out.contains_key(&key) || self.active.contains(name) {
            return Ok(Type::Reference(key));
        }
        let document = match self.types_src.get(name).cloned() {
            Some(document) => document,
            None => {
                let fallback = undefined_reference(name)
                    .ok_or_else(|| format!("{ctx}: unknown endstone type {name}"))?;
                warn!(
                    r#type = name,
                    context = ctx,
                    "endstone corpus references a type it does not define; using the documented fallback"
                );
                return Ok(fallback);
            }
        };

        self.active.insert(name.to_string());
        // Insert a placeholder first so a self-reference discovered while
        // lowering the body resolves rather than reporting an unknown type.
        self.out
            .insert(key.clone(), Type::Primitive(Primitive::Void));
        let lowered = self.lower_container(&document, &key, name);
        self.active.remove(name);
        let container = lowered?;
        self.out.insert(key.clone(), Type::Container(container));
        Ok(Type::Reference(key))
    }

    fn lower_container(
        &mut self,
        document: &Value,
        struct_name: &str,
        ctx: &str,
    ) -> Result<Container, Box<dyn std::error::Error>> {
        let fields = document
            .get("fields")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{ctx}: document has no fields array"))?;
        let mut out = Vec::with_capacity(fields.len());
        for (index, field) in fields.iter().enumerate() {
            out.push(self.lower_field(field, index, ctx)?);
        }
        Ok(Container {
            name: struct_name.to_string(),
            fields: out,
        })
    }

    fn lower_field(
        &mut self,
        field: &Value,
        index: usize,
        ctx: &str,
    ) -> Result<Field, Box<dyn std::error::Error>> {
        let object = field
            .as_object()
            .ok_or_else(|| format!("{ctx}: field {index} is not an object"))?;

        // A field with a literal value and no name is a constant the server
        // writes unconditionally (PlayerAuthInput's presence flag, for one).
        // It occupies wire space, so it must be emitted, not skipped.
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("constant_{index}"));

        let hint = format!("{}{}", ctx, safe_name(&name));
        let mut lowered = self.lower_type_value(
            object
                .get("type")
                .ok_or_else(|| format!("{ctx}: field {name} has no type"))?,
            &hint,
            &format!("{ctx}.{name}"),
        )?;

        // An `enum` on a scalar field names the symbolic members; the width
        // stays that of the field itself.
        if let Some(enum_name) = object.get("enum").and_then(Value::as_str)
            && let Type::Primitive(underlying) = lowered
        {
            lowered = self.lower_enum(enum_name, underlying, &format!("{ctx}.{name}"))?;
        }

        if let Some(repeat) = object.get("repeat") {
            lowered = lower_repeat(repeat, lowered, &format!("{ctx}.{name}"))?;
        }

        // Applied last: optionality wraps the whole field, including an array.
        if object.get("optional").and_then(Value::as_bool) == Some(true) {
            lowered = Type::Option(Box::new(lowered));
        }

        Ok(Field {
            name,
            type_def: lowered,
        })
    }

    fn lower_enum(
        &mut self,
        name: &str,
        underlying: Primitive,
        ctx: &str,
    ) -> Result<Type, Box<dyn std::error::Error>> {
        let document = self
            .enums_src
            .get(name)
            .ok_or_else(|| format!("{ctx}: unknown endstone enum {name}"))?;
        let values = document
            .get("values")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{ctx}: enum {name} has no values"))?;
        let mut variants = Vec::with_capacity(values.len());
        for value in values {
            let variant = value
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{ctx}: enum {name} has an unnamed value"))?;
            let discriminant = value
                .get("value")
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("{ctx}: enum {name} value {variant} has no value"))?;
            variants.push((variant.to_string(), discriminant));
        }
        Ok(Type::Enum {
            underlying,
            variants,
        })
    }

    fn lower_type_value(
        &mut self,
        ty: &Value,
        hint: &str,
        ctx: &str,
    ) -> Result<Type, Box<dyn std::error::Error>> {
        if let Some(name) = ty.as_str() {
            if name == "string" {
                return Ok(string_type());
            }
            if let Some(primitive) = scalar(name) {
                return Ok(Type::Primitive(primitive));
            }
            return self.lower_named(name, ctx);
        }
        let object = ty
            .as_object()
            .ok_or_else(|| format!("{ctx}: type is neither a name nor an object"))?;

        if object.contains_key("switch") {
            return self.lower_switch(object, ctx);
        }
        if let (Some(key), Some(value)) = (object.get("key"), object.get("value")) {
            // A map is a length-prefixed array of key/value entries.
            let key_type = self.lower_type_value(key, &format!("{hint}Key"), ctx)?;
            let value_type = self.lower_type_value(value, &format!("{hint}Value"), ctx)?;
            let entry = Type::Container(Container {
                name: format!("{hint}Entry"),
                fields: vec![
                    Field {
                        name: "key".to_string(),
                        type_def: key_type,
                    },
                    Field {
                        name: "value".to_string(),
                        type_def: value_type,
                    },
                ],
            });
            return Ok(Type::Array {
                count_type: Box::new(Type::Primitive(Primitive::VarInt)),
                inner_type: Box::new(entry),
            });
        }
        if let Some(inner) = object.get("type") {
            let mut lowered = self.lower_type_value(inner, hint, ctx)?;
            // An enum in type position names the symbolic members. It only
            // changes the wire when the underlying is numeric: a string keyed
            // by an enum (persona piece types, say) is still a string.
            if let Some(enum_name) = object.get("enum").and_then(Value::as_str)
                && let Type::Primitive(underlying) = lowered
            {
                lowered = self.lower_enum(enum_name, underlying, ctx)?;
            }
            if let Some(repeat) = object.get("repeat") {
                lowered = lower_repeat(repeat, lowered, ctx)?;
            }
            return Ok(lowered);
        }
        Err(format!(
            "{ctx}: unsupported endstone type shape with keys [{}]",
            object.keys().cloned().collect::<Vec<_>>().join(", ")
        )
        .into())
    }

    fn lower_switch(
        &mut self,
        object: &Map<String, Value>,
        ctx: &str,
    ) -> Result<Type, Box<dyn std::error::Error>> {
        let switch = object
            .get("switch")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("{ctx}: switch is not an object"))?;
        let control_name = switch
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{ctx}: switch has no discriminator type"))?;
        let control_type = scalar(control_name)
            .ok_or_else(|| format!("{ctx}: unknown switch discriminator type {control_name}"))?;
        let cases = object
            .get("cases")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{ctx}: switch has no cases"))?;

        // Cases carry no discriminant of their own; they correspond positionally
        // to the discriminator enum's values. Bail rather than guess if the two
        // ever stop lining up.
        let discriminants: Vec<i64> = match switch.get("enum").and_then(Value::as_str) {
            Some(enum_name) => {
                let document = self
                    .enums_src
                    .get(enum_name)
                    .ok_or_else(|| format!("{ctx}: unknown switch enum {enum_name}"))?;
                let values = document
                    .get("values")
                    .and_then(Value::as_array)
                    .ok_or_else(|| format!("{ctx}: switch enum {enum_name} has no values"))?;
                if values.len() != cases.len() {
                    // TextPacket is the only site where these differ: its outer
                    // discriminant is the 0/1/2 payload category (gophertunnel
                    // derives the same byte in packet/text.go before writing
                    // the text type), while the referenced enum is the 12-value
                    // TextPacketType. The enum reference is mislabelled rather
                    // than the mapping being lost — each case payload carries
                    // the real text-type byte with a constraints.enum_values
                    // list. Fall back to positional discriminants and say so.
                    warn!(
                        context = ctx,
                        r#enum = enum_name,
                        enum_values = values.len(),
                        cases = cases.len(),
                        "switch enum size does not match the case count; \
                         treating the discriminant as positional"
                    );
                    (0..cases.len() as i64).collect()
                } else {
                    values
                        .iter()
                        .map(|value| {
                            value
                                .get("value")
                                .and_then(Value::as_i64)
                                .ok_or_else(|| format!("{ctx}: switch enum value has no value"))
                        })
                        .collect::<Result<_, _>>()?
                }
            }
            None => (0..cases.len() as i64).collect(),
        };

        let mut variants = Vec::with_capacity(cases.len());
        for (index, (case, control_value)) in cases.iter().zip(discriminants).enumerate() {
            // A null case is a discriminant that carries no payload.
            if case.is_null() {
                variants.push(UnionVariant {
                    control_value,
                    name: format!("Empty{index}"),
                    type_def: Type::Primitive(Primitive::Void),
                });
                continue;
            }
            let case_name = case.as_str().ok_or_else(|| {
                format!("{ctx}: switch case {index} is neither a type name nor null")
            })?;
            // A case may name a struct or be a bare scalar (a variant carrying
            // just a float, say), so resolve it like any other type position.
            let type_def = self.lower_type_value(case, case_name, ctx)?;
            variants.push(UnionVariant {
                control_value,
                name: safe_name(case_name.rsplit("::").next().unwrap_or(case_name)),
                type_def,
            });
        }
        Ok(Type::Union {
            control_type,
            variants,
        })
    }
}

/// Unlike Mojang's schemas, which stamp the version into every document, this
/// corpus records it only in `README.md`. Parse it rather than accept a
/// version from the caller: a protocol number guessed or passed by hand is how
/// a client ends up advertising something servers reject.
pub fn discover_versions(
    source_root: &Path,
) -> Result<Vec<VersionInfo>, Box<dyn std::error::Error>> {
    let readme_path = source_root.join("README.md");
    let readme = fs::read_to_string(&readme_path).map_err(|error| {
        format!(
            "unable to read {} for version metadata: {error}",
            readme_path.display()
        )
    })?;

    let field = |label: &str| -> Option<String> {
        readme.lines().find_map(|line| {
            let rest = line.split_once(&format!("**{label}:**"))?.1;
            Some(rest.trim().trim_matches('`').to_string())
        })
    };

    let minecraft_version = field("Minecraft Version").ok_or_else(|| {
        format!(
            "{} has no `**Minecraft Version:**` line",
            readme_path.display()
        )
    })?;
    let raw_protocol = field("Network Version").ok_or_else(|| {
        format!(
            "{} has no `**Network Version:**` line",
            readme_path.display()
        )
    })?;
    let protocol_version = raw_protocol.parse::<i32>().map_err(|error| {
        format!(
            "{}: network version {raw_protocol:?} is not a number: {error}",
            readme_path.display()
        )
    })?;

    // The dumper reports the four-part build (1.26.40.31). Version crates and
    // the handshake version string use the three-part release, which is what
    // gophertunnel's CurrentVersion and Mojang's schemas both carry; keeping
    // the build number would fork the crate name on every patch release.
    let minecraft_version = minecraft_version
        .split('.')
        .take(3)
        .collect::<Vec<_>>()
        .join(".");

    Ok(vec![VersionInfo {
        minecraft_version,
        protocol_version,
    }])
}

pub fn parse(
    source_root: &Path,
    override_dir: &Path,
) -> Result<ParseResult, Box<dyn std::error::Error>> {
    let packets_src = load_dir(source_root, "packets")?;
    if packets_src.is_empty() {
        return Err(format!(
            "no endstone packet documents below {}",
            source_root.display()
        )
        .into());
    }
    let mut documents = packets_src.clone();
    // The correction layer is shared with the Mojang frontend but is expected
    // to stay empty here: this corpus is dumped from the server, so a needed
    // correction means the dumper is wrong and is worth reporting upstream.
    overrides::apply(&mut documents, override_dir)?;

    let mut lowerer = Lowerer {
        types_src: load_dir(source_root, "types")?,
        enums_src: load_dir(source_root, "enums")?,
        out: HashMap::new(),
        active: HashSet::new(),
    };

    let mut packets = Vec::with_capacity(documents.len());
    for (name, document) in &documents {
        let id = document
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("endstone packet {name} has no id"))?;
        let body = lowerer.lower_container(document, &safe_name(name), name)?;
        packets.push(Packet {
            id: id as u32,
            name: name.clone(),
            body,
        });
    }
    packets.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(ParseResult {
        packets,
        types: lowerer.out,
    })
}
