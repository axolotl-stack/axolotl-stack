use crate::ir::{Container, Field, PackedField, Packet, Primitive, Type};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use tracing::{debug, warn};

#[derive(Debug, Deserialize)]
struct Protocol {
    types: HashMap<String, JsonTypeDef>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum JsonTypeDef {
    String(String),
    Complex(Vec<Value>),                    // ["kind", options]
    Object(serde_json::Map<String, Value>), // { "type": "kind", ... }
}

pub struct ParseResult {
    pub packets: Vec<Packet>,
    pub types: HashMap<String, Type>,
}

pub fn parse(path: &Path) -> Result<ParseResult, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let protocol: Protocol = serde_json::from_reader(reader)?;

    // 1. Find mcpe_packet or packet_id to determine ID mappings
    let packet_mapper = protocol
        .types
        .get("mcpe_packet")
        .or_else(|| protocol.types.get("packet_id"))
        .ok_or("Could not find mcpe_packet or packet_id definition")?;

    let mappings = find_packet_mappings(packet_mapper)
        .ok_or("Could not locate packet mappings in mcpe_packet")?;

    debug!(mapping_count = mappings.len(), "Found packet ID mappings");

    let mut types_map = HashMap::new();
    let mut type_parse_failures: usize = 0;
    // Parse all types
    for (name, def) in &protocol.types {
        // Special Case: Force NBT types to our primitive, ignoring "native" mapping in yml
        if name == "nbt" || name == "lnbt" || name == "nbtLoop" {
            types_map.insert(name.clone(), Type::Primitive(Primitive::Nbt));
            continue;
        }

        match parse_type(def, &protocol.types, Some(name)) {
            Ok(t) => {
                types_map.insert(name.clone(), t);
            }
            Err(e) => {
                type_parse_failures += 1;
                debug!(type_name = %name, error = %e, "Failed to parse type");
            }
        }
    }

    if type_parse_failures > 0 {
        warn!(
            failed = type_parse_failures,
            path = %path.display(),
            "Some types failed to parse"
        );
    }

    debug!(type_count = types_map.len(), "Parsed protocol types");

    let mut packets: Vec<Packet> = Vec::new();
    let mut missing_packet_bodies: usize = 0;
    for (id_str, name_val) in mappings {
        let id: u32 = if let Some(hex) = id_str
            .strip_prefix("0x")
            .or_else(|| id_str.strip_prefix("0X"))
        {
            u32::from_str_radix(hex, 16)?
        } else {
            id_str.parse()?
        };
        let name = name_val.as_str().ok_or("Packet name is not a string")?;

        // Link the packet ID to its Body type
        let mut found_packet = false;
        if let Some(Type::Container(container)) = types_map.get(name) {
            packets.push(Packet {
                id,
                name: name.to_string(),
                body: container.clone(),
            });
            found_packet = true;
        } else {
            // Try with "packet_" prefix
            let prefixed_name = format!("packet_{}", name);
            if let Some(Type::Container(container)) = types_map.get(&prefixed_name) {
                packets.push(Packet {
                    id,
                    name: prefixed_name.to_string(),
                    body: container.clone(),
                });
                found_packet = true;
            }
        }

        if !found_packet {
            // Fallback: Try parsing explicitly if not found in map (rare)
            if let Some(def) = protocol.types.get(name) {
                if let Ok(container) = parse_container(name, def, &protocol.types) {
                    packets.push(Packet {
                        id,
                        name: name.to_string(),
                        body: container,
                    });
                    found_packet = true;
                }
            } else {
                // Also try with prefixed name for fallback
                let prefixed_name = format!("packet_{}", name);
                if let Some(def) = protocol.types.get(&prefixed_name) {
                    if let Ok(container) = parse_container(&prefixed_name, def, &protocol.types) {
                        packets.push(Packet {
                            id,
                            name: prefixed_name.to_string(),
                            body: container,
                        });
                        found_packet = true;
                    }
                }
            }
        }

        if !found_packet {
            missing_packet_bodies += 1;
            debug!(packet_id = id, packet_name = %name, "Missing packet body");
        }
    }

    if missing_packet_bodies > 0 {
        warn!(
            missing = missing_packet_bodies,
            path = %path.display(),
            "Some packets had no body"
        );
    }

    debug!(packet_count = packets.len(), "Populated packets");

    // Inject explicit definition for "enum_size_based_on_values_len" to replace "native" placeholder.
    // This allows us to treat it as a strongly typed Enum in the generated code.
    types_map.insert(
        "enum_size_based_on_values_len".to_string(),
        Type::Enum {
            underlying: Primitive::I32,
            variants: vec![
                ("Byte".to_string(), 0),
                ("Short".to_string(), 1),
                ("Int".to_string(), 2),
            ],
        },
    );

