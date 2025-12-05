use crate::ir::{Container, Field, Packet, Primitive, Type};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Protocol {
    types: HashMap<String, JsonTypeDef>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonTypeDef {
    String(String),
    Complex(Vec<Value>), // [kind (String), options (Value)]
}

pub struct ParseResult {
    pub packets: Vec<Packet>,
    pub types: HashMap<String, Type>,
}

pub fn parse(path: &Path) -> Result<ParseResult, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let protocol: Protocol = serde_json::from_reader(reader)?;

    // 1. Find mcpe_packet
    let packet_mapper = protocol
        .types
        .get("mcpe_packet")
        .or_else(|| protocol.types.get("packet_id"))
        .ok_or("Could not find mcpe_packet or packet_id definition")?;

    let mappings = match packet_mapper {
        JsonTypeDef::Complex(vec) => {
            if vec.len() >= 2 && vec[0].as_str() == Some("mapper") {
                let options = &vec[1];
                options
                    .get("mappings")
                    .and_then(|m| m.as_object())
                    .cloned()
                    .ok_or("mcpe_packet mapper has no mappings")?
            } else if vec.len() >= 2 && vec[0].as_str() == Some("container") {
                let fields = vec[1]
                    .as_array()
                    .ok_or("mcpe_packet container fields not array")?;
                let mut found_mappings: Option<serde_json::Map<String, Value>> = None;

                for f in fields {
                    if let Some(f_obj) = f.as_object()
                        && let Some(type_val) = f_obj.get("type")
                        && let Ok(JsonTypeDef::Complex(inner_vec)) =
                            serde_json::from_value::<JsonTypeDef>(type_val.clone())
                        && inner_vec.len() >= 2
                        && inner_vec[0].as_str() == Some("mapper")
                    {
                        let options = &inner_vec[1];
                        if let Some(m) = options.get("mappings").and_then(|m| m.as_object()) {
                            found_mappings = Some(m.clone());
                            break;
                        }
                    }
                }
                found_mappings.ok_or("Could not find mapper in mcpe_packet container")?
            } else {
                return Err("mcpe_packet is not a mapper or container".into());
            }
        }
        _ => return Err("mcpe_packet is not a complex type".into()),
    };

    let mut types_map = HashMap::new();
    // Parse all types
    for (name, def) in &protocol.types {
        // We pass the name down so containers get named properly
        match parse_type(def, &protocol.types, Some(name)) {
            Ok(t) => {
                types_map.insert(name.clone(), t);
            }
            Err(e) => {
                // Some types might fail (e.g. native), just log warning
                println!("Warning: Failed to parse type {}: {}", name, e);
            }
        }
    }

    let mut packets = Vec::new();
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

        // We look up the resolved type in our map, or re-parse if needed (but map should have it)
        if let Some(Type::Container(container)) = types_map.get(name) {
            packets.push(Packet {
                id,
                name: name.to_string(),
                body: container.clone(),
            });
        } else {
            // It might be that the packet type name refers to a container defined in "types"
            // Check if it exists
            if let Some(def) = protocol.types.get(name) {
                // Try parsing explicitly as container
                if let Ok(container) = parse_container(name, def, &protocol.types) {
                    packets.push(Packet {
                        id,
                        name: name.to_string(),
                        body: container,
                    });
                }
            }
        }
    }

    packets.sort_by_key(|p| p.id);

    Ok(ParseResult {
        packets,
        types: types_map,
    })
}

