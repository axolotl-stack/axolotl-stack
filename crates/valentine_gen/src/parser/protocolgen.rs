//! Frontend for protocolgen's canonical v2 manifest.

use super::ParseResult;
use crate::ir::{Container, Field, Packet, Primitive, Type, UnionVariant};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Debug)]
pub struct ParsedManifest {
    pub minecraft_version: String,
    pub protocol_version: i32,
    pub result: ParseResult,
}

pub fn parse(path: &Path) -> Result<ParsedManifest, Box<dyn std::error::Error>> {
    parse_manifest(File::open(path)?)
}

#[cfg(test)]
fn parse_reader(reader: impl Read) -> Result<ParseResult, Box<dyn std::error::Error>> {
    Ok(parse_manifest(reader)?.result)
}

fn parse_manifest(reader: impl Read) -> Result<ParsedManifest, Box<dyn std::error::Error>> {
    let manifest: CanonicalManifest = serde_json::from_reader(reader)?;
    if manifest.schema_version != 2 {
        return Err(format!(
            "unsupported protocolgen manifest schema {}; expected 2",
            manifest.schema_version
        )
        .into());
    }

    let mut types = HashMap::new();
    let mut packets = Vec::with_capacity(manifest.packets.len());
    for packet in manifest.packets {
        packets.push(lower_packet(packet, &mut types)?);
    }
    packets.sort_by_key(|packet| packet.id);
    Ok(ParsedManifest {
        minecraft_version: manifest.target.minecraft_version,
        protocol_version: manifest.target.protocol_version,
        result: ParseResult { packets, types },
    })
}

#[derive(Deserialize)]
struct CanonicalManifest {
    schema_version: u32,
    target: CanonicalTarget,
    packets: Vec<CanonicalPacket>,
}

#[derive(Deserialize)]
struct CanonicalTarget {
    minecraft_version: String,
    protocol_version: i32,
}