    packets.sort_by_key(|p| p.id);

    Ok(ParseResult {
        packets,
        types: types_map,
    })
}

// Helper to recursively find "mappings" object inside the packet definition
fn find_packet_mappings(def: &JsonTypeDef) -> Option<serde_json::Map<String, Value>> {
    match def {
        JsonTypeDef::Complex(vec) => {
            if vec.len() >= 2 {
                let kind = vec[0].as_str()?;
                match kind {
                    "mapper" => vec[1].get("mappings")?.as_object().cloned(),
                    "container" => {
                        let fields = vec[1].as_array()?;
                        for f in fields {
                            if let Some(type_val) = f.get("type") {
                                if let Ok(inner_def) =
                                    serde_json::from_value::<JsonTypeDef>(type_val.clone())
                                {
                                    if let Some(m) = find_packet_mappings(&inner_def) {
                                        return Some(m);
                                    }
                                }
                            }
                        }
                        None
                    }
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn parse_type(
    def: &JsonTypeDef,
    types_map: &HashMap<String, JsonTypeDef>,
    name_hint: Option<&str>,
) -> Result<Type, String> {
    match def {
        JsonTypeDef::String(s) => parse_primitive_or_ref(s),
        JsonTypeDef::Object(obj) => {
            // Handle { "type": "kind", ... } style
            let kind = obj
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let options = Value::Object(obj.clone());
            parse_complex_type(kind, &options, types_map, name_hint)
        }
        JsonTypeDef::Complex(vec) => {
            // Handle ["kind", {options}] style
            if vec.is_empty() {
                return Err("Empty complex type".into());
            }
            let kind = vec[0].as_str().ok_or("Type kind must be a string")?;
            let options = if vec.len() > 1 { &vec[1] } else { &Value::Null };
            parse_complex_type(kind, options, types_map, name_hint)
        }
    }
}

fn parse_complex_type(
    kind: &str,
    options: &Value,
    types_map: &HashMap<String, JsonTypeDef>,
    name_hint: Option<&str>,
) -> Result<Type, String> {
    match kind {
        "container" => {
            let name = name_hint.unwrap_or("anon");
            // Options might be the array of fields directly (if from Complex) or an object with "fields"
            let fields_val = if options.is_array() {
                options
            } else {
                options.get("fields").unwrap_or(options)
            };

            let container = parse_container_body(name, fields_val, types_map)?;
            Ok(Type::Container(container))
        }
        "array" => {
            let count_type = options
                .get("countType")
                .and_then(|v| v.as_str())
                .unwrap_or("varint");
            let inner = options.get("type").ok_or("Array missing 'type'")?;

            let inner_def: JsonTypeDef = serde_json::from_value(inner.clone())
                .map_err(|e| format!("Failed to parse array inner type: {}", e))?;

            Ok(Type::Array {
                count_type: Box::new(parse_primitive_or_ref(count_type)?),
                inner_type: Box::new(parse_type(&inner_def, types_map, None)?),
            })
        }
        "mapper" => {
            let underlying_str = options
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("varint");
            let underlying_prim = match parse_primitive_or_ref(underlying_str)? {
                Type::Primitive(p) => p,
                _ => Primitive::VarInt,
            };

            let mappings = options
                .get("mappings")
                .and_then(|m| m.as_object())
                .ok_or("mapper missing mappings")?;

            let mut variants: Vec<(String, i64)> = Vec::new();
            for (k, v) in mappings {
                let name = v
                    .as_str()
                    .ok_or("mapper mapping value not string")?
                    .to_string();
                let val: i64 =
                    if let Some(hex) = k.strip_prefix("0x").or_else(|| k.strip_prefix("0X")) {
                        i64::from_str_radix(hex, 16)
                            .map_err(|e| format!("mapper key parse hex: {}", e))?
                    } else {
                        k.parse::<i64>()
                            .map_err(|e| format!("mapper key parse: {}", e))?
                    };
                variants.push((name, val));
            }
            variants.sort_by_key(|(_, v)| *v);

            Ok(Type::Enum {
                underlying: underlying_prim,
                variants,
            })
        }
        "switch" => {
            let compare_to = options
                .get("compareTo")
                .and_then(|v| v.as_str())
                .ok_or("Switch missing compareTo")?;

            let fields_map = options
                .get("fields")
                .and_then(|v| v.as_object())
                .ok_or("Switch missing fields")?;

            let mut fields = Vec::new();
            for (k, v) in fields_map {
                let type_def: JsonTypeDef = serde_json::from_value(v.clone())
                    .map_err(|e| format!("Switch field error: {}", e))?;
                fields.push((k.clone(), parse_type(&type_def, types_map, None)?));
            }

            let default_type = if let Some(default_val) = options.get("default") {
                let default_def: JsonTypeDef = serde_json::from_value(default_val.clone())
                    .map_err(|e| format!("Switch default error: {}", e))?;
                parse_type(&default_def, types_map, None)?
            } else {
                Type::Primitive(Primitive::Void)
            };

            Ok(Type::Switch {
                compare_to: compare_to.to_string(),
                fields,
                default: Box::new(default_type),
            })
        }
        "bitflags" => {
            let storage = options
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("varint");
            let storage_prim = match parse_primitive_or_ref(storage)? {
                Type::Primitive(p) => p,
                _ => Primitive::VarInt,
            };

            let mut flags = Vec::new();

            // 1. Array Format
            if let Some(list) = options.get("flags").and_then(|v| v.as_array()) {
                for (i, v) in list.iter().enumerate() {
                    let name = v.as_str().unwrap_or("unknown").to_string();
                    let val: u64 = 1u64.checked_shl(i as u32).unwrap_or(0);
                    flags.push((name, val));
                }
            }
            // 2. Object Format
            else if let Some(map) = options.get("flags").and_then(|v| v.as_object()) {
                for (name, val) in map {
                    let val_u64 = val
                        .as_u64()
                        .ok_or_else(|| format!("Bitflag value for {} is not an integer", name))?;
                    flags.push((name.clone(), val_u64));
                }
            }

            Ok(Type::Bitfield {
                name: name_hint.unwrap_or("anon_bitflags").to_string(),
                storage_type: storage_prim,
                flags,
            })
        }
        "bitfield" => {
            // FIX: Handle Packed Fields (Array of objects with "size")
            if let Some(list) = options.as_array() {
                let mut total_bits: u32 = 0;
                let mut packed_fields = Vec::new();
                let mut current_shift = 0;

                for seg in list {
                    if let Some(size) = seg.get("size").and_then(|v| v.as_u64()) {
                        let size = size as u32;
                        let name = seg.get("name").and_then(|v| v.as_str()).unwrap_or("unused");

                        // Calculate mask (e.g. size 3 -> 0b111 -> 7)
                        let mask = (1u64 << size) - 1;

                        if name != "unused" {
                            packed_fields.push(PackedField {
                                name: name.to_string(),
                                shift: current_shift,
                                mask,
                            });
                        }

                        total_bits += size;
                        current_shift += size;
                    }
                }

                let backing = if total_bits <= 8 {
                    Primitive::U8
                } else if total_bits <= 16 {
                    Primitive::U16
                } else if total_bits <= 32 {
                    Primitive::U32
                } else {
                    Primitive::U64
                };

                Ok(Type::Packed {
                    backing,
                    fields: packed_fields,
                })
            } else {
                Err("Invalid bitfield format (expected array of sized fields)".into())
            }
        }
        "entityMetadataLoop" => {
            let inner = options
                .get("type")
                .ok_or("entityMetadataLoop missing type")?;
            let inner_def: JsonTypeDef = serde_json::from_value(inner.clone())
                .map_err(|e| format!("entityMetadataLoop inner error: {}", e))?;
            Ok(Type::Array {
                count_type: Box::new(Type::Primitive(Primitive::VarInt)),
                inner_type: Box::new(parse_type(&inner_def, types_map, None)?),
            })
        }
        "count" => {
            if let Some(inner) = options.get("type") {
                let inner_def: JsonTypeDef = serde_json::from_value(inner.clone())
                    .map_err(|e| format!("count inner error: {}", e))?;
                parse_type(&inner_def, types_map, name_hint)
            } else {
                Err("count missing type".into())
            }
        }
        "encapsulated" => {
            let length_type = options
                .get("lengthType")
                .and_then(|v| v.as_str())
                .unwrap_or("varint");
            let length_ty = match parse_primitive_or_ref(length_type)? {
                Type::Primitive(p) => Type::Primitive(p),
                other => {
                    return Err(format!(
                        "encapsulated lengthType must be primitive, got {:?}",
                        other
                    ));
                }
            };

            if let Some(inner) = options.get("type") {
                let inner_def: JsonTypeDef = serde_json::from_value(inner.clone())
                    .map_err(|e| format!("encapsulated inner error: {}", e))?;
                let inner_ty = parse_type(&inner_def, types_map, name_hint)?;
                Ok(Type::Encapsulated {
                    length_type: Box::new(length_ty),
                    inner: Box::new(inner_ty),
                })
            } else {
                // Default to raw buffer with the given length prefix.
                Ok(Type::Encapsulated {
                    length_type: Box::new(length_ty),
                    inner: Box::new(Type::Primitive(Primitive::ByteArray)),
                })
            }
        }
        "option" => {
            let inner_def: JsonTypeDef = if let Some(inner) = options.get("type") {
                serde_json::from_value(inner.clone())
                    .map_err(|e| format!("option inner error: {}", e))?
            } else {
                serde_json::from_value(options.clone())
                    .map_err(|e| format!("option value error: {}", e))?
            };
            Ok(Type::Option(Box::new(parse_type(
                &inner_def, types_map, None,
            )?)))
        }
        "pstring" => {
            let count_type = options
                .get("countType")
                .and_then(|v| v.as_str())
                .unwrap_or("varint");
            let encoding = options
                .get("encoding")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let count_ty = match parse_primitive_or_ref(count_type)? {
                Type::Primitive(p) => Type::Primitive(p),
                other => {
                    return Err(format!(
                        "pstring countType must be primitive, got {:?}",
                        other
                    ));
                }
            };
            Ok(Type::String {
                count_type: Box::new(count_ty),
                encoding,
            })
        }
        "buffer" => {
            let count_type = options
                .get("countType")
                .and_then(|v| v.as_str())
                .unwrap_or("varint");
            let count_ty = match parse_primitive_or_ref(count_type)? {
                Type::Primitive(p) => Type::Primitive(p),
                other => {
                    return Err(format!(
                        "buffer countType must be primitive, got {:?}",
                        other
                    ));
                }
            };
            Ok(Type::Array {
                count_type: Box::new(count_ty),
                inner_type: Box::new(Type::Primitive(Primitive::U8)),
            })
        }
        "enum_size_based_on_values_len" => Ok(Type::Reference(kind.to_string())),
        _ => {
            if kind == "native" {
                Ok(Type::Primitive(Primitive::Void))
            } else {
                // FALLBACK: Named Type Reference
                if let Some(named) = types_map.get(kind) {
                    // FIX: Variable Substitution Logic
                    let definition_to_parse = if !options.is_null() {
                        substitute_args(named, options)
                    } else {
                        named.clone()
                    };

                    parse_type(&definition_to_parse, types_map, name_hint)
                } else {
                    Err(format!("Unknown type kind: {}", kind))
                }
            }
        }
    }
}

fn parse_container(
    name: &str,
    def: &JsonTypeDef,
    types_map: &HashMap<String, JsonTypeDef>,
) -> Result<Container, String> {
    match def {
        JsonTypeDef::Complex(vec) => {
            if vec.len() >= 2 && vec[0].as_str() == Some("container") {
                let fields_val = &vec[1];
                parse_container_body(name, fields_val, types_map)
            } else {
                Err(format!("{} is not a container", name))
            }
        }
        _ => Err(format!("{} definition is not complex", name)),
    }
}

fn parse_container_body(
    name: &str,
    fields_val: &Value,
    types_map: &HashMap<String, JsonTypeDef>,
) -> Result<Container, String> {
    let fields_arr = fields_val
        .as_array()
        .ok_or("Container fields must be an array")?;
    let mut fields = Vec::new();

    for f in fields_arr {
        let f_obj = f.as_object().ok_or("Field must be an object")?;

        let is_anon = f_obj.get("anon").and_then(|v| v.as_bool()).unwrap_or(false);
        let field_name = if is_anon {
            "content".to_string()
        } else {
            f_obj
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or("Field missing name")?
                .to_string()
        };

        let type_val = f_obj.get("type").ok_or("Field missing type")?;
        let type_def: JsonTypeDef = serde_json::from_value(type_val.clone())
            .map_err(|e| format!("Field type error: {}", e))?;

        // We can pass a hint like "ContainerName_FieldName"
        let sub_hint = format!("{}{}", name, field_name);
        let field_type = parse_type(&type_def, types_map, Some(&sub_hint))?;

        fields.push(Field {
            name: field_name,
            type_def: field_type,
        });
    }

    Ok(Container {
        name: name.to_string(),
        fields,
    })
}

fn parse_primitive_or_ref(s: &str) -> Result<Type, String> {
    match s {
        "enum_size_based_on_values_len" => Ok(Type::Reference(s.to_string())),
        s if s.eq_ignore_ascii_case("native") => Ok(Type::Primitive(Primitive::ByteArray)),
        "bool" => Ok(Type::Primitive(Primitive::Bool)),
        "u8" | "byte" => Ok(Type::Primitive(Primitive::U8)),
        "i8" | "li8" => Ok(Type::Primitive(Primitive::I8)),
        "u16" | "us short" | "unsigned short" => Ok(Type::Primitive(Primitive::U16)),
        "lu16" => Ok(Type::Primitive(Primitive::U16LE)),
        "i16" | "short" => Ok(Type::Primitive(Primitive::I16)),
        "li16" => Ok(Type::Primitive(Primitive::I16LE)),
        "u32" | "unsigned int" => Ok(Type::Primitive(Primitive::U32)),
        "lu32" => Ok(Type::Primitive(Primitive::U32LE)),
        "i32" | "int" => Ok(Type::Primitive(Primitive::I32)),
        "li32" => Ok(Type::Primitive(Primitive::I32LE)),
        "u64" | "unsigned long" => Ok(Type::Primitive(Primitive::U64)),
        "lu64" => Ok(Type::Primitive(Primitive::U64LE)),
        "i64" | "long" => Ok(Type::Primitive(Primitive::I64)),
        "li64" => Ok(Type::Primitive(Primitive::I64LE)),
        "f32" | "float" => Ok(Type::Primitive(Primitive::F32)),
        "lf32" => Ok(Type::Primitive(Primitive::F32LE)),
        "f64" | "double" => Ok(Type::Primitive(Primitive::F64)),
        "lf64" => Ok(Type::Primitive(Primitive::F64LE)),
        "varint" => Ok(Type::Primitive(Primitive::VarInt)),
        "varlong" | "varint64" | "Varint64" => Ok(Type::Primitive(Primitive::VarLong)),
        "zigzag32" | "Zigzag32" => Ok(Type::Primitive(Primitive::ZigZag32)),
        "zigzag64" | "Zigzag64" => Ok(Type::Primitive(Primitive::ZigZag64)),
        "string" | "pstring" | "mcpe_string" => Ok(Type::String {
            count_type: Box::new(Type::Primitive(Primitive::VarInt)),
            encoding: None,
        }),
        "uuid" | "mcpe_uuid" => Ok(Type::Primitive(Primitive::Uuid)),
        "void" => Ok(Type::Primitive(Primitive::Void)),
        "restBuffer" | "RestBuffer" => Ok(Type::Primitive(Primitive::ByteArray)),
        "nbt" | "lnbt" | "Lnbt" => Ok(Type::Primitive(Primitive::Nbt)),
        // common native aliases
        "Varint128" | "varint128" => Ok(Type::Primitive(Primitive::VarLong)),
        _ => Ok(Type::Reference(s.to_string())),
    }
}

// === ARGUMENT SUBSTITUTION LOGIC ===

fn substitute_args(def: &JsonTypeDef, args: &Value) -> JsonTypeDef {
    match def {
        JsonTypeDef::String(s) => {
            if s.starts_with('$') {
                let var_name = &s[1..];
                if let Some(val) = args.get(var_name) {
                    if let Some(s_val) = val.as_str() {
                        return JsonTypeDef::String(s_val.to_string());
                    }
                }
                // Keep original if replacement not found
                JsonTypeDef::String(s.clone())
            } else {
                JsonTypeDef::String(s.clone())
            }
        }
        JsonTypeDef::Complex(vec) => {
            let new_vec = vec.iter().map(|v| substitute_value(v, args)).collect();
            JsonTypeDef::Complex(new_vec)
        }
        JsonTypeDef::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (k, v) in map {
                new_map.insert(k.clone(), substitute_value(v, args));
            }
            JsonTypeDef::Object(new_map)
        }
    }
}

fn substitute_value(v: &Value, args: &Value) -> Value {
    match v {
        Value::String(s) => {
            if s.starts_with('$') {
                let var_name = &s[1..];
                if let Some(replacement) = args.get(var_name) {
                    return replacement.clone();
                }
            }
            Value::String(s.clone())
        }
        Value::Array(arr) => Value::Array(arr.iter().map(|i| substitute_value(i, args)).collect()),
        Value::Object(obj) => {
            let mut new_obj = serde_json::Map::new();
            for (k, val) in obj {
                new_obj.insert(k.clone(), substitute_value(val, args));
            }
            Value::Object(new_obj)
        }
        _ => v.clone(),
    }
}