fn parse_type(
    def: &JsonTypeDef,
    types_map: &HashMap<String, JsonTypeDef>,
    name_hint: Option<&str>,
) -> Result<Type, String> {
    match def {
        JsonTypeDef::String(s) => parse_primitive_or_ref(s),
        JsonTypeDef::Complex(vec) => {
            if vec.is_empty() {
                return Err("Empty complex type".into());
            }
            let kind = vec[0].as_str().ok_or("Type kind must be a string")?;
            let options = if vec.len() > 1 { &vec[1] } else { &Value::Null };

            match kind {
                "container" => {
                    let name = name_hint.unwrap_or("anon");
                    let container = parse_container_body(name, options, types_map)?;
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
                        let val: i64 = if let Some(hex) =
                            k.strip_prefix("0x").or_else(|| k.strip_prefix("0X"))
                        {
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

                    // 1. Handle Array Format ["flag_a", "flag_b"] -> 1<<0, 1<<1
                    if let Some(list) = options.get("flags").and_then(|v| v.as_array()) {
                        for (i, v) in list.iter().enumerate() {
                            let name = v.as_str().unwrap_or("unknown").to_string();
                            let val: u64 = 1u64.checked_shl(i as u32).unwrap_or(0);
                            flags.push((name, val));
                        }
                    }
                    // 2. Handle Object Format {"flag_a": 1, "flag_b": 4} (CRITICAL FIX)
                    else if let Some(map) = options.get("flags").and_then(|v| v.as_object()) {
                        for (name, val) in map {
                            let val_u64 = val.as_u64().ok_or_else(|| {
                                format!("Bitflag value for {} is not an integer", name)
                            })?;
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
                    // Check if it's the "Size based" format (Array of objects with "size")
                    if let Some(list) = options.as_array() {
                        // Calculate total bits to determine backing type
                        let mut total_bits: u32 = 0;
                        let mut packed_fields = Vec::new();
                        let mut current_shift = 0;

                        for seg in list {
                            if let Some(size) = seg.get("size").and_then(|v| v.as_u64()) {
                                let size = size as u32;
                                let name =
                                    seg.get("name").and_then(|v| v.as_str()).unwrap_or("unused");

                                // Calculate mask for this segment (e.g. size 3 -> 0b111 -> 7)
                                let mask = (1u64 << size) - 1;

                                if name != "unused" {
                                    packed_fields.push(crate::ir::PackedField {
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
                        // ... (Keep your existing Bitflags logic for ["flag", "flag"] here) ...
                        // The previous "bitflags" block I gave you handles the OTHER format.
                        // This block handles "bitfield" (singular).
                        // Ensure you don't delete the "bitflags" logic!
                        Err("Invalid bitfield format".into())
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
                    if let Some(inner) = options.get("type") {
                        let inner_def: JsonTypeDef = serde_json::from_value(inner.clone())
                            .map_err(|e| format!("encapsulated inner error: {}", e))?;
                        parse_type(&inner_def, types_map, name_hint)
                    } else {
                        Ok(Type::Primitive(Primitive::ByteArray))
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
                "pstring" => Ok(Type::Primitive(Primitive::McString)),
                "buffer" => Ok(Type::Primitive(Primitive::ByteArray)),
                _ => {
                    if kind == "native" {
                        Ok(Type::Primitive(Primitive::Void))
                    } else {
                        if let Some(named) = types_map.get(kind) {
                            parse_type(named, types_map, name_hint)
                        } else {
                            Err(format!("Unknown type kind: {}", kind))
                        }
                    }
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
        let sub_hint = format!("{}_{}", name, field_name);
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
        s if s.eq_ignore_ascii_case("native") => Ok(Type::Primitive(Primitive::ByteArray)),
        "bool" => Ok(Type::Primitive(Primitive::Bool)),
        "u8" | "byte" => Ok(Type::Primitive(Primitive::U8)),
        "i8" | "li8" => Ok(Type::Primitive(Primitive::I8)),
        "u16" | "us short" | "lu16" | "unsigned short" => Ok(Type::Primitive(Primitive::U16)),
        "i16" | "short" | "li16" => Ok(Type::Primitive(Primitive::I16)),
        "u32" | "lu32" | "unsigned int" => Ok(Type::Primitive(Primitive::U32)),
        "i32" | "int" | "li32" => Ok(Type::Primitive(Primitive::I32)),
        "u64" | "lu64" | "unsigned long" => Ok(Type::Primitive(Primitive::U64)),
        "i64" | "long" | "li64" => Ok(Type::Primitive(Primitive::I64)),
        "f32" | "float" | "lf32" => Ok(Type::Primitive(Primitive::F32)),
        "f64" | "double" | "lf64" => Ok(Type::Primitive(Primitive::F64)),
        "varint" | "optvarint" => Ok(Type::Primitive(Primitive::VarInt)),
        "varlong" => Ok(Type::Primitive(Primitive::VarLong)),
        "zigzag32" | "Zigzag32" => Ok(Type::Primitive(Primitive::ZigZag32)),
        "zigzag64" | "Zigzag64" => Ok(Type::Primitive(Primitive::ZigZag64)),
        "string" | "pstring" | "mcpe_string" => Ok(Type::Primitive(Primitive::McString)),
        "uuid" | "mcpe_uuid" => Ok(Type::Primitive(Primitive::Uuid)),
        "void" => Ok(Type::Primitive(Primitive::Void)),
        "restBuffer" | "RestBuffer" => Ok(Type::Primitive(Primitive::ByteArray)),
        "nbt" | "lnbt" | "Lnbt" => Ok(Type::Primitive(Primitive::ByteArray)),
        // common native aliases
        "Varint64" | "varint64" => Ok(Type::Primitive(Primitive::I64)),
        "Varint128" | "varint128" => Ok(Type::Primitive(Primitive::I64)),
        _ => Ok(Type::Reference(s.to_string())),
    }
}