#[derive(Deserialize)]
struct CanonicalPacket {
    id: u32,
    name: String,
    fields: Option<Vec<CanonicalField>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct CanonicalField {
    ordinal: usize,
    name: String,
    encode: CanonicalNode,
    #[serde(default)]
    decode: Option<CanonicalNode>,
    symmetry: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct CanonicalVariant {
    value: i64,
    name: String,
    encode: CanonicalNode,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct PrimitiveShape {
    code: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct CanonicalNode {
    kind: String,
    primitive: Option<PrimitiveShape>,
    prefix: Option<Box<CanonicalNode>>,
    control: Option<Box<CanonicalNode>>,
    element: Option<Box<CanonicalNode>>,
    key: Option<Box<CanonicalNode>>,
    value: Option<Box<CanonicalNode>>,
    length: Option<usize>,
    encoding: Option<String>,
    type_id: Option<String>,
    target: Option<String>,
    reachable: Option<bool>,
    reason: Option<String>,
    fields: Option<Vec<CanonicalField>>,
    variants: Option<Vec<CanonicalVariant>>,
}

fn lower_packet(
    packet: CanonicalPacket,
    types: &mut HashMap<String, Type>,
) -> Result<Packet, String> {
    let mut fields = packet.fields.unwrap_or_default();
    fields.sort_by_key(|field| field.ordinal);
    Ok(Packet {
        id: packet.id,
        name: packet.name.clone(),
        body: Container {
            name: packet.name,
            fields: fields
                .into_iter()
                .map(|field| lower_field(field, types))
                .collect::<Result<_, _>>()?,
        },
    })
}

fn lower_field(field: CanonicalField, types: &mut HashMap<String, Type>) -> Result<Field, String> {
    if field.symmetry != "symmetric" || field.decode.is_some() {
        return Err(format!(
            "protocolgen field {:?} has an asymmetric decode layout, which Valentine cannot represent",
            field.name
        ));
    }
    Ok(Field {
        name: field.name,
        type_def: lower_node(field.encode, types)?,
    })
}

fn lower_node(node: CanonicalNode, types: &mut HashMap<String, Type>) -> Result<Type, String> {
    let kind = node.kind.clone();
    let type_id = node.type_id.clone().filter(|type_id| !type_id.is_empty());
    let lowered = match kind.as_str() {
        "primitive" => Type::Primitive(lower_primitive(
            node.primitive
                .ok_or("protocolgen primitive node has no primitive shape")?
                .code
                .as_str(),
        )?),
        "void" => Type::Primitive(Primitive::Void),
        "string" => Type::String {
            count_type: Box::new(lower_node(
                *node.prefix.ok_or("protocolgen string node has no prefix")?,
                types,
            )?),
            encoding: Some(
                node.encoding
                    .filter(|encoding| encoding == "utf8")
                    .ok_or("protocolgen string is not explicitly UTF-8")?,
            ),
        },
        "array" => Type::Array {
            count_type: Box::new(lower_node(
                *node.prefix.ok_or("protocolgen array node has no prefix")?,
                types,
            )?),
            inner_type: Box::new(lower_node(
                *node
                    .element
                    .ok_or("protocolgen array node has no element")?,
                types,
            )?),
        },
        "map" => Type::Array {
            count_type: Box::new(lower_node(
                *node.prefix.ok_or("protocolgen map node has no prefix")?,
                types,
            )?),
            inner_type: Box::new(Type::Container(Container {
                name: String::new(),
                fields: vec![
                    Field {
                        name: "Key".to_string(),
                        type_def: lower_node(
                            *node.key.ok_or("protocolgen map node has no key")?,
                            types,
                        )?,
                    },
                    Field {
                        name: "Value".to_string(),
                        type_def: lower_node(
                            *node.value.ok_or("protocolgen map node has no value")?,
                            types,
                        )?,
                    },
                ],
            })),
        },
        "struct" => {
            let mut fields = node.fields.unwrap_or_default();
            fields.sort_by_key(|field| field.ordinal);
            Type::Container(Container {
                name: type_id.clone().unwrap_or_default(),
                fields: fields
                    .into_iter()
                    .map(|field| lower_field(field, types))
                    .collect::<Result<_, _>>()?,
            })
        }
        "bytes" => Type::Array {
            count_type: Box::new(lower_node(
                *node.prefix.ok_or("protocolgen bytes node has no prefix")?,
                types,
            )?),
            inner_type: Box::new(Type::Primitive(Primitive::U8)),
        },
        "fixed_array" => Type::FixedArray {
            size: node
                .length
                .ok_or("protocolgen fixed-array node has no length")?,
            inner_type: Box::new(lower_node(
                *node
                    .element
                    .ok_or("protocolgen fixed-array node has no element")?,
                types,
            )?),
        },
        "bitset" => Type::Bitset {
            bits: node
                .length
                .filter(|bits| *bits > 0)
                .ok_or("protocolgen bitset node has no positive bit length")?,
        },
        "optional" => Type::Option(Box::new(lower_node(
            *node.value.ok_or("protocolgen optional node has no value")?,
            types,
        )?)),
        "recursive" => Type::Reference(
            node.target
                .or(node.type_id)
                .filter(|target| !target.is_empty())
                .ok_or("protocolgen recursive node has no target")?,
        ),
        "enum" => Type::Enum {
            underlying: lower_primitive(
                node.primitive
                    .ok_or("protocolgen enum node has no primitive shape")?
                    .code
                    .as_str(),
            )?,
            variants: node
                .variants
                .unwrap_or_default()
                .into_iter()
                .map(|variant| {
                    (
                        variant
                            .name
                            .rsplit("::")
                            .next()
                            .unwrap_or(&variant.name)
                            .to_string(),
                        variant.value,
                    )
                })
                .collect(),
        },
        "union" => {
            let control = lower_node(
                *node
                    .control
                    .ok_or("protocolgen union node has no control")?,
                types,
            )?;
            let Type::Primitive(control_type) = control else {
                return Err("protocolgen union control is not primitive".to_string());
            };
            Type::Union {
                control_type,
                variants: node
                    .variants
                    .unwrap_or_default()
                    .into_iter()
                    .map(|variant| {
                        Ok(UnionVariant {
                            control_value: variant.value,
                            name: variant
                                .name
                                .rsplit("::")
                                .next()
                                .unwrap_or(&variant.name)
                                .to_string(),
                            type_def: lower_node(variant.encode, types)?,
                        })
                    })
                    .collect::<Result<_, String>>()?,
            }
        }
        "opaque" | "unresolved" if node.reachable.unwrap_or(true) => {
            return Err(format!(
                "reachable protocolgen {} node cannot be generated: {}",
                node.kind,
                node.reason
                    .unwrap_or_else(|| "no reason supplied".to_string())
            ));
        }
        other => return Err(format!("unsupported protocolgen node kind {other:?}")),
    };

    if kind != "recursive"
        && let Some(type_id) = type_id
    {
        if let Some(existing) = types.get(&type_id) {
            if existing != &lowered {
                return Ok(lowered);
            }
        } else {
            types.insert(type_id.clone(), lowered);
        }
        Ok(Type::Reference(type_id))
    } else {
        Ok(lowered)
    }
}

fn lower_primitive(code: &str) -> Result<Primitive, String> {
    match code {
        "u8" => Ok(Primitive::U8),
        "i8" => Ok(Primitive::I8),
        "bool" => Ok(Primitive::Bool),
        "i16le" => Ok(Primitive::I16LE),
        "u16le" => Ok(Primitive::U16LE),
        "i32le" => Ok(Primitive::I32LE),
        "u32le" => Ok(Primitive::U32LE),
        "i32be" => Ok(Primitive::I32),
        "u32be" => Ok(Primitive::U32),
        "i64le" => Ok(Primitive::I64LE),
        "u64le" => Ok(Primitive::U64LE),
        "f32le" => Ok(Primitive::F32LE),
        "f64le" => Ok(Primitive::F64LE),
        "var_i32" => Ok(Primitive::VarInt),
        "var_u32" => Ok(Primitive::VarUInt),
        "var_i64" => Ok(Primitive::VarLong),
        "var_u64" => Ok(Primitive::VarULong),
        "zigzag_i32" => Ok(Primitive::ZigZag32),
        "zigzag_i64" => Ok(Primitive::ZigZag64),
        "uuid" => Ok(Primitive::Uuid),
        "nbt_le" => Ok(Primitive::Nbt),
        other => Err(format!("unsupported protocolgen primitive {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Primitive, Type};

    #[test]
    fn lowers_resource_pack_response_union_with_zero_based_tags() {
        let fixture = r#"{
          "schema_version": 2,
          "target": {"minecraft_version": "1.26.40", "protocol_version": 2168},
          "sources": [{"id":"endstone"},{"id":"mojang"}],
          "packets": [{
            "id": 8,
            "name": "ResourcePackClientResponsePacket",
            "direction": "bidirectional",
            "fields": [{
              "ordinal": 0,
              "name": "Response",
              "symmetry": "symmetric",
              "encode": {
                "kind": "union",
                "control": {"kind":"primitive","primitive":{"code":"i8"}},
                "variants": [
                  {"value":0,"name":"ResourcePackClientResponsePacketPayload::Cancel","encode":{"kind":"void"}},
                  {"value":1,"name":"ResourcePackClientResponsePacketPayload::Downloading","encode":{"kind":"struct","fields":[{"ordinal":0,"name":"Pack IDs","symmetry":"symmetric","encode":{"kind":"array","prefix":{"kind":"primitive","primitive":{"code":"u16le"}},"element":{"kind":"string","prefix":{"kind":"primitive","primitive":{"code":"var_u32"}},"encoding":"utf8"}}}] }},
                  {"value":2,"name":"ResourcePackClientResponsePacketPayload::DownloadingFinished","encode":{"kind":"void"}},
                  {"value":3,"name":"ResourcePackClientResponsePacketPayload::ResourcePackStackFinished","encode":{"kind":"void"}}
                ]
              }
            }]
          }]
        }"#;

        let parsed = parse_reader(fixture.as_bytes()).expect("parse protocolgen manifest");
        assert_eq!(parsed.packets.len(), 1);
        let packet = &parsed.packets[0];
        assert_eq!(packet.id, 8);
        assert_eq!(packet.name, "ResourcePackClientResponsePacket");
        assert_eq!(packet.body.fields.len(), 1);

        let Type::Union {
            control_type,
            variants,
        } = &packet.body.fields[0].type_def
        else {
            panic!("response was not lowered as a union");
        };
        assert_eq!(*control_type, Primitive::I8);
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.control_value)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        assert!(matches!(
            variants[1].type_def,
            Type::Container(ref container)
                if matches!(container.fields[0].type_def, Type::Array { .. })
        ));
    }

    #[test]
    fn rejects_asymmetric_fields() {
        let fixture = r#"{
          "schema_version": 2,
          "target": {"minecraft_version":"1.26.40","protocol_version":2168},
          "packets": [{"id":1,"name":"Packet","fields":[{
            "ordinal":0,"name":"Value","symmetry":"asymmetric",
            "encode":{"kind":"primitive","primitive":{"code":"u8"}},
            "decode":{"kind":"primitive","primitive":{"code":"u16le"}}
          }]}]
        }"#;
        let error = parse_reader(fixture.as_bytes()).expect_err("asymmetry must fail closed");
        assert!(error.to_string().contains("asymmetric decode layout"));
    }

    #[test]
    fn preserves_distinct_named_types_with_identical_wire_shapes() {
        let fixture = r#"{
          "schema_version": 2,
          "target": {"minecraft_version":"1.26.40","protocol_version":2168},
          "packets": [{"id":8,"name":"Packet","fields":[{
            "ordinal":0,"name":"Value","symmetry":"symmetric",
            "encode":{"kind":"union","control":{"kind":"primitive","primitive":{"code":"i8"}},"variants":[
              {"value":0,"name":"Payload::First","encode":{"kind":"struct","type_id":"Payload::First","fields":[]}},
              {"value":1,"name":"Payload::Second","encode":{"kind":"struct","type_id":"Payload::Second","fields":[]}}
            ]}
          }]}]
        }"#;
        let parsed = parse_reader(fixture.as_bytes()).expect("parse named payloads");
        assert!(parsed.types.contains_key("Payload::First"));
        assert!(parsed.types.contains_key("Payload::Second"));
        let Type::Union { variants, .. } = &parsed.packets[0].body.fields[0].type_def else {
            panic!("expected union");
        };
        assert!(matches!(&variants[0].type_def, Type::Reference(name) if name == "Payload::First"));
        assert!(
            matches!(&variants[1].type_def, Type::Reference(name) if name == "Payload::Second")
        );
    }

    #[test]
    fn rejects_reachable_unresolved_nodes() {
        let fixture = r#"{
          "schema_version": 2,
          "target": {"minecraft_version":"1.26.40","protocol_version":2168},
          "packets": [{"id":1,"name":"Packet","fields":[{
            "ordinal":0,"name":"Value","symmetry":"symmetric",
            "encode":{"kind":"unresolved","reachable":true,"reason":"source disagreement"}
          }]}]
        }"#;
        let error =
            parse_reader(fixture.as_bytes()).expect_err("unresolved nodes must fail closed");
        assert!(error.to_string().contains("source disagreement"));
    }

    #[test]
    fn rejects_unknown_manifest_schema() {
        let fixture = r#"{"schema_version":3,"target":{"minecraft_version":"1.26.40","protocol_version":2168},"packets":[]}"#;
        let error = parse_reader(fixture.as_bytes()).expect_err("unknown schema must fail closed");
        assert!(error.to_string().contains("expected 2"));
    }
}
